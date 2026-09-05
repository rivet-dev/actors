import type { AnyActorDefinition } from "@/actor/definition";
import type { ActorQuery } from "@/client/query";
import type { Encoding } from "@/common/encoding";
import type { EngineControlClient } from "@/engine-client/driver";
import type { Registry } from "@/registry";
import type { CurrentActorInvocation } from "@/registry/runtime";
import type { ActorActionFunction, ActorGatewayOptions } from "./actor-common";
import {
	type ActorConn,
	type ActorConnRaw,
	CONNECT_SYMBOL,
} from "./actor-conn";
import { type ActorHandle, ActorHandleRaw } from "./actor-handle";
import { logger } from "./log";
import { resolveGatewayTarget } from "./resolve-gateway-target";

export type { ClientConfig, ClientConfigInput } from "./config";

/** Extract the actor registry from the registry definition. */
export type ExtractActorsFromRegistry<A extends Registry<any>> =
	A extends Registry<infer Actors> ? Actors : never;

/** Extract the registry definition from the client. */
export type ExtractRegistryFromClient<C extends Client<Registry<{}>>> =
	C extends Client<infer A> ? A : never;

/**
 * Represents a actor accessor that provides methods to interact with a specific actor.
 */
export interface ActorAccessor<AD extends AnyActorDefinition> {
	/**
	 * Gets a stateless handle to a actor by its key, but does not create the actor if it doesn't exist.
	 * The actor name is automatically injected from the property accessor.
	 *
	 * Each operation resolves the current actor for the key before sending the
	 * request.
	 *
	 * @template AD The actor class that this handle is for.
	 * @param {string | string[]} [key=[]] - The key to identify the actor. Can be a single string or an array of strings.
	 * @param {GetWithIdOptions} [opts] - Options for getting the actor.
	 * @returns {ActorHandle<AD>} - A handle to the actor.
	 */
	get(key?: string | string[], opts?: GetWithIdOptions): ActorHandle<AD>;

	/**
	 * Gets a stateless handle to a actor by its key, creating it if necessary.
	 * The actor name is automatically injected from the property accessor.
	 *
	 * Each operation resolves the current actor for the key before sending the
	 * request.
	 *
	 * @template AD The actor class that this handle is for.
	 * @param {string | string[]} [key=[]] - The key to identify the actor. Can be a single string or an array of strings.
	 * @param {GetOptions} [opts] - Options for getting the actor.
	 * @returns {ActorHandle<AD>} - A handle to the actor.
	 */
	getOrCreate(
		key?: string | string[],
		opts?: GetOrCreateOptions,
	): ActorHandle<AD>;

	/**
	 * Gets a stateless handle to a actor by its ID.
	 *
	 * If the actor is destroyed, operations will throw without retrying since
	 * there is no key to re-resolve.
	 *
	 * @template AD The actor class that this handle is for.
	 * @param {string} actorId - The ID of the actor.
	 * @param {GetWithIdOptions} [opts] - Options for getting the actor.
	 * @returns {ActorHandle<AD>} - A handle to the actor.
	 */
	getForId(actorId: string, opts?: GetWithIdOptions): ActorHandle<AD>;

	/**
	 * Creates a new actor with the name automatically injected from the property accessor,
	 * and returns a stateless handle to it with the actor ID resolved.
	 *
	 * If the actor is destroyed, operations will throw without retrying since
	 * creation is a one-shot operation.
	 *
	 * @template AD The actor class that this handle is for.
	 * @param {string | string[]} key - The key to identify the actor. Can be a single string or an array of strings.
	 * @param {CreateOptions} [opts] - Options for creating the actor (excluding name and key).
	 * @returns {Promise<ActorHandle<AD>>} - A promise that resolves to a handle to the actor.
	 */
	create(
		key?: string | string[],
		opts?: CreateOptions,
	): Promise<ActorHandle<AD>>;
}

