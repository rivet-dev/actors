import { describe, expect, test } from "vitest";
import { ActorHandleRaw } from "@/client/actor-handle";
import { ActorError } from "@/client/errors";
import type {
	EngineControlClient,
	GatewayTarget,
} from "@/engine-client/driver";

const ENVOY_ADMISSION_ERRORS = [
	["actor_not_found", "Actor not found"],
	["actor_generation_mismatch", "Actor generation does not match"],
] as const;

function envoyAdmissionErrorResponse(code: string, message: string) {
	return Response.json(
		{
			group: "envoy",
			code,
			message,
			actor: { actorId: "actor-id", generation: 7 },
		},
		{
			status: 503,
			headers: { "x-rivet-error": `envoy.${code}` },
		},
	);
}

function dynamicHandle(driver: EngineControlClient) {
	return new ActorHandleRaw({}, driver, undefined, undefined, "json", {
		getOrCreateForKey: { name: "example", key: ["key"] },
	});
}

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
					throw new ActorError(
						"actor",
						"starting",
						"actor is starting",
					);
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

	test.each(
		ENVOY_ADMISSION_ERRORS,
	)("surfaces envoy.%s as an ActorError without retrying an action", async (code, message) => {
		let attempts = 0;
		const driver = {
			async sendRequest() {
				attempts++;
				return envoyAdmissionErrorResponse(code, message);
			},
		} as EngineControlClient;
		const handle = dynamicHandle(driver);

		let error: unknown;
		try {
			await handle.action({ name: "test", args: [] });
		} catch (cause) {
			error = cause;
		}

		expect(error).toBeInstanceOf(ActorError);
		expect(error).toMatchObject({
			group: "envoy",
			code,
			message,
			actor: { actorId: "actor-id", generation: 7 },
		});
		expect(attempts).toBe(1);
	});

	test.each(
		ENVOY_ADMISSION_ERRORS,
	)("keeps an envoy.%s raw fetch response readable without retrying", async (code, message) => {
		let attempts = 0;
		const driver = {
			async sendRequest() {
				attempts++;
				return envoyAdmissionErrorResponse(code, message);
			},
		} as EngineControlClient;
		const handle = dynamicHandle(driver);

		const response = await handle.fetch("http://actor/request");

		expect(response.ok).toBe(false);
		expect(response.status).toBe(503);
		expect(response.headers.get("content-type")).toContain(
			"application/json",
		);
		expect(response.headers.get("x-rivet-error")).toBe(`envoy.${code}`);
		expect(await response.json()).toEqual({
			group: "envoy",
			code,
			message,
			actor: { actorId: "actor-id", generation: 7 },
		});
		expect(attempts).toBe(1);
	});
});
