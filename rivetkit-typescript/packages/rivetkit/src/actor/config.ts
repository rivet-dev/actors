import { z } from "zod/v4";
import type { Client } from "@/client/client";
import type {
	AnyDatabaseProvider,
	InferDatabaseClient,
} from "@/common/database/config";
import type { UniversalWebSocket } from "@/common/websocket-interface";
import type { WorkflowState } from "@/inspector/workflow";
import type { Registry } from "@/registry";
import { flattenActionHandlers } from "./actions";
import type { BaseActorDefinition } from "./definition";
import type {
	EventSchemaConfig,
	InferQueueCompleteMap,
	InferSchemaMap,
	PrimitiveSchema,
	QueueSchemaConfig,
} from "./schema";

export const DEFAULT_SLEEP_GRACE_PERIOD = 15_000;
export const DEFAULT_MAX_ACTIONS = 128;

export const ACTOR_CONTEXT_INTERNAL_SYMBOL = Symbol(
	"rivetkit.actor_context_internal",
);
export const RAW_STATE_SYMBOL = Symbol("rivetkit.raw_state");
export const CONN_DRIVER_SYMBOL = Symbol("rivetkit.conn_driver");
export const CONN_STATE_MANAGER_SYMBOL = Symbol("rivetkit.conn_state_manager");

export interface ActorLogger {
	level: any;
	fatal: any;
	trace: any;
	silent: any;
	msgPrefix: any;
	debug: any;
	info: any;
	warn: any;
	error: any;
	[key: string]: any;
}

type ActorKvValueType = "text" | "arrayBuffer" | "binary";
type ActorKvKeyType = "text" | "binary";
type ActorKvValueTypeMap = {
	text: string;
	arrayBuffer: ArrayBuffer;
	binary: Uint8Array;
};
type ActorKvKeyTypeMap = {
	text: string;
	binary: Uint8Array;
};
type ActorKvValueOptions<T extends ActorKvValueType = "text"> = {
	type?: T;
};
type ActorKvListOptions<
	T extends ActorKvValueType = "text",
	K extends ActorKvKeyType = "text",
> = ActorKvValueOptions<T> & {
	keyType?: K;
	reverse?: boolean;
	limit?: number;
};

type ActorClientFor<T> = T extends Registry<any> ? Client<T> : T;

/**
 * @deprecated Actor KV is deprecated. Use embedded SQLite (`c.db`)
 * or actor state instead.
 */
export interface ActorKv {
	get<T extends ActorKvValueType = "text">(
		key: Uint8Array | string,
		options?: ActorKvValueOptions<T>,
	): Promise<ActorKvValueTypeMap[T] | null>;
	put<T extends ActorKvValueType = "text">(
		key: Uint8Array | string,
		value: Uint8Array | string | ArrayBuffer,
		options?: ActorKvValueOptions<T>,
	): Promise<void>;
	delete(key: Uint8Array | string): Promise<void>;
	batchPut(entries: [Uint8Array, Uint8Array][]): Promise<void>;
	batchGet(keys: Uint8Array[]): Promise<(Uint8Array | null)[]>;
	batchDelete(keys: Uint8Array[]): Promise<void>;
	deleteRange(start: Uint8Array, end: Uint8Array): Promise<void>;
	listPrefix<
		T extends ActorKvValueType = "text",
		K extends ActorKvKeyType = "text",
	>(
		prefix: Uint8Array | string,
		options?: ActorKvListOptions<T, K>,
	): Promise<Array<[ActorKvKeyTypeMap[K], ActorKvValueTypeMap[T]]>>;
	listRange<
		T extends ActorKvValueType = "text",
		K extends ActorKvKeyType = "text",
	>(
		start: Uint8Array | string,
		end: Uint8Array | string,
		options?: ActorKvListOptions<T, K>,
	): Promise<Array<[ActorKvKeyTypeMap[K], ActorKvValueTypeMap[T]]>>;
	list<
		T extends ActorKvValueType = "text",
		K extends ActorKvKeyType = "text",
	>(
		prefix: Uint8Array | string,
		options?: ActorKvListOptions<T, K>,
	): Promise<Array<[ActorKvKeyTypeMap[K], ActorKvValueTypeMap[T]]>>;
	[key: string]: any;
}

export interface ActorSchedule {
	after(
		duration: number,
		action: string,
		...args: unknown[]
	): Promise<string>;
	at(timestamp: number, action: string, ...args: unknown[]): Promise<string>;
	cancel(id: string): Promise<boolean>;
	get(id: string): Promise<ScheduledEventInfo | undefined>;
	list(): Promise<ScheduledEventInfo[]>;
	[key: string]: any;
}

export interface ActorCronSetOptions {
	name: string;
	expression: string;
	action: string;
	args?: unknown[];
	timezone?: string;
	/** Defaults to 100. Set to 0 to disable and clear history. Maximum 1,000. */
	maxHistory?: number;
}

export interface ActorCronEveryOptions {
	name: string;
	/** Fixed interval in milliseconds. Minimum 5,000. */
	interval: number;
	action: string;
	args?: unknown[];
	/** Defaults to 100. Set to 0 to disable and clear history. Maximum 1,000. */
	maxHistory?: number;
}

export interface ActorCron {
	set(options: ActorCronSetOptions): Promise<void>;
	every(options: ActorCronEveryOptions): Promise<void>;
	get(name: string): Promise<CronJobInfo | undefined>;
	list(): Promise<CronJobInfo[]>;
	delete(name: string): Promise<boolean>;
	history(name: string, options?: { limit?: number }): Promise<CronFire[]>;
	[key: string]: any;
}

export interface ScheduledEventInfo {
	id: string;
	action: string;
	args: unknown[];
	runAt: number;
}

export type CronJobInfo = {
	name: string;
	action: string;
	args: unknown[];
	nextRunAt: number;
	lastRunAt?: number;
	maxHistory: number;
} & (
	| { kind: "cron"; expression: string; timezone: string }
	| { kind: "every"; interval: number }
);

export interface ScheduledFireInfo {
	kind: "at" | "cron" | "every";
	id: string;
	name?: string;
	scheduledAt: number;
	firedAt: number;
}

export interface CronFire {
	action: string;
	scheduledAt: number;
	firedAt: number;
	finishedAt?: number;
	result: "running" | "ok" | "error" | "skipped";
	error?: {
		group: string;
		code: string;
		message: string;
		metadata?: unknown;
	};
}

export type QueueMessageOf<Name extends string, Body> = {
	id: number | bigint;
	name: Name;
	body: Body;
	createdAt: number;
	[key: string]: unknown;
};

export type QueueName<TQueues extends QueueSchemaConfig> = keyof TQueues &
	string;
export type QueueFilterName<TQueues extends QueueSchemaConfig> =
	keyof TQueues extends never ? string : QueueName<TQueues>;

type QueueMessageForName<
	TQueues extends QueueSchemaConfig,
	TName extends QueueFilterName<TQueues>,
> = keyof TQueues extends never
	? QueueMessageOf<string, unknown>
	: TName extends QueueName<TQueues>
		? QueueMessageOf<TName, InferSchemaMap<TQueues>[TName]>
		: never;

