import { VirtualWebSocket } from "@rivetkit/virtual-websocket";
import {
	flattenActionHandlers,
	flattenActionInputSchemas,
} from "@/actor/actions";
import {
	ACTOR_CONTEXT_INTERNAL_SYMBOL,
	type ActorCron,
	type ActorCronEveryOptions,
	type ActorCronSetOptions,
	type ActorLogger,
	type ActorSchedule,
	CONN_STATE_MANAGER_SYMBOL,
	type CronFire,
	type CronJobInfo,
	disposeRunInspector,
	getRunFunction,
	getRunInspectorConfig,
	getRunInspectorKind,
	hasRunInspectorConfig,
	RAW_STATE_SYMBOL,
	type ScheduledEventInfo,
	type WorkflowInspectorConfig,
} from "@/actor/config";
import type { AnyActorDefinition } from "@/actor/definition";
import {
	decodeBridgeRivetError,
	encodeBridgeRivetError,
	forbiddenError,
	INTERNAL_ERROR_CODE,
	isActorAbortedError,
	isRivetErrorLike,
	RivetError,
	type RivetErrorLike,
	toRivetError,
} from "@/actor/errors";
import { makePrefixedKey, removePrefixFromKey } from "@/actor/keys";
import {
	getEventCanSubscribe,
	getQueueCanPublish,
	hasSchemaConfigKey,
} from "@/actor/schema";
import {
	type AnyClient,
	type Client,
	createClientWithDriver,
} from "@/client/client";
import { convertRegistryConfigToClientConfig } from "@/client/config";
import { HEADER_CONN_PARAMS } from "@/common/actor-router-consts";
import type {
	AnyDatabaseProvider,
	SqliteProfilingOptions,
} from "@/common/database/config";
import {
	bindNativeStateTransactionContext,
	db as defaultDb,
	registerNativeStateTransactionOpener,
} from "@/common/database/mod";
import { wrapJsNativeDatabase } from "@/common/database/native-database";
import { assertJsonCompatValue, type JsonCompatValue } from "@/common/encoding";
import { isResponseLike } from "@/common/fetch-like";
import {
	decodeWorkflowHistoryTransport,
	type WorkflowHistoryBytes,
} from "@/common/inspector-transport";
import { deconstructError, stringifyError } from "@/common/utils";
import type {
	RivetCloseEvent,
	RivetEvent,
	RivetMessageEvent,
	UniversalWebSocket,
} from "@/common/websocket-interface";
import { RemoteEngineControlClient } from "@/engine-client/mod";
import type { Registry } from "@/registry";
import type {
	RegistryConfig,
	RuntimeKind,
	SqliteBackend,
} from "@/registry/config";
import { decodeCborCompat, encodeCborCompat } from "@/serde";
import { getEnvUniversal, toUint8Array, VERSION } from "@/utils";
import {
	getNodeFsSync,
	getNodePath,
	importNodeDependencies,
} from "@/utils/node";
import { logger } from "./log";
import { loadNapiRuntime } from "./napi-runtime";
import {
	buildNativeHttpRequest,
	cancelNativeHttpRequestBody,
	convertNativeHttpResponse,
	type NativeHttpRequestBodyStream,
	type NativeHttpResponseBodyStream,
} from "./native-http";
import {
	type NativeValidationConfig,
	validateActionArgs,
	validateConnParams,
	validateEventArgs,
	validateQueueBody,
	validateQueueComplete,
} from "./native-validation";
import { RunHandlerCoordinator } from "./run-handler-coordinator";
import type {
	ActorContextHandle,
	ActorFactoryHandle,
	CancellationTokenHandle,
	ConnHandle,
	CoreRuntime,
	RegistryHandle,
	RuntimeActorConfig,
	RuntimeActorInvocationTraceContext,
	RuntimeBytes,
	RuntimeCronFire,
	RuntimeCronJobInfo,
	RuntimeInspectorTabEntry,
	RuntimeQueueMessage,
	RuntimeScheduledEventInfo,
	RuntimeScheduledFireInfo,
	RuntimeServeConfig,
	RuntimeStateDeltaPayload,
	WebSocketHandle,
} from "./runtime";
import { loadWasmRuntime } from "./wasm-runtime";
import {
	createWriteThroughProxy,
	unwrapWriteThroughProxy,
} from "./write-through-proxy";

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

type ResolvedRuntimeKind = Exclude<RuntimeKind, "auto">;
type RuntimeHostKind = "node-like" | "edge-like";
export type RuntimeLoaders = {
	loadNative: () => ReturnType<typeof loadNapiRuntime>;
	loadWasm: (
		config?: RegistryConfig["wasm"],
	) => ReturnType<typeof loadWasmRuntime>;
	detectHost: () => RuntimeHostKind;
};

type SerializeStateReason = "save" | "inspector";
type NativeOnStateChangeHandler = (
	ctx: ActorContextHandleAdapter,
	state: unknown,
) => void | Promise<void>;
type NativePersistConnState = {
	state: unknown;
};

type NativeStateTransactionScope = {
	actorContext: ActorContextHandleAdapter;
	actorStateBaseline: unknown;
	connectionStateBaselines: Map<string, Uint8Array>;
	committed: boolean;
	release: () => void;
};

const defaultRuntimeLoaders: RuntimeLoaders = {
	loadNative: loadNapiRuntime,
	loadWasm: loadWasmRuntime,
	detectHost: detectRuntimeHost,
};

function trySetProcessEnv(key: string, value: string) {
	if (typeof process === "undefined") return;
	try {
		process.env[key] = value;
	} catch {
		// Some edge runtimes expose a read-only Node-compatible process.env.
	}
}

export function detectRuntimeHost(): RuntimeHostKind {
	const globalScope = globalThis as typeof globalThis & {
		Bun?: unknown;
		Deno?: unknown;
		process?: { versions?: { node?: string } };
		self?: unknown;
		window?: unknown;
	};

	if (
		globalScope.Deno !== undefined ||
		globalScope.Bun !== undefined ||
		typeof globalScope.process?.versions?.node === "string"
	) {
		return "node-like";
	}

	return "edge-like";
}

export function resolveRuntimeKind(
	runtime: RuntimeKind | undefined,
): RuntimeKind {
	return runtime ?? "auto";
}

function loadedRuntimeKind(runtime: CoreRuntime): ResolvedRuntimeKind {
	switch (runtime.kind) {
		case "napi":
			return "native";
		case "wasm":
			return "wasm";
	}

	throw new RivetError(
		"config",
		"unknown_runtime",
		"RivetKit runtime must be NAPI or wasm.",
		{
			public: true,
			statusCode: 500,
		},
	);
}

export async function loadAutoRuntime(
	config: RegistryConfig,
	loaders: RuntimeLoaders = defaultRuntimeLoaders,
): Promise<CoreRuntime> {
	if (loaders.detectHost() === "edge-like") {
		return (await loaders.loadWasm(config.wasm)).runtime;
	}

	try {
		return (await loaders.loadNative()).runtime;
	} catch {
		return (await loaders.loadWasm(config.wasm)).runtime;
	}
}

export async function loadConfiguredRuntime(
	config: RegistryConfig,
	loaders: RuntimeLoaders = defaultRuntimeLoaders,
): Promise<CoreRuntime> {
	const requested = resolveRuntimeKind(config.runtime);

	if (requested === "native") {
		return (await loaders.loadNative()).runtime;
	}

	if (requested === "wasm") {
		return (await loaders.loadWasm(config.wasm)).runtime;
	}

	return loadAutoRuntime(config, loaders);
}

function sqliteBackendForConfig(
	config: RegistryConfig,
): SqliteBackend | undefined {
	return config.sqlite?.backend ?? config.test?.sqliteBackend;
}

export function normalizeRuntimeConfigForKind(
	config: RegistryConfig,
	runtimeKind: ResolvedRuntimeKind,
): RegistryConfig {
	if (runtimeKind === "native") {
		return config;
	}

	if (sqliteBackendForConfig(config) === "local") {
		throw new RivetError(
			"config",
			"wasm_local_sqlite",
			"WebAssembly runtime cannot use local SQLite. Use remote SQLite instead.",
			{
				public: true,
				statusCode: 400,
				metadata: { runtime: "wasm", sqliteBackend: "local" },
			},
		);
	}

	return {
		...config,
		sqlite: {
			...config.sqlite,
			backend: "remote",
		},
		test: {
			...config.test,
			enabled: config.test?.enabled ?? false,
			sqliteBackend: "remote",
		},
	};
}

export function normalizeRuntimeConfig(
	config: RegistryConfig,
	runtime: CoreRuntime,
): RegistryConfig {
	return normalizeRuntimeConfigForKind(config, loadedRuntimeKind(runtime));
}
type NativePersistActorState = {
	state: unknown;
	isInOnStateChange: boolean;
	connStates: Map<string, NativePersistConnState>;
	// Set when a coalesced save and onStateChange flush is pending for the
	// current event loop tick.
	saveScheduled?: boolean;
	pendingSaveHandle?: ReturnType<typeof setImmediate>;
	activeStateTransactionOwner?: symbol;
	pendingStateTransactionOwners?: Set<symbol>;
	stateTransactionTail?: Promise<void>;
	stateTransactionSaveDeferred?: boolean;
};
type NativeDestroyGate = {
	destroyCompletion?: Promise<void>;
	resolveDestroy?: () => void;
};
type NativeDatabaseClientState = {
	client: unknown;
};
type NativeActorRuntimeState = {
	sql?: ReturnType<typeof wrapJsNativeDatabase>;
	databaseClient?: NativeDatabaseClientState;
	varsInitialized?: boolean;
	vars?: unknown;
	destroyGate?: NativeDestroyGate;
	persistState?: NativePersistActorState;
};

// Keep JS-only actor caches on the NAPI ActorContext runtime-state bag instead
// of actorId-keyed module globals so same-key recreates start from a fresh
// generation.
function getNativeRuntimeState(
	runtime: CoreRuntime,
	ctx: ActorContextHandle,
): NativeActorRuntimeState {
	const runtimeState = callNativeSync(() =>
		runtime.actorRuntimeState(ctx),
	) as NativeActorRuntimeState;
	if (!runtimeState.destroyGate) {
		runtimeState.destroyGate = {};
	}
	if (!runtimeState.persistState) {
		runtimeState.persistState = {
			state: undefined,
			isInOnStateChange: false,
			connStates: new Map(),
		};
	}
	return runtimeState;
}

function getNativePersistState(
	runtime: CoreRuntime,
	ctx: ActorContextHandle,
): NativePersistActorState {
	const persistState = getNativeRuntimeState(runtime, ctx).persistState;
	if (!persistState) {
		throw new Error("native persist state was not initialized");
	}
	return persistState;
}

function isPromiseLike(value: unknown): value is PromiseLike<void> {
	return (
		typeof value === "object" &&
		value !== null &&
		"then" in value &&
		typeof value.then === "function"
	);
}

function getNativeConnPersistState(
	runtime: CoreRuntime,
	ctx: ActorContextHandle,
	conn: ConnHandle,
): NativePersistConnState {
	const persistState = getNativePersistState(runtime, ctx);
	const connId = callNativeSync(() => runtime.connId(conn));
	let connState = persistState.connStates.get(connId);
	if (!connState) {
		connState = {
			state: undefined,
		};
		persistState.connStates.set(connId, connState);
	}
	return connState;
}

function stateMutationReentrantError(): RivetError {
	return new RivetError(
		"actor",
		"state_mutation_reentrant",
		"State mutations are not allowed inside onStateChange.",
	);
}

function stateTransactionConflictError(): RivetError {
	return new RivetError(
		"actor",
		"state_transaction_conflict",
		"State cannot be mutated by another action while an experimental state-aware database transaction is active.",
		{ public: true, statusCode: 409 },
	);
}

function databaseNotConfiguredError(): RivetError {
	return new RivetError(
		"actor",
		"database_not_configured",
		"database is not configured for this actor",
		{ public: true },
	);
}

function databaseClientNotReadyError(): RivetError {
	return new RivetError(
		"actor",
		"database_client_not_ready",
		"actor database client was not initialized before user code ran. this is an internal lifecycle error; the migration callback should have pre-warmed the client. file an issue if you can reproduce.",
		{ public: true },
	);
}
function runHandlerNotConfiguredError(): RivetError {
	return new RivetError(
		"actor",
		"run_handler_unavailable",
		"Cannot schedule a run wake because this actor does not define a run handler.",
		{ public: true, statusCode: 409 },
	);
}

function stateNotEnabledError(): RivetError {
	return new RivetError(
		"actor",
		"state_not_enabled",
		"State not enabled. Must implement `createState` or `state` to use state. (https://www.rivet.dev/docs/actors/state/#initializing-state)",
		{ public: true },
	);
}

function nativeClientNotConfiguredError(): RivetError {
	return new RivetError(
		"native",
		"client_not_configured",
		"native actor client is not configured",
		{ public: true },
	);
}

function nativeEndpointNotConfiguredError(): RivetError {
	return new RivetError(
		"native",
		"endpoint_not_configured",
		"registry endpoint is required for native envoy startup",
		{ public: true },
	);
}

function getNativeDestroyGate(runtime: CoreRuntime, ctx: ActorContextHandle) {
	const destroyGate = getNativeRuntimeState(runtime, ctx).destroyGate;
	if (!destroyGate) {
		throw new Error("native destroy gate was not initialized");
	}
	return destroyGate;
}

function markNativeDestroyRequested(
	runtime: CoreRuntime,
	ctx: ActorContextHandle,
) {
	const gate = getNativeDestroyGate(runtime, ctx);
	if (!gate.destroyCompletion) {
		gate.destroyCompletion = new Promise<void>((resolve) => {
			gate.resolveDestroy = resolve;
		});
	}
}

function resolveNativeDestroy(runtime: CoreRuntime, ctx: ActorContextHandle) {
	const gate = getNativeRuntimeState(runtime, ctx).destroyGate;
	if (!gate?.resolveDestroy) {
		return;
	}

	gate.resolveDestroy();
	gate.resolveDestroy = undefined;
	gate.destroyCompletion = undefined;
}

function clearNativeRuntimeState(
	runtime: CoreRuntime,
	ctx: ActorContextHandle,
) {
	callNativeSync(() => runtime.actorClearRuntimeState(ctx));
}

async function cleanupNativeSleepRuntimeState(
	runtime: CoreRuntime,
	ctx: ActorContextHandle,
	afterTrackedWorkDrained?: () => Promise<void>,
): Promise<void> {
	// The bounded wait gives shutdown work one grace-period chance to finish.
	// Drained means all tracked shutdown work completed before the deadline, so
	// we can save final state and clear runtime state immediately. If it did not
	// drain, close database handles now, then defer the final save and clear until
	// the tracked work finishes without a deadline.
	const drained = await runtime.actorWaitForTrackedShutdownWork(ctx);
	if (!drained) {
		await closeNativeDatabaseClient(runtime, ctx);
		await closeNativeSqlDatabase(runtime, ctx);
		void runtime
			.actorWaitForTrackedShutdownWorkUnbounded(ctx)
			.then(async () => {
				await afterTrackedWorkDrained?.();
				clearNativeRuntimeState(runtime, ctx);
			})
			.catch((error) => {
				logger().warn({
					msg: "deferred native sleep cleanup failed",
					error: stringifyError(error),
				});
			});
		return;
	}

	await afterTrackedWorkDrained?.();
	await closeNativeDatabaseClient(runtime, ctx);
	await closeNativeSqlDatabase(runtime, ctx);
	clearNativeRuntimeState(runtime, ctx);
}

function closeNativeSqlDatabase(
	runtime: CoreRuntime,
	ctx: ActorContextHandle,
): Promise<void> | undefined {
	const runtimeState = getNativeRuntimeState(runtime, ctx);
	const database = runtimeState.sql;
	if (!database) {
		return;
	}

	runtimeState.sql = undefined;
	return database.close();
}

async function closeNativeDatabaseClient(
	runtime: CoreRuntime,
	ctx: ActorContextHandle,
): Promise<void> {
	const runtimeState = getNativeRuntimeState(runtime, ctx);
	const entry = runtimeState.databaseClient;
	if (!entry) {
		return;
	}

	runtimeState.databaseClient = undefined;

	if (
		entry.client &&
		typeof entry.client === "object" &&
		"close" in entry.client &&
		typeof entry.client.close === "function"
	) {
		await entry.client.close();
	}
}

function getOrCreateNativeSqlDatabase(
	runtime: CoreRuntime,
	ctx: ActorContextHandle,
): ReturnType<typeof wrapJsNativeDatabase> {
	const runtimeState = getNativeRuntimeState(runtime, ctx);
	const cachedDatabase = runtimeState.sql;
	if (cachedDatabase) {
		return cachedDatabase;
	}

	const database = wrapJsNativeDatabase({
		exec: (sql) => runtime.actorSqlExec(ctx, sql),
		execute: (sql, params) => runtime.actorSqlExecute(ctx, sql, params),
		executeBatch: (statements) =>
			runtime.actorSqlExecuteBatch(ctx, statements),
		beginTransaction: async (timeoutMs, name) => {
			const transaction = await runtime.actorSqlBeginTransaction(
				ctx,
				timeoutMs,
				name,
			);
			return {
				exec: (sql) =>
					runtime.actorSqlTransactionExec(transaction, sql),
				execute: (sql, params) =>
					runtime.actorSqlTransactionExecute(
						transaction,
						sql,
						params,
					),
				commit: () => runtime.actorSqlTransactionCommit(transaction),
				rollback: () =>
					runtime.actorSqlTransactionRollback(transaction),
			};
		},
		beginStateTransaction: async (timeoutMs, context) => {
			const scope = context as NativeStateTransactionScope | undefined;
			if (!scope) {
				throw new Error(
					"actor state transaction context is not configured",
				);
			}
			const transaction = await runtime.actorBeginStateTransaction(
				ctx,
				timeoutMs,
			);
			return {
				exec: async () => {
					throw new Error(
						"actor state transactions only support single-statement execute calls",
					);
				},
				execute: (sql, params) =>
					runtime.actorStateTransactionExecute(
						transaction,
						sql,
						params,
					),
				commit: async () => {
					await runtime.actorStateTransactionCommit(
						transaction,
						scope.actorContext.serializeForTick("save"),
					);
					scope.committed = true;
				},
				rollback: () =>
					runtime.actorStateTransactionRollback(transaction),
			};
		},
		query: (sql, params) => runtime.actorSqlQuery(ctx, sql, params),
		run: (sql, params) => runtime.actorSqlRun(ctx, sql, params),
		metrics: () => runtime.actorSqlMetrics(ctx),
		takeLastKvError: () => runtime.actorSqlTakeLastKvError(ctx),
		close: () => runtime.actorSqlClose(ctx),
	});
	runtimeState.sql = database;
	return database;
}

