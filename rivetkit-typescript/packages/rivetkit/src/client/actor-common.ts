import type {
	AnyActorDefinition,
	BaseActorDefinition,
} from "@/actor/definition";
import type {
	EventSchemaConfig,
	InferEventArgs,
	InferQueueCompleteMap,
	InferSchemaMap,
	QueueSchemaConfig,
} from "@/actor/schema";
import type {
	QueueSendNoWaitOptions,
	QueueSendResult,
	QueueSendWaitOptions,
} from "./queue";

/**
 * Action function returned by Actor connections and handles.
 *
 * @typedef {Function} ActorActionFunction
 * @template Args
 * @template Response
 * @param {...Args} args - Arguments for the action function.
 * @returns {Promise<Response>}
 */
export type ActorActionFunction<
	Args extends Array<unknown> = unknown[],
	Response = unknown,
> = (
	...args: Args extends [unknown, ...infer Rest] ? Rest : Args
) => Promise<Response>;

type IsAny<T> = 0 extends 1 & T ? true : false;

type LooseEventSubscribe = (
	eventName: string,
	callback: (...args: any[]) => void,
) => () => void;

type ActorActionMap<R> = {
	[K in keyof NonNullable<R>]: NonNullable<R>[K] extends (
		...args: infer Args
	) => infer Return
		? ActorActionFunction<Args, Awaited<Return>>
		: NonNullable<R>[K] extends Record<string, unknown>
			? ActorActionMap<NonNullable<R>[K]>
			: never;
};

type ActionsOf<AD extends AnyActorDefinition> = AD["config"] extends {
	actions?: infer R;
}
	? R
	: never;

export interface ActorGatewayOptions {
	skipReadyWait?: boolean;
}

export type ResolvedActorGatewayOptions = Required<ActorGatewayOptions>;

export function resolveActorGatewayOptions(
	defaults: ActorGatewayOptions = {},
	overrides?: ActorGatewayOptions,
): ResolvedActorGatewayOptions {
	const skipReadyWait =
		overrides?.skipReadyWait ?? defaults.skipReadyWait ?? false;

	return {
		skipReadyWait,
	};
}

export interface ActorActionOptions extends ActorGatewayOptions {
	signal?: AbortSignal;
}

export type ActorConnectOptions = ActorGatewayOptions;

export type ActorFetchInit = RequestInit & ActorGatewayOptions;

export type ActorWebSocketOptions = ActorGatewayOptions;

/**
 * Maps action methods from actor definition to typed function signatures.
 */
export type ActorDefinitionActions<AD extends AnyActorDefinition> =
	// biome-ignore lint/suspicious/noExplicitAny: safe to use any here
	IsAny<AD> extends true
		? Record<string, ActorActionFunction<any[], any>>
		: ActorActionMap<ActionsOf<AD>>;

type ActorQueueSend<TQueues extends QueueSchemaConfig> = {
	<K extends keyof TQueues & string>(
		name: K,
		body: InferSchemaMap<TQueues>[K],
		options: QueueSendWaitOptions,
	): Promise<QueueSendResult<InferQueueCompleteMap<TQueues>[K]>>;
	<K extends keyof TQueues & string>(
		name: K,
		body: InferSchemaMap<TQueues>[K],
		options?: QueueSendNoWaitOptions,
	): Promise<void>;
	(
		name: keyof TQueues extends never ? string : never,
		body: unknown,
		options: QueueSendWaitOptions,
	): Promise<QueueSendResult>;
	(
		name: keyof TQueues extends never ? string : never,
		body: unknown,
		options?: QueueSendNoWaitOptions,
	): Promise<void>;
};

type ActorEventSubscribe<TEvents extends EventSchemaConfig> = {
	<K extends keyof TEvents & string>(
		eventName: K,
		callback: (...args: InferEventArgs<InferSchemaMap<TEvents>[K]>) => void,
	): () => void;
	(
		eventName: keyof TEvents extends never ? string : never,
		callback: (...args: any[]) => void,
	): () => void;
};

export type ActorDefinitionQueueSend<AD extends AnyActorDefinition> =
	// biome-ignore lint/suspicious/noExplicitAny: safe to use any here
	IsAny<AD> extends true
		? { send: ActorQueueSend<Record<string, any>> }
		: AD extends { config: { queues?: infer Q } }
			? Q extends QueueSchemaConfig
				? { send: ActorQueueSend<Q> }
				: {}
			: AD extends BaseActorDefinition<
						any,
						any,
						any,
						any,
						any,
						any,
						any,
						infer Q,
						any
					>
				? Q extends QueueSchemaConfig
					? { send: ActorQueueSend<Q> }
					: {}
				: {};

export type ActorDefinitionEventSubscriptions<AD extends AnyActorDefinition> =
	// biome-ignore lint/suspicious/noExplicitAny: safe to use any here
	IsAny<AD> extends true
		? {
				on: LooseEventSubscribe;
				once: LooseEventSubscribe;
			}
		: AD extends { config: { events?: infer E } }
			? E extends EventSchemaConfig
				? {
						on: ActorEventSubscribe<E>;
						once: ActorEventSubscribe<E>;
					}
				: {}
			: AD extends BaseActorDefinition<
						any,
						any,
						any,
						any,
						any,
						any,
						infer E,
						any,
						any
					>
				? E extends EventSchemaConfig
					? {
							on: ActorEventSubscribe<E>;
							once: ActorEventSubscribe<E>;
						}
					: {}
				: {};
