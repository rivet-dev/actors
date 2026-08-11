import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { getEnginePath } from "@rivetkit/engine-cli";
import getPort from "get-port";
import { createClient } from "../../src/client/mod";

const TEST_DIR = join(dirname(fileURLToPath(import.meta.url)), "..");
const PACKAGE_DIR = dirname(TEST_DIR);
const REPO_ENGINE_BINARY =
	process.env.ENGINE_BINARY ??
	join(PACKAGE_DIR, "../../../target/debug/rivet-engine");
const SERVERLESS_RUNTIME_PATH = join(
	TEST_DIR,
	"fixtures/engine-restart-serverless-runtime.ts",
);
const TOKEN = "dev";
const HOST = "127.0.0.1";
const ENGINE_START_TIMEOUT_MS = 90_000;
const SERVERLESS_START_TIMEOUT_MS = 30_000;
const INITIAL_COUNTER_READY_TIMEOUT_MS = 90_000;
const POST_RESTART_HEARTBEAT_OBSERVATION_MS = Number(
	process.env.RIVETKIT_POST_RESTART_WAIT_MS ?? "12000",
);
const POST_RESTART_PROBE_TIMEOUT_MS = 20_000;
const GATEWAY_HEALTH_TIMEOUT_MS = Number(
	process.env.RIVETKIT_GATEWAY_HEALTH_TIMEOUT_MS ?? "5000",
);
const COMMIT_FAILURE_TIMEOUT_MS = 45_000;
const RUNTIME_ROLL_ACTION_MS = Number(
	process.env.RIVETKIT_RUNTIME_ROLL_ACTION_MS ?? "25000",
);
const RESTART_MODE = process.env.RIVETKIT_ENGINE_RESTART_MODE ?? "commit";
const HEARTBEAT_MODE = process.env.RIVETKIT_HEARTBEAT_MODE ?? "sqlite";
const POST_RESTART_PROBE_TIMING =
	process.env.RIVETKIT_POST_RESTART_PROBE_TIMING ?? "after-heartbeat";
const GATEWAY_HEALTH_DELAYS_MS = parseDelayList(
	process.env.RIVETKIT_GATEWAY_HEALTH_DELAYS_MS,
);
const GATEWAY_WEBSOCKET_DELAYS_MS = parseDelayList(
	process.env.RIVETKIT_GATEWAY_WEBSOCKET_DELAYS_MS,
);

if (!["none", "sqlite", "kv"].includes(HEARTBEAT_MODE)) {
	throw new Error("RIVETKIT_HEARTBEAT_MODE must be one of: none, sqlite, kv");
}
if (!["immediate", "after-heartbeat"].includes(POST_RESTART_PROBE_TIMING)) {
	throw new Error(
		"RIVETKIT_POST_RESTART_PROBE_TIMING must be one of: immediate, after-heartbeat",
	);
}

interface RuntimeLogs {
	stdout: string;
	stderr: string;
}

interface HeartbeatStats {
	ticks: number;
	sqlOk: number;
	sqlErr: number;
	kvOk: number;
	kvErr: number;
	onWake: number;
	onSleep: number;
	abort: number;
	rollbackErr: number;
	lastOkCount: number | undefined;
	lastError: string | undefined;
}

interface CommitSignalServer {
	url: string;
	waitForSignal: Promise<void>;
	close: () => Promise<void>;
}

interface GatewayHealthTarget {
	delayMs: number;
	key: string;
	url: string;
}

interface GatewayWebSocketTarget {
	delayMs: number;
	key: string;
	url: string;
}

class OwnedEngine {
	readonly dbRoot = mkdtempSync(join(tmpdir(), "rivetkit-engine-restart-"));
	readonly endpoint: string;
	readonly peerUrl: string;
	readonly configPath = join(this.dbRoot, "config.json");
	readonly #guardPort: number;
	readonly #apiPeerPort: number;
	readonly #metricsPort: number;
	#child: ChildProcess | undefined;
	#logs: RuntimeLogs = { stdout: "", stderr: "" };

	private constructor(
		guardPort: number,
		apiPeerPort: number,
		metricsPort: number,
	) {
		this.#guardPort = guardPort;
		this.#apiPeerPort = apiPeerPort;
		this.#metricsPort = metricsPort;
		this.endpoint = `http://${HOST}:${guardPort}`;
		this.peerUrl = `http://${HOST}:${apiPeerPort}`;
		this.#writeConfig();
	}

	static async start(): Promise<OwnedEngine> {
		const guardPort = await getPort({ host: HOST });
		const apiPeerPort = await getPort({ host: HOST, exclude: [guardPort] });
		const metricsPort = await getPort({
			host: HOST,
			exclude: [guardPort, apiPeerPort],
		});
		const engine = new OwnedEngine(guardPort, apiPeerPort, metricsPort);
		await engine.startProcess();
		return engine;
	}

