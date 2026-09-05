import type {
	CoreRuntime,
	RegistryHandle,
	RuntimeWorkerRegistration,
	RuntimeWorkerRetireRequest,
	RuntimeWorkerSpawnRequest,
} from "./runtime";
import { logger } from "./log";

const WORKER_BOOTSTRAP_SYMBOL = Symbol.for(
	"rivetkit.actorWorkerThread.bootstrap",
);

export interface ActorWorkerBootstrapData {
	poolId: string;
	workerId: number;
	spawnToken: string;
	class: "baseline" | "overflow";
	entrypoint: string;
}

export interface ActorWorkerBootstrapState extends ActorWorkerBootstrapData {
	claimed: boolean;
	attachPromise?: Promise<void>;
	runtime?: CoreRuntime;
	registry?: RegistryHandle;
	registration?: RuntimeWorkerRegistration;
}

type WorkerStatusMessage =
	| ({ kind: "ready" } & RuntimeWorkerRegistration)
	| { kind: "bootstrapError"; reason: string }
	| ({ kind: "retired" } & RuntimeWorkerRegistration);

interface ManagedWorker {
	worker: import("node:worker_threads").Worker;
	request: RuntimeWorkerSpawnRequest;
	registration?: RuntimeWorkerRegistration;
	spawnFailureReported: boolean;
	bootstrapTimeout: ReturnType<typeof setTimeout>;
	retireFallback?: ReturnType<typeof setTimeout>;
	pendingRetire?: RuntimeWorkerRetireRequest;
	retirementRequested: boolean;
	workerError?: string;
	exited: Promise<void>;
	resolveExited: () => void;
}

export interface NodeActorWorkerPool {
	poolId: string;
	close: () => Promise<void>;
}

const WORKER_BOOTSTRAP_TIMEOUT_MS = 60_000;
const WORKER_RETIRE_TIMEOUT_MS = 5_000;

function requireWorkerRuntimeMethod<T>(method: T | undefined, name: string): T {
	if (method === undefined) {
		throw new Error(
			`actorsPerThread requires a native RivetKit runtime with ${name} support`,
		);
	}
	return method;
}

function stringifyWorkerError(error: unknown): string {
	if (error instanceof Error) return error.stack ?? error.message;
	return String(error);
}

export function getActorWorkerBootstrap():
	| ActorWorkerBootstrapState
	| undefined {
	return (
		globalThis as typeof globalThis & {
			[WORKER_BOOTSTRAP_SYMBOL]?: ActorWorkerBootstrapState;
		}
	)[WORKER_BOOTSTRAP_SYMBOL];
}

export function claimActorWorkerBootstrap():
	| ActorWorkerBootstrapState
	| undefined {
	const bootstrap = getActorWorkerBootstrap();
	if (!bootstrap) return undefined;
	if (bootstrap.claimed) {
		throw new Error(
			"A worker-thread entrypoint attempted to start more than one RivetKit registry",
		);
	}
	bootstrap.claimed = true;
	return bootstrap;
}

export function setActorWorkerAttachPromise(
	bootstrap: ActorWorkerBootstrapState,
	promise: Promise<void>,
): void {
	bootstrap.attachPromise = promise;
}

export async function postActorWorkerReady(
	registration: RuntimeWorkerRegistration,
): Promise<void> {
	const { parentPort } = await import("node:worker_threads");
	if (!parentPort) {
		throw new Error("RivetKit actor worker has no parent MessagePort");
	}
	parentPort.postMessage({ kind: "ready", ...registration });
}

function actorWorkerBootstrapSource(): string {
	return `
import { parentPort, workerData } from "node:worker_threads";
const symbol = Symbol.for("rivetkit.actorWorkerThread.bootstrap");
const state = { ...workerData, claimed: false };
globalThis[symbol] = state;
try {
	if (!parentPort) {
		throw new Error("RivetKit actor worker has no parent MessagePort");
	}
	await import(workerData.entrypoint);
	if (!state.claimed || !state.attachPromise) {
		throw new Error("The worker entrypoint did not start its RivetKit registry");
	}
	await state.attachPromise;
	parentPort.on("message", (message) => {
		if (message?.kind !== "retire") return;
		if (!state.runtime || !state.registry || !state.registration) {
			throw new Error("RivetKit actor worker retired before registration");
		}
		if (
			message.workerId !== state.registration.workerId ||
			message.workerEpoch !== state.registration.workerEpoch
		) {
			throw new Error("RivetKit actor worker received a stale retire request");
		}
		if (!state.runtime.detachWorker) {
			throw new Error("The native runtime does not support worker detach");
		}
		state.runtime.detachWorker(state.registry);
		parentPort.postMessage({
			kind: "retired",
			...state.registration,
		});
	});
} catch (error) {
	const reason = error instanceof Error ? (error.stack || error.message) : String(error);
	parentPort?.postMessage({ kind: "bootstrapError", reason });
	throw error;
}
`;
}

