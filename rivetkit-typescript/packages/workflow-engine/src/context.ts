import type { Logger } from "pino";
import type { EngineDriver } from "./driver.js";
import {
	extractErrorInfo,
	getErrorEventTag,
	markErrorReported,
} from "./error-utils.js";
import {
	CancelledError,
	CriticalError,
	EntryInProgressError,
	EvictedError,
	HistoryDivergedError,
	JoinError,
	MessageWaitError,
	RaceError,
	RollbackCheckpointError,
	RollbackError,
	RollbackStopError,
	SleepError,
	StepExhaustedError,
	StepFailedError,
} from "./errors.js";
import { buildEntryMetadataKey, buildLoopIterationRange } from "./keys.js";
import {
	appendLoopIteration,
	appendName,
	emptyLocation,
	isLocationPrefix,
	isLoopIterationMarker,
	locationToKey,
	registerName,
} from "./location.js";
import {
	createEntry,
	deleteEntriesWithPrefix,
	flush,
	getOrCreateMetadata,
	loadMetadata,
	type PendingDeletions,
	setEntry,
} from "./storage.js";
import type {
	BranchConfig,
	BranchOutput,
	Entry,
	EntryKindType,
	EntryMetadata,
	Location,
	LoopConfig,
	LoopIterationResult,
	LoopResult,
	Message,
	RollbackContextInterface,
	StepConfig,
	Storage,
	TryBlockCatchKind,
	TryBlockConfig,
	TryBlockFailure,
	TryBlockResult,
	TryStepCatchKind,
	TryStepConfig,
	TryStepFailure,
	TryStepResult,
	WorkflowContextInterface,
	WorkflowError,
	WorkflowErrorEvent,
	WorkflowErrorHandler,
	WorkflowMessageDriver,
	WorkflowQueue,
	WorkflowQueueMessage,
	WorkflowQueueNextBatchOptions,
	WorkflowQueueNextOptions,
} from "./types.js";

/**
 * Default values for step configuration.
 * These are exported so users can reference them when overriding.
 */
export const DEFAULT_MAX_RETRIES = 3;
export const DEFAULT_RETRY_BACKOFF_BASE = 100;
export const DEFAULT_RETRY_BACKOFF_MAX = 30000;
export const DEFAULT_LOOP_HISTORY_PRUNE_INTERVAL = 20;
export const DEFAULT_STEP_TIMEOUT = 30000; // 30 seconds
const DEFAULT_TRY_STEP_CATCH: readonly TryStepCatchKind[] = [
	"critical",
	"timeout",
	"exhausted",
];
const DEFAULT_TRY_BLOCK_CATCH: readonly TryBlockCatchKind[] = [
	"step",
	"join",
	"race",
];

const QUEUE_HISTORY_MESSAGE_MARKER = "__rivetWorkflowQueueMessage";
const TRY_STEP_FAILURE_SYMBOL = Symbol("workflow.try-step.failure");
const TRY_BLOCK_FAILURE_SYMBOL = Symbol("workflow.try-block.failure");

/**
 * Calculate backoff delay with exponential backoff.
 * Uses deterministic calculation (no jitter) for replay consistency.
 */
function calculateBackoff(attempts: number, base: number, max: number): number {
	// Exponential backoff without jitter for determinism
	return Math.min(max, base * 2 ** attempts);
}

/**
 * Error thrown when a step times out.
 */
export class StepTimeoutError extends Error {
	constructor(
		public readonly stepName: string,
		public readonly timeoutMs: number,
	) {
		super(`Step "${stepName}" timed out after ${timeoutMs}ms`);
		this.name = "StepTimeoutError";
	}
}

type SchedulerYieldState = {
	deadline?: number;
	messageNames: Set<string>;
};

type TryBlockFailureInfo = Pick<TryBlockFailure, "source" | "name">;

function attachTryStepFailure<T extends Error>(
	error: T,
	failure: TryStepFailure,
): T {
	(
		error as T & {
			[TRY_STEP_FAILURE_SYMBOL]?: TryStepFailure;
		}
	)[TRY_STEP_FAILURE_SYMBOL] = failure;
	return error;
}

function readTryStepFailure(error: unknown): TryStepFailure | undefined {
	if (!(error instanceof Error)) {
		return undefined;
	}

	return (
		error as Error & {
			[TRY_STEP_FAILURE_SYMBOL]?: TryStepFailure;
		}
	)[TRY_STEP_FAILURE_SYMBOL];
}

function attachTryBlockFailure<T extends Error>(
	error: T,
	failure: TryBlockFailureInfo,
): T {
	(
		error as T & {
			[TRY_BLOCK_FAILURE_SYMBOL]?: TryBlockFailureInfo;
		}
	)[TRY_BLOCK_FAILURE_SYMBOL] = failure;
	return error;
}

function readTryBlockFailure(error: unknown): TryBlockFailureInfo | undefined {
	if (!(error instanceof Error)) {
		return undefined;
	}

	return (
		error as Error & {
			[TRY_BLOCK_FAILURE_SYMBOL]?: TryBlockFailureInfo;
		}
	)[TRY_BLOCK_FAILURE_SYMBOL];
}

function shouldRethrowTryError(error: unknown): boolean {
	return (
		error instanceof StepFailedError ||
		error instanceof SleepError ||
		error instanceof MessageWaitError ||
		error instanceof EvictedError ||
		error instanceof HistoryDivergedError ||
		error instanceof EntryInProgressError ||
		error instanceof RollbackCheckpointError ||
		error instanceof RollbackStopError
	);
}

function shouldCatchTryStepFailure(
	failure: TryStepFailure,
	catchKinds?: readonly TryStepCatchKind[],
): boolean {
	const effectiveCatch = catchKinds ?? DEFAULT_TRY_STEP_CATCH;
	return effectiveCatch.includes(failure.kind);
}

function shouldCatchTryBlockFailure(
	failure: TryBlockFailure,
	catchKinds?: readonly TryBlockCatchKind[],
): boolean {
	const effectiveCatch = catchKinds ?? DEFAULT_TRY_BLOCK_CATCH;

	if (failure.source === "step") {
		return failure.step?.kind === "rollback"
			? effectiveCatch.includes("rollback")
			: effectiveCatch.includes("step");
	}
	if (failure.source === "join") {
		return effectiveCatch.includes("join");
	}
	if (failure.source === "race") {
		return effectiveCatch.includes("race");
	}
	return effectiveCatch.includes("rollback");
}

function parseStoredWorkflowError(message: string | undefined): WorkflowError {
	if (!message) {
		return {
			name: "Error",
			message: "unknown error",
		};
	}

	const match = /^([^:]+):\s*(.*)$/s.exec(message);
	if (!match) {
		return {
			name: "Error",
			message,
		};
	}

	return {
		name: match[1],
		message: match[2],
	};
}

function getTryStepFailureFromExhaustedError(
	stepName: string,
	attempts: number,
	error: StepExhaustedError,
): TryStepFailure {
	return {
		kind: "exhausted",
		stepName,
		attempts,
		error: parseStoredWorkflowError(error.lastError),
	};
}

function mergeSchedulerYield(
	state: SchedulerYieldState | undefined,
	error: SleepError | MessageWaitError | StepFailedError,
): SchedulerYieldState {
	const nextState: SchedulerYieldState = state ?? {
		messageNames: new Set<string>(),
	};

	if (error instanceof SleepError) {
		nextState.deadline =
			nextState.deadline === undefined
				? error.deadline
				: Math.min(nextState.deadline, error.deadline);
		for (const messageName of error.messageNames ?? []) {
			nextState.messageNames.add(messageName);
		}
		return nextState;
	}

	if (error instanceof MessageWaitError) {
		for (const messageName of error.messageNames) {
			nextState.messageNames.add(messageName);
		}
		return nextState;
	}

	nextState.deadline =
		nextState.deadline === undefined
			? error.retryAt
			: Math.min(nextState.deadline, error.retryAt);
	return nextState;
}

function buildSchedulerYieldError(
	state: SchedulerYieldState,
): SleepError | MessageWaitError {
	const messageNames = [...state.messageNames];
	if (state.deadline !== undefined) {
		return new SleepError(
			state.deadline,
			messageNames.length > 0 ? messageNames : undefined,
		);
	}
	return new MessageWaitError(messageNames);
}

function controlFlowErrorPriority(error: Error): number {
	if (error instanceof EvictedError) {
		return 0;
	}
	if (error instanceof HistoryDivergedError) {
		return 1;
	}
	if (error instanceof EntryInProgressError) {
		return 2;
	}
	if (error instanceof RollbackCheckpointError) {
		return 3;
	}
	if (error instanceof RollbackStopError) {
		return 4;
	}
	return 5;
}

function selectControlFlowError(
	current: Error | undefined,
	candidate: Error,
): Error {
	if (!current) {
		return candidate;
	}
	return controlFlowErrorPriority(candidate) <
		controlFlowErrorPriority(current)
		? candidate
		: current;
}

