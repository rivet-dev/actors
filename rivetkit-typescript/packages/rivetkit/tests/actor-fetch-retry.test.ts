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
	test("does not replay a Request body after delivery starts", async () => {
		const bodies: string[] = [];
		const driver = {
			async sendRequest(_target: GatewayTarget, request: Request) {
				bodies.push(await request.text());
				throw new ActorError(
					"actor",
					"starting",
					"actor is starting",
				);
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

		await expect(handle.fetch(request)).rejects.toMatchObject({
			group: "actor",
			code: "starting",
		});

		expect(bodies).toEqual(["persistent request body"]);
		expect(request.bodyUsed).toBe(true);
	});

	test("does not retry a body provided through init", async () => {
		let attempts = 0;
		const driver = {
			async sendRequest(_target: GatewayTarget, request: Request) {
				attempts++;
				expect(await request.text()).toBe("body from init");
				throw new ActorError(
					"actor",
					"starting",
					"actor is starting",
				);
			},
		} as EngineControlClient;
		const handle = dynamicHandle(driver);

		await expect(
			handle.fetch("http://example.test/submit", {
				method: "POST",
				body: "body from init",
			}),
		).rejects.toMatchObject({ group: "actor", code: "starting" });
		expect(attempts).toBe(1);
	});

	test("retains lifecycle retries for bodyless requests", async () => {
		let attempts = 0;
		const driver = {
			async sendRequest() {
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
		const handle = dynamicHandle(driver);

		const response = await handle.fetch("http://example.test/status");

		expect(response.ok).toBe(true);
		expect(attempts).toBe(2);
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
