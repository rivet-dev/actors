import { Derived, Effect, Store } from "@tanstack/store";
import equal from "fast-deep-equal";
import type { AnyActorDefinition, Registry } from "rivetkit";
import type {
	ActorConn,
	ActorConnStatus,
	ActorHandle,
	Client,
	ExtractActorsFromRegistry,
} from "rivetkit/client";

export type AnyActorRegistry = Registry<any>;

export type { ActorConnStatus };

/**
 * Registry-erased form of {@link ActorOptions} stored on {@link ActorStateReference}.
 * Mirrors the public discriminated union so an actor is identified by either
 * `key` or `id`, never both at once.
 */
type StoredActorOptions =
	| {
			name: string;
			/**
			 * Unique key for the actor instance.
			 * This can be a string or an array of strings to create multiple instances.
			 * @example "abc" or ["abc", "def"]
			 */
			key: string | string[];
			/** Parameters for the actor. */
			params?: unknown;
			/** Region to create the actor in if it doesn't exist. */
			createInRegion?: string;
			/** Input data to pass to the actor. */
			createWithInput?: unknown;
			/** Whether the actor is enabled. Defaults to true. */
			enabled?: boolean;
			/**
			 * If true, only gets the actor if it already exists. Does not create it.
			 * Defaults to false.
			 */
			noCreate?: boolean;
	  }
	| {
			name: string;
			/** The raw ID of an existing actor instance. */
			id: string;
			/** Parameters for the actor. */
			params?: unknown;
			/** Whether the actor is enabled. Defaults to true. */
			enabled?: boolean;
	  };

interface ActorStateReference {
	/**
	 * The unique identifier for the actor.
	 * This is a hash generated from the actor's options.
	 * It is used to identify the actor instance in the store.
	 * @internal
	 */
	hash: string;
	/**
	 * The state of the actor, derived from the store.
	 * This includes the actor's connection and handle.
	 */
	handle: ActorHandle<AnyActorDefinition> | null;
	/**
	 * The connection to the actor.
	 * This is used to communicate with the actor in realtime.
	 */
	connection: ActorConn<AnyActorDefinition> | null;
	/**
	 * The connection status of the actor.
	 */
	connStatus: ActorConnStatus;
	/**
	 * The error that occurred while trying to connect to the actor, if any.
	 */
	error: Error | null;
	/**
	 * Options for the actor, including its name, key or id, parameters, and whether
	 * it is enabled.
	 */
	opts: StoredActorOptions;
}

interface InternalRivetKitStore {
	actors: Record<string, ActorStateReference>;
}

/**
 * Options shared by every way of identifying an actor.
 */
interface ActorOptionsBase<
	Registry extends AnyActorRegistry,
	ActorName extends keyof ExtractActorsFromRegistry<Registry> & string,
> {
	/**
	 * Typesafe name of the actor.
	 * This should match the actor's name in the app's actor definitions.
	 * @example "chatRoom"
	 */
	name: ActorName;
	/**
	 * Parameters for the actor.
	 */
	params?: ExtractActorsFromRegistry<Registry>[ActorName]["params"];
	/**
	 * Whether the actor is enabled.
	 * Defaults to true.
	 */
	enabled?: boolean;
}

/**
 * Options for identifying an actor by its name and key.
 */
export interface ActorByKeyOptions<
	Registry extends AnyActorRegistry,
	ActorName extends keyof ExtractActorsFromRegistry<Registry> & string,
> extends ActorOptionsBase<Registry, ActorName> {
	/**
	 * Unique key for the actor instance.
	 * This can be a string or an array of strings to create multiple instances.
	 * @example "abc" or ["abc", "def"]
	 */
	key: string | string[];
	/** Region to create the actor in if it doesn't exist. */
	createInRegion?: string;
	/** Input data to pass to the actor. */
	createWithInput?: unknown;
	/**
	 * If true, only gets the actor if it already exists. Does not create the actor.
	 * Throws an error if the actor is not found.
	 * Defaults to false.
	 */
	noCreate?: boolean;
}