type QueueCompleteArgs<T> = undefined extends T
	? [response?: T]
	: [response: T];

type QueueCompleteArgsForName<
	TQueues extends QueueSchemaConfig,
	TName extends QueueFilterName<TQueues>,
> = keyof TQueues extends never
	? [response?: unknown]
	: TName extends QueueName<TQueues>
		? [InferQueueCompleteMap<TQueues>[TName]] extends [never]
			? [response?: unknown]
			: QueueCompleteArgs<InferQueueCompleteMap<TQueues>[TName]>
		: [response?: unknown];

type QueueCompletableMessageForName<
	TQueues extends QueueSchemaConfig,
	TName extends QueueFilterName<TQueues>,
> = QueueMessageForName<TQueues, TName> & {
	complete(...args: QueueCompleteArgsForName<TQueues, TName>): Promise<void>;
};

type QueueCompletionResultForName<
	TQueues extends QueueSchemaConfig,
	TName extends QueueFilterName<TQueues>,
> = keyof TQueues extends never
	? unknown | undefined
	: TName extends QueueName<TQueues>
		? InferQueueCompleteMap<TQueues>[TName] | undefined
		: unknown | undefined;

export type QueueResultMessageForName<
	TQueues extends QueueSchemaConfig,
	TName extends QueueFilterName<TQueues>,
	TCompletable extends boolean,
> = TCompletable extends true
	? QueueCompletableMessageForName<TQueues, TName>
	: QueueMessageForName<TQueues, TName>;

export interface QueueNextOptions<
	TName extends string = string,
	TCompletable extends boolean = boolean,
> {
	names?: readonly TName[];
	timeout?: number;
	signal?: AbortSignal;
	completable?: TCompletable;
}

export interface QueueNextBatchOptions<
	TName extends string = string,
	TCompletable extends boolean = boolean,
> {
	names?: readonly TName[];
	count?: number;
	timeout?: number;
	signal?: AbortSignal;
	completable?: TCompletable;
}

export interface QueueWaitOptions<TCompletable extends boolean = boolean> {
	timeout?: number;
	signal?: AbortSignal;
	completable?: TCompletable;
}

export interface QueueEnqueueAndWaitOptions {
	timeout?: number;
	signal?: AbortSignal;
}

export interface QueueTryNextOptions<
	TName extends string = string,
	TCompletable extends boolean = boolean,
> {
	names?: readonly TName[];
	completable?: TCompletable;
}

export interface QueueTryNextBatchOptions<
	TName extends string = string,
	TCompletable extends boolean = boolean,
> {
	names?: readonly TName[];
	count?: number;
	completable?: TCompletable;
}

export interface QueueIterOptions<
	TName extends string = string,
	TCompletable extends boolean = boolean,
> {
	names?: readonly TName[];
	signal?: AbortSignal;
	completable?: TCompletable;
}

export interface ActorQueue<
	TQueues extends QueueSchemaConfig = Record<never, never>,
> {
	send<TName extends QueueFilterName<TQueues>>(
		name: TName,
		body: QueueMessageForName<TQueues, TName>["body"],
	): Promise<any>;
	next<
		const TName extends QueueFilterName<TQueues>,
		const TCompletable extends boolean = false,
	>(opts?: QueueNextOptions<TName, TCompletable>): Promise<any>;
	nextBatch<
		const TName extends QueueFilterName<TQueues>,
		const TCompletable extends boolean = false,
	>(opts?: QueueNextBatchOptions<TName, TCompletable>): Promise<any[]>;
	waitForNames<
		const TName extends QueueFilterName<TQueues>,
		const TCompletable extends boolean = false,
	>(
		names: readonly TName[],
		opts?: QueueWaitOptions<TCompletable>,
	): Promise<any>;
	/** @experimental */
	waitForAvailable<const TName extends QueueFilterName<TQueues>>(
		names?: readonly TName[],
		opts?: Omit<QueueWaitOptions<false>, "completable">,
	): Promise<void>;
	/** @experimental */
	complete<const TName extends QueueFilterName<TQueues>>(
		message: { id: number | bigint; name: TName },
		...args: QueueCompleteArgsForName<TQueues, TName>
	): Promise<void>;
	enqueueAndWait<const TName extends QueueFilterName<TQueues>>(
		name: TName,
		body: QueueMessageForName<TQueues, TName>["body"],
		opts?: QueueEnqueueAndWaitOptions,
	): Promise<QueueCompletionResultForName<TQueues, TName>>;
	tryNext<
		const TName extends QueueFilterName<TQueues>,
		const TCompletable extends boolean = false,
	>(opts?: QueueTryNextOptions<TName, TCompletable>): Promise<any>;
	tryNextBatch<
		const TName extends QueueFilterName<TQueues>,
		const TCompletable extends boolean = false,
	>(opts?: QueueTryNextBatchOptions<TName, TCompletable>): Promise<any[]>;
	iter<
		const TName extends QueueFilterName<TQueues>,
		const TCompletable extends boolean = false,
	>(opts?: QueueIterOptions<TName, TCompletable>): AsyncIterable<any>;
	[key: string]: any;
}

export interface Conn<
	_TState = unknown,
	TConnParams = unknown,
	TConnState = unknown,
	_TVars = unknown,
	_TInput = unknown,
	_TDatabase extends AnyDatabaseProvider = AnyDatabaseProvider,
	_TEvents extends EventSchemaConfig = Record<never, never>,
	_TQueues extends QueueSchemaConfig = Record<never, never>,
> {
	id: string;
	params: TConnParams;
	state: TConnState;
	isHibernatable: boolean;
	send(name: string, ...args: any[]): void;
	disconnect(reason?: string): Promise<void>;
	[key: string]: any;
}

export type AnyConn = Conn<any, any, any, any, any, any, any, any>;

export interface ActorContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> {
	[ACTOR_CONTEXT_INTERNAL_SYMBOL]?: unknown;
	/** Returns the raw unwrapped state without the write-through proxy. */
	[RAW_STATE_SYMBOL](): TState;
	state: TState;
	vars: TVars;
	/**
	 * @deprecated Actor KV is deprecated. Use embedded SQLite (`c.db`)
	 * or actor state instead.
	 */
	readonly kv: ActorKv;
	readonly db: InferDatabaseClient<TDatabase>;
	readonly schedule: ActorSchedule;
	readonly cron: ActorCron;
	readonly queue: ActorQueue<TQueues>;
	/** @experimental */
	readonly run: ActorRun;
	readonly actorId: string;
	readonly name: string;
	readonly key: string[];
	readonly region: string;
	/** Provisions the experimental Actor Runtime Socket for this actor generation. */
	actorRuntimeSocket(): Promise<{ path: string }>;
	readonly conns: Map<
		string,
		Conn<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>
	>;
	readonly log: ActorLogger;
	readonly abortSignal: AbortSignal;
	readonly aborted: boolean;
	readonly request?: Request;
	/** @deprecated No-op. Always returns `false`. Use `keepAwake(promise)` or `waitUntil(promise)` instead. Will be removed in 2.2.0. */
	readonly preventSleep: boolean;
	broadcast(name: string, ...args: any[]): void;
	saveState(opts?: { immediate?: boolean; maxWait?: number }): Promise<void>;
	keepAwake<T>(promise: Promise<T>): Promise<T>;
	waitUntil(promise: Promise<unknown>): void;
	/** @deprecated No-op. Use `keepAwake(promise)` to hold the actor awake for a specific promise. Will be removed in 2.2.0. */
	setPreventSleep(preventSleep: boolean): void;
	sleep(): void;
	destroy(): void;
	client<T = any>(): ActorClientFor<T>;
	[key: string]: any;
}

