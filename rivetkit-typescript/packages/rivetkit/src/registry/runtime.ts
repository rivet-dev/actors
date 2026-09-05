import type {
	ActorInvocationSpanContext,
	ActorInvocationTraceContext,
} from "@/common/actor-telemetry-context";
import type {
	SqliteNativeMetrics,
	SqliteProfilingOptions,
} from "@/common/database/config";
import { stringifyError } from "@/common/utils";
import type { RegistryConfig } from "./config";
import { logger } from "./log";

declare const handleBrand: unique symbol;

type OpaqueHandle<Name extends string> = {
	readonly [handleBrand]: Name;
};

export type RegistryHandle = OpaqueHandle<"registry">;
export type ActorFactoryHandle = OpaqueHandle<"actorFactory">;
export type ActorContextHandle = OpaqueHandle<"actorContext">;
export type ConnHandle = OpaqueHandle<"conn">;
export type WebSocketHandle = OpaqueHandle<"webSocket">;
export type CancellationTokenHandle = OpaqueHandle<"cancellationToken">;
export type ActorStateTransactionHandle = OpaqueHandle<"actorStateTransaction">;
export type SqliteTransactionHandle = OpaqueHandle<"sqliteTransaction">;

export type RuntimeBytes = Uint8Array;

export interface RuntimeActorKeySegment {
	kind: string;
	stringValue?: string;
	numberValue?: number;
}

/** Active actor invocation correlation available to runtime-owned clients and logs. */
export type RuntimeActorInvocationTraceContext = ActorInvocationTraceContext;
export type RuntimeActorInvocationSpanContext = ActorInvocationSpanContext;

/** Resolves correlation at operation time so retained clients cannot freeze stale context. */
export type CurrentActorInvocation = () =>
	| RuntimeActorInvocationTraceContext
	| undefined;

export interface RuntimeHttpRequest {
	method: string;
	uri: string;
	headers?: Record<string, string>;
	body?: RuntimeBytes;
}

export interface RuntimeHttpResponse {
	status?: number;
	headers?: Record<string, string>;
	body?: RuntimeBytes;
	stream?: boolean;
}

export interface RuntimeHttpResponseBodyStream {
	cancelled(): Promise<void>;
	write(chunk: Uint8Array): Promise<void>;
	end(): Promise<void>;
	error(message: string): Promise<void>;
}

export interface RuntimeStateDeltaPayload {
	state?: RuntimeBytes;
	connHibernation: Array<{
		connId: string;
		bytes: RuntimeBytes;
	}>;
	connHibernationRemoved: string[];
}

export interface RuntimeWorkflowKvWrite {
	key: RuntimeBytes;
	value: RuntimeBytes;
}

export interface RuntimeRequestSaveOpts {
	immediate?: boolean;
	maxWaitMs?: number;
}

export interface RuntimeInspectorSnapshot {
	stateRevision: number;
	connectionsRevision: number;
	queueRevision: number;
	activeConnections: number;
	queueSize: number;
	connectedClients: number;
}

export interface RuntimeQueueMessage {
	id(): bigint;
	name(): string;
	body(): RuntimeBytes;
	createdAt(): number;
	isCompletable(): boolean;
	complete(response?: RuntimeBytes | undefined | null): Promise<void>;
}

export interface RuntimeQueueInspectMessage {
	id: number;
	name: string;
	createdAtMs: number;
}

export interface RuntimeQueueSendResult {
	status: string;
	response?: RuntimeBytes;
}

export interface RuntimeScheduledEventInfo {
	id: string;
	action: string;
	args: RuntimeBytes;
	runAt: number;
}

export interface RuntimeCronJobInfo {
	name: string;
	kind: "cron" | "every";
	action: string;
	args: RuntimeBytes;
	nextRunAt: number;
	lastRunAt?: number;
	expression?: string;
	timezone?: string;
	intervalMs?: number;
	maxHistory: number;
}

export interface RuntimeScheduleErrorInfo {
	group: string;
	code: string;
	message: string;
	metadata?: unknown;
}

export interface RuntimeCronFire {
	action: string;
	scheduledAt: number;
	firedAt: number;
	finishedAt?: number;
	result: "running" | "ok" | "error" | "skipped";
	error?: RuntimeScheduleErrorInfo;
}

