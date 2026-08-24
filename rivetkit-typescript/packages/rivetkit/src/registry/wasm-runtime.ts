import type {
	ActorContext as WasmActorContext,
	ActorFactory as WasmActorFactory,
	CancellationToken as WasmCancellationToken,
	ConnHandle as WasmConnHandle,
	CoreRegistry as WasmCoreRegistry,
	WebSocketHandle as WasmWebSocketHandle,
} from "@rivetkit/rivetkit-wasm";
import { decodeBridgeRivetError, RivetError } from "@/actor/errors";
import type {
	WasmRuntimeBindings,
	WasmRuntimeConfig,
	WasmRuntimeInitInput,
} from "./config";
import type {
	ActorContextHandle,
	ActorFactoryHandle,
	CancellationTokenHandle,
	ConnHandle,
	CoreRuntime,
	RegistryHandle,
	RuntimeActorConfig,
	RuntimeActorKeySegment,
	RuntimeApplicationListenerConfig,
	RuntimeBytes,
	RuntimeHttpRequest,
	RuntimeInspectorSnapshot,
	RuntimeKvEntry,
	RuntimeKvListOptions,
	RuntimeListenerConfig,
	RuntimeQueueEnqueueAndWaitOptions,
	RuntimeQueueInspectMessage,
	RuntimeQueueMessage,
	RuntimeQueueNextBatchOptions,
	RuntimeQueueTryNextBatchOptions,
	RuntimeQueueWaitOptions,
	RuntimeCronFire,
	RuntimeCronJobInfo,
	RuntimeRegistryRouteResponse,
	RuntimeRequestSaveOpts,
	RuntimeServeConfig,
	RuntimeScheduledEventInfo,
	RuntimeServerlessRequest,
	RuntimeServerlessResponseHead,
	RuntimeServerlessStreamCallback,
	RuntimeSqlBatchStatement,
	RuntimeSqlBindParams,
	RuntimeSqlDatabase,
	RuntimeSqlExecResult,
	RuntimeSqlExecuteResult,
	RuntimeSqlQueryResult,
	RuntimeSqlRunResult,
	RuntimeStateDeltaPayload,
	RuntimeWebSocketEvent,
	RuntimeWorkflowKvWrite,
	SqliteTransactionHandle,
	WebSocketHandle,
} from "./runtime";
import { normalizeRuntimeSqlExecuteResult } from "./runtime";

type WasmBindings = WasmRuntimeBindings;
export type WasmInitInput = WasmRuntimeInitInput;
type AnyFunction = (...args: unknown[]) => unknown;
type WasmRuntimeLoadConfig = Pick<WasmRuntimeConfig, "bindings" | "initInput">;

function asWasmRegistry(handle: RegistryHandle): WasmCoreRegistry {
	return handle as unknown as WasmCoreRegistry;
}

function asWasmFactory(handle: ActorFactoryHandle): WasmActorFactory {
	return handle as unknown as WasmActorFactory;
}

function asWasmActorContext(handle: ActorContextHandle): WasmActorContext {
	return handle as unknown as WasmActorContext;
}

function asWasmConn(handle: ConnHandle): WasmConnHandle {
	return handle as unknown as WasmConnHandle;
}

function asWasmWebSocket(handle: WebSocketHandle): WasmWebSocketHandle {
	return handle as unknown as WasmWebSocketHandle;
}

function asWasmCancellationToken(
	handle: CancellationTokenHandle,
): WasmCancellationToken {
	return handle as unknown as WasmCancellationToken;
}

function asRegistryHandle(handle: WasmCoreRegistry): RegistryHandle {
	return handle as unknown as RegistryHandle;
}

function asActorFactoryHandle(handle: WasmActorFactory): ActorFactoryHandle {
	return handle as unknown as ActorFactoryHandle;
}

function toBytes(value: RuntimeBytes | null | undefined): RuntimeBytes {
	return value ?? new Uint8Array(0);
}

function optionalBytes(
	value: RuntimeBytes | null | undefined,
): RuntimeBytes | null {
	if (value === null || value === undefined) {
		return null;
	}
	return toBytes(value);
}

function optionalWasmNumber(
	value: number | bigint | null | undefined,
): number | null | undefined {
	if (value === null || value === undefined) {
		return value;
	}
	return typeof value === "bigint" ? Number(value) : value;
}

function wasmNumber(value: number | bigint): number {
	return typeof value === "bigint" ? Number(value) : value;
}