/** @experimental */
export interface ActorRun {
	/**
	 * @experimental Sets or clears the durable Unix-millisecond deadline that
	 * ensures this actor's run handler is active, starting it if inactive.
	 */
	setWakeAt(timestamp: number | null): Promise<void>;
}

export type ActionContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = ActorContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
> & {
	conn: Conn<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	>;
};

export type BeforeActionResponseContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = ActionContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type BeforeConnectContext<
	TState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = ActorContext<
	TState,
	unknown,
	unknown,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type ConnectContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = ActionContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type CreateConnStateContext<
	TState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = ActorContext<
	TState,
	unknown,
	unknown,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type CreateContext<
	TState,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = ActorContext<
	TState,
	unknown,
	unknown,
	unknown,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type CreateVarsContext<
	TState,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = CreateContext<TState, TInput, TDatabase, TEvents, TQueues>;

export type DestroyContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = ActorContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type DisconnectContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = ActionContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type RequestContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = ActionContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type RunContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = ActorContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type SleepContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = RunContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type StateChangeContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = RunContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type WakeContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = RunContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type MigrateContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = WakeContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type WebSocketContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = ActionContext<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
>;

export type ActorContextOf<
	AD extends BaseActorDefinition<any, any, any, any, any, any, any, any, any>,
> =
	AD extends BaseActorDefinition<
		infer TState,
		infer TConnParams,
		infer TConnState,
		infer TVars,
		infer TInput,
		infer TDatabase,
		infer TEvents,
		infer TQueues,
		any
	>
		? ActorContext<
				TState,
				TConnParams,
				TConnState,
				TVars,
				TInput,
				TDatabase,
				TEvents,
				TQueues
			>
		: never;

export interface ActorTypes<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
> {
	state?: TState;
	connParams?: TConnParams;
	connState?: TConnState;
	vars?: TVars;
	input?: TInput;
	database?: TDatabase;
}

// Helper for validating function types - accepts generic for specific function signatures
const zFunction = <
	T extends (...args: any[]) => any = (...args: unknown[]) => unknown,
>() => z.custom<T>((val) => typeof val === "function");

const zActionTree = z
	.custom<Record<string, unknown>>((value) => {
		if (
			typeof value !== "object" ||
			value === null ||
			Array.isArray(value)
		) {
			return false;
		}
		const prototype = Object.getPrototypeOf(value);
		return prototype === Object.prototype || prototype === null;
	})
	.superRefine((actions, ctx) => {
		try {
			flattenActionHandlers(actions);
		} catch (error) {
			ctx.addIssue({
				code: "custom",
				message:
					error instanceof Error
						? error.message
						: "Invalid action definition",
			});
		}
	});

export type InspectorUnsubscribe = () => void;

/** @experimental */
export interface WorkflowInspectorConfig<THistory = unknown> {
	getHistory: () => THistory | null;
	getState?: () => Promise<WorkflowState | null>;
	onHistoryUpdated?: (
		listener: (history: THistory) => void,
	) => InspectorUnsubscribe;
	replayFromStep?: (entryId?: string) => Promise<THistory | null>;
}

/** @experimental */
export interface RunInspectorConfig<THistory = unknown> {
	workflow?: WorkflowInspectorConfig<THistory>;
}

const WorkflowInspectorConfigSchema = z.object({
	getHistory: zFunction<WorkflowInspectorConfig<unknown>["getHistory"]>(),
	getState:
		zFunction<
			NonNullable<WorkflowInspectorConfig<unknown>["getState"]>
		>().optional(),
	onHistoryUpdated:
		zFunction<
			NonNullable<WorkflowInspectorConfig<unknown>["onHistoryUpdated"]>
		>().optional(),
	replayFromStep:
		zFunction<
			NonNullable<WorkflowInspectorConfig<unknown>["replayFromStep"]>
		>().optional(),
});

const RunInspectorConfigSchema = z
	.object({
		workflow: WorkflowInspectorConfigSchema.optional(),
	})
	.optional();

/**
 * Built-in inspector tabs the dashboard ships. Used to validate
 * `hidden: true` entries and reject custom-tab ids that collide with
 * a built-in.
 */
export const BUILTIN_INSPECTOR_TAB_IDS = [
	"workflow",
	"database",
	"state",
	"queue",
	"schedules",
	"connections",
	"console",
] as const;

export const BuiltinInspectorTabIdSchema = z.enum(BUILTIN_INSPECTOR_TAB_IDS);

// Custom tab id grammar — mirrored in Rust at
// `rivetkit-rust/packages/rivetkit-core/src/inspector/tabs.rs`. Slashes are
// forbidden because the runtime splits `/inspector/custom-tabs/<id>/<rest>`
// on the first `/`.
const CUSTOM_INSPECTOR_TAB_ID_RE = /^[A-Za-z0-9_-]+$/;

export const CustomInspectorTabEntrySchema = z
	.object({
		id: z
			.string()
			.regex(
				CUSTOM_INSPECTOR_TAB_ID_RE,
				"inspector.tabs[].id must contain only letters, digits, underscore, or dash",
			),
		label: z.string().min(1),
		source: z.string().min(1),
		/**
		 * Optional icon id. The dashboard maps strings to glyphs (see its
		 * icon registry); unknown ids fall back to a generic icon.
		 */
		icon: z.string().min(1).optional(),
		hidden: z.literal(false).optional(),
	})
	.strict();

export const HideInspectorTabEntrySchema = z
	.object({
		id: BuiltinInspectorTabIdSchema,
		hidden: z.literal(true),
	})
	.strict();

export const InspectorTabEntrySchema = z.union([
	CustomInspectorTabEntrySchema,
	HideInspectorTabEntrySchema,
]);

export const ActorInspectorConfigSchema = z
	.object({
		tabs: z.array(InspectorTabEntrySchema).default(() => []),
	})
	.strict()
	.refine(
		(data) => {
			const ids = data.tabs.map((t) => t.id);
			return new Set(ids).size === ids.length;
		},
		{ message: "Duplicate id in inspector.tabs", path: ["tabs"] },
	)
	.refine(
		(data) => {
			// A custom entry's id must not collide with a built-in id.
			const builtinSet = new Set(BUILTIN_INSPECTOR_TAB_IDS);
			return data.tabs.every(
				(t) => t.hidden === true || !builtinSet.has(t.id as any),
			);
		},
		{
			message:
				"Custom inspector tab id collides with a built-in (use hidden: true to hide a built-in)",
			path: ["tabs"],
		},
	);

