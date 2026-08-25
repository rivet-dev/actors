import { randomUUID } from "node:crypto";
import { SPEC_VERSION_CURRENT } from "@workflow/world";
import { setup } from "rivetkit";
import { setupTest } from "rivetkit/test";
import { describe, expect, test } from "vitest";
import { hookToken } from "../../src/actors/hook-token.ts";
import { workflowRun } from "../../src/actors/workflow-run.ts";

const registry = setup({ use: { hookToken, workflowRun } });

const uid = () => randomUUID().slice(0, 8);
type Client = Awaited<ReturnType<typeof setupTest>>["client"];

function tokenActor(client: Client, token: string) {
	return client.hookToken.getOrCreate([token]);
}

describe("hookToken ownership generations", () => {
	test("reserve, confirm, and release are idempotent for one owner", async (c) => {
		const { client } = await setupTest(c, registry);
		const token = `token-${uid()}`;
		const owner = { runId: `wrun_${uid()}`, hookId: `hook-${uid()}` };
		const handle = tokenActor(client, token);

		const first = await handle.reserve(token, owner.runId, owner.hookId);
		expect(first.ok).toBe(true);
		const repeated = await handle.reserve(token, owner.runId, owner.hookId);
		expect(repeated.generation).toBe(first.generation);

		expect(
			await handle.confirm(
				token,
				first.generation,
				owner.runId,
				owner.hookId,
			),
		).toEqual({ ok: true });
		expect(
			await handle.confirm(
				token,
				first.generation,
				owner.runId,
				owner.hookId,
			),
		).toEqual({ ok: true });
		expect(await handle.get(token)).toMatchObject({
			...owner,
			generation: first.generation,
		});

		expect(
			await handle.release(
				token,
				first.generation,
				owner.runId,
				owner.hookId,
			),
		).toEqual({ ok: true });
		expect(
			await handle.release(
				token,
				first.generation,
				owner.runId,
				owner.hookId,
			),
		).toEqual({ ok: true });
		expect(await handle.get(token)).toBeNull();
	});

	test("operations from an older generation cannot change a replacement owner", async (c) => {
		const { client } = await setupTest(c, registry);
		const token = `token-${uid()}`;
		const handle = tokenActor(client, token);
		const oldOwner = { runId: `wrun_${uid()}`, hookId: `hook-${uid()}` };
		const newOwner = { runId: `wrun_${uid()}`, hookId: `hook-${uid()}` };

		const oldReservation = await handle.reserve(
			token,
			oldOwner.runId,
			oldOwner.hookId,
		);
		await handle.confirm(
			token,
			oldReservation.generation,
			oldOwner.runId,
			oldOwner.hookId,
		);
		await handle.release(
			token,
			oldReservation.generation,
			oldOwner.runId,
			oldOwner.hookId,
		);

		const replacement = await handle.reserve(
			token,
			newOwner.runId,
			newOwner.hookId,
		);
		expect(replacement.generation).toBeGreaterThan(oldReservation.generation);
		await handle.confirm(
			token,
			replacement.generation,
			newOwner.runId,
			newOwner.hookId,
		);

		expect(
			await handle.confirm(
				token,
				oldReservation.generation,
				oldOwner.runId,
				oldOwner.hookId,
			),
		).toEqual({ ok: false });
		expect(
			await handle.release(
				token,
				oldReservation.generation,
				oldOwner.runId,
				oldOwner.hookId,
			),
		).toEqual({ ok: false });
		expect(await handle.get(token)).toMatchObject(newOwner);
	});

	test("pending lookup verifies canonical workflow ownership before exposure", async (c) => {
		const { client } = await setupTest(c, registry);
		const token = `token-${uid()}`;
		const runId = `wrun_${uid()}`;
		const hookId = `hook-${uid()}`;
		const handle = tokenActor(client, token);

		const reservation = await handle.reserve(token, runId, hookId);
		expect(await handle.get(token)).toBeNull();

		const workflow = client.workflowRun.getOrCreate([runId]);
		await workflow.appendEvent(runId, {
			eventType: "run_created",
			specVersion: SPEC_VERSION_CURRENT,
			eventData: {
				deploymentId: "rivet",
				workflowName: "pending-hook-verification",
				input: {},
			},
		});
		await workflow.appendEvent(runId, {
			eventType: "hook_created",
			specVersion: SPEC_VERSION_CURRENT,
			correlationId: hookId,
			eventData: { token, metadata: {} },
		});

		expect(await handle.get(token)).toMatchObject({
			runId,
			hookId,
			generation: reservation.generation,
		});
	});
});