/**
 * Internal representation of a rollback handler.
 */
export interface RollbackAction<T = unknown> {
	entryId: string;
	name: string;
	output: T;
	rollback: (ctx: RollbackContextInterface, output: T) => Promise<void>;
}

/**
 * Internal implementation of WorkflowContext.
 */
export class WorkflowContextImpl implements WorkflowContextInterface {
	private entryInProgress = false;
	private abortController: AbortController;
	private currentLocation: Location;
	private visitedKeys: Set<string>;
	private mode: "forward" | "rollback";
	private rollbackActions?: RollbackAction[];
	private rollbackCheckpointSet: boolean;
	/** Track names used in current execution to detect duplicates */
	private usedNamesInExecution = new Set<string>();
	private pendingCompletableMessageIds = new Set<string>();
	private historyNotifier?: () => void;
	private onError?: WorkflowErrorHandler;
	private logger?: Logger;

	constructor(
		public readonly workflowId: string,
		private storage: Storage,
		private driver: EngineDriver,
		private messageDriver: WorkflowMessageDriver,
		location: Location = emptyLocation(),
		abortController?: AbortController,
		mode: "forward" | "rollback" = "forward",
		rollbackActions?: RollbackAction[],
		rollbackCheckpointSet = false,
		historyNotifier?: () => void,
		onError?: WorkflowErrorHandler,
		logger?: Logger,
		visitedKeys?: Set<string>,
	) {
		this.currentLocation = location;
		this.abortController = abortController ?? new AbortController();
		this.mode = mode;
		this.rollbackActions = rollbackActions;
		this.rollbackCheckpointSet = rollbackCheckpointSet;
		this.historyNotifier = historyNotifier;
		this.onError = onError;
		this.logger = logger;
		this.visitedKeys = visitedKeys ?? new Set();
	}

	get abortSignal(): AbortSignal {
		return this.abortController.signal;
	}

	get queue(): WorkflowQueue {
		return {
			next: async (name, opts) => await this.queueNext(name, opts),
			nextBatch: async (name, opts) =>
				await this.queueNextBatch(name, opts),
			send: async (name, body) => await this.queueSend(name, body),
		};
	}

	isEvicted(): boolean {
		return this.abortSignal.aborted;
	}

	private assertNotInProgress(): void {
		if (this.entryInProgress) {
			throw new EntryInProgressError();
		}
	}

	private checkEvicted(): void {
		if (this.abortSignal.aborted) {
			throw new EvictedError();
		}
	}

	private async flushStorage(): Promise<void> {
		await flush(this.storage, this.driver, this.historyNotifier);
	}

	/**
	 * Create a new branch context for parallel/nested execution.
	 */
	createBranch(
		location: Location,
		abortController?: AbortController,
	): WorkflowContextImpl {
		return new WorkflowContextImpl(
			this.workflowId,
			this.storage,
			this.driver,
			this.messageDriver,
			location,
			abortController ?? this.abortController,
			this.mode,
			this.rollbackActions,
			this.rollbackCheckpointSet,
			this.historyNotifier,
			this.onError,
			this.logger,
			this.visitedKeys,
		);
	}

	/**
	 * Log a debug message using the configured logger.
	 */
	private log(
		level: "debug" | "info" | "warn" | "error",
		data: Record<string, unknown>,
	): void {
		if (!this.logger) return;
		this.logger[level](data);
	}

	private async notifyError(event: WorkflowErrorEvent): Promise<void> {
		if (!this.onError) {
			return;
		}

		try {
			await this.onError(event);
		} catch (error) {
			this.log("warn", {
				msg: "workflow error hook failed",
				hookEventType: getErrorEventTag(event),
				error: extractErrorInfo(error),
			});
		}
	}

	private async notifyStepError<T>(
		config: StepConfig<T>,
		attempt: number,
		error: unknown,
		opts: {
			willRetry: boolean;
			retryDelay?: number;
			retryAt?: number;
		},
	): Promise<void> {
		const maxRetries = config.maxRetries ?? DEFAULT_MAX_RETRIES;
		await this.notifyError({
			step: {
				workflowId: this.workflowId,
				stepName: config.name,
				attempt,
				maxRetries,
				remainingRetries: Math.max(0, maxRetries - (attempt - 1)),
				willRetry: opts.willRetry,
				retryDelay: opts.retryDelay,
				retryAt: opts.retryAt,
				error: extractErrorInfo(error),
			},
		});
	}

	/**
	 * Mark a key as visited.
	 */
	private markVisited(key: string): void {
		this.visitedKeys.add(key);
	}

	/**
	 * Mark every surviving history entry belonging to already-completed
	 * iterations of a resuming loop as visited.
	 *
	 * A loop that resumes from saved state starts at its persisted iteration
	 * and never replays earlier iterations, so their history entries are never
	 * re-visited on this run. When the loop is nested inside another loop (or
	 * any branch that calls validateComplete), the enclosing branch validates
	 * its entire subtree and would otherwise reject those unvisited entries as
	 * a history divergence. Marking them visited here keeps the enclosing
	 * validation consistent with the loop's intentional skip. Entries for the
	 * resumed iteration and later are left alone so they are still validated as
	 * the loop replays forward.
	 */
	private markCompletedLoopIterationsVisited(
		loopLocation: Location,
		resumedIteration: number,
	): void {
		if (resumedIteration <= 0) {
			return;
		}

		const loopSegment = loopLocation[loopLocation.length - 1];
		if (typeof loopSegment !== "number") {
			throw new Error("Expected loop location to end with a name index");
		}

		for (const [key, entry] of this.storage.history.entries) {
			if (!isLocationPrefix(loopLocation, entry.location)) {
				continue;
			}

			const iterationSegment = entry.location[loopLocation.length];
			if (
				!iterationSegment ||
				!isLoopIterationMarker(iterationSegment) ||
				iterationSegment.loop !== loopSegment ||
				iterationSegment.iteration >= resumedIteration
			) {
				continue;
			}

			this.markVisited(key);
		}
	}

	/**
	 * Check if a name has already been used at the current location in this execution.
	 * Throws HistoryDivergedError if duplicate detected.
	 */
	private checkDuplicateName(name: string): void {
		const fullKey = `${locationToKey(this.storage, this.currentLocation)}/${name}`;
		if (this.usedNamesInExecution.has(fullKey)) {
			throw new HistoryDivergedError(
				`Duplicate entry name "${name}" at location "${locationToKey(this.storage, this.currentLocation)}". ` +
					`Each step/loop/sleep/queue.next/join/race must have a unique name within its scope.`,
			);
		}
		this.usedNamesInExecution.add(fullKey);
	}

	private stopRollback(): never {
		throw new RollbackStopError();
	}

	private stopRollbackIfMissing(entry: Entry | undefined): void {
		if (this.mode === "rollback" && !entry) {
			this.stopRollback();
		}
	}

	private stopRollbackIfIncomplete(condition: boolean): void {
		if (this.mode === "rollback" && condition) {
			this.stopRollback();
		}
	}

	private registerRollbackAction<T>(
		config: StepConfig<T>,
		entryId: string,
		output: T,
		metadata: EntryMetadata,
	): void {
		if (!config.rollback) {
			return;
		}
		if (metadata.rollbackCompletedAt !== undefined) {
			return;
		}
		this.rollbackActions?.push({
			entryId,
			name: config.name,
			output: output as unknown,
			rollback: config.rollback as (
				ctx: RollbackContextInterface,
				output: unknown,
			) => Promise<void>,
		});
	}

	/**
	 * Ensure a rollback checkpoint exists before registering rollback handlers.
	 */
	private ensureRollbackCheckpoint<T>(config: StepConfig<T>): void {
		if (!config.rollback) {
			return;
		}
		if (!this.rollbackCheckpointSet) {
			throw new RollbackCheckpointError();
		}
	}

	/**
	 * Validate that all expected entries in the branch were visited.
	 * Throws HistoryDivergedError if there are unvisited entries.
	 */
	validateComplete(): void {
		const prefix = locationToKey(this.storage, this.currentLocation);

		for (const key of this.storage.history.entries.keys()) {
			// Check if this key is under our current location prefix
			// Handle root prefix (empty string) specially - all keys are under root
			const isUnderPrefix =
				prefix === ""
					? true // Root: all keys are children
					: key.startsWith(`${prefix}/`) || key === prefix;

			if (isUnderPrefix) {
				if (!this.visitedKeys.has(key)) {
					// Entry exists in history but wasn't visited
					// This means workflow code may have changed
					throw new HistoryDivergedError(
						`Entry "${key}" exists in history but was not visited. ` +
							`Workflow code may have changed. Use ctx.removed() to handle migrations.`,
					);
				}
			}
		}
	}

	/**
	 * Evict the workflow.
	 */
	evict(): void {
		this.abortController.abort(new EvictedError());
	}