function toRuntimeBytes(
	value: string | Uint8Array | ArrayBuffer,
): RuntimeBytes {
	if (typeof value === "string") {
		return textEncoder.encode(value);
	}
	if (value instanceof Uint8Array) {
		return value;
	}
	return new Uint8Array(value);
}

function arrayBufferViewToRuntimeBytes(value: ArrayBufferView): RuntimeBytes {
	return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
}

function runtimeBytesToArrayBuffer(value: RuntimeBytes): ArrayBuffer {
	return value.buffer.slice(
		value.byteOffset,
		value.byteOffset + value.byteLength,
	) as ArrayBuffer;
}

type NativeKvValueType = "text" | "arrayBuffer" | "binary";
type NativeKvKeyType = "text" | "binary";

type NativeKvValueTypeMap = {
	text: string;
	arrayBuffer: ArrayBuffer;
	binary: Uint8Array;
};

type NativeKvKeyTypeMap = {
	text: string;
	binary: Uint8Array;
};

type NativeKvValueOptions<T extends NativeKvValueType = "text"> = {
	type?: T;
};

type NativeKvListOptions<
	T extends NativeKvValueType = "text",
	K extends NativeKvKeyType = "text",
> = NativeKvValueOptions<T> & {
	keyType?: K;
	reverse?: boolean;
	limit?: number;
};

function decodeNativeKvKey<K extends NativeKvKeyType = "text">(
	key: Uint8Array,
	keyType?: K,
): NativeKvKeyTypeMap[K] {
	const resolvedKeyType = keyType ?? "text";
	switch (resolvedKeyType) {
		case "text":
			return textDecoder.decode(key) as NativeKvKeyTypeMap[K];
		case "binary":
			return key as NativeKvKeyTypeMap[K];
		default:
			throw new TypeError("Invalid kv key type");
	}
}

function encodeNativeKvUserKey<K extends NativeKvKeyType = NativeKvKeyType>(
	key: NativeKvKeyTypeMap[K],
	keyType?: K,
): Uint8Array {
	if (key instanceof Uint8Array) {
		return key;
	}
	const resolvedKeyType = keyType ?? "text";
	if (resolvedKeyType === "binary") {
		throw new TypeError("Expected a Uint8Array when keyType is binary");
	}
	return textEncoder.encode(key);
}

function decodeNativeKvValue<T extends NativeKvValueType = "text">(
	value: Uint8Array,
	options?: NativeKvValueOptions<T>,
): NativeKvValueTypeMap[T] {
	const type = options?.type ?? "text";
	switch (type) {
		case "text":
			return textDecoder.decode(value) as NativeKvValueTypeMap[T];
		case "arrayBuffer": {
			const copy = new Uint8Array(value.byteLength);
			copy.set(value);
			return copy.buffer as NativeKvValueTypeMap[T];
		}
		case "binary":
			return value as NativeKvValueTypeMap[T];
		default:
			throw new TypeError("Invalid kv value type");
	}
}

async function loadEngineCli(): Promise<typeof import("@rivetkit/engine-cli")> {
	// LOAD-BEARING: the specifier is computed (`.join("/")`) on purpose. Do NOT
	// "simplify" it to a literal `import("@rivetkit/engine-cli")`. Edge adapters
	// (e.g. @rivetkit/supabase) pre-bundle this module for Deno; a literal
	// dynamic import is statically resolvable, so Deno's eszip bundler would
	// snapshot the ~117 MB engine-cli binary into the deploy and 413. The
	// computed specifier keeps it opaque to static analysis so it is never
	// bundled. Enforced by scripts/ci/check-edge-native-closure.mjs.
	return import(["@rivetkit", "engine-cli"].join("/"));
}

async function loadServices(): Promise<typeof import("@rivet-dev/services")> {
	// Keep the native binary opaque to edge bundlers for the same reason as
	// engine-cli above.
	return import(["@rivet-dev", "services"].join("/"));
}

function decodeValue<T>(value?: RuntimeBytes | null): T {
	if (!value || value.length === 0) {
		return undefined as T;
	}

	return decodeCborCompat(value);
}

// Rejects `WorkflowHistoryBytes`: the compat layer rewrites an ArrayBuffer as
// `["$ArrayBuffer", "<base64>"]`, and the inspector wire field carries raw BARE.
// The conditional distributes, so it catches the branded type on its own but
// only strips it from a union that also has other members. A non-distributive
// `[T] extends [...]` resolves to `never` for the `any` arguments this takes
// elsewhere, so the union case stays uncovered.
function encodeValue<T>(
	value: T extends WorkflowHistoryBytes ? never : T,
): RuntimeBytes {
	return encodeCborCompat(value as JsonCompatValue);
}

function normalizeArgs(value: unknown): unknown[] {
	return Array.isArray(value)
		? value
		: value === undefined || value === null
			? []
			: [value];
}

function unwrapTsfnPayload<T>(error: unknown, payload: T): T {
	if (error !== null && error !== undefined) {
		throw error;
	}

	return payload;
}

function normalizeNativeBridgeError(error: unknown): unknown {
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

function isStructuredBridgeError(
	error: unknown,
): error is RivetError | RivetErrorLike {
	if (error instanceof RivetError) {
		return true;
	}

	return (
		isRivetErrorLike(error) &&
		"__type" in error &&
		(error.__type === "RivetError" || error.__type === "ActorError")
	);
}

function encodeNativeCallbackError(error: unknown): Error {
	const structuredError = isStructuredBridgeError(error)
		? error
		: deconstructError(error, true);

	const bridgeError = new Error(encodeBridgeRivetError(structuredError), {
		cause: error instanceof Error ? error : undefined,
	});
	return Object.assign(bridgeError, {
		group: structuredError.group,
		code: structuredError.code,
		metadata: structuredError.metadata,
		rayId: structuredError.rayId,
	});
}

async function callNative<T>(invoke: () => Promise<T>): Promise<T> {
	try {
		return await invoke();
	} catch (error) {
		throw normalizeNativeBridgeError(error);
	}
}

function callNativeSync<T>(invoke: () => T): T {
	try {
		return invoke();
	} catch (error) {
		throw normalizeNativeBridgeError(error);
	}
}

type NativeWorkflowInspectorConfig =
	WorkflowInspectorConfig<WorkflowHistoryBytes> & {
		getState?: () => Promise<unknown> | unknown;
	};

function isClosedTaskRegistrationError(error: unknown): boolean {
	const metadata = error instanceof RivetError ? error.metadata : undefined;
	const metadataError =
		metadata && typeof metadata === "object" && "error" in metadata
			? metadata.error
			: undefined;
	return (
		error instanceof RivetError &&
		error.group === "core" &&
		error.code === INTERNAL_ERROR_CODE &&
		typeof metadataError === "string" &&
		/actor task registration is (closed|not configured)/.test(metadataError)
	);
}

async function createCancellationTokenHandle(
	runtime: CoreRuntime,
	signal?: AbortSignal,
): Promise<{
	token?: CancellationTokenHandle;
	cleanup?: () => void;
}> {
	if (!signal) {
		return {};
	}

	const token = runtime.createCancellationToken();

	if (signal.aborted) {
		runtime.cancelCancellationToken(token);
		return { token };
	}

	const abort = () => runtime.cancelCancellationToken(token);
	signal.addEventListener("abort", abort, { once: true });
	return {
		token,
		cleanup: () => signal.removeEventListener("abort", abort),
	};
}

function decodeWorkflowCbor(data: ArrayBuffer | null): unknown | null {
	if (data === null) {
		return null;
	}

	try {
		return decodeCborCompat(new Uint8Array(data));
	} catch {
		return null;
	}
}

function serializeWorkflowLocation(
	location: ReturnType<
		typeof decodeWorkflowHistoryTransport
	>["entries"][number]["location"],
): Array<
	| { tag: "WorkflowNameIndex"; val: number }
	| {
			tag: "WorkflowLoopIterationMarker";
			val: { loop: number; iteration: number };
	  }
> {
	return location.map((segment) => {
		if (segment.tag === "WorkflowNameIndex") {
			return {
				tag: segment.tag,
				val: segment.val,
			};
		}

		return {
			tag: segment.tag,
			val: {
				loop: segment.val.loop,
				iteration: segment.val.iteration,
			},
		};
	});
}

function serializeWorkflowBranches(
	branches: ReadonlyMap<
		string,
		ReturnType<
			typeof decodeWorkflowHistoryTransport
		>["entries"][number]["kind"] extends infer T
			? T extends { tag: "WorkflowJoinEntry"; val: { branches: infer B } }
				? B extends ReadonlyMap<string, infer V>
					? V
					: never
				: T extends {
							tag: "WorkflowRaceEntry";
							val: { branches: infer B };
						}
					? B extends ReadonlyMap<string, infer V>
						? V
						: never
					: never
			: never
	>,
): Record<
	string,
	{ status: string; output: unknown | null; error: string | null }
> {
	return Object.fromEntries(
		Array.from(branches.entries()).map(([name, branch]) => [
			name,
			{
				status: branch.status,
				output: decodeWorkflowCbor(branch.output),
				error: branch.error,
			},
		]),
	);
}

function serializeWorkflowEntryKind(
	kind: ReturnType<
		typeof decodeWorkflowHistoryTransport
	>["entries"][number]["kind"],
):
	| {
			tag: "WorkflowStepEntry";
			val: { output: unknown | null; error: string | null };
	  }
	| {
			tag: "WorkflowLoopEntry";
			val: {
				state: unknown | null;
				iteration: number;
				output: unknown | null;
			};
	  }
	| { tag: "WorkflowSleepEntry"; val: { deadline: number; state: string } }
	| {
			tag: "WorkflowMessageEntry";
			val: { name: string; messageData: unknown | null };
	  }
	| { tag: "WorkflowRollbackCheckpointEntry"; val: { name: string } }
	| {
			tag: "WorkflowJoinEntry";
			val: {
				branches: Record<
					string,
					{
						status: string;
						output: unknown | null;
						error: string | null;
					}
				>;
			};
	  }
	| {
			tag: "WorkflowRaceEntry";
			val: {
				winner: string | null;
				branches: Record<
					string,
					{
						status: string;
						output: unknown | null;
						error: string | null;
					}
				>;
			};
	  }
	| {
			tag: "WorkflowRemovedEntry";
			val: { originalType: string; originalName: string | null };
	  }
	| {
			tag: "WorkflowVersionCheckEntry";
			val: { resolved: number; latest: number };
	  } {
	switch (kind.tag) {
		case "WorkflowStepEntry":
			return {
				tag: kind.tag,
				val: {
					output: decodeWorkflowCbor(kind.val.output),
					error: kind.val.error,
				},
			};
		case "WorkflowLoopEntry":
			return {
				tag: kind.tag,
				val: {
					state: decodeWorkflowCbor(kind.val.state),
					iteration: kind.val.iteration,
					output: decodeWorkflowCbor(kind.val.output),
				},
			};
		case "WorkflowSleepEntry":
			return {
				tag: kind.tag,
				val: {
					deadline: Number(kind.val.deadline),
					state: kind.val.state,
				},
			};
		case "WorkflowMessageEntry":
			return {
				tag: kind.tag,
				val: {
					name: kind.val.name,
					messageData: decodeWorkflowCbor(kind.val.messageData),
				},
			};
		case "WorkflowRollbackCheckpointEntry":
			return {
				tag: kind.tag,
				val: {
					name: kind.val.name,
				},
			};
		case "WorkflowJoinEntry":
			return {
				tag: kind.tag,
				val: {
					branches: serializeWorkflowBranches(kind.val.branches),
				},
			};
		case "WorkflowRaceEntry":
			return {
				tag: kind.tag,
				val: {
					winner: kind.val.winner,
					branches: serializeWorkflowBranches(kind.val.branches),
				},
			};
		case "WorkflowRemovedEntry":
			return {
				tag: kind.tag,
				val: {
					originalType: kind.val.originalType,
					originalName: kind.val.originalName,
				},
			};
		case "WorkflowVersionCheckEntry":
			return {
				tag: kind.tag,
				val: {
					resolved: kind.val.resolved,
					latest: kind.val.latest,
				},
			};
	}
}

// TODO: Switch inspector routes to CBOR encoding
function serializeWorkflowHistoryForJson(data: ArrayBuffer | null): {
	nameRegistry: string[];
	entries: Array<{
		id: string;
		location: Array<
			| { tag: "WorkflowNameIndex"; val: number }
			| {
					tag: "WorkflowLoopIterationMarker";
					val: { loop: number; iteration: number };
			  }
		>;
		kind: ReturnType<typeof serializeWorkflowEntryKind>;
	}>;
	entryMetadata: Record<
		string,
		{
			status: string;
			error: string | null;
			attempts: number;
			lastAttemptAt: number;
			createdAt: number;
			completedAt: number | null;
			rollbackCompletedAt: number | null;
			rollbackError: string | null;
		}
	>;
} | null {
	if (data === null) {
		return null;
	}

	const history = decodeWorkflowHistoryTransport(data);

	return jsonSafe({
		nameRegistry: [...history.nameRegistry],
		entries: history.entries.map((entry) => ({
			id: entry.id,
			location: serializeWorkflowLocation(entry.location),
			kind: serializeWorkflowEntryKind(entry.kind),
		})),
		entryMetadata: Object.fromEntries(
			Array.from(history.entryMetadata.entries()).map(
				([entryId, meta]) => [
					entryId,
					{
						status: meta.status,
						error: meta.error,
						attempts: meta.attempts,
						lastAttemptAt: Number(meta.lastAttemptAt),
						createdAt: Number(meta.createdAt),
						completedAt:
							meta.completedAt === null
								? null
								: Number(meta.completedAt),
						rollbackCompletedAt:
							meta.rollbackCompletedAt === null
								? null
								: Number(meta.rollbackCompletedAt),
						rollbackError: meta.rollbackError,
					},
				],
			),
		),
	});
}

function toHttpJsonCompatible<T>(value: T): T {
	return JSON.parse(
		JSON.stringify(value, (_key, nestedValue) =>
			typeof nestedValue === "bigint"
				? Number(nestedValue)
				: nestedValue instanceof Uint8Array
					? Array.from(nestedValue)
					: nestedValue,
		),
	) as T;
}

function jsonSafe<T>(value: T): T {
	return toHttpJsonCompatible(value);
}

function normalizeSqlitePropertyBindings(
	properties: Record<string, unknown>,
): Record<string, unknown> {
	const normalized: Record<string, unknown> = {};
	for (const [key, value] of Object.entries(properties)) {
		if (/^[:@$]/.test(key)) {
			normalized[key] = value;
			continue;
		}

		normalized[`:${key}`] = value;
		normalized[`@${key}`] = value;
		normalized[`$${key}`] = value;
	}
	return normalized;
}

function queryRows(result: unknown): Array<Record<string, unknown>> {
	if (Array.isArray(result)) {
		return result as Array<Record<string, unknown>>;
	}
	if (
		result &&
		typeof result === "object" &&
		"columns" in result &&
		"rows" in result &&
		Array.isArray((result as { columns: unknown }).columns) &&
		Array.isArray((result as { rows: unknown }).rows)
	) {
		const columns = (result as { columns: string[] }).columns;
		return ((result as { rows: unknown[][] }).rows ?? []).map((row) =>
			Object.fromEntries(
				columns.map((column, index) => [column, row[index]]),
			),
		);
	}
	return [];
}

function wrapNativeCallback<Args extends Array<unknown>, Result>(
	callback: (...args: Args) => Result | Promise<Result>,
): (...args: Args) => Promise<Result> {
	return async (...args: Args) => {
		try {
			return await callback(...args);
		} catch (error) {
			throw encodeNativeCallbackError(error);
		}
	};
}

function decodeArgs(value?: RuntimeBytes | null): unknown[] {
	const decoded = decodeValue<unknown>(value);
	return normalizeArgs(decoded);
}

function toActorKey(
	segments: Array<{
		kind: string;
		stringValue?: string;
		numberValue?: number;
	}>,
): string[] {
	return segments.map((segment) =>
		segment.kind === "number"
			? String(segment.numberValue ?? 0)
			: (segment.stringValue ?? ""),
	);
}

class NativeConnAdapter {
	#runtime: CoreRuntime;
	#conn: ConnHandle;
	#schemas: NativeValidationConfig;
	#ctx?: ActorContextHandle;
	#queueHibernationRemoval?: (connId: string) => void;
	#assertCanMutateState?: () => void;
	#stateProxy?: unknown;
	#stateProxyTarget?: unknown;

	constructor(
		runtime: CoreRuntime,
		conn: ConnHandle,
		schemas: NativeValidationConfig = {},
		ctx?: ActorContextHandle,
		queueHibernationRemoval?: (connId: string) => void,
		assertCanMutateState?: () => void,
	) {
		this.#runtime = runtime;
		this.#conn = conn;
		this.#schemas = schemas;
		this.#ctx = ctx;
		this.#queueHibernationRemoval = queueHibernationRemoval;
		this.#assertCanMutateState = assertCanMutateState;
		(
			this as NativeConnAdapter & {
				[CONN_STATE_MANAGER_SYMBOL]?: unknown;
			}
		)[CONN_STATE_MANAGER_SYMBOL] = {
			stateEnabled: true,
			get state() {
				return thisConn.state;
			},
		};
		const thisConn = this;
	}

	get id(): string {
		return this.#runtime.connId(this.#conn);
	}

	get params(): unknown {
		return validateConnParams(
			this.#schemas.connParamsSchema,
			decodeValue(this.#runtime.connParams(this.#conn)),
		);
	}

	[RAW_STATE_SYMBOL](): unknown {
		return this.#readState();
	}

	get state(): unknown {
		const nextState = this.#readState();
		if (!this.#ctx) {
			return this.#createStateProxy(nextState);
		}

		// Reading `conn.state` rebuilds the deep write-through proxy, which
		// allocates fresh on-change caches and rewraps the whole tree. Memoize
		// the proxy keyed on the underlying state object so repeated reads and
		// deep read cascades reuse a single proxy.
		if (
			this.#stateProxy === undefined ||
			this.#stateProxyTarget !== nextState
		) {
			this.#stateProxyTarget = nextState;
			this.#stateProxy = this.#createStateProxy(nextState);
		}
		return this.#stateProxy;
	}

	#createStateProxy(state: unknown): unknown {
		const assertCurrentState = () => {
			this.#assertCanMutateState?.();
			if (this.#ctx && this.#readState() !== state) {
				throw stateTransactionConflictError();
			}
		};
		return createWriteThroughProxy(
			state,
			(nextValue) => {
				assertCurrentState();
				this.#writeState(nextValue, { writeNative: true });
			},
			(newValue) => {
				assertCurrentState();
				assertJsonCompatValue(newValue);
			},
		);
	}

	set state(value: unknown) {
		this.#assertCanMutateState?.();
		const nextValue = unwrapWriteThroughProxy(value);
		assertJsonCompatValue(nextValue);
		this.#writeState(nextValue, { writeNative: true });
	}

	initializeState(value: unknown): void {
		this.#writeState(unwrapWriteThroughProxy(value), {
			writeNative: false,
		});
	}

	get isHibernatable(): boolean {
		return callNativeSync(() =>
			this.#runtime.connIsHibernatable(this.#conn),
		);
	}

	send(name: string, ...args: unknown[]): void {
		const validatedArgs = validateEventArgs(
			this.#schemas.events,
			name,
			args,
		);
		callNativeSync(() =>
			this.#runtime.connSend(
				this.#conn,
				name,
				encodeValue(validatedArgs),
			),
		);
	}

	async disconnect(reason?: string): Promise<void> {
		const connId = this.id;
		await callNative(() =>
			this.#runtime.connDisconnect(this.#conn, reason),
		);
		if (this.isHibernatable) {
			this.#queueHibernationRemoval?.(connId);
		}
	}

	#readState(): unknown {
		if (!this.#ctx) {
			return decodeValue(this.#runtime.connState(this.#conn));
		}

		const connState = getNativeConnPersistState(
			this.#runtime,
			this.#ctx,
			this.#conn,
		);
		if (connState.state === undefined) {
			connState.state = decodeValue(this.#runtime.connState(this.#conn));
		}
		return connState.state;
	}

	#writeState(
		value: unknown,
		options: {
			writeNative: boolean;
		},
	): void {
		const encoded = encodeValue(value);
		if (!this.#ctx) {
			this.#runtime.connSetState(this.#conn, encoded);
			return;
		}

		const connState = getNativeConnPersistState(
			this.#runtime,
			this.#ctx,
			this.#conn,
		);
		connState.state = value;
		if (options.writeNative) {
			this.#runtime.connSetState(this.#conn, encoded);
		}
	}
}