/**
 * Options for querying actors.
 * @typedef {Object} QueryOptions
 * @property {unknown} [parameters] - Parameters to pass to the connection.
 */
export interface QueryOptions {
	/** Parameters to pass to the connection. */
	params?: unknown;
	/**
	 * Lazily resolves connection parameters for `.connect()` and `.webSocket()`.
	 * This is called each time a new connection is opened.
	 */
	getParams?: () => Promise<unknown>;
	/** Signal to abort the request. */
	signal?: AbortSignal;
}

/**
 * Options for getting a actor by ID.
 * @typedef {QueryOptions} GetWithIdOptions
 */
export interface GetWithIdOptions extends QueryOptions {}

/**
 * Options for getting a actor.
 * @typedef {QueryOptions} GetOptions
 */
export interface GetOptions extends QueryOptions {}

/**
 * Options for getting or creating a actor.
 * @typedef {QueryOptions} GetOrCreateOptions
 * @property {string} [createInRegion] - Region to create the actor in if it doesn't exist.
 */
export interface GetOrCreateOptions extends QueryOptions {
	/** Region to create the actor in if it doesn't exist. */
	createInRegion?: string;
	/** Input data to pass to the actor. */
	createWithInput?: unknown;
	/**
	 * Name of the envoy pool to select for this actor if it is created.
	 * Overrides the client's configured `poolName` for this call.
	 */
	poolName?: string;
}

/**
 * Options for creating a actor.
 * @typedef {QueryOptions} CreateOptions
 * @property {string} [region] - The region to create the actor in.
 */
export interface CreateOptions extends QueryOptions {
	/** The region to create the actor in. */
	region?: string;
	/** Input data to pass to the actor. */
	input?: unknown;
	/**
	 * Name of the envoy pool to select for this actor.
	 * Overrides the client's configured `poolName` for this call.
	 */
	poolName?: string;
}

/**
 * Represents a region to connect to.
 * @typedef {Object} Region
 * @property {string} id - The region ID.
 * @property {string} name - The region name.
 * @see {@link https://rivet.dev/docs/edge|Edge Networking}
 * @see {@link https://rivet.dev/docs/regions|Available Regions}
 */
export interface Region {
	/**
	 * The region slug.
	 */
	id: string;

	/**
	 * The human-friendly region name.
	 */
	name: string;
}

export const ACTOR_CONNS_SYMBOL = Symbol("actorConns");
export const CREATE_ACTOR_CONN_PROXY = Symbol("createActorConnProxy");

/**
 * Client for managing & connecting to actors.
 *
 * @template A The actors map type that defines the available actors.
 * @see {@link https://rivet.dev/docs/manage|Create & Manage Actors}
 */
export interface ClientRawOptions {
	encoding?: Encoding;
	gateway?: ActorGatewayOptions;
	/** Supplies the calling actor's invocation so actor-owned clients propagate its trace and ray. */
	currentActorInvocation?: CurrentActorInvocation;
}

export class ClientRaw {
	#disposed = false;

	[ACTOR_CONNS_SYMBOL] = new Set<ActorConnRaw>();

	#driver: EngineControlClient;
	#encodingKind: Encoding;
	#gatewayOptions: ActorGatewayOptions;
	#currentActorInvocation?: CurrentActorInvocation;

	/**
	 * Creates an instance of Client.
	 */
	public constructor(
		driver: EngineControlClient,
		options: ClientRawOptions = {},
	) {
		this.#driver = driver;

		this.#encodingKind = options.encoding ?? "bare";
		this.#gatewayOptions = options.gateway ?? {};
		this.#currentActorInvocation = options.currentActorInvocation;
	}

