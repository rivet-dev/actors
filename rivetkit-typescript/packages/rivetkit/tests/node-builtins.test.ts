import { afterEach, describe, expect, test, vi } from "vitest";
import { loadNodeBuiltin } from "@/utils/node";

describe("Node built-in loading", () => {
	afterEach(() => {
		vi.restoreAllMocks();
	});

	test("prefers process.getBuiltinModule when available", () => {
		const builtin = { marker: "builtin" };
		const getBuiltinModule = vi
			.spyOn(process, "getBuiltinModule")
			.mockReturnValue(builtin);

		expect(loadNodeBuiltin("node:test")).toBe(builtin);
		expect(getBuiltinModule).toHaveBeenCalledWith("node:test");
	});

	test("falls back to createRequire on older Node versions", () => {
		vi.spyOn(process, "getBuiltinModule").mockReturnValue(undefined);

		const path = loadNodeBuiltin<typeof import("node:path")>("node:path");

		expect(path.join("one", "two")).toBe("one/two");
	});
});
