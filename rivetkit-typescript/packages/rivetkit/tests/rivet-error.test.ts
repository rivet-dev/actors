import { describe, expect, test } from "vitest";
import {
	decodeBridgeRivetError,
	encodeBridgeRivetError,
	RivetError,
	toRivetError,
} from "../src/actor/errors";
import { createClientWithDriver } from "../src/client/client";
import { deconstructError } from "../src/common/utils";
import type { EngineControlClient } from "../src/engine-client/driver";

describe("RivetError bridge helpers", () => {
	test("round trips structured bridge payloads", () => {
		const error = new RivetError("user", "boom", "typed failure", {
			metadata: { source: "native" },
			rayId: "ray-123",
			public: true,
			actor: {
				actorId: "actor-123",
				generation: 7,
				key: "chat/1",
			},
		});

		const decoded = decodeBridgeRivetError(encodeBridgeRivetError(error));

		expect(decoded).toBeInstanceOf(RivetError);
		expect(decoded).toMatchObject({
			group: "user",
			code: "boom",
			message: "typed failure",
			metadata: { source: "native" },
			rayId: "ray-123",
			actor: {
				actorId: "actor-123",
				generation: 7,
				key: "chat/1",
			},
		});
	});

	test("wraps plain errors with actor/internal_error defaults", () => {
		const error = toRivetError(new Error("plain failure"), {
			group: "actor",
			code: "internal_error",
		});

		expect(error).toMatchObject({
			group: "actor",
			code: "internal_error",
			message: "plain failure",
		});
	});

	test("passes through canonical RivetError instances", () => {
		const error = new RivetError(
			"actor",
			"action_timed_out",
			"Action timed out",
			{
				public: true,
				statusCode: 408,
				metadata: { source: "core" },
				rayId: "ray-456",
			},
		);

		const result = deconstructError(error);

		expect(result).toMatchObject({
			statusCode: 408,
			public: true,
			group: "actor",
			code: "action_timed_out",
			message: "Action timed out",
			metadata: { source: "core" },
			rayId: "ray-456",
		});
	});

	test("keeps ray ID separate from application metadata", () => {
		const metadata = { rayId: "application-value", source: "user" };
		const error = toRivetError(
			new RivetError("user", "boom", "typed failure", {
				metadata,
				rayId: "engine-value",
			}),
		);

		expect(error.rayId).toBe("engine-value");
		expect(error.metadata).toBe(metadata);
	});

	test("does not treat plain objects as structured errors", () => {
		const result = deconstructError({
			group: "foo",
			code: "bar",
			message: "baz",
		});

		expect(result).toMatchObject({
			statusCode: 500,
			public: false,
			group: "rivetkit",
			code: "internal_error",
			message: "An internal error occurred",
		});
	});

	test("classifies malformed tagged RivetError payloads", () => {
		const result = deconstructError(
			{ __type: "RivetError", code: "bar", message: "baz" },
			true,
		);

		expect(result).toMatchObject({
			statusCode: 500,
			public: false,
			group: "rivetkit",
			code: "internal_error",
			message: "baz",
		});
	});
});

describe("RivetError HTTP diagnostics", () => {
	test("exposes response ray ID from an actor action to the user", async () => {
		const metadata = { source: "engine" };
		const driver = {
			sendRequest: async () =>
				new Response(
					JSON.stringify({
						group: "core",
						code: "internal_error",
						message: "An internal error occurred",
						metadata,
					}),
					{
						status: 500,
						headers: {
							"content-type": "application/json",
							"x-rivet-ray-id": "ray-http-123",
						},
					},
				),
		} as EngineControlClient;
		const client = createClientWithDriver(driver, { encoding: "json" });
		const handle = client.getForId("test-actor", "actor-123");
		let thrown: unknown;

		try {
			await handle.action({ name: "fail", args: [] });
		} catch (error) {
			thrown = error;
		}

		expect(thrown).toBeInstanceOf(RivetError);
		expect(thrown).toMatchObject({
			group: "core",
			code: "internal_error",
			message: "An internal error occurred",
			metadata,
			rayId: "ray-http-123",
		});
	});
});