export type ActorInspectorConfig = z.input<typeof ActorInspectorConfigSchema>;

// Schema for run handler with metadata
export const RunConfigSchema = z.object({
	/** Display name for the actor in the Inspector UI. */
	name: z.string().optional(),
	/** Icon for the actor in the Inspector UI. Can be an emoji or FontAwesome icon name. */
	icon: z.string().optional(),
	/** The run handler function. */
	run: zFunction(),
	/** Inspector integration for long-running run handlers. */
	inspector: RunInspectorConfigSchema.optional(),
});
type RunConfigRuntime = z.infer<typeof RunConfigSchema>;
export type RunConfig<
	TState = unknown,
	TConnParams = unknown,
	TConnState = unknown,
	TVars = unknown,
	TInput = unknown,
	TDatabase extends AnyDatabaseProvider = AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> = Omit<RunConfigRuntime, "run"> & {
	run: (
		c: RunContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
	) => void | Promise<void>;
};

type AnyRunConfig = RunConfig<
	any,
	any,
	any,
	any,
	any,
	AnyDatabaseProvider,
	any,
	any
>;

const RUN_FUNCTION_CONFIG_SYMBOL = Symbol.for("rivetkit.run_function_config");

/** @experimental */
export type RunInspectorKind = "workflow";

/** @experimental */
export interface RunWithInactiveOptions {
	/** Restart the run handler before releasing the exclusive gate on success. */
	restartOnSuccess?: boolean;
}

/** @experimental */
export interface RunControl {
	run: {
		/**
		 * Executes only while no run handler is active or waiting to start.
		 *
		 * This is a non-blocking acquisition: concurrent replay/control requests
		 * fail instead of waiting behind an active run.
		 */
		withInactive<T>(
			options: RunWithInactiveOptions,
			callback: () => T | Promise<T>,
		): Promise<T>;
	};
}

/** @experimental */
export interface RunInspectorFactoryContext {
	actorId: string;
	control: RunControl;
}

/** @experimental */
export interface RunInspectorFactoryResult<THistory = unknown> {
	inspector: {
		workflow: Required<WorkflowInspectorConfig<THistory>> & {
			getState: () => Promise<WorkflowState | null>;
		};
	};
	/** Releases actor-specific listeners and encoded history. */
	dispose?: () => void;
}

interface RunHandlerDisplayOptions {
	name?: string;
	icon?: string;
}

/** @experimental */
export type DefineRunHandlerOptions<THistory = unknown> =
	RunHandlerDisplayOptions &
		(
			| {
					inspectorKind?: never;
					createInspector?: never;
			  }
			| {
					/** Static metadata used to install Inspector callbacks at registry build time. */
					inspectorKind: RunInspectorKind;
					/** Creates one Inspector adapter for each live actor. */
					createInspector: (
						context: RunInspectorFactoryContext,
					) => RunInspectorFactoryResult<THistory>;
			  }
		);

interface RunFunctionConfig {
	name?: string;
	icon?: string;
	inspector?: RunInspectorConfig;
	inspectorFactory?: (actor: unknown) => RunInspectorConfig | undefined;
	/** Release any per-actor inspector state held for this actor id. */
	disposeInspector?: (actorId: string) => void;
	inspectorKind?: RunInspectorKind;
	createInspector?: (
		context: RunInspectorFactoryContext,
	) => RunInspectorFactoryResult;
}

type RunFunctionWithConfig = ((...args: any[]) => any) & {
	[RUN_FUNCTION_CONFIG_SYMBOL]?: RunFunctionConfig;
};

/**
 * @experimental Adds supported RivetKit run metadata while preserving the function's exact
 * callable type.
 */
export function defineRunHandler<
	TRun extends (...args: any[]) => any,
	THistory = unknown,
>(run: TRun, options: DefineRunHandlerOptions<THistory>): TRun {
	if (
		(options.inspectorKind === undefined) !==
		(options.createInspector === undefined)
	) {
		throw new TypeError(
			"defineRunHandler requires inspectorKind and createInspector together",
		);
	}

	Object.defineProperty(run, RUN_FUNCTION_CONFIG_SYMBOL, {
		configurable: false,
		enumerable: false,
		writable: false,
		value: {
			name: options.name,
			icon: options.icon,
			inspectorKind: options.inspectorKind,
			createInspector:
				options.createInspector as RunFunctionConfig["createInspector"],
		} satisfies RunFunctionConfig,
	});

	return run;
}

/** @internal */
export function getRunInspectorKind(
	run: ((...args: any[]) => any) | AnyRunConfig | undefined,
): RunInspectorKind | undefined {
	if (!run || typeof run !== "function") return undefined;
	return (run as RunFunctionWithConfig)[RUN_FUNCTION_CONFIG_SYMBOL]
		?.inspectorKind;
}

/** @internal */
export function createRunInspector(
	run: ((...args: any[]) => any) | AnyRunConfig | undefined,
	context: RunInspectorFactoryContext,
): RunInspectorFactoryResult | undefined {
	if (!run || typeof run !== "function") return undefined;
	return (run as RunFunctionWithConfig)[
		RUN_FUNCTION_CONFIG_SYMBOL
	]?.createInspector?.(context);
}

// Run can be either a function or an object with name/icon/run
const zRunHandler = z.union([zFunction(), RunConfigSchema]).optional();

/** Extract the run function from either a function or RunConfig object. */
export function getRunFunction(
	run: ((...args: any[]) => any) | AnyRunConfig | undefined,
): ((...args: any[]) => any) | undefined {
	if (!run) return undefined;
	if (typeof run === "function") return run;
	return run.run;
}

/** Extract run metadata (name/icon) from RunConfig if provided. */
export function getRunMetadata(
	run: ((...args: any[]) => any) | AnyRunConfig | undefined,
): { name?: string; icon?: string } {
	if (!run) return {};
	if (typeof run === "function") {
		const config = (run as RunFunctionWithConfig)[
			RUN_FUNCTION_CONFIG_SYMBOL
		];
		if (!config) return {};
		return { name: config.name, icon: config.icon };
	}
	return { name: run.name, icon: run.icon };
}

/** Extract run inspector configuration if provided. */
export function getRunInspectorConfig(
	run: ((...args: any[]) => any) | AnyRunConfig | undefined,
	actor?: unknown,
): RunInspectorConfig | undefined {
	if (!run) return undefined;
	if (typeof run === "function") {
		const config = (run as RunFunctionWithConfig)[
			RUN_FUNCTION_CONFIG_SYMBOL
		];
		return config?.inspectorFactory
			? config.inspectorFactory(actor)
			: config?.inspector;
	}
	return run.inspector;
}

/** @internal */
export function hasRunInspectorConfig(
	run: ((...args: any[]) => any) | AnyRunConfig | undefined,
): boolean {
	if (!run) return false;
	if (typeof run !== "function") return run.inspector !== undefined;
	const config = (run as RunFunctionWithConfig)[RUN_FUNCTION_CONFIG_SYMBOL];
	return (
		config?.inspectorKind !== undefined ||
		config?.createInspector !== undefined ||
		config?.inspector !== undefined ||
		config?.inspectorFactory !== undefined
	);
}

