import type {
	EventStreamStore,
	FlueContextInternal,
	RunStore,
} from '@flue/runtime/adapter-kit';
import type { WorkflowDefinition } from '@flue/runtime';
import {
	admitDetachedWorkflow,
	failRecoveredRun,
	handleRunRouteRequest,
	handleStreamHead,
	handleStreamRead,
	handleWorkflowRequest,
} from '@flue/runtime/adapter-kit';
import { createRivetRunStore } from './run-store-composite.js';
import { createAsyncRunStore, ensureAsyncSqlSchema, type AsyncSqlDb } from './stores/index.js';

export interface RivetWorkflowActorContext {
	readonly actorId: string;
	readonly name: string;
	readonly key: readonly string[];
	readonly region: string;
	readonly env?: Record<string, unknown>;
	readonly request?: Request;
	readonly abortSignal: AbortSignal;
	readonly db: AsyncSqlDb;
	keepAwake<T>(promise: Promise<T>): Promise<T>;
}

interface RivetWorkflowPreparedCoordinator {
	readonly workflowName: string;
	readonly runStore: RunStore;
}

export interface RivetWorkflowRuntimeOptions {
	readonly workflows: ReadonlyArray<{ name: string; definition: WorkflowDefinition }>;
	readonly createContext: (options: {
		readonly actor: RivetWorkflowActorContext;
		readonly request?: Request;
		readonly runId: string;
		readonly initialEventIndex?: number;
		readonly dispatchId?: string;
	}) => FlueContextInternal;
	readonly createEventStreamStore: (actor: RivetWorkflowActorContext) => EventStreamStore;
	readonly createRegistryRunStore?: (actor: RivetWorkflowActorContext) => RunStore | undefined;
	readonly resolveRunId?: (actor: RivetWorkflowActorContext) => string;
}

export interface RivetWorkflowRuntime {
	prepare(options: {
		readonly db: AsyncSqlDb;
		readonly workflowName: string;
		readonly actor: RivetWorkflowActorContext;
	}): Promise<RivetWorkflowPreparedCoordinator>;
	attach(
		actor: RivetWorkflowActorContext,
		prepared: RivetWorkflowPreparedCoordinator,
	): RivetWorkflowCoordinator;
	onWake(
		actor: RivetWorkflowActorContext,
		inherited?: () => Promise<unknown> | unknown,
	): Promise<void>;
	admitWorkflow(
		actor: RivetWorkflowActorContext,
		input: { readonly runId: string; readonly input: unknown },
	): Promise<{ runId: string }>;
	onRequest(actor: RivetWorkflowActorContext, request: Request): Promise<Response | null>;
}

export function createRivetWorkflowRuntime(options: RivetWorkflowRuntimeOptions): RivetWorkflowRuntime {
	const coordinators = new WeakMap<RivetWorkflowActorContext, RivetWorkflowCoordinator>();

	const getCoordinator = (actor: RivetWorkflowActorContext): RivetWorkflowCoordinator => {
		const coordinator = coordinators.get(actor);
		if (!coordinator) {
			throw new Error('[flue] Generated Rivet workflow coordinator was not initialized.');
		}
		return coordinator;
	};

	return {
		async prepare({ db, workflowName, actor }) {
			await ensureAsyncSqlSchema(db);
			const primary = createAsyncRunStore(db);
			const mirror = options.createRegistryRunStore?.(actor);
			return {
				workflowName,
				runStore: createRivetRunStore(primary, mirror),
			};
		},
		attach(actor, prepared) {
			const coordinator = new RivetWorkflowCoordinator(
				actor,
				prepared,
				options,
				options.createEventStreamStore(actor),
			);
			coordinators.set(actor, coordinator);
			return coordinator;
		},
		onWake(actor, inherited = () => undefined) {
			return getCoordinator(actor).onWake(inherited);
		},
		admitWorkflow(actor, input) {
			return getCoordinator(actor).admitWorkflow(input);
		},
		onRequest(actor, request) {
			return getCoordinator(actor).onRequest(request);
		},
	};
}

export class RivetWorkflowCoordinator {
	private readonly actor: RivetWorkflowActorContext;
	private readonly prepared: RivetWorkflowPreparedCoordinator;
	private readonly options: RivetWorkflowRuntimeOptions;
	private readonly eventStreamStore: EventStreamStore;