/**
 * Options for identifying an existing actor by its raw actor ID.
 * Unlike key-based options, this always resolves an existing actor and never
 * creates one, so create-only options do not apply.
 */
export interface ActorByIdOptions<
	Registry extends AnyActorRegistry,
	ActorName extends keyof ExtractActorsFromRegistry<Registry> & string,
> extends ActorOptionsBase<Registry, ActorName> {
	/**
	 * The raw ID of an existing actor instance.
	 * @example "a1b2c3d4-..."
	 */
	id: string;
}

/**
 * Options for configuring a actor in RivetKit. An actor is identified either by
 * its name and key or by its raw actor id.
 */
export type ActorOptions<
	Registry extends AnyActorRegistry,
	ActorName extends keyof ExtractActorsFromRegistry<Registry> & string,
> =
	| ActorByKeyOptions<Registry, ActorName>
	| ActorByIdOptions<Registry, ActorName>;

export type ActorsStateDerived<
	Registry extends AnyActorRegistry,
	WorkerName extends keyof ExtractActorsFromRegistry<Registry> & string,
> = Derived<
	Omit<InternalRivetKitStore["actors"][string], "handle" | "connection"> & {
		handle: ActorHandle<
			ExtractActorsFromRegistry<Registry>[WorkerName]
		> | null;
		connection: ActorConn<
			ExtractActorsFromRegistry<Registry>[WorkerName]
		> | null;
		/** @deprecated Use `connStatus === "connected"` instead */
		isConnected: boolean;
	}
>;

export type AnyActorOptions = ActorOptions<AnyActorRegistry, any>;

export interface CreateRivetKitOptions<Registry extends AnyActorRegistry> {
	hashFunction?: (opts: ActorOptions<Registry, any>) => string;
}

type ComputedActorState = InternalRivetKitStore["actors"][string] & {
	/** @deprecated Use `connStatus === "connected"` instead */
	isConnected: boolean;
};

type ActorCache = Map<
	string,
	{
		state: Derived<ComputedActorState>;
		key: string;
		mount: () => () => void;
		create: () => void;
		refCount: number;
		cleanupTimeout: ReturnType<typeof setTimeout> | null;
	}
>;

export function createRivetKit<Registry extends AnyActorRegistry>(
	client: Client<Registry>,
	createOpts: CreateRivetKitOptions<Registry> = {},
) {
	const store = new Store<InternalRivetKitStore>({
		actors: {},
	});

	const cache: ActorCache = new Map();

	return {
		getOrCreateActor: <
			ActorName extends keyof ExtractActorsFromRegistry<Registry> &
				string,
		>(
			actorOpts: ActorOptions<Registry, ActorName>,
		): {
			mount: () => () => void;
			state: ActorsStateDerived<Registry, ActorName>;
			key: string;
		} =>
			(getOrCreateActor as any)(
				client,
				createOpts,
				store,
				cache,
				actorOpts,
			),
		store,
	};
}

type ActorUpdates = Partial<InternalRivetKitStore["actors"][string]>;

function updateActor(
	store: Store<InternalRivetKitStore>,
	key: string,
	updates: ActorUpdates,
) {
	store.setState((prev) => ({
		...prev,
		actors: {
			...prev.actors,
			[key]: { ...prev.actors[key], ...updates },
		},
	}));
}

// See README.md for lifecycle documentation.
function getOrCreateActor<
	Registry extends AnyActorRegistry,
	ActorName extends keyof ExtractActorsFromRegistry<Registry> & string,
