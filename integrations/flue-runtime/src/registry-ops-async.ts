import type { FlueForwardRunIndex, RunStore } from '@flue/runtime/adapter-kit';
import { createAsyncRunStore, ensureAsyncSqlSchema, type AsyncSqlDb } from './stores/index.js';

export interface RivetRegistryOps extends FlueForwardRunIndex {
	readonly runStore: RunStore;
}

export async function createAsyncRegistryOps(db: AsyncSqlDb): Promise<RivetRegistryOps> {
	await ensureAsyncSqlSchema(db);
	const runStore = createAsyncRunStore(db);
	return {
		runStore,
		lookupRun: (runId) => runStore.lookupRun(runId),
		getRun: (runId) => runStore.getRun(runId),
		listRuns: (opts) => runStore.listRuns(opts),
	};
}
