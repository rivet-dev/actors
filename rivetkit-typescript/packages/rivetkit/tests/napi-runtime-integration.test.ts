import { type ChildProcess, spawn } from "node:child_process";
import { mkdtemp, readdir, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import getPort from "get-port";
import { afterEach, describe, expect, test } from "vitest";
import { createClient } from "../src/client/mod";

const TEST_DIR = dirname(fileURLToPath(import.meta.url));
const FIXTURE_PATH = join(TEST_DIR, "fixtures", "napi-runtime-server.ts");
const NAMESPACE = "default";
const TOKEN = "dev";
const SERVICES_POOL_NAME = "services";
let runtimeLogs = {
	stdout: "",
	stderr: "",
};
let engineEndpoint: string | undefined;
let storagePath: string | undefined;

function childOutput(child: ChildProcess): string {
	void child;
	return [runtimeLogs.stdout, runtimeLogs.stderr].filter(Boolean).join("\n");
}

async function engineOutput(): Promise<string> {
	if (!storagePath) return "";
	const logsPath = join(
		storagePath,
		".rivetkit",
		"var",
		"logs",
		"rivet-engine",
	);
	try {
		const files = await readdir(logsPath);
		return (
			await Promise.all(
				files.map(
					async (file) =>
						`${file}:\n${await readFile(join(logsPath, file), "utf8")}`,
				),
			)
		).join("\n");
	} catch {
		return "";
	}
}

async function waitForHealth(
	child: ChildProcess,
	endpoint: string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;

	while (Date.now() < deadline) {
		if (child.exitCode !== null) {
			throw new Error(
				`native runtime exited before health check passed:\n${childOutput(child)}\n${await engineOutput()}`,
			);
		}

		try {
			const response = await fetch(`${endpoint}/health`);
			if (response.ok) {
				return;
			}
		} catch {}

		await new Promise((resolve) => setTimeout(resolve, 500));
	}

	throw new Error(
		`timed out waiting for native runtime health:\n${childOutput(child)}\n${await engineOutput()}`,
	);
}

async function waitForActorSleep(
	endpoint: string,
	actorId: string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;

	while (Date.now() < deadline) {
		const response = await fetch(
			`${endpoint}/actors?actor_ids=${encodeURIComponent(actorId)}&namespace=${encodeURIComponent(NAMESPACE)}`,
			{
				headers: {
					Authorization: `Bearer ${TOKEN}`,
				},
			},
		);
		expect(response.ok).toBe(true);

		const body = (await response.json()) as {
			actors: Array<{ sleep_ts?: number | null }>;
		};
		const actor = body.actors[0];
		if (actor?.sleep_ts) {
			return;
		}

		await new Promise((resolve) => setTimeout(resolve, 500));
	}

	throw new Error(`timed out waiting for actor ${actorId} to sleep`);
}

async function waitForActorReady<T>(
	callback: () => Promise<T>,
	timeoutMs: number,
): Promise<T> {
	const deadline = Date.now() + timeoutMs;
	let lastError: unknown;

	while (Date.now() < deadline) {
		try {
			return await callback();
		} catch (error) {
			lastError = error;
			const errorCode =
				typeof error === "object" &&
				error !== null &&
				"code" in error &&
				typeof error.code === "string"
					? error.code
					: undefined;
			if (
				!(
					(errorCode &&
						/^(no_envoys|actor_ready_timeout|actor_wake_retries_exceeded|service_unavailable)$/.test(
							errorCode,
						)) ||
					(error instanceof Error &&
						/(no_envoys|actor_ready_timeout|actor_wake_retries_exceeded|service_unavailable)/.test(
							error.message,
						))
				)
			) {
				throw error;
			}
		}

		await new Promise((resolve) => setTimeout(resolve, 500));
	}

	throw lastError instanceof Error
		? lastError
		: new Error("timed out waiting for actor to become ready");
}

async function waitForEnvoy(
	child: ChildProcess,
	endpoint: string,
	poolName: string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;

	while (Date.now() < deadline) {
		if (child.exitCode !== null) {
			throw new Error(
				`native runtime exited before envoy registration:\n${childOutput(child)}`,
			);
		}

		const response = await fetch(
			`${endpoint}/envoys?namespace=${encodeURIComponent(NAMESPACE)}&name=${encodeURIComponent(poolName)}`,
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
		`timed out waiting for envoy registration in pool ${poolName}\n${childOutput(child)}`,
	);
}

function servicesPid(): number {
	const match = runtimeLogs.stdout.match(
		/Services process is ready[^\n]*\bpid=(\d+)/,
	);
	if (!match) {
		throw new Error("Services readiness log did not include a pid");
	}
	return Number(match[1]);
}

async function waitForProcessExit(
	pid: number,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	let lastState: string | undefined;
	while (Date.now() < deadline) {
		try {
			process.kill(pid, 0);
		} catch {
			return;
		}
		try {
			const stat = await readFile(`/proc/${pid}/stat`, "utf8");
			lastState = stat.slice(stat.lastIndexOf(") ") + 2).charAt(0);
			if (lastState === "Z") return;
		} catch {
			// `/proc` is Linux-specific; process.kill remains the portable check.
		}
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
	throw new Error(
		`timed out waiting for process ${pid} to stop (state ${lastState ?? "unknown"})`,
	);
}

async function expectNormalRunnerConfig(
	endpoint: string,
	poolName: string,
): Promise<void> {
	const response = await fetch(
		`${endpoint}/runner-configs?namespace=${encodeURIComponent(NAMESPACE)}&runner_name=${encodeURIComponent(poolName)}`,
		{
			headers: { Authorization: `Bearer ${TOKEN}` },
		},
	);
	expect(response.ok).toBe(true);
	const body = (await response.json()) as {
		runner_configs: Record<
			string,
			{ datacenters: Record<string, { normal?: unknown }> }
		>;
	};
	const runnerConfig = body.runner_configs[poolName];
	expect(runnerConfig).toBeDefined();
	expect(
		Object.values(runnerConfig?.datacenters ?? {}).some(
			(datacenter) => datacenter.normal !== undefined,
		),
	).toBe(true);
}

async function createServicesActor(endpoint: string): Promise<string> {
	const response = await fetch(
		`${endpoint}/actors?namespace=${encodeURIComponent(NAMESPACE)}`,
		{
			method: "POST",
			headers: {
				Authorization: `Bearer ${TOKEN}`,
				"Content-Type": "application/json",
			},
			body: JSON.stringify({
				name: "services",
				key: `integration-${crypto.randomUUID()}`,
				runner_name_selector: SERVICES_POOL_NAME,
				crash_policy: "destroy",
			}),
		},
	);
	if (!response.ok) {
		throw new Error(
			`failed to create Services actor: ${response.status} ${await response.text()}`,
		);
	}
	const body = (await response.json()) as { actor: { actor_id: string } };
	return body.actor.actor_id;
}

async function waitForActorStarted(
	endpoint: string,
	actorId: string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		const response = await fetch(
			`${endpoint}/actors?actor_ids=${encodeURIComponent(actorId)}&namespace=${encodeURIComponent(NAMESPACE)}`,
			{
				headers: { Authorization: `Bearer ${TOKEN}` },
			},
		);
		if (response.ok) {
			const body = (await response.json()) as {
				actors: Array<{ start_ts?: number | null; error?: unknown }>;
			};
			const actor = body.actors[0];
			if (actor?.error) {
				throw new Error(
					`Services actor failed: ${JSON.stringify(actor.error)}`,
				);
			}
			if (actor?.start_ts) return;
		}
		await new Promise((resolve) => setTimeout(resolve, 250));
	}
	throw new Error(`timed out waiting for Services actor ${actorId}`);
}

async function upsertNormalRunnerConfig(
	child: ChildProcess,
	endpoint: string,
	poolName: string,
): Promise<void> {
	const datacentersResponse = await fetch(
		`${endpoint}/datacenters?namespace=${encodeURIComponent(NAMESPACE)}`,
		{
			headers: {
				Authorization: `Bearer ${TOKEN}`,
			},
		},
	);

	if (!datacentersResponse.ok) {
		throw new Error(
			`failed to list datacenters: ${datacentersResponse.status} ${await datacentersResponse.text()}\n${childOutput(child)}`,
		);
	}

	const datacentersBody = (await datacentersResponse.json()) as {
		datacenters: Array<{ name: string }>;
	};
	const datacenter = datacentersBody.datacenters[0]?.name;

	if (!datacenter) {
		throw new Error(
			`engine returned no datacenters\n${childOutput(child)}`,
		);
	}

	const response = await fetch(
		`${endpoint}/runner-configs/${encodeURIComponent(poolName)}?namespace=${encodeURIComponent(NAMESPACE)}`,
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
		return;
	}

	throw new Error(
		`failed to upsert runner config ${poolName}: ${response.status} ${await response.text()}\n${childOutput(child)}`,
	);
}

async function stopRuntime(child: ChildProcess): Promise<void> {
	if (child.exitCode !== null) {
		return;
	}

	child.kill("SIGINT");

	await new Promise<void>((resolve) => {
		const timeout = setTimeout(() => {
			if (child.exitCode === null) {
				child.kill("SIGKILL");
			}
		}, 10_000);

		child.once("exit", () => {
			clearTimeout(timeout);
			resolve();
		});
	});
}

async function stopTestEngine(): Promise<void> {
	if (!storagePath || !engineEndpoint) return;
	const stampPath = join(
		storagePath,
		".rivetkit",
		"var",
		"engine",
		"runtime.json",
	);
	try {
		const stamp = JSON.parse(await readFile(stampPath, "utf8")) as {
			pid: number;
			endpoint: string;
		};
		if (new URL(stamp.endpoint).href !== new URL(engineEndpoint).href) {
			throw new Error(
				`refusing to stop Engine for unexpected endpoint ${stamp.endpoint}`,
			);
		}
		process.kill(stamp.pid, "SIGTERM");
		const deadline = Date.now() + 5_000;
		while (Date.now() < deadline) {
			try {
				process.kill(stamp.pid, 0);
			} catch {
				return;
			}
			await new Promise((resolve) => setTimeout(resolve, 100));
		}
		process.kill(stamp.pid, "SIGKILL");
	} catch (error) {
		if (
			!(
				error instanceof Error &&
				"code" in error &&
				error.code === "ENOENT"
			)
		) {
			throw error;
		}
	}
}

describe.sequential("native NAPI runtime integration", () => {
	let runtime: ChildProcess | undefined;

	afterEach(async () => {
		if (runtime) {
			await stopRuntime(runtime);
			runtime = undefined;
		}
		await stopTestEngine();
		if (storagePath) {
			await rm(storagePath, { recursive: true, force: true });
			storagePath = undefined;
		}
		engineEndpoint = undefined;
	}, 30_000);

	test("runs a TS actor through registry, NAPI, core, envoy, and engine", async () => {
		const poolName = "default";
		const port = await getPort({ host: "127.0.0.1" });
		const endpoint = `http://127.0.0.1:${port}`;
		engineEndpoint = endpoint;
		storagePath = await mkdtemp(join(tmpdir(), "rivetkit-services-"));
		runtimeLogs = { stdout: "", stderr: "" };
		runtime = spawn(process.execPath, ["--import", "tsx", FIXTURE_PATH], {
			cwd: dirname(TEST_DIR),
			env: {
				...process.env,
				RIVET_TOKEN: TOKEN,
				RIVET_NAMESPACE: NAMESPACE,
				RIVET_RUN_ENGINE_HOST: "127.0.0.1",
				RIVET_RUN_ENGINE_PORT: String(port),
				RIVETKIT_TEST_ENDPOINT: endpoint,
				RIVETKIT_TEST_POOL_NAME: poolName,
				RIVETKIT_STORAGE_PATH: storagePath,
			},
			stdio: ["ignore", "pipe", "pipe"],
		});
		runtime.stdout?.on("data", (chunk) => {
			runtimeLogs.stdout += chunk.toString();
		});
		runtime.stderr?.on("data", (chunk) => {
			runtimeLogs.stderr += chunk.toString();
		});

		await waitForHealth(runtime, endpoint, 90_000);
		await upsertNormalRunnerConfig(runtime, endpoint, poolName);
		await waitForEnvoy(runtime, endpoint, poolName, 30_000);
		await waitForEnvoy(runtime, endpoint, SERVICES_POOL_NAME, 30_000);
		await expectNormalRunnerConfig(endpoint, SERVICES_POOL_NAME);
		const servicesActorId = await createServicesActor(endpoint);
		await waitForActorStarted(endpoint, servicesActorId, 30_000);

		const client = createClient<any>({
			endpoint,
			token: TOKEN,
			namespace: NAMESPACE,
			poolName,
			disableMetadataLookup: true,
		}) as any;

		const handle = await waitForActorReady(
			() =>
				client.integrationActor.create(
					[`napi-runtime-${crypto.randomUUID()}`],
					{
						params: { userId: "integration-test" },
					},
				),
			30_000,
		);
		const actorId = await handle.resolve();

		expect(await waitForActorReady(() => handle.getCount(), 30_000)).toBe(
			0,
		);
		expect(
			await waitForActorReady(
				() => handle.validatedAction({ amount: 4 }),
				30_000,
			),
		).toBe(4);
		await expect(
			waitForActorReady(
				() => handle.validatedAction({ amount: "bad" }),
				30_000,
			),
		).rejects.toMatchObject({
			group: "actor",
			code: "validation_error",
		});
		expect(
			await waitForActorReady(
				() => handle.emitValidatedEvent({ count: 2 }),
				30_000,
			),
		).toBe(2);
		await expect(
			waitForActorReady(
				() => handle.emitValidatedEvent({ count: "bad" }),
				30_000,
			),
		).rejects.toMatchObject({
			group: "actor",
			code: "validation_error",
		});
		expect(
			await waitForActorReady(
				() => handle.enqueueValidatedJob({ id: "job-1" }),
				30_000,
			),
		).toBe("job-1");
		await expect(
			waitForActorReady(
				() => handle.enqueueValidatedJob({ id: "" }),
				30_000,
			),
		).rejects.toMatchObject({
			group: "actor",
			code: "validation_error",
		});

		expect(
			await waitForActorReady(() => handle.increment(2), 30_000),
		).toEqual({
			count: 2,
			sqliteValues: [2],
		});
		expect(await handle.snapshot()).toEqual({
			count: 2,
			kvCount: 2,
			sqliteValues: [2],
		});

		expect(await handle.goToSleep()).toEqual({ ok: true });
		await waitForActorSleep(endpoint, actorId, 30_000);

		expect(
			await waitForActorReady(
				() => handle.incrementWithoutSql(3),
				30_000,
			),
		).toEqual({
			count: 5,
		});
		expect(await handle.getCountViaClient()).toBe(5);
		expect(await handle.stateSnapshot()).toEqual({
			count: 5,
			kvCount: 5,
		});
		await expect(handle.throwTypedError()).rejects.toMatchObject({
			group: "user",
			code: "boom",
			message: "native typed error",
			metadata: {
				source: "native",
			},
		});
		await expect(handle.throwUntypedError()).rejects.toMatchObject({
			group: "rivetkit",
			code: "internal_error",
			message: "An internal error occurred",
		});
		await client.dispose();

		const processId = servicesPid();
		await stopRuntime(runtime);
		runtime = undefined;
		await waitForProcessExit(processId, 5_000);
	}, 120_000);
});