export async function configureNodeActorWorkerPool(
	runtime: CoreRuntime,
	registry: RegistryHandle,
	actorsPerThread: number,
): Promise<NodeActorWorkerPool> {
	const configureWorkerPool = requireWorkerRuntimeMethod(
		runtime.configureWorkerPool?.bind(runtime),
		"worker-pool configuration",
	);
	const workerSpawnFailed = requireWorkerRuntimeMethod(
		runtime.workerSpawnFailed?.bind(runtime),
		"worker spawn failure reporting",
	);
	const workerExited = requireWorkerRuntimeMethod(
		runtime.workerExited?.bind(runtime),
		"worker exit reporting",
	);
	const [{ Worker }, { availableParallelism }, { pathToFileURL }] =
		await Promise.all([
			import("node:worker_threads"),
			import("node:os"),
			import("node:url"),
		]);
	const entrypointPath = process.argv[1];
	if (!entrypointPath) {
		throw new Error(
			"actorsPerThread requires a file-based Node.js entrypoint in process.argv[1]",
		);
	}
	const entrypoint = pathToFileURL(entrypointPath).href;
	const workers = new Map<number, ManagedWorker>();
	const queuedWorkerIds = new Set<number>();
	const spawnQueue: RuntimeWorkerSpawnRequest[] = [];
	let spawnDrainScheduled = false;
	let closing = false;
	let poolId: string;

	const reportSpawnFailure = (
		managed: ManagedWorker,
		reason: string,
	): void => {
		if (managed.registration || managed.spawnFailureReported) return;
		managed.spawnFailureReported = true;
		logger().error(
			{ workerId: managed.request.workerId, error: reason },
			"actor worker thread failed to start",
		);
		workerSpawnFailed(
			registry,
			managed.request.workerId,
			managed.request.spawnToken,
			reason,
		);
	};

	const spawnWorker = (request: RuntimeWorkerSpawnRequest): void => {
		if (closing) return;
		let worker: import("node:worker_threads").Worker;
		try {
			worker = new Worker(
				new URL(
					`data:text/javascript,${encodeURIComponent(actorWorkerBootstrapSource())}`,
				),
				{
					name: `rivetkit-actors-${request.workerId}`,
					workerData: { ...request, poolId, entrypoint },
				},
			);
		} catch (error) {
			workerSpawnFailed(
				registry,
				request.workerId,
				request.spawnToken,
				stringifyWorkerError(error),
			);
			return;
		}
		let resolveExited!: () => void;
		const exited = new Promise<void>((resolve) => {
			resolveExited = resolve;
		});
		const managed: ManagedWorker = {
			worker,
			request,
			spawnFailureReported: false,
			retirementRequested: false,
			bootstrapTimeout: setTimeout(() => {
				reportSpawnFailure(
					managed,
					`worker did not register within ${WORKER_BOOTSTRAP_TIMEOUT_MS}ms`,
				);
				void worker.terminate();
			}, WORKER_BOOTSTRAP_TIMEOUT_MS),
			exited,
			resolveExited,
		};
		managed.bootstrapTimeout.unref?.();
		workers.set(request.workerId, managed);
		worker.on("message", (message: WorkerStatusMessage) => {
			if (message.kind === "ready") {
				if (
					message.workerId !== request.workerId ||
					managed.registration
				) {
					void worker.terminate();
					return;
				}
				managed.registration = message;
				clearTimeout(managed.bootstrapTimeout);
				if (managed.pendingRetire) {
					const pendingRetire = managed.pendingRetire;
					managed.pendingRetire = undefined;
					if (managed.retireFallback) {
						clearTimeout(managed.retireFallback);
						managed.retireFallback = undefined;
					}
					requestRetirement(managed, pendingRetire);
				}
			} else if (message.kind === "bootstrapError") {
				reportSpawnFailure(managed, message.reason);
			} else if (
				managed.registration &&
				message.workerId === managed.registration.workerId &&
				message.workerEpoch === managed.registration.workerEpoch
			) {
				if (managed.retireFallback)
					clearTimeout(managed.retireFallback);
				void worker.terminate();
			}
		});
		worker.on("error", (error) => {
			const reason = stringifyWorkerError(error);
			if (managed.registration) {
				managed.workerError = reason;
			} else {
				reportSpawnFailure(managed, reason);
			}
		});
		worker.on("exit", (code) => {
			clearTimeout(managed.bootstrapTimeout);
			if (managed.retireFallback) clearTimeout(managed.retireFallback);
			workers.delete(request.workerId);
			managed.resolveExited();
			if (managed.registration) {
				if (!closing && !managed.retirementRequested) {
					logger().error(
						{
							workerId: managed.registration.workerId,
							workerEpoch: managed.registration.workerEpoch,
							exitCode: code,
							error: managed.workerError,
						},
						"actor worker thread exited unexpectedly",
					);
				}
				workerExited(
					registry,
					managed.registration.workerId,
					managed.registration.workerEpoch,
				);
			} else {
				reportSpawnFailure(
					managed,
					`worker exited before registration with code ${code}`,
				);
			}
		});
	};

	const scheduleSpawnDrain = (): void => {
		if (closing || spawnDrainScheduled || spawnQueue.length === 0) return;
		spawnDrainScheduled = true;
		setImmediate(() => {
			spawnDrainScheduled = false;
			const request = spawnQueue.shift();
			if (request) {
				queuedWorkerIds.delete(request.workerId);
				spawnWorker(request);
			}
			scheduleSpawnDrain();
		});
	};

	const spawnWorkers = (requests: RuntimeWorkerSpawnRequest[]): void => {
		for (const request of requests) {
			if (closing) return;
			if (
				workers.has(request.workerId) ||
				queuedWorkerIds.has(request.workerId)
			) {
				workerSpawnFailed(
					registry,
					request.workerId,
					request.spawnToken,
					`RivetKit requested duplicate worker id ${request.workerId}`,
				);
				continue;
			}
			queuedWorkerIds.add(request.workerId);
			spawnQueue.push(request);
		}
		scheduleSpawnDrain();
	};

	const requestRetirement = (
		managed: ManagedWorker,
		request: RuntimeWorkerRetireRequest,
	): void => {
		if (managed.retireFallback) return;
		if (!managed.registration) {
			managed.pendingRetire = request;
			managed.retireFallback = setTimeout(() => {
				void managed.worker.terminate();
			}, WORKER_RETIRE_TIMEOUT_MS);
			managed.retireFallback.unref?.();
			return;
		} else if (managed.registration.workerEpoch !== request.workerEpoch) {
			return;
		}
		managed.retirementRequested = true;
		try {
			managed.worker.postMessage({ kind: "retire", ...request });
			managed.retireFallback = setTimeout(() => {
				void managed.worker.terminate();
			}, WORKER_RETIRE_TIMEOUT_MS);
			managed.retireFallback.unref?.();
		} catch {
			void managed.worker.terminate();
		}
	};

	const retireWorker = (request: RuntimeWorkerRetireRequest): void => {
		const managed = workers.get(request.workerId);
		if (!managed) return;
		requestRetirement(managed, request);
	};

	poolId = configureWorkerPool(
		registry,
		actorsPerThread,
		availableParallelism(),
		spawnWorkers,
		retireWorker,
	);
	return {
		poolId,
		close: async () => {
			if (closing) {
				await Promise.all(
					[...workers.values()].map((worker) => worker.exited),
				);
				return;
			}
			closing = true;
			spawnQueue.length = 0;
			queuedWorkerIds.clear();
			for (const managed of workers.values()) {
				if (!managed.registration) void managed.worker.terminate();
			}
			const deadline = new Promise<void>((resolve) => {
				const timeout = setTimeout(resolve, WORKER_RETIRE_TIMEOUT_MS);
				timeout.unref?.();
			});
			await Promise.race([
				Promise.all(
					[...workers.values()].map((worker) => worker.exited),
				),
				deadline,
			]);
			await Promise.all(
				[...workers.values()].map((managed) =>
					managed.worker.terminate(),
				),
			);
		},
	};
}
