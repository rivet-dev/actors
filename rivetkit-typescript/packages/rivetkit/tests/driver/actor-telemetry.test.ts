import getPort from "get-port";
import { expect, test } from "vitest";
import type { registry } from "../../fixtures/driver-test-suite/registry-static";
import { type Client, createClient } from "../../src/client/mod";
import { describeDriverMatrix } from "./shared-matrix";
import type { DriverDeployOutput, DriverTestConfig } from "./shared-types";

function tracedEnv(tracesEndpoint: string): Record<string, string> {
	return {
		OTEL_EXPORTER_OTLP_TRACES_ENDPOINT: tracesEndpoint,
		OTEL_EXPORTER_OTLP_TRACES_PROTOCOL: "http/json",
		OTEL_TRACES_SAMPLER: "always_on",
		OTEL_BSP_SCHEDULE_DELAY: "10",
	};
}

interface TracedRuntime {
	runtime: DriverDeployOutput;
	client: Client<typeof registry>;
	stop(): Promise<void>;
}

async function startTracedRuntime(
	config: DriverTestConfig,
	tracesEndpoint: string,
	extraEnv: Record<string, string> = {},
): Promise<TracedRuntime> {
	const runtime = await config.start({
		env: { ...tracedEnv(tracesEndpoint), ...extraEnv },
	});
	const client = createClient<typeof registry>({
		endpoint: runtime.endpoint,
		namespace: runtime.namespace,
		poolName: runtime.runnerName,
		encoding: config.encoding,
		disableMetadataLookup: true,
	});
	return {
		runtime,
		client,
		stop: async () => {
			await client.dispose();
			await runtime.cleanup();
		},
	};
}

describeDriverMatrix(
	"Actor Telemetry",
	(driverTestConfig) => {
		test("keeps actor behavior intact when the trace exporter is unavailable", async () => {
			const unavailable = `http://127.0.0.1:${await getPort({ host: "127.0.0.1" })}/v1/traces`;
			const traced = await startTracedRuntime(
				driverTestConfig,
				unavailable,
			);
			try {
				const handle = traced.client.telemetryActor.getOrCreate([
					`telemetry-down-${crypto.randomUUID()}`,
				]);
				expect(await handle.increment(4)).toBe(4);
			} finally {
				await traced.stop();
			}
		}, 60_000);
	},
	{
		runtimes: ["native"],
		sqliteBackends: ["local"],
		encodings: ["bare"],
	},
);
