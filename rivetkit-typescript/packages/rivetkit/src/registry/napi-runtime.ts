import type {
	ActorContext as NativeActorContext,
	NapiActorFactory as NativeActorFactory,
	CancellationToken as NativeCancellationToken,
	ConnHandle as NativeConnHandle,
	CoreRegistry as NativeCoreRegistry,
	WebSocket as NativeWebSocket,
} from "@rivetkit/rivetkit-napi";
import type {
	ActorContextHandle,
	ActorFactoryHandle,
	CancellationTokenHandle,
	ConnHandle,
	CoreRuntime,
	RegistryHandle,
	RuntimeActorConfig,
	RuntimeApplicationFetch,
	RuntimeApplicationListenerConfig,
	RuntimeBytes,
	RuntimeCronFire,
	RuntimeCronJobInfo,
	RuntimeHttpRequest,
	RuntimeKvEntry,
	RuntimeKvListOptions,
	RuntimeListenerConfig,
	RuntimeQueueEnqueueAndWaitOptions,
	RuntimeQueueMessage,
	RuntimeQueueNextBatchOptions,
	RuntimeQueueTryNextBatchOptions,
	RuntimeQueueWaitOptions,
	RuntimeRegistryRouteResponse,
	RuntimeRequestSaveOpts,
	RuntimeServeConfig,
	RuntimeScheduledEventInfo,
	RuntimeServerlessRequest,
	RuntimeServerlessResponseHead,
	RuntimeServerlessStreamCallback,
	RuntimeSqlBatchStatement,
	RuntimeSqlBindParam,
	RuntimeSqlBindParams,
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

type NativeBindings = typeof import("@rivetkit/rivetkit-napi");
type NapiSqlDatabase = ReturnType<NativeActorContext["sql"]>;
type NapiSqlBindParams = Parameters<NapiSqlDatabase["execute"]>[1];
type NapiSqlBatchStatement = Parameters<
	NapiSqlDatabase["executeBatch"]
>[0][number];
type NapiSqlTransaction = Awaited<
	ReturnType<NapiSqlDatabase["beginTransaction"]>
>;

function asNativeRegistry(handle: RegistryHandle): NativeCoreRegistry {
	return handle as unknown as NativeCoreRegistry;
}

function asNativeFactory(handle: ActorFactoryHandle): NativeActorFactory {
	return handle as unknown as NativeActorFactory;
}

function asNativeActorContext(handle: ActorContextHandle): NativeActorContext {
	return handle as unknown as NativeActorContext;
}

function asNativeConn(handle: ConnHandle): NativeConnHandle {
	return handle as unknown as NativeConnHandle;
}

function asNativeWebSocket(handle: WebSocketHandle): NativeWebSocket {
	return handle as unknown as NativeWebSocket;
}

function asNativeSqlTransaction(
	handle: SqliteTransactionHandle,
): NapiSqlTransaction {
	return handle as unknown as NapiSqlTransaction;
}

function asNativeCancellationToken(
	handle: CancellationTokenHandle,
): NativeCancellationToken {
	return handle as unknown as NativeCancellationToken;
}

function asRegistryHandle(handle: NativeCoreRegistry): RegistryHandle {
	return handle as unknown as RegistryHandle;
}

function asActorFactoryHandle(handle: NativeActorFactory): ActorFactoryHandle {
	return handle as unknown as ActorFactoryHandle;
}

function toNapiSqlBindParam(
	param: RuntimeSqlBindParam,
): NonNullable<NapiSqlBindParams>[number] {
	switch (param.kind) {
		case "null":
			return { kind: "null" };
		case "int":
			return { kind: "int", intValue: param.intValue };
		case "float":
			return { kind: "float", floatValue: param.floatValue };
		case "text":
			return { kind: "text", textValue: param.textValue };
		case "blob":
			return { kind: "blob", blobValue: Buffer.from(param.blobValue) };
	}
}

function toNapiSqlBindParams(params?: RuntimeSqlBindParams): NapiSqlBindParams {
	if (params == null) {
		return params;
	}
	return params.map((param) => toNapiSqlBindParam(param));
}

function toNapiSqlBatchStatement(
	statement: RuntimeSqlBatchStatement,
): NapiSqlBatchStatement {
	return {
		sql: statement.sql,
		params: toNapiSqlBindParams(statement.params) ?? undefined,
	};
}

function toNapiBuffer(value: RuntimeBytes): Buffer {
	return Buffer.from(value);
}

function toNapiApplication(application: RuntimeApplicationFetch) {
	return async (
		error: unknown,
		request?: {
			method: string;
			url: string;
			headers: Record<string, string>;
			body: Buffer;
		},
	) => {
		if (error) throw error;
		if (!request) {
			throw new Error("application fetch callback received no request");
		}
		const response = await application({
			...request,
			body: new Uint8Array(request.body),
		});
		return {
			...response,
			body: toNapiBuffer(response.body),
		};
	};
}

function toNapiHttpRequest(
	request?: RuntimeHttpRequest | undefined | null,
): Parameters<NativeActorContext["connectConn"]>[1] {
	if (!request) {
		return request;
	}
	return {
		...request,
		body: request.body ? toNapiBuffer(request.body) : undefined,
	};
}

function toNapiStateDeltaPayload(
	payload: RuntimeStateDeltaPayload,
): Parameters<NativeActorContext["saveState"]>[0] {
	return {
		...payload,
		state: payload.state ? toNapiBuffer(payload.state) : undefined,
		connHibernation: payload.connHibernation.map((conn) => ({
			...conn,
			bytes: toNapiBuffer(conn.bytes),
		})),
	};
}

function toNapiKvEntry(entry: RuntimeKvEntry): {
	key: Buffer;
	value: Buffer;
} {
	return {
		key: toNapiBuffer(entry.key),
		value: toNapiBuffer(entry.value),
	};
}

function toNapiQueueMessage(message: RuntimeQueueMessage): RuntimeQueueMessage {
	return {
		id: () => message.id(),
		name: () => message.name(),
		body: () => message.body(),
		createdAt: () => message.createdAt(),
		isCompletable: () => message.isCompletable(),
		complete: async (response?: RuntimeBytes | undefined | null) => {
			await message.complete(
				response === null || response === undefined
					? response
					: toNapiBuffer(response),
			);
		},
	};
}

export class NapiCoreRuntime implements CoreRuntime {
	readonly kind = "napi";

	#bindings: NativeBindings;
	#sql = new WeakMap<NativeActorContext, NapiSqlDatabase>();

	constructor(bindings: NativeBindings) {
		this.#bindings = bindings;
	}

	#actorSql(ctx: ActorContextHandle): NapiSqlDatabase {
		const nativeCtx = asNativeActorContext(ctx);
		let database = this.#sql.get(nativeCtx);
		if (!database) {
			database = nativeCtx.sql();
			this.#sql.set(nativeCtx, database);
		}
		return database;
	}

	createRegistry(): RegistryHandle {
		return asRegistryHandle(new this.#bindings.CoreRegistry());
	}

	registerActor(
		registry: RegistryHandle,
		name: string,
		factory: ActorFactoryHandle,
	): void {
		asNativeRegistry(registry).register(name, asNativeFactory(factory));
	}

	async serveRegistry(
		registry: RegistryHandle,
		config: RuntimeServeConfig,
	): Promise<void> {
		await asNativeRegistry(registry).serve(config);
	}

	async waitRegistryReady(registry: RegistryHandle): Promise<void> {
		await asNativeRegistry(registry).waitReady();
	}

	async shutdownRegistry(registry: RegistryHandle): Promise<void> {
		await asNativeRegistry(registry).shutdown();
	}

	async registryActorStopThresholdMs(
		registry: RegistryHandle,
	): Promise<number | undefined> {
		return (
			(await asNativeRegistry(registry).actorStopThresholdMs()) ??
			undefined
		);
	}

	async registryHealth(
		registry: RegistryHandle,
	): Promise<RuntimeRegistryRouteResponse> {
		const response = await asNativeRegistry(registry).health();
		return {
			status: response.status,
			headers: response.headers,
			body: response.body,
		};
	}

	async registryMetadata(
		registry: RegistryHandle,
	): Promise<RuntimeRegistryRouteResponse> {
		const response = asNativeRegistry(registry).metadata();
		return {
			status: response.status,
			headers: response.headers,
			body: response.body,
		};
	}

	async registryMetrics(
		registry: RegistryHandle,
	): Promise<RuntimeRegistryRouteResponse> {
		const response = asNativeRegistry(registry).metrics();
		return {
			status: response.status,
			headers: response.headers,
			body: response.body,
		};
	}

	async handleServerlessRequest(
		registry: RegistryHandle,
		req: RuntimeServerlessRequest,
		onStreamEvent: RuntimeServerlessStreamCallback,
		cancelToken: CancellationTokenHandle,
		config: RuntimeServeConfig,
	): Promise<RuntimeServerlessResponseHead> {
		return await asNativeRegistry(registry).handleServerlessRequest(
			{ ...req, body: toNapiBuffer(req.body) },
			onStreamEvent,
			asNativeCancellationToken(cancelToken),
			config,
		);
	}

	async serveListener(
		registry: RegistryHandle,
		listener: RuntimeListenerConfig,
		config: RuntimeServeConfig,
	): Promise<void> {
		const application = listener.application
			? toNapiApplication(listener.application)
			: undefined;
		await asNativeRegistry(registry).serveListener(
			{
				port: listener.port,
				host: listener.host,
				publicDir: listener.publicDir,
			},
			config,
			application,
		);
	}

	async serveApplicationListener(
		registry: RegistryHandle,
		listener: RuntimeApplicationListenerConfig,
		config: RuntimeServeConfig,
	): Promise<void> {
		await asNativeRegistry(registry).serveApplicationListener(
			{
				port: listener.port,
				host: listener.host,
				publicDir: listener.publicDir,
			},
			toNapiApplication(listener.application),
			config,
		);
	}

	createActorFactory(
		callbacks: object,
		config?: RuntimeActorConfig | undefined | null,
	): ActorFactoryHandle {
		return asActorFactoryHandle(
			new this.#bindings.NapiActorFactory(callbacks, config),
		);
	}

	createCancellationToken(): CancellationTokenHandle {
		return new this.#bindings.CancellationToken() as unknown as CancellationTokenHandle;
	}

	decodeInspectorRequest(
		bytes: RuntimeBytes,
		advertisedVersion: number,
	): RuntimeBytes {
		return this.#bindings.decodeInspectorRequest(
			toNapiBuffer(bytes),
			advertisedVersion,
		);
	}

	encodeInspectorResponse(
		bytes: RuntimeBytes,
		targetVersion: number,
	): RuntimeBytes {
		return this.#bindings.encodeInspectorResponse(
			toNapiBuffer(bytes),
			targetVersion,
		);
	}

	cancellationTokenAborted(token: CancellationTokenHandle): boolean {
		return asNativeCancellationToken(token).aborted();
	}

	cancelCancellationToken(token: CancellationTokenHandle): void {
		asNativeCancellationToken(token).cancel();
	}

	onCancellationTokenCancelled(
		token: CancellationTokenHandle,
		callback: (...args: unknown[]) => unknown,
	): void {
		asNativeCancellationToken(token).onCancelled(callback);
	}

	actorState(ctx: ActorContextHandle): Buffer {
		return asNativeActorContext(ctx).state();
	}

	actorBeginOnStateChange(ctx: ActorContextHandle): void {
		asNativeActorContext(ctx).beginOnStateChange();
	}

	actorEndOnStateChange(ctx: ActorContextHandle): void {
		asNativeActorContext(ctx).endOnStateChange();
	}

	actorSetAlarm(
		ctx: ActorContextHandle,
		timestampMs?: number | undefined | null,
	): void {
		asNativeActorContext(ctx).setAlarm(timestampMs);
	}

	actorRequestSave(
		ctx: ActorContextHandle,
		opts?: RuntimeRequestSaveOpts | undefined | null,
	): void {
		asNativeActorContext(ctx).requestSave(opts);
	}

	async actorRequestSaveAndWait(
		ctx: ActorContextHandle,
		opts?: RuntimeRequestSaveOpts | undefined | null,
	): Promise<void> {
		await asNativeActorContext(ctx).requestSaveAndWait(opts);
	}

	actorInspectorSnapshot(ctx: ActorContextHandle) {
		return asNativeActorContext(ctx).inspectorSnapshot();
	}

	actorDecodeInspectorRequest(
		ctx: ActorContextHandle,
		bytes: RuntimeBytes,
		advertisedVersion: number,
	): RuntimeBytes {
		return asNativeActorContext(ctx).decodeInspectorRequest(
			toNapiBuffer(bytes),
			advertisedVersion,
		);
	}

	actorEncodeInspectorResponse(
		ctx: ActorContextHandle,
		bytes: RuntimeBytes,
		targetVersion: number,
	): RuntimeBytes {
		return asNativeActorContext(ctx).encodeInspectorResponse(
			toNapiBuffer(bytes),
			targetVersion,
		);
	}

	async actorVerifyInspectorAuth(
		ctx: ActorContextHandle,
		bearerToken?: string | undefined | null,
	): Promise<void> {
		await asNativeActorContext(ctx).verifyInspectorAuth(bearerToken);
	}

	actorQueueHibernationRemoval(
		ctx: ActorContextHandle,
		connId: string,
	): void {
		asNativeActorContext(ctx).queueHibernationRemoval(connId);
	}

	actorTakePendingHibernationChanges(ctx: ActorContextHandle): string[] {
		return asNativeActorContext(ctx).takePendingHibernationChanges();
	}

	actorDirtyHibernatableConns(ctx: ActorContextHandle): ConnHandle[] {
		return asNativeActorContext(
			ctx,
		).dirtyHibernatableConns() as unknown as ConnHandle[];
	}

	async actorSaveState(
		ctx: ActorContextHandle,
		payload: RuntimeStateDeltaPayload,
	): Promise<void> {
		await asNativeActorContext(ctx).saveState(
			toNapiStateDeltaPayload(payload),
		);
	}

	async actorSaveStateAndWorkflowBatch(
		ctx: ActorContextHandle,
		writes: RuntimeWorkflowKvWrite[],
	): Promise<void> {
		await asNativeActorContext(ctx).saveStateAndWorkflowBatch(
			writes.map((write) => ({
				key: toNapiBuffer(write.key),
				value: toNapiBuffer(write.value),
			})),
		);
	}

	actorId(ctx: ActorContextHandle): string {
		return asNativeActorContext(ctx).actorId();
	}

	actorName(ctx: ActorContextHandle): string {
		return asNativeActorContext(ctx).name();
	}

	actorKey(ctx: ActorContextHandle) {
		return asNativeActorContext(ctx).key();
	}

	actorRegion(ctx: ActorContextHandle): string {
		return asNativeActorContext(ctx).region();
	}

	actorSleep(ctx: ActorContextHandle): void {
		asNativeActorContext(ctx).sleep();
	}

	actorDestroy(ctx: ActorContextHandle): void {
		asNativeActorContext(ctx).destroy();
	}

	actorAbortSignal(ctx: ActorContextHandle): AbortSignal {
		return asNativeActorContext(ctx).abortSignal();
	}

	actorConns(ctx: ActorContextHandle): ConnHandle[] {
		return asNativeActorContext(ctx).conns() as unknown as ConnHandle[];
	}

	async actorConnectConn(
		ctx: ActorContextHandle,
		params: RuntimeBytes,
		request?: RuntimeHttpRequest | undefined | null,
	): Promise<ConnHandle> {
		return (await asNativeActorContext(ctx).connectConn(
			toNapiBuffer(params),
			toNapiHttpRequest(request),
		)) as unknown as ConnHandle;
	}

	actorBroadcast(
		ctx: ActorContextHandle,
		name: string,
		args: RuntimeBytes,
	): void {
		asNativeActorContext(ctx).broadcast(name, toNapiBuffer(args));
	}

	actorWaitUntil(ctx: ActorContextHandle, promise: Promise<unknown>): void {
		asNativeActorContext(ctx).waitUntil(promise);
	}

	async actorWaitForTrackedShutdownWork(
		ctx: ActorContextHandle,
	): Promise<boolean> {
		return await asNativeActorContext(ctx).waitForTrackedShutdownWork();
	}

	async actorWaitForTrackedShutdownWorkUnbounded(
		ctx: ActorContextHandle,
	): Promise<void> {
		await asNativeActorContext(ctx).waitForTrackedShutdownWorkUnbounded();
	}

	actorKeepAwake(ctx: ActorContextHandle, promise: Promise<unknown>): void {
		asNativeActorContext(ctx).keepAwake(promise);
	}

	actorBeginKeepAwake(ctx: ActorContextHandle): number {
		return asNativeActorContext(ctx).beginKeepAwake();
	}

	actorEndKeepAwake(ctx: ActorContextHandle, regionId: number): void {
		asNativeActorContext(ctx).endKeepAwake(regionId);
	}

	actorRegisterTask(
		ctx: ActorContextHandle,
		promise: Promise<unknown>,
	): void {
		asNativeActorContext(ctx).registerTask(promise);
	}

	actorRuntimeState(ctx: ActorContextHandle): object {
		return asNativeActorContext(ctx).runtimeState();
	}

	actorClearRuntimeState(ctx: ActorContextHandle): void {
		asNativeActorContext(ctx).clearRuntimeState();
	}

	actorRestartRunHandler(ctx: ActorContextHandle): void {
		asNativeActorContext(ctx).restartRunHandler();
	}

	actorBeginWebsocketCallback(ctx: ActorContextHandle): number {
		return asNativeActorContext(ctx).beginWebsocketCallback();
	}

	actorEndWebsocketCallback(ctx: ActorContextHandle, regionId: number): void {
		asNativeActorContext(ctx).endWebsocketCallback(regionId);
	}

	async actorKvGet(
		ctx: ActorContextHandle,
		key: RuntimeBytes,
	): Promise<RuntimeBytes | null> {
		return await asNativeActorContext(ctx).kv().get(toNapiBuffer(key));
	}

	async actorKvPut(
		ctx: ActorContextHandle,
		key: RuntimeBytes,
		value: RuntimeBytes,
	): Promise<void> {
		await asNativeActorContext(ctx)
			.kv()
			.put(toNapiBuffer(key), toNapiBuffer(value));
	}

	async actorKvDelete(
		ctx: ActorContextHandle,
		key: RuntimeBytes,
	): Promise<void> {
		await asNativeActorContext(ctx).kv().delete(toNapiBuffer(key));
	}

	async actorKvDeleteRange(
		ctx: ActorContextHandle,
		start: RuntimeBytes,
		end: RuntimeBytes,
	): Promise<void> {
		await asNativeActorContext(ctx)
			.kv()
			.deleteRange(toNapiBuffer(start), toNapiBuffer(end));
	}

	async actorKvListPrefix(
		ctx: ActorContextHandle,
		prefix: RuntimeBytes,
		options?: RuntimeKvListOptions | undefined | null,
	): Promise<RuntimeKvEntry[]> {
		return await asNativeActorContext(ctx)
			.kv()
			.listPrefix(toNapiBuffer(prefix), options);
	}

	async actorKvListRange(
		ctx: ActorContextHandle,
		start: RuntimeBytes,
		end: RuntimeBytes,
		options?: RuntimeKvListOptions | undefined | null,
	): Promise<RuntimeKvEntry[]> {
		return await asNativeActorContext(ctx)
			.kv()
			.listRange(toNapiBuffer(start), toNapiBuffer(end), options);
	}

	async actorKvBatchGet(
		ctx: ActorContextHandle,
		keys: RuntimeBytes[],
	): Promise<Array<RuntimeBytes | undefined | null>> {
		return await asNativeActorContext(ctx)
			.kv()
			.batchGet(keys.map(toNapiBuffer));
	}

	async actorKvBatchPut(
		ctx: ActorContextHandle,
		entries: RuntimeKvEntry[],
	): Promise<void> {
		await asNativeActorContext(ctx)
			.kv()
			.batchPut(entries.map(toNapiKvEntry));
	}

	async actorKvBatchDelete(
		ctx: ActorContextHandle,
		keys: RuntimeBytes[],
	): Promise<void> {
		await asNativeActorContext(ctx)
			.kv()
			.batchDelete(keys.map(toNapiBuffer));
	}

	async actorSqlExec(
		ctx: ActorContextHandle,
		sql: string,
	): Promise<RuntimeSqlExecResult> {
		return await this.#actorSql(ctx).exec(sql);
	}

	async actorSqlExecute(
		ctx: ActorContextHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlExecuteResult> {
		const result = await this.#actorSql(ctx).execute(
			sql,
			toNapiSqlBindParams(params),
		);
		return normalizeRuntimeSqlExecuteResult(result);
	}

	async actorSqlExecuteBatch(
		ctx: ActorContextHandle,
		statements: RuntimeSqlBatchStatement[],
	): Promise<RuntimeSqlExecuteResult[]> {
		const results = await this.#actorSql(ctx).executeBatch(
			statements.map(toNapiSqlBatchStatement),
		);
		return results.map(normalizeRuntimeSqlExecuteResult);
	}

	async actorSqlBeginTransaction(
		ctx: ActorContextHandle,
		timeoutMs?: number,
	): Promise<SqliteTransactionHandle> {
		return (await this.#actorSql(ctx).beginTransaction(
			timeoutMs,
		)) as unknown as SqliteTransactionHandle;
	}

	async actorSqlTransactionExec(
		transaction: SqliteTransactionHandle,
		sql: string,
	): Promise<RuntimeSqlExecResult> {
		return await asNativeSqlTransaction(transaction).exec(sql);
	}

	async actorSqlTransactionExecute(
		transaction: SqliteTransactionHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlExecuteResult> {
		const result = await asNativeSqlTransaction(transaction).execute(
			sql,
			toNapiSqlBindParams(params),
		);
		return normalizeRuntimeSqlExecuteResult(result);
	}

	async actorSqlTransactionCommit(
		transaction: SqliteTransactionHandle,
	): Promise<void> {
		await asNativeSqlTransaction(transaction).commit();
	}

	async actorSqlTransactionRollback(
		transaction: SqliteTransactionHandle,
	): Promise<void> {
		await asNativeSqlTransaction(transaction).rollback();
	}

	async actorSqlQuery(
		ctx: ActorContextHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlQueryResult> {
		return await this.#actorSql(ctx).query(
			sql,
			toNapiSqlBindParams(params),
		);
	}

	async actorSqlRun(
		ctx: ActorContextHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlRunResult> {
		return await this.#actorSql(ctx).run(sql, toNapiSqlBindParams(params));
	}

	actorSqlMetrics(ctx: ActorContextHandle) {
		return this.#actorSql(ctx).metrics?.() ?? null;
	}

	actorSqlTakeLastKvError(ctx: ActorContextHandle): string | null {
		return this.#actorSql(ctx).takeLastKvError?.() ?? null;
	}

	async actorSqlClose(ctx: ActorContextHandle): Promise<void> {
		const nativeCtx = asNativeActorContext(ctx);
		const database = this.#sql.get(nativeCtx);
		if (!database) {
			return;
		}

		this.#sql.delete(nativeCtx);
		await database.close();
	}

	async actorRuntimeSocketProvision(ctx: ActorContextHandle) {
		return await asNativeActorContext(ctx).provisionActorRuntimeSocket();
	}

	async actorQueueSend(
		ctx: ActorContextHandle,
		name: string,
		body: RuntimeBytes,
	): Promise<RuntimeQueueMessage> {
		return toNapiQueueMessage(
			await asNativeActorContext(ctx)
				.queue()
				.send(name, toNapiBuffer(body)),
		);
	}

	async actorQueueNextBatch(
		ctx: ActorContextHandle,
		options?: RuntimeQueueNextBatchOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<RuntimeQueueMessage[]> {
		const messages = await asNativeActorContext(ctx)
			.queue()
			.nextBatch(
				options,
				signal ? asNativeCancellationToken(signal) : signal,
			);
		return messages.map(toNapiQueueMessage);
	}

	async actorQueueWaitForNames(
		ctx: ActorContextHandle,
		names: string[],
		options?: RuntimeQueueWaitOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<RuntimeQueueMessage> {
		return toNapiQueueMessage(
			await asNativeActorContext(ctx)
				.queue()
				.waitForNames(
					names,
					options,
					signal ? asNativeCancellationToken(signal) : signal,
				),
		);
	}

	async actorQueueWaitForNamesAvailable(
		ctx: ActorContextHandle,
		names: string[],
		options?: RuntimeQueueWaitOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<void> {
		await asNativeActorContext(ctx)
			.queue()
			.waitForNamesAvailable(
				names,
				options,
				signal ? asNativeCancellationToken(signal) : signal,
			);
	}

	async actorQueueEnqueueAndWait(
		ctx: ActorContextHandle,
		name: string,
		body: RuntimeBytes,
		options?: RuntimeQueueEnqueueAndWaitOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<RuntimeBytes | null> {
		return await asNativeActorContext(ctx)
			.queue()
			.enqueueAndWait(
				name,
				toNapiBuffer(body),
				options,
				signal ? asNativeCancellationToken(signal) : signal,
			);
	}

	actorQueueTryNextBatch(
		ctx: ActorContextHandle,
		options?: RuntimeQueueTryNextBatchOptions | undefined | null,
	): RuntimeQueueMessage[] {
		return asNativeActorContext(ctx)
			.queue()
			.tryNextBatch(options)
			.map(toNapiQueueMessage);
	}

	actorQueueMaxSize(ctx: ActorContextHandle): number {
		return asNativeActorContext(ctx).queue().maxSize();
	}

	async actorQueueInspectMessages(ctx: ActorContextHandle) {
		return await asNativeActorContext(ctx).queue().inspectMessages();
	}

	async actorQueueReset(ctx: ActorContextHandle): Promise<void> {
		await asNativeActorContext(ctx).queue().reset();
	}

	async actorScheduleAfter(
		ctx: ActorContextHandle,
		durationMs: number,
		actionName: string,
		args: RuntimeBytes,
	): Promise<string> {
		return await asNativeActorContext(ctx)
			.schedule()
			.after(durationMs, actionName, toNapiBuffer(args));
	}

	async actorScheduleAt(
		ctx: ActorContextHandle,
		timestampMs: number,
		actionName: string,
		args: RuntimeBytes,
	): Promise<string> {
		return await asNativeActorContext(ctx)
			.schedule()
			.at(timestampMs, actionName, toNapiBuffer(args));
	}

	async actorScheduleCancel(ctx: ActorContextHandle, id: string) {
		return await asNativeActorContext(ctx).schedule().cancel(id);
	}

	async actorScheduleGet(
		ctx: ActorContextHandle,
		id: string,
	): Promise<RuntimeScheduledEventInfo | undefined> {
		return (
			(await asNativeActorContext(ctx).schedule().get(id)) ?? undefined
		);
	}

	async actorScheduleList(ctx: ActorContextHandle) {
		return await asNativeActorContext(ctx).schedule().list();
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
		await asNativeActorContext(ctx)
			.schedule()
			.cronSet(
				name,
				expression,
				timezone,
				actionName,
				toNapiBuffer(args),
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
		await asNativeActorContext(ctx)
			.schedule()
			.cronEvery(
				name,
				intervalMs,
				actionName,
				toNapiBuffer(args),
				maxHistory,
			);
	}

	async actorCronGet(
		ctx: ActorContextHandle,
		name: string,
	): Promise<RuntimeCronJobInfo | undefined> {
		return ((await asNativeActorContext(ctx).schedule().cronGet(name)) ??
			undefined) as RuntimeCronJobInfo | undefined;
	}

	async actorCronList(
		ctx: ActorContextHandle,
	): Promise<RuntimeCronJobInfo[]> {
		return (await asNativeActorContext(ctx)
			.schedule()
			.cronList()) as RuntimeCronJobInfo[];
	}

	async actorCronDelete(ctx: ActorContextHandle, name: string) {
		return await asNativeActorContext(ctx).schedule().cronDelete(name);
	}

	async actorCronHistory(
		ctx: ActorContextHandle,
		name: string,
		limit: number | undefined,
	): Promise<RuntimeCronFire[]> {
		return (await asNativeActorContext(ctx)
			.schedule()
			.cronHistory(name, limit)) as RuntimeCronFire[];
	}

	connId(conn: ConnHandle): string {
		return asNativeConn(conn).id();
	}

	connParams(conn: ConnHandle): RuntimeBytes {
		return asNativeConn(conn).params();
	}

	connState(conn: ConnHandle): RuntimeBytes {
		return asNativeConn(conn).state();
	}

	connSetState(conn: ConnHandle, state: RuntimeBytes): void {
		asNativeConn(conn).setState(toNapiBuffer(state));
	}

	connIsHibernatable(conn: ConnHandle): boolean {
		return asNativeConn(conn).isHibernatable();
	}

	connSend(conn: ConnHandle, name: string, args: RuntimeBytes): void {
		asNativeConn(conn).send(name, toNapiBuffer(args));
	}

	async connDisconnect(
		conn: ConnHandle,
		reason?: string | undefined | null,
	): Promise<void> {
		await asNativeConn(conn).disconnect(reason);
	}

	webSocketSend(
		ws: WebSocketHandle,
		data: RuntimeBytes,
		binary: boolean,
	): void {
		asNativeWebSocket(ws).send(toNapiBuffer(data), binary);
	}

	async webSocketClose(
		ws: WebSocketHandle,
		code?: number | undefined | null,
		reason?: string | undefined | null,
	): Promise<void> {
		await asNativeWebSocket(ws).close(code, reason);
	}

	webSocketSetEventCallback(
		ws: WebSocketHandle,
		callback: (event: RuntimeWebSocketEvent) => void,
	): void {
		asNativeWebSocket(ws).setEventCallback(callback);
	}
}

export type NapiBindings = NativeBindings;

export async function loadNapiRuntime(): Promise<{
	bindings: NapiBindings;
	runtime: NapiCoreRuntime;
}> {
	// LOAD-BEARING: the specifier is computed (`.join("/")`) on purpose. Do NOT
	// "simplify" it to a literal `import("@rivetkit/rivetkit-napi")`. Edge
	// adapters (e.g. @rivetkit/supabase) pre-bundle this module for Deno; a
	// literal dynamic import is statically resolvable, so Deno's eszip bundler
	// would snapshot the native `.node` addon into the deploy and 413. The
	// computed specifier keeps it opaque to static analysis so it is never
	// bundled. Enforced by scripts/ci/check-edge-native-closure.mjs.
	const bindings = await import(["@rivetkit", "rivetkit-napi"].join("/"));
	return {
		bindings,
		runtime: new NapiCoreRuntime(bindings),
	};
}
