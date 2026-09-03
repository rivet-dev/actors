import { describe, expect, test } from "vitest";
import { ActorError } from "../src/client/errors";
import { isRetryableLifecycleRequestError } from "../src/client/lifecycle-errors";

describe("lifecycle error retry classification", () => {
	test.each([
		"service_unavailable",
		"actor_wake_retries_exceeded",
		"actor_stopped_while_waiting",
		"tunnel_request_aborted",
		"tunnel_message_timeout",
		"tunnel_response_closed",
		"gateway_response_start_timeout",
	])("classifies guard.%s as retryable", (code) => {
		expect(
			isRetryableLifecycleRequestError(
				new ActorError("guard", code, "transient gateway error"),
			),
		).toBe(true);
	});

	test("does not retry unconfirmed request delivery", () => {
		expect(
			isRetryableLifecycleRequestError(
				new ActorError(
					"guard",
					"request_delivery_unconfirmed",
					"Actor did not respond to request start",
				),
			),
		).toBe(false);
	});
});