	async startProcess(): Promise<void> {
		if (isProcessRunning(this.#child)) {
			return;
		}

		this.#logs = { stdout: "", stderr: "" };
		const child = spawn(
			resolveEngineBinaryPath(),
			["start", "--config", this.configPath],
			{
				env: {
					...process.env,
					RIVET__PEGBOARD__ENVOY_LOST_THRESHOLD:
						RESTART_MODE === "runtime-roll" ? "35000" : undefined,
					RIVET__PEGBOARD__SERVERLESS_DRAIN_GRACE_PERIOD:
						RESTART_MODE === "runtime-roll" ? "35000" : undefined,
					RIVET__GUARD__HOST: HOST,
					RIVET__GUARD__PORT: this.#guardPort.toString(),
					RIVET__API_PEER__HOST: HOST,
					RIVET__API_PEER__PORT: this.#apiPeerPort.toString(),
					RIVET__METRICS__HOST: HOST,
					RIVET__METRICS__PORT: this.#metricsPort.toString(),
					RIVET__FILE_SYSTEM__PATH: join(this.dbRoot, "db"),
				},
				stdio: ["ignore", "pipe", "pipe"],
			},
		);
		this.#child = child;
		child.stdout?.on("data", (chunk) => {
			const text = chunk.toString();
			this.#logs.stdout += text;
			if (process.env.DRIVER_ENGINE_LOGS === "1") {
				process.stderr.write(`[RESTART_ENG.OUT] ${text}`);
			}
		});
		child.stderr?.on("data", (chunk) => {
			const text = chunk.toString();
			this.#logs.stderr += text;
			if (process.env.DRIVER_ENGINE_LOGS === "1") {
				process.stderr.write(`[RESTART_ENG.ERR] ${text}`);
			}
		});
		await waitForEngineHealth(
			child,
			this.#logs,
			this.endpoint,
			ENGINE_START_TIMEOUT_MS,
		);
		console.log(`engine listening at ${this.endpoint} (${this.dbRoot})`);
	}

	async stopProcess(signal: NodeJS.Signals = "SIGTERM"): Promise<void> {
		const child = this.#child;
		if (!isProcessRunning(child)) {
			return;
		}

		await stopChildProcess(child, signal);
	}

	async cleanup(): Promise<void> {
		await this.stopProcess();
		rmSync(this.dbRoot, { force: true, recursive: true });
	}

	#writeConfig(): void {
		writeFileSync(
			this.configPath,
			JSON.stringify({
				topology: {
					datacenter_label: 1,
					datacenters: {
						default: {
							datacenter_label: 1,
							is_leader: true,
							public_url: this.endpoint,
							peer_url: this.peerUrl,
						},
					},
				},
			}),
		);
	}
}

async function stopChildProcess(
	child: ChildProcess,
	signal: NodeJS.Signals = "SIGTERM",
	forceAfterMs = 5_000,
): Promise<boolean> {
	if (!isProcessRunning(child)) {
		return false;
	}

	return new Promise<boolean>((resolve) => {
		let forceKill: NodeJS.Timeout | undefined;
		let forced = false;
		const finish = () => {
			if (forceKill) {
				clearTimeout(forceKill);
			}
			resolve(forced);
		};
		child.once("exit", finish);
		child.kill(signal);
		forceKill = setTimeout(() => {
			if (isProcessRunning(child)) {
				forced = true;
				child.kill("SIGKILL");
			}
		}, forceAfterMs);
	});
}

class ServerlessRuntime {
	readonly url: string;
	readonly #port: number;
	#child: ChildProcess | undefined;
	#logs: RuntimeLogs = { stdout: "", stderr: "" };

	private constructor(port: number) {
		this.#port = port;
		this.url = `http://${HOST}:${port}/api/rivet`;
	}

	static async start(input: {
		endpoint: string;
		namespace: string;
		poolName: string;
	}): Promise<ServerlessRuntime> {
		const port = await getPort({ host: HOST });
		const runtime = new ServerlessRuntime(port);
		await runtime.startProcess(input);
		return runtime;
	}

