import { describe, expect, test } from "vitest";
import {
	encodeToEnvoy,
	HttpStreamAbortReasonKind,
} from "../src/index";

const REQUEST_ABORT_GOLDEN = [
	4, 1, 1, 1, 1, 7, 7, 7, 7, 1, 0, 2, 1, 1, 24, 99, 108, 105, 101, 110,
	116, 32, 99, 108, 111, 115, 101, 100, 32, 99, 111, 110, 110, 101, 99, 116,
	105, 111, 110,
];

describe("envoy HTTP abort protocol", () => {
	test("matches the Rust golden bytes", () => {
		const encoded = encodeToEnvoy({
			tag: "ToEnvoyTunnelMessage",
			val: {
				messageId: {
					gatewayId: new Uint8Array([1, 1, 1, 1]).buffer,
					requestId: new Uint8Array([7, 7, 7, 7]).buffer,
					messageIndex: 1,
				},
				messageKind: {
					tag: "ToEnvoyRequestAbort",
					val: {
						reason: {
							kind: HttpStreamAbortReasonKind.ClientDisconnect,
							detail: "client closed connection",
						},
					},
				},
			},
		});

		expect([...encoded]).toEqual(REQUEST_ABORT_GOLDEN);
	});
});
