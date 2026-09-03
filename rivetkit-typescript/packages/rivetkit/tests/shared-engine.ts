import { type ChildProcess, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
	existsSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
	rmSync,
	statSync,
	unlinkSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { getEnginePath } from "@rivetkit/engine-cli";
import getPort from "get-port";

const TEST_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ENGINE_BINARY = join(
	TEST_DIR,
	"../../../../target/debug/rivet-engine",
);
const TIMING_ENABLED = process.env.RIVETKIT_DRIVER_TEST_TIMING === "1";
const ENGINE_START_LOCK_STALE_MS = 120_000;

interface RuntimeLogs {
	stdout: string;
	stderr: string;
}

export const TEST_ENGINE_TOKEN = "dev";

export interface SharedTestEngine {
	endpoint: string;
	metricsEndpoint: string;
	pid: number;
	dbRoot: string;
}

export interface SharedTestEngineOptions {
	gateway3?: boolean;
}

interface SharedEngineFiles {
	lockDir: string;
	statePath: string;
}

interface SharedEngineState extends SharedTestEngine {
	refs: number;
}

let sharedEnginePromise: Promise<SharedTestEngine> | undefined;
let sharedEngineRefAcquired = false;
let sharedEngineProfile: string | undefined;
let sharedEngineFiles: SharedEngineFiles | undefined;

function resolveSharedEngineProfile(options: SharedTestEngineOptions): string {
	return JSON.stringify({ gateway3: options.gateway3 ?? false });
}

function resolveSharedEngineFiles(profile: string): SharedEngineFiles {
	const stateId = createHash("sha256")
		.update(`${TEST_DIR}\0driver-engine-v2\0${profile}`)
		.digest("hex")
		.slice(0, 16);
	return {
		lockDir: join(tmpdir(), `rivetkit-driver-engine-${stateId}.lock`),
		statePath: join(tmpdir(), `rivetkit-driver-engine-${stateId}.json`),
	};
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

function resolveEngineBinaryPath(): string {
	if (existsSync(REPO_ENGINE_BINARY)) {
		return REPO_ENGINE_BINARY;
	}

	return getEnginePath();
}

async function acquireEngineStartLock(
	files: SharedEngineFiles,
): Promise<() => void> {
	const startedAt = performance.now();

	while (true) {
		try {
			mkdirSync(files.lockDir);
			timing("engine.start_lock", startedAt);
			return () => {
				rmSync(files.lockDir, { force: true, recursive: true });
			};
		} catch (error) {
			const code = (error as NodeJS.ErrnoException).code;
			if (code !== "EEXIST") {
				throw error;
			}

			try {
				const stat = statSync(files.lockDir);
				if (Date.now() - stat.mtimeMs > ENGINE_START_LOCK_STALE_MS) {
					rmSync(files.lockDir, {
						force: true,
						recursive: true,
					});
					continue;
				}
			} catch {}

			await new Promise((resolve) => setTimeout(resolve, 50));
		}
	}
}

async function waitForEngineHealth(
	child: ChildProcess,
	logs: RuntimeLogs,
	endpoint: string,
	timeoutMs: number,
): Promise<void> {
	const deadline = Date.now() + timeoutMs;

	while (Date.now() < deadline) {
		if (child.exitCode !== null) {
			throw new Error(
				`shared engine exited before health check passed:\n${childOutput(logs)}`,
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
		`timed out waiting for shared engine health:\n${childOutput(logs)}`,
	);
}

function readSharedEngineState(
	files: SharedEngineFiles,
): SharedEngineState | undefined {
	try {
		return JSON.parse(readFileSync(files.statePath, "utf8"));
	} catch {
		return undefined;
	}
}

function writeSharedEngineState(
	files: SharedEngineFiles,
	state: SharedEngineState,
): void {
	writeFileSync(files.statePath, JSON.stringify(state), "utf8");
}

function removeSharedEngineState(files: SharedEngineFiles): void {
	try {
		unlinkSync(files.statePath);
	} catch {}
}

function isPidRunning(pid: number): boolean {
	try {
		process.kill(pid, 0);
		return true;
	} catch {
		return false;
	}
}

async function isEngineHealthy(endpoint: string): Promise<boolean> {
	try {
		const response = await fetch(`${endpoint}/health`);
		return response.ok;
	} catch {
		return false;
	}
}

async function stopProcess(
	child: ChildProcess,
	signal: NodeJS.Signals,
	timeoutMs: number,
): Promise<void> {
	if (child.exitCode !== null) {
		return;
	}

	child.kill(signal);

	await new Promise<void>((resolve) => {
		const timeout = setTimeout(() => {
			if (child.exitCode === null) {
				child.kill("SIGKILL");
			}
		}, timeoutMs);

		child.once("exit", () => {
			clearTimeout(timeout);
			resolve();
		});
	});
}

async function stopPid(pid: number, timeoutMs: number): Promise<void> {
	if (!isPidRunning(pid)) {
		return;
	}

	process.kill(pid, "SIGTERM");

	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		if (!isPidRunning(pid)) {
			return;
		}
		await new Promise((resolve) => setTimeout(resolve, 100));
	}

	if (isPidRunning(pid)) {
		process.kill(pid, "SIGKILL");
	}
}

async function stopRuntime(child: ChildProcess): Promise<void> {
	const startedAt = performance.now();
	await stopProcess(child, "SIGTERM", 1_000);
	timing("runtime.stop", startedAt);
}

async function spawnSharedEngine(
	options: SharedTestEngineOptions,
): Promise<SharedTestEngine> {
	const startedAt = performance.now();
	const portStartedAt = performance.now();
	const host = "127.0.0.1";
	const guardPort = await getPort({ host });
	const apiPeerPort = await getPort({
		host,
		exclude: [guardPort],
	});
	const metricsPort = await getPort({
		host,
		exclude: [guardPort, apiPeerPort],
	});
	const endpoint = `http://${host}:${guardPort}`;
	const metricsEndpoint = `http://${host}:${metricsPort}`;
	const dbRoot = mkdtempSync(join(tmpdir(), "rivetkit-driver-engine-"));
	const configPath = join(dbRoot, "config.json");
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
			pegboard: options.gateway3
				? {
						gateway_update_ping_interval_ms: 250,
						gateway_tunnel_ping_timeout_ms: 2_000,
						gateway_response_start_timeout_ms: 3_000,
					}
				: undefined,
			features: options.gateway3
				? {
						guard_gateway_v3: {
							mode: "on",
							percentage: 100,
						},
					}
				: undefined,
		}),
	);
	timing("engine.allocate", portStartedAt, { endpoint });

	const spawnStartedAt = performance.now();
	const logs: RuntimeLogs = { stdout: "", stderr: "" };
	const engine = spawn(
		resolveEngineBinaryPath(),
		["start", "--config", configPath],
		{
			env: {
				...process.env,
				RIVET__GUARD__HOST: host,
				RIVET__GUARD__PORT: guardPort.toString(),
				RIVET__API_PEER__HOST: host,
				RIVET__API_PEER__PORT: apiPeerPort.toString(),
				RIVET__METRICS__HOST: host,
				RIVET__METRICS__PORT: metricsPort.toString(),
				RIVET__FILE_SYSTEM__PATH: join(dbRoot, "db"),
			},
			stdio: ["ignore", "pipe", "pipe"],
		},
	);
	timing("engine.spawn", spawnStartedAt, { endpoint });

	engine.stdout?.on("data", (chunk) => {
		const text = chunk.toString();
		logs.stdout += text;
		if (process.env.DRIVER_ENGINE_LOGS === "1") {
			process.stderr.write(`[ENG.OUT] ${text}`);
		}
	});
	engine.stderr?.on("data", (chunk) => {
		const text = chunk.toString();
		logs.stderr += text;
		if (process.env.DRIVER_ENGINE_LOGS === "1") {
			process.stderr.write(`[ENG.ERR] ${text}`);
		}
	});

	try {
		const healthStartedAt = performance.now();
		await waitForEngineHealth(engine, logs, endpoint, 90_000);
		timing("engine.health", healthStartedAt, { endpoint });
	} catch (error) {
		await stopRuntime(engine);
		rmSync(dbRoot, { force: true, recursive: true });
		throw error;
	}

	if (engine.pid === undefined) {
		await stopRuntime(engine);
		rmSync(dbRoot, { force: true, recursive: true });
		throw new Error("shared engine started without a pid");
	}

	const sharedEngine = {
		endpoint,
		metricsEndpoint,
		pid: engine.pid,
		dbRoot,
	};
	timing("engine.start_total", startedAt, { endpoint });
	return sharedEngine;
}

