import type {
	CreateRunInput,
	EndRunInput,
	ListRunsOpts,
	ListRunsResponse,
	RunPointer,
	RunRecord,
	RunStore,
	WorkflowRunPointer,
} from '@flue/runtime/adapter-kit';
export function createRivetRunStore(primary: RunStore, mirror?: RunStore): RunStore {
	return new RivetRunStore(primary, mirror);
}

class RivetRunStore implements RunStore {
	private readonly primary: RunStore;
	private readonly mirror: RunStore | undefined;

	constructor(primary: RunStore, mirror: RunStore | undefined) {
		this.primary = primary;
		this.mirror = mirror;
	}

	async createRun(input: CreateRunInput): Promise<void> {
		await this.primary.createRun(input);
		await this.mirrorWrite(() => this.mirror?.createRun(input));
	}

	async endRun(input: EndRunInput): Promise<void> {
		await this.primary.endRun(input);
		await this.mirrorWrite(() => this.mirror?.endRun(input));
	}

	getRun(runId: string): Promise<RunRecord | null> {
		return this.primary.getRun(runId);
	}

	lookupRun(runId: string): Promise<WorkflowRunPointer | null> {
		return this.primary.lookupRun(runId);
	}

	listRuns(opts?: ListRunsOpts): Promise<ListRunsResponse> {
		return this.primary.listRuns(opts);
	}

	private async mirrorWrite(write: () => Promise<unknown> | undefined): Promise<void> {
		// The registry index is a best-effort mirror of the authoritative per-actor
		// run record: an index fault must NOT fail run admission or finalization.
			// This mirrors the Cloudflare adapter's `safeIndexWrite` contract
			// (`@flue/runtime` `src/cloudflare/run-store.ts`). RivetKit owns actor
			// readiness; an unavailable registry actor is logged as an index fault
			// rather than hidden behind an adapter-specific retry policy.
		try {
			await write();
		} catch (error) {
			console.error('[flue:run-index] mirror write failed:', error);
		}
	}
}
