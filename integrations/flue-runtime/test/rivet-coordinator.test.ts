import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { afterEach, describe, it } from 'node:test';
import { defineAgent } from '@flue/runtime';
import {
	createFlueContext,
	fauxAssistantMessage,
	registerFauxProvider,
} from '@flue/runtime/adapter-kit';
import { createRivetAgentRuntime } from '../src/index.ts';

const providers = [];
afterEach(() => {
	for (const provider of providers.splice(0)) provider.unregister();
});

describe('RivetAgentCoordinator', () => {
	it('returns an admission receipt while the agent turn remains alive in the background', async () => {
		const provider = createProvider({ tokensPerSecond: 20 });
		provider.setResponses([fauxAssistantMessage('A deliberately delayed Rivet reply.')]);
		const host = await createHost(provider);

		const response = await host.runtime.onRequest(
			host.actor,
			new Request('http://flue.local/agents/assistant/instance-1', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ kind: 'user', body: 'Hello' }),
			}),
		);

		assert.equal(response.status, 202);
		assert.ok(host.actor.keepAwakePromises.size > 0);
		await host.actor.waitForKeepAwake();
		assert.equal(await host.prepared.executionStore.submissions.hasUnsettledSubmissions(), false);
	});

	it('durably admits a prompt and serves canonical conversation history', async () => {
		const provider = createProvider();
		provider.setResponses([fauxAssistantMessage('Rivet reply.')]);
		const host = await createHost(provider);

		const response = await host.runtime.onRequest(
			host.actor,
			new Request('http://flue.local/agents/assistant/instance-1', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ kind: 'user', body: 'Hello' }),
			}),
		);
		assert.equal(response.status, 202);
		const receipt = await response.json();
		assert.equal(typeof receipt.submissionId, 'string');
		await host.actor.waitForKeepAwake();
		assert.equal(await host.prepared.executionStore.submissions.hasUnsettledSubmissions(), false);

		const history = await host.runtime.onRequest(
			host.actor,
			new Request('http://flue.local/agents/assistant/instance-1?view=history'),
		);
		assert.equal(history.status, 200);
		const snapshot = await history.json();
		assert.ok(snapshot.messages.some((message) =>
			message.role === 'assistant' &&
			message.parts.some((part) => part.type === 'text' && part.text === 'Rivet reply.'),
		));
	});

	it('resumes a canonically unready queued submission from onWake', async () => {
		const provider = createProvider();
		provider.setResponses([fauxAssistantMessage('Recovered after wake.')]);
		const host = await createHost(provider);
		const input = {
			kind: 'direct',
			submissionId: 'direct-recover',
			agent: 'assistant',
			id: 'instance-1',
			message: { kind: 'user', body: 'Hello after restart' },
			acceptedAt: new Date().toISOString(),
		};
		await host.prepared.executionStore.submissions.admitDirect(input);

		const restarted = await createHost(provider, host.db);
		await restarted.runtime.onWake(restarted.actor);
		await restarted.actor.waitForKeepAwake();
		const submission = await restarted.prepared.executionStore.submissions.getSubmission(input.submissionId);
		assert.equal(submission.status, 'settled');
		assert.equal(submission.error, undefined);
	});

	it('admits dispatches through the typed actor action boundary', async () => {
		const provider = createProvider();
		provider.setResponses([fauxAssistantMessage('Dispatched reply.')]);
		const host = await createHost(provider);
		const input = dispatchInput('dispatch-action', 'Hello');

		const receipt = await host.runtime.admitDispatch(host.actor, input);

		assert.deepEqual(receipt, {
			dispatchId: input.dispatchId,
			acceptedAt: input.acceptedAt,
		});
		await host.actor.waitForKeepAwake();
		assert.equal(await host.prepared.executionStore.submissions.hasUnsettledSubmissions(), false);
	});

	it('rejects conflicting dispatch replays', async () => {
		const host = await createHost(createProvider());
		const original = dispatchInput('dispatch-replay', 'Hello');
		await host.prepared.executionStore.submissions.admitDispatch(original);
		await assert.rejects(host.runtime.admitDispatch(
			host.actor,
			{ ...original, message: { kind: 'user', body: 'Different' } },
		), /Conflicting dispatch replay/);
	});
});

async function createHost(provider, db = new TestSqliteDb()) {
	const actor = new FakeActor(db);
	const agent = defineAgent(() => ({ model: `${provider.getModel().provider}/${provider.getModel().id}` }));
	const runtime = createRivetAgentRuntime({
		agents: [{ name: 'assistant', definition: agent }],
		createContext: ({ executionStore, actor: actorContext, agentName, request, initialEventIndex, dispatchId }) =>
			createFlueContext({
				id: actorContext.key[0],
				agentName,
				dispatchId,
				env: actorContext.env ?? {},
				req: request,
				initialEventIndex,
				agentConfig: { resolveModel: () => provider.getModel() },
				createDefaultEnv: async () => createNoopSessionEnv(),
				submissionStore: executionStore.submissions,
			}),
	});
	const prepared = await runtime.prepare({ db, agentName: 'assistant' });
	runtime.attach(actor, prepared);
	return { db, actor, runtime, prepared };
}

function createProvider(options = {}) {
	const provider = registerFauxProvider({
		provider: `rivet-test-${crypto.randomUUID()}`,
		...options,
	});
	providers.push(provider);
	return provider;
}

class FakeActor {
	actorId = crypto.randomUUID();
	name = 'assistant';
	key = ['instance-1'];
	region = 'local';
	env = {};
	abortController = new AbortController();
	keepAwakePromises = new Set();
	scheduled = [];
	constructor(db) { this.db = db; }
	get abortSignal() { return this.abortController.signal; }
	async keepAwake(promise) {
		this.keepAwakePromises.add(promise);
		try { return await promise; } finally { this.keepAwakePromises.delete(promise); }
	}
	schedule = { after: async (duration, action, ...args) => {
		this.scheduled.push({ duration, action, args });
	} };
	async waitForKeepAwake() {
		while (this.keepAwakePromises.size > 0) await Promise.allSettled([...this.keepAwakePromises]);
	}
}

class TestSqliteDb {
	db = new DatabaseSync(':memory:');
	tail = Promise.resolve();
	async query(text, params = []) { await this.tail; return this.queryDirect(text, params); }
	async transaction(fn) {
		const previous = this.tail;
		let release;
		this.tail = new Promise((resolve) => { release = resolve; });
		await previous;
		this.db.exec('BEGIN IMMEDIATE');
		try {
			const result = await fn({ query: (text, params = []) => this.queryDirect(text, params) });
			this.db.exec('COMMIT');
			return result;
		} catch (error) {
			this.db.exec('ROLLBACK');
			throw error;
		} finally { release(); }
	}
	async queryDirect(text, params = []) {
		return this.db.prepare(text).all(...params.map((value) => typeof value === 'boolean' ? Number(value) : value))
			.map((row) => ({ ...row }));
	}
}

function createNoopSessionEnv() {
	return {
		cwd: '/repo',
		resolvePath: (value) => value,
		exec: async () => ({ stdout: '', stderr: '', exitCode: 0 }),
		readFile: async () => '',
		readFileBuffer: async () => new Uint8Array(),
		writeFile: async () => {},
		stat: async () => ({ isFile: false, isDirectory: false }),
		readdir: async () => [],
		exists: async () => false,
		mkdir: async () => {},
		rm: async () => {},
	};
}

function dispatchInput(dispatchId, body) {
	return {
		dispatchId,
		agent: 'assistant',
		id: 'instance-1',
		message: { kind: 'user', body },
		acceptedAt: new Date().toISOString(),
	};
}
