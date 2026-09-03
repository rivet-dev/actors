import { type ChildProcess, spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { DriverRegistryVariant } from "../driver-registry-variants";
import {
	getOrStartSharedTestEngine,
	releaseSharedTestEngine,
	type SharedTestEngine,
	type SharedTestEngineOptions,
	TEST_ENGINE_TOKEN,
} from "../shared-engine";
import type {
	DriverDeployOutput,
	DriverSqliteBackend,
	DriverTestConfig,
} from "./shared-types";

const DRIVER_TEST_DIR = dirname(fileURLToPath(import.meta.url));
const TEST_DIR = join(DRIVER_TEST_DIR, "..");
const FIXTURE_PATH = join(TEST_DIR, "fixtures", "driver-test-suite-runtime.ts");
const WASM_FIXTURE_PATH = join(
	TEST_DIR,
	"fixtures",
	"driver-test-suite-wasm-runtime.ts",
);
export const TOKEN = TEST_ENGINE_TOKEN;
const TIMING_ENABLED = process.env.RIVETKIT_DRIVER_TEST_TIMING === "1";

interface RuntimeLogs {
	stdout: string;
	stderr: string;
}

export type SharedEngine = SharedTestEngine;

export interface NativeDriverTestConfigOptions {
	variant: DriverRegistryVariant;
	encoding: NonNullable<DriverTestConfig["encoding"]>;
	sqliteBackend: DriverSqliteBackend;
	useRealTimers?: boolean;
	skip?: DriverTestConfig["skip"];
	features?: DriverTestConfig["features"];
	engine?: SharedTestEngineOptions;
}

function childOutput(logs: RuntimeLogs): string {
	return [logs.stdout, logs.stderr].filter(Boolean).join("\n");
}

function timing(
	label: string,
	startedAt: number,
	fields: Record<string, string> = {},
) {
	if (!TIMING_ENABLED) {
		return;
	}

	const fieldText = Object.entries(fields)
		.map(([key, value]) => `${key}=${value}`)
		.join(" ");
	console.log(
		`DRIVER_TIMING ${label} ms=${Math.round(performance.now() - startedAt)}${fieldText ? ` ${fieldText}` : ""}`,
	);
}

async function waitForEnvoy(
	child: ChildProcess,
	logs: RuntimeLogs,
	endpoint: string,
	namespace: string,
	poolName: string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;

	while (Date.now() < deadline) {
		if (child.exitCode !== null) {
			throw new Error(
				`native runtime exited before envoy registration:\n${childOutput(logs)}`,
			);
		}

		const response = await fetch(
			`${endpoint}/envoys?namespace=${encodeURIComponent(namespace)}&name=${encodeURIComponent(poolName)}`,
			{
				headers: {
					Authorization: `Bearer ${TOKEN}`,
				},
			},
		);

		if (response.ok) {
			const body = (await response.json()) as {
				envoys: Array<{ envoy_key: string }>;
			};

			if (body.envoys.length > 0) {
				return;
			}
		}

		await new Promise((resolve) => setTimeout(resolve, 500));
	}

	throw new Error(
		`timed out waiting for envoy registration in pool ${poolName}\n${childOutput(logs)}`,
	);
}

async function upsertNormalRunnerConfig(
	logs: RuntimeLogs,
	endpoint: string,
	namespace: string,
	poolName: string,
): Promise<void> {
	const datacentersStartedAt = performance.now();
	const datacentersResponse = await fetch(
		`${endpoint}/datacenters?namespace=${encodeURIComponent(namespace)}`,
		{
			headers: {
				Authorization: `Bearer ${TOKEN}`,
			},
		},
	);

	if (!datacentersResponse.ok) {
		throw new Error(
			`failed to list datacenters: ${datacentersResponse.status} ${await datacentersResponse.text()}\n${childOutput(logs)}`,
		);
	}

	const datacentersBody = (await datacentersResponse.json()) as {
		datacenters: Array<{ name: string }>;
	};
	const datacenter = datacentersBody.datacenters[0]?.name;

	if (!datacenter) {
		throw new Error(`engine returned no datacenters\n${childOutput(logs)}`);
	}
	timing("runner_config.datacenters", datacentersStartedAt, { namespace });

	const deadline = Date.now() + 30_000;

	while (Date.now() < deadline) {
		const upsertStartedAt = performance.now();
		const response = await fetch(
			`${endpoint}/runner-configs/${encodeURIComponent(poolName)}?namespace=${encodeURIComponent(namespace)}`,
			{
				method: "PUT",
				headers: {
					Authorization: `Bearer ${TOKEN}`,
					"Content-Type": "application/json",
				},
				body: JSON.stringify({
					datacenters: {
						[datacenter]: {
							normal: {},
						},
					},
				}),
			},
		);

		if (response.ok) {
			timing("runner_config.upsert", upsertStartedAt, {
				namespace,
				poolName,
			});
			return;
		}

		const responseBody = await response.text();
		if (
			(response.status === 400 &&
				responseBody.includes('"group":"namespace"') &&
				responseBody.includes('"code":"not_found"')) ||
			(response.status === 500 &&
				responseBody.includes('"group":"core"') &&
				responseBody.includes('"code":"internal_error"'))
		) {
			await new Promise((resolve) => setTimeout(resolve, 500));
			continue;
		}

		throw new Error(
			`failed to upsert runner config ${poolName}: ${response.status} ${responseBody}\n${childOutput(logs)}`,
		);
	}

	throw new Error(
		`timed out waiting to upsert runner config ${poolName}\n${childOutput(logs)}`,
	);
}

async function createNamespace(
	endpoint: string,
	namespace: string,
): Promise<void> {
	const startedAt = performance.now();
	const response = await fetch(`${endpoint}/namespaces`, {
		method: "POST",
		headers: {
			Authorization: `Bearer ${TOKEN}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({
			name: namespace,
			display_name: `Driver test ${namespace}`,
		}),
	});

	if (!response.ok) {
		throw new Error(
			`failed to create namespace ${namespace}: ${response.status} ${await response.text()}`,
		);
	}
	timing("namespace.create", startedAt, { namespace });
}

async function stopProcess(
	child: ChildProcess,
	signal: NodeJS.Signals,
	timeoutMs: number,
): Promise<void> {
	if (child.exitCode !== null || child.signalCode !== null) {
		return;
	}

	await new Promise<void>((resolve) => {
		let settled = false;
		let timeout: ReturnType<typeof setTimeout> | undefined;
		const finish = () => {
			if (settled) return;
			settled = true;
			if (timeout) clearTimeout(timeout);
			child.off("exit", finish);
			resolve();
		};
		child.once("exit", finish);
		timeout = setTimeout(() => {
			if (child.exitCode === null && child.signalCode === null) {
				child.kill("SIGKILL");
			}
			finish();
		}, timeoutMs);

		if (
			!child.kill(signal) ||
			child.exitCode !== null ||
			child.signalCode !== null
		) {
			finish();
		}
	});
}

export async function getOrStartSharedEngine(
	options: SharedTestEngineOptions = {},
): Promise<SharedEngine> {
	return getOrStartSharedTestEngine(options);
}

export async function releaseSharedEngine(): Promise<void> {
	await releaseSharedTestEngine();
}

async function stopRuntime(child: ChildProcess): Promise<void> {
	const startedAt = performance.now();
	await stopProcess(child, "SIGTERM", 1_000);
	timing("runtime.stop", startedAt);
}

export async function startNativeDriverRuntime(
	variant: DriverRegistryVariant,
	engine: SharedEngine,
	sqliteBackend: DriverSqliteBackend,
): Promise<DriverDeployOutput> {
	const startedAt = performance.now();
	const endpoint = engine.endpoint;
	const namespace = `driver-${crypto.randomUUID()}`;
	const poolName = `driver-suite-${crypto.randomUUID()}`;
	const logs: RuntimeLogs = { stdout: "", stderr: "" };

	await createNamespace(endpoint, namespace);
	await upsertNormalRunnerConfig(logs, endpoint, namespace, poolName);

	const spawnStartedAt = performance.now();
	const runtime = spawn(process.execPath, ["--import", "tsx", FIXTURE_PATH], {
		cwd: dirname(TEST_DIR),
		env: {
			...process.env,
			RIVET_TOKEN: TOKEN,
			RIVET_NAMESPACE: namespace,
			RIVETKIT_DRIVER_REGISTRY_PATH: variant.registryPath,
			RIVETKIT_TEST_ENDPOINT: endpoint,
			RIVETKIT_TEST_POOL_NAME: poolName,
			RIVETKIT_TEST_SQLITE_BACKEND: sqliteBackend,
		},
		stdio: ["ignore", "pipe", "pipe"],
	});
	timing("runtime.spawn", spawnStartedAt, { namespace, poolName });

	runtime.stdout?.on("data", (chunk) => {
		const text = chunk.toString();
		logs.stdout += text;
		if (process.env.DRIVER_RUNTIME_LOGS === "1") {
			process.stderr.write(`[RT.OUT] ${text}`);
		}
	});
	runtime.stderr?.on("data", (chunk) => {
		const text = chunk.toString();
		logs.stderr += text;
		if (process.env.DRIVER_RUNTIME_LOGS === "1") {
			process.stderr.write(`[RT.ERR] ${text}`);
		}
	});

	try {
		const envoyStartedAt = performance.now();
		await waitForEnvoy(
			runtime,
			logs,
			endpoint,
			namespace,
			poolName,
			30_000,
		);
		timing("runtime.envoy", envoyStartedAt, { namespace, poolName });
	} catch (error) {
		await stopRuntime(runtime);
		throw error;
	}
	timing("runtime.start_total", startedAt, { namespace, poolName });

	return {
		endpoint,
		metricsEndpoint: engine.metricsEndpoint,
		namespace,
		runnerName: poolName,
		hardCrashRuntime: async () => {
			await stopProcess(runtime, "SIGKILL", 1_000);
		},
		getRuntimeOutput: () => childOutput(logs),
		cleanup: async () => {
			await stopRuntime(runtime);
		},
	};
}

export async function startWasmDriverRuntime(
	variant: DriverRegistryVariant,
	engine: SharedEngine,
): Promise<DriverDeployOutput> {
	const startedAt = performance.now();
	const endpoint = engine.endpoint;
	const namespace = `driver-${crypto.randomUUID()}`;
	const poolName = `driver-suite-${crypto.randomUUID()}`;
	const logs: RuntimeLogs = { stdout: "", stderr: "" };

	await createNamespace(endpoint, namespace);
	await upsertNormalRunnerConfig(logs, endpoint, namespace, poolName);
	// The parent test process may need an explicit native engine binary to start
	// the shared engine. Do not leak that native-only override into the Wasm
	// runtime, which connects to the already-running endpoint and must never try
	// to configure an engine child of its own.
	const {
		RIVET_ENGINE_BINARY: _engineBinary,
		RIVET_ENGINE_BINARY_PATH: _engineBinaryPath,
		...wasmEnv
	} = process.env;

	const spawnStartedAt = performance.now();
	const runtime = spawn(
		process.execPath,
		["--import", "tsx", WASM_FIXTURE_PATH],
		{
			cwd: dirname(TEST_DIR),
			env: {
				...wasmEnv,
				RIVET_TOKEN: TOKEN,
				RIVET_NAMESPACE: namespace,
				RIVETKIT_DRIVER_REGISTRY_PATH: variant.registryPath,
				RIVETKIT_TEST_ENDPOINT: endpoint,
				RIVETKIT_TEST_POOL_NAME: poolName,
				RIVETKIT_TEST_SQLITE_BACKEND: "remote",
			},
			stdio: ["ignore", "pipe", "pipe"],
		},
	);
	timing("wasm_runtime.spawn", spawnStartedAt, { namespace, poolName });

	runtime.stdout?.on("data", (chunk) => {
		const text = chunk.toString();
		logs.stdout += text;
		if (process.env.DRIVER_RUNTIME_LOGS === "1") {
			process.stderr.write(`[WASM_RT.OUT] ${text}`);
		}
	});
	runtime.stderr?.on("data", (chunk) => {
		const text = chunk.toString();
		logs.stderr += text;
		if (process.env.DRIVER_RUNTIME_LOGS === "1") {
			process.stderr.write(`[WASM_RT.ERR] ${text}`);
		}
	});

	try {
		const envoyStartedAt = performance.now();
		await waitForEnvoy(
			runtime,
			logs,
			endpoint,
			namespace,
			poolName,
			30_000,
		);
		timing("wasm_runtime.envoy", envoyStartedAt, { namespace, poolName });
	} catch (error) {
		await stopRuntime(runtime);
		throw error;
	}
	timing("wasm_runtime.start_total", startedAt, { namespace, poolName });

	return {
		endpoint,
		metricsEndpoint: engine.metricsEndpoint,
		namespace,
		runnerName: poolName,
		getRuntimeOutput: () => childOutput(logs),
		cleanup: async () => {
			await stopRuntime(runtime);
		},
	};
}

export function createNativeDriverTestConfig(
	options: NativeDriverTestConfigOptions,
): DriverTestConfig {
	return {
		runtime: "native",
		sqliteBackend: options.sqliteBackend,
		encoding: options.encoding,
		skip: options.skip,
		features: {
			hibernatableWebSocketProtocol: true,
			...options.features,
		},
		useRealTimers: options.useRealTimers ?? true,
		start: async () => {
			const engine = await getOrStartSharedEngine(options.engine);
			return startNativeDriverRuntime(
				options.variant,
				engine,
				options.sqliteBackend,
			);
		},
	};
}

export function createWasmDriverTestConfig(
	options: Omit<NativeDriverTestConfigOptions, "sqliteBackend">,
): DriverTestConfig {
	return {
		runtime: "wasm",
		sqliteBackend: "remote",
		encoding: options.encoding,
		skip: options.skip,
		features: {
			hibernatableWebSocketProtocol: false,
			...options.features,
		},
		useRealTimers: options.useRealTimers ?? true,
		start: async () => {
			const engine = await getOrStartSharedEngine(options.engine);
			return startWasmDriverRuntime(options.variant, engine);
		},
	};
}
