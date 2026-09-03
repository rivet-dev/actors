import type { EngineDriver, KVEntry, KVWrite } from "./driver.js";
import { EvictedError } from "./errors.js";
import { compareKeys, keyStartsWith, keyToHex } from "./keys.js";
import type { Message, WorkflowMessageDriver } from "./types.js";
import { sleep } from "./utils.js";

interface Waiter {
	nameSet?: Set<string>;
	resolve: () => void;
	reject: (error: Error) => void;
	abortSignal: AbortSignal;
	onAbort: () => void;
}

class InMemoryWorkflowMessageDriver implements WorkflowMessageDriver {
	#messages: Message[] = [];
	#waiters = new Set<Waiter>();

	async addMessage(message: Message): Promise<void> {
		this.#messages.push(message);
		this.#notifyWaiters(message.name);
	}

	async receiveMessages(opts: {
		names?: readonly string[];
		count: number;
		completable: boolean;
	}): Promise<Message[]> {
		const limitedCount = Math.max(1, opts.count);
		const nameSet =
			opts.names && opts.names.length > 0
				? new Set(opts.names)
				: undefined;
		const selected: Array<{ message: Message; index: number }> = [];

		for (
			let i = 0;
			i < this.#messages.length && selected.length < limitedCount;
			i++
		) {
			const message = this.#messages[i];
			if (nameSet && !nameSet.has(message.name)) {
				continue;
			}
			selected.push({ message, index: i });
		}

		if (selected.length === 0) {
			return [];
		}

		if (!opts.completable) {
			for (let i = selected.length - 1; i >= 0; i--) {
				this.#messages.splice(selected[i].index, 1);
			}
			return selected.map((entry) => entry.message);
		}

		return selected.map((entry) => {
			const { message } = entry;
			return {
				...message,
				complete: async () => {
					await this.completeMessage(message.id);
				},
			};
		});
	}

	async completeMessage(messageId: string): Promise<void> {
		const index = this.#messages.findIndex(
			(message) => message.id === messageId,
		);
		if (index !== -1) {
			this.#messages.splice(index, 1);
		}
	}

	async waitForMessages(
		messageNames: string[],
		abortSignal: AbortSignal,
	): Promise<void> {
		if (abortSignal.aborted) {
			throw new EvictedError();
		}

		const nameSet =
			messageNames.length > 0 ? new Set(messageNames) : undefined;
		if (
			this.#messages.some((message) =>
				nameSet ? nameSet.has(message.name) : true,
			)
		) {
			return;
		}

		await new Promise<void>((resolve, reject) => {
			const waiter: Waiter = {
				nameSet,
				resolve: () => {
					this.#removeWaiter(waiter);
					resolve();
				},
				reject: (error) => {
					this.#removeWaiter(waiter);
					reject(error);
				},
				abortSignal,
				onAbort: () => {
					waiter.reject(new EvictedError());
				},
			};
			abortSignal.addEventListener("abort", waiter.onAbort, {
				once: true,
			});
			this.#waiters.add(waiter);
		});
	}

	#removeWaiter(waiter: Waiter): void {
		if (this.#waiters.delete(waiter)) {
			waiter.abortSignal.removeEventListener("abort", waiter.onAbort);
		}
	}

	#notifyWaiters(name: string): void {
		for (const waiter of [...this.#waiters]) {
			if (waiter.nameSet && !waiter.nameSet.has(name)) {
				continue;
			}
			waiter.resolve();
		}
	}

	clear(): void {
		this.#messages = [];
		for (const waiter of [...this.#waiters]) {
			waiter.reject(new Error("cleared"));
		}
	}
}

/**
 * In-memory implementation of EngineDriver for testing.
 * Uses binary keys (Uint8Array) with hex encoding for internal Map storage.
 */
export class InMemoryDriver implements EngineDriver {
	// Map from hex-encoded key to { originalKey, value }
	private kv = new Map<string, { key: Uint8Array; value: Uint8Array }>();
	private alarms = new Map<string, number>();
	#inMemoryMessageDriver = new InMemoryWorkflowMessageDriver();

	/** Simulated latency per operation (ms) */
	latency = 10;

	/** How often the worker polls for work */
	workerPollInterval = 100;
	messageDriver: WorkflowMessageDriver = this.#inMemoryMessageDriver;