>(
	client: Client<Registry>,
	createOpts: CreateRivetKitOptions<Registry>,
	store: Store<InternalRivetKitStore>,
	cache: ActorCache,
	actorOpts: ActorOptions<Registry, ActorName>,
) {
	const hash = createOpts.hashFunction || defaultHashFunction;

	const normalizedOpts = {
		...actorOpts,
		enabled: actorOpts.enabled ?? true,
	};

	const key = hash(normalizedOpts);

	// Sync opts to store on every call (even for cached entries)
	// Use queueMicrotask for updates to avoid "Cannot update a component while rendering" React error
	const existing = store.state.actors[key];
	if (!existing) {
		store.setState((prev) => ({
			...prev,
			actors: {
				...prev.actors,
				[key]: {
					hash: key,
					connStatus: "idle",
					connection: null,
					handle: null,
					error: null,
					opts: normalizedOpts,
				},
			},
		}));
	} else if (!optsEqual(existing.opts, normalizedOpts)) {
		// Defer opts update to avoid triggering re-render during render
		queueMicrotask(() => {
			updateActor(store, key, { opts: normalizedOpts });
		});
	}

	const cached = cache.get(key);
	if (cached) {
		return {
			...cached,
			state: cached.state as ActorsStateDerived<Registry, ActorName>,
		};
	}

	const derived = new Derived({
		fn: ({ currDepVals: [store] }) => {
			const actor = store.actors[key];
			return {
				...actor,
				/** @deprecated Use `connStatus === "connected"` instead */
				isConnected: actor.connStatus === "connected",
			};
		},
		deps: [store],
	});

	// Handle enabled/disabled state changes.
	// Initial connection is triggered directly in mount() since Effect
	// only runs on state changes, not on mount.
	const effect = new Effect({
		fn: () => {
			const actor = store.state.actors[key];
			if (!actor) {
				throw new Error(
					`Actor with key "${key}" not found in store. This indicates a bug in cleanup logic.`,
				);
			}

			// Dispose connection if disabled
			if (!actor.opts.enabled && actor.connection) {
				actor.connection.dispose();

				// Reset state so re-enabling will reconnect
				updateActor(store, key, {
					connection: null,
					handle: null,
					connStatus: "idle",
				});
				return;
			}

			// Reconnect when re-enabled after being disabled
			// Defer to avoid "Cannot update a component while rendering" React error
			if (actor.connStatus === "idle" && actor.opts.enabled) {
				queueMicrotask(() => {
					// Re-check state after microtask in case it changed
					const currentActor = store.state.actors[key];
					if (
						currentActor &&
						currentActor.connStatus === "idle" &&
						currentActor.opts.enabled
					) {
						(create as Function)(client, store, key);
					}
				});
			}
		},
		deps: [derived],
	});

	// Track subscriptions for ref counting
	let unsubscribeDerived: (() => void) | null = null;
	let unsubscribeEffect: (() => void) | null = null;

	const mount = () => {
		const cached = cache.get(key);
		if (!cached) {
			throw new Error(
				`Actor with key "${key}" not found in cache. This indicates a bug in cleanup logic.`,
			);
		}

		// Cancel pending cleanup
		if (cached.cleanupTimeout !== null) {
			clearTimeout(cached.cleanupTimeout);
			cached.cleanupTimeout = null;
		}

		// Increment ref count
		cached.refCount++;

		// Mount derived/effect on first reference (or re-mount after cleanup)
		if (cached.refCount === 1) {
			unsubscribeDerived = derived.mount();
			unsubscribeEffect = effect.mount();

			// Effect doesn't run immediately on mount, only on state changes.
			// Trigger initial connection if actor is enabled and idle.
			const actor = store.state.actors[key];
			if (actor?.opts.enabled && actor.connStatus === "idle") {
				(create as Function)(client, store, key);
			}
		}

		return () => {
			// Decrement ref count
			cached.refCount--;

			if (cached.refCount === 0) {
				// Deferred cleanup prevents needless reconnection when:
				// - React Strict Mode's unmount/remount cycle
				// - useActor hook moves between components in the same render cycle
				cached.cleanupTimeout = setTimeout(() => {
					cached.cleanupTimeout = null;
					if (cached.refCount > 0) return;

					// Unsubscribe from derived/effect
					unsubscribeDerived?.();
					unsubscribeEffect?.();
					unsubscribeDerived = null;
					unsubscribeEffect = null;

					// Dispose connection
					const actor = store.state.actors[key];
					if (actor?.connection) {
						actor.connection.dispose();
					}

					// Remove from store and cache
					store.setState((prev) => {
						const { [key]: _, ...rest } = prev.actors;
						return { ...prev, actors: rest };
					});
					cache.delete(key);
				}, 0);
			}
		};
	};

	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	const boundCreate: () => void = () => (create as any)(client, store, key);
	cache.set(key, {
		state: derived,
		key,
		mount,
		create: boundCreate,
		refCount: 0,
		cleanupTimeout: null,
	});

	return {
		mount,
		state: derived as ActorsStateDerived<Registry, ActorName>,
		key,
	};
}

