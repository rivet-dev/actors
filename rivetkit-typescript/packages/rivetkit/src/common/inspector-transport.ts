import type { WorkflowHistory } from "@/common/bare/transport/v1";
import {
	decodeWorkflowHistory,
	encodeWorkflowHistory,
} from "@/common/bare/transport/v1";
import { bufferToArrayBuffer, toUint8Array } from "@/utils";

declare const workflowHistoryBare: unique symbol;

// Branded so these bytes cannot be handed to the CBOR compat encoder, which
// would rewrite the buffer as `["$ArrayBuffer", "<base64>"]`.
export type WorkflowHistoryBytes = ArrayBuffer & {
	readonly [workflowHistoryBare]: true;
};

export function encodeWorkflowHistoryTransport(
	history: WorkflowHistory,
): WorkflowHistoryBytes {
	return bufferToArrayBuffer(
		encodeWorkflowHistory(history),
	) as WorkflowHistoryBytes;
}

export function decodeWorkflowHistoryTransport(
	data: ArrayBuffer | ArrayBufferView,
): WorkflowHistory {
	return decodeWorkflowHistory(toUint8Array(data));
}
