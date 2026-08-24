import { randomUUID } from "node:crypto";
import { setupTest } from "rivetkit/test";
import { afterEach, describe, expect, test } from "vitest";
import { SPEC_VERSION_CURRENT } from "@workflow/world";
import { registry } from "../../src/actors.ts";
import { RivetClientWorld } from "../../src/index.ts";

const uid = () => randomUUID().slice(0, 8);

async function createRun(world: RivetClientWorld, name: string) {
	const created = await world.events.create(null, {
		eventType: "run_created",
		specVersion: SPEC_VERSION_CURRENT,
		eventData: { deploymentId: "rivet", workflowName: name, input: {} },
	});
	const runId = created.run?.runId;
	if (!runId) throw new Error("expected run id");
	return runId;
}

describe("hook token reserve/append orphan (P0 #3)", () => {
	let world: RivetClientWorld | undefined;
	afterEach(async () => {
		if (world) await world.close();
		world = undefined;
	});

	test("releases a reserved token when the hook_created append fails", async (c) => {
		const { client } = await setupTest(c, registry);
		world = new RivetClientWorld({ client, runtimeUrl: "http://127.0.0.1:1" });
		const token = `token-${uid()}`;
		const runA = await createRun(world, "orphanProbeA");
		const runB = await createRun(world, "orphanProbeB");

		// Drive runA terminal first: a hook_created against a terminal run reserves
		// the token (World step 1) but then throws EntityConflictError inside the
		// append (workflow-run step 2) — a real failure path that strands the
		// reservation unless events.create releases it on failure.
		await world.events.create(runA, {
			eventType: "run_completed",
			specVersion: SPEC_VERSION_CURRENT,
			eventData: { output: { ok: true } },
		});
		await expect(
			world.events.create(runA, {
				eventType: "hook_created",
				specVersion: SPEC_VERSION_CURRENT,
				correlationId: `hook-${uid()}`,
				eventData: { token, metadata: {} },
			}),
		).rejects.toThrow();

		// The reservation must have been rolled back, so a different run can now
		// claim the same token without hitting a (stale) conflict.
		const second = await world.events.create(runB, {
			eventType: "hook_created",
			specVersion: SPEC_VERSION_CURRENT,
			correlationId: `hook-${uid()}`,
			eventData: { token, metadata: {} },
		});
		expect(second.event?.eventType).toBe("hook_created");
		expect(second.hook?.runId).toBe(runB);
		expect(second.hook?.token).toBe(token);
	});

	test("a genuine concurrent token claim still surfaces as hook_conflict", async (c) => {
		const { client } = await setupTest(c, registry);
		world = new RivetClientWorld({ client, runtimeUrl: "http://127.0.0.1:1" });
		const token = `token-${uid()}`;
		const runA = await createRun(world, "conflictProbeA");
		const runB = await createRun(world, "conflictProbeB");

		const first = await world.events.create(runA, {
			eventType: "hook_created",
			specVersion: SPEC_VERSION_CURRENT,
			correlationId: `hook-${uid()}`,
			eventData: { token, metadata: {} },
		});
		expect(first.hook?.runId).toBe(runA);

		const conflict = await world.events.create(runB, {
			eventType: "hook_created",
			specVersion: SPEC_VERSION_CURRENT,
			correlationId: `hook-${uid()}`,
			eventData: { token, metadata: {} },
		});
		expect(conflict.event?.eventType).toBe("hook_conflict");
		expect(conflict.event?.eventData?.conflictingRunId).toBe(runA);
	});

	test("a duplicate hook id cannot confirm a different token", async (c) => {
		const { client } = await setupTest(c, registry);
		world = new RivetClientWorld({ client, runtimeUrl: "http://127.0.0.1:1" });
		const runA = await createRun(world, "duplicateHookA");
		const runB = await createRun(world, "duplicateHookB");
		const hookId = `hook-${uid()}`;
		const firstToken = `token-${uid()}`;
		const secondToken = `token-${uid()}`;

		await world.events.create(runA, {
			eventType: "hook_created",
			specVersion: SPEC_VERSION_CURRENT,
			correlationId: hookId,
			eventData: { token: firstToken },
		});
		await expect(
			world.events.create(runA, {
				eventType: "hook_created",
				specVersion: SPEC_VERSION_CURRENT,
				correlationId: hookId,
				eventData: { token: secondToken },
			}),
		).rejects.toThrow("different token");

		const created = await world.events.create(runB, {
			eventType: "hook_created",
			specVersion: SPEC_VERSION_CURRENT,
			correlationId: `hook-${uid()}`,
			eventData: { token: secondToken },
		});
		expect(created.hook?.token).toBe(secondToken);
	});
});