function create<Registry extends AnyActorRegistry>(
	client: Client<Registry>,
	store: Store<InternalRivetKitStore>,
	key: string,
) {
	const actor = store.state.actors[key];
	if (!actor) {
		throw new Error(
			`Actor with key "${key}" not found in store. This indicates a bug in cleanup logic.`,
		);
	}

	// Save actor to map
	updateActor(store, key, {
		connStatus: "connecting",
		error: null,
	});

	try {
		let handle: ActorHandle<AnyActorDefinition>;
		if ("id" in actor.opts) {
			handle = client.getForId(actor.opts.name as string, actor.opts.id, {
				params: actor.opts.params,
			});
		} else if (actor.opts.noCreate) {
			handle = client.get(actor.opts.name as string, actor.opts.key, {
				params: actor.opts.params,
			});
		} else {
			handle = client.getOrCreate(actor.opts.name as string, actor.opts.key, {
				params: actor.opts.params,
				createInRegion: actor.opts.createInRegion,
				createWithInput: actor.opts.createWithInput,
			});
		}

		const connection = handle.connect();
		const storedHandle = handle as ActorHandle<AnyActorDefinition>;
		const storedConnection = connection as ActorConn<AnyActorDefinition>;

		// Store connection BEFORE registering callbacks to avoid race condition
		// where status change fires before connection is stored
		// framework-base stores every handle in a store typed as
		// ActorHandle<AnyActorDefinition>, so the specific actor type is
		// erased here regardless. Cast straight to the store's field type to
		// avoid relating the deep Registry-generic type against
		// AnyActorDefinition, which exceeds TS's instantiation-depth limit.
		updateActor(store, key, {
			handle: storedHandle,
			connection: storedConnection,
		});

		// Subscribe to connection state changes
		connection.onStatusChange((status) => {
			store.setState((prev) => {
				// Only update if this is still the active connection
				const isActiveConnection =
					prev.actors[key]?.connection === connection;
				if (!isActiveConnection) return prev;
				return {
					...prev,
					actors: {
						...prev.actors,
						[key]: {
							...prev.actors[key],
							connStatus: status,
							// Only clear error when successfully connected
							...(status === "connected" ? { error: null } : {}),
						},
					},
				};
			});
		});

		// onError is followed by onClose which will set connStatus to Disconnected
		connection.onError((error) => {
			store.setState((prev) => {
				// Only update if this is still the active connection
				if (prev.actors[key]?.connection !== connection) return prev;
				return {
					...prev,
					actors: {
						...prev.actors,
						[key]: {
							...prev.actors[key],
							error,
						},
					},
				};
			});
		});
	} catch (error) {
		console.error("Failed to create actor connection", error);
		// Use Disconnected so Effect won't auto-retry
		// User must re-enable or take action to retry
		updateActor(store, key, {
			connStatus: "disconnected",
			error: error as Error,
		});
	}
}

function defaultHashFunction(opts: AnyActorOptions) {
	const key = "key" in opts ? opts.key : undefined;
	const id = "id" in opts ? opts.id : undefined;
	const noCreate = "noCreate" in opts ? opts.noCreate : undefined;
	return JSON.stringify({ name: opts.name, key, id, params: opts.params, noCreate });
}

function optsEqual(
	a: ActorStateReference["opts"],
	b: ActorStateReference["opts"],
) {
	return equal(a, b);
}