function normalizeKvEntry(entry: RuntimeKvEntry): RuntimeKvEntry {
	return {
		key: toBytes(entry.key),
		value: toBytes(entry.value),
	};
}

function normalizeQueueMessage(
	message: RuntimeQueueMessage,
): RuntimeQueueMessage {
	return {
		id: () => message.id(),
		name: () => message.name(),
		body: () => toBytes(message.body()),
		createdAt: () => message.createdAt(),
		isCompletable: () => message.isCompletable(),
		complete: async (response?: RuntimeBytes | undefined | null) => {
			await callWasm(() => message.complete(response));
		},
	};
}

function normalizeWasmBridgeError(error: unknown): unknown {
	if (typeof error === "string") {
		return decodeBridgeRivetError(error) ?? error;
	}

	if (error instanceof Error) {
		const bridged = decodeBridgeRivetError(error.message);
		if (bridged) {
			return bridged;
		}
	}

	if (
		typeof error === "object" &&
		error !== null &&
		"reason" in error &&
		typeof error.reason === "string"
	) {
		const bridged = decodeBridgeRivetError(error.reason);
		if (bridged) {
			return bridged;
		}
	}

	return error;
}

async function callWasm<T>(invoke: () => Promise<T>): Promise<T> {
	try {
		return await invoke();
	} catch (error) {
		throw normalizeWasmBridgeError(error);
	}
}

function callWasmSync<T>(invoke: () => T): T {
	try {
		return invoke();
	} catch (error) {
		throw normalizeWasmBridgeError(error);
	}
}

function unsupportedWasmMethod(method: string): never {
	throw new RivetError(
		"runtime",
		"unsupported",
		`Unsupported wasm runtime method: ${method}`,
		{
			metadata: {
				runtime: "wasm",
				method,
			},
		},
	);
}

function method<T extends AnyFunction>(target: unknown, name: string): T {
	if (
		typeof target === "object" &&
		target !== null &&
		name in target &&
		typeof target[name as keyof typeof target] === "function"
	) {
		return target[name as keyof typeof target] as T;
	}
	return unsupportedWasmMethod(name);
}

function callHandle<T>(handle: unknown, name: string, ...args: unknown[]): T {
	return callWasmSync(() => method(handle, name).apply(handle, args) as T);
}

async function callHandleAsync<T>(
	handle: unknown,
	name: string,
	...args: unknown[]
): Promise<T> {
	return await callWasm(
		async () => (await method(handle, name).apply(handle, args)) as T,
	);
}

function childHandle<T>(handle: unknown, name: string): T {
	return callHandle<T>(handle, name);
}

export class WasmCoreRuntime implements CoreRuntime {
	readonly kind = "wasm";

	#bindings: WasmBindings;
	#sql = new WeakMap<WasmActorContext, RuntimeSqlDatabase>();

	constructor(bindings: WasmBindings) {
		this.#bindings = bindings;
	}