/** Release per-actor inspector state for a destroyed actor, if the run handler registered a disposer. */
export function disposeRunInspector(
	run: ((...args: any[]) => any) | AnyRunConfig | undefined,
	actorId: string,
): void {
	if (!run || typeof run !== "function") {
		return;
	}
	const config = (run as RunFunctionWithConfig)[RUN_FUNCTION_CONFIG_SYMBOL];
	config?.disposeInspector?.(actorId);
}

// This schema is used to validate the input at runtime. The generic types are defined below in `ActorConfig`.
//
// We don't use Zod generics with `z.custom` because:
// (a) there seems to be a weird bug in either Zod, tsup, or TSC that causese external packages to have different types from `z.infer` than from within the same package and
// (b) it makes the type definitions incredibly difficult to read as opposed to vanilla TypeScript.
const GlobalActorOptionsBaseSchema = z
	.object({
		/** Display name for the actor in the Inspector UI. */
		name: z.string().optional(),
		/** Icon for the actor in the Inspector UI. Can be an emoji or FontAwesome icon name. */
		icon: z.string().optional(),
		/** Maximum number of action handlers that may be defined on this actor. */
		maxActions: z.number().int().nonnegative().default(DEFAULT_MAX_ACTIONS),
		/** Enables the experimental Actor Runtime Socket for this actor. */
		enableActorRuntimeSocket: z.boolean().default(false),
		/**
		 * Can hibernate WebSockets for onWebSocket.
		 *
		 * WebSockets using actions/events are hibernatable by default.
		 *
		 * @experimental
		 **/
		canHibernateWebSocket: z
			.union([z.boolean(), zFunction<(request: Request) => boolean>()])
			.default(false),
	})
	.strict();

export const GlobalActorOptionsSchema = GlobalActorOptionsBaseSchema.prefault(
	() => ({}),
);

export type GlobalActorOptions = z.infer<typeof GlobalActorOptionsSchema>;
export type GlobalActorOptionsInput = z.input<typeof GlobalActorOptionsSchema>;

const InstanceActorOptionsBaseSchema = z
	.object({
		createVarsTimeout: z.number().positive().default(5000),
		createConnStateTimeout: z.number().positive().default(5000),
		onBeforeConnectTimeout: z.number().positive().default(5000),
		onConnectTimeout: z.number().positive().default(5000),
		onMigrateTimeout: z.number().positive().default(30_000),
		sleepGracePeriod: z
			.number()
			.positive()
			.default(DEFAULT_SLEEP_GRACE_PERIOD),
		/** @deprecated `onDestroyTimeout` is folded into `sleepGracePeriod`, which now bounds the entire graceful shutdown window for both sleep and destroy. Will be removed in 2.2.0. */
		onDestroyTimeout: z.number().positive().optional(),
		/** @deprecated `waitUntilTimeout` is folded into `sleepGracePeriod`, which now bounds the entire graceful shutdown window for both sleep and destroy. Will be removed in 2.2.0. */
		waitUntilTimeout: z.number().positive().optional(),
		stateSaveInterval: z.number().positive().default(1_000),
		actionTimeout: z.number().positive().default(60_000),
		connectionLivenessTimeout: z.number().positive().default(2500),
		connectionLivenessInterval: z.number().positive().default(5000),
		/** @deprecated Use `c.keepAwake(promise)` to scope keep-awake to a specific operation, or keep `noSleep` for actors that must stay awake indefinitely. Will be removed in 2.2.0. */
		noSleep: z.boolean().default(false),
		sleepTimeout: z.number().positive().default(30_000),
		maxQueueSize: z.number().positive().default(1000),
		/** Maximum pending one-shot and recurring schedules. */
		maxSchedules: z.number().int().nonnegative().default(1000),
		maxQueueMessageSize: z
			.number()
			.positive()
			.default(64 * 1024),
		/** @deprecated Internal storage moved to SQLite and no longer uses KV preloading, so this option is ignored. Will be removed in 2.2.0. */
		preloadMaxWorkflowBytes: z.number().nonnegative().optional(),
		/** @deprecated Internal storage moved to SQLite and no longer uses KV preloading, so this option is ignored. Will be removed in 2.2.0. */
		preloadMaxConnectionsBytes: z.number().nonnegative().optional(),
	})
	.strict();

export const InstanceActorOptionsSchema =
	InstanceActorOptionsBaseSchema.prefault(() => ({}));

export type InstanceActorOptions = z.infer<typeof InstanceActorOptionsSchema>;
export type InstanceActorOptionsInput = z.input<
	typeof InstanceActorOptionsSchema
>;

export const ActorOptionsSchema = GlobalActorOptionsBaseSchema.extend(
	InstanceActorOptionsBaseSchema.shape,
)
	.strict()
	.prefault(() => ({}));

export type ActorOptions = z.infer<typeof ActorOptionsSchema>;
export type ActorOptionsInput = z.input<typeof ActorOptionsSchema>;

export const ActorConfigSchema = z
	.object({
		onCreate: zFunction().optional(),
		onDestroy: zFunction().optional(),
		onMigrate: zFunction().optional(),
		onWake: zFunction().optional(),
		onSleep: zFunction().optional(),
		run: zRunHandler,
		onStateChange: zFunction().optional(),
		onBeforeConnect: zFunction().optional(),
		onConnect: zFunction().optional(),
		onDisconnect: zFunction().optional(),
		onBeforeActionResponse: zFunction().optional(),
		onRequest: zFunction().optional(),
		onWebSocket: zFunction().optional(),
		actions: zActionTree.default(() => ({})),
		actionInputSchemas: z.record(z.string(), z.any()).optional(),
		connParamsSchema: z.any().optional(),
		events: z.record(z.string(), z.any()).optional(),
		queues: z.record(z.string(), z.any()).optional(),
		state: z.any().optional(),
		createState: zFunction().optional(),
		connState: z.any().optional(),
		createConnState: zFunction().optional(),
		vars: z.any().optional(),
		db: z.any().optional(),
		createVars: zFunction().optional(),
		options: ActorOptionsSchema,
		inspector: ActorInspectorConfigSchema.optional(),
	})
	.strict()
	.refine(
		(data) => !(data.state !== undefined && data.createState !== undefined),
		{
			message: "Cannot define both 'state' and 'createState'",
			path: ["state"],
		},
	)
	.refine(
		(data) =>
			!(
				data.connState !== undefined &&
				data.createConnState !== undefined
			),
		{
			message: "Cannot define both 'connState' and 'createConnState'",
			path: ["connState"],
		},
	)
	.refine(
		(data) => !(data.vars !== undefined && data.createVars !== undefined),
		{
			message: "Cannot define both 'vars' and 'createVars'",
			path: ["vars"],
		},
	)
	.superRefine((data, ctx) => {
		const actionCount = Object.keys(
			flattenActionHandlers(data.actions),
		).length;
		if (actionCount > data.options.maxActions) {
			ctx.addIssue({
				code: "custom",
				message: `Actor defines ${actionCount} actions, but maxActions is ${data.options.maxActions}`,
				path: ["actions"],
			});
		}
	});

