import { describe, expect, test, vi } from "vitest";
import { z } from "zod/v4";
import { queue } from "@/actor/schema";
import { ActorContextHandleAdapter } from "@/registry/native";
import type { ActorContextHandle, CoreRuntime } from "@/registry/runtime";
import { decodeCborCompat } from "@/serde";
import { WORKFLOW_STORAGE_V1, type WorkflowStorageV1 } from "@/storage";

function contextWithRuntime(overrides: Partial<CoreRuntime>) {
	return new ActorContextHandleAdapter(
		{
			actorId: () => "workflow-host-test",
			...overrides,
		} as CoreRuntime,
		{} as ActorContextHandle,
	);
}

describe("public workflow host capabilities", () => {
	test("opens only the stable closed storage capability", async () => {
		const get = vi.fn(async () => Uint8Array.of(3));
		const c = contextWithRuntime({ actorWorkflowStorageGet: get });
		const duplicatePackageToken =
			"rivetkit.workflow-storage.v1" as WorkflowStorageV1;

		const storage = c.storage.open(duplicatePackageToken);
		expect(await storage.get(Uint8Array.of(1))).toEqual(Uint8Array.of(3));
		expect(get).toHaveBeenCalledWith(expect.anything(), Uint8Array.of(1));
		expect(() =>
			c.storage.open("rivetkit.internal.other" as WorkflowStorageV1),
		).toThrow("Unsupported RivetKit storage capability");
		expect(WORKFLOW_STORAGE_V1).toBe(duplicatePackageToken);
	});

	test("routes storage-only and lifecycle-owned atomic batches separately", async () => {
		const batch = vi.fn(async () => {});
		const flushWithState = vi.fn(async () => {});
		const c = contextWithRuntime({
			actorWorkflowStorageBatch: batch,
			actorSaveStateAndWorkflowBatch: flushWithState,
		});
		const writes = [{ key: Uint8Array.of(1), value: Uint8Array.of(2) }];
		const storage = c.storage.open(WORKFLOW_STORAGE_V1);

		await storage.batch(writes);
		await storage.flushWithState(writes);

		expect(batch).toHaveBeenCalledWith(expect.anything(), writes);
		expect(flushWithState).toHaveBeenCalledWith(expect.anything(), writes);
	});

	test("sets and clears a logical run wake independently", async () => {
		const setRunWakeAt = vi.fn(async () => {});
		const c = contextWithRuntime({ actorSetRunWakeAt: setRunWakeAt });

		await c.run.setWakeAt(1_700_000_000_000);
		await c.run.setWakeAt(null);

		expect(setRunWakeAt.mock.calls.map((call) => call[1])).toEqual([
			1_700_000_000_000,
			null,
		]);
		await expect(c.run.setWakeAt(Number.NaN)).rejects.toThrow(
			"non-negative safe integer",
		);
	});

	test("waits without consuming and completes by durable identity", async () => {
		const waitForAvailable = vi.fn(async () => {});
		const verify = vi.fn(async () => "ready");
		const complete = vi.fn(async () => true);
		const c = contextWithRuntime({
			actorQueueWaitForNamesAvailable: waitForAvailable,
			actorQueueVerifyPersistedIdentity: verify,
			actorQueueCompletePersisted: complete,
		});

		await c.queue.waitForAvailable(["ready"]);
		await c.queue.complete({ id: 42n, name: "ready" }, { ok: true });

		expect(waitForAvailable).toHaveBeenCalledWith(
			expect.anything(),
			["ready"],
			{ timeoutMs: undefined },
			undefined,
		);
		expect(verify).toHaveBeenCalledWith(expect.anything(), 42n, "ready");
		expect(complete).toHaveBeenCalledWith(
			expect.anything(),
			42n,
			"ready",
			expect.any(Uint8Array),
		);
		expect(
			decodeCborCompat(complete.mock.calls[0]?.[3] as Uint8Array),
		).toEqual({ ok: true });
	});

	test("does not validate or complete an already-missing durable message", async () => {
		const verify = vi.fn(async () => null);
		const complete = vi.fn(async () => true);
		const c = contextWithRuntime({
			actorQueueVerifyPersistedIdentity: verify,
			actorQueueCompletePersisted: complete,
		});

		await c.queue.complete({ id: 99n, name: "gone" }, { ignored: true });

		expect(verify).toHaveBeenCalledOnce();
		expect(complete).not.toHaveBeenCalled();
	});

	test("verifies the durable name before response validation and preserves invalid responses", async () => {
		const complete = vi.fn(async () => true);
		const identityMismatch = vi.fn(async () => {
			throw new Error("persisted identity mismatch");
		});
		const mismatched = new ActorContextHandleAdapter(
			{
				actorId: () => "workflow-host-test",
				actorQueueVerifyPersistedIdentity: identityMismatch,
				actorQueueCompletePersisted: complete,
			} as unknown as CoreRuntime,
			{} as ActorContextHandle,
			undefined,
			{
				queues: {
					expected: queue({
						message: z.unknown(),
						complete: z.object({ ok: z.literal(true) }),
					}),
				},
			},
		);
		await expect(
			mismatched.queue.complete(
				{ id: 7n, name: "expected" },
				{ ok: false },
			),
		).rejects.toThrow("persisted identity mismatch");
		expect(complete).not.toHaveBeenCalled();

		const verified = vi.fn(async () => "expected");
		const invalidResponse = new ActorContextHandleAdapter(
			{
				actorId: () => "workflow-host-test",
				actorQueueVerifyPersistedIdentity: verified,
				actorQueueCompletePersisted: complete,
			} as unknown as CoreRuntime,
			{} as ActorContextHandle,
			undefined,
			{
				queues: {
					expected: queue({
						message: z.unknown(),
						complete: z.object({ ok: z.literal(true) }),
					}),
				},
			},
		);
		await expect(
			invalidResponse.queue.complete(
				{ id: 8n, name: "expected" },
				{ ok: false },
			),
		).rejects.toMatchObject({
			group: "actor",
			code: "validation_error",
		});
		expect(verified).toHaveBeenCalledOnce();
		expect(complete).not.toHaveBeenCalled();
	});
});