	/**
	 * Gets a stateless handle to a actor by its ID.
	 *
	 * @template AD The actor class that this handle is for.
	 * @param {string} name - The name of the actor.
	 * @param {string} actorId - The ID of the actor.
	 * @param {GetWithIdOptions} [opts] - Options for getting the actor.
	 * @returns {ActorHandle<AD>} - A handle to the actor.
	 */
	getForId<AD extends AnyActorDefinition>(
		name: string,
		actorId: string,
		opts?: GetWithIdOptions,
	): ActorHandle<AD> {
		logger().debug({
			msg: "get handle to actor with id",
			name,
			actorId,
			params: opts?.params,
		});

		const actorQuery: ActorQuery = {
			getForId: {
				name,
				actorId,
			},
		};

		const handle = this.#createHandle(
			opts?.params,
			opts?.getParams,
			actorQuery,
		);
		return createActorProxy(handle) as ActorHandle<AD>;
	}

	/**
	 * Gets a stateless handle to a actor by its key, but does not create the actor if it doesn't exist.
	 *
	 * @template AD The actor class that this handle is for.
	 * @param {string} name - The name of the actor.
	 * @param {string | string[]} [key=[]] - The key to identify the actor. Can be a single string or an array of strings.
	 * @param {GetWithIdOptions} [opts] - Options for getting the actor.
	 * @returns {ActorHandle<AD>} - A handle to the actor.
	 */
	get<AD extends AnyActorDefinition>(
		name: string,
		key?: string | string[],
		opts?: GetWithIdOptions,
	): ActorHandle<AD> {
		// Convert string to array of strings
		const keyArray: string[] = typeof key === "string" ? [key] : key || [];

		logger().debug({
			msg: "get handle to actor",
			name,
			key: keyArray,
			parameters: opts?.params,
		});

		const actorQuery: ActorQuery = {
			getForKey: {
				name,
				key: keyArray,
			},
		};

		const handle = this.#createHandle(
			opts?.params,
			opts?.getParams,
			actorQuery,
		);
		return createActorProxy(handle) as ActorHandle<AD>;
	}

	/**
	 * Gets a stateless handle to a actor by its key, creating it if necessary.
	 *
	 * @template AD The actor class that this handle is for.
	 * @param {string} name - The name of the actor.
	 * @param {string | string[]} [key=[]] - The key to identify the actor. Can be a single string or an array of strings.
	 * @param {GetOptions} [opts] - Options for getting the actor.
	 * @returns {ActorHandle<AD>} - A handle to the actor.
	 */
	getOrCreate<AD extends AnyActorDefinition>(
		name: string,
		key?: string | string[],
		opts?: GetOrCreateOptions,
	): ActorHandle<AD> {
		// Convert string to array of strings
		const keyArray: string[] = typeof key === "string" ? [key] : key || [];

		logger().debug({
			msg: "get or create handle to actor",
			name,
			key: keyArray,
			parameters: opts?.params,
			createInRegion: opts?.createInRegion,
		});

		const actorQuery: ActorQuery = {
			getOrCreateForKey: {
				name,
				key: keyArray,
				input: opts?.createWithInput,
				region: opts?.createInRegion,
				poolName: opts?.poolName,
			},
		};

		const handle = this.#createHandle(
			opts?.params,
			opts?.getParams,
			actorQuery,
		);
		return createActorProxy(handle) as ActorHandle<AD>;
	}

	/**
	 * Creates a new actor with the provided key and returns a stateless handle to it.
	 * Resolves the actor ID and returns a handle with getForId query.
	 *
	 * @template AD The actor class that this handle is for.
	 * @param {string} name - The name of the actor.
	 * @param {string | string[]} key - The key to identify the actor. Can be a single string or an array of strings.
	 * @param {CreateOptions} [opts] - Options for creating the actor (excluding name and key).
	 * @returns {Promise<ActorHandle<AD>>} - A promise that resolves to a handle to the actor.
	 */
	async create<AD extends AnyActorDefinition>(
		name: string,
		key?: string | string[],
		opts?: CreateOptions,
	): Promise<ActorHandle<AD>> {
		// Convert string to array of strings
		const keyArray: string[] = typeof key === "string" ? [key] : key || [];

		const createQuery = {
			create: {
				...opts,
				// Do these last to override `opts`
				name,
				key: keyArray,
			},
		} satisfies ActorQuery;

		logger().debug({
			msg: "create actor handle",
			name,
			key: keyArray,
			parameters: opts?.params,
			create: createQuery.create,
		});

		// Create the actor
		const actorId = await resolveGatewayTarget(this.#driver, createQuery);
		logger().debug({
			msg: "created actor with ID",
			name,
			key: keyArray,
			actorId,
		});

		// Create handle with actor ID
		const getForIdQuery = {
			getForId: {
				name,
				actorId,
			},
		} satisfies ActorQuery;
		const handle = this.#createHandle(
			opts?.params,
			opts?.getParams,
			getForIdQuery,
		);

		const proxy = createActorProxy(handle) as ActorHandle<AD>;

		return proxy;
	}

	#createHandle(
		params: unknown,
		getParams: (() => Promise<unknown>) | undefined,
		actorQuery: ActorQuery,
	): ActorHandleRaw {
		return new ActorHandleRaw(
			this,
			this.#driver,
			params,
			getParams,
			this.#encodingKind,
			actorQuery,
			this.#gatewayOptions,
			this.#currentActorInvocation,
		);
	}

	[CREATE_ACTOR_CONN_PROXY]<AD extends AnyActorDefinition>(
		conn: ActorConnRaw,
	): ActorConn<AD> {
		// Save to connection list
		this[ACTOR_CONNS_SYMBOL].add(conn);

		// Start connection
		conn[CONNECT_SYMBOL]();

		return createActorProxy(conn) as ActorConn<AD>;
	}

	/**
	 * Disconnects from all actors.
	 *
	 * @returns {Promise<void>} A promise that resolves when all connections are closed.
	 */
	async dispose(): Promise<void> {
		if (this.#disposed) {
			logger().warn({ msg: "client already disconnected" });
			return;
		}
		this.#disposed = true;

		logger().debug({ msg: "disposing client" });

		const disposePromises = [];

		// Dispose all connections
		for (const conn of this[ACTOR_CONNS_SYMBOL].values()) {
			disposePromises.push(conn.dispose());
		}

		await Promise.all(disposePromises);
	}
}

