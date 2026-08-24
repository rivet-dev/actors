import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { describe, it } from 'node:test';
import { createAttachmentRef } from '@flue/runtime/adapter-kit';
import { createAsyncSqlStores, ensureAsyncSqlSchema } from '../src/index.ts';

describe('rivet async stores', () => {
	it('persists the current submission, conversation, attachment, run, and event contracts', async () => {
		const db = new TestSqliteDb();
		await ensureAsyncSqlSchema(db);
		const stores = createAsyncSqlStores(db);
		const submissions = stores.executionStore.submissions;
		const input = {
			kind: 'direct',
			submissionId: 'direct-1',
			agent: 'assistant',
			id: 'instance-1',
			message: { kind: 'user', body: 'hello' },
			acceptedAt: new Date().toISOString(),
		};
		const admitted = await submissions.admitDirect(input);
		assert.equal(admitted.canonicalReadyAt, null);
		assert.deepEqual((await submissions.listUnreadySubmissions()).map((value) => value.submissionId), ['direct-1']);
		await submissions.markSubmissionCanonicalReady('direct-1');
		assert.deepEqual((await submissions.listRunnableSubmissions()).map((value) => value.submissionId), ['direct-1']);
		const claimed = await submissions.claimSubmission({
			submissionId: 'direct-1',
			attemptId: 'attempt-1',
			ownerId: 'owner-1',
			leaseExpiresAt: Date.now() - 1,
		});
		assert.equal(claimed.attemptId, 'attempt-1');
		assert.equal((await submissions.listExpiredSubmissions()).length, 1);
		await submissions.markSubmissionInputApplied({ submissionId: 'direct-1', attemptId: 'attempt-1' });
		const replaced = await submissions.replaceSubmissionAttempt(
			{ submissionId: 'direct-1', attemptId: 'attempt-1' },
			'attempt-2',
			{ ownerId: 'owner-2', leaseExpiresAt: 0 },
		);
		assert.equal(replaced.attemptId, 'attempt-2');
		assert.equal(await submissions.completeSubmission({ submissionId: 'direct-1', attemptId: 'attempt-2' }), true);

		const path = 'agents/assistant/instance-1';
		await stores.conversationStreamStore.createStream(path, { agentName: 'assistant', instanceId: 'instance-1' });
		const claim = await stores.conversationStreamStore.acquireProducer(path, 'producer-1');
		await stores.conversationStreamStore.append({
			path,
			producerId: claim.producerId,
			producerEpoch: claim.producerEpoch,
			incarnation: claim.incarnation,
			producerSequence: claim.nextProducerSequence,
			records: [{ id: 'record-1', type: 'conversation_created', conversationId: 'conversation-1', harness: 'default', session: 'default', createdAt: new Date().toISOString() }],
		});
		assert.equal((await stores.conversationStreamStore.read(path)).batches.length, 1);

		const bytes = new TextEncoder().encode('image');
		const attachment = await createAttachmentRef({ id: 'attachment-1', mimeType: 'image/png', bytes });
		await stores.attachmentStore.put({ streamPath: path, conversationId: 'conversation-1', attachment, bytes });
		assert.deepEqual((await stores.attachmentStore.get({ streamPath: path, conversationId: 'conversation-1', attachmentId: attachment.id })).bytes, bytes);

		await stores.runStore.createRun({
			runId: 'run-1', workflowName: 'workflow', startedAt: '2026-06-17T00:00:00.000Z', input: { hello: true },
		});
		await stores.runStore.endRun({
			runId: 'run-1', endedAt: '2026-06-17T00:00:01.000Z', isError: false, durationMs: 1000, result: { ok: true },
		});
		assert.equal((await stores.runStore.getRun('run-1')).status, 'completed');

		await stores.eventStreamStore.createStream('runs/run-1');
		const offset = await stores.eventStreamStore.appendEventOnce('runs/run-1', 'terminal', { type: 'end' });
		assert.equal(await stores.eventStreamStore.appendEventOnce('runs/run-1', 'terminal', { type: 'end' }), offset);
		assert.equal((await stores.eventStreamStore.readEvents('runs/run-1')).events.length, 1);
	});
});

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
