import { isLocalEngineEndpoint } from "@/common/engine";
import { isResponseLike } from "@/common/fetch-like";
import { configureBaseLogger } from "@/common/log";
import { configureServerlessPool } from "@/serverless/configure";
import { VERSION } from "@/utils";
import {
	getRivetkitPublicDir,
	getRivetkitRuntimeMode,
	parsePortEnv,
} from "@/utils/env-vars";
import {
	type RegistryActors,
	type RegistryConfig,
	type RegistryConfigInput,
	RegistryConfigSchema,
} from "./config";
import { logger } from "./log";
import { buildConfiguredRegistry } from "./native";
import type {
	RuntimeApplicationFetch,
	RuntimeServerlessResponseHead,
} from "./runtime";

type ShutdownSignal = "SIGINT" | "SIGTERM";

const REGISTRY_READY_TIMEOUT_MS = 30_000;

function signalExitCode(signal: ShutdownSignal): number {
	switch (signal) {
		case "SIGINT":
			return 130;
		case "SIGTERM":
			return 143;
	}
}

function finishShutdownSignal(signal: ShutdownSignal): void {
	if (process.pid === 1) {
		process.exit(signalExitCode(signal));
	}
	process.kill(process.pid, signal);
}

function createApplicationFetch(
	application: RegistryApplication,
): RuntimeApplicationFetch {
	return async (request) => {
		const method = request.method.toUpperCase();
		const body =
			method === "GET" || method === "HEAD" || request.body.length === 0
				? undefined
				: Uint8Array.from(request.body).buffer;
		const response = await application.fetch(
			new Request(request.url, {
				method,
				headers: request.headers,
				body,
			}),
		);
		if (!isResponseLike(response)) {
			throw new TypeError(
				"registry.listen() application fetch must return a Response",
			);
		}
		return {
			status: response.status,
			headers: Object.fromEntries(response.headers.entries()),
			body: new Uint8Array(await response.arrayBuffer()),
		};
	};
}

export type FetchHandler = (
	request: Request,
	...args: any
) => Response | Promise<Response>;

export interface ServerlessHandler {
	fetch: FetchHandler;
}

export interface RegistryApplication {
	fetch: FetchHandler;
}

export interface RegistryListenOptions {
	port?: number;
	host?: string;
	publicDir?: string;
	application?: RegistryApplication;
}

export interface RegistryRoutes {
	health(): Promise<Response>;
	metadata(): Promise<Response>;
	prometheusMetrics(request?: Request): Promise<Response>;
}

/**
 * Injectable dependencies for {@link Registry}. Production code uses the
 * defaults. Tests override `buildConfiguredRegistry` to drive lifecycle
 * orchestration against a fake `CoreRuntime` without an engine.
 */
export interface RegistryDeps {
	buildConfiguredRegistry: typeof buildConfiguredRegistry;
}

export class Registry<A extends RegistryActors> {
	#config: RegistryConfigInput<A>;
	#buildConfiguredRegistry: typeof buildConfiguredRegistry;
	public readonly routes: RegistryRoutes;

	get config(): RegistryConfigInput<A> {
		return this.#config;
	}

	parseConfig(): RegistryConfig {
		return RegistryConfigSchema.parse(this.#config);
	}

	#runtimeServePromise?: Promise<void>;
	#runtimeServeLifecyclePromise?: Promise<void>;
	#runtimeReadyPromise?: Promise<void>;
	#runtimeServeConfiguredPromise?: ReturnType<typeof buildConfiguredRegistry>;
	#runtimeServerlessPromise?: ReturnType<typeof buildConfiguredRegistry>;
	#applicationListenerPromise?: Promise<void>;
	#configureServerlessPoolPromise?: Promise<void>;
	#welcomePrinted = false;
	#shutdownInstalled = false;
	#shutdownInFlight: Promise<void> | null = null;
	#signalHandlers: Partial<Record<ShutdownSignal, () => void>> = {};

	constructor(config: RegistryConfigInput<A>, deps?: Partial<RegistryDeps>) {
		this.#config = config;
		if (config.logging?.baseLogger) {
			configureBaseLogger(config.logging.baseLogger);
		}
		this.#buildConfiguredRegistry =
			deps?.buildConfiguredRegistry ?? buildConfiguredRegistry;
		this.routes = {
			health: () => this.#healthRoute(),
			metadata: () => this.#metadataRoute(),
			prometheusMetrics: (request?: Request) =>
				this.#prometheusMetricsRoute(request),
		};
	}

	/**
	 * Fires `configureServerlessPool` once per process when the registry
	 * config opts into it. Cached on the instance so repeated calls (from
	 * `handler()` and `listen()`) only run the upsert once. The retry loop
	 * inside `configureServerlessPool` tolerates the engine still warming up.
	 */
	#ensureServerlessPoolConfigured(
		config: RegistryConfig,
	): Promise<void> | undefined {
		if (!config.configurePool) return undefined;

		if (!this.#configureServerlessPoolPromise) {
			this.#configureServerlessPoolPromise = configureServerlessPool(
				config,
			).catch((error) => {
				this.#configureServerlessPoolPromise = undefined;
				throw error;
			});
			this.#configureServerlessPoolPromise.catch(() => {});
		}

