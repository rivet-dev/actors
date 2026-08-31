import { afterEach, describe, expect, test, vi } from "vitest";
import { loadNodeBuiltin } from "@/utils/node";

const originalGetBuiltinModule = Object.getOwnPropertyDescriptor(
	process,
	"getBuiltinModule",
);

function setGetBuiltinModule(value: ((id: string) => unknown) | undefined) {
	Object.defineProperty(process, "getBuiltinModule", {
		configurable: true,
		value,
		writable: true,
	});
}

describe("Node built-in loading", () => {
	afterEach(() => {
		vi.restoreAllMocks();
		if (originalGetBuiltinModule) {
			Object.defineProperty(
				process,
				"getBuiltinModule",
				originalGetBuiltinModule,
			);
		} else {
			delete (process as Partial<NodeJS.Process>).getBuiltinModule;
		}
	});

	test("prefers process.getBuiltinModule when available", () => {
		const builtin = { marker: "builtin" };
		const getBuiltinModule = vi.fn(() => builtin);
		setGetBuiltinModule(getBuiltinModule);

		expect(loadNodeBuiltin("node:test")).toBe(builtin);
		expect(getBuiltinModule).toHaveBeenCalledWith("node:test");
	});

	test("falls back to createRequire on older Node versions", () => {
		setGetBuiltinModule(undefined);

		const path = loadNodeBuiltin<typeof import("node:path")>("node:path");

		expect(path.basename(path.join("one", "two"))).toBe("two");
	});
});
