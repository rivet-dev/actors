import { afterEach, describe, expect, test } from "vitest";
import { actor } from "@/actor/mod";
import { type RegistryConfig, RegistryConfigSchema } from "@/registry/config";
import {
	loadConfiguredRuntime,
	normalizeRuntimeConfig,
	normalizeRuntimeConfigForKind,
	type RuntimeLoaders,
} from "@/registry/native";
import type { CoreRuntime } from "@/registry/runtime";

const previousRuntimeEnv = process.env.RIVETKIT_RUNTIME;

const testActor = actor({
	state: {},
	actions: {},
});

function parseConfig(input: Record<string, unknown> = {}) {
	return RegistryConfigSchema.parse({
		use: { test: testActor },
		startEngine: false,
		...input,
	});
}

function fakeRuntime(kind: CoreRuntime["kind"]): CoreRuntime {
	return { kind } as CoreRuntime;
}

function fakeLoaders(options: {
	nativeRuntime?: CoreRuntime;
	wasmRuntime?: CoreRuntime;
	nativeError?: Error;
	host?: "node-like" | "edge-like";
	onLoadWasm?: (config: RegistryConfig["wasm"] | undefined) => void;
	onLoadNative?: () => void;
}): RuntimeLoaders {
	return {
		detectHost: () => options.host ?? "node-like",
		loadNative: async () => {
			options.onLoadNative?.();
			if (options.nativeError) {
				throw options.nativeError;
			}
			return {
				bindings: {},
				runtime: options.nativeRuntime ?? fakeRuntime("napi"),
			} as Awaited<ReturnType<RuntimeLoaders["loadNative"]>>;
		},
		loadWasm: async (config) => {
			options.onLoadWasm?.(config);
			return {
				bindings: {},
				runtime: options.wasmRuntime ?? fakeRuntime("wasm"),
			} as Awaited<ReturnType<RuntimeLoaders["loadWasm"]>>;
		},
	};
}