	/**
	 * Wait for `ms`, rejecting early with EvictedError if the workflow is
	 * evicted. Both the timer and the abort listener are torn down on either
	 * outcome, so a completed sleep never leaves a dangling listener on the
	 * long-lived run abort signal.
	 */
	private async sleepOrEvict(ms: number): Promise<void> {
		if (this.abortSignal.aborted) {
			throw new EvictedError();
		}
		let timer: ReturnType<typeof setTimeout> | undefined;
		let onAbort: (() => void) | undefined;
		try {
			await new Promise<void>((resolve, reject) => {
				timer = setTimeout(resolve, ms);
				onAbort = () => reject(new EvictedError());
				this.abortSignal.addEventListener("abort", onAbort, {
					once: true,
				});
			});
		} finally {
			if (timer !== undefined) {
				clearTimeout(timer);
			}
			if (onAbort) {
				this.abortSignal.removeEventListener("abort", onAbort);
			}
		}
	}

	// === Step ===

	async step<T>(
		nameOrConfig: string | StepConfig<T>,
		run?: () => Promise<T>,
	): Promise<T> {
		this.assertNotInProgress();
		this.checkEvicted();

		const config: StepConfig<T> =
			typeof nameOrConfig === "string"
				? { name: nameOrConfig, run: run! }
				: nameOrConfig;

		this.entryInProgress = true;
		try {
			return await this.executeStep(config);
		} finally {
			this.entryInProgress = false;
		}
	}

	async tryStep<T>(
		nameOrConfig: string | TryStepConfig<T>,
		run?: () => Promise<T>,
	): Promise<TryStepResult<T>> {
		const config =
			typeof nameOrConfig === "string"
				? ({
						name: nameOrConfig,
						run: run!,
					} satisfies TryStepConfig<T>)
				: nameOrConfig;

		try {
			return {
				ok: true,
				value: await this.step(config),
			};
		} catch (error) {
			if (shouldRethrowTryError(error)) {
				throw error;
			}

			const failure = readTryStepFailure(error);
			if (!failure || !shouldCatchTryStepFailure(failure, config.catch)) {
				throw error;
			}

			return {
				ok: false,
				failure,
			};
		}
	}

	async try<T>(
		nameOrConfig: string | TryBlockConfig<T>,
		run?: (ctx: WorkflowContextInterface) => Promise<T>,
	): Promise<TryBlockResult<T>> {
		this.assertNotInProgress();
		this.checkEvicted();

		const config =
			typeof nameOrConfig === "string"
				? ({
						name: nameOrConfig,
						run: run!,
					} satisfies TryBlockConfig<T>)
				: nameOrConfig;

		this.entryInProgress = true;
		try {
			return await this.executeTry(config);
		} finally {
			this.entryInProgress = false;
		}
	}

	private async executeTry<T>(
		config: TryBlockConfig<T>,
	): Promise<TryBlockResult<T>> {
		this.checkDuplicateName(config.name);

		const location = appendName(
			this.storage,
			this.currentLocation,
			config.name,
		);
		const blockCtx = this.createBranch(location);

		try {
			const value = await config.run(blockCtx);
			blockCtx.validateComplete();
			return {
				ok: true,
				value,
			};
		} catch (error) {
			if (shouldRethrowTryError(error)) {
				throw error;
			}

			const stepFailure = readTryStepFailure(error);
			if (stepFailure) {
				const failure: TryBlockFailure = {
					source: "step",
					name: stepFailure.stepName,
					error: stepFailure.error,
					step: stepFailure,
				};
				if (!shouldCatchTryBlockFailure(failure, config.catch)) {
					throw error;
				}
				return {
					ok: false,
					failure,
				};
			}

			const operationFailure = readTryBlockFailure(error);
			if (operationFailure) {
				const failure: TryBlockFailure = {
					...operationFailure,
					error: extractErrorInfo(error),
				};
				if (!shouldCatchTryBlockFailure(failure, config.catch)) {
					throw error;
				}
				return {
					ok: false,
					failure,
				};
			}

			if (error instanceof RollbackError) {
				const failure: TryBlockFailure = {
					source: "block",
					name: config.name,
					error: extractErrorInfo(error),
				};
				if (!shouldCatchTryBlockFailure(failure, config.catch)) {
					throw error;
				}
				return {
					ok: false,
					failure,
				};
			}

			throw error;
		}
	}

	private async executeStep<T>(config: StepConfig<T>): Promise<T> {
		this.ensureRollbackCheckpoint(config);
		if (this.mode === "rollback") {
			return await this.executeStepRollback(config);
		}

		// Check for duplicate name in current execution
		this.checkDuplicateName(config.name);

		const location = appendName(
			this.storage,
			this.currentLocation,
			config.name,
		);
		const key = locationToKey(this.storage, location);
		const existing = this.storage.history.entries.get(key);

		// Mark this entry as visited for validateComplete
		this.markVisited(key);

		if (existing) {
			if (existing.kind.type !== "step") {
				throw new HistoryDivergedError(
					`Expected step "${config.name}" at ${key}, found ${existing.kind.type}`,
				);
			}

			const stepData = existing.kind.data;

			const metadata = await loadMetadata(
				this.storage,
				this.driver,
				existing.id,
			);

			// Replay successful result (including void steps).
			if (
				metadata.status === "completed" ||
				stepData.output !== undefined
			) {
				return stepData.output as T;
			}

			// Check if we should retry
			const maxRetries = config.maxRetries ?? DEFAULT_MAX_RETRIES;

			if (metadata.attempts > maxRetries) {
				const lastError = metadata.error;
				const exhaustedError = new StepExhaustedError(
					config.name,
					lastError,
				);
				attachTryStepFailure(
					exhaustedError,
					getTryStepFailureFromExhaustedError(
						config.name,
						metadata.attempts,
						exhaustedError,
					),
				);
				markErrorReported(exhaustedError);
				if (metadata.status !== "exhausted") {
					metadata.status = "exhausted";
					metadata.dirty = true;
					await this.flushStorage();
					await this.notifyStepError(
						config,
						metadata.attempts,
						exhaustedError,
						{ willRetry: false },
					);
				}
				throw exhaustedError;
			}

			// Calculate backoff and yield to scheduler
			// This allows the workflow to be evicted during backoff
			const backoffDelay = calculateBackoff(
				metadata.attempts,
				config.retryBackoffBase ?? DEFAULT_RETRY_BACKOFF_BASE,
				config.retryBackoffMax ?? DEFAULT_RETRY_BACKOFF_MAX,
			);
			const retryAt = metadata.lastAttemptAt + backoffDelay;
			const now = Date.now();

			if (now < retryAt) {
				// Yield to scheduler - will be woken up at retryAt
				throw new SleepError(retryAt);
			}
		}

		// Execute the step
		const entry =
			existing ?? createEntry(location, { type: "step", data: {} });
		if (!existing) {
			// New entry - register name
			this.log("debug", {
				msg: "executing new step",
				step: config.name,
				key,
			});
			const nameIndex = registerName(this.storage, config.name);
			entry.location = [...location];
			entry.location[entry.location.length - 1] = nameIndex;
			setEntry(this.storage, location, entry);
		} else {
			this.log("debug", { msg: "retrying step", step: config.name, key });
		}

		const metadata = getOrCreateMetadata(this.storage, entry.id);
		const maxRetries = config.maxRetries ?? DEFAULT_MAX_RETRIES;
		const retryBackoffBase =
			config.retryBackoffBase ?? DEFAULT_RETRY_BACKOFF_BASE;
		const retryBackoffMax =
			config.retryBackoffMax ?? DEFAULT_RETRY_BACKOFF_MAX;
		metadata.status = "running";
		metadata.attempts++;
		metadata.lastAttemptAt = Date.now();
		metadata.dirty = true;

		// Get timeout configuration
		const timeout = config.timeout ?? DEFAULT_STEP_TIMEOUT;

		try {
			// Execute with timeout
			const output = await this.executeWithTimeout(
				config.run(),
				timeout,
				config.name,
			);

			if (entry.kind.type === "step") {
				entry.kind.data.output = output;
			}
			entry.dirty = true;
			metadata.status = "completed";
			metadata.error = undefined;
			metadata.completedAt = Date.now();

			// Ephemeral steps don't trigger an immediate flush. This avoids the
			// synchronous write overhead for transient operations. Note that the
			// step's entry is still marked dirty and WILL be persisted on the
			// next flush from a non-ephemeral operation. The purpose of ephemeral
			// is to batch writes, not to avoid persistence entirely.
			if (!config.ephemeral) {
				this.log("debug", {
					msg: "flushing step",
					step: config.name,
					key,
				});
				await this.flushStorage();
			}

			this.log("debug", {
				msg: "step completed",
				step: config.name,
				key,
			});
			return output;
		} catch (error) {
			if (entry.kind.type === "step") {
				entry.kind.data.error = String(error);
			}
			entry.dirty = true;

			// Timeout errors are treated as critical by default. Steps opt
			// into retrying on timeout with retryOnTimeout: true.
			if (error instanceof StepTimeoutError && !config.retryOnTimeout) {
				metadata.status = "exhausted";
				metadata.error = String(error);
				await this.notifyStepError(config, metadata.attempts, error, {
					willRetry: false,
				});
				throw markErrorReported(
					attachTryStepFailure(new CriticalError(error.message), {
						kind: "timeout",
						stepName: config.name,
						attempts: metadata.attempts,
						error: extractErrorInfo(error),
					}),
				);
			}

			if (
				error instanceof CriticalError ||
				error instanceof RollbackError
			) {
				metadata.status = "exhausted";
				metadata.error = String(error);
				await this.notifyStepError(config, metadata.attempts, error, {
					willRetry: false,
				});
				throw markErrorReported(
					attachTryStepFailure(error, {
						kind:
							error instanceof RollbackError
								? "rollback"
								: "critical",
						stepName: config.name,
						attempts: metadata.attempts,
						error: extractErrorInfo(error),
					}),
				);
			}

			const willRetry = metadata.attempts <= maxRetries;
			metadata.status = willRetry ? "failed" : "exhausted";
			metadata.error = String(error);

			if (willRetry) {
				const retryDelay = calculateBackoff(
					metadata.attempts,
					retryBackoffBase,
					retryBackoffMax,
				);
				const retryAt = metadata.lastAttemptAt + retryDelay;
				await this.notifyStepError(config, metadata.attempts, error, {
					willRetry: true,
					retryDelay,
					retryAt,
				});
				throw new StepFailedError(
					config.name,
					error,
					metadata.attempts,
					retryAt,
				);
			}

			const exhaustedError = markErrorReported(
				attachTryStepFailure(
					new StepExhaustedError(config.name, String(error)),
					{
						kind:
							error instanceof StepTimeoutError
								? "timeout"
								: "exhausted",
						stepName: config.name,
						attempts: metadata.attempts,
						error: extractErrorInfo(error),
					},
				),
			);
			await this.notifyStepError(config, metadata.attempts, error, {
				willRetry: false,
			});
			throw exhaustedError;
		}
	}

