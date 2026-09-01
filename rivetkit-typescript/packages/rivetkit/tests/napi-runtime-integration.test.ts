import { type ChildProcess, spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import getPort from "get-port";
import { afterEach, describe, expect, test } from "vitest";
import { createClient } from "../src/client/mod";

const TEST_DIR = dirname(fileURLToPath(import.meta.url));
const FIXTURE_PATH = join(TEST_DIR, "fixtures", "napi-runtime-server.ts");
const NAMESPACE = "default";
const TOKEN = "dev";
let runtimeLogs = {
	stdout: "",
	stderr: "",
};

function childOutput(child: ChildProcess): string {
	void child;
	return [runtimeLogs.stdout, runtimeLogs.stderr].filter(Boolean).join("\n");
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
				`native runtime exited before health check passed:\n${childOutput(child)}`,
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
		`timed out waiting for native runtime health:\n${childOutput(child)}`,
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
		}, 5_000);

		child.once("exit", () => {
			clearTimeout(timeout);
			resolve();
		});
	});
}

describe.sequential("native NAPI runtime integration", () => {
	let runtime: ChildProcess | undefined;

	afterEach(async () => {
		if (runtime) {
			await stopRuntime(runtime);
			runtime = undefined;
		}
	}, 30_000);

	test("runs a TS actor through registry, NAPI, core, envoy, and engine", async () => {
		const poolName = "default";
		const port = await getPort({ host: "127.0.0.1" });
		const endpoint = `http://127.0.0.1:${port}`;
		runtimeLogs = { stdout: "", stderr: "" };
		runtime = spawn(process.execPath, ["--import", "tsx", FIXTURE_PATH], {
			cwd: dirname(TEST_DIR),
			env: {
				...process.env,
				RIVET_TOKEN: TOKEN,
				RIVET_NAMESPACE: NAMESPACE,
				RIVETKIT_TEST_ENDPOINT: endpoint,
				RIVETKIT_TEST_POOL_NAME: poolName,
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
					{ params: { userId: "native-integration-test" } },
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
	}, 120_000);
});