	constructor(
		actor: RivetWorkflowActorContext,
		prepared: RivetWorkflowPreparedCoordinator,
		options: RivetWorkflowRuntimeOptions,
		eventStreamStore: EventStreamStore,
	) {
		this.actor = actor;
		this.prepared = prepared;
		this.options = options;
		this.eventStreamStore = eventStreamStore;
	}

	async onWake(inherited: () => Promise<unknown> | unknown): Promise<void> {
		await inherited();
		const run = await this.prepared.runStore.getRun(this.runId);
		if (run?.status !== 'active') return;
		await failRecoveredRun({
			workflowName: this.workflowName,
			runId: this.runId,
			error: new Error(
				'Flue workflow execution was interrupted. Start a new workflow run explicitly if retry is appropriate.',
			),
			runStore: this.prepared.runStore,
			eventStreamStore: this.eventStreamStore,
			createContext: ({ runId, request, initialEventIndex }) =>
				this.createContext(request, runId, initialEventIndex),
		});
	}

	async onRequest(request: Request): Promise<Response | null> {
		const runRoute = parseRunRoute(request);
		if (runRoute) {
			if (runRoute.action === 'ds-stream') {
				const streamPath = runStreamPath(runRoute.runId);
				if (request.method === 'HEAD') return handleStreamHead(this.eventStreamStore, streamPath);
				return handleStreamRead({ store: this.eventStreamStore, path: streamPath, request });
			}
			return handleRunRouteRequest({
				workflowName: this.workflowName,
				runId: runRoute.runId,
				runStore: this.prepared.runStore,
			});
		}

		if (!parseWorkflowStart(request, this.workflowName)) return null;
		const workflow = this.options.workflows.find((record) => record.name === this.workflowName)?.definition;
		if (!workflow) return null;
		return handleWorkflowRequest({
			request,
			workflowName: this.workflowName,
			runId: this.runId,
			workflow,
			runStore: this.prepared.runStore,
			eventStreamStore: this.eventStreamStore,
			createContext: ({ runId, request, initialEventIndex }) =>
				this.createContext(request, runId, initialEventIndex),
			startWorkflowAdmission: (_runId, run) => {
				const completion = this.actor.keepAwake(Promise.resolve().then(run));
				return { admitted: Promise.resolve(), completion };
			},
		});
	}

	async admitWorkflow(options: {
		readonly runId: string;
		readonly input: unknown;
	}): Promise<{ runId: string }> {
		const workflow = this.options.workflows.find(
			(record) => record.name === this.workflowName,
		)?.definition;
		if (!workflow) {
			throw new Error(`[flue] Workflow target "${this.workflowName}" is unavailable.`);
		}
		return admitDetachedWorkflow({
			workflowName: this.workflowName,
			runId: options.runId,
			workflow,
			input: options.input,
			runStore: this.prepared.runStore,
			eventStreamStore: this.eventStreamStore,
			createContext: ({ runId, request, initialEventIndex }) =>
				this.createContext(request, runId, initialEventIndex),
			startWorkflowAdmission: (_runId, run) => {
				const completion = this.actor.keepAwake(Promise.resolve().then(run));
				return { admitted: Promise.resolve(), completion };
			},
		});
	}

	private get workflowName(): string {
		return this.prepared.workflowName;
	}

	private get runId(): string {
		return this.options.resolveRunId?.(this.actor) ?? this.actor.key[0] ?? this.actor.actorId;
	}

	private createContext(
		request: Request | undefined,
		runId: string,
		initialEventIndex?: number,
	): FlueContextInternal {
		return this.options.createContext({
			actor: this.actor,
			request,
			runId,
			initialEventIndex,
		});
	}

}

function parseWorkflowStart(request: Request, workflowName: string): boolean {
	if (request.method !== 'POST') return false;
	const url = new URL(request.url);
	const segments = url.pathname.split('/').filter(Boolean);
	if (segments.length !== 2 || segments[0] !== 'workflows') return false;
	return decodeURIComponent(segments[1] ?? '') === workflowName;
}

function parseRunRoute(request: Request): { action: 'ds-stream' | 'meta'; runId: string } | null {
	const url = new URL(request.url);
	const segments = url.pathname.split('/').filter(Boolean);
	if (segments.length !== 2 || segments[0] !== 'runs') return null;
	const runId = decodeURIComponent(segments[1] ?? '');
	if (!runId) return null;
	return { action: request.method === 'GET' && url.searchParams.has('meta') ? 'meta' : 'ds-stream', runId };
}

function runStreamPath(runId: string): string {
	return `runs/${runId}`;
}
