import type {
	AgentDefinition,
	AgentExecutionStore,
	AgentSubmission,
	AgentSubmissionStore,
	AttachmentStore,
	ConversationStreamStore,
	DeliveredMessage,
	DispatchInput,
	FlueContextInternal,
	FlueTraceCarrier,
} from '@flue/runtime/adapter-kit';
import type { DispatchReceipt } from '@flue/runtime';
import {
	ConversationRecordWriter,
	SubmissionAbortedError,
	agentStreamPath,
	agentSubmissionDispatchId,
	assertAgentDispatchAdmissionInput,
	createDirectAgentSubmissionInput,
	createDispatchAgentSubmissionInput,
	createSessionStorageKey,
	handleAgentAttachmentRead,
	handleAgentConversationHead,
	handleAgentConversationRead,
	handleAgentRequest,
	materializeAgentSubmissionSession,
	processSubmission,
	reconcileInterruptedSubmission,
	submissionSyntheticRequest,
} from '@flue/runtime/adapter-kit';
import {
	createAsyncSqlStores,
	ensureAsyncSqlSchema,
	type AsyncSqlDb,
} from './stores/index.js';

const WAKE_ACTION = '__flueWakeAgentSubmissions';
const WAKE_SECONDS = 30;
const SUBMISSION_HARNESS_NAME = 'default';
const SUBMISSION_SESSION_NAME = 'default';

export interface RivetAgentActorContext {
	readonly actorId: string;
	readonly name: string;
	readonly key: readonly string[];
	readonly region: string;
	readonly env?: Record<string, unknown>;
	readonly request?: Request;
	readonly abortSignal: AbortSignal;
	readonly db: AsyncSqlDb;
	readonly schedule?: { after(duration: number, action: string, ...args: unknown[]): Promise<void> };
	keepAwake<T>(promise: Promise<T>): Promise<T>;
}

interface PreparedCoordinator {
	readonly agentName: string;
	readonly executionStore: AgentExecutionStore;
	readonly conversationStreamStore: ConversationStreamStore;
	readonly attachmentStore: AttachmentStore;
}

export interface RivetAgentRuntimeOptions {
	readonly agents: ReadonlyArray<{ name: string; definition: AgentDefinition }>;
	readonly createContext: (options: {
		readonly executionStore: AgentExecutionStore;
		readonly actor: RivetAgentActorContext;
		readonly agentName: string;
		readonly request: Request;
		readonly initialEventIndex?: number;
		readonly dispatchId?: string;
	}) => FlueContextInternal;
	readonly resolveInstanceId?: (actor: RivetAgentActorContext) => string;
}

export interface RivetAgentRuntime {
	prepare(options: { db: AsyncSqlDb; agentName: string }): Promise<PreparedCoordinator>;
	attach(actor: RivetAgentActorContext, prepared: PreparedCoordinator): RivetAgentCoordinator;
	onWake(actor: RivetAgentActorContext, inherited?: () => Promise<unknown> | unknown): Promise<void>;
	wakeSubmissions(actor: RivetAgentActorContext): Promise<void>;
	admitDispatch(actor: RivetAgentActorContext, input: DispatchInput): Promise<DispatchReceipt>;
	onRequest(actor: RivetAgentActorContext, request: Request): Promise<Response | null>;
}

export function createRivetAgentRuntime(options: RivetAgentRuntimeOptions): RivetAgentRuntime {
	const coordinators = new WeakMap<RivetAgentActorContext, RivetAgentCoordinator>();
	const activeAttempts = new Set<string>();
	const get = (actor: RivetAgentActorContext) => {
		const coordinator = coordinators.get(actor);
		if (!coordinator) throw new Error('[flue] Rivet agent coordinator was not initialized.');
		return coordinator;
	};
	return {
		async prepare({ db, agentName }) {
			await ensureAsyncSqlSchema(db);
			const stores = createAsyncSqlStores(db);
			return {
				agentName,
				executionStore: stores.executionStore,
				conversationStreamStore: stores.conversationStreamStore,
				attachmentStore: stores.attachmentStore,
			};
		},
		attach(actor, prepared) {
			const coordinator = new RivetAgentCoordinator(actor, prepared, options, activeAttempts);
			coordinators.set(actor, coordinator);
			return coordinator;
		},
		onWake: (actor, inherited = () => undefined) => get(actor).onWake(inherited),
		wakeSubmissions: (actor) => get(actor).wakeSubmissions(),
		admitDispatch: (actor, input) => get(actor).admitDispatch(input),
		onRequest: (actor, request) => get(actor).onRequest(request),
	};
}

