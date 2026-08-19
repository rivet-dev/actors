import type {
	BranchStatus,
	BranchStatusType,
	EntryKind,
	EntryStatus,
	Location,
	SleepState,
	WorkflowEntryMetadataSnapshot,
	WorkflowHistoryEntry,
	WorkflowHistorySnapshot,
	WorkflowState,
} from "@rivetkit/workflow-engine";
import * as transport from "@/common/bare/transport/v1";
import type { JsonCompatValue } from "@/common/encoding";
import type { WorkflowHistoryBytes } from "@/common/inspector-transport";
import { encodeWorkflowHistoryTransport } from "@/common/inspector-transport";
import { encodeCborCompat } from "@/serde";
import { assertUnreachable, bufferToArrayBuffer } from "@/utils";

type HistoryListener = (history: WorkflowHistoryBytes) => void;

function createHistoryEmitter() {
	const listeners = new Set<HistoryListener>();

	return {
		on: (listener: HistoryListener) => {
			listeners.add(listener);
			return () => listeners.delete(listener);
		},
		emit: (history: WorkflowHistoryBytes) => {
			for (const listener of listeners) {
				listener(history);
			}
		},
	};
}

export interface WorkflowInspectorAdapter {
	getHistory: () => WorkflowHistoryBytes | null;
	getState: () => Promise<WorkflowState | null>;
	onHistoryUpdated: (
		listener: (history: WorkflowHistoryBytes) => void,
	) => () => void;
	replayFromStep: (entryId?: string) => Promise<WorkflowHistoryBytes | null>;
}

export function createWorkflowInspectorAdapter(): {
	adapter: WorkflowInspectorAdapter;
	update: (snapshot: WorkflowHistorySnapshot) => void;
	setGetState: (fn: () => Promise<WorkflowState | null>) => void;
	setReplayFromStep: (
		fn: (entryId?: string) => Promise<WorkflowHistoryBytes | null>,
	) => void;
} {
	const emitter = createHistoryEmitter();
	let history: WorkflowHistoryBytes | null = null;
	let getState: () => Promise<WorkflowState | null> = async () => null;
	let replayFromStep: (
		entryId?: string,
	) => Promise<WorkflowHistoryBytes | null> = async () => {
		throw new Error("Workflow replay controls are not initialized");
	};

	const adapter: WorkflowInspectorAdapter = {
		getHistory: () => history,
		getState: async () => await getState(),
		onHistoryUpdated: (listener) => emitter.on(listener),
		replayFromStep: async (entryId) => await replayFromStep(entryId),
	};

	const update = (snapshot: WorkflowHistorySnapshot) => {
		const transportHistory = toWorkflowHistory(snapshot);
		const next = encodeWorkflowHistoryTransport(transportHistory);
		history = next;
		emitter.emit(next);
	};

	return {
		adapter,
		update,
		setGetState: (fn) => {
			getState = fn;
		},
		setReplayFromStep: (fn) => {
			replayFromStep = fn;
		},
	};
}

function encodeCbor(value: unknown): ArrayBuffer {
	return bufferToArrayBuffer(encodeCborCompat(value as JsonCompatValue));
}

function encodeOptionalCbor(value: unknown): ArrayBuffer | null {
	if (value === undefined) {
		return null;
	}
	return encodeCbor(value);
}

function toU64(value: number): bigint {
	return BigInt(Math.max(0, Math.floor(value)));
}

function toWorkflowLocation(location: Location): transport.WorkflowLocation {
	return location.map((segment) => {
		if (typeof segment === "number") {
			return { tag: "WorkflowNameIndex", val: segment };
		}
		return {
			tag: "WorkflowLoopIterationMarker",
			val: {
				loop: segment.loop,
				iteration: segment.iteration,
			},
		};
	});
}

function toWorkflowEntryKind(kind: EntryKind): transport.WorkflowEntryKind {
	switch (kind.type) {
		case "step":
			return {
				tag: "WorkflowStepEntry",
				val: {
					output: encodeOptionalCbor(kind.data.output),
					error: kind.data.error ?? null,
				},
			};
		case "loop":
			return {
				tag: "WorkflowLoopEntry",
				val: {
					state: encodeCbor(kind.data.state),
					iteration: kind.data.iteration,
					output: encodeOptionalCbor(kind.data.output),
				},
			};
		case "sleep":
			return {
				tag: "WorkflowSleepEntry",
				val: {
					deadline: toU64(kind.data.deadline),
					state: toWorkflowSleepState(kind.data.state),
				},
			};
		case "message":
			return {
				tag: "WorkflowMessageEntry",
				val: {
					name: kind.data.name,
					messageData: encodeCbor(kind.data.data),
				},
			};
		case "rollback_checkpoint":
			return {
				tag: "WorkflowRollbackCheckpointEntry",
				val: { name: kind.data.name },
			};
		case "join":
			return {
				tag: "WorkflowJoinEntry",
				val: {
					branches: toWorkflowBranchStatusMap(kind.data.branches),
				},
			};
		case "race":
			return {
				tag: "WorkflowRaceEntry",
				val: {
					winner: kind.data.winner ?? null,
					branches: toWorkflowBranchStatusMap(kind.data.branches),
				},
			};
		case "removed":
			return {
				tag: "WorkflowRemovedEntry",
				val: {
					originalType: kind.data.originalType,
					originalName: kind.data.originalName ?? null,
				},
			};
		case "version_check":
			return {
				tag: "WorkflowVersionCheckEntry",
				val: {
					resolved: kind.data.resolved,
					latest: kind.data.latest,
				},
			};
		default:
			assertUnreachable(kind as never);
	}
}

