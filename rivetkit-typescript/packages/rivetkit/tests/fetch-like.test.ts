import { describe, expect, test } from "vitest";
import { rawHttpFetch } from "@/client/raw-utils";
import { isResponseLike } from "@/common/fetch-like";
import type { EngineControlClient } from "@/engine-client/driver";

describe("fetch objects across JavaScript realms", () => {
	test("raw HTTP accepts a request-compatible object without prototype identity", async () => {
		let forwarded: Request | undefined;
		const driver = {
			sendRequest: async (_target: unknown, request: Request) => {
				forwarded = request;
				return new Response("ok");
			},
		} as unknown as EngineControlClient;
		const source = new Request("https://example.com/path?query=yes", {
			headers: { "x-source": "present" },
		});
		const requestFromAnotherRealm = {
			url: source.url,
			method: source.method,
			headers: source.headers,
			body: source.body,
			mode: source.mode,
			credentials: source.credentials,
			redirect: source.redirect,
			referrer: source.referrer,
			referrerPolicy: source.referrerPolicy,
			integrity: source.integrity,
			keepalive: source.keepalive,
			signal: source.signal,
		} as Request;

		await rawHttpFetch(
			driver,
			{ directId: "actor-id" },
			undefined,
			requestFromAnotherRealm,
		);

		expect(forwarded?.url).toBe(
			"http://actor/request/path?query=yes",
		);
		expect(forwarded?.headers.get("x-source")).toBe("present");
		expect(
			forwarded?.headers.get("x-rivet-internal-original-request-url"),
		).toBe("https://example.com/path?query=yes");
	});

	test("recognizes a response-compatible object without prototype identity", () => {
		const source = new Response("ok", {
			status: 201,
			headers: { "x-source": "present" },
		});
		const responseFromAnotherRealm = {
			status: source.status,
			headers: source.headers,
			arrayBuffer: () => source.arrayBuffer(),
		};

		expect(isResponseLike(responseFromAnotherRealm)).toBe(true);
		expect(isResponseLike(undefined)).toBe(false);
	});
});
