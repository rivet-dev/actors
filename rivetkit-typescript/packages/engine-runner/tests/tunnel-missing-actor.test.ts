import { describe, expect, it, vi } from "vitest";
import { Tunnel } from "../src/tunnel";

describe("Tunnel missing actor request handling", () => {
	it("returns a retryable 503 when a request arrives for an unloaded actor", async () => {
		const sent: any[] = [];

		const runner = {
			log: undefined,
			getAndWaitForActor: vi.fn().mockResolvedValue(undefined),
			__sendToServer: vi.fn((message) => {
				sent.push(message);
			}),
		} as any;

		const tunnel = new Tunnel(runner);

		const gatewayId = new Uint8Array(16).buffer;
		const requestId = new Uint8Array(16).buffer;

		await tunnel.handleTunnelMessage({
			messageId: {
				gatewayId,
				requestId,
				messageIndex: 0,
			},
			messageKind: {
				tag: "ToClientRequestStart",
				val: {
					actorId: "missing-actor",
					method: "GET",
					path: "/",
					headers: new Map(),
					body: null,
					stream: false,
				},
			},
		} as any);

		expect(sent).toHaveLength(1);
		expect(sent[0].tag).toBe("ToServerTunnelMessage");
		expect(sent[0].val.messageKind.tag).toBe("ToServerResponseStart");
		expect(sent[0].val.messageKind.val.status).toBe(503);
		expect(sent[0].val.messageKind.val.headers.get("x-rivet-error")).toBe(
			"runner.actor_not_found",
		);
		expect(sent[0].val.messageKind.val.headers.get("content-length")).toBe(
			"15",
		);
		expect(new TextDecoder().decode(sent[0].val.messageKind.val.body)).toBe(
			"Actor not found",
		);
		expect(sent[0].val.messageKind.val.stream).toBe(false);
	});
});