/**
 * Client type with actor accessors.
 * This adds property accessors for actor names to the ClientRaw base class.
 *
 * @template A The actor registry type.
 */
export type Client<A extends Registry<any>> = ClientRaw & {
	[K in keyof ExtractActorsFromRegistry<A>]: ActorAccessor<
		ExtractActorsFromRegistry<A>[K]
	>;
};

export type AnyClient = Client<Registry<any>>;

export function createClientWithDriver<A extends Registry<any>>(
	driver: EngineControlClient,
	options: ClientRawOptions = {},
): Client<A> {
	const client = new ClientRaw(driver, options);

	// Create proxy for accessing actors by name
	return new Proxy(client, {
		get: (target: ClientRaw, prop: string | symbol, receiver: unknown) => {
			// Get the real property if it exists
			if (typeof prop === "symbol" || prop in target) {
				const value = Reflect.get(target, prop, receiver);
				// Preserve method binding
				if (typeof value === "function") {
					return value.bind(target);
				}
				return value;
			}

			// Handle actor accessor for string properties (actor names)
			if (typeof prop === "string") {
				// Return actor accessor object with methods
				return {
					// Handle methods (stateless action)
					get: (
						key?: string | string[],
						opts?: GetWithIdOptions,
					): ActorHandle<
						ExtractActorsFromRegistry<A>[typeof prop]
					> => {
						return target.get<
							ExtractActorsFromRegistry<A>[typeof prop]
						>(prop, key, opts);
					},
					getOrCreate: (
						key?: string | string[],
						opts?: GetOptions,
					): ActorHandle<
						ExtractActorsFromRegistry<A>[typeof prop]
					> => {
						return target.getOrCreate<
							ExtractActorsFromRegistry<A>[typeof prop]
						>(prop, key, opts);
					},
					getForId: (
						actorId: string,
						opts?: GetWithIdOptions,
					): ActorHandle<
						ExtractActorsFromRegistry<A>[typeof prop]
					> => {
						return target.getForId<
							ExtractActorsFromRegistry<A>[typeof prop]
						>(prop, actorId, opts);
					},
					create: async (
						key: string | string[],
						opts: CreateOptions = {},
					): Promise<
						ActorHandle<ExtractActorsFromRegistry<A>[typeof prop]>
					> => {
						return await target.create<
							ExtractActorsFromRegistry<A>[typeof prop]
						>(prop, key, opts);
					},
				} as ActorAccessor<ExtractActorsFromRegistry<A>[typeof prop]>;
			}

			return undefined;
		},
	}) as Client<A>;
}

