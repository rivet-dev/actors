import type {
	EngineDriver,
	KVEntry,
	KVWrite,
	Message,
	WorkflowMessageDriver,
} from "@rivetkit/workflow-engine";
import type { RunContext } from "@/actor/config";
import type { AnyStaticActorInstance } from "@/actor/definition";
import { makeWorkflowKey, workflowStoragePrefix } from "@/actor/keys";
import type { RawAccess } from "@/common/database/config";

const WORKFLOW_STORAGE_PREFIX = workflowStoragePrefix();
// Keep workflow flushes below depot's 320-dirty-page commit ceiling. The
// former actor-KV transport allowed 976 KiB batches; SQLite also dirties table
// and index pages, so it needs the same conservative budget as core imports.
const WORKFLOW_SQLITE_MAX_BATCH_ROWS = 128;
const WORKFLOW_SQLITE_MAX_BATCH_BYTES = 512 * 1024;

// Mirrors the element shape returned by `queueManager.receive`. The actor
// instance is reached through a loose type here, so the call's result is
// untyped and the message must be annotated explicitly.
interface ReceivedQueueMessage {
	id: bigint;
	name: string;
	body: unknown;
	createdAt: number;
	complete?: (response?: unknown) => Promise<void>;
}

type KVEntryTuple = [Uint8Array, Uint8Array];

function stripWorkflowKey(prefixed: Uint8Array): Uint8Array {
	return prefixed.slice(WORKFLOW_STORAGE_PREFIX.length);
}

function computeUpperBound(prefix: Uint8Array): Uint8Array | null {
	const upperBound = prefix.slice();
	for (let i = upperBound.length - 1; i >= 0; i--) {
		if (upperBound[i] !== 0xff) {
			upperBound[i]++;
			return upperBound.slice(0, i + 1);
		}
	}
	return null;
}

function normalizeSqlBlob(value: unknown): Uint8Array {
	if (value instanceof Uint8Array) {
		return value;
	}
	if (value instanceof ArrayBuffer) {
		return new Uint8Array(value);
	}
	if (ArrayBuffer.isView(value)) {
		return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
	}
	if (Array.isArray(value)) {
		const bytes = new Uint8Array(value.length);
		for (const [i, item] of value.entries()) {
			if (!Number.isInteger(item) || item < 0 || item > 255) {
				throw new Error("workflow sqlite value was not a byte array");
			}
			bytes[i] = item;
		}
		return bytes;
	}
	throw new Error("workflow sqlite value was not a blob");
}

function runtimeDbFromContext(
	runCtx?: RunContext<any, any, any, any, any, any, any, any>,
): RawAccess | undefined {
	const db = (runCtx as unknown as { db?: unknown } | undefined)?.db;
	if (
		db &&
		typeof db === "object" &&
		"execute" in db &&
		"transaction" in db
	) {
		return db as RawAccess;
	}
	return undefined;
}

class WorkflowStorage {
	#db: RawAccess;

	constructor(db?: RawAccess) {
		if (!db) {
			throw new Error(
				"workflow storage requires embedded SQLite; actor KV workflow storage is no longer supported",
			);
		}
		this.#db = db;
	}

