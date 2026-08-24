import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

describe('@rivet-dev/flue entrypoints', () => {
	it('resolves the target, runtime, and internal entrypoints', async () => {
		const root = await import('@rivet-dev/flue');
		const runtime = await import('@rivet-dev/flue/runtime');
		const internal = await import('@rivet-dev/flue/internal');

		assert.equal(root.default.name, 'rivet');
		assert.equal(typeof root.rivet, 'function');
		assert.equal(typeof root.createRivetAgentRuntime, 'function');
		assert.equal(typeof runtime.createRivetAgentRuntime, 'function');
		assert.equal('flueRegistry' in runtime, false);
		assert.equal(typeof internal.actor, 'function');
	});
});