export interface RuntimeScheduledFireInfo {
	kind: "at" | "cron" | "every";
	id: string;
	name?: string;
	scheduledAt: number;
	firedAt: number;
}

export interface RuntimeQueueNextBatchOptions {
	names?: string[];
	count?: number;
	timeoutMs?: number;
	completable?: boolean;
}

export interface RuntimeQueueWaitOptions {
	timeoutMs?: number;
	completable?: boolean;
}

export interface RuntimeQueueEnqueueAndWaitOptions {
	timeoutMs?: number;
}

export interface RuntimeQueueTryNextBatchOptions {
	names?: string[];
	count?: number;
	completable?: boolean;
}

export interface RuntimeKvListOptions {
	reverse?: boolean;
	limit?: number;
}

export interface RuntimeKvEntry {
	key: RuntimeBytes;
	value: RuntimeBytes;
}

type RuntimeSqlBindNoValues = {
	intValue?: never;
	floatValue?: never;
	textValue?: never;
	blobValue?: never;
};

export type RuntimeSqlBindParam =
	| ({ kind: "null" } & RuntimeSqlBindNoValues)
	| {
			kind: "int";
			intValue: number;
			floatValue?: never;
			textValue?: never;
			blobValue?: never;
	  }
	| {
			kind: "float";
			intValue?: never;
			floatValue: number;
			textValue?: never;
			blobValue?: never;
	  }
	| {
			kind: "text";
			intValue?: never;
			floatValue?: never;
			textValue: string;
			blobValue?: never;
	  }
	| {
			kind: "blob";
			intValue?: never;
			floatValue?: never;
			textValue?: never;
			blobValue: RuntimeBytes;
	  };

export type RuntimeSqlBindParams = RuntimeSqlBindParam[] | null;

export interface RuntimeActorRuntimeSocketEndpointInfo {
	path: string;
}

export interface RuntimeSqlQueryResult {
	columns: string[];
	rows: unknown[][];
}

export type RuntimeSqlExecResult = RuntimeSqlQueryResult;

export interface RuntimeSqlExecuteResult extends RuntimeSqlQueryResult {
	changes: number;
	lastInsertRowId?: number | null;
}

export interface RuntimeSqlBatchStatement {
	sql: string;
	params?: RuntimeSqlBindParams;
}

export function normalizeRuntimeSqlExecuteResult(
	result: RuntimeSqlQueryResult & {
		changes: number;
		lastInsertRowId?: number | null;
	},
): RuntimeSqlExecuteResult {
	return result;
}

export interface RuntimeSqlRunResult {
	changes: number;
}

