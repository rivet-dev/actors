import {
	type ToServer,
	TO_SERVER_VERSIONED as toServer,
} from "rivetkit/inspector/client";
import { describe, expect, it, vi } from "vitest";
import {
	buildInspectorWebSocketUrl,
	getInspectorProtocolVersion,
	serverMessage,
	usesNegotiatedInspectorProtocol,
} from "./actor-inspector-context";

vi.mock("reconnectingwebsocket", () => ({ default: class {} }));

const request: ToServer = {
	body: { tag: "StateRequest", val: { id: 1n } },
};

describe("inspector protocol negotiation", () => {
	it("uses query-parameter negotiation for supporting actors", () => {
		const version = getInspectorProtocolVersion("2.3.4");
		expect(version).toBe(6);
		expect(usesNegotiatedInspectorProtocol("2.3.4")).toBe(true);
		expect(buildInspectorWebSocketUrl("https://actor", version, true)).toBe(
			"https://actor/inspector/connect?protocol_version=6",
		);
	});

	it("retains embedded framing for older actors", () => {
		const version = getInspectorProtocolVersion("2.3.3");
		expect(version).toBe(5);
		expect(usesNegotiatedInspectorProtocol("2.3.3")).toBe(false);
		expect(
			buildInspectorWebSocketUrl("https://actor", version, false),
		).toBe("https://actor/inspector/connect");
	});

	it("serializes negotiated messages without an embedded version", () => {
		const bytes = serverMessage(request, 6, true);
		expect(toServer.deserialize(bytes, 6).body.tag).toBe("StateRequest");
		expect(() => toServer.deserializeWithEmbeddedVersion(bytes)).toThrow();
	});

	it("serializes legacy messages with embedded v5 framing", () => {
		const bytes = serverMessage(request, 5, false);
		expect(toServer.deserializeWithEmbeddedVersion(bytes).body.tag).toBe(
			"StateRequest",
		);
	});
});
