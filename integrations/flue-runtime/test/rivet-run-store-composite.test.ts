import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createRivetRunStore } from '../src/index.ts';

describe('createRivetRunStore()', () => {
	it('writes to both the primary and a healthy mirror', async () => {
		const primary = new InMemoryRunStore();
		const mirror = new InMemoryRunStore();
		const store = createRivetRunStore(primary, mirror);

		await store.createRun(createInput('run-1'));
		await store.endRun(endInput('run-1'));

		assert.equal((await primary.getRun('run-1'))?.status, 'completed');
		assert.equal((await mirror.getRun('run-1'))?.status, 'completed');
	});

	it('works as a primary-only store when no mirror is configured', async () => {
		const primary = new InMemoryRunStore();
		const store = createRivetRunStore(primary);

		await store.createRun(createInput('run-1'));
		await store.endRun(endInput('run-1'));

		assert.equal((await primary.getRun('run-1'))?.status, 'completed');
	});

	// The registry index is a best-effort mirror (matches Cloudflare's `safeIndexWrite`):
	// a mirror failure is swallowed + logged, never propagated, so a registry fault never
	// fails run admission or finalization. The primary commits and the run proceeds; the
	// mirror is left stale (it never receives the run).
	it('keeps the primary write and self-heals when a mirror write throws', async () => {
		const primary = new InMemoryRunStore();
		const mirror = new ThrowingRunStore(new Error('mirror unavailable'));
		const store = createRivetRunStore(primary, mirror);

		// Neither call rejects even though the mirror throws on every write.
		await store.createRun(createInput('run-1'));
		await store.endRun(endInput('run-1'));

		// Primary committed both writes; the mirror was attempted but diverged.
		assert.equal((await primary.getRun('run-1'))?.status, 'completed');
		assert.equal(mirror.createRunCalls, 1);
		assert.equal(mirror.endRunCalls, 1);
	});

	it('serves reads from the primary without touching the mirror', async () => {
		const primary = new InMemoryRunStore();
		await primary.createRun(createInput('run-1'));
		// A mirror that throws on every operation proves reads never consult it.
		const mirror = new ThrowingRunStore(new Error('mirror must not be read'));
		const store = createRivetRunStore(primary, mirror);

		assert.equal((await store.getRun('run-1'))?.runId, 'run-1');
		assert.equal((await store.lookupRun('run-1'))?.runId, 'run-1');
		assert.deepEqual(
			(await store.listRuns()).runs.map((run) => run.runId),
			['run-1'],
		);
	});
});

function createInput(runId) {
	return {
		runId,
		workflowName: 'dispatch',
		startedAt: '2026-01-01T00:00:00.000Z',
		payload: { source: 'test' },
	};
}

function endInput(runId) {
	return {
		runId,
		endedAt: '2026-01-01T00:00:01.000Z',
		isError: false,
		durationMs: 1000,
		result: { ok: true },
	};
}

function toPointer(record) {
	return {
		runId: record.runId,
		workflowName: record.workflowName,
		status: record.status,
		startedAt: record.startedAt,
		endedAt: record.endedAt,
		durationMs: record.durationMs,
		isError: record.isError,
	};
}

class InMemoryRunStore {
	runs = new Map();

	async createRun(input) {
		if (!this.runs.has(input.runId)) {
			this.runs.set(input.runId, { ...input, status: 'active' });
		}
	}

	async endRun(input) {
		const record = this.runs.get(input.runId);
		if (!record) return;
		record.status = input.isError ? 'errored' : 'completed';
		record.endedAt = input.endedAt;
		record.isError = input.isError;
		record.durationMs = input.durationMs;
		record.result = input.result;
		record.error = input.error;
	}

	async getRun(runId) {
		return this.runs.get(runId) ?? null;
	}

	async lookupRun(runId) {
		const record = this.runs.get(runId);
		return record ? toPointer(record) : null;
	}

	async listRuns() {
		return { runs: [...this.runs.values()].map(toPointer) };
	}
}

class ThrowingRunStore {
	createRunCalls = 0;
	endRunCalls = 0;

	constructor(error) {
		this.error = error;
	}

	async createRun() {
		this.createRunCalls++;
		throw this.error;
	}

	async endRun() {
		this.endRunCalls++;
		throw this.error;
	}

	async getRun() {
		throw this.error;
	}

	async lookupRun() {
		throw this.error;
	}

	async listRuns() {
		throw this.error;
	}
}