export class RivetAgentCoordinator {
	private conversationWriter: ConversationRecordWriter | undefined;
	private conversationWriterCreation: Promise<ConversationRecordWriter> | undefined;
	private conversationMaterialization: Promise<void> = Promise.resolve();
	private activeControllers = new Map<string, AbortController>();

	constructor(
		private readonly actor: RivetAgentActorContext,
		private readonly prepared: PreparedCoordinator,
		private readonly options: RivetAgentRuntimeOptions,
		private readonly activeAttempts: Set<string>,
	) {}

	async onWake(inherited: () => Promise<unknown> | unknown): Promise<void> {
		await this.restoreSubmissionWake();
		await inherited();
		await this.reconcileSubmissions({ driverAlreadyArmed: true });
	}

	async wakeSubmissions(): Promise<void> {
		if (!(await this.submissions.hasUnsettledSubmissions())) return;
		await this.armSubmissionWake();
		await this.reconcileSubmissions({ driverAlreadyArmed: true });
	}

	onRequest(request: Request): Promise<Response | null> {
		return this.routeRequest(request);
	}

	private async routeRequest(request: Request): Promise<Response | null> {
		if (isAbortRequest(request, this.agentName, this.instanceId)) {
			return Response.json({ aborted: await this.abortInstance() });
		}
		if (request.method === 'GET' || request.method === 'HEAD') {
			const path = agentStreamPath(this.agentName, this.instanceId);
			const segments = new URL(request.url).pathname.split('/');
			const attachmentId = request.method === 'GET' &&
				segments.at(-2) === 'attachments' &&
				decodeURIComponent(segments.at(-3) ?? '') === this.instanceId &&
				decodeURIComponent(segments.at(-4) ?? '') === this.agentName
				? decodeURIComponent(segments.at(-1) ?? '') : undefined;
			if (attachmentId) return handleAgentAttachmentRead({
				conversationStore: this.prepared.conversationStreamStore,
				attachmentStore: this.prepared.attachmentStore,
				path,
				attachmentId,
			});
			if (request.method === 'HEAD') {
				return handleAgentConversationHead(this.prepared.conversationStreamStore, path);
			}
			return handleAgentConversationRead({
				store: this.prepared.conversationStreamStore,
				path,
				request,
			});
		}
		return handleAgentRequest({
			request,
			id: this.instanceId,
			agentName: this.agentName,
			admitAttachedSubmission: (message, traceCarrier) =>
				this.admitAttachedSubmission(message, traceCarrier),
		});
	}

	private get agentName() { return this.prepared.agentName; }
	private get executionStore() { return this.prepared.executionStore; }
	private get submissions(): AgentSubmissionStore { return this.executionStore.submissions; }
	private get instanceId() {
		return this.options.resolveInstanceId?.(this.actor) ?? this.actor.key[0] ?? this.actor.actorId;
	}

	private async ensureConversationWriter(): Promise<ConversationRecordWriter> {
		if (this.conversationWriter && !this.conversationWriter.failed) return this.conversationWriter;
		if (!this.conversationWriterCreation) {
			const creation = ConversationRecordWriter.create({
				store: this.prepared.conversationStreamStore,
				path: agentStreamPath(this.agentName, this.instanceId),
				identity: { agentName: this.agentName, instanceId: this.instanceId },
				producerId: this.actor.actorId,
				onFailed: (writer) => {
					if (this.conversationWriter === writer) this.conversationWriter = undefined;
				},
			});
			this.conversationWriterCreation = creation;
			void creation.then((writer) => {
				if (!writer.failed) this.conversationWriter = writer;
				if (this.conversationWriterCreation === creation) this.conversationWriterCreation = undefined;
			}, () => {
				if (this.conversationWriterCreation === creation) this.conversationWriterCreation = undefined;
			});
		}
		return this.conversationWriterCreation;
	}

	private createContext(request: Request, dispatchId?: string): FlueContextInternal {
		return this.options.createContext({
			executionStore: this.executionStore,
			actor: this.actor,
			agentName: this.agentName,
			request,
			dispatchId,
		});
	}

	private createDurableContext(request: Request, dispatchId?: string): FlueContextInternal {
		const ctx = this.createContext(request, dispatchId);
		ctx.setConversationWriter?.(this.conversationWriter);
		ctx.setAttachmentStore?.(this.prepared.attachmentStore);
		return ctx;
	}