	/**
	 * Execute a promise with timeout.
	 *
	 * Note: This does NOT cancel the underlying operation. JavaScript Promises
	 * cannot be cancelled once started. When a timeout occurs:
	 * - The step is rejected with StepTimeoutError. By default this is treated
	 *   as a critical failure with no retry. Set retryOnTimeout: true on the
	 *   step config to retry timeouts like any other error.
	 * - The underlying async operation continues running in the background
	 * - Any side effects from the operation may still occur
	 *
	 * For cancellable operations, pass ctx.abortSignal to APIs that support AbortSignal:
	 *
	 *     return fetch(url, { signal: ctx.abortSignal });

	 *   });
	 *
	 * Or check ctx.isEvicted() periodically in long-running loops.
	 */
	private async executeStepRollback<T>(config: StepConfig<T>): Promise<T> {
		this.checkDuplicateName(config.name);
		this.ensureRollbackCheckpoint(config);

		const location = appendName(
			this.storage,
			this.currentLocation,
			config.name,
		);
		const key = locationToKey(this.storage, location);
		const existing = this.storage.history.entries.get(key);

		this.markVisited(key);

		if (!existing || existing.kind.type !== "step") {
			this.stopRollback();
		}

		const metadata = await loadMetadata(
			this.storage,
			this.driver,
			existing.id,
		);
		if (metadata.status !== "completed") {
			this.stopRollback();
		}

		const output = existing.kind.data.output as T;
		this.registerRollbackAction(config, existing.id, output, metadata);

		return output;
	}

	private async executeWithTimeout<T>(
		promise: Promise<T>,
		timeoutMs: number,
		stepName: string,
	): Promise<T> {
		if (timeoutMs <= 0) {
			return promise;
		}

		let timeoutId: ReturnType<typeof setTimeout> | undefined;
		const timeoutPromise = new Promise<never>((_, reject) => {
			timeoutId = setTimeout(() => {
				reject(new StepTimeoutError(stepName, timeoutMs));
			}, timeoutMs);
		});

		try {
			return await Promise.race([promise, timeoutPromise]);
		} finally {
			if (timeoutId !== undefined) {
				clearTimeout(timeoutId);
			}
		}
	}

	// === Loop ===

	async loop<S, T>(
		nameOrConfig: string | LoopConfig<S, T>,
		run?: (
			ctx: WorkflowContextInterface,
		) => LoopIterationResult<undefined, T>,
	): Promise<T> {
		this.assertNotInProgress();
		this.checkEvicted();

		const config: LoopConfig<S, T> =
			typeof nameOrConfig === "string"
				? { name: nameOrConfig, run: run as LoopConfig<S, T>["run"] }
				: nameOrConfig;

		this.entryInProgress = true;
		try {
			return await this.executeLoop(config);
		} finally {
			this.entryInProgress = false;
		}
	}

	private async executeLoop<S, T>(config: LoopConfig<S, T>): Promise<T> {
		// Check for duplicate name in current execution
		this.checkDuplicateName(config.name);

		const location = appendName(
			this.storage,
			this.currentLocation,
			config.name,
		);
		const key = locationToKey(this.storage, location);
		const existing = this.storage.history.entries.get(key);

		// Mark this entry as visited for validateComplete
		this.markVisited(key);

		let entry: Entry;
		let metadata: EntryMetadata | undefined;
		let state: S;
		let iteration: number;
		let rollbackSingleIteration = false;
		let rollbackIterationRan = false;
		let rollbackOutput: T | undefined;
		const rollbackMode = this.mode === "rollback";

		if (existing) {
			if (existing.kind.type !== "loop") {
				throw new HistoryDivergedError(
					`Expected loop "${config.name}" at ${key}, found ${existing.kind.type}`,
				);
			}

			const loopData = existing.kind.data;
			metadata = await loadMetadata(
				this.storage,
				this.driver,
				existing.id,
			);

			if (rollbackMode) {
				if (loopData.output !== undefined) {
					return loopData.output as T;
				}
				rollbackSingleIteration = true;
				rollbackIterationRan = false;
				rollbackOutput = undefined;
			}

			if (metadata.status === "completed") {
				return loopData.output as T;
			}

			// Loop already completed
			if (loopData.output !== undefined) {
				return loopData.output as T;
			}

			// Resume from saved state
			entry = existing;
			state = loopData.state as S;
			iteration = loopData.iteration;
			this.markCompletedLoopIterationsVisited(location, iteration);
			if (rollbackMode) {
				rollbackOutput = loopData.output as T | undefined;
				rollbackIterationRan = rollbackOutput !== undefined;
			}
		} else {
			this.stopRollbackIfIncomplete(true);

			// New loop
			state = config.state as S;
			iteration = 0;
			entry = createEntry(location, {
				type: "loop",
				data: { state, iteration },
			});
			setEntry(this.storage, location, entry);
			metadata = getOrCreateMetadata(this.storage, entry.id);
		}

		if (metadata) {
			metadata.status = "running";
			metadata.error = undefined;
			metadata.dirty = true;
		}

		const historyPruneInterval =
			config.historyPruneInterval ??
			config.commitInterval ??
			config.historyEvery ??
			DEFAULT_LOOP_HISTORY_PRUNE_INTERVAL;
		const historySize =
			config.historySize ?? config.historyKeep ?? historyPruneInterval;

		// Track the last iteration we pruned up to so we only delete
		// newly-expired iterations instead of re-scanning from 0.
		let lastPrunedUpTo = 0;

		// Deferred flush promise from the previous prune cycle. Awaited at the
		// start of the next iteration so the flush runs in parallel with user code.
		let deferredFlush: Promise<void> | null = null;

		// Execute loop iterations
		while (true) {
			// Await any deferred flush from the previous prune cycle
			if (deferredFlush) {
				await deferredFlush;
				deferredFlush = null;
			}

			if (rollbackMode && rollbackSingleIteration) {
				if (rollbackIterationRan) {
					return rollbackOutput as T;
				}
				this.stopRollbackIfIncomplete(true);
			}
			this.checkEvicted();

			// Create branch for this iteration
			const iterationLocation = appendLoopIteration(
				this.storage,
				location,
				config.name,
				iteration,
			);
			const branchCtx = this.createBranch(iterationLocation);

			// Execute iteration
			const iterationResult = await config.run(branchCtx, state);
			if (iterationResult === undefined && state !== undefined) {
				throw new Error(
					`Loop "${config.name}" returned undefined for a stateful iteration. Return Loop.continue(state) or Loop.break(value).`,
				);
			}
			const result =
				iterationResult === undefined
					? ({ continue: true, state } as LoopResult<S, T>)
					: iterationResult;

			// Validate branch completed cleanly
			branchCtx.validateComplete();

			if ("break" in result && result.break) {
				// Loop complete
				if (entry.kind.type === "loop") {
					entry.kind.data.output = result.value;
					entry.kind.data.state = state;
					entry.kind.data.iteration = iteration;
				}
				entry.dirty = true;
				if (metadata) {
					metadata.status = "completed";
					metadata.completedAt = Date.now();
					metadata.dirty = true;
				}

				// Collect pruning deletions and flush
				const deletions = this.collectLoopPruning(
					location,
					iteration + 1,
					historySize,
					lastPrunedUpTo,
				);
				await this.flushStorageWithDeletions(deletions);

				if (rollbackMode && rollbackSingleIteration) {
					rollbackOutput = result.value;
					rollbackIterationRan = true;
					continue;
				}

				return result.value;
			}

			// Continue with new state
			if ("continue" in result && result.continue) {
				state = result.state;
			}
			iteration++;

			if (!rollbackMode) {
				if (entry.kind.type === "loop") {
					entry.kind.data.state = state;
					entry.kind.data.iteration = iteration;
				}
				entry.dirty = true;
			}

			// Periodically defer the flush so the next iteration can overlap
			// with loop pruning and any pending dirty state writes.
			if (iteration % historyPruneInterval === 0) {
				const deletions = this.collectLoopPruning(
					location,
					iteration,
					historySize,
					lastPrunedUpTo,
				);
				lastPrunedUpTo = Math.max(0, iteration - historySize);

				// Defer the flush to run in parallel with the next iteration
				deferredFlush = this.flushStorageWithDeletions(deletions);
			}
		}
	}