describe("runtime selection", () => {
	afterEach(() => {
		if (previousRuntimeEnv === undefined) {
			delete process.env.RIVETKIT_RUNTIME;
		} else {
			process.env.RIVETKIT_RUNTIME = previousRuntimeEnv;
		}
	});

	test("config runtime wins over env runtime", async () => {
		process.env.RIVETKIT_RUNTIME = "wasm";
		const nativeRuntime = fakeRuntime("napi");

		const runtime = await loadConfiguredRuntime(
			parseConfig({ runtime: "native" }),
			fakeLoaders({ nativeRuntime }),
		);

		expect(runtime).toBe(nativeRuntime);
	});

	test("env selects wasm when config omits runtime", async () => {
		process.env.RIVETKIT_RUNTIME = "wasm";
		const wasmRuntime = fakeRuntime("wasm");

		const runtime = await loadConfiguredRuntime(
			parseConfig(),
			fakeLoaders({ wasmRuntime }),
		);

		expect(runtime).toBe(wasmRuntime);
	});

	test("rejects invalid RIVETKIT_RUNTIME values", () => {
		process.env.RIVETKIT_RUNTIME = "bad-runtime";

		expect(() => parseConfig()).toThrow(
			/RIVETKIT_RUNTIME must be one of auto, native, or wasm/,
		);
	});

	test("auto selects native in Node-like runtimes", async () => {
		const nativeRuntime = fakeRuntime("napi");

		const runtime = await loadConfiguredRuntime(
			parseConfig({ runtime: "auto" }),
			fakeLoaders({ host: "node-like", nativeRuntime }),
		);

		expect(runtime).toBe(nativeRuntime);
	});

	test("auto falls back to wasm when native import fails", async () => {
		const wasmRuntime = fakeRuntime("wasm");

		const runtime = await loadConfiguredRuntime(
			parseConfig({ runtime: "auto" }),
			fakeLoaders({
				host: "node-like",
				nativeError: new Error("native unavailable"),
				wasmRuntime,
			}),
		);

		expect(runtime).toBe(wasmRuntime);
	});

	test("auto selects wasm in edge-like runtimes", async () => {
		const wasmRuntime = fakeRuntime("wasm");
		let nativeLoads = 0;

		const runtime = await loadConfiguredRuntime(
			parseConfig({ runtime: "auto" }),
			fakeLoaders({
				host: "edge-like",
				wasmRuntime,
				onLoadNative: () => {
					nativeLoads += 1;
				},
			}),
		);

		expect(runtime).toBe(wasmRuntime);
		expect(nativeLoads).toBe(0);
	});

	test("actorsPerThread forces the native runtime without wasm fallback", async () => {
		const nativeRuntime = fakeRuntime("napi");
		let wasmLoads = 0;
		const runtime = await loadConfiguredRuntime(
			parseConfig({ runtime: "auto", actorsPerThread: 4 }),
			fakeLoaders({
				nativeRuntime,
				onLoadWasm: () => {
					wasmLoads += 1;
				},
			}),
		);

		expect(runtime).toBe(nativeRuntime);
		expect(wasmLoads).toBe(0);
	});

	test("actorsPerThread rejects the wasm runtime", async () => {
		await expect(
			loadConfiguredRuntime(
				parseConfig({ runtime: "wasm", actorsPerThread: 1 }),
				fakeLoaders({}),
			),
		).rejects.toThrow(/requires the native Node.js runtime/);
	});

	test("actorsPerThread rejects edge-like hosts", async () => {
		await expect(
			loadConfiguredRuntime(
				parseConfig({ actorsPerThread: 1 }),
				fakeLoaders({ host: "edge-like" }),
			),
		).rejects.toThrow(/only supported in Node.js/);
	});

	test.each([
		"Bun",
		"Deno",
	] as const)("actorsPerThread rejects the %s Node compatibility layer", async (runtimeGlobal) => {
		const config = parseConfig({ actorsPerThread: 1 });
		const globals = globalThis as typeof globalThis &
			Record<"Bun" | "Deno", unknown>;
		globals[runtimeGlobal] = {};
		try {
			await expect(
				loadConfiguredRuntime(
					config,
					fakeLoaders({ host: "node-like" }),
				),
			).rejects.toThrow(/Bun and Deno are not supported/);
		} finally {
			delete globals[runtimeGlobal];
		}
	});

	test("passes explicit wasm init input to the wasm loader", async () => {
		const initInput = new Uint8Array([0, 1, 2]);
		let observedInitInput: unknown;

		await loadConfiguredRuntime(
			parseConfig({ runtime: "wasm", wasm: { initInput } }),
			fakeLoaders({
				onLoadWasm: (config) => {
					observedInitInput = config?.initInput;
				},
			}),
		);

		expect(observedInitInput).toBe(initInput);
	});

	test("passes configured wasm bindings to the wasm loader", async () => {
		const bindings = { default: async () => {} };
		let observedBindings: unknown;

		await loadConfiguredRuntime(
			parseConfig({ runtime: "wasm", wasm: { bindings } }),
			fakeLoaders({
				onLoadWasm: (config) => {
					observedBindings = config?.bindings;
				},
			}),
		);

		expect(observedBindings).toBe(bindings);
	});

	test("wasm defaults SQLite to remote when SQLite is unset", () => {
		const config = parseConfig({ runtime: "wasm" });
		const normalized = normalizeRuntimeConfigForKind(config, "wasm");

		expect(config.sqlite?.backend).toBe("remote");
		expect(normalized.test.sqliteBackend).toBe("remote");
	});

	test("wasm allows explicit remote SQLite", () => {
		const config = parseConfig({
			runtime: "wasm",
			sqlite: "remote",
		});
		const normalized = normalizeRuntimeConfigForKind(config, "wasm");

		expect(config.sqlite?.backend).toBe("remote");
		expect(normalized.test.sqliteBackend).toBe("remote");
	});

	test("wasm rejects explicit local SQLite during setup config parsing", () => {
		expect(() =>
			parseConfig({
				runtime: "wasm",
				sqlite: "local",
			}),
		).toThrow(/WebAssembly runtime cannot use local SQLite/);
	});

	test("native keeps SQLite default unset and allows local or remote SQLite", () => {
		expect(parseConfig({ runtime: "native" }).sqlite).toBeUndefined();
		expect(
			normalizeRuntimeConfigForKind(
				parseConfig({ runtime: "native", sqlite: "local" }),
				"native",
			).sqlite?.backend,
		).toBe("local");
		expect(
			normalizeRuntimeConfigForKind(
				parseConfig({ runtime: "native", sqlite: "remote" }),
				"native",
			).sqlite?.backend,
		).toBe("remote");
	});

	test("normalizes plain object NAPI runtime fakes as native", () => {
		const config = parseConfig({
			runtime: "native",
			test: { sqliteBackend: "local" },
		});
		const normalized = normalizeRuntimeConfig(config, fakeRuntime("napi"));

		expect(normalized.test.sqliteBackend).toBe("local");
	});

	test("normalizes plain object wasm runtime fakes as wasm", () => {
		const normalized = normalizeRuntimeConfig(
			parseConfig({ runtime: "wasm" }),
			fakeRuntime("wasm"),
		);

		expect(normalized.test.sqliteBackend).toBe("remote");
	});

	test("wasm rejects local SQLite", () => {
		const config = parseConfig({
			runtime: "auto",
			test: { sqliteBackend: "local" },
		});

		expect(() => normalizeRuntimeConfigForKind(config, "wasm")).toThrow(
			/WebAssembly runtime cannot use local SQLite/,
		);
	});
});