function toWorkflowEntry(entry: WorkflowHistoryEntry): transport.WorkflowEntry {
	return {
		id: entry.id,
		location: toWorkflowLocation(entry.location),
		kind: toWorkflowEntryKind(entry.kind),
	};
}

function toWorkflowEntryStatus(
	status: EntryStatus,
): transport.WorkflowEntryStatus {
	switch (status) {
		case "pending":
			return transport.WorkflowEntryStatus.PENDING;
		case "running":
			return transport.WorkflowEntryStatus.RUNNING;
		case "completed":
			return transport.WorkflowEntryStatus.COMPLETED;
		case "failed":
			return transport.WorkflowEntryStatus.FAILED;
		case "exhausted":
			return transport.WorkflowEntryStatus.EXHAUSTED;
		default:
			assertUnreachable(status as never);
	}
}

function toWorkflowSleepState(state: SleepState): transport.WorkflowSleepState {
	switch (state) {
		case "pending":
			return transport.WorkflowSleepState.PENDING;
		case "completed":
			return transport.WorkflowSleepState.COMPLETED;
		case "interrupted":
			return transport.WorkflowSleepState.INTERRUPTED;
		default:
			assertUnreachable(state as never);
	}
}

function toWorkflowBranchStatusType(
	status: BranchStatusType,
): transport.WorkflowBranchStatusType {
	switch (status) {
		case "pending":
			return transport.WorkflowBranchStatusType.PENDING;
		case "running":
			return transport.WorkflowBranchStatusType.RUNNING;
		case "completed":
			return transport.WorkflowBranchStatusType.COMPLETED;
		case "failed":
			return transport.WorkflowBranchStatusType.FAILED;
		case "cancelled":
			return transport.WorkflowBranchStatusType.CANCELLED;
		default:
			assertUnreachable(status as never);
	}
}

function toWorkflowBranchStatus(
	status: BranchStatus,
): transport.WorkflowBranchStatus {
	return {
		status: toWorkflowBranchStatusType(status.status),
		output: encodeOptionalCbor(status.output),
		error: status.error ?? null,
	};
}

function toWorkflowBranchStatusMap(
	branches: Record<string, BranchStatus>,
): ReadonlyMap<string, transport.WorkflowBranchStatus> {
	return new Map(
		Object.entries(branches).map(([name, status]) => [
			name,
			toWorkflowBranchStatus(status),
		]),
	);
}

function toWorkflowEntryMetadata(
	metadata: WorkflowEntryMetadataSnapshot,
): transport.WorkflowEntryMetadata {
	const rollbackCompletedAt = (
		metadata as WorkflowEntryMetadataSnapshot & {
			rollbackCompletedAt?: number;
		}
	).rollbackCompletedAt;
	const rollbackError = (
		metadata as WorkflowEntryMetadataSnapshot & {
			rollbackError?: string | null;
		}
	).rollbackError;

	return {
		status: toWorkflowEntryStatus(metadata.status),
		error: metadata.error ?? null,
		attempts: metadata.attempts,
		lastAttemptAt: toU64(metadata.lastAttemptAt),
		createdAt: toU64(metadata.createdAt),
		completedAt:
			metadata.completedAt === undefined
				? null
				: toU64(metadata.completedAt),
		rollbackCompletedAt:
			rollbackCompletedAt === undefined
				? null
				: toU64(rollbackCompletedAt),
		rollbackError: rollbackError ?? null,
	};
}

function toWorkflowHistory(
	snapshot: WorkflowHistorySnapshot,
): transport.WorkflowHistory {
	const entryMetadata = new Map<string, transport.WorkflowEntryMetadata>();
	for (const [id, metadata] of snapshot.entryMetadata) {
		entryMetadata.set(id, toWorkflowEntryMetadata(metadata));
	}

	return {
		nameRegistry: [...snapshot.nameRegistry],
		entries: snapshot.entries.map(toWorkflowEntry),
		entryMetadata,
	};
}