	private armSubmissionWake(): Promise<void> {
		return this.actor.schedule?.after(WAKE_SECONDS, WAKE_ACTION) ?? Promise.resolve();
	}

	private async restoreSubmissionWake(): Promise<boolean> {
		if (!(await this.submissions.hasUnsettledSubmissions())) return false;
		await this.armSubmissionWake();
		return true;
	}

	private async reconcileSubmissions(options: { driverAlreadyArmed?: boolean } = {}): Promise<boolean> {
		if (!(await this.submissions.hasUnsettledSubmissions())) return false;
		if (!options.driverAlreadyArmed) await this.restoreSubmissionWake();
		for (const submission of await this.submissions.listUnreadySubmissions()) {
			const agent = this.resolveAgent(submission.input.agent);
			try {
				await this.materializeSubmissionConversation(submission.input, agent);
				await this.submissions.markSubmissionCanonicalReady(submission.submissionId);
			} catch (error) { this.logFailure(submission, 'materialize_submission', error); }
		}
		for (const settlement of await this.submissions.listPendingSubmissionSettlements()) {
			const submission = await this.submissions.getSubmission(settlement.submissionId);
			if (!submission || this.activeAttempts.has(this.attemptKey(submission))) continue;
			const writer = await this.ensureConversationWriter();
			const attempt = { submissionId: settlement.submissionId, attemptId: settlement.attemptId };
			const canonical = await writer.getRecord(settlement.recordId);
			if (!canonical) await writer.append([settlement.record], { submission: attempt });
			else if (JSON.stringify(canonical) !== JSON.stringify(settlement.record)) {
				throw new Error('[flue] Pending settlement conflicts with its canonical record.');
			}
			await this.submissions.finalizeSubmissionSettlement(attempt, settlement.recordId);
		}
		for (const submission of await this.submissions.listRunningSubmissions()) {
			if (this.activeAttempts.has(this.attemptKey(submission))) continue;
			try { await this.reconcileInterrupted(submission); }
			catch (error) { this.logFailure(submission, 'reconcile_submission', error); }
		}
		for (const submission of await this.submissions.listRunnableSubmissions()) {
			const claimed = await this.submissions.claimSubmission({
				submissionId: submission.submissionId,
				attemptId: crypto.randomUUID(),
				ownerId: this.actor.actorId,
				leaseExpiresAt: 0,
			});
			if (claimed) this.startSubmissionAttempt(claimed);
		}
		return this.submissions.hasUnsettledSubmissions();
	}

	private resolveAgent(name: string): AgentDefinition {
		const agent = this.options.agents.find((record) => record.name === name)?.definition;
		if (!agent) throw new Error(`[flue] Agent target "${name}" is unavailable.`);
		return agent;
	}

	private async reconcileInterrupted(submission: AgentSubmission): Promise<void> {
		const writer = await this.ensureConversationWriter();
		const replacement = await reconcileInterruptedSubmission(
			this.submissions,
			submission,
			this.resolveAgent(this.agentName),
			(dispatchId) => this.createDurableContext(submissionSyntheticRequest(submission.input), dispatchId),
			{ ownerId: this.actor.actorId, leaseExpiresAt: 0 },
			writer,
		);
		if (replacement) this.startSubmissionAttempt(replacement);
	}

	private startSubmissionAttempt(submission: AgentSubmission): void {
		if (submission.status !== 'running' || !submission.attemptId) return;
		const key = this.attemptKey(submission);
		if (this.activeAttempts.has(key)) return;
		this.activeAttempts.add(key);
		const controller = new AbortController();
		this.activeControllers.set(submission.submissionId, controller);
		const attempt = { submissionId: submission.submissionId, attemptId: submission.attemptId };
		const running = Promise.resolve().then(async () => {
			await this.submissions.insertAttemptMarker(attempt);
			await this.processSubmissionEntry(submission, controller.signal);
		});
		void this.actor.keepAwake(running).catch((error) => this.logFailure(submission, 'process_submission', error))
			.finally(() => {
				this.activeAttempts.delete(key);
				this.activeControllers.delete(submission.submissionId);
				void this.submissions.deleteAttemptMarker(attempt).catch(() => {});
			});
	}