export async function getOrStartSharedTestEngine(
	options: SharedTestEngineOptions = {},
): Promise<SharedTestEngine> {
	const profile = resolveSharedEngineProfile(options);
	if (sharedEnginePromise !== undefined) {
		if (sharedEngineProfile !== profile) {
			throw new Error(
				`shared engine already acquired with profile ${sharedEngineProfile}; requested ${profile}`,
			);
		}
		return sharedEnginePromise;
	}
	const files = resolveSharedEngineFiles(profile);
	sharedEngineProfile = profile;
	sharedEngineFiles = files;

	sharedEnginePromise = (async () => {
		const releaseStartLock = await acquireEngineStartLock(files);
		try {
			const existing = readSharedEngineState(files);
			if (
				existing &&
				isPidRunning(existing.pid) &&
				(await isEngineHealthy(existing.endpoint))
			) {
				const state = { ...existing, refs: existing.refs + 1 };
				writeSharedEngineState(files, state);
				sharedEngineRefAcquired = true;
				timing("engine.reuse", performance.now(), {
					endpoint: existing.endpoint,
				});
				return {
					endpoint: existing.endpoint,
					metricsEndpoint: existing.metricsEndpoint,
					pid: existing.pid,
					dbRoot: existing.dbRoot,
				};
			}

			if (existing) {
				await stopPid(existing.pid, 5_000);
				rmSync(existing.dbRoot, { force: true, recursive: true });
				removeSharedEngineState(files);
			}

			const engine = await spawnSharedEngine(options);
			writeSharedEngineState(files, { ...engine, refs: 1 });
			sharedEngineRefAcquired = true;
			return engine;
		} catch (error) {
			sharedEnginePromise = undefined;
			sharedEngineProfile = undefined;
			sharedEngineFiles = undefined;
			throw error;
		} finally {
			releaseStartLock();
		}
	})();

	return sharedEnginePromise;
}

export async function releaseSharedTestEngine(): Promise<void> {
	if (!sharedEngineRefAcquired) {
		return;
	}
	sharedEngineRefAcquired = false;
	sharedEnginePromise = undefined;
	const files = sharedEngineFiles;
	sharedEngineFiles = undefined;
	sharedEngineProfile = undefined;
	if (!files) {
		throw new Error("shared engine files missing for acquired reference");
	}

	const releaseStartLock = await acquireEngineStartLock(files);
	const startedAt = performance.now();
	try {
		const state = readSharedEngineState(files);
		if (!state) {
			return;
		}

		const refs = Math.max(0, state.refs - 1);
		if (refs > 0) {
			writeSharedEngineState(files, { ...state, refs });
			return;
		}

		await stopPid(state.pid, 5_000);
		rmSync(state.dbRoot, { force: true, recursive: true });
		removeSharedEngineState(files);
		timing("engine.stop", startedAt, { endpoint: state.endpoint });
	} finally {
		releaseStartLock();
	}
}
