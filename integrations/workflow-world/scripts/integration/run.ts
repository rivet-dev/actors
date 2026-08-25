import { spawn, type ChildProcess } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { getEnginePath } from "@rivetkit/engine-cli";
import { SPEC_VERSION_CURRENT } from "@workflow/world";
import { createClient, type Client } from "rivetkit/client";
import { registry } from "../../src/actors.ts";
import { RivetClientWorld } from "../../src/index.ts";

const TOKEN = "dev";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const RUNTIME_PATH = join(ROOT, "src/runtime.ts");

type ManagedProcess = {
	child: ChildProcess;
	output: () => string;
};

type EngineProcess = ManagedProcess & {
	endpoint: string;
	dbRoot: string;
};

type Delivery = {
	route: string;
	body: unknown;
	headers: Headers;
};

type MockWorkflowBehavior = {
	failuresBeforeSuccess: Map<string, number>;
	alwaysFail: Set<string>;
	/** runId -> ms to hold the response open before replying (long-step). */
	holdMs: Map<string, number>;
};

function log(message: string) {
	console.log(`[integration] ${message}`);
}

function assert(condition: unknown, message: string): asserts condition {
	if (!condition) throw new Error(message);
}

function sleep(ms: number) {
	return new Promise((resolvePromise) => setTimeout(resolvePromise, ms));
}

async function waitFor(
	label: string,
	predicate: () => Promise<boolean> | boolean,
	timeoutMs: number,
	intervalMs = 250,
) {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		if (await predicate()) return;
		await sleep(intervalMs);
	}
	throw new Error(`Timed out waiting for ${label}`);
}

function freePort(host = "127.0.0.1"): Promise<number> {
	return new Promise((resolvePromise, reject) => {
		const server = createServer();
		server.listen(0, host, () => {
			const address = server.address();
			if (!address || typeof address === "string") {
				server.close(() => reject(new Error("Could not allocate port")));
				return;
			}
			server.close(() => resolvePromise(address.port));
		});
		server.on("error", reject);
	});
}

function capture(child: ChildProcess): () => string {
	let stdout = "";
	let stderr = "";
	child.stdout?.on("data", (chunk) => {
		stdout += chunk.toString();
	});
	child.stderr?.on("data", (chunk) => {
		stderr += chunk.toString();
	});
	return () => `${stdout}\n${stderr}`;
}

async function stopProcess(child: ChildProcess, name: string) {
	if (child.exitCode !== null || child.signalCode !== null) return;
	child.kill("SIGTERM");
	const stopped = await new Promise<boolean>((resolvePromise) => {
		const timeout = setTimeout(() => resolvePromise(false), 1500);
		child.once("exit", () => {
			clearTimeout(timeout);
			resolvePromise(true);
		});
	});
	if (!stopped) {
		child.kill("SIGKILL");
		await new Promise<void>((resolvePromise) =>
			child.once("exit", () => resolvePromise()),
		);
	}
	log(`stopped ${name}`);
}

async function startEngine(): Promise<EngineProcess> {
	const host = "127.0.0.1";
	const guardPort = await freePort(host);
	const apiPeerPort = await freePort(host);
	const metricsPort = await freePort(host);
	const endpoint = `http://${host}:${guardPort}`;
	const dbRoot = mkdtempSync(join(tmpdir(), "workflow-world-rivet-it-"));
	const configPath = join(dbRoot, "config.json");

	mkdirSync(join(dbRoot, "db"), { recursive: true });
	writeFileSync(
		configPath,
		JSON.stringify({
			topology: {
				datacenter_label: 1,
				datacenters: {
					default: {
						datacenter_label: 1,
						is_leader: true,
						public_url: endpoint,
						peer_url: `http://${host}:${apiPeerPort}`,
					},
				},
			},
		}),
	);

	const child = spawn(getEnginePath(), ["start", "--config", configPath], {
		env: {
			...process.env,
			RIVET__GUARD__HOST: host,
			RIVET__GUARD__PORT: String(guardPort),
			RIVET__API_PEER__HOST: host,
			RIVET__API_PEER__PORT: String(apiPeerPort),
			RIVET__METRICS__HOST: host,
			RIVET__METRICS__PORT: String(metricsPort),
			RIVET__FILE_SYSTEM__PATH: join(dbRoot, "db"),
		},
		stdio: ["ignore", "pipe", "pipe"],
	});
	const output = capture(child);

	await waitFor(
		"engine health",
		async () => {
			if (child.exitCode !== null) {
				throw new Error(`Engine exited early:\n${output()}`);
			}
			try {
				return (await fetch(`${endpoint}/health`)).ok;
			} catch {
				return false;
			}
		},
		90_000,
		500,
	);

	log(`engine ready at ${endpoint}`);
	return { child, endpoint, dbRoot, output };
}

