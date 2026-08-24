import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { debugLog } from "../../src/actors/dispatcher.ts";

describe("structured debug logging", () => {
	let spy: ReturnType<typeof vi.spyOn>;
	const prev = process.env.RIVET_WORLD_RIVET_DEBUG;
	beforeEach(() => {
		process.env.RIVET_WORLD_RIVET_DEBUG = "1";
		spy = vi.spyOn(console, "error").mockImplementation(() => {});
	});
	afterEach(() => {
		spy.mockRestore();
		if (prev === undefined) delete process.env.RIVET_WORLD_RIVET_DEBUG;
		else process.env.RIVET_WORLD_RIVET_DEBUG = prev;
	});

	test("emits a single structured JSON line with scope, event, and trace id", () => {
		debugLog("dispatcher.deliver", { messageId: "msg_1", route: "flow" });
		expect(spy).toHaveBeenCalledTimes(1);
		const line = JSON.parse(spy.mock.calls[0][0] as string);
		expect(line).toMatchObject({
			scope: "@rivet-dev/workflow-world",
			event: "dispatcher.deliver",
			traceId: "msg_1",
			route: "flow",
		});
	});

	test("NEVER logs the bearer secret — sensitive keys are redacted", () => {
		debugLog("dispatcher.deliver", {
			messageId: "msg_2",
			headers: { authorization: "Bearer super-secret" },
			authorization: "Bearer super-secret",
			token: "super-secret",
		});
		const raw = spy.mock.calls[0][0] as string;
		expect(raw).not.toContain("super-secret");
		const line = JSON.parse(raw);
		expect(line.authorization).toBe("[redacted]");
		expect(line.headers).toBe("[redacted]");
		expect(line.token).toBe("[redacted]");
	});

	test("is silent unless RIVET_WORLD_RIVET_DEBUG=1", () => {
		delete process.env.RIVET_WORLD_RIVET_DEBUG;
		debugLog("dispatcher.deliver", { messageId: "msg_3" });
		expect(spy).not.toHaveBeenCalled();
	});
});