export interface RuntimeSqlDatabase {
	exec(sql: string): Promise<RuntimeSqlExecResult>;
	execute(
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlExecuteResult>;
	executeBatch(
		statements: RuntimeSqlBatchStatement[],
	): Promise<RuntimeSqlExecuteResult[]>;
	query(
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlQueryResult>;
	run(
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlRunResult>;
	beginTransaction(
		timeoutMs?: number,
		name?: string,
	): Promise<RuntimeSqlTransactionDatabase>;
	metrics?(): SqliteNativeMetrics | null;
	takeLastKvError?(): string | null;
	close(): Promise<void>;
}

export interface RuntimeSqlTransactionDatabase {
	exec(sql: string): Promise<RuntimeSqlExecResult>;
	execute(
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlExecuteResult>;
	commit(): Promise<void>;
	rollback(): Promise<void>;
}

export interface RuntimeActorConfig {
	name?: string;
	icon?: string;
	hasDatabase?: boolean;
	remoteSqlite?: boolean;
	sqliteProfiling?: SqliteProfilingOptions;
	enableActorRuntimeSocket?: boolean;
	hasState?: boolean;
	canHibernateWebsocket?: boolean;
	stateSaveIntervalMs?: number;
	createStateTimeoutMs?: number;
	onCreateTimeoutMs?: number;
	createVarsTimeoutMs?: number;
	createConnStateTimeoutMs?: number;
	onBeforeConnectTimeoutMs?: number;
	onConnectTimeoutMs?: number;
	onMigrateTimeoutMs?: number;
	onWakeTimeoutMs?: number;
	onBeforeActorStartTimeoutMs?: number;
	actionTimeoutMs?: number;
	onRequestTimeoutMs?: number;
	sleepTimeoutMs?: number;
	noSleep?: boolean;
	sleepGracePeriodMs?: number;
	connectionLivenessTimeoutMs?: number;
	connectionLivenessIntervalMs?: number;
	maxQueueSize?: number;
	maxSchedules?: number;
	maxQueueMessageSize?: number;
	maxIncomingMessageSize?: number;
	maxOutgoingMessageSize?: number;
	preloadMaxWorkflowBytes?: number;
	preloadMaxConnectionsBytes?: number;
	actions?: Array<{ name: string }>;
	inspectorTabs?: Array<RuntimeInspectorTabEntry>;
}

export interface RuntimeInspectorTabEntry {
	id: string;
	/** Required for custom entries; omitted for built-in hides. */
	label?: string;
	/**
	 * Required for custom entries — absolute path to the source directory.
	 * Resolved on the TS side before being handed to the runtime.
	 */
	source?: string;
	/** Optional icon id for custom entries. */
	icon?: string;
	/** Set to true for built-in hide entries. */
	hidden?: boolean;
}

export interface RuntimeServeConfig {
	version: number;
	endpoint: string;
	token?: string;
	namespace: string;
	poolName: string;
	engineBinaryPath?: string;
	startServices?: boolean;
	servicesBinaryPath?: string;
	engineHost?: string;
	enginePort?: number;
	handleInspectorHttpInRuntime?: boolean;
	inspectorTestToken?: string;
	serverlessBasePath?: string;
	serverlessPackageVersion: string;
	serverlessClientEndpoint?: string;
	serverlessClientNamespace?: string;
	serverlessClientToken?: string;
	serverlessValidateEndpoint: boolean;
	serverlessMaxStartPayloadBytes: number;
}

export interface RuntimeListenerConfig {
	port: number;
	host?: string;
	publicDir?: string;
	application?: RuntimeApplicationFetch;
}

export interface RuntimeServerlessRequest {
	method: string;
	url: string;
	headers: Record<string, string>;
	body: RuntimeBytes;
}

export interface RuntimeApplicationRequest extends RuntimeServerlessRequest {
	cancelToken?: CancellationTokenHandle;
}

export interface RuntimeApplicationResponse {
	status: number;
	headers: Record<string, string>;
	body?: RuntimeBytes;
	stream?: boolean;
}

export type RuntimeApplicationFetch = (
	request: RuntimeApplicationRequest,
	responseBodyStream?: RuntimeHttpResponseBodyStream,
) => Promise<RuntimeApplicationResponse>;

export interface RuntimeApplicationListenerConfig
	extends RuntimeListenerConfig {
	application: RuntimeApplicationFetch;
}

export interface RuntimeServerlessResponseHead {
	status: number;
	headers: Record<string, string>;
}

export interface RuntimeRegistryRouteResponse {
	status: number;
	headers: Record<string, string>;
	body: RuntimeBytes;
}

export type RuntimeServerlessStreamEvent =
	| {
			kind: "chunk";
			chunk?: RuntimeBytes;
	  }
	| {
			kind: "end";
			error?: {
				group: string;
				code: string;
				message: string;
			};
	  };

export type RuntimeServerlessStreamCallback = (
	error: unknown,
	event?: RuntimeServerlessStreamEvent,
) => unknown;

export type RuntimeWebSocketEvent =
	| {
			kind: "message";
			data: string | RuntimeBytes;
			binary: boolean;
			messageIndex?: number;
	  }
	| {
			kind: "close";
			code: number;
			reason: string;
			wasClean: boolean;
	  };

export interface CoreRuntime {
	readonly kind: "napi" | "wasm";

	createRegistry(): RegistryHandle;
	registerActor(
		registry: RegistryHandle,
		name: string,
		factory: ActorFactoryHandle,
	): void;
	serveRegistry(
		registry: RegistryHandle,
		config: RuntimeServeConfig,
	): Promise<void>;
	waitRegistryReady(registry: RegistryHandle): Promise<void>;
	shutdownRegistry(registry: RegistryHandle): Promise<void>;
	registryActorStopThresholdMs?(
		registry: RegistryHandle,
	): Promise<number | undefined>;
	handleServerlessRequest(
		registry: RegistryHandle,
		req: RuntimeServerlessRequest,
		onStreamEvent: RuntimeServerlessStreamCallback,
		cancelToken: CancellationTokenHandle,
		config: RuntimeServeConfig,
	): Promise<RuntimeServerlessResponseHead>;
	serveListener(
		registry: RegistryHandle,
		listener: RuntimeListenerConfig,
		config: RuntimeServeConfig,
	): Promise<void>;
	serveApplicationListener(
		registry: RegistryHandle,
		listener: RuntimeApplicationListenerConfig,
		config: RuntimeServeConfig,
	): Promise<void>;
	registryHealth?(
		registry: RegistryHandle,
	): Promise<RuntimeRegistryRouteResponse>;
	registryMetadata?(
		registry: RegistryHandle,
	): Promise<RuntimeRegistryRouteResponse>;
	registryMetrics?(
		registry: RegistryHandle,
	): Promise<RuntimeRegistryRouteResponse>;
	createActorFactory(
		callbacks: object,
		config?: RuntimeActorConfig | undefined | null,
	): ActorFactoryHandle;

	createCancellationToken(): CancellationTokenHandle;
	cancellationTokenAborted(token: CancellationTokenHandle): boolean;
	cancelCancellationToken(token: CancellationTokenHandle): void;
	onCancellationTokenCancelled(
		token: CancellationTokenHandle,
		callback: (...args: unknown[]) => unknown,
	): void;

	actorState(ctx: ActorContextHandle): RuntimeBytes;
	actorBeginOnStateChange(ctx: ActorContextHandle): void;
	actorEndOnStateChange(ctx: ActorContextHandle): void;
	actorSetAlarm(
		ctx: ActorContextHandle,
		timestampMs?: number | undefined | null,
	): void;
	actorSetRunWakeAt(
		ctx: ActorContextHandle,
		timestampMs?: number | undefined | null,
	): Promise<void>;
	actorRequestSave(
		ctx: ActorContextHandle,
		opts?: RuntimeRequestSaveOpts | undefined | null,
	): void;
	actorRequestSaveAndWait(
		ctx: ActorContextHandle,
		opts?: RuntimeRequestSaveOpts | undefined | null,
	): Promise<void>;
	actorInspectorSnapshot(ctx: ActorContextHandle): RuntimeInspectorSnapshot;
	actorDecodeInspectorRequest(
		ctx: ActorContextHandle,
		bytes: RuntimeBytes,
		advertisedVersion: number,
	): RuntimeBytes;
	actorEncodeInspectorResponse(
		ctx: ActorContextHandle,
		bytes: RuntimeBytes,
		targetVersion: number,
	): RuntimeBytes;
	actorVerifyInspectorAuth(
		ctx: ActorContextHandle,
		bearerToken?: string | undefined | null,
	): Promise<void>;
	actorQueueHibernationRemoval(ctx: ActorContextHandle, connId: string): void;
	actorTakePendingHibernationChanges(ctx: ActorContextHandle): string[];
	actorDirtyHibernatableConns(ctx: ActorContextHandle): ConnHandle[];
	actorSaveState(
		ctx: ActorContextHandle,
		payload: RuntimeStateDeltaPayload,
	): Promise<void>;
	actorSaveStateAndWorkflowBatch(
		ctx: ActorContextHandle,
		writes: RuntimeWorkflowKvWrite[],
	): Promise<void>;
	actorId(ctx: ActorContextHandle): string;
	/**
	 * Runs one actor callback with `ctx` as the current invocation: operations
	 * on retained handles for the same actor resolve to it, and its Core span
	 * is the active OpenTelemetry span for the duration of `run`.
	 */
	runWithActorInvocationContext<T>(ctx: ActorContextHandle, run: () => T): T;
	/**
	 * Correlation of the invocation currently executing for this actor, or
	 * `undefined` outside an invocation or after it finished. A sampled-out
	 * invocation can still expose valid span context for propagation.
	 */
	actorInvocationTraceContext(
		ctx: ActorContextHandle,
	): RuntimeActorInvocationTraceContext | undefined;
	actorName(ctx: ActorContextHandle): string;
	actorKey(ctx: ActorContextHandle): RuntimeActorKeySegment[];
	actorRegion(ctx: ActorContextHandle): string;
	actorSleep(ctx: ActorContextHandle): void;
	actorDestroy(ctx: ActorContextHandle): void;
	actorAbortSignal(ctx: ActorContextHandle): AbortSignal;
	actorConns(ctx: ActorContextHandle): ConnHandle[];
	actorConnectConn(
		ctx: ActorContextHandle,
		params: RuntimeBytes,
		request?: RuntimeHttpRequest | undefined | null,
	): Promise<ConnHandle>;
	actorBroadcast(
		ctx: ActorContextHandle,
		name: string,
		args: RuntimeBytes,
	): void;
	actorWaitUntil(ctx: ActorContextHandle, promise: Promise<unknown>): void;
	actorWaitForTrackedShutdownWork(ctx: ActorContextHandle): Promise<boolean>;
	actorWaitForTrackedShutdownWorkUnbounded(
		ctx: ActorContextHandle,
	): Promise<void>;
	actorKeepAwake(ctx: ActorContextHandle, promise: Promise<unknown>): void;
	actorBeginKeepAwake(ctx: ActorContextHandle): number;
	actorEndKeepAwake(ctx: ActorContextHandle, regionId: number): void;
	actorRegisterTask(ctx: ActorContextHandle, promise: Promise<unknown>): void;
	actorRuntimeState(ctx: ActorContextHandle): object;
	actorClearRuntimeState(ctx: ActorContextHandle): void;
	actorRestartRunHandler(ctx: ActorContextHandle): void;
	actorBeginWebsocketCallback(ctx: ActorContextHandle): number;
	actorEndWebsocketCallback(ctx: ActorContextHandle, regionId: number): void;

	actorKvGet(
		ctx: ActorContextHandle,
		key: RuntimeBytes,
	): Promise<RuntimeBytes | null>;
	actorKvPut(
		ctx: ActorContextHandle,
		key: RuntimeBytes,
		value: RuntimeBytes,
	): Promise<void>;
	actorKvDelete(ctx: ActorContextHandle, key: RuntimeBytes): Promise<void>;
	actorKvDeleteRange(
		ctx: ActorContextHandle,
		start: RuntimeBytes,
		end: RuntimeBytes,
	): Promise<void>;
	actorKvListPrefix(
		ctx: ActorContextHandle,
		prefix: RuntimeBytes,
		options?: RuntimeKvListOptions | undefined | null,
	): Promise<RuntimeKvEntry[]>;
	actorKvListRange(
		ctx: ActorContextHandle,
		start: RuntimeBytes,
		end: RuntimeBytes,
		options?: RuntimeKvListOptions | undefined | null,
	): Promise<RuntimeKvEntry[]>;
	actorKvBatchGet(
		ctx: ActorContextHandle,
		keys: RuntimeBytes[],
	): Promise<Array<RuntimeBytes | undefined | null>>;
	actorKvBatchPut(
		ctx: ActorContextHandle,
		entries: RuntimeKvEntry[],
	): Promise<void>;
	actorKvBatchDelete(
		ctx: ActorContextHandle,
		keys: RuntimeBytes[],
	): Promise<void>;

	actorSqlExec(
		ctx: ActorContextHandle,
		sql: string,
	): Promise<RuntimeSqlExecResult>;
	actorSqlExecute(
		ctx: ActorContextHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlExecuteResult>;
	actorSqlExecuteBatch(
		ctx: ActorContextHandle,
		statements: RuntimeSqlBatchStatement[],
	): Promise<RuntimeSqlExecuteResult[]>;
	actorSqlBeginTransaction(
		ctx: ActorContextHandle,
		timeoutMs?: number,
		name?: string,
	): Promise<SqliteTransactionHandle>;
	actorSqlTransactionExec(
		transaction: SqliteTransactionHandle,
		sql: string,
	): Promise<RuntimeSqlExecResult>;
	actorSqlTransactionExecute(
		transaction: SqliteTransactionHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlExecuteResult>;
	actorSqlTransactionCommit(
		transaction: SqliteTransactionHandle,
	): Promise<void>;
	actorSqlTransactionRollback(
		transaction: SqliteTransactionHandle,
	): Promise<void>;
	actorBeginStateTransaction(
		ctx: ActorContextHandle,
		timeoutMs?: number,
	): Promise<ActorStateTransactionHandle>;
	actorStateTransactionExecute(
		transaction: ActorStateTransactionHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlExecuteResult>;
	actorStateTransactionCommit(
		transaction: ActorStateTransactionHandle,
		payload: RuntimeStateDeltaPayload,
	): Promise<void>;
	actorStateTransactionRollback(
		transaction: ActorStateTransactionHandle,
	): Promise<void>;
	actorSqlQuery(
		ctx: ActorContextHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlQueryResult>;
	actorSqlRun(
		ctx: ActorContextHandle,
		sql: string,
		params?: RuntimeSqlBindParams,
	): Promise<RuntimeSqlRunResult>;
	actorSqlMetrics(ctx: ActorContextHandle): SqliteNativeMetrics | null;
	actorSqlTakeLastKvError(ctx: ActorContextHandle): string | null;
	actorSqlClose(ctx: ActorContextHandle): Promise<void>;
	actorRuntimeSocketProvision(
		ctx: ActorContextHandle,
	): Promise<RuntimeActorRuntimeSocketEndpointInfo>;

	actorQueueSend(
		ctx: ActorContextHandle,
		name: string,
		body: RuntimeBytes,
	): Promise<RuntimeQueueMessage>;
	actorQueueNextBatch(
		ctx: ActorContextHandle,
		options?: RuntimeQueueNextBatchOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<RuntimeQueueMessage[]>;
	actorQueueWaitForNames(
		ctx: ActorContextHandle,
		names: string[],
		options?: RuntimeQueueWaitOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<RuntimeQueueMessage>;
	actorQueueWaitForNamesAvailable(
		ctx: ActorContextHandle,
		names: string[],
		options?: RuntimeQueueWaitOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<void>;
	actorQueueCompletePersisted(
		ctx: ActorContextHandle,
		messageId: bigint,
		expectedName: string,
		response?: RuntimeBytes | undefined | null,
	): Promise<boolean>;
	actorQueueEnqueueAndWait(
		ctx: ActorContextHandle,
		name: string,
		body: RuntimeBytes,
		options?: RuntimeQueueEnqueueAndWaitOptions | undefined | null,
		signal?: CancellationTokenHandle | undefined | null,
	): Promise<RuntimeBytes | null>;
	actorQueueTryNextBatch(
		ctx: ActorContextHandle,
		options?: RuntimeQueueTryNextBatchOptions | undefined | null,
	): RuntimeQueueMessage[];
	actorQueueMaxSize(ctx: ActorContextHandle): number;
	actorQueueInspectMessages(
		ctx: ActorContextHandle,
	): Promise<RuntimeQueueInspectMessage[]>;
	actorQueueReset(ctx: ActorContextHandle): Promise<void>;

	actorScheduleAfter(
		ctx: ActorContextHandle,
		durationMs: number,
		actionName: string,
		args: RuntimeBytes,
	): Promise<string>;
	actorScheduleAt(
		ctx: ActorContextHandle,
		timestampMs: number,
		actionName: string,
		args: RuntimeBytes,
	): Promise<string>;
	actorScheduleCancel(ctx: ActorContextHandle, id: string): Promise<boolean>;
	actorScheduleGet(
		ctx: ActorContextHandle,
		id: string,
	): Promise<RuntimeScheduledEventInfo | undefined>;
	actorScheduleList(
		ctx: ActorContextHandle,
	): Promise<RuntimeScheduledEventInfo[]>;
	actorCronSet(
		ctx: ActorContextHandle,
		name: string,
		expression: string,
		timezone: string | undefined,
		actionName: string,
		args: RuntimeBytes,
		maxHistory: number | undefined,
	): Promise<void>;
	actorCronEvery(
		ctx: ActorContextHandle,
		name: string,
		intervalMs: number,
		actionName: string,
		args: RuntimeBytes,
		maxHistory: number | undefined,
	): Promise<void>;
	actorCronGet(
		ctx: ActorContextHandle,
		name: string,
	): Promise<RuntimeCronJobInfo | undefined>;
	actorCronList(ctx: ActorContextHandle): Promise<RuntimeCronJobInfo[]>;
	actorCronDelete(ctx: ActorContextHandle, name: string): Promise<boolean>;
	actorCronHistory(
		ctx: ActorContextHandle,
		name: string,
		limit: number | undefined,
	): Promise<RuntimeCronFire[]>;

	connId(conn: ConnHandle): string;
	connParams(conn: ConnHandle): RuntimeBytes;
	connState(conn: ConnHandle): RuntimeBytes;
	connSetState(conn: ConnHandle, state: RuntimeBytes): void;
	connIsHibernatable(conn: ConnHandle): boolean;
	connSend(conn: ConnHandle, name: string, args: RuntimeBytes): void;
	connDisconnect(
		conn: ConnHandle,
		reason?: string | undefined | null,
	): Promise<void>;

	webSocketSend(
		ws: WebSocketHandle,
		data: RuntimeBytes,
		binary: boolean,
	): void;
	webSocketClose(
		ws: WebSocketHandle,
		code?: number | undefined | null,
		reason?: string | undefined | null,
	): Promise<void>;
	webSocketSetEventCallback(
		ws: WebSocketHandle,
		callback: (event: RuntimeWebSocketEvent) => void,
	): void;
}

export interface RuntimeBundle {
	runtime: CoreRuntime;
}

export async function buildServeConfig(
	config: RegistryConfig,
	loadEnginePath: () => Promise<string>,
	version: string,
	loadServicesPath: () => Promise<string>,
): Promise<RuntimeServeConfig> {
	if (!config.endpoint) {
		throw new Error("registry endpoint is required");
	}

	const serveConfig: RuntimeServeConfig = {
		version: config.envoy.version,
		endpoint: config.endpoint,
		token: config.token,
		namespace: config.namespace,
		poolName: config.envoy.poolName,
		startServices: config.startServices,
		handleInspectorHttpInRuntime: true,
		serverlessBasePath: config.serverless.basePath,
		serverlessPackageVersion: version,
		serverlessClientEndpoint: config.publicEndpoint,
		serverlessClientNamespace: config.publicNamespace,
		serverlessClientToken: config.publicToken,
		serverlessValidateEndpoint: config.validateServerlessEndpoint,
		serverlessMaxStartPayloadBytes: config.serverless.maxStartPayloadBytes,
	};

	// Always best-effort resolve the engine binary path and hand it to the core.
	// The core alone decides whether to actually spawn a local engine, so JS must
	// not duplicate that decision here. `loadEnginePath` throws when no binary is
	// available (remote-only install, unsupported platform, optional deps
	// skipped); in that case leave it unset and let the core report
	// `engine.binary_unavailable` only if it actually needs one.
	try {
		serveConfig.engineBinaryPath = await loadEnginePath();
	} catch (error) {
		// The engine binary could not be resolved. The core still decides whether
		// it needs to spawn a local engine; if it does, it will fail with
		// engine.binary_unavailable (auto-download is off in the napi runtime).
		logger().warn({
			msg: "could not resolve a local engine binary; if a local engine must be spawned it will fail with engine.binary_unavailable — set RIVET_ENGINE_BINARY_PATH or install the @rivetkit/engine-cli platform package",
			error: stringifyError(error),
		});
	}
	if (config.startServices) {
		try {
			serveConfig.servicesBinaryPath = await loadServicesPath();
		} catch (error) {
			logger().warn({
				msg: "could not resolve the Services binary; a local Engine will fail with services.binary_unavailable — set RIVET_RUN_SERVICES=0 to disable Services or RIVET_SERVICES_BINARY to a local rivet-services binary",
				error: stringifyError(error),
			});
		}
	}
	serveConfig.engineHost = config.engineHost;
	serveConfig.enginePort = config.enginePort;
	if (config.test?.enabled) {
		serveConfig.inspectorTestToken =
			process.env._RIVET_TEST_INSPECTOR_TOKEN ?? "token";
	}

	return serveConfig;
}