	async get(key: Uint8Array): Promise<Uint8Array | null> {
		const prefixed = makeWorkflowKey(key);
		const rows = await this.#db.execute<{ value: unknown }>(
			"SELECT value FROM _rivet_wf_kv WHERE key = ?",
			prefixed,
		);
		return rows[0]?.value == null ? null : normalizeSqlBlob(rows[0].value);
	}

	async set(key: Uint8Array, value: Uint8Array): Promise<void> {
		await this.batch([{ key, value }]);
	}

	async delete(key: Uint8Array): Promise<void> {
		const prefixed = makeWorkflowKey(key);
		await this.#db.execute(
			"DELETE FROM _rivet_wf_kv WHERE key = ?",
			prefixed,
		);
	}

	async deletePrefix(prefix: Uint8Array): Promise<void> {
		const start = makeWorkflowKey(prefix);
		const end = computeUpperBound(start);
		if (end) {
			await this.#db.execute(
				"DELETE FROM _rivet_wf_kv WHERE key >= ? AND key < ?",
				start,
				end,
			);
			return;
		}
		const entries = await this.listRaw(start);
		await this.deleteRawKeys(
			entries
				.filter(([key]) => keyStartsWith(key, start))
				.map(([key]) => key),
		);
	}

	async deleteRange(start: Uint8Array, end: Uint8Array): Promise<void> {
		const prefixedStart = makeWorkflowKey(start);
		const prefixedEnd = makeWorkflowKey(end);
		await this.#db.execute(
			"DELETE FROM _rivet_wf_kv WHERE key >= ? AND key < ?",
			prefixedStart,
			prefixedEnd,
		);
	}

	async list(prefix: Uint8Array): Promise<KVEntry[]> {
		const prefixed = makeWorkflowKey(prefix);
		const entries = await this.listRaw(prefixed);
		return entries
			.filter(([key]) => keyStartsWith(key, prefixed))
			.map(([key, value]: KVEntryTuple) => ({
				key: stripWorkflowKey(key),
				value,
			}));
	}

	async batch(writes: KVWrite[]): Promise<void> {
		if (writes.length === 0) return;
		for (const chunk of chunkWorkflowWrites(writes)) {
			await this.#db.transaction(async (tx) => {
				for (const { key, value } of chunk) {
					await tx.execute(
						"INSERT INTO _rivet_wf_kv (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
						makeWorkflowKey(key),
						value,
					);
				}
			});
		}
	}

	async listRaw(prefix: Uint8Array): Promise<KVEntryTuple[]> {
		const end = computeUpperBound(prefix);
		const rows = end
			? await this.#db.execute<{ key: unknown; value: unknown }>(
					"SELECT key, value FROM _rivet_wf_kv WHERE key >= ? AND key < ? ORDER BY key ASC",
					prefix,
					end,
				)
			: await this.#db.execute<{ key: unknown; value: unknown }>(
					"SELECT key, value FROM _rivet_wf_kv WHERE key >= ? ORDER BY key ASC",
					prefix,
				);
		return rows.map((row) => [
			normalizeSqlBlob(row.key),
			normalizeSqlBlob(row.value),
		]);
	}

	async deleteRawKeys(keys: Uint8Array[]): Promise<void> {
		for (
			let start = 0;
			start < keys.length;
			start += WORKFLOW_SQLITE_MAX_BATCH_ROWS
		) {
			await this.#db.transaction(async (tx) => {
				for (const key of keys.slice(
					start,
					start + WORKFLOW_SQLITE_MAX_BATCH_ROWS,
				)) {
					await tx.execute(
						"DELETE FROM _rivet_wf_kv WHERE key = ?",
						key,
					);
				}
			});
		}
	}
}

export function chunkWorkflowWrites(writes: KVWrite[]): KVWrite[][] {
	const chunks: KVWrite[][] = [];
	let chunk: KVWrite[] = [];
	let chunkBytes = 0;
	for (const write of writes) {
		const writeBytes =
			WORKFLOW_STORAGE_PREFIX.length +
			write.key.length +
			write.value.length;
		if (
			chunk.length > 0 &&
			(chunk.length >= WORKFLOW_SQLITE_MAX_BATCH_ROWS ||
				chunkBytes + writeBytes > WORKFLOW_SQLITE_MAX_BATCH_BYTES)
		) {
			chunks.push(chunk);
			chunk = [];
			chunkBytes = 0;
		}
		chunk.push(write);
		chunkBytes += writeBytes;
	}
	if (chunk.length > 0) chunks.push(chunk);
	return chunks;
}

function keyStartsWith(key: Uint8Array, prefix: Uint8Array): boolean {
	if (key.length < prefix.length) return false;
	for (let i = 0; i < prefix.length; i++) {
		if (key[i] !== prefix[i]) return false;
	}
	return true;
}

class ActorWorkflowMessageDriver implements WorkflowMessageDriver {
	#actor: AnyStaticActorInstance;
	#runCtx: RunContext<any, any, any, any, any, any, any, any>;

	constructor(
		actor: AnyStaticActorInstance,
		runCtx: RunContext<any, any, any, any, any, any, any, any>,
	) {
		this.#actor = actor;
		this.#runCtx = runCtx;
	}

	async addMessage(message: Message): Promise<void> {
		await this.#runCtx.internalKeepAwake(
			this.#actor.queueManager.enqueue(message.name, message.data),
		);
	}

	async receiveMessages(opts: {
		names?: readonly string[];
		count: number;
		completable: boolean;
	}): Promise<Message[]> {
		const messages = await this.#runCtx.internalKeepAwake(
			this.#actor.queueManager.receive(
				opts.names && opts.names.length > 0
					? [...opts.names]
					: undefined,
				opts.count,
				0,
				undefined,
				opts.completable,
			),
		);
		return messages.map((message: ReceivedQueueMessage) => ({
			id: message.id.toString(),
			name: message.name,
			data: message.body,
			sentAt: message.createdAt,
			...(opts.completable
				? {
						complete: async (response?: unknown) => {
							await this.#runCtx.internalKeepAwake(
								this.#actor.queueManager.completeMessage(
									message,
									response,
								),
							);
						},
					}
				: {}),
		}));
	}

	async completeMessage(
		messageId: string,
		response?: unknown,
	): Promise<void> {
		let parsedId: bigint;
		try {
			parsedId = BigInt(messageId);
		} catch {
			return;
		}

		await this.#runCtx.internalKeepAwake(
			this.#actor.queueManager.completeMessageById(parsedId, response),
		);
	}
}