/**
 * Creates a proxy for a actor that enables calling actions without explicitly using `.action`.
 **/
export function createActorProxy<AD extends AnyActorDefinition>(
	handle: ActorHandleRaw | ActorConnRaw,
): ActorHandle<AD> | ActorConn<AD> {
	// Stores returned action path proxies for faster calls.
	const methodCache = new Map<string, ActorActionFunction>();
	const actionPath = (name: string): ActorActionFunction => {
		let method = methodCache.get(name);
		if (method) return method;

		method = new Proxy(
			(...args: unknown[]) => handle.action({ name, args }),
			{
				get(target, prop: string | symbol) {
					if (typeof prop === "symbol")
						return Reflect.get(target, prop);
					if (prop === "then") return undefined;
					return actionPath(`${name}.${prop}`);
				},
			},
		) as ActorActionFunction;
		methodCache.set(name, method);
		return method;
	};

	return new Proxy(handle, {
		get(target: ActorHandleRaw, prop: string | symbol, receiver: unknown) {
			// Handle built-in Symbol properties
			if (typeof prop === "symbol") {
				return Reflect.get(target, prop, receiver);
			}

			// Handle built-in Promise methods and existing properties
			if (prop === "constructor" || prop in target) {
				const value = Reflect.get(target, prop, target);
				// Preserve method binding
				if (typeof value === "function") {
					return value.bind(target);
				}
				return value;
			}

			// Create action function that preserves 'this' context
			if (typeof prop === "string") {
				// If JS is attempting to calling this as a promise, ignore it
				if (prop === "then") return undefined;

				return actionPath(prop);
			}
		},

		// Support for 'in' operator
		has(target: ActorHandleRaw, prop: string | symbol) {
			// All string properties are potentially action functions.
			if (typeof prop === "string") return true;
			// For symbols, defer to the target's own has behavior
			return Reflect.has(target, prop);
		},

		// Support instanceof checks
		getPrototypeOf(target: ActorHandleRaw) {
			return Reflect.getPrototypeOf(target);
		},

		// Prevent property enumeration of non-existent action methods
		ownKeys(target: ActorHandleRaw) {
			return Reflect.ownKeys(target);
		},

		// Support proper property descriptors
		getOwnPropertyDescriptor(
			target: ActorHandleRaw,
			prop: string | symbol,
		) {
			if (prop === "then") return undefined;
			const targetDescriptor = Reflect.getOwnPropertyDescriptor(
				target,
				prop,
			);
			if (targetDescriptor) {
				return targetDescriptor;
			}
			if (typeof prop === "string") {
				// Make action methods appear non-enumerable
				return {
					configurable: true,
					enumerable: false,
					writable: false,
					value: actionPath(prop),
				};
			}
			return undefined;
		},
	}) as unknown as ActorHandle<AD> | ActorConn<AD>;
}