	getOutput(): string {
		return childOutput(this.#logs);
	}

	async startProcess(input: {
		endpoint: string;
		namespace: string;
		poolName: string;
	}): Promise<void> {
		const child = spawn(
			process.execPath,
			["--import", "tsx", SERVERLESS_RUNTIME_PATH],
			{
				cwd: PACKAGE_DIR,
				env: {
					...process.env,
					RIVET_TOKEN: TOKEN,
					RIVET_NAMESPACE: input.namespace,
					RIVETKIT_TEST_ENDPOINT: input.endpoint,
					RIVETKIT_HEARTBEAT_MODE: HEARTBEAT_MODE,
					RIVETKIT_TEST_HOST: HOST,
					RIVETKIT_TEST_POOL_NAME: input.poolName,
					RIVETKIT_TEST_PORT: this.#port.toString(),
				},
				stdio: ["ignore", "pipe", "pipe"],
			},
		);
		this.#child = child;
		child.stdout?.on("data", (chunk) => {
			const text = chunk.toString();
			this.#logs.stdout += text;
			if (process.env.DRIVER_RUNTIME_LOGS === "1") {
				process.stderr.write(`[SERVERLESS_RT.OUT] ${text}`);
			}
		});
		child.stderr?.on("data", (chunk) => {
			const text = chunk.toString();
			this.#logs.stderr += text;
			if (process.env.DRIVER_RUNTIME_LOGS === "1") {
				process.stderr.write(`[SERVERLESS_RT.ERR] ${text}`);
			}
		});
		await waitForHttpOk({
			url: `${this.url}/health`,
			child,
			logs: this.#logs,
			timeoutMs: SERVERLESS_START_TIMEOUT_MS,
		});
		console.log(`serverless runtime listening at ${this.url}`);
	}

	async cleanup(): Promise<void> {
		await this.stopProcess();
	}

	async stopProcess(
		signal: NodeJS.Signals = "SIGTERM",
		forceAfterMs = 5_000,
	): Promise<boolean> {
		const child = this.#child;
		if (isProcessRunning(child)) {
			return stopChildProcess(child, signal, forceAfterMs);
		}
		return false;
	}
}

async function main() {
	const namespace = `restart-${crypto.randomUUID()}`;
	const poolName = `serverless-restart-${crypto.randomUUID()}`;
	const actorKey = `sqlite-counter-${crypto.randomUUID()}`;
	const engine = await OwnedEngine.start();
	let runtime: ServerlessRuntime | undefined;
	let client: ReturnType<typeof createClient> | undefined;
	let signalServer: CommitSignalServer | undefined;

	try {
		await createNamespace(engine.endpoint, namespace);
		runtime = await ServerlessRuntime.start({
			endpoint: engine.endpoint,
			namespace,
			poolName,
		});
		await upsertServerlessRunnerConfig({
			endpoint: engine.endpoint,
			namespace,
			poolName,
			serverlessUrl: runtime.url,
		});
		await waitForRunnerConfigReady({
			endpoint: engine.endpoint,
			namespace,
			poolName,
		});

		client = createClient({
			endpoint: engine.endpoint,
			namespace,
			poolName,
			token: TOKEN,
			encoding: "bare",
			disableMetadataLookup: true,
		});
		const actorHandle = client.sqliteCounter.getOrCreate([actorKey]);
		const heartbeatWarmupStartedAt = Date.now();
		const countBeforeRestart = (await actorHandle.getCount()) as number;
		console.log(
			`restart scenario configured. restartMode=${RESTART_MODE} heartbeatMode=${HEARTBEAT_MODE} postRestartProbeTiming=${POST_RESTART_PROBE_TIMING}`,
		);
		if (HEARTBEAT_MODE !== "none") {
			await waitFor(
				() =>
					heartbeatSuccessCount(
						getHeartbeatStats(
							runtime?.getOutput() ?? "",
							heartbeatWarmupStartedAt,
						),
					) >= 2,
				INITIAL_COUNTER_READY_TIMEOUT_MS,
				() => {
					const stats = getHeartbeatStats(
						runtime?.getOutput() ?? "",
						heartbeatWarmupStartedAt,
					);
					return `actor heartbeat did not start. ${formatHeartbeatStats(stats)}`;
				},
			);
		}
		const warmupStats = getHeartbeatStats(
			runtime.getOutput(),
			heartbeatWarmupStartedAt,
		);
		console.log(
			`actor-originated heartbeat warmup finished. before=${countBeforeRestart} ${formatHeartbeatStats(warmupStats)}`,
		);
		if (RESTART_MODE === "runtime-roll") {
			signalServer = await createCommitSignalServer();
			const rollStartedAt = Date.now();
			const action = withTimeout(
				actorHandle.finishDuringRuntimeRoll({
					signalUrl: signalServer.url,
					delayMs: RUNTIME_ROLL_ACTION_MS,
				}) as Promise<unknown>,
				RUNTIME_ROLL_ACTION_MS + 30_000,
				"in-flight action did not finish during the runtime roll",
			);

			await signalServer.waitForSignal;
			const stopping = runtime.stopProcess(
				"SIGTERM",
				RUNTIME_ROLL_ACTION_MS + 15_000,
			);
			await action;
			const forced = await stopping;
			if (forced) {
				throw new Error(
					"runtime did not exit within its configured grace period",
				);
			}

			const drainDurationMs = Date.now() - rollStartedAt;
			if (drainDurationMs < 20_000) {
				throw new Error(
					`runtime stopped before the action's >20s graceful drain completed: ${drainDurationMs}ms`,
				);
			}
			if (runtime.getOutput().includes("forcing immediate stop")) {
				throw new Error(
					"runtime forced the envoy to stop during the graceful drain",
				);
			}

			await runtime.startProcess({
				endpoint: engine.endpoint,
				namespace,
				poolName,
			});
			const countAfterRoll = (await actorHandle.getCount()) as number;
			if (countAfterRoll <= countBeforeRestart) {
				throw new Error(
					`durable counter did not advance across runtime roll: before=${countBeforeRestart} after=${countAfterRoll}`,
				);
			}
			const replacementResult = (await actorHandle.tick()) as {
				count: number;
				events: number;
			};
			if (replacementResult.count <= countAfterRoll) {
				throw new Error(
					"replacement runtime did not wake and advance the actor",
				);
			}
			console.log(
				`runtime roll preserved the in-flight action and durable actor state. drainMs=${drainDurationMs} count=${replacementResult.count} events=${replacementResult.events}`,
			);
			return;
		}
		const gatewayHealthTargets =
			GATEWAY_HEALTH_DELAYS_MS.length > 0
				? await prepareGatewayHealthTargets({
						client,
						baseKey: actorKey,
						delaysMs: GATEWAY_HEALTH_DELAYS_MS,
					})
				: [];
		const gatewayWebSocketTargets =
			GATEWAY_WEBSOCKET_DELAYS_MS.length > 0
				? await prepareGatewayWebSocketTargets({
						client,
						baseKey: actorKey,
						delaysMs: GATEWAY_WEBSOCKET_DELAYS_MS,
					})
				: [];
		if (RESTART_MODE === "idle") {
			console.log(
				`restarting engine while actor is idle at count=${countBeforeRestart}`,
			);
			await sleep(1_000);
			const restartStartedAt = Date.now();
			await engine.stopProcess("SIGTERM");
			console.log("restarting engine");
			await engine.startProcess();
			const engineRestartedAt = Date.now();

			await runPostRestartSequence({
				runtime,
				client,
				actorHandle,
				actorKey,
				countBeforeRestart,
				mode: "idle",
				restartStartedAt,
				engineRestartedAt,
				gatewayHealthTargets,
				gatewayWebSocketTargets,
			});
			return;
		}

		if (RESTART_MODE !== "commit") {
			throw new Error(
				`unsupported RIVETKIT_ENGINE_RESTART_MODE: ${RESTART_MODE}`,
			);
		}

		console.log(
			`starting coordinated commit failure at count=${countBeforeRestart}`,
		);
		signalServer = await createCommitSignalServer();
		const commitAttemptStartedAt = Date.now();
		const commitAttempt = withTimeout(
			actorHandle.commitDuringEngineRestart({
				signalUrl: signalServer.url,
				delayBeforeCommitMs: 500,
				payloadBytes: 8192,
			}) as Promise<unknown>,
			COMMIT_FAILURE_TIMEOUT_MS,
			"commit attempt did not finish after engine restart signal",
		).then(
			(value) => ({ ok: true as const, value }),
			(error) => ({ ok: false as const, error }),
		);

		await signalServer.waitForSignal;
		console.log("actor reached pre-commit point; sending engine SIGTERM");
		const restartStartedAt = Date.now();
		await engine.stopProcess("SIGTERM");

		const commitResult = await commitAttempt;
		if (commitResult.ok) {
			console.warn(
				`commit unexpectedly succeeded after engine shutdown in ${Date.now() - commitAttemptStartedAt}ms`,
			);
		} else {
			console.log(
				`commit failed after engine shutdown in ${Date.now() - commitAttemptStartedAt}ms: ${stringifyError(commitResult.error)}`,
			);
		}

		console.log("restarting engine");
		await engine.startProcess();
		const engineRestartedAt = Date.now();

		await runPostRestartSequence({
			runtime,
			client,
			actorHandle,
			actorKey,
			countBeforeRestart,
			mode: "commit",
			restartStartedAt,
			engineRestartedAt,
			gatewayHealthTargets,
			gatewayWebSocketTargets,
		});
	} finally {
		await signalServer?.close();
		await client?.dispose();
		await runtime?.cleanup();
		await engine.cleanup();
	}
}

function resolveEngineBinaryPath(): string {
	if (existsSync(REPO_ENGINE_BINARY)) {
		return REPO_ENGINE_BINARY;
	}

	return getEnginePath();
}

function childOutput(logs: RuntimeLogs): string {
	return [logs.stdout, logs.stderr].filter(Boolean).join("\n");
}

function isProcessRunning(
	child: ChildProcess | undefined,
): child is ChildProcess {
	return (
		child !== undefined &&
		child.exitCode === null &&
		child.signalCode === null
	);
}

async function waitForEngineHealth(
	child: ChildProcess,
	logs: RuntimeLogs,
	endpoint: string,
	timeoutMs: number,
): Promise<void> {
	await waitForHttpOk({
		child,
		logs,
		timeoutMs,
		url: `${endpoint}/health`,
	});
}

async function waitForHttpOk(input: {
	child?: ChildProcess;
	logs: RuntimeLogs;
	timeoutMs: number;
	url: string;
}): Promise<void> {
	const deadline = Date.now() + input.timeoutMs;

	while (Date.now() < deadline) {
		if (input.child && !isProcessRunning(input.child)) {
			throw new Error(
				`process exited before health check passed:\n${childOutput(input.logs)}`,
			);
		}

		try {
			const response = await fetch(input.url);
			if (response.ok) {
				return;
			}
		} catch {}

		await sleep(500);
	}

	throw new Error(
		`timed out waiting for health at ${input.url}:\n${childOutput(input.logs)}`,
	);
}

async function createNamespace(
	endpoint: string,
	namespace: string,
): Promise<void> {
	const response = await fetch(`${endpoint}/namespaces`, {
		method: "POST",
		headers: {
			Authorization: `Bearer ${TOKEN}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({
			name: namespace,
			display_name: `Engine restart ${namespace}`,
		}),
	});

	if (!response.ok) {
		throw new Error(
			`failed to create namespace ${namespace}: ${response.status} ${await response.text()}`,
		);
	}
}

async function getDatacenter(
	endpoint: string,
	namespace: string,
): Promise<string> {
	const response = await fetch(
		`${endpoint}/datacenters?namespace=${encodeURIComponent(namespace)}`,
		{
			headers: {
				Authorization: `Bearer ${TOKEN}`,
			},
		},
	);

	if (!response.ok) {
		throw new Error(
			`failed to list datacenters: ${response.status} ${await response.text()}`,
		);
	}

	const body = (await response.json()) as {
		datacenters: Array<{ name: string }>;
	};
	const datacenter = body.datacenters[0]?.name;
	if (!datacenter) {
		throw new Error("engine returned no datacenters");
	}

	return datacenter;
}

async function upsertServerlessRunnerConfig(input: {
	endpoint: string;
	namespace: string;
	poolName: string;
	serverlessUrl: string;
}): Promise<void> {
	const datacenter = await getDatacenter(input.endpoint, input.namespace);
	const response = await fetch(
		`${input.endpoint}/runner-configs/${encodeURIComponent(input.poolName)}?namespace=${encodeURIComponent(input.namespace)}`,
		{
			method: "PUT",
			headers: {
				Authorization: `Bearer ${TOKEN}`,
				"Content-Type": "application/json",
			},
			body: JSON.stringify({
				datacenters: {
					[datacenter]: {
						serverless: {
							url: input.serverlessUrl,
							headers: {
								"x-rivet-token": TOKEN,
							},
							request_lifespan: 3600,
							drain_grace_period:
								RESTART_MODE === "runtime-roll" ? 30 : 5,
							metadata_poll_interval: 1000,
							max_runners: 10,
							min_runners: 0,
							runners_margin: 0,
							slots_per_runner: 1,
						},
						metadata: {},
						drain_on_version_upgrade: true,
					},
				},
			}),
		},
	);

	if (!response.ok) {
		throw new Error(
			`failed to upsert serverless runner config: ${response.status} ${await response.text()}`,
		);
	}
}

async function waitForRunnerConfigReady(input: {
	endpoint: string;
	namespace: string;
	poolName: string;
}): Promise<void> {
	const deadline = Date.now() + 30_000;
	let bodyText = "";

	while (Date.now() < deadline) {
		const response = await fetch(
			`${input.endpoint}/runner-configs?namespace=${encodeURIComponent(input.namespace)}&runner_name=${encodeURIComponent(input.poolName)}`,
			{
				headers: {
					Authorization: `Bearer ${TOKEN}`,
				},
			},
		);
		bodyText = await response.text();
		if (response.ok) {
			const body = JSON.parse(bodyText) as {
				runner_configs?: Record<
					string,
					{
						datacenters?: Record<
							string,
							{
								protocol_version?: number | null;
							}
						>;
					}
				>;
			};
			const config = body.runner_configs?.[input.poolName];
			const datacenters = Object.values(config?.datacenters ?? {});
			if (
				datacenters.length > 0 &&
				datacenters.every(
					(datacenter) => datacenter.protocol_version != null,
				)
			) {
				return;
			}
		}

		await sleep(250);
	}

	throw new Error(`serverless runner config was not ready: ${bodyText}`);
}

async function createCommitSignalServer(): Promise<CommitSignalServer> {
	const port = await getPort({ host: HOST });
	let resolveSignal!: () => void;
	const waitForSignal = new Promise<void>((resolve) => {
		resolveSignal = resolve;
	});
	const server: Server = createServer((request, response) => {
		if (request.method === "POST" && request.url === "/before-commit") {
			resolveSignal();
			response.writeHead(204);
			response.end();
			return;
		}

		response.writeHead(404);
		response.end();
	});

	await new Promise<void>((resolve) => {
		server.listen(port, HOST, resolve);
	});

	return {
		url: `http://${HOST}:${port}/before-commit`,
		waitForSignal,
		close: () =>
			new Promise<void>((resolve, reject) => {
				server.close((error) => {
					if (error) {
						reject(error);
					} else {
						resolve();
					}
				});
			}),
	};
}

async function probeActor(
	name: string,
	operation: () => Promise<unknown>,
): Promise<{
	name: string;
	elapsedMs: number;
	result: unknown;
}> {
	const startedAt = Date.now();
	const result = await withTimeout(
		operation(),
		POST_RESTART_PROBE_TIMEOUT_MS,
		`${name} did not complete after engine restart`,
	);

	return {
		name,
		elapsedMs: Date.now() - startedAt,
		result,
	};
}

async function prepareGatewayHealthTargets(input: {
	client: ReturnType<typeof createClient>;
	baseKey: string;
	delaysMs: number[];
}): Promise<GatewayHealthTarget[]> {
	const targets: GatewayHealthTarget[] = [];
	for (const delayMs of input.delaysMs) {
		const key = `${input.baseKey}-gateway-health-${delayMs}`;
		const handle = input.client.sqliteCounter.getOrCreate([key]);
		await handle.getCount();
		const url = buildActorRequestUrl(
			await handle.getGatewayUrl(),
			"health",
		);
		const response = await fetch(url, {
			signal: AbortSignal.timeout(GATEWAY_HEALTH_TIMEOUT_MS),
		});
		if (!response.ok) {
			throw new Error(
				`gateway health preflight failed for delay ${delayMs}: ${response.status} ${await response.text()}`,
			);
		}
		targets.push({
			delayMs,
			key,
			url,
		});
	}

	console.log(
		`gateway health targets warmed. delaysMs=${input.delaysMs.join(",")}`,
	);
	return targets;
}

async function runGatewayHealthDelaySweep(input: {
	targets: GatewayHealthTarget[];
	engineRestartedAt: number;
	mode: string;
}): Promise<void> {
	const startedAt = Date.now();
	console.log(
		`gateway health delay sweep starting. mode=${input.mode} delaysMs=${input.targets.map((target) => target.delayMs).join(",")}`,
	);

	const results = await Promise.all(
		input.targets.map(async (target) => {
			const sleepMs = Math.max(
				0,
				input.engineRestartedAt + target.delayMs - Date.now(),
			);
			if (sleepMs > 0) {
				await sleep(sleepMs);
			}

			const probeStartedAt = Date.now();
			try {
				const response = await fetch(target.url, {
					signal: AbortSignal.timeout(GATEWAY_HEALTH_TIMEOUT_MS),
				});
				const body = await response.text();
				return {
					...target,
					ok: response.ok,
					status: response.status,
					elapsedMs: Date.now() - probeStartedAt,
					startOffsetMs: probeStartedAt - input.engineRestartedAt,
					body,
				};
			} catch (error) {
				return {
					...target,
					ok: false,
					status: "error",
					elapsedMs: Date.now() - probeStartedAt,
					startOffsetMs: probeStartedAt - input.engineRestartedAt,
					body: stringifyError(error),
				};
			}
		}),
	);

	for (const result of results.sort((a, b) => a.delayMs - b.delayMs)) {
		const body =
			result.body.length > 240
				? `${result.body.slice(0, 240)}...`
				: result.body;
		console.log(
			`gateway-health delayMs=${result.delayMs} startOffsetMs=${result.startOffsetMs} ok=${result.ok} status=${result.status} elapsedMs=${result.elapsedMs} key=${result.key} body=${JSON.stringify(body)}`,
		);
	}

	const firstOk = results
		.filter((result) => result.ok)
		.sort((a, b) => a.delayMs - b.delayMs)[0];
	if (firstOk) {
		console.log(
			`gateway health first success. mode=${input.mode} delayMs=${firstOk.delayMs} totalSweepMs=${Date.now() - startedAt}`,
		);
	} else {
		console.log(
			`gateway health no successes. mode=${input.mode} totalSweepMs=${Date.now() - startedAt}`,
		);
	}
}

function buildActorRequestUrl(gatewayUrl: string, path: string): string {
	const url = new URL(gatewayUrl);
	const normalizedPath = path.replace(/^\/+/, "");
	url.pathname = `${url.pathname.replace(/\/$/, "")}/request/${normalizedPath}`;
	return url.toString();
}

async function prepareGatewayWebSocketTargets(input: {
	client: ReturnType<typeof createClient>;
	baseKey: string;
	delaysMs: number[];
}): Promise<GatewayWebSocketTarget[]> {
	const targets: GatewayWebSocketTarget[] = [];
	for (const delayMs of input.delaysMs) {
		const key = `${input.baseKey}-gateway-ws-${delayMs}`;
		const handle = input.client.sqliteCounter.getOrCreate([key]);
		await handle.getCount();
		const url = buildActorWebSocketUrl(
			await handle.getGatewayUrl(),
			"ping",
		);
		await openWebSocketPingPong(url);
		targets.push({
			delayMs,
			key,
			url,
		});
	}

	console.log(
		`gateway websocket targets warmed. delaysMs=${input.delaysMs.join(",")}`,
	);
	return targets;
}

async function runGatewayWebSocketDelaySweep(input: {
	targets: GatewayWebSocketTarget[];
	engineRestartedAt: number;
	mode: string;
}): Promise<void> {
	const startedAt = Date.now();
	console.log(
		`gateway websocket delay sweep starting. mode=${input.mode} delaysMs=${input.targets.map((target) => target.delayMs).join(",")}`,
	);

	const results = await Promise.all(
		input.targets.map(async (target) => {
			const sleepMs = Math.max(
				0,
				input.engineRestartedAt + target.delayMs - Date.now(),
			);
			if (sleepMs > 0) {
				await sleep(sleepMs);
			}

			const probeStartedAt = Date.now();
			try {
				const message = await openWebSocketPingPong(target.url);
				return {
					...target,
					ok: true,
					elapsedMs: Date.now() - probeStartedAt,
					startOffsetMs: probeStartedAt - input.engineRestartedAt,
					body: message,
				};
			} catch (error) {
				return {
					...target,
					ok: false,
					elapsedMs: Date.now() - probeStartedAt,
					startOffsetMs: probeStartedAt - input.engineRestartedAt,
					body: stringifyError(error),
				};
			}
		}),
	);

	for (const result of results.sort((a, b) => a.delayMs - b.delayMs)) {
		const body =
			result.body.length > 240
				? `${result.body.slice(0, 240)}...`
				: result.body;
		console.log(
			`gateway-websocket delayMs=${result.delayMs} startOffsetMs=${result.startOffsetMs} ok=${result.ok} elapsedMs=${result.elapsedMs} key=${result.key} body=${JSON.stringify(body)}`,
		);
	}

	const firstOk = results
		.filter((result) => result.ok)
		.sort((a, b) => a.delayMs - b.delayMs)[0];
	if (firstOk) {
		console.log(
			`gateway websocket first success. mode=${input.mode} delayMs=${firstOk.delayMs} totalSweepMs=${Date.now() - startedAt}`,
		);
	} else {
		console.log(
			`gateway websocket no successes. mode=${input.mode} totalSweepMs=${Date.now() - startedAt}`,
		);
	}
}

function buildActorWebSocketUrl(gatewayUrl: string, path: string): string {
	const url = new URL(gatewayUrl);
	const normalizedPath = path.replace(/^\/+/, "");
	url.pathname = `${url.pathname.replace(/\/$/, "")}/websocket/${normalizedPath}`;
	url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
	return url.toString();
}

function openWebSocketPingPong(url: string): Promise<string> {
	return new Promise((resolve, reject) => {
		let settled = false;
		const websocket = new WebSocket(url, ["rivet", "rivet_encoding.bare"]);
		const timeout = setTimeout(() => {
			finish(new Error("websocket ping/pong timed out"));
		}, GATEWAY_HEALTH_TIMEOUT_MS);

		const finish = (result: string | Error) => {
			if (settled) {
				return;
			}
			settled = true;
			clearTimeout(timeout);
			try {
				websocket.close();
			} catch {}
			if (result instanceof Error) {
				reject(result);
			} else {
				resolve(result);
			}
		};

		websocket.addEventListener("open", () => {
			websocket.send(
				JSON.stringify({ type: "ping", sentAt: Date.now() }),
			);
		});
		websocket.addEventListener("message", (event) => {
			const data =
				typeof event.data === "string"
					? event.data
					: String(event.data);
			try {
				const message = JSON.parse(data) as { type?: string };
				if (message.type === "pong") {
					finish(data);
				}
			} catch {
				finish(new Error(`invalid websocket message: ${data}`));
			}
		});
		websocket.addEventListener("error", () => {
			finish(new Error("websocket error"));
		});
		websocket.addEventListener("close", (event) => {
			if (!settled) {
				finish(
					new Error(
						`websocket closed before pong: code=${event.code} reason=${event.reason}`,
					),
				);
			}
		});
	});
}

async function runPostRestartSequence(input: {
	runtime: ServerlessRuntime;
	client: ReturnType<typeof createClient>;
	actorHandle: ReturnType<
		ReturnType<typeof createClient>["sqliteCounter"]["getOrCreate"]
	>;
	actorKey: string;
	countBeforeRestart: number;
	mode: string;
	restartStartedAt: number;
	engineRestartedAt: number;
	gatewayHealthTargets: GatewayHealthTarget[];
	gatewayWebSocketTargets: GatewayWebSocketTarget[];
}): Promise<void> {
	if (input.gatewayWebSocketTargets.length > 0) {
		await runGatewayWebSocketDelaySweep({
			targets: input.gatewayWebSocketTargets,
			engineRestartedAt: input.engineRestartedAt,
			mode: input.mode,
		});
		return;
	}

	if (input.gatewayHealthTargets.length > 0) {
		await runGatewayHealthDelaySweep({
			targets: input.gatewayHealthTargets,
			engineRestartedAt: input.engineRestartedAt,
			mode: input.mode,
		});
		return;
	}

	if (POST_RESTART_PROBE_TIMING === "immediate") {
		await runPostRestartProbes(input);
		await observePostRestartHeartbeat(input);
		return;
	}

	await observePostRestartHeartbeat(input);
	await runPostRestartProbes(input);
}

async function runPostRestartProbes(input: {
	client: ReturnType<typeof createClient>;
	actorHandle: ReturnType<
		ReturnType<typeof createClient>["sqliteCounter"]["getOrCreate"]
	>;
	actorKey: string;
	countBeforeRestart: number;
	mode: string;
}): Promise<void> {
	const probeResults = await Promise.allSettled([
		probeActor("same-handle-getCount", () => input.actorHandle.getCount()),
		probeActor("same-handle-tick", () => input.actorHandle.tick(8192)),
		probeActor("fresh-handle-getCount", () =>
			input.client.sqliteCounter.getOrCreate([input.actorKey]).getCount(),
		),
		probeActor("new-key-tick", () =>
			input.client.sqliteCounter
				.getOrCreate([`post-restart-${crypto.randomUUID()}`])
				.tick(8192),
		),
	]);
	let postRestartProbeFailures = 0;
	for (const probeResult of probeResults) {
		if (probeResult.status === "fulfilled") {
			console.log(
				`${probeResult.value.name} post-restart probe ok elapsedMs=${probeResult.value.elapsedMs} result=${JSON.stringify(probeResult.value.result)}`,
			);
		} else {
			postRestartProbeFailures += 1;
			console.warn(
				`post-restart probe failed: ${stringifyError(probeResult.reason)}`,
			);
		}
	}

	if (postRestartProbeFailures > 0) {
		console.log(
			`bricked actor symptom reproduced. mode=${input.mode} failedPostRestartProbes=${postRestartProbeFailures} before=${input.countBeforeRestart}`,
		);
	} else {
		console.log(
			`serverless restart scenario passed without bricking. mode=${input.mode} before=${input.countBeforeRestart}`,
		);
	}
}

async function observePostRestartHeartbeat(input: {
	runtime: ServerlessRuntime;
	mode: string;
	restartStartedAt: number;
	engineRestartedAt: number;
}): Promise<void> {
	await sleep(POST_RESTART_HEARTBEAT_OBSERVATION_MS);

	const output = input.runtime.getOutput();
	const duringRestart = getHeartbeatStats(output, input.restartStartedAt);
	const afterEngineRestarted = getHeartbeatStats(
		output,
		input.engineRestartedAt,
	);

	console.log(
		`heartbeat observation since restart signal. mode=${input.mode} ${formatHeartbeatStats(duringRestart)}`,
	);
	console.log(
		`heartbeat observation after engine healthy. mode=${input.mode} ${formatHeartbeatStats(afterEngineRestarted)}`,
	);

	if (HEARTBEAT_MODE === "none") {
		console.log(
			`no actor-originated heartbeat configured after engine restart. mode=${input.mode}`,
		);
	} else if (heartbeatSuccessCount(afterEngineRestarted) > 0) {
		console.log(
			`actor-originated ${HEARTBEAT_MODE} survived engine restart. mode=${input.mode}`,
		);
	} else if (heartbeatErrorCount(afterEngineRestarted) > 0) {
		console.log(
			`actor-originated ${HEARTBEAT_MODE} is failing after engine restart. mode=${input.mode} lastError=${afterEngineRestarted.lastError}`,
		);
	} else if (duringRestart.onSleep > 0 || duringRestart.abort > 0) {
		console.log(
			`actor heartbeat stopped because actor shutdown ran during restart. mode=${input.mode}`,
		);
	} else {
		console.log(
			`actor heartbeat produced no post-restart ${HEARTBEAT_MODE} signal. mode=${input.mode}`,
		);
	}
}

function getHeartbeatStats(output: string, sinceTs: number): HeartbeatStats {
	const stats: HeartbeatStats = {
		ticks: 0,
		sqlOk: 0,
		sqlErr: 0,
		kvOk: 0,
		kvErr: 0,
		onWake: 0,
		onSleep: 0,
		abort: 0,
		rollbackErr: 0,
		lastOkCount: undefined,
		lastError: undefined,
	};

	for (const line of output.split(/\r?\n/)) {
		if (!line.startsWith("{")) {
			continue;
		}

		let event: {
			event?: string;
			ts?: number;
			count?: number;
			error?: string;
		};
		try {
			event = JSON.parse(line);
		} catch {
			continue;
		}

		if (
			!event.event ||
			typeof event.ts !== "number" ||
			event.ts < sinceTs
		) {
			continue;
		}

		switch (event.event) {
			case "heartbeat_tick":
				stats.ticks += 1;
				break;
			case "heartbeat_sql_ok":
				stats.sqlOk += 1;
				if (typeof event.count === "number") {
					stats.lastOkCount = event.count;
				}
				break;
			case "heartbeat_sql_err":
				stats.sqlErr += 1;
				stats.lastError = event.error;
				break;
			case "heartbeat_kv_ok":
				stats.kvOk += 1;
				if (typeof event.count === "number") {
					stats.lastOkCount = event.count;
				}
				break;
			case "heartbeat_kv_err":
				stats.kvErr += 1;
				stats.lastError = event.error;
				break;
			case "heartbeat_on_wake":
				stats.onWake += 1;
				break;
			case "heartbeat_on_sleep":
				stats.onSleep += 1;
				break;
			case "heartbeat_abort":
				stats.abort += 1;
				break;
			case "heartbeat_rollback_err":
				stats.rollbackErr += 1;
				stats.lastError = event.error;
				break;
			default:
				break;
		}
	}

	return stats;
}

function formatHeartbeatStats(stats: HeartbeatStats): string {
	return [
		`ticks=${stats.ticks}`,
		`sqlOk=${stats.sqlOk}`,
		`sqlErr=${stats.sqlErr}`,
		`kvOk=${stats.kvOk}`,
		`kvErr=${stats.kvErr}`,
		`onWake=${stats.onWake}`,
		`onSleep=${stats.onSleep}`,
		`abort=${stats.abort}`,
		`rollbackErr=${stats.rollbackErr}`,
		`lastOkCount=${stats.lastOkCount ?? "none"}`,
		`lastError=${stats.lastError ?? "none"}`,
	].join(" ");
}

function heartbeatSuccessCount(stats: HeartbeatStats): number {
	if (HEARTBEAT_MODE === "sqlite") {
		return stats.sqlOk;
	}
	if (HEARTBEAT_MODE === "kv") {
		return stats.kvOk;
	}
	return 0;
}

function heartbeatErrorCount(stats: HeartbeatStats): number {
	if (HEARTBEAT_MODE === "sqlite") {
		return stats.sqlErr;
	}
	if (HEARTBEAT_MODE === "kv") {
		return stats.kvErr;
	}
	return 0;
}

function parseDelayList(value: string | undefined): number[] {
	if (!value) {
		return [];
	}

	return value
		.split(",")
		.map((part) => Number(part.trim()))
		.filter((delayMs) => Number.isFinite(delayMs) && delayMs >= 0)
		.sort((a, b) => a - b);
}

async function waitFor(
	predicate: () => boolean,
	timeoutMs: number,
	makeError: () => string,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;

	while (Date.now() < deadline) {
		if (predicate()) {
			return;
		}
		await sleep(250);
	}

	throw new Error(makeError());
}

function stringifyError(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}

function withTimeout<T>(
	promise: Promise<T>,
	timeoutMs: number,
	message: string,
): Promise<T> {
	return new Promise<T>((resolve, reject) => {
		const timeout = setTimeout(() => {
			reject(new Error(message));
		}, timeoutMs);

		promise.then(
			(value) => {
				clearTimeout(timeout);
				resolve(value);
			},
			(error) => {
				clearTimeout(timeout);
				reject(error);
			},
		);
	});
}

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

main().catch((error) => {
	console.error(error);
	process.exit(1);
});