	/**
	 * Collect pending deletions for loop history pruning.
	 *
	 * Only deletes iterations in the range [fromIteration, keepFrom) where
	 * keepFrom = currentIteration - historySize. This avoids re-scanning
	 * already-deleted iterations.
	 */
	private collectLoopPruning(
		loopLocation: Location,
		currentIteration: number,
		historySize: number,
		fromIteration: number,
	): PendingDeletions | undefined {
		if (currentIteration <= historySize) {
			return undefined;
		}

		const keepFrom = Math.max(0, currentIteration - historySize);
		if (fromIteration >= keepFrom) {
			return undefined;
		}

		const loopSegment = loopLocation[loopLocation.length - 1];
		if (typeof loopSegment !== "number") {
			throw new Error("Expected loop location to end with a name index");
		}

		const range = buildLoopIterationRange(
			loopLocation,
			loopSegment,
			fromIteration,
			keepFrom,
		);
		const metadataKeys: Uint8Array[] = [];

		for (const [key, entry] of this.storage.history.entries) {
			if (!isLocationPrefix(loopLocation, entry.location)) {
				continue;
			}

			const iterationSegment = entry.location[loopLocation.length];
			if (
				!iterationSegment ||
				typeof iterationSegment === "number" ||
				iterationSegment.loop !== loopSegment ||
				iterationSegment.iteration < fromIteration ||
				iterationSegment.iteration >= keepFrom
			) {
				continue;
			}

			metadataKeys.push(buildEntryMetadataKey(entry.id));
			this.storage.entryMetadata.delete(entry.id);
			this.storage.history.entries.delete(key);
		}

		return {
			prefixes: [],
			keys: metadataKeys,
			ranges: [range],
		};
	}

	/**
	 * Flush storage with optional pending deletions so pruning
	 * happens alongside the state write.
	 */
	private async flushStorageWithDeletions(
		deletions?: PendingDeletions,
	): Promise<void> {
		await flush(this.storage, this.driver, this.historyNotifier, deletions);
	}

	// === Sleep ===

	async sleep(name: string, durationMs: number): Promise<void> {
		const deadline = Date.now() + durationMs;
		return this.sleepUntil(name, deadline);
	}

	async sleepUntil(name: string, timestampMs: number): Promise<void> {
		this.assertNotInProgress();
		this.checkEvicted();

		this.entryInProgress = true;
		try {
			await this.executeSleep(name, timestampMs);
		} finally {
			this.entryInProgress = false;
		}
	}

	private async executeSleep(name: string, deadline: number): Promise<void> {
		// Check for duplicate name in current execution
		this.checkDuplicateName(name);

		const location = appendName(this.storage, this.currentLocation, name);
		const key = locationToKey(this.storage, location);
		const existing = this.storage.history.entries.get(key);

		// Mark this entry as visited for validateComplete
		this.markVisited(key);

		let entry: Entry;

		if (existing) {
			if (existing.kind.type !== "sleep") {
				throw new HistoryDivergedError(
					`Expected sleep "${name}" at ${key}, found ${existing.kind.type}`,
				);
			}

			const sleepData = existing.kind.data;

			if (this.mode === "rollback") {
				this.stopRollbackIfIncomplete(sleepData.state === "pending");
				return;
			}

			// Already completed or interrupted
			if (sleepData.state !== "pending") {
				return;
			}

			// Use stored deadline
			deadline = sleepData.deadline;
			entry = existing;
		} else {
			this.stopRollbackIfIncomplete(true);

			entry = createEntry(location, {
				type: "sleep",
				data: { deadline, state: "pending" },
			});
			setEntry(this.storage, location, entry);
			entry.dirty = true;
			await this.flushStorage();
		}

		const now = Date.now();
		const remaining = deadline - now;

		if (remaining <= 0) {
			// Deadline passed
			if (entry.kind.type === "sleep") {
				entry.kind.data.state = "completed";
			}
			entry.dirty = true;
			await this.flushStorage();
			return;
		}

		// Short sleep: wait in memory
		if (remaining < this.driver.workerPollInterval) {
			await this.sleepOrEvict(remaining);

			this.checkEvicted();

			if (entry.kind.type === "sleep") {
				entry.kind.data.state = "completed";
			}
			entry.dirty = true;
			await this.flushStorage();
			return;
		}

		// Long sleep: yield to scheduler
		throw new SleepError(deadline);
	}

	// === Rollback Checkpoint ===

	async rollbackCheckpoint(name: string): Promise<void> {
		this.assertNotInProgress();
		this.checkEvicted();

		this.entryInProgress = true;
		try {
			await this.executeRollbackCheckpoint(name);
		} finally {
			this.entryInProgress = false;
		}
	}

	private async executeRollbackCheckpoint(name: string): Promise<void> {
		this.checkDuplicateName(name);

		const location = appendName(this.storage, this.currentLocation, name);
		const key = locationToKey(this.storage, location);
		const existing = this.storage.history.entries.get(key);

		this.markVisited(key);

		if (existing) {
			if (existing.kind.type !== "rollback_checkpoint") {
				throw new HistoryDivergedError(
					`Expected rollback checkpoint "${name}" at ${key}, found ${existing.kind.type}`,
				);
			}
			this.rollbackCheckpointSet = true;
			return;
		}

		if (this.mode === "rollback") {
			throw new HistoryDivergedError(
				`Missing rollback checkpoint "${name}" at ${key}`,
			);
		}

		const entry = createEntry(location, {
			type: "rollback_checkpoint",
			data: { name },
		});
		setEntry(this.storage, location, entry);
		entry.dirty = true;
		await this.flushStorage();

		this.rollbackCheckpointSet = true;
	}

	// === Queue ===

	private async queueSend(name: string, body: unknown): Promise<void> {
		const message: Message = {
			id: crypto.randomUUID(),
			name,
			data: body,
			sentAt: Date.now(),
		};
		await this.messageDriver.addMessage(message);
	}

	private async queueNext<T>(
		name: string,
		opts?: WorkflowQueueNextOptions,
	): Promise<WorkflowQueueMessage<T>> {
		const messages = await this.queueNextBatch<T>(name, {
			...(opts ?? {}),
			count: 1,
		});
		const message = messages[0];
		if (!message) {
			throw new Error(
				`queue.next("${name}") timed out before receiving a message. Use queue.nextBatch(...) for optional/time-limited reads.`,
			);
		}
		return message;
	}

	private async queueNextBatch<T>(
		name: string,
		opts?: WorkflowQueueNextBatchOptions,
	): Promise<Array<WorkflowQueueMessage<T>>> {
		this.assertNotInProgress();
		this.checkEvicted();

		this.entryInProgress = true;
		try {
			return await this.executeQueueNextBatch<T>(name, opts);
		} finally {
			this.entryInProgress = false;
		}
	}