// Creates state config
//
// This must have only one or the other or else TState will not be able to be inferred
//
// Data returned from this handler will be available on `c.state`.
type CreateState<
	TState,
	_TConnParams,
	_TConnState,
	_TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig,
	TQueues extends QueueSchemaConfig,
> =
	| { state: TState }
	| {
			createState: (
				c: CreateContext<TState, TInput, TDatabase, TEvents, TQueues>,
				input: TInput,
			) => TState | Promise<TState>;
	  }
	| Record<never, never>;

// Creates connection state config
//
// This must have only one or the other or else TState will not be able to be inferred
//
// Data returned from this handler will be available on `c.conn.state`.
// The pending connection is not visible in `c.conns` until this succeeds.
type CreateConnState<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig,
	TQueues extends QueueSchemaConfig,
> =
	| { connState: TConnState }
	| {
			createConnState: (
				c: CreateConnStateContext<
					TState,
					TVars,
					TInput,
					TDatabase,
					TEvents,
					TQueues
				>,
				params: TConnParams,
			) => TConnState | Promise<TConnState>;
	  }
	| Record<never, never>;

// Creates vars config
//
// This must have only one or the other or else TState will not be able to be inferred
/**
 * @experimental
 */
type CreateVars<
	TState,
	_TConnParams,
	_TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig,
	TQueues extends QueueSchemaConfig,
> =
	| {
			/**
			 * @experimental
			 */
			vars: TVars;
	  }
	| {
			/**
			 * @experimental
			 */
			createVars: (
				c: CreateVarsContext<
					TState,
					TInput,
					TDatabase,
					TEvents,
					TQueues
				>,
				driverCtx: any,
			) => TVars | Promise<TVars>;
	  }
	| Record<never, never>;

export interface Actions<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
> {
	[Action: string]:
		| ((
				c: ActionContext<
					TState,
					TConnParams,
					TConnState,
					TVars,
					TInput,
					TDatabase,
					TEvents,
					TQueues
				>,
				...args: any[]
		  ) => any)
		| Actions<
				TState,
				TConnParams,
				TConnState,
				TVars,
				TInput,
				TDatabase,
				TEvents,
				TQueues
		  >;
}

export interface ActionInputSchemas {
	[Action: string]: PrimitiveSchema | ActionInputSchemas;
}

//export type ActorConfig<TState, TConnParams, TConnState, TVars, TInput, TAuthData> = BaseActorConfig<TState, TConnParams, TConnState, TVars, TInput, TAuthData> &
//	ActorConfigLifecycle<TState, TConnParams, TConnState, TVars, TInput, TAuthData> &
//	CreateState<TState, TConnParams, TConnState, TVars, TInput, TAuthData> &
//	CreateConnState<TState, TConnParams, TConnState, TVars, TInput, TAuthData>;

/**
 * @experimental
 */
export type AuthIntent = "get" | "create" | "connect" | "action" | "message";

interface BaseActorConfig<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig,
	TQueues extends QueueSchemaConfig,
	TActions extends Actions<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	>,
> {
	/**
	 * Called when the actor is first initialized.
	 *
	 * Use this hook to initialize your actor's state.
	 * This is called before any other lifecycle hooks.
	 */
	onCreate?: (
		c: CreateContext<TState, TInput, TDatabase, TEvents, TQueues>,
		input: TInput,
	) => void | Promise<void>;

	/**
	 * Called when the actor is destroyed.
	 */
	onDestroy?: (
		c: DestroyContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
	) => void | Promise<void>;

	/**
	 * Called on every actor start after persisted state loads and before onWake.
	 *
	 * Use this hook for repeatable schema migrations or other startup work that
	 * must run on both first boot and wake.
	 */
	onMigrate?: (
		c: MigrateContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
		isNew: boolean,
	) => void | Promise<void>;

	/**
	 * Called when the actor is started and ready to receive connections and action.
	 *
	 * Use this hook to initialize resources needed for the actor's operation
	 * (timers, external connections, etc.)
	 *
	 * @returns Void or a Promise that resolves when startup is complete
	 */
	onWake?: (
		c: WakeContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
	) => void | Promise<void>;

	/**
	 * Called when the actor is stopping or sleeping.
	 *
	 * Use this hook to clean up resources, save state, or perform
	 * any shutdown operations before the actor sleeps or stops.
	 *
	 * Not supported on all platforms.
	 *
	 * @returns Void or a Promise that resolves when shutdown is complete
	 */
	onSleep?: (
		c: SleepContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
	) => void | Promise<void>;

	/**
	 * Called after the actor starts up. Does not block actor startup.
	 *
	 * Use this for background tasks like:
	 * - Reading from queues in a loop
	 * - Tick loops for periodic work
	 * - Custom workflow logic
	 *
	 * **Important:** The actor may go to sleep at any time during the `run`
	 * handler. Wrap work that must keep the actor awake with
	 * `c.keepAwake(promise)` to block idle sleep and shutdown finalize until
	 * the promise settles, or use `c.waitUntil(promise)` to let the graceful
	 * shutdown window (`sleepGracePeriod`) cover deferred work.
	 *
	 * The handler receives an abort signal via `c.abortSignal` and a
	 * `c.aborted` alias. Use these to gracefully exit when shutdown starts.
	 *
	 * If this handler exits, the actor will follow the normal idle sleep timeout
	 * once it becomes idle.
	 * If this handler throws, the actor logs the error and then sleeps once it
	 * becomes idle.
	 * Call `c.destroy()` explicitly if a run handler should destroy the actor.
	 *
	 * Can be either a function or a RunConfig object with optional name/icon metadata.
	 *
	 * @returns Void or a Promise.
	 */
	run?:
		| ((
				c: RunContext<
					TState,
					TConnParams,
					TConnState,
					TVars,
					TInput,
					TDatabase,
					TEvents,
					TQueues
				>,
		  ) => void | Promise<void>)
		| RunConfig<
				TState,
				TConnParams,
				TConnState,
				TVars,
				TInput,
				TDatabase,
				TEvents,
				TQueues
		  >;

	/**
	 * Called when the actor's state changes.
	 *
	 * Use this hook to react to state changes, such as updating
	 * external systems or triggering events.
	 *
	 * State changes made within this hook will NOT trigger
	 * another onStateChange call, preventing infinite recursion.
	 *
	 * @param newState The updated state
	 */
	onStateChange?: (
		c: StateChangeContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
		newState: TState,
	) => void;

	/**
	 * Called before a client connects to the actor.
	 *
	 * Use this hook to determine if a connection should be accepted
	 * and to validate client-provided parameters. The pending connection
	 * is not visible in `c.conns` while this hook runs.
	 *
	 * @param opts Connection parameters including client-provided data
	 * @returns Void or a Promise.
	 * @throws Throw an error to reject the connection
	 */
	onBeforeConnect?: (
		c: BeforeConnectContext<
			TState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
		params: TConnParams,
	) => void | Promise<void>;

	/**
	 * Called when a client successfully connects to the actor.
	 *
	 * Use this hook to perform actions when a connection is established,
	 * such as sending initial data or updating the actor's state. The
	 * connection is visible in `c.conns` before this hook runs.
	 *
	 * @param conn The connection object
	 * @returns Void or a Promise that resolves when connection handling is complete
	 */
	onConnect?: (
		c: ConnectContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
		conn: Conn<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
	) => void | Promise<void>;

	/**
	 * Called when a client disconnects from the actor.
	 *
	 * Use this hook to clean up resources associated with the connection
	 * or update the actor's state.
	 *
	 * @param conn The connection that is being closed
	 * @returns Void or a Promise that resolves when disconnect handling is complete
	 */
	onDisconnect?: (
		c: DisconnectContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
		conn: Conn<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
	) => void | Promise<void>;

	/**
	 * Called before sending an action response to the client.
	 *
	 * Use this hook to modify or transform the output of an action before it's sent
	 * to the client. This is useful for formatting responses, adding metadata,
	 * or applying transformations to the output.
	 *
	 * @param name The name of the action that was called
	 * @param args The arguments that were passed to the action
	 * @param output The output that will be sent to the client
	 * @returns The modified output to send to the client
	 */
	onBeforeActionResponse?: <Out>(
		c: BeforeActionResponseContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
		name: string,
		args: unknown[],
		output: Out,
	) => Out | Promise<Out>;

	/**
	 * Called when a raw HTTP request is made to the actor.
	 *
	 * This handler receives raw HTTP requests made to `/actors/{actorName}/http/*` endpoints.
	 * Use this hook to handle custom HTTP patterns, REST APIs, or other HTTP-based protocols.
	 *
	 * @param c The request context with access to the connection
	 * @param request The raw HTTP request object
	 * @param opts Additional options
	 * @returns A Response object to send back, or void to continue with default routing
	 */
	onRequest?: (
		c: RequestContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
		request: Request,
	) => Response | Promise<Response>;

	/**
	 * Called when a raw WebSocket connection is established to the actor.
	 *
	 * This handler receives WebSocket connections made to `/actors/{actorName}/websocket/*` endpoints.
	 * Use this hook to handle custom WebSocket protocols, binary streams, or other WebSocket-based communication.
	 *
	 * @param c The WebSocket context with access to the connection
	 * @param websocket The actor-facing raw WebSocket connection
	 * @param opts Additional options including the original HTTP upgrade request
	 */
	onWebSocket?: (
		c: WebSocketContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			TDatabase,
			TEvents,
			TQueues
		>,
		websocket: UniversalWebSocket,
	) => void | Promise<void>;

	actions?: TActions;

	/**
	 * Optional schemas for validating action argument tuples in native runtimes.
	 * May mirror nested action groups or use dot-separated low-level action names.
	 */
	actionInputSchemas?: ActionInputSchemas;

	/**
	 * Optional schema for validating connection params in native runtimes.
	 */
	connParamsSchema?: PrimitiveSchema;

	/**
	 * Schema map for events broadcasted by this actor.
	 */
	events?: TEvents;

	/**
	 * Schema map for queue payloads sent by this actor.
	 */
	queues?: TQueues;
}

