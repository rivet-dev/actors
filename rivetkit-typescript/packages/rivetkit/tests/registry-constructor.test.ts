import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { actor, setup } from "@/mod";
import { RegistryConfigSchema } from "@/registry/config";
import { buildNativeRegistry, buildServeConfig } from "../src/registry/native";

vi.mock("@rivetkit/engine-cli", () => ({
	getEnginePath: () => "/tmp/rivet-engine",
}));

const testActor = actor({
	state: {},
	actions: {},
});

describe("Registry constructor", () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});

	afterEach(() => {
		vi.useRealTimers();
		vi.restoreAllMocks();
	});

	test("does not schedule prestart when it is not explicitly enabled", async () => {
		const setTimeoutSpy = vi.spyOn(globalThis, "setTimeout");
		const initialTimeoutCalls = setTimeoutSpy.mock.calls.length;

		setup({
			use: {
				test: testActor,
			},
		});

		expect(setTimeoutSpy.mock.calls).toHaveLength(initialTimeoutCalls);

		await vi.runAllTimersAsync();
		expect(setTimeoutSpy.mock.calls).toHaveLength(initialTimeoutCalls);
	});

	test("reads config mutations made before the native registry is built", async () => {
		const setTimeoutSpy = vi.spyOn(globalThis, "setTimeout");
		const initialTimeoutCalls = setTimeoutSpy.mock.calls.length;

		const registry = setup({
			use: {
				test: testActor,
			},
			startEngine: false,
			endpoint: "http://127.0.0.1:6642",
			token: "dev",
			namespace: "before-build",
			envoy: {
				poolName: "before-build-pool",
			},
		});

		registry.config.namespace = "after-build";
		registry.config.endpoint = "http://127.0.0.1:7755";
		registry.config.envoy = {
			...registry.config.envoy,
			poolName: "after-build-pool",
		};

		const { serveConfig } = await buildNativeRegistry(
			registry.parseConfig(),
		);

		expect(setTimeoutSpy.mock.calls).toHaveLength(initialTimeoutCalls);
		expect(new URL(serveConfig.endpoint).origin).toBe(
			"http://127.0.0.1:7755",
		);
		expect(serveConfig.namespace).toBe("after-build");
		expect(serveConfig.poolName).toBe("after-build-pool");
	});

	test("uses run-engine host and port for spawned local engine config", async () => {
		const config = RegistryConfigSchema.parse({
			use: {
				test: testActor,
			},
			startEngine: true,
			engineHost: "127.0.0.1",
			enginePort: 7654,
		});

		expect(config.endpoint).toBe("http://127.0.0.1:7654");
		expect(config.publicEndpoint).toBe("http://127.0.0.1:7654");

		const serveConfig = await buildServeConfig(config);

		expect(serveConfig.endpoint).toBe("http://127.0.0.1:7654");
		expect(serveConfig.engineHost).toBe("127.0.0.1");
		expect(serveConfig.enginePort).toBe(7654);
	});

	test("uses the configured local engine port without an explicit spawn flag", () => {
		const config = RegistryConfigSchema.parse({
			use: {
				test: testActor,
			},
			startEngine: false,
			engineHost: "127.0.0.1",
			enginePort: 7655,
		});

		expect(config.endpoint).toBe("http://127.0.0.1:7655");
		expect(config.publicEndpoint).toBe("http://127.0.0.1:7655");
	});

	test("does not override the bind address for an explicit engine endpoint", async () => {
		const config = RegistryConfigSchema.parse({
			use: {
				test: testActor,
			},
			startEngine: false,
			endpoint: "http://127.0.0.1:7656",
		});

		const serveConfig = await buildServeConfig(config);

		expect(new URL(serveConfig.endpoint).origin).toBe(
			"http://127.0.0.1:7656",
		);
		expect(serveConfig.engineHost).toBeUndefined();
		expect(serveConfig.enginePort).toBeUndefined();
	});

	test("keeps endpoint separate from spawned local engine config", () => {
		const result = RegistryConfigSchema.safeParse({
			use: {
				test: testActor,
			},
			startEngine: true,
			endpoint: "http://127.0.0.1:7654",
		});

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.error.issues).toEqual(
				expect.arrayContaining([
					expect.objectContaining({
						message: "cannot specify both startEngine and endpoint",
					}),
				]),
			);
		}
	});
});