export class ActorWorkflowDriver implements EngineDriver {
	readonly atomicBatch = true;
	readonly workerPollInterval = 100;
	readonly messageDriver: WorkflowMessageDriver;
	#actor: AnyStaticActorInstance;
	#runCtx: RunContext<any, any, any, any, any, any, any, any>;
	#storage: WorkflowStorage;

	constructor(
		actor: AnyStaticActorInstance,
		runCtx: RunContext<any, any, any, any, any, any, any, any>,
	) {
		this.#actor = actor;
		this.#runCtx = runCtx;
		this.messageDriver = new ActorWorkflowMessageDriver(actor, runCtx);
		this.#storage = new WorkflowStorage(runtimeDbFromContext(runCtx));
	}

	async get(key: Uint8Array): Promise<Uint8Array | null> {
		return await this.#runCtx.internalKeepAwake(this.#storage.get(key));
	}

	async set(key: Uint8Array, value: Uint8Array): Promise<void> {
		await this.#runCtx.internalKeepAwake(this.#storage.set(key, value));
	}

	async delete(key: Uint8Array): Promise<void> {
		await this.#runCtx.internalKeepAwake(this.#storage.delete(key));
	}

	async deletePrefix(prefix: Uint8Array): Promise<void> {
		await this.#runCtx.internalKeepAwake(
			this.#storage.deletePrefix(prefix),
		);
	}

	async deleteRange(start: Uint8Array, end: Uint8Array): Promise<void> {
		await this.#runCtx.internalKeepAwake(
			this.#storage.deleteRange(start, end),
		);
	}

	async list(prefix: Uint8Array): Promise<KVEntry[]> {
		return await this.#runCtx.internalKeepAwake(this.#storage.list(prefix));
	}

	async batch(writes: KVWrite[]): Promise<void> {
		if (writes.length === 0) return;

		await this.#runCtx.internalKeepAwake(
			this.#actor.stateManager.saveStateAndWorkflowBatch(writes),
		);
	}

	async setAlarm(_workflowId: string, wakeAt: number): Promise<void> {
		await this.#runCtx.internalKeepAwake(
			this.#actor.driver.setAlarm(this.#actor, wakeAt),
		);
	}

	async clearAlarm(_workflowId: string): Promise<void> {
		// No dedicated clear alarm support in actor drivers.
		return;
	}

	waitForMessages(
		messageNames: string[],
		abortSignal: AbortSignal,
	): Promise<void> {
		return this.#actor.queueManager.waitForNames(
			messageNames.length > 0 ? messageNames : undefined,
			abortSignal,
		);
	}
}

class NoopWorkflowMessageDriver implements WorkflowMessageDriver {
	async addMessage(_message: Message): Promise<void> {
		throw new Error("Workflow control driver does not support messages");
	}

	async receiveMessages(_opts: {
		names?: readonly string[];
		count: number;
		completable: boolean;
	}): Promise<Message[]> {
		throw new Error("Workflow control driver does not support messages");
	}

	async completeMessage(
		_messageId: string,
		_response?: unknown,
	): Promise<void> {
		throw new Error("Workflow control driver does not support messages");
	}
}

export class ActorWorkflowControlDriver implements EngineDriver {
	readonly workerPollInterval = 100;
	readonly messageDriver: WorkflowMessageDriver =
		new NoopWorkflowMessageDriver();
	#actor: AnyStaticActorInstance;
	#storage: WorkflowStorage;

	constructor(
		actor: AnyStaticActorInstance,
		runCtx?: RunContext<any, any, any, any, any, any, any, any>,
	) {
		this.#actor = actor;
		this.#storage = new WorkflowStorage(runtimeDbFromContext(runCtx));
	}

	async get(key: Uint8Array): Promise<Uint8Array | null> {
		return await this.#storage.get(key);
	}

	async set(key: Uint8Array, value: Uint8Array): Promise<void> {
		await this.#storage.set(key, value);
	}

	async delete(key: Uint8Array): Promise<void> {
		await this.#storage.delete(key);
	}

	async deletePrefix(prefix: Uint8Array): Promise<void> {
		await this.#storage.deletePrefix(prefix);
	}

	async deleteRange(start: Uint8Array, end: Uint8Array): Promise<void> {
		await this.#storage.deleteRange(start, end);
	}

	async list(prefix: Uint8Array): Promise<KVEntry[]> {
		return await this.#storage.list(prefix);
	}

	async batch(writes: KVWrite[]): Promise<void> {
		if (writes.length === 0) {
			return;
		}

		await this.#storage.batch(writes);
	}

	async setAlarm(_workflowId: string, wakeAt: number): Promise<void> {
		await this.#actor.driver.setAlarm(this.#actor, wakeAt);
	}

	async clearAlarm(_workflowId: string): Promise<void> {
		return;
	}

	waitForMessages(
		_messageNames: string[],
		_abortSignal: AbortSignal,
	): Promise<void> {
		throw new Error("Workflow control driver does not support messages");
	}
}