	async get(key: Uint8Array): Promise<Uint8Array | null> {
		await sleep(this.latency);
		const entry = this.kv.get(keyToHex(key));
		return entry?.value ?? null;
	}

	async set(key: Uint8Array, value: Uint8Array): Promise<void> {
		await sleep(this.latency);
		this.kv.set(keyToHex(key), { key, value });
	}

	async delete(key: Uint8Array): Promise<void> {
		await sleep(this.latency);
		this.kv.delete(keyToHex(key));
	}

	async batchDelete(keys: Uint8Array[]): Promise<void> {
		await sleep(this.latency);
		for (const key of keys) {
			this.kv.delete(keyToHex(key));
		}
	}

	async deletePrefix(prefix: Uint8Array): Promise<void> {
		await sleep(this.latency);
		for (const [hexKey, entry] of this.kv) {
			if (keyStartsWith(entry.key, prefix)) {
				this.kv.delete(hexKey);
			}
		}
	}

	async deleteRange(start: Uint8Array, end: Uint8Array): Promise<void> {
		await sleep(this.latency);
		for (const [hexKey, entry] of this.kv) {
			if (
				compareKeys(entry.key, start) >= 0 &&
				compareKeys(entry.key, end) < 0
			) {
				this.kv.delete(hexKey);
			}
		}
	}

	async list(prefix: Uint8Array): Promise<KVEntry[]> {
		await sleep(this.latency);
		const results: KVEntry[] = [];
		for (const entry of this.kv.values()) {
			if (keyStartsWith(entry.key, prefix)) {
				results.push({ key: entry.key, value: entry.value });
			}
		}
		// Sort by key lexicographically
		return results.sort((a, b) => compareKeys(a.key, b.key));
	}

	async batch(writes: KVWrite[]): Promise<void> {
		await sleep(this.latency);
		for (const { key, value } of writes) {
			this.kv.set(keyToHex(key), { key, value });
		}
	}

	async setAlarm(workflowId: string, wakeAt: number): Promise<void> {
		await sleep(this.latency);
		this.alarms.set(workflowId, wakeAt);
	}

	async clearAlarm(workflowId: string): Promise<void> {
		await sleep(this.latency);
		this.alarms.delete(workflowId);
	}

	async waitForMessages(
		messageNames: string[],
		abortSignal: AbortSignal,
	): Promise<void> {
		const driver = this.messageDriver as WorkflowMessageDriver & {
			waitForMessages?: (
				messageNames: string[],
				abortSignal: AbortSignal,
			) => Promise<void>;
		};
		if (driver.waitForMessages) {
			await driver.waitForMessages(messageNames, abortSignal);
			return;
		}

		while (true) {
			if (abortSignal.aborted) {
				throw new EvictedError();
			}
			const messages = await this.messageDriver.receiveMessages({
				names: messageNames.length > 0 ? messageNames : undefined,
				count: 1,
				completable: true,
			});
			if (messages.length > 0) {
				return;
			}
			await sleep(Math.max(1, this.latency));
		}
	}

	/**
	 * Get the alarm time for a workflow (for testing).
	 */
	getAlarm(workflowId: string): number | undefined {
		return this.alarms.get(workflowId);
	}

	/**
	 * Check if any alarms are due and return their workflow IDs.
	 */
	getDueAlarms(): string[] {
		const now = Date.now();
		const due: string[] = [];
		for (const [workflowId, wakeAt] of this.alarms) {
			if (wakeAt <= now) {
				due.push(workflowId);
			}
		}
		return due;
	}

	/**
	 * Clear all data (for testing).
	 */
	clear(): void {
		this.kv.clear();
		this.alarms.clear();
		this.#inMemoryMessageDriver.clear();
	}

	/**
	 * Get a snapshot of all data (for testing/debugging).
	 */
	snapshot(): {
		kv: Record<string, Uint8Array>;
		alarms: Record<string, number>;
	} {
		const kvSnapshot: Record<string, Uint8Array> = {};
		for (const [hexKey, entry] of this.kv) {
			kvSnapshot[hexKey] = entry.value;
		}
		return {
			kv: kvSnapshot,
			alarms: Object.fromEntries(this.alarms),
		};
	}

	/**
	 * Get all hex-encoded keys (for testing).
	 */
	keys(): string[] {
		return [...this.kv.keys()];
	}
}

// Re-export main exports for convenience
export * from "./index.js";
