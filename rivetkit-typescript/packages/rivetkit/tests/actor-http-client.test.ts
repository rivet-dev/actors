import { afterEach, describe, expect, test, vi } from "vitest";
import type { ClientConfig } from "@/client/config";
import { sendHttpRequestToGateway } from "@/engine-client/actor-http-client";

describe("sendHttpRequestToGateway", () => {
	afterEach(() => {
		vi.unstubAllGlobals();
	});

	test("relays an already-streaming gateway response without reconstructing its body", async () => {
		let finish!: () => void;
		const finishGate = new Promise<void>((resolve) => {
			finish = resolve;
		});
		const gatewayResponse = new Response(
			new ReadableStream<Uint8Array>({
				async start(controller) {
					controller.enqueue(new TextEncoder().encode("data: ready\n\n"));
					await finishGate;
					controller.enqueue(new TextEncoder().encode("data: done\n\n"));
					controller.close();
				},
			}),
			{
				headers: { "content-type": "text/event-stream" },
			},
		);
		const transportReader = gatewayResponse.body?.getReader();
		expect(transportReader).toBeDefined();
		let releaseTransport!: () => void;
		const transportReleased = new Promise<void>((resolve) => {
			releaseTransport = resolve;
		});
		vi.stubGlobal(
			"fetch",
			vi.fn(async () => {
				// A gateway implementation can still hold its transport reader
				// when the Response promise resolves, then release it before the
				// caller starts consuming the body. Reconstructing the Response
				// in this window throws before the stream can be relayed.
				setTimeout(() => {
					transportReader?.releaseLock();
					releaseTransport();
				}, 0);
				return gatewayResponse;
			}),
		);

		const response = await sendHttpRequestToGateway(
			{
				headers: {},
			} as ClientConfig,
			"http://gateway.test/actors/actor-id",
			new Request("http://actor/request/events"),
		);

		expect(response).toBe(gatewayResponse);
		await transportReleased;
		const reader = response.body?.getReader();
		expect(reader).toBeDefined();
		expect(
			new TextDecoder().decode((await reader?.read())?.value),
		).toBe("data: ready\n\n");
		finish();
		expect(
			new TextDecoder().decode((await reader?.read())?.value),
		).toBe("data: done\n\n");
		expect((await reader?.read())?.done).toBe(true);
	});
});