	#actorSql(ctx: ActorContextHandle): RuntimeSqlDatabase {
		const wasmCtx = asWasmActorContext(ctx);
		let database = this.#sql.get(wasmCtx);
		if (!database) {
			database = callHandle<RuntimeSqlDatabase>(wasmCtx, "sql");
			this.#sql.set(wasmCtx, database);
		}
		return database;
	}

	createRegistry(): RegistryHandle {
		return callWasmSync(() =>
			asRegistryHandle(new this.#bindings.CoreRegistry()),
		);
	}

	registerActor(
		registry: RegistryHandle,
		name: string,
		factory: ActorFactoryHandle,
	): void {
		callWasmSync(() =>
			asWasmRegistry(registry).register(name, asWasmFactory(factory)),
		);
	}

	async serveRegistry(
		registry: RegistryHandle,
		config: RuntimeServeConfig,
	): Promise<void> {
		await callWasm(() => asWasmRegistry(registry).serve(config));
	}

	async waitRegistryReady(): Promise<void> {
		throw new Error(
			"registry.startAndWait() is not supported by the WASM runtime",
		);
	}

	async shutdownRegistry(registry: RegistryHandle): Promise<void> {
		await callWasm(() => asWasmRegistry(registry).shutdown());
	}

	async registryHealth(): Promise<RuntimeRegistryRouteResponse> {
		return {
			status: 200,
			headers: { "content-type": "application/json" },
			body: new TextEncoder().encode(
				JSON.stringify({
					status: "ok",
					runtime: "rivetkit",
					version: "wasm",
				}),
			),
		};
	}

	async handleServerlessRequest(
		registry: RegistryHandle,
		req: RuntimeServerlessRequest,
		onStreamEvent: RuntimeServerlessStreamCallback,
		cancelToken: CancellationTokenHandle,
		config: RuntimeServeConfig,
	): Promise<RuntimeServerlessResponseHead> {
		return await callHandleAsync<RuntimeServerlessResponseHead>(
			asWasmRegistry(registry),
			"handleServerlessRequest",
			req,
			onStreamEvent,
			asWasmCancellationToken(cancelToken),
			config,
		);
	}

	async serveListener(
		_registry: RegistryHandle,
		_listener: RuntimeListenerConfig,
		_config: RuntimeServeConfig,
	): Promise<void> {
		throw new Error(
			"registry.listen() is not supported on the wasm runtime; use registry.serve() and mount the handler in your platform's HTTP server instead",
		);
	}

	async serveApplicationListener(
		_registry: RegistryHandle,
		_listener: RuntimeApplicationListenerConfig,
		_config: RuntimeServeConfig,
	): Promise<void> {
		throw new Error(
			"registry.listen() is not supported on the wasm runtime; use an application-owned HTTP server instead",
		);
	}

	createActorFactory(
		callbacks: object,
		config?: RuntimeActorConfig | undefined | null,
	): ActorFactoryHandle {
		return callWasmSync(() =>
			asActorFactoryHandle(
				new this.#bindings.ActorFactory(callbacks, config),
			),
		);
	}

	createCancellationToken(): CancellationTokenHandle {
		return callWasmSync(
			() =>
				new this.#bindings.CancellationToken() as unknown as CancellationTokenHandle,
		);
	}

	cancellationTokenAborted(token: CancellationTokenHandle): boolean {
		return callWasmSync(() => asWasmCancellationToken(token).aborted());
	}

	cancelCancellationToken(token: CancellationTokenHandle): void {
		callWasmSync(() => asWasmCancellationToken(token).cancel());
	}

	onCancellationTokenCancelled(
		token: CancellationTokenHandle,
		callback: (...args: unknown[]) => unknown,
	): void {
		callWasmSync(() =>
			asWasmCancellationToken(token).onCancelled(callback),
		);
	}

	actorState(ctx: ActorContextHandle): RuntimeBytes {
		return toBytes(
			callHandle<RuntimeBytes | Uint8Array>(
				asWasmActorContext(ctx),
				"state",
			),
		);
	}

	actorBeginOnStateChange(ctx: ActorContextHandle): void {
		callHandle(asWasmActorContext(ctx), "beginOnStateChange");
	}

	actorEndOnStateChange(ctx: ActorContextHandle): void {
		callHandle(asWasmActorContext(ctx), "endOnStateChange");
	}

	actorSetAlarm(
		ctx: ActorContextHandle,
		timestampMs?: number | bigint | undefined | null,
	): void {
		callHandle(
			asWasmActorContext(ctx),
			"setAlarm",
			optionalWasmNumber(timestampMs),
		);
	}

	actorRequestSave(
		ctx: ActorContextHandle,
		opts?: RuntimeRequestSaveOpts | undefined | null,
	): void {
		callHandle(asWasmActorContext(ctx), "requestSave", opts);
	}

	async actorRequestSaveAndWait(
		ctx: ActorContextHandle,
		opts?: RuntimeRequestSaveOpts | undefined | null,
	): Promise<void> {
		await callHandleAsync(
			asWasmActorContext(ctx),
			"requestSaveAndWait",
			opts,
		);
	}

	actorInspectorSnapshot(ctx: ActorContextHandle): RuntimeInspectorSnapshot {
		return callHandle(asWasmActorContext(ctx), "inspectorSnapshot");
	}

	actorDecodeInspectorRequest(
		ctx: ActorContextHandle,
		bytes: RuntimeBytes,
		advertisedVersion: number,
	): RuntimeBytes {
		return toBytes(
			callHandle<RuntimeBytes | Uint8Array>(
				asWasmActorContext(ctx),
				"decodeInspectorRequest",
				bytes,
				advertisedVersion,
			),
		);
	}

	actorEncodeInspectorResponse(
		ctx: ActorContextHandle,
		bytes: RuntimeBytes,
		targetVersion: number,
	): RuntimeBytes {
		return toBytes(
			callHandle<RuntimeBytes | Uint8Array>(
				asWasmActorContext(ctx),
				"encodeInspectorResponse",
				bytes,
				targetVersion,
			),
		);
	}

	async actorVerifyInspectorAuth(
		ctx: ActorContextHandle,
		bearerToken?: string | undefined | null,
	): Promise<void> {
		await callHandleAsync(
			asWasmActorContext(ctx),
			"verifyInspectorAuth",
			bearerToken,
		);
	}

	actorQueueHibernationRemoval(
		ctx: ActorContextHandle,
		connId: string,
	): void {
		callHandle(asWasmActorContext(ctx), "queueHibernationRemoval", connId);
	}

	actorTakePendingHibernationChanges(ctx: ActorContextHandle): string[] {
		return callHandle(
			asWasmActorContext(ctx),
			"takePendingHibernationChanges",
		);
	}

	actorDirtyHibernatableConns(ctx: ActorContextHandle): ConnHandle[] {
		return callHandle(asWasmActorContext(ctx), "dirtyHibernatableConns");
	}

	async actorSaveState(
		ctx: ActorContextHandle,
		payload: RuntimeStateDeltaPayload,
	): Promise<void> {
		await callHandleAsync(asWasmActorContext(ctx), "saveState", payload);
	}

	async actorSaveStateAndWorkflowBatch(
		ctx: ActorContextHandle,
		writes: RuntimeWorkflowKvWrite[],
	): Promise<void> {
		await callHandleAsync(
			asWasmActorContext(ctx),
			"saveStateAndWorkflowBatch",
			writes,
		);
	}

	actorId(ctx: ActorContextHandle): string {
		return callHandle(asWasmActorContext(ctx), "actorId");
	}

	actorName(ctx: ActorContextHandle): string {
		return callHandle(asWasmActorContext(ctx), "name");
	}

	actorKey(ctx: ActorContextHandle): RuntimeActorKeySegment[] {
		return callHandle(asWasmActorContext(ctx), "key");
	}

	actorRegion(ctx: ActorContextHandle): string {
		return callHandle(asWasmActorContext(ctx), "region");
	}

	actorSleep(ctx: ActorContextHandle): void {
		callHandle(asWasmActorContext(ctx), "sleep");
	}

	actorDestroy(ctx: ActorContextHandle): void {
		callHandle(asWasmActorContext(ctx), "destroy");
	}

	actorAbortSignal(ctx: ActorContextHandle): AbortSignal {
		return callHandle(asWasmActorContext(ctx), "abortSignal");
	}

	actorConns(ctx: ActorContextHandle): ConnHandle[] {
		return callHandle(asWasmActorContext(ctx), "conns");
	}

	async actorConnectConn(
		ctx: ActorContextHandle,
		params: RuntimeBytes,
		request?: RuntimeHttpRequest | undefined | null,
	): Promise<ConnHandle> {
		return await callHandleAsync(
			asWasmActorContext(ctx),
			"connectConn",
			params,
			request,
		);
	}

	actorBroadcast(
		ctx: ActorContextHandle,
		name: string,
		args: RuntimeBytes,
	): void {
		callHandle(asWasmActorContext(ctx), "broadcast", name, args);
	}

	actorWaitUntil(ctx: ActorContextHandle, promise: Promise<unknown>): void {
		callHandle(asWasmActorContext(ctx), "waitUntil", promise);
	}

	async actorWaitForTrackedShutdownWork(
		ctx: ActorContextHandle,
	): Promise<boolean> {
		return await callHandle<Promise<boolean>>(
			asWasmActorContext(ctx),
			"waitForTrackedShutdownWork",
		);
	}

	async actorWaitForTrackedShutdownWorkUnbounded(
		ctx: ActorContextHandle,
	): Promise<void> {
		await callHandle<Promise<void>>(
			asWasmActorContext(ctx),
			"waitForTrackedShutdownWorkUnbounded",
		);
	}

	actorKeepAwake(ctx: ActorContextHandle, promise: Promise<unknown>): void {
		const wasmCtx = asWasmActorContext(ctx);
		const regionId = callHandle<number>(wasmCtx, "beginKeepAwake");
		const trackedPromise = Promise.resolve(promise)
			.finally(() => {
				callHandle(wasmCtx, "endKeepAwake", regionId);
			})
			.then(() => null);
		callHandle(wasmCtx, "registerTask", trackedPromise);
	}

	actorBeginKeepAwake(ctx: ActorContextHandle): number {
		return callHandle<number>(asWasmActorContext(ctx), "beginKeepAwake");
	}

	actorEndKeepAwake(ctx: ActorContextHandle, regionId: number): void {
		callHandle(asWasmActorContext(ctx), "endKeepAwake", regionId);
	}

	actorRegisterTask(
		ctx: ActorContextHandle,
		promise: Promise<unknown>,
	): void {
		callHandle(asWasmActorContext(ctx), "registerTask", promise);
	}

	actorRuntimeState(ctx: ActorContextHandle): object {
		return callHandle(asWasmActorContext(ctx), "runtimeState");
	}

	actorClearRuntimeState(ctx: ActorContextHandle): void {
		const runtimeState = this.actorRuntimeState(ctx);
		for (const key of Object.keys(runtimeState)) {
			delete (runtimeState as Record<string, unknown>)[key];
		}
	}

	actorRestartRunHandler(ctx: ActorContextHandle): void {
		callHandle(asWasmActorContext(ctx), "restartRunHandler");
	}

	actorBeginWebsocketCallback(ctx: ActorContextHandle): number {
		return callHandle(asWasmActorContext(ctx), "beginWebsocketCallback");
	}

	actorEndWebsocketCallback(ctx: ActorContextHandle, regionId: number): void {
		callHandle(asWasmActorContext(ctx), "endWebsocketCallback", regionId);
	}

	async actorKvGet(
		ctx: ActorContextHandle,
		key: RuntimeBytes,
	): Promise<RuntimeBytes | null> {
		const kv = childHandle(asWasmActorContext(ctx), "kv");
		return optionalBytes(await callHandleAsync(kv, "get", key));
	}

	async actorKvPut(
		ctx: ActorContextHandle,
		key: RuntimeBytes,
		value: RuntimeBytes,
	): Promise<void> {
		const kv = childHandle(asWasmActorContext(ctx), "kv");
		await callHandleAsync(kv, "put", key, value);
	}

	async actorKvDelete(
		ctx: ActorContextHandle,
		key: RuntimeBytes,
	): Promise<void> {
		const kv = childHandle(asWasmActorContext(ctx), "kv");
		await callHandleAsync(kv, "delete", key);
	}

	async actorKvDeleteRange(
		ctx: ActorContextHandle,
		start: RuntimeBytes,
		end: RuntimeBytes,
	): Promise<void> {
		const kv = childHandle(asWasmActorContext(ctx), "kv");
		await callHandleAsync(kv, "deleteRange", start, end);
	}

	async actorKvListPrefix(
		ctx: ActorContextHandle,
		prefix: RuntimeBytes,
		options?: RuntimeKvListOptions | undefined | null,
	): Promise<RuntimeKvEntry[]> {
		const kv = childHandle(asWasmActorContext(ctx), "kv");
		const entries = await callHandleAsync<RuntimeKvEntry[]>(
			kv,
			"listPrefix",
			prefix,
			options,
		);
		return entries.map(normalizeKvEntry);
	}

	async actorKvListRange(
		ctx: ActorContextHandle,
		start: RuntimeBytes,
		end: RuntimeBytes,
		options?: RuntimeKvListOptions | undefined | null,
	): Promise<RuntimeKvEntry[]> {
		const kv = childHandle(asWasmActorContext(ctx), "kv");
		const entries = await callHandleAsync<RuntimeKvEntry[]>(
			kv,
			"listRange",
			start,
			end,
			options,
		);
		return entries.map(normalizeKvEntry);
	}

	async actorKvBatchGet(
		ctx: ActorContextHandle,
		keys: RuntimeBytes[],
	): Promise<Array<RuntimeBytes | undefined | null>> {
		const kv = childHandle(asWasmActorContext(ctx), "kv");
		const values = await callHandleAsync<
			Array<RuntimeBytes | Uint8Array | null | undefined>
		>(kv, "batchGet", keys);
		return values.map((value) =>
			value === undefined ? undefined : optionalBytes(value),
		);
	}

	async actorKvBatchPut(
		ctx: ActorContextHandle,
		entries: RuntimeKvEntry[],
	): Promise<void> {
		const kv = childHandle(asWasmActorContext(ctx), "kv");
		await callHandleAsync(kv, "batchPut", entries);
	}

	async actorKvBatchDelete(
		ctx: ActorContextHandle,
		keys: RuntimeBytes[],
	): Promise<void> {
		const kv = childHandle(asWasmActorContext(ctx), "kv");
		await callHandleAsync(kv, "batchDelete", keys);
	}

	async actorSqlExec(
		ctx: ActorContextHandle,
		sql: string,
	): Promise<RuntimeSqlExecResult> {
		return await callWasm(() => this.#actorSql(ctx).exec(sql));
	}

	async actorSqlExecute(
		ctx: ActorContextHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlExecuteResult> {
		const result = await callWasm(() =>
			this.#actorSql(ctx).execute(sql, params),
		);
		return normalizeRuntimeSqlExecuteResult(result);
	}

	async actorSqlExecuteBatch(
		ctx: ActorContextHandle,
		statements: RuntimeSqlBatchStatement[],
	): Promise<RuntimeSqlExecuteResult[]> {
		const results = await callWasm(() =>
			this.#actorSql(ctx).executeBatch(statements),
		);
		return results.map(normalizeRuntimeSqlExecuteResult);
	}

	async actorSqlBeginTransaction(
		ctx: ActorContextHandle,
		timeoutMs?: number,
	): Promise<SqliteTransactionHandle> {
		return (await callWasm(() =>
			this.#actorSql(ctx).beginTransaction(timeoutMs),
		)) as unknown as SqliteTransactionHandle;
	}

	async actorSqlTransactionExec(
		transaction: SqliteTransactionHandle,
		sql: string,
	): Promise<RuntimeSqlExecResult> {
		return await callWasm(() =>
			(
				transaction as unknown as {
					exec(sql: string): Promise<RuntimeSqlExecResult>;
				}
			).exec(sql),
		);
	}

	async actorSqlTransactionExecute(
		transaction: SqliteTransactionHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlExecuteResult> {
		const result = await callWasm(() =>
			(
				transaction as unknown as {
					execute(
						sql: string,
						params?: RuntimeSqlBindParams,
					): Promise<RuntimeSqlExecuteResult>;
				}
			).execute(sql, params),
		);
		return normalizeRuntimeSqlExecuteResult(result);
	}

	async actorSqlTransactionCommit(
		transaction: SqliteTransactionHandle,
	): Promise<void> {
		await callWasm(() =>
			(transaction as unknown as { commit(): Promise<void> }).commit(),
		);
	}

	async actorSqlTransactionRollback(
		transaction: SqliteTransactionHandle,
	): Promise<void> {
		await callWasm(() =>
			(
				transaction as unknown as { rollback(): Promise<void> }
			).rollback(),
		);
	}

	async actorSqlQuery(
		ctx: ActorContextHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlQueryResult> {
		return await callWasm(() => this.#actorSql(ctx).query(sql, params));
	}

	async actorSqlRun(
		ctx: ActorContextHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlRunResult> {
		return await callWasm(() => this.#actorSql(ctx).run(sql, params));
	}

	actorSqlMetrics(ctx: ActorContextHandle) {
		return this.#actorSql(ctx).metrics?.() ?? null;
	}

	actorSqlTakeLastKvError(ctx: ActorContextHandle): string | null {
		return this.#actorSql(ctx).takeLastKvError?.() ?? null;
	}

	async actorSqlClose(ctx: ActorContextHandle): Promise<void> {
		const wasmCtx = asWasmActorContext(ctx);
		const database = this.#sql.get(wasmCtx);
		if (!database) {
			return;
		}

		this.#sql.delete(wasmCtx);
		await callWasm(() => database.close());
	}

	async actorRuntimeSocketProvision(
		_ctx: ActorContextHandle,
	): Promise<never> {
		throw new RivetError(
			"actor_runtime_socket",
			"unsupported",
			"Actor Runtime Socket is only available on Unix native runtimes.",
			{
				public: true,
				metadata: { runtime: "wasm" },
			},
		);
	}

	async actorQueueSend(
		ctx: ActorContextHandle,
		name: string,
		body: RuntimeBytes,
	): Promise<RuntimeQueueMessage> {
		const queue = childHandle(asWasmActorContext(ctx), "queue");
		return normalizeQueueMessage(
			await callHandleAsync(queue, "send", name, body),
		);
	}

	async actorQueueNextBatch(
		ctx: ActorContextHandle,
		options?: RuntimeQueueNextBatchOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<RuntimeQueueMessage[]> {
		const queue = childHandle(asWasmActorContext(ctx), "queue");
		const messages = await callHandleAsync<RuntimeQueueMessage[]>(
			queue,
			"nextBatch",
			options,
			signal ? asWasmCancellationToken(signal) : signal,
		);
		return messages.map(normalizeQueueMessage);
	}

	async actorQueueWaitForNames(
		ctx: ActorContextHandle,
		names: string[],
		options?: RuntimeQueueWaitOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<RuntimeQueueMessage> {
		const queue = childHandle(asWasmActorContext(ctx), "queue");
		return normalizeQueueMessage(
			await callHandleAsync(
				queue,
				"waitForNames",
				names,
				options,
				signal ? asWasmCancellationToken(signal) : signal,
			),
		);
	}

	async actorQueueWaitForNamesAvailable(
		ctx: ActorContextHandle,
		names: string[],
		options?: RuntimeQueueWaitOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<void> {
		const queue = childHandle(asWasmActorContext(ctx), "queue");
		await callHandleAsync(
			queue,
			"waitForNamesAvailable",
			names,
			options,
			signal,
		);
	}

	async actorQueueEnqueueAndWait(
		ctx: ActorContextHandle,
		name: string,
		body: RuntimeBytes,
		options?: RuntimeQueueEnqueueAndWaitOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<RuntimeBytes | null> {
		const queue = childHandle(asWasmActorContext(ctx), "queue");
		return optionalBytes(
			await callHandleAsync(
				queue,
				"enqueueAndWait",
				name,
				body,
				options,
				signal ? asWasmCancellationToken(signal) : signal,
			),
		);
	}

	actorQueueTryNextBatch(
		ctx: ActorContextHandle,
		options?: RuntimeQueueTryNextBatchOptions | undefined | null,
	): RuntimeQueueMessage[] {
		const queue = childHandle(asWasmActorContext(ctx), "queue");
		return callHandle<RuntimeQueueMessage[]>(
			queue,
			"tryNextBatch",
			options,
		).map(normalizeQueueMessage);
	}

	actorQueueMaxSize(ctx: ActorContextHandle): number {
		const queue = childHandle(asWasmActorContext(ctx), "queue");
		return callHandle(queue, "maxSize");
	}

	async actorQueueInspectMessages(
		ctx: ActorContextHandle,
	): Promise<RuntimeQueueInspectMessage[]> {
		const queue = childHandle(asWasmActorContext(ctx), "queue");
		return await callHandleAsync(queue, "inspectMessages");
	}

	async actorQueueReset(ctx: ActorContextHandle): Promise<void> {
		const queue = childHandle(asWasmActorContext(ctx), "queue");
		await callHandleAsync(queue, "reset");
	}

	async actorScheduleAfter(
		ctx: ActorContextHandle,
		durationMs: number | bigint,
		actionName: string,
		args: RuntimeBytes,
	): Promise<string> {
		const schedule = childHandle(asWasmActorContext(ctx), "schedule");
		return await callHandleAsync<string>(
			schedule,
			"after",
			wasmNumber(durationMs),
			actionName,
			args,
		);
	}

	async actorScheduleAt(
		ctx: ActorContextHandle,
		timestampMs: number | bigint,
		actionName: string,
		args: RuntimeBytes,
	): Promise<string> {
		const schedule = childHandle(asWasmActorContext(ctx), "schedule");
		return await callHandleAsync<string>(
			schedule,
			"at",
			wasmNumber(timestampMs),
			actionName,
			args,
		);
	}

	async actorScheduleCancel(ctx: ActorContextHandle, id: string) {
		const schedule = childHandle(asWasmActorContext(ctx), "schedule");
		return await callHandleAsync<boolean>(schedule, "cancel", id);
	}

	async actorScheduleGet(ctx: ActorContextHandle, id: string) {
		const schedule = childHandle(asWasmActorContext(ctx), "schedule");
		return await callHandleAsync<RuntimeScheduledEventInfo | undefined>(
			schedule,
			"get",
			id,
		);
	}

	async actorScheduleList(ctx: ActorContextHandle) {
		const schedule = childHandle(asWasmActorContext(ctx), "schedule");
		return await callHandleAsync<RuntimeScheduledEventInfo[]>(
			schedule,
			"list",
		);
	}

	async actorCronSet(
		ctx: ActorContextHandle,
		name: string,
		expression: string,
		timezone: string | undefined,
		actionName: string,
		args: RuntimeBytes,
		maxHistory: number | undefined,
	) {
		const schedule = childHandle(asWasmActorContext(ctx), "schedule");
		await callHandleAsync(
			schedule,
			"cronSet",
			name,
			expression,
			timezone,
			actionName,
			args,
			maxHistory,
		);
	}

	async actorCronEvery(
		ctx: ActorContextHandle,
		name: string,
		intervalMs: number,
		actionName: string,
		args: RuntimeBytes,
		maxHistory: number | undefined,
	) {
		const schedule = childHandle(asWasmActorContext(ctx), "schedule");
		await callHandleAsync(
			schedule,
			"cronEvery",
			name,
			intervalMs,
			actionName,
			args,
			maxHistory,
		);
	}

	async actorCronGet(ctx: ActorContextHandle, name: string) {
		const schedule = childHandle(asWasmActorContext(ctx), "schedule");
		return await callHandleAsync<RuntimeCronJobInfo | undefined>(
			schedule,
			"cronGet",
			name,
		);
	}

	async actorCronList(ctx: ActorContextHandle) {
		const schedule = childHandle(asWasmActorContext(ctx), "schedule");
		return await callHandleAsync<RuntimeCronJobInfo[]>(
			schedule,
			"cronList",
		);
	}

	async actorCronDelete(ctx: ActorContextHandle, name: string) {
		const schedule = childHandle(asWasmActorContext(ctx), "schedule");
		return await callHandleAsync<boolean>(schedule, "cronDelete", name);
	}

	async actorCronHistory(
		ctx: ActorContextHandle,
		name: string,
		limit: number | undefined,
	) {
		const schedule = childHandle(asWasmActorContext(ctx), "schedule");
		return await callHandleAsync<RuntimeCronFire[]>(
			schedule,
			"cronHistory",
			name,
			limit,
		);
	}

	connId(conn: ConnHandle): string {
		return callHandle(asWasmConn(conn), "id");
	}

	connParams(conn: ConnHandle): RuntimeBytes {
		return toBytes(callHandle(asWasmConn(conn), "params"));
	}

	connState(conn: ConnHandle): RuntimeBytes {
		return toBytes(callHandle(asWasmConn(conn), "state"));
	}

	connSetState(conn: ConnHandle, state: RuntimeBytes): void {
		callHandle(asWasmConn(conn), "setState", state);
	}

	connIsHibernatable(conn: ConnHandle): boolean {
		return callHandle(asWasmConn(conn), "isHibernatable");
	}

	connSend(conn: ConnHandle, name: string, args: RuntimeBytes): void {
		callHandle(asWasmConn(conn), "send", name, args);
	}

	async connDisconnect(
		conn: ConnHandle,
		reason?: string | undefined | null,
	): Promise<void> {
		await callHandleAsync(asWasmConn(conn), "disconnect", reason);
	}

	webSocketSend(
		ws: WebSocketHandle,
		data: RuntimeBytes,
		binary: boolean,
	): void {
		callHandle(asWasmWebSocket(ws), "send", data, binary);
	}

	async webSocketClose(
		ws: WebSocketHandle,
		code?: number | undefined | null,
		reason?: string | undefined | null,
	): Promise<void> {
		await callHandleAsync(asWasmWebSocket(ws), "close", code, reason);
	}

	webSocketSetEventCallback(
		ws: WebSocketHandle,
		callback: (event: RuntimeWebSocketEvent) => void,
	): void {
		callHandle(
			asWasmWebSocket(ws),
			"setEventCallback",
			(event: RuntimeWebSocketEvent) => {
				if (event.kind === "message" && event.binary) {
					callback({
						...event,
						data: toBytes(event.data as RuntimeBytes | Uint8Array),
					});
					return;
				}
				callback(event);
			},
		);
	}
}

export type { WasmBindings };

export async function loadWasmRuntime(config?: WasmRuntimeLoadConfig): Promise<{
	bindings: WasmBindings;
	runtime: WasmCoreRuntime;
}> {
	const bindings =
		config?.bindings ??
		(await import(["@rivetkit", "rivetkit-wasm"].join("/")));
	await bindings.default(config?.initInput);
	return {
		bindings,
		runtime: new WasmCoreRuntime(bindings),
	};
}
