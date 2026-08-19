declare const workflowStorageV1Brand: unique symbol;

/**
 * Closed capability for the workflow engine's version-1 persisted namespace.
 *
 * The runtime value is stable across duplicate RivetKit installations while
 * the brand prevents callers from selecting arbitrary internal namespaces.
 */
export type WorkflowStorageV1 = "rivetkit.workflow-storage.v1" & {
	readonly [workflowStorageV1Brand]: true;
};

export const WORKFLOW_STORAGE_V1 =
	"rivetkit.workflow-storage.v1" as WorkflowStorageV1;

export interface WorkflowStorageEntry {
	key: Uint8Array;
	value: Uint8Array;
}

export interface WorkflowStorageWrite {
	key: Uint8Array;
	value: Uint8Array;
}

/** Opaque byte storage owned and migrated by RivetKit core. */
export interface WorkflowStorageHandle {
	get(key: Uint8Array): Promise<Uint8Array | null>;
	set(key: Uint8Array, value: Uint8Array): Promise<void>;
	delete(key: Uint8Array): Promise<void>;
	deletePrefix(prefix: Uint8Array): Promise<void>;
	deleteRange(start: Uint8Array, end: Uint8Array): Promise<void>;
	list(prefix: Uint8Array): Promise<WorkflowStorageEntry[]>;
	/** Commits one storage-only atomic batch, rejecting oversized batches. */
	batch(writes: WorkflowStorageWrite[]): Promise<void>;
	/** Commits one batch atomically with lifecycle-owned actor state. */
	flushWithState(writes: WorkflowStorageWrite[]): Promise<void>;
}

export interface ActorStorage {
	open(token: WorkflowStorageV1): WorkflowStorageHandle;
}
