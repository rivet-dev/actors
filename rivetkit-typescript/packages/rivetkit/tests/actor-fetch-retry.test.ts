import { describe, expect, test } from "vitest";
import { ActorHandleRaw } from "@/client/actor-handle";
import { ActorError } from "@/client/errors";
import type {
	EngineControlClient,
	GatewayTarget,
} from "@/engine-client/driver";

describe("ActorHandleRaw.fetch", () => {
	test("replays a Request body after a lifecycle retry", async () => {
		const bodies: string[] = [];
		let attempts = 0;
		const driver = {
			async getOrCreateWithKey() {
				return { actorId: "actor-id", name: "example", key: ["key"] };
			},
			async sendRequest(_target: GatewayTarget, request: Request) {
				bodies.push(await request.text());
				attempts++;
				if (attempts === 1) {
					throw new ActorError("actor", "starting", "actor is starting");
				}
				return Response.json({ ok: true });
			},
		} as EngineControlClient;
		const handle = new ActorHandleRaw(
			{},
			driver,
			undefined,
			undefined,
			"json",
			{ getOrCreateForKey: { name: "example", key: ["key"] } },
		);
		const request = new Request("http://example.test/submit", {
			method: "POST",
			body: "persistent request body",
		});

		const response = await handle.fetch(request);

		expect(response.ok).toBe(true);
		expect(bodies).toEqual([
			"persistent request body",
			"persistent request body",
		]);
		expect(request.bodyUsed).toBe(false);
	});

});
