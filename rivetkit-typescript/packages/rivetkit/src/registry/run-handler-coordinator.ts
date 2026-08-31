import {
	createRunInspector,
	getRunInspectorKind,
	type RunControl,
	type RunInspectorConfig,
	type RunInspectorFactoryResult,
	type RunInspectorKind,
} from "@/actor/config";
import { RivetError } from "@/actor/errors";

type RunHandler = ((...args: any[]) => any) | { run: (...args: any[]) => any };

interface ActorRunState {
	active: boolean;
	closed: boolean;
	exclusive: boolean;
	exclusiveGeneration: number;
	exclusiveOutcome?: "success" | "failure";
	queued: number;
	restart?: () => void | Promise<void>;
	inspectorInitialized: boolean;
	inspector?: RunInspectorFactoryResult;
	waiters: Set<() => void>;
}

type RunHandlerOutcome = "ran" | "suppressed" | "closed";

function runUnavailable(actorId: string, reason: string): RivetError {
	return new RivetError(
		"actor",
		"run_handler_unavailable",
		`Run handler control is unavailable for actor ${actorId}: ${reason}.`,
		{
			public: true,
			statusCode: 409,
			metadata: { actorId, reason },
		},
	);
}

function notifyStateChanged(state: ActorRunState): void {
	const waiters = [...state.waiters];
	state.waiters.clear();
	for (const waiter of waiters) {
		waiter();
	}
}

function waitForStateChange(state: ActorRunState): Promise<void> {
	return new Promise((resolve) => {
		state.waiters.add(resolve);
	});
}

/** Coordinates a run handler and its Inspector controls within one registry. */
export class RunHandlerCoordinator {
	readonly #run: RunHandler | undefined;
	readonly #states = new Map<string, ActorRunState>();

	constructor(run: RunHandler | undefined) {
		this.#run = run;
	}

	get inspectorKind(): RunInspectorKind | undefined {
		return getRunInspectorKind(this.#run);
	}

	async run(
		actorId: string,
		restart: () => void | Promise<void>,
		callback: () => void | Promise<void>,
	): Promise<RunHandlerOutcome> {
		const state = this.#getOrCreate(actorId);
		state.restart = restart;
		this.#initializeInspector(actorId, state);
		state.queued += 1;
		const blockedGeneration = state.exclusive
			? state.exclusiveGeneration
			: undefined;

		try {
			while (!state.closed && (state.exclusive || state.active)) {
				await waitForStateChange(state);
			}
		} finally {
			state.queued -= 1;
		}

		if (state.closed) {
			return "closed";
		}
		if (
			blockedGeneration !== undefined &&
			state.exclusiveGeneration === blockedGeneration &&
			state.exclusiveOutcome === "failure"
		) {
			return "suppressed";
		}

		state.active = true;
		try {
			await callback();
			return "ran";
		} finally {
			state.active = false;
			notifyStateChanged(state);
		}
	}

	getInspector(
		actorId: string,
		restart: () => void | Promise<void>,
	): RunInspectorConfig | undefined {
		const state = this.#getOrCreate(actorId);
		state.restart = restart;
		this.#initializeInspector(actorId, state);
		return state.inspector?.inspector;
	}

	destroy(actorId: string): void {
		const state = this.#states.get(actorId);
		if (!state) return;

		state.closed = true;
		state.inspector?.dispose?.();
		state.inspector = undefined;
		notifyStateChanged(state);
		this.#states.delete(actorId);
	}

	#control(actorId: string, state: ActorRunState): RunControl {
		return {
			run: {
				withInactive: async (options, callback) => {
					if (state.closed) {
						throw runUnavailable(actorId, "actor is destroyed");
					}
					if (state.active || state.queued > 0 || state.exclusive) {
						throw runUnavailable(
							actorId,
							"the run handler is active or waiting to start",
						);
					}

					state.exclusive = true;
					state.exclusiveGeneration += 1;
					state.exclusiveOutcome = undefined;
					try {
						const result = await callback();
						if (state.closed) {
							throw runUnavailable(actorId, "actor is destroyed");
						}
						state.exclusiveOutcome = "success";
						if (options.restartOnSuccess && state.queued === 0) {
							if (!state.restart) {
								throw runUnavailable(
									actorId,
									"the runtime restart callback is not available",
								);
							}
							await state.restart();
						}
						return result;
					} catch (error) {
						state.exclusiveOutcome = "failure";
						throw error;
					} finally {
						state.exclusive = false;
						notifyStateChanged(state);
					}
				},
			},
		};
	}

	#getOrCreate(actorId: string): ActorRunState {
		let state = this.#states.get(actorId);
		if (!state) {
			state = {
				active: false,
				closed: false,
				exclusive: false,
				exclusiveGeneration: 0,
				inspectorInitialized: false,
				queued: 0,
				waiters: new Set(),
			};
			this.#states.set(actorId, state);
		}
		return state;
	}

	#initializeInspector(actorId: string, state: ActorRunState): void {
		if (state.inspectorInitialized) return;
		const inspector = createRunInspector(this.#run, {
			actorId,
			control: this.#control(actorId, state),
		});
		if (this.inspectorKind === "workflow") {
			const workflow =
				inspector === undefined
					? undefined
					: inspector.inspector.workflow;
			if (
				!workflow ||
				typeof workflow.getHistory !== "function" ||
				typeof workflow.getState !== "function" ||
				typeof workflow.onHistoryUpdated !== "function" ||
				typeof workflow.replayFromStep !== "function"
			) {
				throw new TypeError(
					`defineRunHandler createInspector returned an invalid workflow adapter for actor ${actorId}`,
				);
			}
		}
		state.inspector = inspector;
		state.inspectorInitialized = true;
	}
}