		return this.#configureServerlessPoolPromise;
	}

	/**
	 * Handle an incoming HTTP request for serverless deployments.
	 *
	 * @example
	 * ```ts
	 * const app = new Hono();
	 * app.all("/api/rivet/*", (c) => registry.handler(c.req.raw));
	 * export default app;
	 * ```
	 */
	public async handler(request: Request): Promise<Response> {
		const config = this.parseConfig();
		this.#printWelcome(config, "serverless");

		if (!this.#runtimeServerlessPromise) {
			this.#runtimeServerlessPromise =
				this.#buildConfiguredRegistry(config);
		}

		const { runtime, registry, serveConfig } =
			await this.#runtimeServerlessPromise;
		const isStartRequest = isServerlessStartRequest(
			request,
			serveConfig.serverlessBasePath ?? "/api/rivet",
		);
		const isMetadataRequest = isServerlessMetadataRequest(
			request,
			serveConfig.serverlessBasePath ?? "/api/rivet",
		);
		const isEngineMetadataRequest =
			request.headers.get("user-agent")?.startsWith("RivetEngine/") ??
			false;

		if (isStartRequest) {
			try {
				await this.#ensureServerlessPoolConfigured(config);
			} catch (_error) {
				return new Response(
					JSON.stringify({
						group: "guard",
						code: "service_unavailable",
						message: "Serverless pool is not configured.",
						metadata: null,
					}),
					{
						status: 503,
						headers: { "content-type": "application/json" },
					},
				);
			}
		}

		const cancelToken = runtime.createCancellationToken();
		const abort = () => runtime.cancelCancellationToken(cancelToken);
		if (request.signal.aborted) {
			abort();
		} else {
			request.signal.addEventListener("abort", abort, { once: true });
		}

		const requestBody = await request.arrayBuffer();
		if (
			isStartRequest &&
			requestBody.byteLength > serveConfig.serverlessMaxStartPayloadBytes
		) {
			request.signal.removeEventListener("abort", abort);
			runtime.cancelCancellationToken(cancelToken);
			return new Response(
				JSON.stringify({
					group: "message",
					code: "incoming_too_long",
					message: `Incoming message too long. Received ${requestBody.byteLength} bytes, limit is ${serveConfig.serverlessMaxStartPayloadBytes} bytes.`,
					metadata: null,
				}),
				{
					status: 413,
					headers: { "content-type": "application/json" },
				},
			);
		}

		let settled = false;
		let controllerRef:
			| ReadableStreamDefaultController<Uint8Array>
			| undefined;
		const backpressureWaiters: Array<() => void> = [];
		const resolveBackpressure = () => {
			while (
				controllerRef &&
				(controllerRef.desiredSize ?? 1) > 0 &&
				backpressureWaiters.length > 0
			) {
				backpressureWaiters.shift()?.();
			}
		};
		const waitForBackpressure = async () => {
			if (!controllerRef || (controllerRef.desiredSize ?? 1) > 0) return;
			await new Promise<void>((resolve) => {
				backpressureWaiters.push(resolve);
			});
		};
		const stream = new ReadableStream<Uint8Array>({
			start(controller) {
				controllerRef = controller;
			},
			pull() {
				resolveBackpressure();
			},
			cancel() {
				settled = true;
				resolveBackpressure();
				runtime.cancelCancellationToken(cancelToken);
			},
		});

		const headers: Record<string, string> = {};
		request.headers.forEach((value, key) => {
			headers[key] = value;
		});

		let head: RuntimeServerlessResponseHead;
		try {
			head = await runtime.handleServerlessRequest(
				registry,
				{
					method: request.method,
					url: request.url,
					headers,
					body: new Uint8Array(requestBody),
				},
				async (
					error: unknown,
					event?: {
						kind: "chunk" | "end";
						chunk?: Uint8Array;
						error?: {
							group: string;
							code: string;
							message: string;
						};
					},
				) => {
					if (error) throw error;
					if (!event || settled) return;
					if (event.kind === "chunk") {
						await waitForBackpressure();
						if (settled) return;
						if (event.chunk) controllerRef?.enqueue(event.chunk);
						return;
					}

					settled = true;
					resolveBackpressure();
					request.signal.removeEventListener("abort", abort);
					if (event.error) {
						controllerRef?.error(
							new Error(
								`${event.error.group}.${event.error.code}: ${event.error.message}`,
							),
						);
					} else {
						controllerRef?.close();
					}
				},
				cancelToken,
				serveConfig,
			);
		} catch (err) {
			// The runtime call itself rejected (e.g. `registry_shut_down_error`).
			// Clean up the abort listener so it doesn't leak, then propagate.
			request.signal.removeEventListener("abort", abort);
			runtime.cancelCancellationToken(cancelToken);
			throw err;
		}

		if (isMetadataRequest && !isEngineMetadataRequest) {
			try {
				await this.#ensureServerlessPoolConfigured(config);
			} catch (_error) {
				return new Response(
					JSON.stringify({
						group: "guard",
						code: "service_unavailable",
						message: "Serverless pool is not configured.",
						metadata: null,
					}),
					{
						status: 503,
						headers: { "content-type": "application/json" },
					},
				);
			}
		}

		return new Response(stream, {
			status: head.status,
			headers: head.headers,
		});
	}

	/**
	 * Returns a fetch handler for serverless deployments.
	 *
	 * @example
	 * ```ts
	 * export default registry.serve();
	 * ```
	 */
	public serve(): ServerlessHandler {
		return {
			fetch: (request) => this.handler(request),
		};
	}

	/**
	 * Bind an HTTP listener provided by the native (Rust) runtime and serve
	 * the registry's serverless endpoints over it. Resolves only after the
	 * registry is shut down (SIGINT/SIGTERM or `nativeRegistry.shutdown()`).
	 *
	 * @param opts.port      Port to listen on. Defaults to `process.env.RIVET_PORT`
	 *                       if set, otherwise 3000.
	 * @param opts.host      Address to bind. Defaults to `0.0.0.0`.
	 * @param opts.publicDir If set, serves static files from this directory
	 *                       as a fallback below the framework routes.
	 * @param opts.application If set, handles requests that do not match a
	 *                         framework route.
	 *
	 * @example
	 * ```ts
	 * await registry.listen();
	 * await registry.listen({ application: app });
	 * ```
	 */
	public async listen(opts: RegistryListenOptions = {}): Promise<void> {
		const port = opts.port ?? parsePortEnv(process.env.RIVET_PORT) ?? 3000;
		const publicDir = opts.publicDir ?? getRivetkitPublicDir();
		const config = this.parseConfig();
		const application = opts.application
			? createApplicationFetch(opts.application)
			: undefined;

		if (application && getRivetkitRuntimeMode() !== "serverless") {
			if (config.runtime === "wasm") {
				throw new Error(
					"registry.listen() requires the native runtime; use an application-owned HTTP server with WebAssembly",
				);
			}
			this.#installSignalHandlers(config);
			this.#printWelcome(config, "serverful", {
				port,
				host: opts.host,
				publicDir,
			});
			this.#startEnvoy(config, true);
			const readyPromise = this.startAndWait();
			const configuredRegistryPromise =
				this.#runtimeServeConfiguredPromise;
			if (!configuredRegistryPromise) {
				throw new Error("registry envoy startup did not initialize");
			}
			const { runtime, registry, serveConfig } =
				await configuredRegistryPromise;
			const listenerPromise = runtime.serveApplicationListener(
				registry,
				{
					port,
					host: opts.host,
					publicDir,
					application,
				},
				serveConfig,
			);
			this.#applicationListenerPromise = listenerPromise;
			const serveLifecyclePromise = this.#runtimeServeLifecyclePromise;
			if (!serveLifecyclePromise) {
				throw new Error(
					"registry envoy serve lifecycle did not initialize",
				);
			}
			await Promise.all([
				readyPromise,
				listenerPromise,
				serveLifecyclePromise,
			]);
			return;
		}

		// Cache on both promise fields so the shutdown drain sees Mode A and B.
		const configuredRegistryPromise = buildConfiguredRegistry(config);
		this.#runtimeServeConfiguredPromise = configuredRegistryPromise;
		this.#runtimeServerlessPromise = configuredRegistryPromise;
		this.#installSignalHandlers(config);

		this.#printWelcome(config, "serverless", {
			port,
			host: opts.host,
			publicDir,
		});

		// Background fire; the retry loop tolerates engine warm-up.
		this.#ensureServerlessPoolConfigured(config);

		const { runtime, registry, serveConfig } =
			await configuredRegistryPromise;
		await runtime.serveListener(
			registry,
			{
				port,
				host: opts.host,
				publicDir,
				application,
			},
			serveConfig,
		);
	}

	/**
	 * Returns a health response suitable for mounting in a user-owned router.
	 */
	async #healthRoute(): Promise<Response> {
		const configured = await this.#activeConfiguredRegistry();
		if (!configured) {
			return jsonRouteResponse(503, {
				status: "not_started",
				runtime: "rivetkit",
				version: VERSION,
			});
		}

		const { runtime, registry } = configured;
		if (!runtime.registryHealth) {
			return jsonRouteResponse(501, {
				status: "unsupported",
				runtime: "rivetkit",
				version: VERSION,
			});
		}

		const response = await runtime.registryHealth(registry);
		return new Response(new Uint8Array(response.body), {
			status: response.status,
			headers: response.headers,
		});
	}

	/**
	 * Returns serverless metadata suitable for mounting in a user-owned router.
	 */
	async #metadataRoute(): Promise<Response> {
		const configured = await this.#activeConfiguredRegistry();
		if (!configured) {
			return new Response("registry not started\n", {
				status: 503,
				headers: { "content-type": "text/plain; charset=utf-8" },
			});
		}

		const { runtime, registry } = configured;
		if (!runtime.registryMetadata) {
			return new Response("metadata is not supported by this runtime\n", {
				status: 501,
				headers: { "content-type": "text/plain; charset=utf-8" },
			});
		}

		const response = await runtime.registryMetadata(registry);
		return new Response(new Uint8Array(response.body), {
			status: response.status,
			headers: response.headers,
		});
	}

	/**
	 * Returns a Prometheus metrics response suitable for mounting in a user-owned router.
	 */
	async #prometheusMetricsRoute(_request?: Request): Promise<Response> {
		const configured = await this.#activeConfiguredRegistry();
		if (!configured) {
			return new Response("registry not started\n", {
				status: 503,
				headers: { "content-type": "text/plain; charset=utf-8" },
			});
		}

		const { runtime, registry } = configured;
		if (!runtime.registryMetrics) {
			return new Response("metrics are not supported by this runtime\n", {
				status: 501,
				headers: { "content-type": "text/plain; charset=utf-8" },
			});
		}

		const response = await runtime.registryMetrics(registry);
		return new Response(new Uint8Array(response.body), {
			status: response.status,
			headers: response.headers,
		});
	}

	async #activeConfiguredRegistry(): Promise<
		Awaited<ReturnType<typeof buildConfiguredRegistry>> | undefined
	> {
		const candidates = [
			this.#runtimeServerlessPromise,
			this.#runtimeServeConfiguredPromise,
		].filter(
			(
				candidate,
			): candidate is ReturnType<typeof buildConfiguredRegistry> =>
				candidate !== undefined,
		);

		if (candidates.length === 0) return undefined;
		return await candidates[0]!;
	}

	/**
	 * Starts an actor envoy for standalone server deployments.
	 */
	#startEnvoy(config: RegistryConfig, printWelcome: boolean) {
		if (!this.#runtimeServePromise) {
			const configuredRegistryPromise =
				this.#buildConfiguredRegistry(config);
			this.#runtimeServeConfiguredPromise = configuredRegistryPromise;
			this.#runtimeServeLifecyclePromise = configuredRegistryPromise.then(
				async ({ runtime, registry, serveConfig }) => {
					await runtime.serveRegistry(registry, serveConfig);
				},
			);
			this.#runtimeServePromise =
				this.#runtimeServeLifecyclePromise.catch((error) => {
					// Always-attached catch so the stored promise never leaves a
					// rejection unhandled. Downstream awaits (e.g. #runShutdown's
					// Promise.race) attach their own catches and still observe
					// resolution via the race.
					logger().warn({ error }, "runtime registry serve errored");
				});
			// Install signal handlers once an envoy lifecycle has begun. Only
			// Mode A ever reaches here. Mode B (handler(request)) intentionally
			// does not install handlers because it runs on Workers/Vercel/Deno
			// Deploy where `process.on` is absent or forbidden; those platforms
			// own their own signal policy.
			this.#installSignalHandlers(config);
		}
		if (printWelcome) {
			this.#printWelcome(config, "serverful");
		}
	}

	#installSignalHandlers(config: RegistryConfig): void {
		if (this.#shutdownInstalled) return;
		if (config.shutdown?.disableSignalHandlers) return;
		// Guard against non-Node runtimes (Workers/Edge) where `process` may
		// exist but `process.on` is unavailable or forbidden.
		if (
			typeof process === "undefined" ||
			typeof process.on !== "function" ||
			typeof process.kill !== "function"
		) {
			return;
		}
		this.#shutdownInstalled = true;

		const install = (signal: ShutdownSignal) => {
			const handler = () => this.#onShutdownSignal(signal, config);
			this.#signalHandlers[signal] = handler;
			process.on(signal, handler);
		};
		install("SIGINT");
		install("SIGTERM");
	}

	#onShutdownSignal(signal: ShutdownSignal, config: RegistryConfig): void {
		if (this.#shutdownInFlight !== null) {
			// Second delivery of the same (or another) shutdown signal, or a
			// drain already started by an explicit `shutdown()` call. Remove
			// our handler only, preserving any user-installed listeners. PID 1
			// must exit directly because re-raised default signals can be
			// swallowed by the container signal path.
			this.#removeSignalHandlers();
			finishShutdownSignal(signal);
			return;
		}
		this.#shutdownInFlight = this.#drain(config)
			.catch((err) => {
				logger().warn({ err }, "shutdown error");
			})
			.then(() => {
				this.#removeSignalHandlers();
				finishShutdownSignal(signal);
			});
	}

	/**
	 * Gracefully drains all live registries.
	 *
	 * Programmatic counterpart to the SIGINT/SIGTERM handlers: tears down
	 * every live `CoreRegistry` (both `start()` and `handler()` modes) and
	 * waits for the serve promise to resolve, all bounded by the shutdown
	 * grace period. Unlike a signal-driven shutdown, this does not re-raise a
	 * signal or exit the process. The caller owns process lifetime.
	 *
	 * Idempotent: concurrent or repeated calls share a single drain. Safe to
	 * call even if nothing has been started.
	 *
	 * @example
	 * ```ts
	 * const registry = setup({ use: { counter } });
	 * registry.start();
	 * // ...later, on your own shutdown trigger:
	 * await registry.shutdown();
	 * ```
	 */
	public async shutdown(): Promise<void> {
		if (this.#shutdownInFlight !== null) return this.#shutdownInFlight;
		const config = this.parseConfig();
		// Uninstall our signal handlers so a later SIGINT/SIGTERM does not
		// re-trigger a drain on already-torn-down registries. Subsequent
		// signals fall back to Node's default termination behavior.
		this.#removeSignalHandlers();
		this.#shutdownInFlight = this.#drain(config).catch((err) => {
			logger().warn({ err }, "shutdown error");
		});
		return this.#shutdownInFlight;
	}

	async #drain(config: RegistryConfig): Promise<void> {
		const modeAPromise = this.#runtimeServeConfiguredPromise;
		const modeBPromise = this.#runtimeServerlessPromise;

		const gracePeriodMs =
			config.shutdown?.gracePeriodMs ??
			(await this.#actorStopThresholdMs(modeAPromise ?? modeBPromise)) ??
			30 * 60 * 1000;
		// Race the entire drain sequence (both modes + serve promise) against
		// a single grace ceiling. By default, this uses the engine-provided
		// actor stop threshold, matching Pegboard's hard cutoff for actors.
		const drain = async () => {
			// Shut down every live `CoreRegistry` we know about. Mode A
			// (`start()`) and Mode B (`handler()`) each build a separate
			// runtime registry, so one drain fans out to both to honor the
			// spec invariant "single shutdown tears down both modes".
			const registries: Promise<void>[] = [];
			if (modeAPromise !== undefined) {
				registries.push(
					(async () => {
						try {
							const { runtime, registry } = await modeAPromise;
							await runtime.shutdownRegistry(registry);
						} catch (err) {
							logger().warn(
								{ err },
								"runtime registry shutdown errored (mode A)",
							);
						}
					})(),
				);
			}
			if (modeBPromise !== undefined) {
				registries.push(
					(async () => {
						try {
							const { runtime, registry } = await modeBPromise;
							await runtime.shutdownRegistry(registry);
						} catch (err) {
							logger().warn(
								{ error: err },
								"runtime registry shutdown errored (mode B)",
							);
						}
					})(),
				);
			}
			await Promise.all(registries);

			const runtimeServePromise = this.#runtimeServePromise;
			if (runtimeServePromise !== undefined) {
				// Swallow rejection so the race doesn't itself reject; the
				// always-attached `.catch` at the promise assignment site has
				// already logged any serve-side error.
				await runtimeServePromise.catch(() => undefined);
			}
			if (this.#applicationListenerPromise !== undefined) {
				await this.#applicationListenerPromise.catch(() => undefined);
			}
		};
		await Promise.race([
			drain(),
			new Promise<void>((resolve) =>
				setTimeout(resolve, gracePeriodMs).unref?.(),
			),
		]);
	}

	async #actorStopThresholdMs(
		configuredRegistryPromise:
			| ReturnType<typeof buildConfiguredRegistry>
			| undefined,
	): Promise<number | undefined> {
		if (configuredRegistryPromise === undefined) return undefined;
		try {
			const { runtime, registry } = await configuredRegistryPromise;
			const thresholdMs =
				await runtime.registryActorStopThresholdMs?.(registry);
			if (
				thresholdMs !== undefined &&
				Number.isFinite(thresholdMs) &&
				thresholdMs > 0
			) {
				return thresholdMs;
			}
		} catch (err) {
			logger().warn(
				{ err },
				"failed to read actor stop threshold for shutdown grace",
			);
		}
		return undefined;
	}

	#removeSignalHandlers(): void {
		for (const [signal, handler] of Object.entries(
			this.#signalHandlers,
		) as [ShutdownSignal, () => void][]) {
			if (handler) process.removeListener(signal, handler);
		}
		this.#signalHandlers = {};
	}

	public startEnvoy() {
		this.#startEnvoy(this.parseConfig(), true);
	}

	/**
	 * Starts the serverful registry if needed and waits until its envoy has
	 * registered with the Engine. Repeated and concurrent calls share one
	 * startup lifecycle and readiness promise.
	 *
	 * Unlike {@link start}, this reports startup failures and does not resolve
	 * merely because an HTTP health endpoint is listening.
	 */
	public startAndWait(): Promise<void> {
		if (this.#shutdownInFlight !== null) {
			return Promise.reject(
				new Error(
					"registry.startAndWait() cannot run after shutdown has begun",
				),
			);
		}
		if (this.#runtimeReadyPromise) return this.#runtimeReadyPromise;
		if (getRivetkitRuntimeMode() === "serverless") {
			return Promise.reject(
				new Error(
					"registry.startAndWait() requires envoy runtime mode; serverless registries become ready per request",
				),
			);
		}

		const config = this.parseConfig();
		if (config.runtime === "wasm") {
			return Promise.reject(
				new Error(
					"registry.startAndWait() requires the native runtime; WebAssembly registries do not host an envoy",
				),
			);
		}
		this.#startEnvoy(config, true);
		const configuredRegistryPromise = this.#runtimeServeConfiguredPromise;
		const serveLifecyclePromise = this.#runtimeServeLifecyclePromise;
		if (!configuredRegistryPromise || !serveLifecyclePromise) {
			throw new Error("registry envoy startup did not initialize");
		}

		let timeout: ReturnType<typeof setTimeout> | undefined;
		const timeoutPromise = new Promise<never>((_resolve, reject) => {
			timeout = setTimeout(
				() =>
					reject(
						new Error(
							`RivetKit registry did not register with the Engine within ${REGISTRY_READY_TIMEOUT_MS}ms`,
						),
					),
				REGISTRY_READY_TIMEOUT_MS,
			);
			timeout.unref?.();
		});
		const readinessPromise = (async () => {
			const { runtime, registry } = await configuredRegistryPromise;
			const stoppedBeforeReady = serveLifecyclePromise.then(() => {
				throw new Error(
					"RivetKit registry stopped before becoming ready",
				);
			});
			await Promise.race([
				runtime.waitRegistryReady(registry),
				stoppedBeforeReady,
			]);
		})();
		this.#runtimeReadyPromise = Promise.race([
			readinessPromise,
			timeoutPromise,
		]).finally(() => {
			if (timeout !== undefined) clearTimeout(timeout);
		});
		// The native registry is consumed once serve begins. Retrying the same
		// instance after failure could duplicate partially-created resources, so
		// retain the rejected promise and return the same failure to later calls.
		this.#runtimeReadyPromise.catch(() => {});
		return this.#runtimeReadyPromise;
	}

	/**
	 * Starts the actor envoy for standalone server deployments.
	 *
	 * Set `RIVETKIT_RUNTIME_MODE=serverless` to instead bind an HTTP listener
	 * via `listen()` (Mode B). Mode A (envoy) and Mode B (listener) are
	 * mutually exclusive per registry instance.
	 *
	 * @example
	 * ```ts
	 * const registry = setup({ use: { counter } });
	 * registry.start();
	 * ```
	 */
	public start() {
		if (getRivetkitRuntimeMode() === "serverless") {
			// start() defaults publicDir to "/public" unless overridden by env.
			const publicDir = getRivetkitPublicDir() ?? "/public";
			// Detached listener; bind failures are fatal so exit hard.
			this.listen({ publicDir }).catch((error) => {
				logger().error({ error }, "auto-listen failed; exiting");
				if (
					typeof process !== "undefined" &&
					typeof process.exit === "function"
				) {
					process.exit(1);
				}
			});
			return;
		}
		const config = this.parseConfig();
		this.#startEnvoy(config, true);
	}

	#printWelcome(
		config: RegistryConfig,
		kind: "serverless" | "serverful",
		listener?: { port: number; host?: string; publicDir?: string },
	): void {
		if (config.noWelcome || this.#welcomePrinted) return;
		this.#welcomePrinted = true;

		const logLine = (label: string, value: string) => {
			const padding = " ".repeat(Math.max(0, 13 - label.length));
			console.log(`  - ${label}:${padding}${value}`);
		};

		console.log();
		console.log(
			`  RivetKit ${VERSION} (Engine - ${kind === "serverless" ? "Serverless" : "Serverful"})`,
		);

		if (config.namespace !== "default") {
			logLine("Namespace", config.namespace);
		}

		if (config.endpoint) {
			const endpointType =
				config.startEngine || isLocalEngineEndpoint(config.endpoint)
					? "local native"
					: "remote";
			logLine("Endpoint", `${config.endpoint} (${endpointType})`);
		}

		if (kind === "serverless" && config.publicEndpoint) {
			logLine("Client", config.publicEndpoint);
		}

		logLine("Actors", Object.keys(config.use).length.toString());

		if (listener) {
			const host = listener.host ?? "0.0.0.0";
			logLine("Listening", `http://${host}:${listener.port}`);
			if (listener.publicDir) {
				logLine("Public Dir", listener.publicDir);
			}
		}

		console.log();
	}
}

function isServerlessStartRequest(request: Request, basePath: string): boolean {
	if (request.method !== "POST") return false;
	const parsed = new URL(request.url);
	const normalizedBase =
		basePath === "/" ? "" : `/${basePath.replace(/^\/+|\/+$/g, "")}`;
	return parsed.pathname === `${normalizedBase}/start`;
}

function isServerlessMetadataRequest(
	request: Request,
	basePath: string,
): boolean {
	if (request.method !== "GET") return false;
	const parsed = new URL(request.url);
	const normalizedBase =
		basePath === "/" ? "" : `/${basePath.replace(/^\/+|\/+$/g, "")}`;
	return parsed.pathname === `${normalizedBase}/metadata`;
}

function jsonRouteResponse(status: number, body: unknown): Response {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "content-type": "application/json" },
	});
}

export function setup<A extends RegistryActors>(
	input: RegistryConfigInput<A>,
): Registry<A> {
	return new Registry(input);
}

export type { RegistryConfig, RegistryActors };
export { RegistryConfigSchema };