	private async executeQueueNextBatch<T>(
		name: string,
		opts?: WorkflowQueueNextBatchOptions,
	): Promise<Array<WorkflowQueueMessage<T>>> {
		if (this.pendingCompletableMessageIds.size > 0) {
			throw new Error(
				"Previous completable queue message is not completed. Call `message.complete(...)` before receiving the next message.",
			);
		}

		const resolvedOpts = opts ?? {};
		const messageNames = this.normalizeQueueNames(resolvedOpts.names);
		const messageNameLabel = this.messageNamesLabel(messageNames);
		const count = Math.max(1, resolvedOpts.count ?? 1);
		const completable = resolvedOpts.completable === true;

		this.checkDuplicateName(name);

		const countLocation = appendName(
			this.storage,
			this.currentLocation,
			`${name}:count`,
		);
		const countKey = locationToKey(this.storage, countLocation);
		const existingCount = this.storage.history.entries.get(countKey);
		this.markVisited(countKey);
		this.stopRollbackIfMissing(existingCount);

		let deadline: number | undefined;
		let deadlineEntry: Entry | undefined;
		if (resolvedOpts.timeout !== undefined) {
			const deadlineLocation = appendName(
				this.storage,
				this.currentLocation,
				`${name}:deadline`,
			);
			const deadlineKey = locationToKey(this.storage, deadlineLocation);
			deadlineEntry = this.storage.history.entries.get(deadlineKey);
			this.markVisited(deadlineKey);
			this.stopRollbackIfMissing(deadlineEntry);

			if (deadlineEntry && deadlineEntry.kind.type === "sleep") {
				deadline = deadlineEntry.kind.data.deadline;
			} else {
				deadline = Date.now() + resolvedOpts.timeout;
				const created = createEntry(deadlineLocation, {
					type: "sleep",
					data: { deadline, state: "pending" },
				});
				setEntry(this.storage, deadlineLocation, created);
				created.dirty = true;
				await this.flushStorage();
				deadlineEntry = created;
			}
		}

		if (existingCount && existingCount.kind.type === "message") {
			const replayCount = existingCount.kind.data.data as number;
			return await this.readReplayQueueMessages<T>(
				name,
				replayCount,
				completable,
			);
		}

		const now = Date.now();
		if (deadline !== undefined && now >= deadline) {
			if (deadlineEntry && deadlineEntry.kind.type === "sleep") {
				deadlineEntry.kind.data.state = "completed";
				deadlineEntry.dirty = true;
			}
			await this.recordQueueCountEntry(
				countLocation,
				`${messageNameLabel}:count`,
				0,
			);
			return [];
		}

		const received = await this.receiveMessagesNow(
			messageNames,
			count,
			completable,
		);
		if (received.length > 0) {
			const historyMessages = received.map((message) =>
				this.toWorkflowQueueMessage<T>(message),
			);
			if (deadlineEntry && deadlineEntry.kind.type === "sleep") {
				deadlineEntry.kind.data.state = "interrupted";
				deadlineEntry.dirty = true;
			}
			await this.recordQueueMessages(
				name,
				countLocation,
				messageNames,
				historyMessages,
			);
			const queueMessages = received.map((message, index) =>
				this.createQueueMessage<T>(message, completable, {
					historyLocation: appendName(
						this.storage,
						this.currentLocation,
						`${name}:${index}`,
					),
				}),
			);
			return queueMessages;
		}

		if (deadline === undefined) {
			throw new MessageWaitError(messageNames);
		}
		throw new SleepError(deadline, messageNames);
	}

	private normalizeQueueNames(names?: readonly string[]): string[] {
		if (!names || names.length === 0) {
			return [];
		}
		const deduped: string[] = [];
		const seen = new Set<string>();
		for (const name of names) {
			if (seen.has(name)) {
				continue;
			}
			seen.add(name);
			deduped.push(name);
		}
		return deduped;
	}

	private messageNamesLabel(messageNames: string[]): string {
		if (messageNames.length === 0) {
			return "*";
		}
		return messageNames.length === 1
			? messageNames[0]
			: messageNames.join("|");
	}

	private async receiveMessagesNow(
		messageNames: string[],
		count: number,
		completable: boolean,
	): Promise<Message[]> {
		return await this.messageDriver.receiveMessages({
			names: messageNames.length > 0 ? messageNames : undefined,
			count,
			completable,
		});
	}

	private async recordQueueMessages<T>(
		name: string,
		countLocation: Location,
		messageNames: string[],
		messages: Array<WorkflowQueueMessage<T>>,
	): Promise<void> {
		for (let i = 0; i < messages.length; i++) {
			const messageLocation = appendName(
				this.storage,
				this.currentLocation,
				`${name}:${i}`,
			);
			const messageEntry = createEntry(messageLocation, {
				type: "message",
				data: {
					name: messages[i].name,
					data: this.toHistoryQueueMessage(messages[i]),
				},
			});
			setEntry(this.storage, messageLocation, messageEntry);
			this.markVisited(locationToKey(this.storage, messageLocation));
		}
		await this.recordQueueCountEntry(
			countLocation,
			`${this.messageNamesLabel(messageNames)}:count`,
			messages.length,
		);
	}

	private async recordQueueCountEntry(
		countLocation: Location,
		countLabel: string,
		count: number,
	): Promise<void> {
		const countEntry = createEntry(countLocation, {
			type: "message",
			data: {
				name: countLabel,
				data: count,
			},
		});
		setEntry(this.storage, countLocation, countEntry);
		await this.flushStorage();
	}

	private async readReplayQueueMessages<T>(
		name: string,
		count: number,
		completable: boolean,
	): Promise<Array<WorkflowQueueMessage<T>>> {
		const results: Array<WorkflowQueueMessage<T>> = [];
		for (let i = 0; i < count; i++) {
			const messageLocation = appendName(
				this.storage,
				this.currentLocation,
				`${name}:${i}`,
			);
			const messageKey = locationToKey(this.storage, messageLocation);
			this.markVisited(messageKey);
			const existingMessage =
				this.storage.history.entries.get(messageKey);
			if (!existingMessage || existingMessage.kind.type !== "message") {
				throw new HistoryDivergedError(
					`Expected queue message "${name}:${i}" in history`,
				);
			}
			const parsed = this.fromHistoryQueueMessage(
				existingMessage.kind.data.name,
				existingMessage.kind.data.data,
			);
			results.push(
				this.createQueueMessage<T>(parsed.message, completable, {
					historyLocation: messageLocation,
					completed: parsed.completed,
					replay: true,
				}),
			);
		}
		return results;
	}

	private toWorkflowQueueMessage<T>(
		message: Message,
	): WorkflowQueueMessage<T> {
		return {
			id: message.id,
			name: message.name,
			body: message.data as T,
			createdAt: message.sentAt,
		};
	}

	private createQueueMessage<T>(
		message: Message,
		completable: boolean,
		opts?: {
			historyLocation?: Location;
			completed?: boolean;
			replay?: boolean;
		},
	): WorkflowQueueMessage<T> {
		const queueMessage = this.toWorkflowQueueMessage<T>(message);
		if (!completable) {
			return queueMessage;
		}

		if (opts?.replay && opts.completed) {
			return {
				...queueMessage,
				complete: async () => {
					// No-op: this message was already completed in a prior run.
				},
			};
		}

		const messageId = message.id;
		this.pendingCompletableMessageIds.add(messageId);
		let completed = false;

		return {
			...queueMessage,
			complete: async (response?: unknown) => {
				if (completed) {
					throw new Error("Queue message already completed");
				}
				completed = true;
				try {
					await this.completeMessage(message, response);
					await this.markQueueMessageCompleted(opts?.historyLocation);
					this.pendingCompletableMessageIds.delete(messageId);
				} catch (error) {
					completed = false;
					throw error;
				}
			},
		};
	}

	private async markQueueMessageCompleted(
		historyLocation: Location | undefined,
	): Promise<void> {
		if (!historyLocation) {
			return;
		}
		const key = locationToKey(this.storage, historyLocation);
		const entry = this.storage.history.entries.get(key);
		if (!entry || entry.kind.type !== "message") {
			return;
		}
		const parsed = this.fromHistoryQueueMessage(
			entry.kind.data.name,
			entry.kind.data.data,
		);
		entry.kind.data.data = this.toHistoryQueueMessage(
			this.toWorkflowQueueMessage(parsed.message),
			true,
		);
		entry.dirty = true;
		await this.flushStorage();
	}

	private async completeMessage(
		message: Message,
		response?: unknown,
	): Promise<void> {
		if (message.complete) {
			await message.complete(response);
			return;
		}
		await this.messageDriver.completeMessage(message.id, response);
	}

	private toHistoryQueueMessage(
		message: WorkflowQueueMessage<unknown>,
		completed = false,
	): unknown {
		return {
			[QUEUE_HISTORY_MESSAGE_MARKER]: 1,
			id: message.id,
			name: message.name,
			body: message.body,
			createdAt: message.createdAt,
			completed,
		};
	}