class NativeScheduleAdapter implements ActorSchedule {
	#runtime: CoreRuntime;
	#ctx: ActorContextHandle;

	constructor(runtime: CoreRuntime, ctx: ActorContextHandle) {
		this.#runtime = runtime;
		this.#ctx = ctx;
	}

	async after(
		duration: number,
		action: string,
		...args: unknown[]
	): Promise<string> {
		return await this.#runtime.actorScheduleAfter(
			this.#ctx,
			duration,
			action,
			encodeValue(args),
		);
	}

	async at(
		timestamp: number,
		action: string,
		...args: unknown[]
	): Promise<string> {
		return await this.#runtime.actorScheduleAt(
			this.#ctx,
			timestamp,
			action,
			encodeValue(args),
		);
	}

	async cancel(id: string): Promise<boolean> {
		return await this.#runtime.actorScheduleCancel(this.#ctx, id);
	}

	async get(id: string): Promise<ScheduledEventInfo | undefined> {
		const event = await this.#runtime.actorScheduleGet(this.#ctx, id);
		return event ? decodeScheduledEventInfo(event) : undefined;
	}

	async list(): Promise<ScheduledEventInfo[]> {
		return (await this.#runtime.actorScheduleList(this.#ctx)).map(
			decodeScheduledEventInfo,
		);
	}
}

class NativeCronAdapter implements ActorCron {
	#runtime: CoreRuntime;
	#ctx: ActorContextHandle;

	constructor(runtime: CoreRuntime, ctx: ActorContextHandle) {
		this.#runtime = runtime;
		this.#ctx = ctx;
	}

	async set(options: ActorCronSetOptions): Promise<void> {
		await this.#runtime.actorCronSet(
			this.#ctx,
			options.name,
			options.expression,
			options.timezone,
			options.action,
			encodeValue(options.args ?? []),
			options.maxHistory,
		);
	}

	async every(options: ActorCronEveryOptions): Promise<void> {
		await this.#runtime.actorCronEvery(
			this.#ctx,
			options.name,
			options.interval,
			options.action,
			encodeValue(options.args ?? []),
			options.maxHistory,
		);
	}

	async get(name: string): Promise<CronJobInfo | undefined> {
		const job = await this.#runtime.actorCronGet(this.#ctx, name);
		return job ? decodeCronJobInfo(job) : undefined;
	}

	async list(): Promise<CronJobInfo[]> {
		return (await this.#runtime.actorCronList(this.#ctx)).map(
			decodeCronJobInfo,
		);
	}

	async delete(name: string): Promise<boolean> {
		return await this.#runtime.actorCronDelete(this.#ctx, name);
	}

	async history(
		name: string,
		options?: { limit?: number },
	): Promise<CronFire[]> {
		return (
			await this.#runtime.actorCronHistory(
				this.#ctx,
				name,
				options?.limit,
			)
		).map(decodeCronFire);
	}
}

function decodeScheduledEventInfo(
	event: RuntimeScheduledEventInfo,
): ScheduledEventInfo {
	return {
		id: event.id,
		action: event.action,
		args: decodeArgs(event.args),
		runAt: event.runAt,
	};
}

function decodeCronJobInfo(job: RuntimeCronJobInfo): CronJobInfo {
	const base = {
		name: job.name,
		action: job.action,
		args: decodeArgs(job.args),
		nextRunAt: job.nextRunAt,
		lastRunAt: job.lastRunAt,
		maxHistory: job.maxHistory,
	};
	if (job.kind === "cron") {
		return {
			...base,
			kind: "cron",
			expression: job.expression as string,
			timezone: job.timezone as string,
		};
	}
	return {
		...base,
		kind: "every",
		interval: job.intervalMs as number,
	};
}

function decodeCronFire(fire: RuntimeCronFire): CronFire {
	return { ...fire };
}

class NativeKvAdapter {
	#runtime: CoreRuntime;
	#ctx: ActorContextHandle;

	constructor(runtime: CoreRuntime, ctx: ActorContextHandle) {
		this.#runtime = runtime;
		this.#ctx = ctx;
	}

	async get<T extends NativeKvValueType = "text">(
		key: string | Uint8Array,
		options?: NativeKvValueOptions<T>,
	): Promise<NativeKvValueTypeMap[T] | null> {
		const value = await callNative(() =>
			this.#runtime.actorKvGet(
				this.#ctx,
				makePrefixedKey(encodeNativeKvUserKey(key)),
			),
		);
		return value
			? decodeNativeKvValue(new Uint8Array(value), options)
			: null;
	}

	async put(
		key: string | Uint8Array,
		value: string | Uint8Array | ArrayBuffer,
		_options?: NativeKvValueOptions,
	): Promise<void> {
		await callNative(() =>
			this.#runtime.actorKvPut(
				this.#ctx,
				makePrefixedKey(encodeNativeKvUserKey(key)),
				toRuntimeBytes(value),
			),
		);
	}

	async delete(key: string | Uint8Array): Promise<void> {
		await callNative(() =>
			this.#runtime.actorKvDelete(
				this.#ctx,
				makePrefixedKey(encodeNativeKvUserKey(key)),
			),
		);
	}

	async deleteRange(
		start: string | Uint8Array,
		end: string | Uint8Array,
	): Promise<void> {
		await callNative(() =>
			this.#runtime.actorKvDeleteRange(
				this.#ctx,
				makePrefixedKey(encodeNativeKvUserKey(start)),
				makePrefixedKey(encodeNativeKvUserKey(end)),
			),
		);
	}

	async rawDeleteRange(start: Uint8Array, end: Uint8Array): Promise<void> {
		await callNative(() =>
			this.#runtime.actorKvDeleteRange(this.#ctx, start, end),
		);
	}

	async listPrefix<
		T extends NativeKvValueType = "text",
		K extends NativeKvKeyType = "text",
	>(
		prefix: string | Uint8Array,
		options?: NativeKvListOptions<T, K>,
	): Promise<Array<[NativeKvKeyTypeMap[K], NativeKvValueTypeMap[T]]>> {
		const entries = await callNative(() =>
			this.#runtime.actorKvListPrefix(
				this.#ctx,
				makePrefixedKey(
					encodeNativeKvUserKey(
						prefix as NativeKvKeyTypeMap[K],
						options?.keyType,
					),
				),
				{
					reverse: options?.reverse,
					limit: options?.limit,
				},
			),
		);
		return entries.map((entry) => [
			decodeNativeKvKey(
				removePrefixFromKey(new Uint8Array(entry.key)),
				options?.keyType,
			),
			decodeNativeKvValue(new Uint8Array(entry.value), options),
		]);
	}

	async rawListPrefix(
		prefix: Uint8Array,
	): Promise<Array<[Uint8Array, Uint8Array]>> {
		const entries = await callNative(() =>
			this.#runtime.actorKvListPrefix(this.#ctx, prefix, {}),
		);
		return entries.map((entry) => [
			new Uint8Array(entry.key),
			new Uint8Array(entry.value),
		]);
	}

	async listRange<
		T extends NativeKvValueType = "text",
		K extends NativeKvKeyType = "text",
	>(
		start: string | Uint8Array,
		end: string | Uint8Array,
		options?: NativeKvListOptions<T, K>,
	): Promise<Array<[NativeKvKeyTypeMap[K], NativeKvValueTypeMap[T]]>> {
		const entries = await callNative(() =>
			this.#runtime.actorKvListRange(
				this.#ctx,
				makePrefixedKey(
					encodeNativeKvUserKey(
						start as NativeKvKeyTypeMap[K],
						options?.keyType,
					),
				),
				makePrefixedKey(
					encodeNativeKvUserKey(
						end as NativeKvKeyTypeMap[K],
						options?.keyType,
					),
				),
				{
					reverse: options?.reverse,
					limit: options?.limit,
				},
			),
		);
		return entries.map((entry) => [
			decodeNativeKvKey(
				removePrefixFromKey(new Uint8Array(entry.key)),
				options?.keyType,
			),
			decodeNativeKvValue(new Uint8Array(entry.value), options),
		]);
	}

	async list<
		T extends NativeKvValueType = "text",
		K extends NativeKvKeyType = "text",
	>(
		prefix: string | Uint8Array,
		options?: NativeKvListOptions<T, K>,
	): Promise<Array<[NativeKvKeyTypeMap[K], NativeKvValueTypeMap[T]]>> {
		return this.listPrefix(prefix, options);
	}

	async batchGet(keys: Uint8Array[]): Promise<Array<Uint8Array | null>> {
		const values = await callNative(() =>
			this.#runtime.actorKvBatchGet(this.#ctx, keys),
		);
		return values.map((value) => (value ? new Uint8Array(value) : null));
	}

	async batchPut(entries: [Uint8Array, Uint8Array][]): Promise<void> {
		await callNative(() =>
			this.#runtime.actorKvBatchPut(
				this.#ctx,
				entries.map(([key, value]) => ({
					key,
					value,
				})),
			),
		);
	}

	async batchDelete(keys: Uint8Array[]): Promise<void> {
		await callNative(() =>
			this.#runtime.actorKvBatchDelete(this.#ctx, keys),
		);
	}
}

function wrapQueueMessage(
	message: RuntimeQueueMessage,
	schemas: NativeValidationConfig["queues"],
) {
	const name = callNativeSync(() => message.name());
	const id = callNativeSync(() => message.id());
	return {
		id: id <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(id) : id,
		name,
		body: validateQueueBody(
			schemas,
			name,
			decodeValue(callNativeSync(() => message.body())),
		),
		createdAt: callNativeSync(() => message.createdAt()),
		complete: callNativeSync(() => message.isCompletable())
			? async (response?: unknown) =>
					await callNative(() =>
						message.complete(
							response === undefined
								? undefined
								: encodeValue(
										validateQueueComplete(
											schemas,
											name,
											response,
										),
									),
						),
					)
			: undefined,
	};
}

class NativeQueueAdapter {
	#runtime: CoreRuntime;
	#ctx: ActorContextHandle;
	#schemas: NativeValidationConfig["queues"];
	#pendingCompletableMessageIds = new Set<string>();

	constructor(
		runtime: CoreRuntime,
		ctx: ActorContextHandle,
		schemas: NativeValidationConfig["queues"] = undefined,
	) {
		this.#runtime = runtime;
		this.#ctx = ctx;
		this.#schemas = schemas;
	}

	async send(name: string, body: unknown) {
		const validatedBody = validateQueueBody(this.#schemas, name, body);
		return wrapQueueMessage(
			await callNative(() =>
				this.#runtime.actorQueueSend(
					this.#ctx,
					name,
					encodeValue(validatedBody),
				),
			),
			this.#schemas,
		);
	}

	async next(options?: {
		names?: readonly string[];
		timeout?: number;
		signal?: AbortSignal;
		completable?: boolean;
	}) {
		const messages = await this.nextBatch({
			names: options?.names,
			count: 1,
			timeout: options?.timeout,
			signal: options?.signal,
			completable: options?.completable,
		});
		return messages[0];
	}

	async nextBatch(options?: {
		names?: readonly string[];
		count?: number;
		timeout?: number;
		signal?: AbortSignal;
		completable?: boolean;
	}) {
		const completable = options?.completable === true;
		if (this.#pendingCompletableMessageIds.size > 0) {
			throw new RivetError(
				"queue",
				"previous_message_not_completed",
				"Previous completable queue message is not completed. Call `message.complete(...)` before receiving the next message.",
				{
					public: true,
					statusCode: 400,
				},
			);
		}

		const { token, cleanup } = await createCancellationTokenHandle(
			this.#runtime,
			options?.signal,
		);

		try {
			const messages = await callNative(() =>
				this.#runtime.actorQueueNextBatch(
					this.#ctx,
					{
						names: this.#normalizeNames(options?.names),
						count: options?.count,
						timeoutMs: options?.timeout,
						completable,
					},
					token,
				),
			);
			const wrapped = messages.map((message) =>
				wrapQueueMessage(message, this.#schemas),
			);
			return completable
				? wrapped.map((message) =>
						this.#makeCompletableMessage(message),
					)
				: wrapped;
		} finally {
			cleanup?.();
		}
	}

	async waitForNames(
		names: readonly string[],
		options?: {
			timeout?: number;
			signal?: AbortSignal;
			completable?: boolean;
		},
	) {
		const { token, cleanup } = await createCancellationTokenHandle(
			this.#runtime,
			options?.signal,
		);

		try {
			return wrapQueueMessage(
				await callNative(() =>
					this.#runtime.actorQueueWaitForNames(
						this.#ctx,
						[...names],
						{
							timeoutMs: options?.timeout,
							completable: options?.completable,
						},
						token,
					),
				),
				this.#schemas,
			);
		} finally {
			cleanup?.();
		}
	}

	async waitForNamesAvailable(
		names: readonly string[],
		options?: {
			timeout?: number;
			signal?: AbortSignal;
		},
	) {
		const { token, cleanup } = await createCancellationTokenHandle(
			this.#runtime,
			options?.signal,
		);

		try {
			await callNative(() =>
				this.#runtime.actorQueueWaitForNamesAvailable(
					this.#ctx,
					[...names],
					{
						timeoutMs: options?.timeout,
					},
					token,
				),
			);
		} finally {
			cleanup?.();
		}
	}
	async waitForAvailable(
		names?: readonly string[],
		options?: { timeout?: number; signal?: AbortSignal },
	): Promise<void> {
		await this.waitForNamesAvailable(names ?? [], options);
	}

	async complete(
		message: { id: number | bigint; name: string },
		response?: unknown,
	): Promise<void> {
		const validatedResponse = validateQueueComplete(
			this.#schemas,
			message.name,
			response,
		);
		await callNative(() =>
			this.#runtime.actorQueueCompletePersisted(
				this.#ctx,
				BigInt(message.id),
				message.name,
				encodeValue(validatedResponse),
			),
		);
		this.#pendingCompletableMessageIds.delete(message.id.toString());
	}

	async enqueueAndWait(
		name: string,
		body: unknown,
		options?: {
			timeout?: number;
			signal?: AbortSignal;
		},
	) {
		const validatedBody = validateQueueBody(this.#schemas, name, body);
		const { token, cleanup } = await createCancellationTokenHandle(
			this.#runtime,
			options?.signal,
		);

		try {
			const response = await callNative(() =>
				this.#runtime.actorQueueEnqueueAndWait(
					this.#ctx,
					name,
					encodeValue(validatedBody),
					{
						timeoutMs: options?.timeout,
					},
					token,
				),
			);
			return response === undefined || response === null
				? undefined
				: validateQueueComplete(
						this.#schemas,
						name,
						decodeValue(response),
					);
		} finally {
			cleanup?.();
		}
	}

	async tryNext(options?: {
		names?: readonly string[];
		completable?: boolean;
	}) {
		const messages = await this.tryNextBatch({
			names: options?.names,
			count: 1,
			completable: options?.completable,
		});
		return messages[0];
	}

	async tryNextBatch(options?: {
		names?: readonly string[];
		count?: number;
		completable?: boolean;
	}) {
		if (options?.completable) {
			return await this.nextBatch({
				names: options.names,
				count: options.count,
				timeout: 0,
				completable: true,
			});
		}

		try {
			return await this.nextBatch({
				names: options?.names,
				count: options?.count,
				timeout: 0,
				completable: false,
			});
		} catch (error) {
			if (
				(error as { group?: string; code?: string }).group ===
					"queue" &&
				(error as { group?: string; code?: string }).code ===
					"timed_out"
			) {
				return [];
			}
			throw error;
		}
	}

	async *iter(options?: {
		names?: readonly string[];
		signal?: AbortSignal;
		completable?: boolean;
	}): AsyncIterableIterator<
		NonNullable<Awaited<ReturnType<NativeQueueAdapter["next"]>>>
	> {
		for (;;) {
			try {
				const message = await this.next(options);
				if (!message) {
					continue;
				}
				yield message;
			} catch (error) {
				if (isActorAbortedError(error)) {
					return;
				}
				throw error;
			}
		}
	}

	#normalizeNames(
		names: readonly string[] | undefined,
	): string[] | undefined {
		if (!names || names.length === 0) {
			return undefined;
		}
		return [...new Set(names)];
	}

	#makeCompletableMessage(
		message: Awaited<ReturnType<typeof wrapQueueMessage>>,
	) {
		const messageId = message.id.toString();
		this.#pendingCompletableMessageIds.add(messageId);
		let completed = false;

		return {
			...message,
			complete: async (response?: unknown) => {
				if (typeof message.complete !== "function") {
					throw new RivetError(
						"queue",
						"complete_not_configured",
						`Queue '${message.name}' does not support completion responses.`,
						{
							public: true,
							statusCode: 400,
							metadata: { name: message.name },
						},
					);
				}
				if (completed) {
					throw new RivetError(
						"queue",
						"already_completed",
						"Queue message was already completed.",
						{
							public: true,
							statusCode: 400,
						},
					);
				}

				await message.complete(response);
				completed = true;
				this.#pendingCompletableMessageIds.delete(messageId);
			},
		};
	}
}