type ActorDatabaseConfig<TDatabase extends AnyDatabaseProvider> =
	| {
			/**
			 * @experimental
			 */
			db: TDatabase;
	  }
	| Record<never, never>;

// 1. Infer schema
// 2. Omit keys that we'll manually define (because of generics)
// 3. Define our own types that have generic constraints
export type ActorConfig<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
	TActions extends Actions<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	> = Actions<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	>,
> = Omit<
	z.infer<typeof ActorConfigSchema>,
	| "actions"
	| "events"
	| "queues"
	| "onCreate"
	| "onDestroy"
	| "onMigrate"
	| "onWake"
	| "onSleep"
	| "run"
	| "onStateChange"
	| "onBeforeConnect"
	| "onConnect"
	| "onDisconnect"
	| "onBeforeActionResponse"
	| "onRequest"
	| "onWebSocket"
	| "state"
	| "createState"
	| "connState"
	| "createConnState"
	| "vars"
	| "createVars"
	| "db"
> &
	BaseActorConfig<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues,
		TActions
	> &
	CreateState<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	> &
	CreateConnState<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	> &
	CreateVars<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	> &
	ActorDatabaseConfig<TDatabase>;

// See description on `ActorConfig`
export type ActorConfigInput<
	TState = undefined,
	TConnParams = undefined,
	TConnState = undefined,
	TVars = undefined,
	TInput = undefined,
	TDatabase extends AnyDatabaseProvider = undefined,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
	TActions extends Actions<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	> = Record<never, never>,
> = {
	types?: ActorTypes<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase
	>;
} & Omit<
	z.input<typeof ActorConfigSchema>,
	| "actions"
	| "events"
	| "queues"
	| "onCreate"
	| "onDestroy"
	| "onMigrate"
	| "onWake"
	| "onSleep"
	| "run"
	| "onStateChange"
	| "onBeforeConnect"
	| "onConnect"
	| "onDisconnect"
	| "onBeforeActionResponse"
	| "onRequest"
	| "onWebSocket"
	| "state"
	| "createState"
	| "connState"
	| "createConnState"
	| "vars"
	| "createVars"
	| "db"
> &
	BaseActorConfig<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues,
		TActions
	> &
	CreateState<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	> &
	CreateConnState<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	> &
	CreateVars<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	> &
	ActorDatabaseConfig<TDatabase>;

// For testing type definitions:
export function test<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase extends AnyDatabaseProvider,
	TEvents extends EventSchemaConfig,
	TQueues extends QueueSchemaConfig,
	TActions extends Actions<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	>,
>(
	input: ActorConfigInput<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues,
		TActions
	>,
): ActorConfig<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	TDatabase,
	TEvents,
	TQueues
> {
	const config = ActorConfigSchema.parse(input) as ActorConfig<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TDatabase,
		TEvents,
		TQueues
	>;
	return config;
}

// MARK: Documentation Schema
// This schema is JSON-serializable for documentation generation.
// It excludes function types and focuses on the configurable options.