	private async processSubmissionEntry(submission: AgentSubmission, signal: AbortSignal): Promise<void> {
		const writer = await this.ensureConversationWriter();
		await processSubmission({
			submissions: this.submissions,
			submission,
			resolveAgent: (name) => this.resolveAgent(name),
			createContext: (dispatchId) =>
				this.createDurableContext(submissionSyntheticRequest(submission.input), dispatchId),
			conversationWriter: writer,
			signal,
			onSettled: () => { this.startSubmissionReconciliation(); },
		});
	}

	private startSubmissionReconciliation(): void {
		const running = Promise.resolve().then(
			() => this.reconcileSubmissions({ driverAlreadyArmed: true }),
		);
		void this.actor.keepAwake(running).catch((error) => {
			console.error('[flue:submission-reconciliation]', {
				agentName: this.agentName,
				instanceId: this.instanceId,
			}, error);
		});
	}

	private materializeSubmissionConversation(
		input: AgentSubmission['input'],
		agent: AgentDefinition,
	): Promise<void> {
		const operation = this.conversationMaterialization.then(async () => {
			await this.ensureConversationWriter();
			const ctx = this.createDurableContext(submissionSyntheticRequest(input), agentSubmissionDispatchId(input));
			await materializeAgentSubmissionSession(ctx, agent, input, this.prepared.attachmentStore);
		});
		this.conversationMaterialization = operation.catch(() => {});
		return operation;
	}

	private async admitAttachedSubmission(message: DeliveredMessage, traceCarrier?: FlueTraceCarrier) {
		const input = createDirectAgentSubmissionInput({
			agent: this.agentName,
			id: this.instanceId,
			message,
			traceCarrier,
		});
		const admitted = await this.submissions.admitDirect(input);
		if (admitted.canonicalReadyAt === null) {
			await this.materializeSubmissionConversation(input, this.resolveAgent(this.agentName));
			await this.submissions.markSubmissionCanonicalReady(input.submissionId);
		}
		const offset = (await this.ensureConversationWriter()).offset;
		await this.armSubmissionWake();
		this.startSubmissionReconciliation();
		return { submissionId: input.submissionId, offset };
	}

	async admitDispatch(input: DispatchInput): Promise<DispatchReceipt> {
		assertAgentDispatchAdmissionInput(input);
		if (input.agent !== this.agentName || input.id !== this.instanceId) {
			throw new Error('[flue] Invalid dispatch target.');
		}
		const agent = this.resolveAgent(this.agentName);
		const admission = await this.submissions.admitDispatch(input);
		if (admission.kind === 'retained_receipt') return {
			dispatchId: admission.receipt.submissionId,
			acceptedAt: new Date(admission.receipt.acceptedAt).toISOString(),
		};
		if (admission.kind === 'conflict') {
			throw new Error('[flue] Conflicting dispatch replay.');
		}
		if (admission.submission.canonicalReadyAt === null) {
			await this.materializeSubmissionConversation(createDispatchAgentSubmissionInput(input), agent);
			if (!(await this.submissions.markSubmissionCanonicalReady(input.dispatchId))) {
				throw new Error('[flue] Dispatch disappeared before canonical readiness.');
			}
		}
		await this.armSubmissionWake();
		this.startSubmissionReconciliation();
		return { dispatchId: admission.submission.submissionId, acceptedAt: input.acceptedAt };
	}

	private async abortInstance(): Promise<boolean> {
		const sessionKey = createSessionStorageKey(
			this.instanceId,
			SUBMISSION_HARNESS_NAME,
			SUBMISSION_SESSION_NAME,
		);
		const affected = await this.submissions.requestSessionAbort(sessionKey);
		for (const id of affected) this.activeControllers.get(id)?.abort(new SubmissionAbortedError());
		if (affected.length > 0) {
			await this.armSubmissionWake();
			this.startSubmissionReconciliation();
		}
		return affected.length > 0;
	}

	private attemptKey(submission: AgentSubmission): string {
		return `${this.actor.actorId}:${submission.attemptId}`;
	}

	private logFailure(submission: AgentSubmission, operation: string, error: unknown): void {
		console.error('[flue:submission]', {
			agentName: this.agentName,
			instanceId: this.instanceId,
			submissionId: submission.submissionId,
			operation,
		}, error);
	}
}

function isAbortRequest(request: Request, agentName: string, instanceId: string): boolean {
	if (request.method !== 'POST') return false;
	const segments = new URL(request.url).pathname.split('/');
	return segments.at(-1) === 'abort' &&
		decodeURIComponent(segments.at(-2) ?? '') === instanceId &&
		decodeURIComponent(segments.at(-3) ?? '') === agentName;
}
