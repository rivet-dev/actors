// @ts-nocheck
import { type TestContext, vi } from "vitest";
import type { registry } from "../../fixtures/driver-test-suite/registry-static";
import {
	type Client,
	type ClientConfigInput,
	createClient,
} from "../../src/client/mod";
import { getLogger } from "../../src/common/log";
import type { DriverTestConfig } from "./shared-types";

export const FAKE_TIME = new Date("2024-01-01T00:00:00.000Z");
const TIMING_ENABLED = process.env.RIVETKIT_DRIVER_TEST_TIMING === "1";

function logger() {
	return getLogger("test-suite");
}

function timing(label: string, startedAt: number, testName?: string) {
	if (!TIMING_ENABLED) {
		return;
	}

	console.log(
		`DRIVER_TIMING ${label} ms=${Math.round(performance.now() - startedAt)}${testName ? ` test=${JSON.stringify(testName)}` : ""}`,
	);
}

// Must use `TestContext` since global hooks do not work when running concurrently.
export async function setupDriverTest(
	c: TestContext,
	driverTestConfig: DriverTestConfig,
	options: {
		client?: Partial<ClientConfigInput>;
	} = {},
): Promise<{
	client: Client<typeof registry>;
	endpoint: string;
	metricsEndpoint?: string;
	namespace: string;
	hardCrashActor?: (actorId: string) => Promise<void>;
	hardCrashRuntime?: () => Promise<void>;
	hardCrashPreservesData: boolean;
	getRuntimeOutput: () => string;
}> {
	if (!driverTestConfig.useRealTimers) {
		vi.useFakeTimers();
		vi.setSystemTime(FAKE_TIME);
	}
	const testName = c.task?.name;
	const setupStartedAt = performance.now();

	const driverStartStartedAt = performance.now();
	const {
		endpoint,
		metricsEndpoint,
		namespace,
		runnerName,
		hardCrashActor,
		hardCrashRuntime,
		hardCrashPreservesData,
		getRuntimeOutput,
		cleanup,
	} = await driverTestConfig.start();
	timing("setup.driver_start", driverStartStartedAt, testName);

	const clientStartedAt = performance.now();
	const client = createClient<typeof registry>({
		endpoint,
		namespace,
		poolName: runnerName,
		encoding: driverTestConfig.encoding,
		// Disable metadata lookup to prevent redirect to the wrong port.
		// Each test starts a runtime on a dynamic namespace and pool.
		disableMetadataLookup: true,
		...options.client,
	});
	timing("setup.client", clientStartedAt, testName);
	timing("setup.total", setupStartedAt, testName);

	c.onTestFinished(async () => {
		try {
			if (!driverTestConfig.HACK_skipCleanupNet) {
				const disposeStartedAt = performance.now();
				await client.dispose();
				timing("cleanup.client_dispose", disposeStartedAt, testName);
			}
		} finally {
			logger().info("cleaning up test");
			const cleanupStartedAt = performance.now();
			await cleanup();
			timing("cleanup.driver", cleanupStartedAt, testName);
		}
	});

	return {
		client,
		endpoint,
		metricsEndpoint,
		namespace,
		hardCrashActor,
		hardCrashRuntime,
		hardCrashPreservesData: hardCrashPreservesData ?? false,
		getRuntimeOutput: getRuntimeOutput ?? (() => ""),
	};
}

export async function waitFor(
	driverTestConfig: DriverTestConfig,
	ms: number,
): Promise<void> {
	if (driverTestConfig.useRealTimers) {
		return new Promise((resolve) => setTimeout(resolve, ms));
	} else {
		vi.advanceTimersByTime(ms);
		return Promise.resolve();
	}
}
