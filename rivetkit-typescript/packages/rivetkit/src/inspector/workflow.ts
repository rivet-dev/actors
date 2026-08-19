import type { JsonCompatValue } from "@/common/encoding";
import { encodeCborCompat } from "@/serde";
import { bufferToArrayBuffer } from "@/utils";

export type {
	WorkflowBranchStatus,
	WorkflowCbor,
	WorkflowEntry,
	WorkflowEntryKind,
	WorkflowEntryMetadata,
	WorkflowHistory,
	WorkflowJoinEntry,
	WorkflowLocation,
	WorkflowLoopEntry,
	WorkflowLoopIterationMarker,
	WorkflowMessageEntry,
	WorkflowNameIndex,
	WorkflowPathSegment,
	WorkflowRaceEntry,
	WorkflowRemovedEntry,
	WorkflowRollbackCheckpointEntry,
	WorkflowSleepEntry,
	WorkflowStepEntry,
	WorkflowVersionCheckEntry,
} from "@/common/bare/transport/v1";
export {
	WorkflowBranchStatusType,
	WorkflowEntryStatus,
	WorkflowSleepState,
} from "@/common/bare/transport/v1";
export {
	decodeWorkflowHistoryTransport,
	encodeWorkflowHistoryTransport,
} from "@/common/inspector-transport";

/** State exposed by a durable workflow run handler to the Inspector. */
export type WorkflowState =
	| "pending"
	| "running"
	| "sleeping"
	| "failed"
	| "completed"
	| "cancelled"
	| "rolling_back";

/** The raw workflow Inspector adapter consumed by RivetKit's transport. */
export interface WorkflowInspectorAdapter {
	getHistory: () => ArrayBuffer | null;
	getState: () => Promise<WorkflowState | null>;
	onHistoryUpdated: (listener: (history: ArrayBuffer) => void) => () => void;
	replayFromStep: (entryId?: string) => Promise<ArrayBuffer | null>;
}

/** Encodes a workflow Inspector value with RivetKit's CBOR-compatible codec. */
export function encodeWorkflowInspectorValue(value: unknown): ArrayBuffer {
	return bufferToArrayBuffer(encodeCborCompat(value as JsonCompatValue));
}