	private fromHistoryQueueMessage(
		name: string,
		value: unknown,
	): { message: Message; completed: boolean } {
		if (
			typeof value === "object" &&
			value !== null &&
			(value as Record<string, unknown>)[QUEUE_HISTORY_MESSAGE_MARKER] ===
				1
		) {
			const serialized = value as Record<string, unknown>;
			const id = typeof serialized.id === "string" ? serialized.id : "";
			const serializedName =
				typeof serialized.name === "string" ? serialized.name : name;
			const createdAt =
				typeof serialized.createdAt === "number"
					? serialized.createdAt
					: 0;
			const completed =
				typeof serialized.completed === "boolean"
					? serialized.completed
					: false;
			return {
				message: {
					id,
					name: serializedName,
					data: serialized.body,
					sentAt: createdAt,
				},
				completed,
			};
		}
		return {
			message: {
				id: "",
				name,
				data: value,
				sentAt: 0,
			},
			completed: false,
		};
	}

	// === Join ===

	async join<T extends Record<string, BranchConfig<unknown>>>(
		name: string,
		branches: T,
	): Promise<{ [K in keyof T]: BranchOutput<T[K]> }> {
		this.assertNotInProgress();
		this.checkEvicted();

		this.entryInProgress = true;
		try {
			return await this.executeJoin(name, branches);
		} finally {
			this.entryInProgress = false;
		}
	}

	private async executeJoin<T extends Record<string, BranchConfig<unknown>>>(
		name: string,
		branches: T,
	): Promise<{ [K in keyof T]: BranchOutput<T[K]> }> {
		// Check for duplicate name in current execution
		this.checkDuplicateName(name);

		const location = appendName(this.storage, this.currentLocation, name);
		const key = locationToKey(this.storage, location);
		const existing = this.storage.history.entries.get(key);

		// Mark this entry as visited for validateComplete
		this.markVisited(key);

		this.stopRollbackIfMissing(existing);

		let entry: Entry;

		if (existing) {
			if (existing.kind.type !== "join") {
				throw new HistoryDivergedError(
					`Expected join "${name}" at ${key}, found ${existing.kind.type}`,
				);
			}
			entry = existing;
		} else {
			entry = createEntry(location, {
				type: "join",
				data: {
					branches: Object.fromEntries(
						Object.keys(branches).map((k) => [
							k,
							{ status: "pending" as const },
						]),
					),
				},
			});
			setEntry(this.storage, location, entry);
			entry.dirty = true;
			// Flush immediately to persist entry before branches execute
			await this.flushStorage();
		}

		if (entry.kind.type !== "join") {
			throw new HistoryDivergedError("Entry type mismatch");
		}

		this.stopRollbackIfIncomplete(
			Object.values(entry.kind.data.branches).some(
				(branch) => branch.status !== "completed",
			),
		);

		const joinData = entry.kind.data;
		const results: Record<string, unknown> = {};
		const errors: Record<string, Error> = {};
		let schedulerYieldState: SchedulerYieldState | undefined;
		let propagatedError: Error | undefined;

		for (const [branchName, branchStatus] of Object.entries(
			joinData.branches,
		)) {
			if (branchStatus.status === "completed") {
				results[branchName] = branchStatus.output;
				continue;
			}

			if (branchStatus.status === "failed") {
				errors[branchName] = new Error(
					branchStatus.error ?? "branch failed",
				);
			}
		}

		// Execute all branches in parallel
		const branchPromises = Object.entries(branches).map(
			async ([branchName, config]) => {
				const branchStatus = joinData.branches[branchName];
				if (!branchStatus) {
					throw new HistoryDivergedError(
						`Expected join branch "${branchName}" in "${name}"`,
					);
				}

				// Already completed
				if (branchStatus.status === "completed") {
					results[branchName] = branchStatus.output;
					return;
				}

				// Already failed
				if (branchStatus.status === "failed") {
					errors[branchName] = new Error(branchStatus.error);
					return;
				}

				// Execute branch
				const branchLocation = appendName(
					this.storage,
					location,
					branchName,
				);
				const branchCtx = this.createBranch(branchLocation);

				branchStatus.status = "running";
				branchStatus.error = undefined;
				entry.dirty = true;

				try {
					const output = await config.run(branchCtx);
					branchCtx.validateComplete();

					branchStatus.status = "completed";
					branchStatus.output = output;
					branchStatus.error = undefined;
					results[branchName] = output;
				} catch (error) {
					if (
						error instanceof SleepError ||
						error instanceof MessageWaitError ||
						error instanceof StepFailedError
					) {
						schedulerYieldState = mergeSchedulerYield(
							schedulerYieldState,
							error,
						);
						branchStatus.status = "running";
						branchStatus.error = undefined;
						entry.dirty = true;
						return;
					}

					if (
						error instanceof EvictedError ||
						error instanceof HistoryDivergedError ||
						error instanceof EntryInProgressError ||
						error instanceof RollbackCheckpointError ||
						error instanceof RollbackStopError
					) {
						propagatedError = selectControlFlowError(
							propagatedError,
							error,
						);
						branchStatus.status = "running";
						branchStatus.error = undefined;
						entry.dirty = true;
						return;
					}

					branchStatus.status = "failed";
					branchStatus.output = undefined;
					branchStatus.error = String(error);
					errors[branchName] = error as Error;
				}

				entry.dirty = true;
			},
		);

		// Wait for ALL branches (no short-circuit on error)
		await Promise.allSettled(branchPromises);
		await this.flushStorage();

		if (propagatedError) {
			throw propagatedError;
		}

		if (
			Object.values(joinData.branches).some(
				(branch) =>
					branch.status === "pending" || branch.status === "running",
			)
		) {
			if (!schedulerYieldState) {
				throw new Error(
					`Join "${name}" has pending branches without a scheduler yield`,
				);
			}
			throw buildSchedulerYieldError(schedulerYieldState);
		}

		// Throw if any branches failed
		if (Object.keys(errors).length > 0) {
			throw attachTryBlockFailure(new JoinError(errors), {
				source: "join",
				name,
			});
		}

		return results as { [K in keyof T]: BranchOutput<T[K]> };
	}

	// === Race ===

	async race<T>(
		name: string,
		branches: Array<{
			name: string;
			run: (ctx: WorkflowContextInterface) => Promise<T>;
		}>,
	): Promise<{ winner: string; value: T }> {
		this.assertNotInProgress();
		this.checkEvicted();

		this.entryInProgress = true;
		try {
			return await this.executeRace(name, branches);
		} finally {
			this.entryInProgress = false;
		}
	}

