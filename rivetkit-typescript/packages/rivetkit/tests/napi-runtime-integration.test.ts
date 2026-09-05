import { type ChildProcess, spawn } from "node:child_process";
import { mkdtemp, readdir, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import getPort from "get-port";
import { afterEach, describe, expect, test, vi } from "vitest";
import { createClient } from "../src/client/mod";
import {
	type OtlpCollector,
	startOtlpCollector,
} from "./fixtures/otlp-collector";

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

function runtimeOutput(): string {
	return [runtimeLogs.stdout, runtimeLogs.stderr].filter(Boolean).join("\n");
}

function childOutput(child: ChildProcess): string {
	void child;
	return runtimeOutput();
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

interface ExportedSpan {
	name: string;
	traceId: string;
	spanId: string;
	parentSpanId?: string;
	attributes: Record<string, string | undefined>;
	links: Array<{ traceId: string; spanId: string }>;
}

/** Flattens OTLP/JSON export bodies into the spans they carry. */
function exportedSpans(exports: Buffer[]): ExportedSpan[] {
	type OtlpAttribute = { key: string; value: { stringValue?: string } };
	type OtlpSpan = Omit<ExportedSpan, "attributes"> & {
		attributes?: OtlpAttribute[];
		links?: Array<{ traceId: string; spanId: string }>;
	};
	type OtlpPayload = {
		resourceSpans?: Array<{ scopeSpans?: Array<{ spans?: OtlpSpan[] }> }>;
	};
	return exports.flatMap((body) => {
		const payload = JSON.parse(body.toString("utf8")) as OtlpPayload;
		return (payload.resourceSpans ?? []).flatMap((resource) =>
			(resource.scopeSpans ?? []).flatMap((scope) =>
				(scope.spans ?? []).map((span) => ({
					name: span.name,
					traceId: span.traceId,
					spanId: span.spanId,
					parentSpanId: span.parentSpanId || undefined,
					attributes: Object.fromEntries(
						(span.attributes ?? []).map((attribute) => [
							attribute.key,
							attribute.value.stringValue,
						]),
					),
					links: (span.links ?? []).map((link) => ({
						traceId: link.traceId,
						spanId: link.spanId,
					})),
				})),
			),
		);
	});
}

/**
 * Polls until the exported spans satisfy `ready`, then returns them. Parent
 * and child spans can land in different export batches, so callers that
 * assert parentage must wait for both.
 */
async function waitForSpans(
	exports: Buffer[],
	description: string,
	ready: (spans: ExportedSpan[]) => boolean,
	timeoutMs: number,
): Promise<ExportedSpan[]> {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		const spans = exportedSpans(exports);
		if (ready(spans)) {
			return spans;
		}
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
	throw new Error(`timed out waiting for ${description}`);
}

function isSqliteSpan(span: ExportedSpan): boolean {
	return span.name === "rivet.sqlite.execute";
}

function isFailedSqliteSpan(span: ExportedSpan): boolean {
	return isSqliteSpan(span) && span.attributes["error.type"] !== undefined;
}

function findInvocation(
	spans: ExportedSpan[],
	actionName: string,
): ExportedSpan | undefined {
	return spans.find(
		(span) =>
			span.attributes["rivet.invocation.type"] !== undefined &&
			span.attributes["rivet.action.name"] === actionName,
	);
}

/** Polls until an invocation span has been exported for every named action. */
async function waitForInvocationSpans(
	exports: Buffer[],
	actionNames: string[],
	timeoutMs: number,
): Promise<ExportedSpan[]> {
	return waitForSpans(
		exports,
		`invocation spans: ${actionNames.join(", ")}`,
		(spans) => actionNames.every((name) => findInvocation(spans, name)),
		timeoutMs,
	);
}

async function waitForRuntimeLog(
	correlationToken: string,
	timeoutMs: number,
): Promise<string> {
	const deadline = Date.now() + timeoutMs;
	const marker = `correlation_token=${correlationToken}`;
	while (Date.now() < deadline) {
		const line = runtimeOutput()
			.split("\n")
			.find((candidate) => candidate.includes(marker));
		if (line) {
			return line;
		}
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
	throw new Error(`timed out waiting for runtime log ${correlationToken}`);
}

/**
 * Starts an engine and a native runtime pointed at one OTLP endpoint, and
 * returns the pieces every telemetry test needs.
 */
async function startTracedRuntime(
	tracesEndpoint: string,
	extraEnv: Record<string, string> = {},
): Promise<{ endpoint: string; poolName: string; child: ChildProcess }> {
	const poolName = "default";
	const port = await getPort({ host: "127.0.0.1" });
	const endpoint = `http://127.0.0.1:${port}`;
	engineEndpoint = endpoint;
	storagePath = await mkdtemp(join(tmpdir(), "rivetkit-services-"));
	runtimeLogs = { stdout: "", stderr: "" };
	const child = spawn(process.execPath, ["--import", "tsx", FIXTURE_PATH], {
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
			OTEL_EXPORTER_OTLP_TRACES_ENDPOINT: tracesEndpoint,
			OTEL_EXPORTER_OTLP_TRACES_PROTOCOL: "http/json",
			OTEL_TRACES_SAMPLER: "always_on",
			OTEL_BSP_SCHEDULE_DELAY: "10",
			...extraEnv,
		},
		stdio: ["ignore", "pipe", "pipe"],
	});
	child.stdout?.on("data", (chunk) => {
		runtimeLogs.stdout += chunk.toString();
	});
	child.stderr?.on("data", (chunk) => {
		runtimeLogs.stderr += chunk.toString();
	});
	await waitForHealth(child, endpoint, 90_000);
	await upsertNormalRunnerConfig(child, endpoint, poolName);
	await waitForEnvoy(child, endpoint, poolName, 30_000);
	return { endpoint, poolName, child };
}

describe.sequential("native NAPI runtime integration", () => {
	let runtime: ChildProcess | undefined;
	let collector: OtlpCollector | undefined;

	afterEach(async () => {
		if (runtime) {
			await stopRuntime(runtime);
			runtime = undefined;
		}
		if (collector) {
			await collector.close();
			collector = undefined;
		}
		await stopTestEngine();
		if (storagePath) {
			await rm(storagePath, { recursive: true, force: true });
			storagePath = undefined;
		}
		engineEndpoint = undefined;
	}, 30_000);

	test("runs a TS actor through registry, NAPI, core, envoy, and engine", async () => {
		collector = await startOtlpCollector(
			await getPort({ host: "127.0.0.1" }),
		);
		const traceExports = collector.spans();
		const { endpoint, poolName, child } = await startTracedRuntime(
			collector.endpoint,
		);
		runtime = child;
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

		const actorKey = `napi-runtime-${crypto.randomUUID()}`;
		const handle = await waitForActorReady(
			() =>
				client.integrationActor.create([actorKey], {
					params: { userId: "integration-test" },
				}),
			30_000,
		);
		const actorId = await handle.resolve();

		const correlationToken = crypto.randomUUID();
		expect(await handle.logContext(correlationToken)).toBe(
			correlationToken,
		);
		const actorLog = await waitForRuntimeLog(correlationToken, 10_000);
		expect(actorLog).toContain(`actorId=${actorId}`);
		expect(actorLog).toContain("actorName=integrationActor");
		expect(actorLog).toContain(actorKey);
		expect(actorLog).toMatch(/ rayId=[0-9a-f-]{36}( |$)/);
		expect(actorLog).toMatch(/ trace_id=[0-9a-f]{32}( |$)/);
		expect(actorLog).toMatch(/ span_id=[0-9a-f]{16}( |$)/);

		expect(await waitForActorReady(() => handle.getCount(), 30_000)).toBe(
			0,
		);
		const getCountSpans = await waitForInvocationSpans(
			traceExports,
			["getCount"],
			10_000,
		);
		expect(
			findInvocation(getCountSpans, "getCount")?.attributes,
		).toMatchObject({
			"rivet.invocation.type": "action",
			"rivet.actor.name": "integrationActor",
		});
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
		// SQLite spans are children of the action that issued them.
		const incrementSpans = await waitForSpans(
			traceExports,
			"increment invocation and sqlite spans",
			(spans) =>
				spans.some(isSqliteSpan) &&
				findInvocation(spans, "increment") !== undefined,
			10_000,
		);
		const incrementSqlite = incrementSpans.find(isSqliteSpan);
		expect(incrementSqlite?.attributes).toMatchObject({
			"rivet.operation.system": "sqlite",
			"rivet.operation.name": "execute",
		});
		expect(incrementSqlite?.parentSpanId).toBe(
			findInvocation(incrementSpans, "increment")?.spanId,
		);
		traceExports.length = 0;
		await expect(handle.sqliteFailure()).rejects.toMatchObject({
			code: expect.any(String),
		});
		// A failed statement records its error identity as group.code, never the message.
		const failureSpans = await waitForSpans(
			traceExports,
			"sqliteFailure invocation and failed sqlite spans",
			(spans) =>
				spans.some(isFailedSqliteSpan) &&
				findInvocation(spans, "sqliteFailure") !== undefined,
			10_000,
		);
		const failedSqlite = failureSpans.find(isFailedSqliteSpan);
		expect(failedSqlite?.attributes["error.type"]).toMatch(
			/^[a-z_]+\.[a-z_]+$/,
		);
		expect(failedSqlite?.parentSpanId).toBe(
			findInvocation(failureSpans, "sqliteFailure")?.spanId,
		);
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
		// An actor-owned client carries the calling invocation's trace and ray
		// across the real Engine boundary, so the callee is its child.
		traceExports.length = 0;
		expect(await handle.getCountViaClient()).toBe(5);
		const clientSpans = await waitForInvocationSpans(
			traceExports,
			["getCountViaClient", "getCount"],
			10_000,
		);
		const caller = findInvocation(clientSpans, "getCountViaClient");
		const callee = findInvocation(clientSpans, "getCount");
		expect(callee?.traceId).toBe(caller?.traceId);
		expect(callee?.parentSpanId).toBe(caller?.spanId);
		expect(callee?.attributes["rivet.ray.id"]).toBe(
			caller?.attributes["rivet.ray.id"],
		);
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

		// A scheduled fire keeps the defining invocation's ray and starts a
		// fresh trace linked to the defining span.
		traceExports.length = 0;
		const scheduleToken = crypto.randomUUID();
		expect(await handle.scheduleTrace(scheduleToken)).toBe(scheduleToken);
		const scheduleSpans = await waitForInvocationSpans(
			traceExports,
			["scheduleTrace", "scheduledTrace"],
			15_000,
		);
		const definer = findInvocation(scheduleSpans, "scheduleTrace");
		const scheduled = findInvocation(scheduleSpans, "scheduledTrace");
		expect(definer).toBeDefined();
		expect(scheduled?.attributes["rivet.invocation.type"]).toBe(
			"scheduled",
		);
		expect(scheduled?.attributes["rivet.ray.id"]).toBe(
			definer?.attributes["rivet.ray.id"],
		);
		// Without this the assertions below hold for a scheduled fire that threw,
		// so a broken action body would still pass.
		expect(scheduled?.attributes["error.type"]).toBeUndefined();
		expect(scheduled?.traceId).not.toBe(definer?.traceId);
		expect(scheduled?.links).toEqual([
			{ traceId: definer?.traceId, spanId: definer?.spanId },
		]);
		await client.dispose();

		const processId = servicesPid();
		await stopRuntime(runtime);
		runtime = undefined;
		await waitForProcessExit(processId, 5_000);
	}, 120_000);

	test("keeps overlapping invocations of one actor telemetrically isolated", async () => {
		collector = await startOtlpCollector(
			await getPort({ host: "127.0.0.1" }),
		);
		const traceExports = collector.spans();
		const { endpoint, poolName, child } = await startTracedRuntime(
			collector.endpoint,
		);
		runtime = child;

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
					[`napi-isolation-${crypto.randomUUID()}`],
					{ params: { userId: "integration-test" } },
				),
			30_000,
		);
		await waitForActorReady(() => handle.getCount(), 30_000);

		// Both calls run against the same actor, so `sameActorInstance` is true
		// for both and AsyncLocalStorage is the only thing keeping them apart.
		const okToken = crypto.randomUUID();
		const failToken = crypto.randomUUID();
		const [ok, failed] = await Promise.allSettled([
			handle.isolationProbe(okToken, false),
			handle.isolationProbe(failToken, true),
		]);
		expect(ok.status).toBe("fulfilled");
		expect(failed.status).toBe("rejected");

		const spans = await waitForSpans(
			traceExports,
			"both isolation probe invocations and the calls each one made",
			(exported) => {
				const probes = exported.filter(
					(span) =>
						span.attributes["rivet.action.name"] ===
						"isolationProbe",
				);
				return (
					probes.length >= 2 &&
					probes.every((probe) =>
						exported.some(
							(span) =>
								span.attributes["rivet.action.name"] ===
									"getCount" &&
								span.traceId === probe.traceId,
						),
					)
				);
			},
			20_000,
		);

		const probes = spans.filter(
			(span) => span.attributes["rivet.action.name"] === "isolationProbe",
		);
		expect(probes).toHaveLength(2);

		// Each invocation owns a distinct ray and trace, and the failing one
		// must not mark the invocation running beside it.
		const rays = probes.map((probe) => probe.attributes["rivet.ray.id"]);
		expect(new Set(rays).size).toBe(2);
		expect(new Set(probes.map((probe) => probe.traceId)).size).toBe(2);
		const failedProbes = probes.filter(
			(probe) => probe.attributes["error.type"] !== undefined,
		);
		expect(failedProbes).toHaveLength(1);
		expect(failedProbes[0]?.attributes["error.type"]).toBe(
			"user.isolation_probe_failed",
		);

		// Every SQLite span belongs to exactly one probe and carries that
		// probe's ray, not the ray of the invocation running beside it.
		for (const probe of probes) {
			const owned = spans.filter(
				(span) =>
					isSqliteSpan(span) && span.parentSpanId === probe.spanId,
			);
			expect(owned.length).toBeGreaterThanOrEqual(2);
			for (const span of owned) {
				expect(span.traceId).toBe(probe.traceId);
				expect(span.attributes["rivet.ray.id"]).toBe(
					probe.attributes["rivet.ray.id"],
				);
			}
		}

		// The outbound call each probe makes while the other is mid-flight
		// stays inside its own trace and carries its own ray.
		for (const probe of probes) {
			const callee = spans.find(
				(span) =>
					span.attributes["rivet.action.name"] === "getCount" &&
					span.traceId === probe.traceId,
			);
			expect(callee).toBeDefined();
			expect(callee?.parentSpanId).toBe(probe.spanId);
			expect(callee?.attributes["rivet.ray.id"]).toBe(
				probe.attributes["rivet.ray.id"],
			);
		}

		// Logs written from inside each invocation carry that invocation's ray.
		const okLog = await waitForRuntimeLog(okToken, 10_000);
		const failLog = await waitForRuntimeLog(failToken, 10_000);
		const rayOf = (line: string) => / rayId=([0-9a-f-]{36})/.exec(line)?.[1];
		expect(rayOf(okLog)).toBeDefined();
		expect(rayOf(okLog)).not.toBe(rayOf(failLog));
		expect(rays).toContain(rayOf(okLog));
		expect(rays).toContain(rayOf(failLog));

		await client.dispose();
	}, 120_000);

	test("keeps actor behavior intact when the trace exporter is unavailable", async () => {
		// Nothing listens on this port, so every OTLP export attempt fails.
		const unavailable = `http://127.0.0.1:${await getPort({ host: "127.0.0.1" })}/v1/traces`;
		const { endpoint, poolName, child } =
			await startTracedRuntime(unavailable);
		runtime = child;

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
					[`napi-telemetry-failure-${crypto.randomUUID()}`],
					{ params: { userId: "integration-test" } },
				),
			30_000,
		);

		expect(await waitForActorReady(() => handle.getCount(), 30_000)).toBe(
			0,
		);
		expect(
			await waitForActorReady(
				() => handle.validatedAction({ amount: 4 }),
				30_000,
			),
		).toBe(4);

		await client.dispose();
	}, 120_000);

	test("keeps actor behavior intact when the trace exporter is slow", async () => {
		// The collector accepts every export and then stalls for longer than the
		// whole test, so the exporter's queue fills instead of failing fast.
		collector = await startOtlpCollector(
			await getPort({ host: "127.0.0.1" }),
			{
				responseDelayMs: 120_000,
			},
		);
		// A queue this small saturates within a handful of actions, so the test
		// reaches the drop path rather than merely filling a buffer.
		const { endpoint, poolName, child } = await startTracedRuntime(
			collector.endpoint,
			{
				OTEL_BSP_MAX_QUEUE_SIZE: "8",
				OTEL_BSP_MAX_EXPORT_BATCH_SIZE: "4",
				// Rust's own log layers admit `opentelemetry_sdk` at warn by
				// default, so they print this same message and the assertion
				// below would pass with the bridge dead. Silencing that one
				// target leaves the JS sink as the only way it reaches stdout.
				RUST_LOG: "warn,opentelemetry_sdk=off",
			},
		);
		runtime = child;

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
					[`napi-telemetry-slow-${crypto.randomUUID()}`],
					{ params: { userId: "integration-test" } },
				),
			30_000,
		);

		// Each increment emits an invocation span plus several SQLite spans, so
		// this run produces far more spans than the queue can hold while the
		// collector is stalled.
		const started = Date.now();
		for (let index = 1; index <= 12; index += 1) {
			expect(
				await waitForActorReady(() => handle.increment(1), 30_000),
			).toMatchObject({ count: index });
		}
		const elapsed = Date.now() - started;

		// A blocking exporter would stall each action behind the collector's
		// 120s delay. Twelve actions finishing well inside one delay window is
		// what proves the queue drops instead of applying backpressure.
		expect(elapsed).toBeLessThan(60_000);

		// The SDK's dropped-span warning is bridged into the actor logger, so
		// it appears in the runtime's own log output rather than only in Rust.
		await vi.waitFor(
			() => {
				expect(runtimeOutput()).toContain(
					"BatchSpanProcessor.SpanDroppingStarted",
				);
			},
			{ timeout: 15_000, interval: 250 },
		);

		expect(await waitForActorReady(() => handle.getCount(), 30_000)).toBe(
			12,
		);

		await client.dispose();
	}, 180_000);
});