export const DocActorOptionsSchema = z
	.object({
		name: z
			.string()
			.optional()
			.describe("Display name for the actor in the Inspector UI."),
		icon: z
			.string()
			.optional()
			.describe(
				"Icon for the actor in the Inspector UI. Can be an emoji (e.g., '🚀') or FontAwesome icon name (e.g., 'rocket').",
			),
		maxActions: z
			.number()
			.int()
			.nonnegative()
			.optional()
			.describe(
				`Maximum number of action handlers that may be defined on this actor. Default: ${DEFAULT_MAX_ACTIONS}`,
			),
		enableActorRuntimeSocket: z
			.boolean()
			.optional()
			.describe(
				"Enables the experimental Actor Runtime Socket for this actor. Default: false",
			),
		createVarsTimeout: z
			.number()
			.optional()
			.describe("Timeout in ms for createVars handler. Default: 5000"),
		createConnStateTimeout: z
			.number()
			.optional()
			.describe(
				"Timeout in ms for createConnState handler. Default: 5000",
			),
		onMigrateTimeout: z
			.number()
			.optional()
			.describe("Timeout in ms for onMigrate handler. Default: 30000"),
		onBeforeConnectTimeout: z
			.number()
			.optional()
			.describe(
				"Timeout in ms for onBeforeConnect handler. Default: 5000",
			),
		onConnectTimeout: z
			.number()
			.optional()
			.describe("Timeout in ms for onConnect handler. Default: 5000"),
		sleepGracePeriod: z
			.number()
			.optional()
			.describe(
				`Max time in ms for the graceful shutdown window. Covers lifecycle hooks (onSleep, onDestroy), the run handler wait, async raw WebSocket handlers, disconnect callbacks, and final state serialization. Default: ${DEFAULT_SLEEP_GRACE_PERIOD}.`,
			),
		onDestroyTimeout: z
			.number()
			.optional()
			.describe(
				"Deprecated. Folded into sleepGracePeriod, which now bounds the entire graceful shutdown window for both sleep and destroy. Will be removed in 2.2.0.",
			),
		waitUntilTimeout: z
			.number()
			.optional()
			.describe(
				"Deprecated. Folded into sleepGracePeriod, which now bounds the entire graceful shutdown window for both sleep and destroy. Will be removed in 2.2.0.",
			),
		stateSaveInterval: z
			.number()
			.optional()
			.describe(
				"Interval in ms between automatic state saves. Default: 1000",
			),
		actionTimeout: z
			.number()
			.optional()
			.describe("Timeout in ms for action handlers. Default: 60000"),
		connectionLivenessTimeout: z
			.number()
			.optional()
			.describe(
				"Timeout in ms for connection liveness checks. Default: 2500",
			),
		connectionLivenessInterval: z
			.number()
			.optional()
			.describe(
				"Interval in ms between connection liveness checks. Default: 5000",
			),
		noSleep: z
			.boolean()
			.optional()
			.describe(
				"Deprecated. If true, the actor will never sleep. Use c.keepAwake(promise) to scope keep-awake to a specific operation instead. Default: false",
			),
		sleepTimeout: z
			.number()
			.optional()
			.describe(
				"Time in ms of inactivity before the actor sleeps. Default: 30000",
			),
		maxQueueSize: z
			.number()
			.optional()
			.describe(
				"Maximum number of queue messages before rejecting new messages. Default: 1000",
			),
		maxSchedules: z
			.number()
			.int()
			.nonnegative()
			.optional()
			.describe(
				"Maximum pending one-shot and recurring schedules before rejecting new schedules. Default: 1000",
			),
		maxQueueMessageSize: z
			.number()
			.optional()
			.describe(
				"Maximum size of each queue message in bytes. Default: 65536",
			),
		canHibernateWebSocket: z
			.boolean()
			.optional()
			.describe(
				"Whether WebSockets using onWebSocket can be hibernated. WebSockets using actions/events are hibernatable by default. Default: false",
			),
	})
	.describe("Actor options for timeouts and behavior configuration.");

export const DocActorConfigSchema = z
	.object({
		state: z
			.unknown()
			.optional()
			.describe(
				"Initial state value for the actor. Cannot be used with createState.",
			),
		createState: z
			.unknown()
			.optional()
			.describe(
				"Function to create initial state. Receives context and input. Cannot be used with state.",
			),
		connState: z
			.unknown()
			.optional()
			.describe(
				"Initial connection state value. Cannot be used with createConnState.",
			),
		createConnState: z
			.unknown()
			.optional()
			.describe(
				"Function to create connection state. Receives context and connection params. The pending connection is not visible in c.conns until this succeeds. Cannot be used with connState.",
			),
		vars: z
			.unknown()
			.optional()
			.describe(
				"Initial ephemeral variables value. Cannot be used with createVars.",
			),
		createVars: z
			.unknown()
			.optional()
			.describe(
				"Function to create ephemeral variables. Receives context and driver context. Cannot be used with vars.",
			),
		db: z
			.unknown()
			.optional()
			.describe("Database provider instance for the actor."),
		onCreate: z
			.unknown()
			.optional()
			.describe(
				"Called when the actor is first initialized. Use to initialize state.",
			),
		onDestroy: z
			.unknown()
			.optional()
			.describe("Called when the actor is destroyed."),
		onMigrate: z
			.unknown()
			.optional()
			.describe(
				"Called on every actor start after persisted state loads and before onWake. Use for repeatable schema migrations.",
			),
		onWake: z
			.unknown()
			.optional()
			.describe(
				"Called when the actor wakes up and is ready to receive connections and actions.",
			),
		onSleep: z
			.unknown()
			.optional()
			.describe(
				"Called when the actor is stopping or sleeping. Use to clean up resources.",
			),
		run: z
			.unknown()
			.optional()
			.describe(
				"Called after actor starts. Does not block startup. Use for background tasks like queue processing or tick loops. If it exits, the actor follows the normal idle sleep timeout once idle. If it throws, the actor logs the error and then follows the normal idle sleep timeout once idle.",
			),
		onStateChange: z
			.unknown()
			.optional()
			.describe(
				"Called when the actor's state changes. State changes within this hook won't trigger recursion.",
			),
		onBeforeConnect: z
			.unknown()
			.optional()
			.describe(
				"Called before a client connects. Throw an error to reject the connection. The pending connection is not visible in c.conns while this runs.",
			),
		onConnect: z
			.unknown()
			.optional()
			.describe(
				"Called when a client successfully connects. The connection is visible in c.conns before this runs.",
			),
		onDisconnect: z
			.unknown()
			.optional()
			.describe("Called when a client disconnects."),
		onBeforeActionResponse: z
			.unknown()
			.optional()
			.describe(
				"Called before sending an action response. Use to transform output.",
			),
		onRequest: z
			.unknown()
			.optional()
			.describe(
				"Called for raw HTTP requests to /actors/{name}/http/* endpoints.",
			),
		onWebSocket: z
			.unknown()
			.optional()
			.describe(
				"Called for raw WebSocket connections to /actors/{name}/websocket/* endpoints.",
			),
		actions: z
			.record(z.string(), z.unknown())
			.optional()
			.describe(
				"Tree of action names or nested groups to handler functions. Nested paths use dot-separated low-level action names. Defaults to an empty object.",
			),
		actionInputSchemas: z
			.record(z.string(), z.unknown())
			.optional()
			.describe(
				"Optional schemas for validating action argument tuples in native runtimes. May mirror nested action groups or use dot-separated low-level action names.",
			),
		connParamsSchema: z
			.unknown()
			.optional()
			.describe(
				"Optional schema for validating connection params in native runtimes.",
			),
		events: z
			.record(z.string(), z.unknown())
			.optional()
			.describe("Map of event names to schemas."),
		queues: z
			.record(z.string(), z.unknown())
			.optional()
			.describe("Map of queue names to schemas."),
		options: DocActorOptionsSchema.optional(),
	})
	.describe("Actor configuration passed to the actor() function.");