	private async executeRace<T>(
		name: string,
		branches: Array<{
			name: string;
			run: (ctx: WorkflowContextInterface) => Promise<T>;
		}>,
	): Promise<{ winner: string; value: T }> {
		// Check for duplicate name in current execution
		this.checkDuplicateName(name);

		const location = appendName(this.storage, this.currentLocation, name);
		const key = locationToKey(this.storage, location);
		const existing = this.storage.history.entries.get(key);

		// Mark this entry as visited for validateComplete
		this.markVisited(key);

		this.stopRollbackIfMissing(existing);

		let entry: Entry;

		if (existing) {
			if (existing.kind.type !== "race") {
				throw new HistoryDivergedError(
					`Expected race "${name}" at ${key}, found ${existing.kind.type}`,
				);
			}
			entry = existing;

			// Check if we already have a winner
			const raceKind = existing.kind;
			if (raceKind.data.winner !== null) {
				const winnerStatus =
					raceKind.data.branches[raceKind.data.winner];
				return {
					winner: raceKind.data.winner,
					value: winnerStatus.output as T,
				};
			}

			this.stopRollbackIfIncomplete(true);
		} else {
			entry = createEntry(location, {
				type: "race",
				data: {
					winner: null,
					branches: Object.fromEntries(
						branches.map((b) => [
							b.name,
							{ status: "pending" as const },
						]),
					),
				},
			});
			setEntry(this.storage, location, entry);
			entry.dirty = true;
			// Flush immediately to persist entry before branches execute
			await this.flushStorage();
		}

		if (entry.kind.type !== "race") {
			throw new HistoryDivergedError("Entry type mismatch");
		}

		const raceData = entry.kind.data;

		// Create abort controller for cancellation
		const raceAbortController = new AbortController();

		// Track all branch promises to wait for cleanup
		const branchPromises: Promise<void>[] = [];

		// Track winner info
		let winnerName: string | null = null;
		let winnerValue: T | undefined;
		let hasWinner = false;
		const errors: Record<string, Error> = {};
		const lateErrors: Array<{ name: string; error: string }> = [];
		let schedulerYieldState: SchedulerYieldState | undefined;
		let propagatedError: Error | undefined;

		// Check for replay winners first
		for (const branch of branches) {
			const branchStatus = raceData.branches[branch.name];
			if (!branchStatus) {
				throw new HistoryDivergedError(
					`Expected race branch "${branch.name}" in "${name}"`,
				);
			}
			if (
				branchStatus.status !== "pending" &&
				branchStatus.status !== "running"
			) {
				if (branchStatus.status === "failed") {
					errors[branch.name] = new Error(
						branchStatus.error ?? "branch failed",
					);
				}
				if (branchStatus.status === "completed" && !hasWinner) {
					hasWinner = true;
					winnerName = branch.name;
					winnerValue = branchStatus.output as T;
				}
			}
		}

		// If we found a replay winner, return immediately
		if (hasWinner && winnerName !== null) {
			return { winner: winnerName, value: winnerValue as T };
		}

		// Execute branches that need to run
		for (const branch of branches) {
			const branchStatus = raceData.branches[branch.name];
			if (!branchStatus) {
				throw new HistoryDivergedError(
					`Expected race branch "${branch.name}" in "${name}"`,
				);
			}

			// Skip already completed/cancelled
			if (
				branchStatus.status !== "pending" &&
				branchStatus.status !== "running"
			) {
				continue;
			}

			const branchLocation = appendName(
				this.storage,
				location,
				branch.name,
			);
			const branchCtx = this.createBranch(
				branchLocation,
				raceAbortController,
			);

			branchStatus.status = "running";
			branchStatus.error = undefined;
			entry.dirty = true;

			const branchPromise = branch.run(branchCtx).then(
				async (output) => {
					if (hasWinner) {
						// This branch completed after a winner was determined
						// Still record the completion for observability
						branchStatus.status = "completed";
						branchStatus.output = output;
						branchStatus.error = undefined;
						entry.dirty = true;
						return;
					}

					if (propagatedError) {
						branchStatus.status = "completed";
						branchStatus.output = output;
						branchStatus.error = undefined;
						entry.dirty = true;
						return;
					}

					hasWinner = true;
					winnerName = branch.name;
					winnerValue = output;

					branchCtx.validateComplete();

					branchStatus.status = "completed";
					branchStatus.output = output;
					branchStatus.error = undefined;
					raceData.winner = branch.name;
					entry.dirty = true;

					// Cancel other branches
					raceAbortController.abort();
				},
				(error) => {
					if (hasWinner) {
						if (
							error instanceof CancelledError ||
							error instanceof EvictedError
						) {
							branchStatus.status = "cancelled";
						} else {
							lateErrors.push({
								name: branch.name,
								error: String(error),
							});
						}
						entry.dirty = true;
						return;
					}

					if (
						error instanceof SleepError ||
						error instanceof MessageWaitError ||
						error instanceof StepFailedError
					) {
						schedulerYieldState = mergeSchedulerYield(
							schedulerYieldState,
							error,
						);
						branchStatus.status = "running";
						branchStatus.error = undefined;
						entry.dirty = true;
						return;
					}

					if (
						error instanceof EvictedError ||
						error instanceof HistoryDivergedError ||
						error instanceof EntryInProgressError ||
						error instanceof RollbackCheckpointError ||
						error instanceof RollbackStopError
					) {
						propagatedError = selectControlFlowError(
							propagatedError,
							error,
						);
						branchStatus.status = "running";
						branchStatus.error = undefined;
						entry.dirty = true;
						return;
					}

					if (error instanceof CancelledError) {
						branchStatus.status = "cancelled";
					} else {
						branchStatus.status = "failed";
						branchStatus.output = undefined;
						branchStatus.error = String(error);
						errors[branch.name] = error as Error;
					}
					entry.dirty = true;
				},
			);

			branchPromises.push(branchPromise);
		}

		// Wait for all branches to complete or be cancelled
		await Promise.allSettled(branchPromises);

		if (propagatedError) {
			await this.flushStorage();
			throw propagatedError;
		}

		if (
			!hasWinner &&
			Object.values(raceData.branches).some(
				(branch) =>
					branch.status === "pending" || branch.status === "running",
			)
		) {
			await this.flushStorage();
			if (!schedulerYieldState) {
				throw new Error(
					`Race "${name}" has pending branches without a scheduler yield`,
				);
			}
			throw buildSchedulerYieldError(schedulerYieldState);
		}

		// Clean up entries from non-winning branches
		if (hasWinner && winnerName !== null) {
			for (const branch of branches) {
				if (branch.name !== winnerName) {
					const branchLocation = appendName(
						this.storage,
						location,
						branch.name,
					);
					await deleteEntriesWithPrefix(
						this.storage,
						this.driver,
						branchLocation,
						this.historyNotifier,
					);
				}
			}
		}

		// Flush final state
		await this.flushStorage();

		// Log late errors if any (these occurred after a winner was determined)
		if (lateErrors.length > 0) {
			console.warn(
				`Race "${name}" had ${lateErrors.length} branch(es) fail after winner was determined:`,
				lateErrors,
			);
		}

		// Return result or throw error
		if (hasWinner && winnerName !== null) {
			return { winner: winnerName, value: winnerValue as T };
		}

		// All branches failed
		throw attachTryBlockFailure(
			new RaceError(
				"All branches failed",
				Object.entries(errors).map(([branchName, error]) => ({
					name: branchName,
					error: String(error),
				})),
			),
			{
				source: "race",
				name,
			},
		);
	}

	// === Removed ===

	async removed(name: string, originalType: EntryKindType): Promise<void> {
		this.assertNotInProgress();
		this.checkEvicted();

		this.entryInProgress = true;
		try {
			await this.executeRemoved(name, originalType);
		} finally {
			this.entryInProgress = false;
		}
	}

	private async executeRemoved(
		name: string,
		originalType: EntryKindType,
	): Promise<void> {
		// Check for duplicate name in current execution
		this.checkDuplicateName(name);

		const location = appendName(this.storage, this.currentLocation, name);
		const key = locationToKey(this.storage, location);
		const existing = this.storage.history.entries.get(key);

		// Mark this entry as visited for validateComplete
		this.markVisited(key);

		this.stopRollbackIfMissing(existing);

		if (existing) {
			// Validate the existing entry matches what we expect
			if (
				existing.kind.type !== "removed" &&
				existing.kind.type !== originalType
			) {
				throw new HistoryDivergedError(
					`Expected ${originalType} or removed at ${key}, found ${existing.kind.type}`,
				);
			}

			// If it's not already marked as removed, we just skip it
			return;
		}

		// Create a removed entry placeholder
		const entry = createEntry(location, {
			type: "removed",
			data: { originalType, originalName: name },
		});
		setEntry(this.storage, location, entry);
		await this.flushStorage();
	}

	// === Version ===

	async getVersion(name: string, latest: number): Promise<number> {
		this.assertNotInProgress();
		this.checkEvicted();

		this.entryInProgress = true;
		try {
			return await this.executeGetVersion(name, latest);
		} finally {
			this.entryInProgress = false;
		}
	}

	private async executeGetVersion(
		name: string,
		latest: number,
	): Promise<number> {
		if (!Number.isInteger(latest) || latest < 1) {
			throw new Error(
				`getVersion("${name}", ${latest}): latest must be an integer >= 1`,
			);
		}

		// Check for duplicate name in current execution
		this.checkDuplicateName(name);

		const location = appendName(this.storage, this.currentLocation, name);
		const key = locationToKey(this.storage, location);
		const existing = this.storage.history.entries.get(key);

		// Mark this entry as visited for validateComplete
		this.markVisited(key);

		this.stopRollbackIfMissing(existing);

		if (existing) {
			if (existing.kind.type !== "version_check") {
				throw new HistoryDivergedError(
					`Expected version_check at ${key}, found ${existing.kind.type}`,
				);
			}
			// Pure replay: this instance is already pinned at this location.
			return existing.kind.data.resolved;
		}

		// No recorded version at this location. Decide whether this instance
		// already executed past this point under older code (old in-flight) or
		// is reaching it fresh at the live frontier.
		//
		// The discriminator is "is there any unvisited history entry under the
		// current scope?". Entries created earlier in this same run are already
		// marked visited, so the only unvisited entries under the scope are
		// leftovers from a prior run, which proves this scope already executed
		// under code that predates this gate. Such instances resolve to the
		// implicit floor version 1 (old branch); fresh instances resolve to
		// `latest`.
		const resolved = this.hasUnvisitedUnderCurrentScope(key) ? 1 : latest;

		const entry = createEntry(location, {
			type: "version_check",
			data: { resolved, latest },
		});
		setEntry(this.storage, location, entry);
		await this.flushStorage();

		return resolved;
	}

	/**
	 * Returns true if any history entry under the current location scope
	 * (other than `excludeKey`) has not yet been visited this run. Used by
	 * getVersion to detect an old in-flight instance whose scope was already
	 * executed by code that predates a version gate.
	 */
	private hasUnvisitedUnderCurrentScope(excludeKey: string): boolean {
		const prefix = locationToKey(this.storage, this.currentLocation);

		for (const key of this.storage.history.entries.keys()) {
			if (key === excludeKey) {
				continue;
			}
			const isUnderPrefix =
				prefix === ""
					? true
					: key.startsWith(`${prefix}/`) || key === prefix;
			if (isUnderPrefix && !this.visitedKeys.has(key)) {
				return true;
			}
		}
		return false;
	}
}