class NativeWebSocketAdapter {
	#runtime: CoreRuntime;
	#ws: WebSocketHandle;
	#virtual: VirtualWebSocket;
	#readyState: 0 | 1 | 2 | 3 = VirtualWebSocket.OPEN;

	constructor(runtime: CoreRuntime, ws: WebSocketHandle) {
		this.#runtime = runtime;
		this.#ws = ws;
		this.#virtual = new VirtualWebSocket({
			getReadyState: () => this.#readyState,
			onSend: (data) => {
				if (typeof data === "string") {
					callNativeSync(() =>
						this.#runtime.webSocketSend(
							this.#ws,
							textEncoder.encode(data),
							false,
						),
					);
					return;
				}

				const bytes = ArrayBuffer.isView(data)
					? arrayBufferViewToRuntimeBytes(data)
					: new Uint8Array(data as ArrayBufferLike);
				callNativeSync(() =>
					this.#runtime.webSocketSend(this.#ws, bytes, true),
				);
			},
			onClose: (code, reason) => {
				this.#readyState = VirtualWebSocket.CLOSING;
				void callNative(() =>
					this.#runtime.webSocketClose(this.#ws, code, reason),
				);
			},
		});
		this.#runtime.webSocketSetEventCallback(this.#ws, (event) => {
			if (event.kind === "message") {
				this.#virtual.triggerMessage(
					event.binary
						? runtimeBytesToArrayBuffer(event.data as RuntimeBytes)
						: event.data,
					event.messageIndex,
				);
				return;
			}

			this.#readyState = VirtualWebSocket.CLOSED;
			this.#virtual.triggerClose(
				event.code,
				event.reason,
				event.wasClean,
			);
		});
	}

	get readyState() {
		return this.#virtual.readyState;
	}

	get CONNECTING() {
		return this.#virtual.CONNECTING;
	}

	get OPEN() {
		return this.#virtual.OPEN;
	}

	get CLOSING() {
		return this.#virtual.CLOSING;
	}

	get CLOSED() {
		return this.#virtual.CLOSED;
	}

	get binaryType() {
		return this.#virtual.binaryType;
	}

	set binaryType(value: "arraybuffer" | "blob") {
		this.#virtual.binaryType = value;
	}

	get bufferedAmount() {
		return this.#virtual.bufferedAmount;
	}

	get extensions() {
		return this.#virtual.extensions;
	}

	get protocol() {
		return this.#virtual.protocol;
	}

	get url() {
		return this.#virtual.url;
	}

	get onopen() {
		return this.#virtual.onopen;
	}

	set onopen(value) {
		this.#virtual.onopen = value;
	}

	get onclose() {
		return this.#virtual.onclose;
	}

	set onclose(value) {
		this.#virtual.onclose = value;
	}

	get onerror() {
		return this.#virtual.onerror;
	}

	set onerror(value) {
		this.#virtual.onerror = value;
	}

	get onmessage() {
		return this.#virtual.onmessage;
	}

	set onmessage(value) {
		this.#virtual.onmessage = value;
	}

	send(data: string | ArrayBuffer | ArrayBufferView): void {
		this.#virtual.send(data);
	}

	close(code?: number, reason?: string): void {
		this.#virtual.close(code, reason);
	}

	addEventListener(
		type: string,
		listener: (event: any) => void | Promise<void>,
	): void {
		this.#virtual.addEventListener(type, listener);
	}

	removeEventListener(
		type: string,
		listener: (event: any) => void | Promise<void>,
	): void {
		this.#virtual.removeEventListener(type, listener);
	}

	dispatchEvent(event: {
		type: string;
		target?: unknown;
		currentTarget?: unknown;
	}): boolean {
		return this.#virtual.dispatchEvent(event);
	}
}

type TrackedWebSocketListener = (event: any) => void | Promise<void>;

class TrackedWebSocketHandleAdapter implements UniversalWebSocket {
	#ctx: ActorContextHandleAdapter;
	#inner: UniversalWebSocket;
	#listeners = new Map<string, TrackedWebSocketListener[]>();
	#onopen: ((event: RivetEvent) => void | Promise<void>) | null = null;
	#onclose: ((event: RivetCloseEvent) => void | Promise<void>) | null = null;
	#onerror: ((event: RivetEvent) => void | Promise<void>) | null = null;
	#onmessage: ((event: RivetMessageEvent) => void | Promise<void>) | null =
		null;

	constructor(ctx: ActorContextHandleAdapter, inner: UniversalWebSocket) {
		this.#ctx = ctx;
		this.#inner = inner;

		inner.addEventListener("open", (event) => {
			this.#dispatch("open", this.#createEvent("open", event));
		});
		inner.addEventListener("message", (event) => {
			this.#dispatch("message", this.#createEvent("message", event));
		});
		inner.addEventListener("close", (event) => {
			this.#dispatch("close", this.#createEvent("close", event));
		});
		inner.addEventListener("error", (event) => {
			this.#dispatch("error", this.#createEvent("error", event));
		});
	}

	get CONNECTING(): 0 {
		return this.#inner.CONNECTING;
	}

	get OPEN(): 1 {
		return this.#inner.OPEN;
	}

	get CLOSING(): 2 {
		return this.#inner.CLOSING;
	}

	get CLOSED(): 3 {
		return this.#inner.CLOSED;
	}

	get readyState(): 0 | 1 | 2 | 3 {
		return this.#inner.readyState;
	}

	get binaryType(): "arraybuffer" | "blob" {
		return this.#inner.binaryType;
	}

	set binaryType(value: "arraybuffer" | "blob") {
		this.#inner.binaryType = value;
	}

	get bufferedAmount(): number {
		return this.#inner.bufferedAmount;
	}

	get extensions(): string {
		return this.#inner.extensions;
	}

	get protocol(): string {
		return this.#inner.protocol;
	}

	get url(): string {
		return this.#inner.url;
	}

	send(data: string | ArrayBufferLike | Blob | ArrayBufferView): void {
		this.#inner.send(data);
	}

	close(code?: number, reason?: string): void {
		this.#inner.close(code, reason);
	}

	addEventListener(type: string, listener: TrackedWebSocketListener): void {
		let listeners = this.#listeners.get(type);
		if (!listeners) {
			listeners = [];
			this.#listeners.set(type, listeners);
		}
		listeners.push(listener);
	}

	removeEventListener(
		type: string,
		listener: TrackedWebSocketListener,
	): void {
		const listeners = this.#listeners.get(type);
		if (!listeners) {
			return;
		}

		const index = listeners.indexOf(listener);
		if (index !== -1) {
			listeners.splice(index, 1);
		}
	}

	dispatchEvent(event: RivetEvent): boolean {
		this.#dispatch(event.type, this.#createEvent(event.type, event));
		return true;
	}

	get onopen(): ((event: RivetEvent) => void | Promise<void>) | null {
		return this.#onopen;
	}

	set onopen(fn: ((event: RivetEvent) => void | Promise<void>) | null) {
		this.#onopen = fn;
	}

	get onclose(): ((event: RivetCloseEvent) => void | Promise<void>) | null {
		return this.#onclose;
	}

	set onclose(fn: ((event: RivetCloseEvent) => void | Promise<void>) | null) {
		this.#onclose = fn;
	}

	get onerror(): ((event: RivetEvent) => void | Promise<void>) | null {
		return this.#onerror;
	}

	set onerror(fn: ((event: RivetEvent) => void | Promise<void>) | null) {
		this.#onerror = fn;
	}

	get onmessage():
		| ((event: RivetMessageEvent) => void | Promise<void>)
		| null {
		return this.#onmessage;
	}

	set onmessage(fn:
		| ((event: RivetMessageEvent) => void | Promise<void>)
		| null,) {
		this.#onmessage = fn;
	}

	#createEvent(type: string, event: any): any {
		switch (type) {
			case "message":
				return {
					type,
					data: event.data,
					rivetMessageIndex: event.rivetMessageIndex,
					target: this,
					currentTarget: this,
				} satisfies RivetMessageEvent;
			case "close":
				return {
					type,
					code: event.code,
					reason: event.reason,
					wasClean: event.wasClean,
					target: this,
					currentTarget: this,
				} satisfies RivetCloseEvent;
			default:
				return {
					type,
					target: this,
					currentTarget: this,
					...(event.message !== undefined
						? { message: event.message }
						: {}),
					...(event.error !== undefined
						? { error: event.error }
						: {}),
				} satisfies RivetEvent;
		}
	}

	#dispatch(type: string, event: any): void {
		const listeners = this.#listeners.get(type);
		if (listeners && listeners.length > 0) {
			for (const listener of [...listeners]) {
				this.#callHandler(type, listener, event);
			}
		}

		switch (type) {
			case "open":
				if (this.#onopen) this.#callHandler(type, this.#onopen, event);
				break;
			case "close":
				if (this.#onclose)
					this.#callHandler(type, this.#onclose, event);
				break;
			case "error":
				if (this.#onerror)
					this.#callHandler(type, this.#onerror, event);
				break;
			case "message":
				if (this.#onmessage)
					this.#callHandler(type, this.#onmessage, event);
				break;
		}
	}

	#callHandler(
		type: string,
		handler: TrackedWebSocketListener,
		event: any,
	): void {
		try {
			const result = handler(event);
			if (!this.#isPromiseLike(result)) {
				return;
			}
			const callbackRegionId = this.#ctx.beginWebSocketCallback();
			this.#ctx.waitUntil(
				Promise.resolve(result)
					.catch((error) => {
						logger().error({
							msg: "async websocket handler failed",
							eventType: type,
							error,
						});
					})
					.finally(() => {
						this.#ctx.endWebSocketCallback(callbackRegionId);
					})
					.then(() => null),
			);
		} catch (error) {
			logger().error({
				msg: "websocket handler failed",
				eventType: type,
				error,
			});
		}
	}

	#isPromiseLike(value: unknown): value is PromiseLike<void> {
		return (
			typeof value === "object" &&
			value !== null &&
			"then" in value &&
			typeof value.then === "function"
		);
	}
}

class NativeConnectionMap implements ReadonlyMap<string, NativeConnAdapter> {
	#runtime: CoreRuntime;
	#ctx: ActorContextHandle;
	#schemas: NativeValidationConfig;
	#assertCanMutateState: () => void;

	constructor(
		runtime: CoreRuntime,
		ctx: ActorContextHandle,
		schemas: NativeValidationConfig,
		assertCanMutateState: () => void,
	) {
		this.#runtime = runtime;
		this.#ctx = ctx;
		this.#schemas = schemas;
		this.#assertCanMutateState = assertCanMutateState;
	}