async function createNamespace(endpoint: string, namespace: string) {
	const response = await fetch(`${endpoint}/namespaces`, {
		method: "POST",
		headers: {
			Authorization: `Bearer ${TOKEN}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({
			name: namespace,
			display_name: `Integration ${namespace}`,
		}),
	});
	if (!response.ok) {
		throw new Error(
			`Failed to create namespace: ${response.status} ${await response.text()}`,
		);
	}
}

async function upsertRunnerConfig(
	endpoint: string,
	namespace: string,
	poolName: string,
) {
	const datacentersResponse = await fetch(
		`${endpoint}/datacenters?namespace=${encodeURIComponent(namespace)}`,
		{ headers: { Authorization: `Bearer ${TOKEN}` } },
	);
	if (!datacentersResponse.ok) {
		throw new Error(
			`Failed to list datacenters: ${datacentersResponse.status} ${await datacentersResponse.text()}`,
		);
	}
	const datacenters = (await datacentersResponse.json()) as {
		datacenters: Array<{ name: string }>;
	};
	const datacenter = datacenters.datacenters[0]?.name;
	if (!datacenter) throw new Error("Engine returned no datacenters");

	const deadline = Date.now() + 30_000;
	while (Date.now() < deadline) {
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
						[datacenter]: { normal: {} },
					},
				}),
			},
		);
		if (response.ok) return;
		const body = await response.text();
		if (body.includes('"code":"not_found"')) {
			await sleep(500);
			continue;
		}
		throw new Error(
			`Failed to upsert runner config: ${response.status} ${body}`,
		);
	}
	throw new Error("Timed out upserting runner config");
}

function startRuntime(
	endpoint: string,
	namespace: string,
	poolName: string,
): ManagedProcess {
	const child = spawn(process.execPath, ["--import", "tsx", RUNTIME_PATH], {
		cwd: ROOT,
		env: {
			...process.env,
			RIVET_TOKEN: TOKEN,
			RIVET_NAMESPACE: namespace,
			RIVET_ENDPOINT: endpoint,
			RIVET_POOL: poolName,
			RIVET_WORLD_RIVET_MAX_DISPATCH_ATTEMPTS: "3",
			RIVET_WORLD_RIVET_RETRY_DELAY_MS: "200",
			RIVET_WORLD_RIVET_STALE_INFLIGHT_MS: "500",
			RIVET_WORLD_RIVET_TESTING: "1",
		},
		stdio: ["ignore", "pipe", "pipe"],
	});
	return { child, output: capture(child) };
}

async function waitForEnvoy(
	runtime: ManagedProcess,
	endpoint: string,
	namespace: string,
	poolName: string,
) {
	await waitFor(
		"runtime envoy registration",
		async () => {
			if (runtime.child.exitCode !== null) {
				throw new Error(`Runtime exited early:\n${runtime.output()}`);
			}
			const response = await fetch(
				`${endpoint}/envoys?namespace=${encodeURIComponent(namespace)}&name=${encodeURIComponent(poolName)}`,
				{ headers: { Authorization: `Bearer ${TOKEN}` } },
			);
			if (!response.ok) return false;
			const body = (await response.json()) as {
				envoys: Array<{ envoy_key: string }>;
			};
			return body.envoys.length > 0;
		},
		30_000,
		500,
	);
}

async function startMockWorkflowApp() {
	const deliveries: Delivery[] = [];
	const healthChecks = { flow: 0 };
	const behavior: MockWorkflowBehavior = {
		failuresBeforeSuccess: new Map(),
		alwaysFail: new Set(),
		holdMs: new Map(),
	};
	const server = createServer(async (req, res) => {
		const path = req.url ?? "";
		if (
			req.method === "GET" &&
			path === "/.well-known/workflow/v1/flow?__health"
		) {
			healthChecks.flow++;
			res.writeHead(200, { "content-type": "application/json" });
			res.end(JSON.stringify({ healthy: true }));
			return;
		}
		if (
			req.method !== "POST" ||
			!path.startsWith("/.well-known/workflow/v1/")
		) {
			res.writeHead(404);
			res.end("not found");
			return;
		}

		const chunks: Buffer[] = [];
		for await (const chunk of req) {
			chunks.push(Buffer.from(chunk));
		}
		const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
		deliveries.push({
			route: path.split("/").at(-1) ?? "",
			body,
			headers: new Headers(req.headers as Record<string, string>),
		});
		const runId =
			typeof body === "object" && body !== null && "runId" in body
				? String((body as { runId: unknown }).runId)
				: null;
		if (runId && behavior.alwaysFail.has(runId)) {
			res.writeHead(503, { "content-type": "application/json" });
			res.end(JSON.stringify({ error: "forced failure" }));
			return;
		}
		if (runId) {
			const remaining = behavior.failuresBeforeSuccess.get(runId) ?? 0;
			if (remaining > 0) {
				behavior.failuresBeforeSuccess.set(runId, remaining - 1);
				res.writeHead(503, { "content-type": "application/json" });
				res.end(JSON.stringify({ error: "forced retry" }));
				return;
			}
		}
		// Long-step: hold the flow response open like a real >60s step body would,
		// so we exercise the real dispatcher's c.keepAwake fetch hold.
		const hold = runId ? (behavior.holdMs.get(runId) ?? 0) : 0;
		const reply = () => {
			res.writeHead(200, { "content-type": "application/json" });
			res.end(JSON.stringify({ ok: true }));
		};
		if (hold > 0) setTimeout(reply, hold);
		else reply();
	});

	const port = await freePort();
	await new Promise<void>((resolvePromise) =>
		server.listen(port, "127.0.0.1", () => resolvePromise()),
	);

	return {
		url: `http://127.0.0.1:${port}`,
		deliveries,
		healthChecks,
		behavior,
		close: () =>
			new Promise<void>((resolvePromise, reject) =>
				(server as Server).close((error) =>
					error ? reject(error) : resolvePromise(),
				),
			),
	};
}

function deliveryCount(
	deliveries: Delivery[],
	predicate: (delivery: Delivery) => boolean,
) {
	return deliveries.filter(predicate).length;
}

async function readWithTimeout<T>(
	promise: Promise<ReadableStreamReadResult<T>>,
	label: string,
) {
	const timeout = sleep(10_000).then(() => {
		throw new Error(`Timed out waiting for ${label}`);
	});
	return await Promise.race([promise, timeout]);
}

async function runChecks(
	world: RivetClientWorld,
	worldConfig: ConstructorParameters<typeof RivetClientWorld>[0],
	deliveries: Delivery[],
	healthChecks: { flow: number },
	behavior: MockWorkflowBehavior,
) {
	const previousSecret = process.env.RIVET_WORKFLOW_SECRET;
	process.env.RIVET_WORKFLOW_SECRET = "integration-secret";
	let handledAuthMessage = false;
	try {
		const authHandler = world.createQueueHandler(
			"__wkf_workflow_",
			async () => {
				handledAuthMessage = true;
			},
		);
		const unauthenticated = await authHandler(
			new Request("http://127.0.0.1/.well-known/workflow/v1/flow", {
				method: "POST",
				headers: {
					"content-type": "application/json",
					"x-vqs-queue-name": "__wkf_workflow_authProbe",
					"x-vqs-message-id": "msg_auth_probe",
					"x-vqs-message-attempt": "1",
				},
				body: JSON.stringify({ runId: "wrun_auth_probe" }),
			}),
		);
		assert(
			unauthenticated.status === 401,
			"Expected queue handler to reject missing bearer token",
		);
		assert(
			!handledAuthMessage,
			"Expected unauthorized queue request to skip handler",
		);
		const authenticated = await authHandler(
			new Request("http://127.0.0.1/.well-known/workflow/v1/flow", {
				method: "POST",
				headers: {
					authorization: "Bearer integration-secret",
					"content-type": "application/json",
					"x-vqs-queue-name": "__wkf_workflow_authProbe",
					"x-vqs-message-id": "msg_auth_probe",
					"x-vqs-message-attempt": "1",
				},
				body: JSON.stringify({ runId: "wrun_auth_probe" }),
			}),
		);
		assert(authenticated.ok, "Expected valid bearer token to be accepted");
		assert(handledAuthMessage, "Expected authorized queue request to run handler");
		log("queue handler auth check passed");
	} finally {
		if (previousSecret === undefined) {
			delete process.env.RIVET_WORKFLOW_SECRET;
		} else {
			process.env.RIVET_WORKFLOW_SECRET = previousSecret;
		}
	}

	const streamName = `stream-${randomUUID()}`;
	const streamRunId = `wrun_stream_${randomUUID()}`;
	const liveStream = await world.readFromStream(streamRunId, streamName, 0);
	const reader = liveStream.getReader();
	const firstRead = readWithTimeout(reader.read(), "live stream chunk");
	await world.writeToStream(streamName, streamRunId, "hello-live-stream");
	const firstChunk = await firstRead;
	assert(!firstChunk.done, "Expected live stream to yield written chunk");
	assert(
		new TextDecoder().decode(firstChunk.value) === "hello-live-stream",
		"Expected live stream chunk contents to match",
	);
	const doneRead = readWithTimeout(reader.read(), "live stream close");
	await world.closeStream(streamName, streamRunId);
	const done = await doneRead;
	assert(done.done, "Expected live stream to close after closeStream");
	log("event-driven stream check passed");

	const ttlStream = `stream-ttl-${randomUUID()}`;
	const ttlRunId = `wrun_stream_ttl_${randomUUID()}`;
	await world.writeToStream(ttlStream, ttlRunId, "expires");
	await world.closeStream(ttlStream, ttlRunId);
	const expired = await world.expireClosedStreamForTesting(ttlRunId, ttlStream);
	assert(
		expired.deletedChunks >= 2,
		"Expected closed stream TTL cleanup to delete data and EOF chunks",
	);
	log("stream TTL cleanup check passed");

	const idempotencyRunId = `wrun_it_idem_${randomUUID()}`;
	const idempotencyKey = `step-${randomUUID()}`;
	const first = await world.queue(
		"__wkf_workflow_idempotencyProbe",
		{ runId: idempotencyRunId },
		{ idempotencyKey },
	);
	const second = await world.queue(
		"__wkf_workflow_idempotencyProbe",
		{ runId: idempotencyRunId },
		{ idempotencyKey },
	);
	assert(
		first.messageId === second.messageId,
		"Expected duplicate idempotencyKey enqueue to return existing message id",
	);
	await waitFor(
		"idempotent queue delivery",
		() =>
			deliveryCount(
				deliveries,
				(delivery) => (delivery.body as { runId?: string }).runId === idempotencyRunId,
			) === 1,
		30_000,
	);
	await sleep(1_000);
	assert(
		deliveryCount(
			deliveries,
			(delivery) => (delivery.body as { runId?: string }).runId === idempotencyRunId,
		) === 1,
		"Expected duplicate idempotencyKey enqueue to dispatch only once",
	);
	log("queue idempotency check passed");

	const retryRunId = `wrun_it_retry_${randomUUID()}`;
	behavior.failuresBeforeSuccess.set(retryRunId, 2);
	await world.queue("__wkf_workflow_retryProbe", { runId: retryRunId });
	await waitFor(
		"retry queue delivery",
		() =>
			deliveryCount(
				deliveries,
				(delivery) => (delivery.body as { runId?: string }).runId === retryRunId,
			) === 3,
		30_000,
	);
	const retrySnapshot = await world.inspectDispatcherQueue(retryRunId);
	const retriedRow = retrySnapshot.rows.find((row) => row.status === "done");
	assert(retriedRow, "Expected retried dispatch row to finish");
	assert(retriedRow.attempt === 3, "Expected dispatch to succeed on attempt 3");
	log("queue retry check passed");

	const failedRunId = `wrun_it_failed_${randomUUID()}`;
	behavior.alwaysFail.add(failedRunId);
	await world.queue("__wkf_workflow_deadLetterProbe", { runId: failedRunId });
	await waitFor(
		"dead-letter queue status",
		async () => {
			const snapshot = await world.inspectDispatcherQueue(failedRunId);
			return snapshot.rows.some(
				(row) => row.status === "failed" && row.attempt === 3,
			);
		},
		30_000,
	);
	assert(
		deliveryCount(
			deliveries,
			(delivery) => (delivery.body as { runId?: string }).runId === failedRunId,
		) === 3,
		"Expected dead-letter dispatch to stop at configured max attempts",
	);
	log("queue dead-letter check passed");

	const staleRunId = `wrun_it_stale_${randomUUID()}`;
	const staleQueued = await world.queue(
		"__wkf_workflow_staleProbe",
		{ runId: staleRunId },
		{ delaySeconds: 60 },
	);
	await world.forceStaleDispatcherMessageForTesting(
		staleRunId,
		staleQueued.messageId,
		1_000,
	);
	await waitFor(
		"stale inflight recovery delivery",
		() =>
			deliveryCount(
				deliveries,
				(delivery) => (delivery.body as { runId?: string }).runId === staleRunId,
			) === 1,
		30_000,
	);
	const staleSnapshot = await world.inspectDispatcherQueue(staleRunId);
	const staleRow = staleSnapshot.rows.find(
		(row) => row.messageId === staleQueued.messageId,
	);
	assert(staleRow?.status === "done", "Expected stale dispatch to recover");
	assert(staleRow.attempt === 1, "Expected recovered stale dispatch to run once");
	log("stale queue recovery check passed");

	const created = await world.events.create(null, {
		eventType: "run_created",
		specVersion: SPEC_VERSION_CURRENT,
		eventData: {
			deploymentId: "rivet",
			workflowName: "recoveryProbe",
			input: { source: "integration" },
		},
	});
	const runId = created.run?.runId;
	assert(runId, "Expected run_created to return a run id");

	await world.start();
	await waitFor(
		"per-run recovery delivery",
		() =>
			deliveryCount(
				deliveries,
				(delivery) => (delivery.body as { runId?: string }).runId === runId,
			) === 1,
		30_000,
	);
	await world.start();
	await sleep(1_000);
	assert(
		deliveryCount(
			deliveries,
			(delivery) => (delivery.body as { runId?: string }).runId === runId,
		) === 1,
		"Expected process-idempotent start() not to duplicate delivery",
	);
	log("world.start idempotency check passed");

	const stepLookupRun = await world.events.create(null, {
		eventType: "run_created",
		specVersion: SPEC_VERSION_CURRENT,
		eventData: {
			deploymentId: "rivet",
			workflowName: "globalStepLookupProbe",
			input: { source: "integration" },
		},
	});
	const stepLookupRunId = stepLookupRun.run?.runId;
	assert(stepLookupRunId, "Expected step lookup run_created to return a run id");
	const stepId = `step-${randomUUID()}`;
	await world.events.create(stepLookupRunId, {
		eventType: "step_created",
		specVersion: SPEC_VERSION_CURRENT,
		correlationId: stepId,
		eventData: {
			stepName: "global-step",
			input: { source: "integration" },
		},
	});
	const globalStep = await world.steps.get(undefined, stepId, {
		resolveData: "none",
	});
	assert(
		globalStep.stepId === stepId && globalStep.runId === stepLookupRunId,
		"Expected steps.get without runId to resolve via correlation index",
	);
	log("global step lookup check passed");

	const waitCreated = await world.events.create(null, {
		eventType: "run_created",
		specVersion: SPEC_VERSION_CURRENT,
		eventData: {
			deploymentId: "rivet",
			workflowName: "waitWakeProbe",
			input: { source: "integration" },
		},
	});
	const waitRunId = waitCreated.run?.runId;
	assert(waitRunId, "Expected wait probe run_created to return a run id");
	await world.events.create(waitRunId, {
		eventType: "wait_created",
		specVersion: SPEC_VERSION_CURRENT,
		correlationId: `wait-${randomUUID()}`,
		eventData: {
			resumeAt: new Date(Date.now() + 750),
		},
	});
	await waitFor(
		"scheduled wait wake delivery",
		() =>
			deliveryCount(
				deliveries,
				(delivery) => (delivery.body as { runId?: string }).runId === waitRunId,
			) === 1,
		30_000,
	);
	log("workflowRun wait wake check passed");

	const token = `token-${randomUUID()}`;
	const firstHookRun = await world.events.create(null, {
		eventType: "run_created",
		specVersion: SPEC_VERSION_CURRENT,
		eventData: {
			deploymentId: "rivet",
			workflowName: "hookConflictProbe",
			input: { owner: 1 },
		},
	});
	const firstHookRunId = firstHookRun.run?.runId;
	assert(firstHookRunId, "Expected first hook run to return a run id");
	const firstHook = await world.events.create(firstHookRunId, {
		eventType: "hook_created",
		specVersion: SPEC_VERSION_CURRENT,
		correlationId: `hook-${randomUUID()}`,
		eventData: { token, metadata: { owner: 1 } },
	});
	assert(firstHook.hook?.token === token, "Expected first hook token claim");
	const hookById = await world.hooks.get(firstHook.hook.hookId, {
		resolveData: "none",
	});
	assert(
		hookById.hookId === firstHook.hook.hookId &&
			hookById.runId === firstHookRunId,
		"Expected hooks.get(hookId) to route through coordinator hook index",
	);
	const globalHooks = await world.hooks.list({
		resolveData: "none",
		pagination: { limit: 100 },
	});
	assert(
		globalHooks.data.some(
			(hook) =>
				hook.hookId === firstHook.hook?.hookId && hook.runId === firstHookRunId,
		),
		"Expected hooks.list without runId to page through coordinator hook index",
	);

	const secondHookRun = await world.events.create(null, {
		eventType: "run_created",
		specVersion: SPEC_VERSION_CURRENT,
		eventData: {
			deploymentId: "rivet",
			workflowName: "hookConflictProbe",
			input: { owner: 2 },
		},
	});
	const secondHookRunId = secondHookRun.run?.runId;
	assert(secondHookRunId, "Expected second hook run to return a run id");
	const conflict = await world.events.create(secondHookRunId, {
		eventType: "hook_created",
		specVersion: SPEC_VERSION_CURRENT,
		correlationId: `hook-${randomUUID()}`,
		eventData: { token, metadata: { owner: 2 } },
	});
	assert(
		conflict.event?.eventType === "hook_conflict",
		"Expected duplicate hook token to create hook_conflict event",
	);
	assert(
		conflict.event.eventData?.conflictingRunId === firstHookRunId,
		"Expected hook_conflict to include conflictingRunId",
	);

	await world.events.create(firstHookRunId, {
		eventType: "run_completed",
		specVersion: SPEC_VERSION_CURRENT,
		eventData: { output: { ok: true } },
	});
	const thirdHookRun = await world.events.create(null, {
		eventType: "run_created",
		specVersion: SPEC_VERSION_CURRENT,
		eventData: {
			deploymentId: "rivet",
			workflowName: "hookConflictProbe",
			input: { owner: 3 },
		},
	});
	const thirdHookRunId = thirdHookRun.run?.runId;
	assert(thirdHookRunId, "Expected third hook run to return a run id");
	const reclaimed = await world.events.create(thirdHookRunId, {
		eventType: "hook_created",
		specVersion: SPEC_VERSION_CURRENT,
		correlationId: `hook-${randomUUID()}`,
		eventData: { token, metadata: { owner: 3 } },
	});
	assert(
		reclaimed.hook?.runId === thirdHookRunId,
		"Expected terminal hook release to allow token reuse",
	);
	log("hook lookup/conflict/release check passed");

	// Real long-step E2E: the app holds the flow response open for >60s, like a
	// real long-running step body. The dispatcher must hold the outbound fetch
	// (c.keepAwake) without a premature client-action timeout or stale-inflight
	// redelivery, then complete the message exactly once.
	const longRunId = `wrun_it_longstep_${randomUUID()}`;
	const HOLD_MS = 62_000;
	behavior.holdMs.set(longRunId, HOLD_MS);
	const longQueued = await world.queue("__wkf_workflow_longStep", {
		runId: longRunId,
	});
	const longStart = Date.now();
	await waitFor(
		"long-step single delivery",
		() =>
			deliveryCount(
				deliveries,
				(d) => (d.body as { runId?: string }).runId === longRunId,
			) >= 1,
		20_000,
	);
	// During the hold, the row must stay in-flight with NO premature retry.
	await sleep(2_000);
	const midSnapshot = await world.inspectDispatcherQueue(longRunId);
	const midRow = midSnapshot.rows.find(
		(r) => r.messageId === longQueued.messageId,
	);
	assert(midRow?.status === "inflight", "Expected long step to stay inflight");
	assert(midRow.attempt === 1, "Expected no premature long-step retry");
	await waitFor(
		"long-step completion",
		async () => {
			const snap = await world.inspectDispatcherQueue(longRunId);
			return snap.rows.some(
				(r) => r.messageId === longQueued.messageId && r.status === "done",
			);
		},
		HOLD_MS + 20_000,
	);
	const heldMs = Date.now() - longStart;
	assert(heldMs >= 60_000, `Expected the step to hold >60s, held ${heldMs}ms`);
	assert(
		deliveryCount(
			deliveries,
			(d) => (d.body as { runId?: string }).runId === longRunId,
		) === 1,
		"Expected the long step to be delivered exactly once (no premature retry)",
	);
	log(`real long-step (>60s through the dispatcher) check passed (${heldMs}ms)`);

	// Recovery at scale: every active run owns its recovery alarm, so recovery
	// does not depend on a World startup scan or a coordinator sweep.
	const SCALE = 40;
	const scaleRunIds: string[] = [];
	for (let i = 0; i < SCALE; i++) {
		const created = await world.events.create(null, {
			eventType: "run_created",
			specVersion: SPEC_VERSION_CURRENT,
			eventData: {
				deploymentId: "rivet",
				workflowName: "scaleRecoveryProbe",
				input: { i },
			},
		});
		const id = created.run?.runId;
		assert(id, "Expected scale run to return a run id");
		scaleRunIds.push(id);
	}
	await waitFor(
		"scale recovery re-enqueue",
		() =>
			scaleRunIds.every(
				(id) =>
					deliveryCount(
						deliveries,
						(d) => (d.body as { runId?: string }).runId === id,
					) >= 1,
			),
		60_000,
	);
	log(`recovery at scale (${SCALE} runs) check passed`);
}

async function run() {
	const engine = await startEngine();
	const namespace = `it-${randomUUID()}`;
	const poolName = `it-${randomUUID()}`;
	let runtime: ManagedProcess | undefined;
	let mockApp: Awaited<ReturnType<typeof startMockWorkflowApp>> | undefined;
	let world: RivetClientWorld | undefined;
	let client: Client<typeof registry> | undefined;

	try {
		await createNamespace(engine.endpoint, namespace);
		await upsertRunnerConfig(engine.endpoint, namespace, poolName);
		runtime = startRuntime(engine.endpoint, namespace, poolName);
		await waitForEnvoy(runtime, engine.endpoint, namespace, poolName);
		log(`runtime ready for namespace ${namespace}`);

		mockApp = await startMockWorkflowApp();
		const app = mockApp;
		client = createClient<typeof registry>({
			endpoint: engine.endpoint,
			namespace,
			poolName,
			token: TOKEN,
		});
		const worldConfig = {
			client,
			runtimeUrl: app.url,
		};
		world = new RivetClientWorld(worldConfig);
		await runChecks(
			world,
			worldConfig,
			app.deliveries,
			app.healthChecks,
			app.behavior,
		);
		const restartRunId = `wrun_it_runtime_restart_${randomUUID()}`;
		await world.queue(
			"__wkf_workflow_runtimeRestartProbe",
			{ runId: restartRunId },
			{ delaySeconds: 3 },
		);
		await stopProcess(runtime.child, "runtime");
		runtime = undefined;
		runtime = startRuntime(engine.endpoint, namespace, poolName);
		await waitForEnvoy(runtime, engine.endpoint, namespace, poolName);
		await waitFor(
			"runtime restart delayed queue delivery",
			() =>
				deliveryCount(
					app.deliveries,
					(delivery) => (delivery.body as { runId?: string }).runId === restartRunId,
				) === 1,
			45_000,
		);
		log("runtime restart delayed queue recovery check passed");
		log("integration checks passed");
	} catch (error) {
		if (runtime) {
			console.error("=== RUNTIME STDIO ===");
			console.error(runtime.output());
		}
		throw error;
	} finally {
		if (world) await world.close();
		if (client) await client.dispose();
		if (mockApp) await mockApp.close();
		if (runtime) await stopProcess(runtime.child, "runtime");
		await stopProcess(engine.child, "engine");
		rmSync(engine.dbRoot, { force: true, recursive: true });
	}
}

run().catch((error) => {
	console.error("[integration] FAILED");
	console.error(error);
	process.exitCode = 1;
});
