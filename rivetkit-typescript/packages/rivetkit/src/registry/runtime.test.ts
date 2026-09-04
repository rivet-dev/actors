import { describe, expect, test } from "vitest";
import type { RegistryConfig } from "./config";
import { loadAutoRuntime, type RuntimeLoaders } from "./native";
import {
	type CoreRuntime,
	normalizeRuntimeSqlExecuteResult,
	type RuntimeSqlBindParam,
	type RuntimeSqlBindParams,
} from "./runtime";

describe("runtime SQL boundary", () => {
	test("accepts exact bind param variants", () => {
		const blob = new Uint8Array([1, 2, 3]);
		const params = [
			{ kind: "null" },
			{ kind: "int", intValue: 1 },
			{ kind: "float", floatValue: 1.5 },
			{ kind: "text", textValue: "text" },
			{ kind: "blob", blobValue: blob },
		] satisfies RuntimeSqlBindParams;

		expect(params).toEqual([
			{ kind: "null" },
			{ kind: "int", intValue: 1 },
			{ kind: "float", floatValue: 1.5 },
			{ kind: "text", textValue: "text" },
			{ kind: "blob", blobValue: blob },
		]);
	});

	test("rejects bind params with mismatched value fields at typecheck time", () => {
		const invalidIntParamCandidate = {
			kind: "int",
			intValue: 1,
			textValue: "extra",
		} as const;
		// @ts-expect-error Runtime SQL int params must only carry intValue.
		const invalidIntParam: RuntimeSqlBindParam = invalidIntParamCandidate;

		expect(invalidIntParam.kind).toBe("int");
	});

	test("normalizes execute result metadata", () => {
		const base = {
			columns: ["value"],
			rows: [[1]],
			changes: 1,
			lastInsertRowId: null,
		};

		expect(normalizeRuntimeSqlExecuteResult(base)).toEqual(base);
	});
});

describe("loadAutoRuntime failure reporting", () => {
	const wasmRuntime = { kind: "wasm" } as unknown as CoreRuntime;
	const nativeRuntime = { kind: "napi" } as unknown as CoreRuntime;
	const config = {} as RegistryConfig;

	function loaders(overrides: Partial<RuntimeLoaders>): RuntimeLoaders {
		return {
			detectHost: () => "node-like",
			loadNative: async () => ({ runtime: nativeRuntime }) as never,
			loadWasm: async () => ({ runtime: wasmRuntime }) as never,
			...overrides,
		};
	}

	test("prefers native when it loads", async () => {
		const runtime = await loadAutoRuntime(config, loaders({}));
		expect(runtime).toBe(nativeRuntime);
	});

	test("falls back to wasm when native fails", async () => {
		const runtime = await loadAutoRuntime(
			config,
			loaders({
				loadNative: async () => {
					throw new Error("missing platform binding");
				},
			}),
		);
		expect(runtime).toBe(wasmRuntime);
	});

	test("reports the native cause when both runtimes fail", async () => {
		// The native failure is the actionable one. Before this was reported,
		// a skipped platform binding surfaced only as the wasm loader's
		// unrelated `file://` fetch error.
		const promise = loadAutoRuntime(
			config,
			loaders({
				loadNative: async () => {
					throw new Error(
						"Cannot find module '@rivetkit/rivetkit-napi-linux-x64-musl'",
					);
				},
				loadWasm: async () => {
					throw new Error("fetch failed");
				},
			}),
		);
		await expect(promise).rejects.toThrow(/rivetkit-napi-linux-x64-musl/);
		await expect(promise).rejects.toThrow(/fetch failed/);
	});

	test("uses wasm directly on an edge-like host without touching native", async () => {
		let nativeCalls = 0;
		const runtime = await loadAutoRuntime(
			config,
			loaders({
				detectHost: () => "edge-like",
				loadNative: async () => {
					nativeCalls += 1;
					throw new Error(
						"native must not be attempted on edge hosts",
					);
				},
			}),
		);
		expect(runtime).toBe(wasmRuntime);
		expect(nativeCalls).toBe(0);
	});
});