	#connToAdapter(conn: ConnHandle): NativeConnAdapter {
		return new NativeConnAdapter(
			this.#runtime,
			conn,
			this.#schemas,
			this.#ctx,
			(connId) =>
				callNativeSync(() =>
					this.#runtime.actorQueueHibernationRemoval(
						this.#ctx,
						connId,
					),
				),
			this.#assertCanMutateState,
		);
	}

	get size(): number {
		return callNativeSync(() => this.#runtime.actorConns(this.#ctx)).length;
	}

	get(key: string): NativeConnAdapter | undefined {
		const conns = callNativeSync(() => this.#runtime.actorConns(this.#ctx));
		const conn = conns.find((c) => this.#runtime.connId(c) === key);
		if (!conn) return undefined;
		return this.#connToAdapter(conn);
	}

	has(key: string): boolean {
		const conns = callNativeSync(() => this.#runtime.actorConns(this.#ctx));
		return conns.some((c) => this.#runtime.connId(c) === key);
	}

	keys(): MapIterator<string> {
		const conns = callNativeSync(() => this.#runtime.actorConns(this.#ctx));
		return conns
			.map((c) => this.#runtime.connId(c))
			[Symbol.iterator]() satisfies MapIterator<string>;
	}

	values(): MapIterator<NativeConnAdapter> {
		const conns = callNativeSync(() => this.#runtime.actorConns(this.#ctx));
		return conns
			.map((c) => this.#connToAdapter(c))
			[Symbol.iterator]() satisfies MapIterator<NativeConnAdapter>;
	}

	entries(): MapIterator<[string, NativeConnAdapter]> {
		const conns = callNativeSync(() => this.#runtime.actorConns(this.#ctx));
		return conns
			.map(
				(c) =>
					[this.#runtime.connId(c), this.#connToAdapter(c)] as [
						string,
						NativeConnAdapter,
					],
			)
			[Symbol.iterator]() satisfies MapIterator<
			[string, NativeConnAdapter]
		>;
	}

	forEach(
		callback: (
			value: NativeConnAdapter,
			key: string,
			map: ReadonlyMap<string, NativeConnAdapter>,
		) => void,
		thisArg?: unknown,
	): void {
		const conns = callNativeSync(() => this.#runtime.actorConns(this.#ctx));
		for (const conn of conns) {
			const id = this.#runtime.connId(conn);
			callback.call(thisArg, this.#connToAdapter(conn), id, this);
		}
	}

	[Symbol.iterator](): MapIterator<[string, NativeConnAdapter]> {
		return this.entries();
	}

	readonly [Symbol.toStringTag] = "NativeConnectionMap";
}

export class ActorContextHandleAdapter {
	#runtime: CoreRuntime;
	#ctx: ActorContextHandle;
	#schemas: NativeValidationConfig;
	#abortSignal?: AbortSignal;
	#abortSignalCleanup?: () => void;
	#client?: AnyClient;
	#clientFactory?: () => AnyClient;
	#connMap?: NativeConnectionMap;
	#cron?: NativeCronAdapter;
	#databaseProvider?: Exclude<AnyDatabaseProvider, undefined>;
	#db?: unknown;
	#dispatchCancelToken?: CancellationTokenHandle;
	#kv?: NativeKvAdapter;
	#log?: ActorLogger;
	#queue?: NativeQueueAdapter;
	#request?: Request;
	#schedule?: NativeScheduleAdapter;
	#run?: { setWakeAt(timestamp: number | null): Promise<void> };
	#runHandlerConfigured: boolean;
	#onStateChange?: NativeOnStateChangeHandler;
	#stateEnabled: boolean;
	#stateProxy?: unknown;
	#stateProxyTarget?: unknown;
	readonly #stateTransactionOwner = Symbol("actor-state-transaction-owner");

	constructor(
		runtime: CoreRuntime,
		ctx: ActorContextHandle,
		clientFactory?: () => AnyClient,
		schemas: NativeValidationConfig = {},
		databaseProvider?: AnyDatabaseProvider,
		request?: Request,
		stateEnabled = true,
		onStateChange?: NativeOnStateChangeHandler,
		dispatchCancelToken?: CancellationTokenHandle,
		runHandlerConfigured = false,
	) {
		this.#runtime = runtime;
		this.#ctx = ctx;
		this.#clientFactory = clientFactory;
		this.#schemas = schemas;
		this.#dispatchCancelToken = dispatchCancelToken;
		this.#runHandlerConfigured = runHandlerConfigured;
		this.#onStateChange = onStateChange;
		this.#stateEnabled = stateEnabled;
		if (databaseProvider) {
			this.#databaseProvider = databaseProvider;
		}
		this.#request = request;
		(
			this as ActorContextHandleAdapter & {
				[ACTOR_CONTEXT_INTERNAL_SYMBOL]?: unknown;
			}
		)[ACTOR_CONTEXT_INTERNAL_SYMBOL] = new NativeWorkflowRuntimeAdapter(
			this,
		);
	}

	get kv() {
		if (!this.#kv) {
			this.#kv = new NativeKvAdapter(this.#runtime, this.#ctx);
		}
		return this.#kv;
	}

	async actorRuntimeSocket(): Promise<{ path: string }> {
		return await callNative(() =>
			this.#runtime.actorRuntimeSocketProvision(this.#ctx),
		);
	}

	get db() {
		if (!this.#databaseProvider) {
			throw databaseNotConfiguredError();
		}

		if (this.#db) {
			return this.#db;
		}

		const runtimeState = getNativeRuntimeState(this.#runtime, this.#ctx);
		const cachedClient = runtimeState.databaseClient;
		if (cachedClient) {
			this.#db = this.#bindDatabaseClient(cachedClient.client);
			return this.#db;
		}

		throw databaseClientNotReadyError();
	}

	[RAW_STATE_SYMBOL](): unknown {
		if (!this.#stateEnabled) {
			throw stateNotEnabledError();
		}
		return this.#readState();
	}

	get state(): unknown {
		if (!this.#stateEnabled) {
			throw stateNotEnabledError();
		}
		const nextState = this.#readState();
		// Reading `c.state` rebuilds the deep write-through proxy, which
		// allocates fresh on-change caches and rewraps the whole tree. Memoize
		// the proxy keyed on the underlying state object so repeated reads and
		// deep read cascades reuse a single proxy.
		if (
			this.#stateProxy === undefined ||
			this.#stateProxyTarget !== nextState
		) {
			const assertCurrentState = () => {
				this.#assertCanMutateState();
				if (this.#readState() !== nextState) {
					throw stateTransactionConflictError();
				}
			};
			this.#stateProxyTarget = nextState;
			this.#stateProxy = createWriteThroughProxy(
				nextState,
				(nextValue) => {
					assertCurrentState();
					this.#writeState(nextValue, { scheduleSave: true });
				},
				(newValue) => {
					assertCurrentState();
					assertJsonCompatValue(newValue);
				},
			);
		}
		return this.#stateProxy;
	}

	set state(value: unknown) {
		if (!this.#stateEnabled) {
			throw stateNotEnabledError();
		}
		this.#assertCanMutateState();
		const nextValue = unwrapWriteThroughProxy(value);
		assertJsonCompatValue(nextValue);
		this.#writeState(nextValue, { scheduleSave: true });
	}

	initializeState(value: unknown): void {
		if (!this.#stateEnabled) {
			return;
		}
		this.#writeState(unwrapWriteThroughProxy(value), {
			scheduleSave: false,
		});
	}

	get vars(): unknown {
		const runtimeState = getNativeRuntimeState(this.#runtime, this.#ctx);
		if (runtimeState.varsInitialized) {
			return runtimeState.vars;
		}

		runtimeState.varsInitialized = true;
		runtimeState.vars = undefined;
		return undefined;
	}

	set vars(value: unknown) {
		const runtimeState = getNativeRuntimeState(this.#runtime, this.#ctx);
		runtimeState.varsInitialized = true;
		runtimeState.vars = value;
	}

	get queue(): NativeQueueAdapter {
		if (!this.#queue) {
			this.#queue = new NativeQueueAdapter(
				this.#runtime,
				this.#ctx,
				this.#schemas.queues,
			);
		}
		return this.#queue;
	}

	get run() {
		if (!this.#run) {
			this.#run = {
				setWakeAt: async (timestamp: number | null) => {
					if (timestamp !== null && !this.#runHandlerConfigured) {
						throw runHandlerNotConfiguredError();
					}
					if (
						timestamp !== null &&
						(!Number.isSafeInteger(timestamp) || timestamp < 0)
					) {
						throw new TypeError(
							"Run wake timestamp must be a non-negative safe integer or null",
						);
					}
					await callNative(() =>
						this.#runtime.actorSetRunWakeAt(this.#ctx, timestamp),
					);
				},
			};
		}
		return this.#run;
	}

	get schedule(): NativeScheduleAdapter {
		if (!this.#schedule) {
			this.#schedule = new NativeScheduleAdapter(
				this.#runtime,
				this.#ctx,
			);
		}
		return this.#schedule;
	}

	get cron(): NativeCronAdapter {
		if (!this.#cron) {
			this.#cron = new NativeCronAdapter(this.#runtime, this.#ctx);
		}
		return this.#cron;
	}

	get actorId(): string {
		return callNativeSync(() => this.#runtime.actorId(this.#ctx));
	}

	get name(): string {
		return callNativeSync(() => this.#runtime.actorName(this.#ctx));
	}

	get key(): string[] {
		return toActorKey(
			callNativeSync(() => this.#runtime.actorKey(this.#ctx)),
		);
	}

	get region(): string {
		return callNativeSync(() => this.#runtime.actorRegion(this.#ctx));
	}

	get conns(): ReadonlyMap<string, NativeConnAdapter> {
		if (!this.#connMap) {
			this.#connMap = new NativeConnectionMap(
				this.#runtime,
				this.#ctx,
				this.#schemas,
				() => this.#assertCanMutateState(),
			);
		}
		return this.#connMap;
	}

	#invocationTraceContext(): RuntimeActorInvocationTraceContext | undefined {
		return callNativeSync(() =>
			this.#runtime.actorInvocationTraceContext(this.#ctx),
		);
	}

	get log() {
		if (!this.#log) {
			// Actor fields follow the camelCase used by the rest of the
			// TypeScript logs. trace_id and span_id stay snake_case because
			// that is what OpenTelemetry log correlation tooling looks for.
			const invocation = this.#invocationTraceContext();
			this.#log = logger().child({
				actorId: this.actorId,
				actorName: this.name,
				actorKey: this.key,
				...(invocation && { rayId: invocation.rayId }),
				...(invocation?.span && {
					trace_id: invocation.span.traceId,
					span_id: invocation.span.spanId,
				}),
			});
		}
		return this.#log;
	}

	get abortSignal(): AbortSignal {
		if (!this.#abortSignal) {
			const actorSignal = this.#createActorAbortSignal();
			if (this.#dispatchCancelToken === undefined) {
				this.#abortSignal = actorSignal;
			} else {
				const controller = new AbortController();
				let cleanedUp = false;
				const onActorAbort = () => {
					cleanup();
					controller.abort();
				};
				const cleanup = () => {
					if (cleanedUp) {
						return;
					}
					cleanedUp = true;
					actorSignal.removeEventListener("abort", onActorAbort);
					this.#abortSignalCleanup = undefined;
				};

				if (
					actorSignal.aborted ||
					this.#runtime.cancellationTokenAborted(
						this.#dispatchCancelToken,
					)
				) {
					controller.abort();
				} else {
					const dispatchCancelToken = this.#dispatchCancelToken;
					this.#abortSignalCleanup = cleanup;
					actorSignal.addEventListener("abort", onActorAbort, {
						once: true,
					});
					callNativeSync(() =>
						this.#runtime.onCancellationTokenCancelled(
							dispatchCancelToken,
							() => {
								cleanup();
								controller.abort();
							},
						),
					);
				}

				this.#abortSignal = controller.signal;
			}
		}
		return this.#abortSignal;
	}

	get aborted(): boolean {
		return this.abortSignal.aborted;
	}

	get request(): Request | undefined {
		return this.#request;
	}

	private async ensureDatabaseClient(): Promise<unknown> {
		if (!this.#databaseProvider) {
			throw databaseNotConfiguredError();
		}

		if (this.#db) {
			return this.#db;
		}

		const runtimeState = getNativeRuntimeState(this.#runtime, this.#ctx);
		const cachedClient = runtimeState.databaseClient;
		if (cachedClient) {
			this.#db = this.#bindDatabaseClient(cachedClient.client);
			return this.#db;
		}

		const actorId = this.actorId;
		const createdClient = await this.#databaseProvider.createClient({
			actorId,
			kv: {
				batchPut: async (entries) => {
					await this.kv.batchPut(
						entries.map(([key, value]) => [key, value]),
					);
				},
				batchGet: async (keys) => {
					return await this.kv.batchGet([...keys]);
				},
				batchDelete: async (keys) => {
					await this.kv.batchDelete([...keys]);
				},
				deleteRange: async (start, end) => {
					await this.kv.deleteRange(start, end);
				},
			},
			log: {
				debug: (obj) => logger().debug(obj),
			},
			nativeDatabaseProvider: registerNativeStateTransactionOpener(
				{
					open: async (requestedActorId) => {
						void requestedActorId;
						return getOrCreateNativeSqlDatabase(
							this.#runtime,
							this.#ctx,
						);
					},
				},
				async (timeoutMs, context) =>
					await getOrCreateNativeSqlDatabase(
						this.#runtime,
						this.#ctx,
					).beginStateTransaction(timeoutMs, context),
			),
		});
		runtimeState.databaseClient = {
			client: createdClient,
		};
		this.#db = this.#bindDatabaseClient(createdClient);
		return this.#db;
	}

	#bindDatabaseClient(client: unknown): unknown {
		return bindNativeStateTransactionContext(client, {
			enter: async () => await this.#enterStateTransaction(),
			exit: (scope) =>
				this.#exitStateTransaction(
					scope as NativeStateTransactionScope,
				),
		});
	}

	async #enterStateTransaction(): Promise<NativeStateTransactionScope> {
		const actorState = getNativePersistState(this.#runtime, this.#ctx);
		let pendingOwners = actorState.pendingStateTransactionOwners;
		if (!pendingOwners) {
			pendingOwners = new Set();
			actorState.pendingStateTransactionOwners = pendingOwners;
		}
		if (
			actorState.activeStateTransactionOwner ===
				this.#stateTransactionOwner ||
			pendingOwners.has(this.#stateTransactionOwner)
		) {
			throw stateTransactionConflictError();
		}
		pendingOwners.add(this.#stateTransactionOwner);
		const predecessor =
			actorState.stateTransactionTail ?? Promise.resolve();
		let release!: () => void;
		const hold = new Promise<void>((resolve) => {
			release = resolve;
		});
		actorState.stateTransactionTail = predecessor.then(
			async () => await hold,
		);
		try {
			await predecessor;
			pendingOwners.delete(this.#stateTransactionOwner);
			actorState.activeStateTransactionOwner =
				this.#stateTransactionOwner;
			return {
				actorContext: this,
				actorStateBaseline: this.#stateEnabled
					? structuredClone(this.#readState())
					: undefined,
				connectionStateBaselines: new Map(
					callNativeSync(() =>
						this.#runtime.actorConns(this.#ctx),
					).map((conn) => [
						callNativeSync(() => this.#runtime.connId(conn)),
						new Uint8Array(
							callNativeSync(() => this.#runtime.connState(conn)),
						),
					]),
				),
				committed: false,
				release,
			};
		} catch (error) {
			pendingOwners.delete(this.#stateTransactionOwner);
			if (
				actorState.activeStateTransactionOwner ===
				this.#stateTransactionOwner
			) {
				actorState.activeStateTransactionOwner = undefined;
			}
			release();
			throw error;
		}
	}

	#exitStateTransaction(scope: NativeStateTransactionScope): void {
		const actorState = getNativePersistState(this.#runtime, this.#ctx);
		try {
			if (!scope.committed) {
				this.#restoreStateTransactionBaseline(scope);
			}
		} finally {
			if (
				actorState.activeStateTransactionOwner ===
				this.#stateTransactionOwner
			) {
				actorState.activeStateTransactionOwner = undefined;
			}
			if (actorState.stateTransactionSaveDeferred) {
				actorState.stateTransactionSaveDeferred = false;
				this.#scheduleSave();
			}
			scope.release();
		}
	}

	#restoreStateTransactionBaseline(scope: NativeStateTransactionScope): void {
		const actorState = getNativePersistState(this.#runtime, this.#ctx);
		if (actorState.pendingSaveHandle !== undefined) {
			clearImmediate(actorState.pendingSaveHandle);
		}
		actorState.pendingSaveHandle = undefined;
		actorState.saveScheduled = false;

		if (this.#stateEnabled) {
			actorState.state = structuredClone(scope.actorStateBaseline);
		}
		for (const conn of callNativeSync(() =>
			this.#runtime.actorConns(this.#ctx),
		)) {
			const connId = callNativeSync(() => this.#runtime.connId(conn));
			const baseline = scope.connectionStateBaselines.get(connId);
			if (baseline === undefined) {
				continue;
			}
			callNativeSync(() => this.#runtime.connSetState(conn, baseline));
			const cached = actorState.connStates.get(connId);
			if (cached) {
				cached.state = decodeValue(baseline);
			}
		}
		this.#scheduleSave();
	}

	async prepare(): Promise<void> {
		if (!this.#databaseProvider) {
			return;
		}

		await this.ensureDatabaseClient();
	}

	async runDatabaseMigrations(): Promise<void> {
		if (!this.#databaseProvider) {
			return;
		}

		await this.#databaseProvider.onMigrate(
			(await this.ensureDatabaseClient()) as never,
		);
	}

	async closeDatabase(): Promise<void> {
		this.#db = undefined;
		await closeNativeDatabaseClient(this.#runtime, this.#ctx);
		await closeNativeSqlDatabase(this.#runtime, this.#ctx);
	}

	broadcast(name: string, ...args: unknown[]): void {
		const validatedArgs = validateEventArgs(
			this.#schemas.events,
			name,
			args,
		);
		callNativeSync(() =>
			this.#runtime.actorBroadcast(
				this.#ctx,
				name,
				encodeValue(validatedArgs),
			),
		);
	}

	async saveState(opts?: {
		immediate?: boolean;
		maxWait?: number;
	}): Promise<void> {
		if (opts?.immediate) {
			this.#assertCanAwaitStateSave();
			await callNative(() =>
				this.#runtime.actorRequestSaveAndWait(this.#ctx, {
					immediate: true,
				}),
			);
			return;
		}

		if (opts?.maxWait != null) {
			callNativeSync(() =>
				this.#runtime.actorRequestSave(this.#ctx, {
					maxWaitMs: opts.maxWait,
				}),
			);
			return;
		}

		callNativeSync(() =>
			this.#runtime.actorRequestSave(this.#ctx, { immediate: false }),
		);
	}

	async saveStateAndWorkflowBatch(
		writes: Array<{ key: Uint8Array; value: Uint8Array }>,
	): Promise<void> {
		this.#assertCanAwaitStateSave();
		await callNative(() =>
			this.#runtime.actorSaveStateAndWorkflowBatch(this.#ctx, writes),
		);
	}

	#assertCanAwaitStateSave(): void {
		const actorState = getNativePersistState(this.#runtime, this.#ctx);
		if (
			actorState.activeStateTransactionOwner ===
			this.#stateTransactionOwner
		) {
			throw stateTransactionConflictError();
		}
	}

	serializeForTick(reason: SerializeStateReason): RuntimeStateDeltaPayload {
		void reason;
		const actorState = getNativePersistState(this.#runtime, this.#ctx);
		const connHibernationRemoved = callNativeSync(() =>
			this.#runtime.actorTakePendingHibernationChanges(this.#ctx),
		);
		for (const connId of connHibernationRemoved) {
			actorState.connStates.delete(connId);
		}
		const state =
			this.#stateEnabled && this.#readState() !== undefined
				? encodeValue(this.#readState())
				: undefined;
		const connHibernation = callNativeSync(() =>
			this.#runtime.actorDirtyHibernatableConns(this.#ctx),
		).map((conn) => {
			const connId = callNativeSync(() => this.#runtime.connId(conn));
			return {
				connId,
				bytes: callNativeSync(() => this.#runtime.connState(conn)),
			};
		});

		return {
			state,
			connHibernation,
			connHibernationRemoved,
		};
	}

	async restartRunHandler(): Promise<void> {
		await callNative(async () => {
			this.#runtime.actorRestartRunHandler(this.#ctx);
		});
	}

	async setAlarm(timestampMs?: number): Promise<void> {
		await callNative(async () => {
			this.#runtime.actorSetAlarm(this.#ctx, timestampMs);
		});
	}

	keepAwake<T>(promise: Promise<T>): Promise<T> {
		const trackedPromise = Promise.resolve(promise)
			.catch((error) => {
				logger().warn({
					msg: "keepAwake promise rejected",
					error: stringifyError(error),
				});
			})
			.then(() => null);
		try {
			callNativeSync(() =>
				this.#runtime.actorKeepAwake(this.#ctx, trackedPromise),
			);
		} catch (error) {
			if (!isClosedTaskRegistrationError(error)) {
				throw error;
			}
		}
		return promise;
	}

	internalKeepAwake<T>(run: Promise<T> | (() => Promise<T>)): Promise<T> {
		const promise = typeof run === "function" ? run() : run;
		// Track only completion, swallowing the outcome. The real result/error
		// is delivered through the returned `promise`; without a rejection
		// handler here every workflow yield (which rejects with SleepError)
		// would be funneled into the registered task and logged as a spurious
		// "keep_awake promise rejected" warning.
		const trackedPromise = promise.then(
			() => null,
			() => null,
		);
		try {
			callNativeSync(() =>
				this.#runtime.actorRegisterTask(this.#ctx, trackedPromise),
			);
		} catch (error) {
			if (!isClosedTaskRegistrationError(error)) {
				throw error;
			}
		}
		return promise;
	}

	waitUntil(promise: Promise<unknown>): void {
		const trackedPromise = Promise.resolve(promise).then(() => null);
		try {
			callNativeSync(() =>
				this.#runtime.actorWaitUntil(this.#ctx, trackedPromise),
			);
		} catch (error) {
			if (!isClosedTaskRegistrationError(error)) {
				throw error;
			}
		}
	}

	beginWebSocketCallback(): number {
		return callNativeSync(() =>
			this.#runtime.actorBeginWebsocketCallback(this.#ctx),
		);
	}

	endWebSocketCallback(callbackRegionId: number): void {
		callNativeSync(() =>
			this.#runtime.actorEndWebsocketCallback(
				this.#ctx,
				callbackRegionId,
			),
		);
	}

	// Intentionally a no-op. `setPreventSleep` / `preventSleep` are kept on the
	// surface for legacy callers but must not gate sleep here. Callers that
	// need to keep an actor awake should use `keepAwake(promise)` or
	// `waitUntil(promise)` so the native counter machinery in rivetkit-core
	// owns the lifecycle.
	/** @deprecated Use `keepAwake(promise)` or `waitUntil(promise)` instead. */
	setPreventSleep(_preventSleep: boolean): void {
		logger().warn({
			msg: "setPreventSleep is deprecated and is a no-op; use keepAwake(promise) or waitUntil(promise) instead",
		});
	}

	/** @deprecated Use `keepAwake(promise)` or `waitUntil(promise)` instead. */
	get preventSleep(): boolean {
		logger().warn({
			msg: "preventSleep is deprecated and always returns false; use keepAwake(promise) or waitUntil(promise) instead",
		});
		return false;
	}

	sleep(): void {
		this.#flushStateChange();
		callNativeSync(() => this.#runtime.actorSleep(this.#ctx));
	}

	destroy(): void {
		// Call the native destroy first so it can throw `actor/starting` or
		// `actor/stopping` without leaving an unresolved destroyCompletion
		// promise behind in the native runtime state.
		callNativeSync(() => this.#runtime.actorDestroy(this.#ctx));
		markNativeDestroyRequested(this.#runtime, this.#ctx);
	}

	client<T = AnyClient>(): T extends Registry<any> ? Client<T> : T {
		if (!this.#client) {
			if (!this.#clientFactory) {
				throw nativeClientNotConfiguredError();
			}
			this.#client = this.#clientFactory();
		}

		return this.#client as T extends Registry<any> ? Client<T> : T;
	}

	async dispose(): Promise<void> {
		// Flush any save coalesced for this tick before the context is torn
		// down so the request-save and onStateChange always run.
		this.#flushStateChange();
		this.#abortSignalCleanup?.();
	}

	#createActorAbortSignal(): AbortSignal {
		const nativeSignal = callNativeSync(() =>
			this.#runtime.actorAbortSignal(this.#ctx),
		);
		const controller = new AbortController();
		if (nativeSignal.aborted) {
			controller.abort();
		} else {
			nativeSignal.addEventListener("abort", () => controller.abort(), {
				once: true,
			});
		}
		return controller.signal;
	}

	#readState(): unknown {
		const actorState = getNativePersistState(this.#runtime, this.#ctx);
		if (actorState.state === undefined) {
			actorState.state = decodeValue(
				callNativeSync(() => this.#runtime.actorState(this.#ctx)),
			);
		}
		return actorState.state;
	}

	#writeState(
		value: unknown,
		options: {
			scheduleSave: boolean;
		},
	): void {
		const actorState = getNativePersistState(this.#runtime, this.#ctx);
		actorState.state = value;
		if (!options.scheduleSave) {
			return;
		}
		this.#scheduleSave();
	}

	#assertCanMutateState(): void {
		const actorState = getNativePersistState(this.#runtime, this.#ctx);
		if (actorState.isInOnStateChange) {
			throw stateMutationReentrantError();
		}
		if (
			actorState.activeStateTransactionOwner !==
				this.#stateTransactionOwner &&
			(actorState.activeStateTransactionOwner !== undefined ||
				(actorState.pendingStateTransactionOwners?.size ?? 0) > 0)
		) {
			throw stateTransactionConflictError();
		}
	}

	/** @internal */
	assertCanMutateState(): void {
		this.#assertCanMutateState();
	}

	// Coalesce the request-save and onStateChange work to once per event loop
	// tick. A synchronous burst of mutations (for example
	// `Object.assign(c.state, ...)`) would otherwise cross the NAPI boundary and
	// run onStateChange once per field, re-serializing the whole state each time
	// and pinning the event loop on large state.
	#scheduleSave(): void {
		const actorState = getNativePersistState(this.#runtime, this.#ctx);
		if (actorState.saveScheduled) {
			return;
		}
		actorState.saveScheduled = true;
		actorState.pendingSaveHandle = setImmediate(() => {
			this.#flushStateChange();
		});
	}

	#flushStateChange(): void {
		const actorState = getNativePersistState(this.#runtime, this.#ctx);
		if (!actorState.saveScheduled) {
			return;
		}
		if (actorState.activeStateTransactionOwner !== undefined) {
			actorState.saveScheduled = false;
			if (actorState.pendingSaveHandle !== undefined) {
				clearImmediate(actorState.pendingSaveHandle);
				actorState.pendingSaveHandle = undefined;
			}
			actorState.stateTransactionSaveDeferred = true;
			return;
		}
		actorState.saveScheduled = false;
		if (actorState.pendingSaveHandle !== undefined) {
			clearImmediate(actorState.pendingSaveHandle);
			actorState.pendingSaveHandle = undefined;
		}

		callNativeSync(() =>
			this.#runtime.actorRequestSave(this.#ctx, { immediate: false }),
		);

		if (!this.#onStateChange) {
			return;
		}

		actorState.isInOnStateChange = true;
		callNativeSync(() => this.#runtime.actorBeginOnStateChange(this.#ctx));
		let shouldFinish = true;
		try {
			const result = this.#onStateChange(
				this,
				actorState.state,
			) as unknown;
			if (isPromiseLike(result)) {
				shouldFinish = false;
				void Promise.resolve(result)
					.catch((error) => {
						logger().error({
							msg: "error in `onStateChange`",
							error,
						});
					})
					.finally(() => {
						actorState.isInOnStateChange = false;
						callNativeSync(() =>
							this.#runtime.actorEndOnStateChange(this.#ctx),
						);
					});
			}
		} finally {
			if (shouldFinish) {
				actorState.isInOnStateChange = false;
				callNativeSync(() =>
					this.#runtime.actorEndOnStateChange(this.#ctx),
				);
			}
		}
	}
}

type NativeWorkflowQueueMessage = Awaited<
	ReturnType<NativeQueueAdapter["next"]>
>;

class NativeWorkflowRuntimeAdapter {
	#ctx: ActorContextHandleAdapter;
	#completions = new Map<string, (response?: unknown) => Promise<void>>();

	readonly id: string;
	readonly driver: {
		kvBatchGet: (
			actorId: string,
			keys: Uint8Array[],
		) => Promise<Array<Uint8Array | null>>;
		kvBatchPut: (
			actorId: string,
			entries: Array<[Uint8Array, Uint8Array]>,
		) => Promise<void>;
		kvBatchDelete: (actorId: string, keys: Uint8Array[]) => Promise<void>;
		kvDeleteRange: (
			actorId: string,
			start: Uint8Array,
			end: Uint8Array,
		) => Promise<void>;
		kvListPrefix: (
			actorId: string,
			prefix: Uint8Array,
		) => Promise<Array<[Uint8Array, Uint8Array]>>;
		setAlarm: (_actor: unknown, wakeAt: number) => Promise<void>;
	};
	readonly queueManager: {
		enqueue: (name: string, body: unknown) => Promise<unknown>;
		receive: (
			names: string[] | undefined,
			count: number,
			timeout?: number,
			_abortSignal?: AbortSignal,
			completable?: boolean,
		) => Promise<
			Array<{
				id: bigint;
				name: string;
				body: unknown;
				createdAt: number;
				complete?: (response?: unknown) => Promise<void>;
			}>
		>;
		completeMessage: (
			message: {
				id: bigint;
				complete?: (response?: unknown) => Promise<void>;
			},
			response?: unknown,
		) => Promise<void>;
		completeMessageById: (
			messageId: bigint,
			response?: unknown,
		) => Promise<void>;
		waitForNames: (
			names: readonly string[] | undefined,
			abortSignal?: AbortSignal,
		) => Promise<void>;
	};
	readonly stateManager: {
		saveState: (opts?: {
			immediate?: boolean;
			maxWait?: number;
		}) => Promise<void>;
		saveStateAndWorkflowBatch: (
			writes: Array<{ key: Uint8Array; value: Uint8Array }>,
		) => Promise<void>;
	};

	constructor(ctx: ActorContextHandleAdapter) {
		this.#ctx = ctx;
		this.id = ctx.actorId;
		this.driver = {
			kvBatchGet: async (actorId, keys) => {
				this.#assertActorId(actorId);
				return await this.#ctx.kv.batchGet(keys);
			},
			kvBatchPut: async (actorId, entries) => {
				this.#assertActorId(actorId);
				await this.#ctx.kv.batchPut(entries);
			},
			kvBatchDelete: async (actorId, keys) => {
				this.#assertActorId(actorId);
				await this.#ctx.kv.batchDelete(keys);
			},
			kvDeleteRange: async (actorId, start, end) => {
				this.#assertActorId(actorId);
				await this.#ctx.kv.rawDeleteRange(start, end);
			},
			kvListPrefix: async (actorId, prefix) => {
				this.#assertActorId(actorId);
				return await this.#ctx.kv.rawListPrefix(prefix);
			},
			setAlarm: async (_actor, wakeAt) => {
				await this.#ctx.setAlarm(wakeAt);
			},
		};
		this.queueManager = {
			enqueue: async (name, body) => {
				return this.#wrapQueueMessage(
					await this.#ctx.queue.send(name, body),
				);
			},
			receive: async (
				names,
				count,
				timeout,
				_abortSignal,
				completable,
			) => {
				const messages = await this.#ctx.queue.nextBatch({
					names,
					count,
					timeout: timeout ?? 0,
					completable,
				});
				return messages.map((message) =>
					this.#wrapQueueMessage(message),
				);
			},
			completeMessage: async (message, response) => {
				await message.complete?.(response);
				this.#completions.delete(message.id.toString());
			},
			completeMessageById: async (messageId, response) => {
				const complete = this.#completions.get(messageId.toString());
				if (!complete) {
					return;
				}
				await complete(response);
				this.#completions.delete(messageId.toString());
			},
			waitForNames: async (names, abortSignal) => {
				await this.#ctx.queue.waitForNamesAvailable(names ?? [], {
					signal: abortSignal,
				});
			},
		};
		this.stateManager = {
			saveState: async (opts) => {
				await this.#ctx.saveState(opts);
			},
			saveStateAndWorkflowBatch: async (writes) => {
				await this.#ctx.saveStateAndWorkflowBatch(writes);
			},
		};
	}

	async restartRunHandler(): Promise<void> {
		await this.#ctx.restartRunHandler();
	}

	#assertActorId(actorId: string): void {
		if (actorId !== this.id) {
			throw new Error(
				`workflow runtime actor id mismatch: expected ${this.id}, got ${actorId}`,
			);
		}
	}

	#wrapQueueMessage(message: NativeWorkflowQueueMessage) {
		if (!message) {
			throw new Error("native workflow queue message missing");
		}

		const id = BigInt(message.id);
		let complete: ((response?: unknown) => Promise<void>) | undefined;
		if (message.complete) {
			complete = async (response?: unknown) => {
				await message.complete?.(response);
			};
			this.#completions.set(id.toString(), complete);
		}

		return {
			id,
			name: message.name,
			body: message.body,
			createdAt: message.createdAt,
			complete,
		};
	}
}

function withConnContext(
	runtime: CoreRuntime,
	ctx: ActorContextHandle,
	conn: ConnHandle,
	clientFactory?: () => AnyClient,
	schemas: NativeValidationConfig = {},
	databaseProvider?: AnyDatabaseProvider,
	request?: Request,
	stateEnabled = true,
	onStateChange?: NativeOnStateChangeHandler,
	dispatchCancelToken?: CancellationTokenHandle,
	runHandlerConfigured = false,
) {
	const actorContext = new ActorContextHandleAdapter(
		runtime,
		ctx,
		clientFactory,
		schemas,
		databaseProvider,
		request,
		stateEnabled,
		onStateChange,
		dispatchCancelToken,
		runHandlerConfigured,
	);
	return Object.assign(actorContext, {
		conn: new NativeConnAdapter(
			runtime,
			conn,
			schemas,
			ctx,
			(connId) =>
				callNativeSync(() =>
					runtime.actorQueueHibernationRemoval(ctx, connId),
				),
			() => actorContext.assertCanMutateState(),
		),
	});
}

function buildActorConfig(
	definition: AnyActorDefinition,
	registryConfig: RegistryConfig,
	runtimeKind: "napi" | "wasm",
): RuntimeActorConfig {
	const config = definition.config as unknown as Record<string, unknown>;
	const options = (config.options ?? {}) as Record<string, unknown>;
	const canHibernate = options.canHibernateWebSocket;
	const usesRemoteSqlite =
		sqliteBackendForConfig(registryConfig) === "remote";
	const sqliteProfiling = (
		config.db as { sqliteProfiling?: SqliteProfilingOptions } | undefined
	)?.sqliteProfiling;

	return {
		name: options.name as string | undefined,
		icon: options.icon as string | undefined,
		hasDatabase: true,
		remoteSqlite: usesRemoteSqlite,
		sqliteProfiling,
		enableActorRuntimeSocket: options.enableActorRuntimeSocket === true,
		hasState:
			config.state !== undefined ||
			typeof config.createState === "function",
		canHibernateWebsocket:
			typeof canHibernate === "boolean" ? canHibernate : undefined,
		stateSaveIntervalMs: options.stateSaveInterval as number | undefined,
		createVarsTimeoutMs: options.createVarsTimeout as number | undefined,
		createConnStateTimeoutMs: options.createConnStateTimeout as
			| number
			| undefined,
		onBeforeConnectTimeoutMs: options.onBeforeConnectTimeout as
			| number
			| undefined,
		onConnectTimeoutMs: options.onConnectTimeout as number | undefined,
		onMigrateTimeoutMs: options.onMigrateTimeout as number | undefined,
		actionTimeoutMs: options.actionTimeout as number | undefined,
		sleepTimeoutMs: options.sleepTimeout as number | undefined,
		noSleep: options.noSleep as boolean | undefined,
		sleepGracePeriodMs: options.sleepGracePeriod as number | undefined,
		connectionLivenessTimeoutMs: options.connectionLivenessTimeout as
			| number
			| undefined,
		connectionLivenessIntervalMs: options.connectionLivenessInterval as
			| number
			| undefined,
		maxQueueSize: options.maxQueueSize as number | undefined,
		maxSchedules: options.maxSchedules as number | undefined,
		maxQueueMessageSize: options.maxQueueMessageSize as number | undefined,
		maxIncomingMessageSize: registryConfig.maxIncomingMessageSize as
			| number
			| undefined,
		maxOutgoingMessageSize: registryConfig.maxOutgoingMessageSize as
			| number
			| undefined,
		preloadMaxWorkflowBytes: options.preloadMaxWorkflowBytes as
			| number
			| undefined,
		preloadMaxConnectionsBytes: options.preloadMaxConnectionsBytes as
			| number
			| undefined,
		actions: Object.keys(flattenActionHandlers(config.actions))
			.sort()
			.map((name) => ({ name })),
		inspectorTabs: buildInspectorTabs(config.inspector, runtimeKind),
	};
}

function buildInspectorTabs(
	inspector: unknown,
	runtimeKind: "napi" | "wasm",
): Array<RuntimeInspectorTabEntry> | undefined {
	if (!inspector || typeof inspector !== "object") return undefined;
	const tabs = (inspector as { tabs?: unknown }).tabs;
	if (!Array.isArray(tabs) || tabs.length === 0) return undefined;
	return tabs.map((raw) => {
		const entry = raw as {
			id: string;
			label?: string;
			source?: string;
			icon?: string;
			hidden?: boolean;
		};
		if (entry.hidden === true) {
			return { id: entry.id, hidden: true };
		}

		if (runtimeKind === "wasm") {
			if (entry.source !== undefined) {
				logger().warn(
					{
						tabId: entry.id,
						runtimeKind,
					},
					"inspector.tabs[].source is not supported on wasm runners (current host: wasm). Tab descriptors will still appear in the dashboard strip but the tab body will render a not-available placeholder.",
				);
			}
			return {
				id: entry.id,
				label: entry.label,
				icon: entry.icon,
				source: undefined,
			};
		}

		const resolved =
			entry.source !== undefined
				? getNodePath().resolve(entry.source)
				: undefined;
		if (resolved !== undefined) {
			validateInspectorTabSource(entry.id, resolved);
		}
		return {
			id: entry.id,
			label: entry.label,
			icon: entry.icon,
			source: resolved,
		};
	});
}

function validateInspectorTabSource(tabId: string, resolved: string): void {
	if (resolved === getNodePath().parse(resolved).root) {
		throw new Error(
			`inspector.tabs[id="${tabId}"].source resolves to the filesystem root (${resolved}). ` +
				"Point it at the tab's own static-asset directory instead.",
		);
	}
	let stat: import("node:fs").Stats;
	try {
		stat = getNodeFsSync().statSync(resolved);
	} catch (err) {
		const code = (err as NodeJS.ErrnoException)?.code;
		if (code === "ENOENT") {
			throw new Error(
				`inspector.tabs[id="${tabId}"].source (${resolved}) does not exist.`,
			);
		}
		if (code === "EACCES") {
			throw new Error(
				`inspector.tabs[id="${tabId}"].source (${resolved}) is not readable (EACCES).`,
			);
		}
		throw new Error(
			`inspector.tabs[id="${tabId}"].source (${resolved}) could not be stat'd: ${
				(err as Error)?.message ?? err
			}`,
		);
	}
	if (!stat.isDirectory()) {
		throw new Error(
			`inspector.tabs[id="${tabId}"].source (${resolved}) must be a directory, got ${
				stat.isFile() ? "file" : "non-directory"
			}.`,
		);
	}
}

export function buildNativeFactory(
	runtime: CoreRuntime,
	registryConfig: RegistryConfig,
	definition: AnyActorDefinition,
): ActorFactoryHandle {
	const config = definition.config as Record<string, any>;
	const databaseProvider = (config.db ?? defaultDb()) as Exclude<
		AnyDatabaseProvider,
		undefined
	>;
	const actionHandlers = flattenActionHandlers(config.actions);
	const schemaConfig: NativeValidationConfig = {
		actionInputSchemas: flattenActionInputSchemas(
			config.actions,
			config.actionInputSchemas,
		),
		connParamsSchema: config.connParamsSchema,
		events: config.events,
		queues: config.queues,
	};
	const createClient = (ctx: ActorContextHandle) =>
		createClientWithDriver(
			new RemoteEngineControlClient(
				convertRegistryConfigToClientConfig(registryConfig),
			),
			{
				encoding: "bare",
				currentActorInvocation: () =>
					callNativeSync(() =>
						runtime.actorInvocationTraceContext(ctx),
					),
			},
		);
	const run = getRunFunction(config.run);
	const runHandlerCoordinator =
		getRunInspectorKind(config.run) === "workflow"
			? new RunHandlerCoordinator(config.run)
			: undefined;
	const getNativeWorkflowInspector = (ctx: ActorContextHandle) => {
		const actorId = callNativeSync(() => runtime.actorId(ctx));
		const restart = () =>
			callNativeSync(() => runtime.actorRestartRunHandler(ctx));
		return (runHandlerCoordinator?.getInspector(actorId, restart)
			?.workflow ??
			getRunInspectorConfig(config.run, actorId)?.workflow) as
			| NativeWorkflowInspectorConfig
			| undefined;
	};
	const onStateChange =
		typeof config.onStateChange === "function"
			? (actorCtx: ActorContextHandleAdapter, nextState: unknown) => {
					config.onStateChange(actorCtx, nextState);
				}
			: undefined;
	const hasStaticState = "state" in config;
	const hasStaticVars = "vars" in config;
	const hasStaticConnState = Object.hasOwn(config, "connState");
	const hasDynamicConnState = typeof config.createConnState === "function";
	const onSleep =
		typeof config.onSleep === "function" ? config.onSleep : undefined;
	const needsDisconnectCallback =
		typeof config.onDisconnect === "function" ||
		hasStaticConnState ||
		hasDynamicConnState ||
		config.options?.canHibernateWebSocket === true;
	const stateEnabled =
		config.state !== undefined || typeof config.createState === "function";
	const makeActorCtx = (
		ctx: ActorContextHandle,
		request?: Request,
		cancelToken?: CancellationTokenHandle,
	) =>
		new ActorContextHandleAdapter(
			runtime,
			ctx,
			() => createClient(ctx),
			schemaConfig,
			databaseProvider,
			request,
			stateEnabled,
			onStateChange,
			cancelToken,
			run !== undefined,
		);
	const makeConnCtx = (
		ctx: ActorContextHandle,
		conn: ConnHandle,
		request?: Request,
		cancelToken?: CancellationTokenHandle,
	) =>
		withConnContext(
			runtime,
			ctx,
			conn,
			() => createClient(ctx),
			schemaConfig,
			databaseProvider,
			request,
			stateEnabled,
			onStateChange,
			cancelToken,
			run !== undefined,
		);
	const maybeHandleNativeInspectorRequest = async (
		ctx: ActorContextHandle,
		rawRequest: {
			method: string;
			uri: string;
			headers?: Record<string, string>;
			body?: RuntimeBytes;
		},
	): Promise<Response | undefined> => {
		const rawUrl = rawRequest.uri.startsWith("http")
			? rawRequest.uri
			: new URL(rawRequest.uri, "http://127.0.0.1").toString();
		const url = new URL(rawUrl);
		if (!url.pathname.startsWith("/inspector/")) {
			return undefined;
		}

		const jsRequest = buildNativeHttpRequest(rawRequest);
		const jsonResponse = (body: unknown, init?: ResponseInit) =>
			new Response(JSON.stringify(body), {
				status: init?.status ?? 200,
				headers: {
					"Content-Type": "application/json",
					...(init?.headers ?? {}),
				},
			});
		const errorResponse = (error: unknown, status?: number) => {
			const rivetError = toRivetError(error);
			return jsonResponse(
				{
					group: rivetError.group,
					code: rivetError.code,
					message: rivetError.message,
					metadata: rivetError.metadata ?? null,
				},
				{
					status:
						status ??
						rivetError.statusCode ??
						(rivetError.public ? 400 : 500),
				},
			);
		};

		// Tab config is delivered over the inspector WS `Init` message, not
		// HTTP. Only custom-tab assets remain a public per-actor path.
		const isPublicPerActorPath =
			jsRequest.method === "GET" &&
			url.pathname.startsWith("/inspector/custom-tabs/");

		if (!isPublicPerActorPath) {
			try {
				await runtime.actorVerifyInspectorAuth(
					ctx,
					jsRequest.headers
						.get("authorization")
						?.replace(/^Bearer\s+/i, "") ?? null,
				);
			} catch (error) {
				return errorResponse(error, 401);
			}
		}

		const workflowHistory = () =>
			serializeWorkflowHistoryForJson(
				getNativeWorkflowInspector(ctx)?.getHistory() ?? null,
			);
		const workflowState = async () =>
			(await getNativeWorkflowInspector(ctx)?.getState?.()) ?? null;
		const actorCtx = makeActorCtx(ctx, jsRequest);
		const sql = getOrCreateNativeSqlDatabase(runtime, ctx);
		try {
			if (
				url.pathname === "/inspector/state" &&
				jsRequest.method === "GET"
			) {
				return jsonResponse({
					state: stateEnabled ? actorCtx.state : undefined,
					isStateEnabled: stateEnabled,
				});
			}
			if (
				url.pathname === "/inspector/state" &&
				jsRequest.method === "PATCH"
			) {
				const body = (await jsRequest.json()) as { state?: unknown };
				actorCtx.state = body.state;
				await actorCtx.saveState({ immediate: true });
				return jsonResponse({ ok: true });
			}
			if (
				url.pathname === "/inspector/connections" &&
				jsRequest.method === "GET"
			) {
				return jsonResponse({
					connections: Array.from(actorCtx.conns.values()).map(
						(conn) => ({
							type: null,
							id: conn.id,
							details: {
								type: null,
								params: conn.params,
								stateEnabled: true,
								state: conn.state,
								subscriptions: 0,
								isHibernatable: conn.isHibernatable,
							},
						}),
					),
				});
			}
			if (
				url.pathname === "/inspector/rpcs" &&
				jsRequest.method === "GET"
			) {
				return jsonResponse({
					rpcs: Object.keys(actionHandlers).sort(),
				});
			}
			if (
				url.pathname === "/inspector/queue" &&
				jsRequest.method === "GET"
			) {
				const limitParam = url.searchParams.get("limit");
				const parsedLimit = limitParam ? Number(limitParam) : 100;
				const limit =
					Number.isFinite(parsedLimit) && parsedLimit > 0
						? Math.floor(parsedLimit)
						: 100;
				const allMessages =
					await runtime.actorQueueInspectMessages(ctx);
				const truncated = allMessages.length > limit;
				const messages = allMessages.slice(0, limit).map((m) => ({
					id: m.id,
					name: m.name,
					createdAtMs: m.createdAtMs,
				}));
				return jsonResponse({
					size: allMessages.length,
					maxSize: runtime.actorQueueMaxSize(ctx),
					truncated,
					messages,
				});
			}
			if (
				url.pathname === "/inspector/queue" &&
				jsRequest.method === "DELETE"
			) {
				await runtime.actorQueueReset(ctx);
				return jsonResponse({});
			}
			if (
				url.pathname === "/inspector/queue" &&
				jsRequest.method === "POST"
			) {
				let body: { name?: string; body?: unknown };
				try {
					body = (await jsRequest.json()) as {
						name?: string;
						body?: unknown;
					};
				} catch {
					return errorResponse(
						new RivetError(
							"actor",
							"invalid_request",
							"Invalid inspector JSON body",
							{ public: true },
						),
						400,
					);
				}
				const name = body.name ?? "";
				if (name === "") {
					return errorResponse(
						new RivetError(
							"actor",
							"invalid_request",
							"Queue message name must not be empty",
							{ public: true },
						),
						400,
					);
				}
				const cbor = encodeCborCompat(
					(body.body ?? null) as JsonCompatValue,
				);
				const message = await runtime.actorQueueSend(ctx, name, cbor);
				return jsonResponse({
					id: message.id().toString(),
					name: message.name(),
					createdAtMs: message.createdAt(),
				});
			}
			if (
				url.pathname === "/inspector/traces" &&
				jsRequest.method === "GET"
			) {
				return jsonResponse({ otlp: [], clamped: false });
			}
			if (
				url.pathname === "/inspector/workflow-history" &&
				jsRequest.method === "GET"
			) {
				return jsonResponse({
					history: workflowHistory(),
					workflowState: await workflowState(),
					isWorkflowEnabled:
						getNativeWorkflowInspector(ctx) !== undefined,
				});
			}
			if (
				url.pathname === "/inspector/workflow/replay" &&
				jsRequest.method === "POST"
			) {
				try {
					const body = (await jsRequest.json()) as {
						entryId?: string;
					};
					const history = await getNativeWorkflowInspector(
						ctx,
					)?.replayFromStep?.(body.entryId);
					return jsonResponse({
						history: serializeWorkflowHistoryForJson(
							history ?? null,
						),
						workflowState: await workflowState(),
						isWorkflowEnabled:
							getNativeWorkflowInspector(ctx) !== undefined,
					});
				} catch (error) {
					logger().error({
						msg: "error replaying workflow history",
						error,
					});
					return errorResponse(error);
				}
			}
			if (
				url.pathname === "/inspector/database/schema" &&
				jsRequest.method === "GET"
			) {
				const db = sql;
				const tables = queryRows(
					await db.query(
						"SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '__drizzle_%' ORDER BY name",
					),
				) as Array<{ name: string; type: string }>;
				const tableInfos = [];
				for (const table of tables) {
					const quoted = `"${table.name.replace(/"/g, '""')}"`;
					const columns = queryRows(
						await db.query(`PRAGMA table_info(${quoted})`),
					);
					const foreignKeys = queryRows(
						await db.query(`PRAGMA foreign_key_list(${quoted})`),
					);
					const countResult = queryRows(
						await db.query(
							`SELECT COUNT(*) as count FROM ${quoted}`,
						),
					) as Array<{ count?: number }>;
					tableInfos.push({
						table: {
							schema: "main",
							name: table.name,
							type: table.type,
						},
						columns: jsonSafe(columns),
						foreignKeys: jsonSafe(foreignKeys),
						records: countResult[0]?.count ?? 0,
					});
				}
				return jsonResponse({ schema: { tables: tableInfos } });
			}
			if (
				url.pathname === "/inspector/database/rows" &&
				jsRequest.method === "GET"
			) {
				const table = url.searchParams.get("table");
				if (!table) {
					return jsonResponse(
						{ error: "Missing required table query parameter" },
						{ status: 400 },
					);
				}
				const limit = Number.parseInt(
					url.searchParams.get("limit") ?? "100",
					10,
				);
				const offset = Number.parseInt(
					url.searchParams.get("offset") ?? "0",
					10,
				);
				const quoted = `"${table.replace(/"/g, '""')}"`;
				const rows = queryRows(
					await sql.query(
						`SELECT * FROM ${quoted} LIMIT ? OFFSET ?`,
						[
							Math.max(0, Math.min(limit, 500)),
							Math.max(0, offset),
						],
					),
				);
				return jsonResponse({ rows: jsonSafe(rows) });
			}
			if (
				url.pathname === "/inspector/database/execute" &&
				jsRequest.method === "POST"
			) {
				const body = (await jsRequest.json()) as {
					sql?: unknown;
					args?: unknown;
					properties?: unknown;
				};
				if (typeof body.sql !== "string" || body.sql.trim() === "") {
					return jsonResponse(
						{ error: "sql is required" },
						{ status: 400 },
					);
				}
				if (
					Array.isArray(body.args) &&
					body.properties &&
					typeof body.properties === "object"
				) {
					return jsonResponse(
						{ error: "use either args or properties, not both" },
						{ status: 400 },
					);
				}
				if (
					body.properties &&
					typeof body.properties === "object" &&
					!Array.isArray(body.properties)
				) {
					const bindings = normalizeSqlitePropertyBindings(
						body.properties as Record<string, unknown>,
					);
					const rows = queryRows(await sql.query(body.sql, bindings));
					return jsonResponse({ rows: jsonSafe(rows) });
				}
				const args = Array.isArray(body.args) ? body.args : [];
				const rows = queryRows(await sql.query(body.sql, args));
				return jsonResponse({ rows: jsonSafe(rows) });
			}
			if (
				url.pathname === "/inspector/summary" &&
				jsRequest.method === "GET"
			) {
				const inspectorSnapshot = callNativeSync(() =>
					runtime.actorInspectorSnapshot(ctx),
				);
				return jsonResponse({
					state: stateEnabled ? actorCtx.state : undefined,
					connections: Array.from(actorCtx.conns.values()).map(
						(conn) => ({
							type: null,
							id: conn.id,
							details: {
								type: null,
								params: conn.params,
								stateEnabled: true,
								state: conn.state,
								subscriptions: 0,
								isHibernatable: conn.isHibernatable,
							},
						}),
					),
					rpcs: Object.keys(actionHandlers).sort(),
					queueSize: inspectorSnapshot.queueSize,
					isStateEnabled: stateEnabled,
					isDatabaseEnabled: true,
					isWorkflowEnabled:
						getNativeWorkflowInspector(ctx) !== undefined,
					workflowState: await workflowState(),
					workflowHistory: workflowHistory(),
				});
			}
			if (
				jsRequest.method === "POST" &&
				url.pathname.startsWith("/inspector/action/")
			) {
				const actionName = url.pathname.replace(
					"/inspector/action/",
					"",
				);
				const action = actionHandlers[actionName];
				if (!action) {
					return errorResponse(
						new RivetError(
							"action",
							"action_not_found",
							`Action ${actionName} not found`,
						),
						404,
					);
				}
				const body = (await jsRequest.json()) as {
					args?: unknown;
					properties?: unknown;
				};
				if (body.args !== undefined && body.properties !== undefined) {
					return jsonResponse(
						{ error: "use either args or properties, not both" },
						{ status: 400 },
					);
				}
				if (
					body.properties !== undefined &&
					(body.properties === null ||
						typeof body.properties !== "object" ||
						Array.isArray(body.properties))
				) {
					return jsonResponse(
						{ error: "properties must be an object" },
						{ status: 400 },
					);
				}
				const args =
					body.properties !== undefined
						? [body.properties]
						: normalizeArgs(body.args);
				try {
					const output = await action(
						actorCtx,
						...validateActionArgs(
							schemaConfig.actionInputSchemas,
							actionName,
							args,
						),
					);
					return jsonResponse({ output });
				} catch (error) {
					logger().error({
						msg: "Error handling inspector action request",
						error,
					});
					return errorResponse(error);
				}
			}

			return jsonResponse(
				{
					group: "actor",
					code: "not_found",
					message: "Inspector route was not found",
					metadata: null,
				},
				{ status: 404 },
			);
		} catch (error) {
			logger().error({
				msg: "Error handling inspector request",
				error,
			});
			return errorResponse(error);
		} finally {
			await actorCtx.dispose();
		}
	};
	const callbacks = {
		createState:
			hasStaticState || typeof config.createState === "function"
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: {
								ctx: ActorContextHandle;
								input?: RuntimeBytes;
							},
						): Promise<RuntimeBytes> => {
							const { ctx, input } = unwrapTsfnPayload(
								error,
								payload,
							);
							const actorCtx = makeActorCtx(ctx);
							try {
								const decodedInput = decodeValue(input);
								const startedAt = performance.now();
								const state = hasStaticState
									? structuredClone(config.state)
									: await config.createState(
											actorCtx,
											decodedInput,
										);
								logger().debug({
									msg: "perf user: createStateMs",
									durationMs: performance.now() - startedAt,
								});
								actorCtx.initializeState(state);
								return encodeValue(state);
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		onCreate:
			typeof config.onCreate === "function"
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: {
								ctx: ActorContextHandle;
								input?: RuntimeBytes;
							},
						): Promise<void> => {
							const { ctx, input } = unwrapTsfnPayload(
								error,
								payload,
							);
							const actorCtx = makeActorCtx(ctx);
							try {
								await config.onCreate(
									actorCtx,
									decodeValue(input),
								);
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		createVars:
			hasStaticVars || typeof config.createVars === "function"
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: { ctx: ActorContextHandle },
						): Promise<void> => {
							const { ctx } = unwrapTsfnPayload(error, payload);
							const actorCtx = makeActorCtx(ctx);
							try {
								const startedAt = performance.now();
								const vars = hasStaticVars
									? structuredClone(config.vars)
									: await config.createVars(
											actorCtx,
											undefined,
										);
								logger().debug({
									msg: "perf user: createVarsMs",
									durationMs: performance.now() - startedAt,
								});
								actorCtx.vars = vars;
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		onMigrate:
			typeof config.onMigrate === "function" ||
			databaseProvider !== undefined
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: {
								ctx: ActorContextHandle;
								isNew: boolean;
							},
						) => {
							const { ctx, isNew } = unwrapTsfnPayload(
								error,
								payload,
							);
							const actorCtx = makeActorCtx(ctx);
							try {
								if (!isNew) {
									await actorCtx.closeDatabase();
								}
								await actorCtx.runDatabaseMigrations();
								if (typeof config.onMigrate === "function") {
									await config.onMigrate(actorCtx, isNew);
								}
							} catch (error) {
								await actorCtx.closeDatabase();
								throw error;
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		onWake:
			typeof config.onWake === "function"
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: { ctx: ActorContextHandle },
						) => {
							const { ctx } = unwrapTsfnPayload(error, payload);
							const actorCtx = makeActorCtx(ctx);
							try {
								const startedAt = performance.now();
								await config.onWake(actorCtx);
								logger().debug({
									msg: "perf user: onWakeMs",
									durationMs: performance.now() - startedAt,
								});
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		onBeforeActorStart:
			typeof config.onBeforeActorStart === "function"
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: { ctx: ActorContextHandle },
						) => {
							const { ctx } = unwrapTsfnPayload(error, payload);
							const actorCtx = makeActorCtx(ctx);
							try {
								await config.onBeforeActorStart(actorCtx);
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		onSleep: wrapNativeCallback(
			async (error: unknown, payload: { ctx: ActorContextHandle }) => {
				const { ctx } = unwrapTsfnPayload(error, payload);
				const actorCtx = makeActorCtx(ctx);
				const actorId = callNativeSync(() => runtime.actorId(ctx));
				// TODO: Move this save hook into cleanupNativeSleepRuntimeState
				// so immediate and deferred sleep cleanup share one save-state
				// path instead of passing a callback through cleanup.
				const saveActorState = async () => {
					if (runtime.kind === "wasm") {
						// Wasm cannot use the native context save helper here because
						// the runtime owns the serialized state handoff.
						await runtime.actorSaveState(
							ctx,
							actorCtx.serializeForTick("save"),
						);
					} else {
						await actorCtx.saveState({
							immediate: true,
						});
					}
				};
				try {
					if (onSleep) {
						await onSleep(actorCtx);
					}
					await saveActorState();
				} finally {
					try {
						await cleanupNativeSleepRuntimeState(
							runtime,
							ctx,
							saveActorState,
						);
					} finally {
						runHandlerCoordinator?.destroy(actorId);
						disposeRunInspector(config.run, actorId);
						await actorCtx.dispose();
					}
				}
			},
		),
		onDestroy: wrapNativeCallback(
			async (error: unknown, payload: { ctx: ActorContextHandle }) => {
				const { ctx } = unwrapTsfnPayload(error, payload);
				const actorCtx = makeActorCtx(ctx);
				const actorId = callNativeSync(() => runtime.actorId(ctx));
				// Close run control before user cleanup so replay cannot race actor
				// destruction. Recreating this actor id receives a fresh controller.
				runHandlerCoordinator?.destroy(actorId);
				disposeRunInspector(config.run, actorId);
				try {
					if (typeof config.onDestroy === "function") {
						await config.onDestroy(actorCtx);
					}
				} finally {
					resolveNativeDestroy(runtime, ctx);
					await actorCtx.closeDatabase();
					clearNativeRuntimeState(runtime, ctx);
					await actorCtx.dispose();
				}
			},
		),
		onBeforeConnect:
			typeof config.onBeforeConnect === "function"
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: {
								ctx: ActorContextHandle;
								params: RuntimeBytes;
								request?: {
									method: string;
									uri: string;
									headers?: Record<string, string>;
									body?: RuntimeBytes;
								};
							},
						) => {
							const { ctx, params, request } = unwrapTsfnPayload(
								error,
								payload,
							);
							const actorCtx = makeActorCtx(
								ctx,
								request
									? buildNativeHttpRequest(request)
									: undefined,
							);
							try {
								await config.onBeforeConnect(
									actorCtx,
									validateConnParams(
										schemaConfig.connParamsSchema,
										decodeValue(params),
									),
								);
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		createConnState:
			hasStaticConnState || hasDynamicConnState
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: {
								ctx: ActorContextHandle;
								conn: ConnHandle;
								params: RuntimeBytes;
								request?: {
									method: string;
									uri: string;
									headers?: Record<string, string>;
									body?: RuntimeBytes;
								};
							},
						): Promise<RuntimeBytes> => {
							const { ctx, conn, params, request } =
								unwrapTsfnPayload(error, payload);
							const actorCtx = makeActorCtx(
								ctx,
								request
									? buildNativeHttpRequest(request)
									: undefined,
							);
							const connAdapter = new NativeConnAdapter(
								runtime,
								conn,
								schemaConfig,
								ctx,
								(connId) =>
									callNativeSync(() =>
										runtime.actorQueueHibernationRemoval(
											ctx,
											connId,
										),
									),
								() => actorCtx.assertCanMutateState(),
							);
							try {
								const nextConnState = hasStaticConnState
									? structuredClone(config.connState)
									: await config.createConnState(
											actorCtx,
											validateConnParams(
												schemaConfig.connParamsSchema,
												decodeValue(params),
											),
										);
								connAdapter.initializeState(nextConnState);
								return encodeValue(nextConnState);
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		onConnect:
			typeof config.onConnect === "function"
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: {
								ctx: ActorContextHandle;
								conn: ConnHandle;
								request?: {
									method: string;
									uri: string;
									headers?: Record<string, string>;
									body?: RuntimeBytes;
								};
							},
						) => {
							const { ctx, conn, request } = unwrapTsfnPayload(
								error,
								payload,
							);
							const actorCtx = makeActorCtx(
								ctx,
								request
									? buildNativeHttpRequest(request)
									: undefined,
							);
							const connAdapter = new NativeConnAdapter(
								runtime,
								conn,
								schemaConfig,
								ctx,
								(connId) =>
									callNativeSync(() =>
										runtime.actorQueueHibernationRemoval(
											ctx,
											connId,
										),
									),
								() => actorCtx.assertCanMutateState(),
							);
							try {
								await config.onConnect(
									Object.assign(actorCtx, {
										conn: connAdapter,
									}),
									connAdapter,
								);
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		onDisconnectFinal: needsDisconnectCallback
			? wrapNativeCallback(
					async (
						error: unknown,
						payload: {
							ctx: ActorContextHandle;
							conn: ConnHandle;
						},
					) => {
						const { ctx, conn } = unwrapTsfnPayload(error, payload);
						const actorCtx = makeConnCtx(ctx, conn);
						try {
							// Core already removed the connection; this hook is
							// pure user dispatch.
							if (typeof config.onDisconnect === "function") {
								await config.onDisconnect(
									actorCtx,
									new NativeConnAdapter(
										runtime,
										conn,
										schemaConfig,
										ctx,
										(connId) =>
											callNativeSync(() =>
												runtime.actorQueueHibernationRemoval(
													ctx,
													connId,
												),
											),
										() => actorCtx.assertCanMutateState(),
									),
								);
							}
						} finally {
							await actorCtx.dispose();
						}
					},
				)
			: undefined,
		onBeforeSubscribe:
			schemaConfig.events &&
			Object.values(schemaConfig.events).some(
				(schema) =>
					typeof (schema as { canSubscribe?: unknown })
						.canSubscribe === "function",
			)
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: {
								ctx: ActorContextHandle;
								conn: ConnHandle;
								eventName: string;
							},
						) => {
							const { ctx, conn, eventName } = unwrapTsfnPayload(
								error,
								payload,
							);
							const actorCtx = makeConnCtx(ctx, conn);
							try {
								const canSubscribe = getEventCanSubscribe(
									schemaConfig.events,
									eventName,
								);
								if (!canSubscribe) {
									return;
								}
								const result = await canSubscribe(actorCtx);
								if (typeof result !== "boolean") {
									throw new Error(
										"canSubscribe must return a boolean",
									);
								}
								if (!result) {
									throw forbiddenError();
								}
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		onBeforeActionResponse:
			typeof config.onBeforeActionResponse === "function"
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: {
								ctx: ActorContextHandle;
								name: string;
								args: RuntimeBytes;
								output: RuntimeBytes;
							},
						) => {
							const { ctx, name, args, output } =
								unwrapTsfnPayload(error, payload);
							const actorCtx = makeActorCtx(ctx);
							try {
								return encodeValue(
									await config.onBeforeActionResponse(
										actorCtx,
										name,
										decodeArgs(args),
										decodeValue(output),
									),
								);
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		onRequest: wrapNativeCallback(
			async (
				error: unknown,
				payload: {
					ctx: ActorContextHandle;
					request: {
						method: string;
						uri: string;
						headers?: Record<string, string>;
						body?: RuntimeBytes;
						bodyStream?: NativeHttpRequestBodyStream;
					};
					cancelToken?: CancellationTokenHandle;
					responseBodyStream?: NativeHttpResponseBodyStream;
				},
			) => {
				try {
					const { ctx, request, cancelToken, responseBodyStream } =
						unwrapTsfnPayload(error, payload);
					const inspectorResponse =
						await maybeHandleNativeInspectorRequest(ctx, request);
					if (inspectorResponse) {
						await cancelNativeHttpRequestBody(request.bodyStream);
						return (
							await convertNativeHttpResponse(
								inspectorResponse,
								responseBodyStream,
							)
						).response;
					}

					if (typeof config.onRequest !== "function") {
						await cancelNativeHttpRequestBody(request.bodyStream);
						return (
							await convertNativeHttpResponse(
								new Response(null, { status: 404 }),
								responseBodyStream,
							)
						).response;
					}

					const requestAbortController = new AbortController();
					const handlerRequest = buildNativeHttpRequest({
						...request,
						abortController: requestAbortController,
					});
					const rawConnParams =
						handlerRequest.headers.get(HEADER_CONN_PARAMS);
					let requestCtx:
						| ReturnType<typeof withConnContext>
						| undefined;
					let conn: ConnHandle | undefined;
					let removeRequestAbortListener: (() => void) | undefined;
					let cleanupDeferredToBody = false;
					let cleanedUp = false;
					const cleanupRequest = async () => {
						if (cleanedUp) return;
						cleanedUp = true;
						removeRequestAbortListener?.();
						try {
							await requestCtx?.dispose();
						} finally {
							if (conn) {
								await runtime.connDisconnect(conn);
							}
						}
					};
					try {
						const connParams = validateConnParams(
							schemaConfig.connParamsSchema,
							rawConnParams
								? JSON.parse(rawConnParams)
								: undefined,
						);
						conn = await callNative(() =>
							runtime.actorConnectConn(
								ctx,
								encodeValue(connParams),
								request,
							),
						);
						requestCtx = makeConnCtx(
							ctx,
							conn,
							handlerRequest,
							cancelToken,
						);
						const ctxAbortSignal = requestCtx.abortSignal;
						const abortRequest = () =>
							requestAbortController.abort(ctxAbortSignal.reason);
						if (ctxAbortSignal.aborted) {
							abortRequest();
						} else {
							ctxAbortSignal.addEventListener(
								"abort",
								abortRequest,
								{ once: true },
							);
							removeRequestAbortListener = () =>
								ctxAbortSignal.removeEventListener(
									"abort",
									abortRequest,
								);
						}
						const response = await config.onRequest(
							requestCtx,
							handlerRequest,
						);
						if (!isResponseLike(response)) {
							throw new Error(
								"onRequest handler must return a Response",
							);
						}
						const conversion = await convertNativeHttpResponse(
							response,
							responseBodyStream,
						);
						if (conversion.bodyCompletion) {
							cleanupDeferredToBody = true;
							void conversion.bodyCompletion
								.then(cleanupRequest)
								.catch((cleanupError) => {
									logger().error({
										msg: "failed to clean up native streaming http request",
										error: cleanupError,
									});
								});
						}
						return conversion.response;
					} finally {
						try {
							// Handler completion ends upload ownership even when
							// the Web Request body is locked or partly consumed.
							await cancelNativeHttpRequestBody(
								request.bodyStream,
							);
						} finally {
							if (!cleanupDeferredToBody) {
								await cleanupRequest();
							}
						}
					}
				} catch (error) {
					logger().error({
						msg: "native onRequest failed",
						error,
					});
					throw error;
				}
			},
		),
		onWebSocket:
			typeof config.onWebSocket === "function"
				? wrapNativeCallback(
						async (
							error: unknown,
							payload: {
								ctx: ActorContextHandle;
								conn: ConnHandle;
								ws: WebSocketHandle;
								request?: {
									method: string;
									uri: string;
									headers?: Record<string, string>;
									body?: RuntimeBytes;
								};
							},
						) => {
							const { ctx, conn, ws, request } =
								unwrapTsfnPayload(error, payload);
							const jsRequest = request
								? buildNativeHttpRequest(request)
								: undefined;
							const actorCtx = makeConnCtx(ctx, conn, jsRequest);
							try {
								await config.onWebSocket(
									actorCtx,
									new TrackedWebSocketHandleAdapter(
										actorCtx,
										new NativeWebSocketAdapter(runtime, ws),
									),
								);
							} finally {
								await actorCtx.dispose();
							}
						},
					)
				: undefined,
		run: (() => {
			if (!run) {
				return undefined;
			}

			return wrapNativeCallback(
				async (
					error: unknown,
					payload: { ctx: ActorContextHandle },
				) => {
					const { ctx } = unwrapTsfnPayload(error, payload);
					const actorId = callNativeSync(() => runtime.actorId(ctx));
					const executeRun = async () => {
						const actorCtx = makeActorCtx(ctx);
						try {
							await run(actorCtx);
						} finally {
							await actorCtx.dispose();
						}
					};
					if (runHandlerCoordinator) {
						const outcome = await runHandlerCoordinator.run(
							actorId,
							() =>
								callNativeSync(() =>
									runtime.actorRestartRunHandler(ctx),
								),
							executeRun,
						);
						// Only a start suppressed behind a failed replay must preserve a
						// consumed durable wake. Closed actors must remain closed.
						return outcome !== "suppressed";
					}
					await executeRun();
					return true;
				},
			);
		})(),
		getWorkflowHistory: hasRunInspectorConfig(config.run)
			? wrapNativeCallback(
					async (
						error: unknown,
						payload: { ctx: ActorContextHandle },
					) => {
						const { ctx } = unwrapTsfnPayload(error, payload);
						const history =
							getNativeWorkflowInspector(ctx)?.getHistory();
						return history == null
							? undefined
							: toUint8Array(history);
					},
				)
			: undefined,
		replayWorkflow: hasRunInspectorConfig(config.run)
			? wrapNativeCallback(
					async (
						error: unknown,
						payload: {
							ctx: ActorContextHandle;
							entryId?: string;
						},
					) => {
						const { ctx, entryId } = unwrapTsfnPayload(
							error,
							payload,
						);
						const workflowInspector =
							getNativeWorkflowInspector(ctx);
						if (!workflowInspector?.replayFromStep) {
							return undefined;
						}

						const history =
							(await workflowInspector.replayFromStep(entryId)) ??
							null;
						return history == null
							? undefined
							: toUint8Array(history);
					},
				)
			: undefined,
		actions: Object.fromEntries(
			Object.entries(actionHandlers).map(([name, handler]) => [
				name,
				wrapNativeCallback(
					async (
						error: unknown,
						payload: {
							ctx: ActorContextHandle;
							conn: ConnHandle | null;
							name: string;
							args: RuntimeBytes;
							scheduledFire?: RuntimeScheduledFireInfo;
							cancelToken?: CancellationTokenHandle;
						},
					) => {
						const { ctx, conn, args, scheduledFire, cancelToken } =
							unwrapTsfnPayload(error, payload);
						const actorCtx =
							conn != null
								? makeConnCtx(ctx, conn, undefined, cancelToken)
								: makeActorCtx(ctx, undefined, cancelToken);
						const runAction = async () => {
							try {
								return encodeValue(
									await handler(
										actorCtx,
										...validateActionArgs(
											schemaConfig.actionInputSchemas,
											name,
											decodeArgs(args),
										),
										...(scheduledFire
											? [scheduledFire]
											: []),
									),
								);
							} finally {
								await actorCtx.dispose();
							}
						};
						return await runtime.runWithActorInvocationContext(
							ctx,
							runAction,
						);
					},
				),
			]),
		),
		onQueueSend: wrapNativeCallback(
			async (
				error: unknown,
				payload: {
					ctx: ActorContextHandle;
					conn: ConnHandle;
					request: {
						method: string;
						uri: string;
						headers?: Record<string, string>;
						body?: RuntimeBytes;
					};
					name: string;
					body: RuntimeBytes;
					wait: boolean;
					timeoutMs?: bigint | number;
					cancelToken?: CancellationTokenHandle;
				},
			) => {
				const {
					ctx,
					conn,
					request,
					name,
					body,
					wait,
					timeoutMs,
					cancelToken,
				} = unwrapTsfnPayload(error, payload);
				const jsRequest = buildNativeHttpRequest(request);
				const actorCtx = withConnContext(
					runtime,
					ctx,
					conn,
					() => createClient(ctx),
					schemaConfig,
					databaseProvider,
					jsRequest,
					stateEnabled,
					onStateChange,
					cancelToken,
					run !== undefined,
				);
				try {
					if (
						!schemaConfig.queues ||
						!hasSchemaConfigKey(schemaConfig.queues, name)
					) {
						return { status: "completed" };
					}

					const canPublish = getQueueCanPublish(
						schemaConfig.queues,
						name,
					);
					if (canPublish && !(await canPublish(actorCtx))) {
						throw forbiddenError();
					}

					const decodedBody = decodeValue(body);
					if (wait) {
						try {
							const response =
								await actorCtx.queue.enqueueAndWait(
									name,
									decodedBody,
									{
										timeout:
											timeoutMs === undefined ||
											timeoutMs === null
												? undefined
												: Number(timeoutMs),
									},
								);
							return {
								status: "completed",
								response:
									response === undefined
										? undefined
										: encodeValue(response),
							};
						} catch (error) {
							if (
								(error as { group?: string; code?: string })
									.group === "queue" &&
								(error as { group?: string; code?: string })
									.code === "timed_out"
							) {
								return { status: "timedOut" };
							}
							throw error;
						}
					}

					await actorCtx.queue.send(name, decodedBody);
					return { status: "completed" };
				} finally {
					await actorCtx.dispose();
				}
			},
		),
		serializeState: wrapNativeCallback(
			async (
				error: unknown,
				payload: {
					ctx: ActorContextHandle;
					reason: SerializeStateReason;
				},
			) => {
				const { ctx, reason } = unwrapTsfnPayload(error, payload);
				const actorCtx = makeActorCtx(ctx);
				try {
					return actorCtx.serializeForTick(reason);
				} finally {
					await actorCtx.dispose();
				}
			},
		),
	};

	return runtime.createActorFactory(
		callbacks,
		buildActorConfig(definition, registryConfig, runtime.kind),
	);
}

export async function buildServeConfig(
	config: RegistryConfig,
	resolveServicesBinary = true,
): Promise<RuntimeServeConfig> {
	if (!config.endpoint) {
		throw nativeEndpointNotConfiguredError();
	}

	const serveConfig: RuntimeServeConfig = {
		version: config.envoy.version,
		endpoint: config.endpoint,
		token: config.token,
		namespace: config.namespace,
		poolName: config.envoy.poolName,
		startServices: resolveServicesBinary && config.startServices,
		handleInspectorHttpInRuntime: true,
		serverlessBasePath: config.serverless.basePath,
		serverlessPackageVersion: VERSION,
		serverlessClientEndpoint: config.publicEndpoint,
		serverlessClientNamespace: config.publicNamespace,
		serverlessClientToken: config.publicToken,
		serverlessValidateEndpoint: config.validateServerlessEndpoint,
		serverlessMaxStartPayloadBytes: config.serverless.maxStartPayloadBytes,
	};

	// Always best-effort resolve the npm-installed engine binary and hand its
	// path to the core. The core alone decides whether to actually spawn a local
	// engine (its `should_manage_engine`, based on the endpoint + spawn mode), so
	// JS must not duplicate that decision here. Only JS knows the npm
	// `node_modules` layout, so it resolves the path; if no binary is available
	// (remote-only install, unsupported platform, optional deps skipped), leave
	// it unset and let the core report `engine.binary_unavailable` if it actually
	// needs one.
	try {
		const { getEnginePath } = await loadEngineCli();
		serveConfig.engineBinaryPath = getEnginePath();
	} catch (error) {
		// The npm-installed engine binary could not be resolved. The core still
		// decides whether it needs to spawn a local engine; if it does, it will
		// fail with engine.binary_unavailable (auto-download is off in the napi
		// runtime). Warn so the cause is actionable.
		logger().warn({
			msg: "could not resolve a local engine binary; if a local engine must be spawned it will fail with engine.binary_unavailable — set RIVET_ENGINE_BINARY_PATH or install the @rivetkit/engine-cli platform package",
			error: stringifyError(error),
		});
	}
	if (resolveServicesBinary && config.startServices) {
		try {
			const { getServicesPath } = await loadServices();
			serveConfig.servicesBinaryPath = getServicesPath();
		} catch (error) {
			logger().warn({
				msg: "could not resolve the Services binary; a local engine will fail with services.binary_unavailable — set RIVET_RUN_SERVICES=0 to disable Services or RIVET_SERVICES_BINARY to a local rivet-services binary",
				error: stringifyError(error),
			});
		}
	}
	serveConfig.engineHost = config.engineHost;
	serveConfig.enginePort = config.enginePort;
	if (config.test?.enabled) {
		serveConfig.inspectorTestToken =
			getEnvUniversal("_RIVET_TEST_INSPECTOR_TOKEN") ?? "token";
	}

	return serveConfig;
}

export async function buildRegistryWithRuntime(
	config: RegistryConfig,
	runtime: CoreRuntime,
): Promise<{
	runtime: CoreRuntime;
	registry: RegistryHandle;
	serveConfig: RuntimeServeConfig;
}> {
	if (
		config.test?.enabled &&
		getEnvUniversal("_RIVET_TEST_INSPECTOR_TOKEN") === undefined
	) {
		trySetProcessEnv("_RIVET_TEST_INSPECTOR_TOKEN", "token");
	}

	// Custom inspector tab `source` paths are resolved with node:path while
	// building actor configs below, so the Node modules must be loaded first.
	// Native (napi) runtime only; wasm has no filesystem.
	if (runtime.kind === "napi") {
		importNodeDependencies();
	}

	const registry = runtime.createRegistry();

	for (const [name, definition] of Object.entries(config.use)) {
		runtime.registerActor(
			registry,
			name,
			buildNativeFactory(runtime, config, definition),
		);
	}

	return {
		runtime,
		registry,
		serveConfig: await buildServeConfig(config, runtime.kind === "napi"),
	};
}

export async function buildNativeRegistry(config: RegistryConfig): Promise<{
	runtime: CoreRuntime;
	registry: RegistryHandle;
	serveConfig: RuntimeServeConfig;
}> {
	const { runtime } = await loadNapiRuntime();
	return buildRegistryWithRuntime(
		normalizeRuntimeConfigForKind(config, "native"),
		runtime,
	);
}

export async function buildConfiguredRegistry(config: RegistryConfig): Promise<{
	runtime: CoreRuntime;
	registry: RegistryHandle;
	serveConfig: RuntimeServeConfig;
}> {
	const runtime = await loadConfiguredRuntime(config);
	return buildRegistryWithRuntime(
		normalizeRuntimeConfig(config, runtime),
		runtime,
	);
}
