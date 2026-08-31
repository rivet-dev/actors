import { describe, expect, expectTypeOf, test } from "vitest";
import type { WorkflowHistory } from "@/common/bare/transport/v1";
import {
	decodeWorkflowHistoryTransport,
	encodeWorkflowHistoryTransport,
} from "@/common/inspector-transport";
import {
	decodeWorkflowHistoryTransport as decodePublicWorkflowHistory,
	encodeWorkflowHistoryTransport as encodePublicWorkflowHistory,
	encodeWorkflowInspectorValue,
	WorkflowEntryStatus,
	type WorkflowHistoryBytes,
	type WorkflowInspectorAdapter,
	WorkflowSleepState,
	type WorkflowState,
} from "@/inspector/workflow";
import { encodeCborCompat } from "@/serde";

function bytes(value: ArrayBuffer): Uint8Array {
	return new Uint8Array(value);
}

describe("rivetkit/experimental/inspector/workflow", () => {
	test("preserves the existing raw BARE history bytes", () => {
		const history: WorkflowHistory = {
			nameRegistry: ["root", "delay"],
			entries: [
				{
					id: "sleep-1",
					location: [
						{ tag: "WorkflowNameIndex", val: 0 },
						{
							tag: "WorkflowLoopIterationMarker",
							val: { loop: 1, iteration: 2 },
						},
					],
					kind: {
						tag: "WorkflowSleepEntry",
						val: {
							deadline: 1_234n,
							state: WorkflowSleepState.PENDING,
						},
					},
				},
			],
			entryMetadata: new Map([
				[
					"sleep-1",
					{
						status: WorkflowEntryStatus.RUNNING,
						error: null,
						attempts: 2,
						lastAttemptAt: 1_200n,
						createdAt: 1_000n,
						completedAt: null,
						rollbackCompletedAt: null,
						rollbackError: null,
					},
				],
			]),
		};

		const publicBytes = encodePublicWorkflowHistory(history);
		const existingBytes = encodeWorkflowHistoryTransport(history);
		expect(bytes(publicBytes)).toEqual(bytes(existingBytes));
		expect(decodePublicWorkflowHistory(publicBytes)).toEqual(history);
		expect(decodeWorkflowHistoryTransport(publicBytes)).toEqual(history);
	});

	test("preserves the existing CBOR-compatible value bytes", () => {
		const value = { count: 3n, nested: [null, "ok"] };
		expect(bytes(encodeWorkflowInspectorValue(value))).toEqual(
			new Uint8Array(encodeCborCompat(value)),
		);
	});

	test("requires state on the public workflow adapter contract", async () => {
		const state: WorkflowState = "sleeping";
		const adapter: WorkflowInspectorAdapter = {
			getHistory: () => null,
			getState: async () => state,
			onHistoryUpdated: () => () => {},
			replayFromStep: async () => null,
		};

		await expect(adapter.getState()).resolves.toBe("sleeping");
		expectTypeOf(
			adapter.getHistory,
		).returns.toEqualTypeOf<WorkflowHistoryBytes | null>();
	});
});
