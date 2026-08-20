import {
	type ChildProcess,
	type SpawnOptions,
	spawn,
} from "node:child_process";
import { randomUUID } from "node:crypto";
import {
	mkdirSync,
	mkdtempSync,
	rmSync,
	symlinkSync,
	type WriteFileOptions,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { type Client, createClient } from "../../src/client/mod";
import {
	getOrStartSharedTestEngine,
	releaseSharedTestEngine,
	type SharedTestEngine,
	TEST_ENGINE_TOKEN,
} from "../shared-engine";
import type { PlatformSqliteCounterRegistry } from "./shared-registry";

export const PLATFORM_TEST_TOKEN = TEST_ENGINE_TOKEN;
export const PLATFORM_TEST_LOGS_ENV = "RIVETKIT_PLATFORM_TEST_LOGS";

interface RuntimeLogs {
	stdout: string;
	stderr: string;
}

export interface PlatformServerlessRunner {
	endpoint: string;
	namespace: string;
	runnerName: string;
	token: string;
	serverlessUrl: string;
}

export interface PlatformServerlessRunnerOptions {
	engine: SharedTestEngine;
	namespace?: string;
	runnerName?: string;
	serverlessUrl: string;
	headers?: Record<string, string>;
	requestLifespan?: number;
	drainGracePeriod?: number;
	metadata?: Record<string, unknown>;
	metadataPollInterval?: number;
	drainOnVersionUpgrade?: boolean;
	maxRunners?: number;
	minRunners?: number;
	runnersMargin?: number;
	slotsPerRunner?: number;
}

export interface LoggedChild {
	child: ChildProcess;
	getOutput(): string;
	stop(signal?: NodeJS.Signals, timeoutMs?: number): Promise<void>;
}

export interface SpawnLoggedChildOptions {
	label: string;
	command: string;
	args?: string[];
	options?: SpawnOptions;
	logEnv?: string;
}

export interface SpawnPinnedPnpmDlxOptions
	extends Omit<SpawnLoggedChildOptions, "command" | "args"> {
	packageName: string;
	packageVersion: string;
	args?: string[];
}

export interface WaitForHttpOkOptions {
	url: string;
	timeoutMs?: number;
	intervalMs?: number;
	child?: ChildProcess;
	getOutput?: () => string;
}

export interface TempPlatformApp {
	path: string;
	writeFile(
		relativePath: string,
		contents: string | Uint8Array,
		options?: WriteFileOptions,
	): void;
	cleanup(): void;
}

type PlatformWasmInitMode = "cloudflare-module-import" | "deno-read-file";

// The shared docs-shaped SQLite counter actor. Used by every platform fixture so
// they differ only in platform bootstrapping (createHandler / serve / raw setup).
const PLATFORM_SQLITE_COUNTER_ACTOR_BODY = `interface SqliteDatabase {
\trun(sql: string, params?: unknown[]): Promise<void>;
\tquery(sql: string, params?: unknown[]): Promise<{ rows: unknown[][] }>;
}

const COUNTER_ID = 1;

const rawSqlDatabaseProvider = {
\tcreateClient: async (ctx: any) => {
\t\tif (!ctx.nativeDatabaseProvider) throw new Error("native SQLite is required");
\t\treturn await ctx.nativeDatabaseProvider.open(ctx.actorId);
\t},
\tonMigrate: async () => {},
};

async function ensureCounterTable(db: SqliteDatabase) {
\tawait db.run(
\t\t"CREATE TABLE IF NOT EXISTS platform_counter (id INTEGER PRIMARY KEY CHECK (id = 1), count INTEGER NOT NULL)",
\t);
}

async function ensureLifecycleTable(db: SqliteDatabase) {
\tawait db.run(
\t\t"CREATE TABLE IF NOT EXISTS platform_counter_lifecycle (event TEXT PRIMARY KEY, count INTEGER NOT NULL)",
\t);
}

async function recordLifecycleEvent(db: SqliteDatabase, event: string) {
\tawait ensureLifecycleTable(db);
\tawait db.run(
\t\t"INSERT INTO platform_counter_lifecycle (event, count) VALUES (?, 1) ON CONFLICT(event) DO UPDATE SET count = count + 1",
\t\t[event],
\t);
}

async function readCounter(db: SqliteDatabase): Promise<number> {
\tconst result = await db.query(
\t\t"SELECT count FROM platform_counter WHERE id = ?",
\t\t[COUNTER_ID],
\t);

\treturn Number(result.rows[0]?.[0] ?? 0);
}

async function readLifecycleCounts(db: SqliteDatabase): Promise<{
\twakeCount: number;
\tsleepCount: number;
}> {
\tawait ensureLifecycleTable(db);
\tconst result = await db.query(
\t\t"SELECT event, count FROM platform_counter_lifecycle",
\t);
\tconst counts = new Map(
\t\tresult.rows.map((row) => [String(row[0]), Number(row[1])]),
\t);

\treturn {
\t\twakeCount: counts.get("wake") ?? 0,
\t\tsleepCount: counts.get("sleep") ?? 0,
\t};
}

export const sqliteCounter = actor({
\tdb: rawSqlDatabaseProvider,
\tonWake: async (ctx) => {
\t\tawait recordLifecycleEvent(ctx.db as SqliteDatabase, "wake");
\t},
\tonSleep: async (ctx) => {
\t\tawait recordLifecycleEvent(ctx.db as SqliteDatabase, "sleep");
\t},
\tactions: {
\t\tincrement: async (ctx, amount = 1) => {
\t\t\tconst db = ctx.db as SqliteDatabase;
\t\t\tawait ensureCounterTable(db);
\t\t\tawait db.run(
\t\t\t\t"INSERT INTO platform_counter (id, count) VALUES (?, ?) ON CONFLICT(id) DO UPDATE SET count = count + excluded.count",
\t\t\t\t[COUNTER_ID, amount],
\t\t\t);

\t\t\treturn await readCounter(db);
\t\t},
\t\tgetCount: async (ctx) => {
\t\t\tconst db = ctx.db as SqliteDatabase;
\t\t\tawait ensureCounterTable(db);

\t\t\treturn await readCounter(db);
\t\t},
\t\tgetLifecycleCounts: async (ctx) => {
\t\t\treturn await readLifecycleCounts(ctx.db as SqliteDatabase);
\t\t},
\t\ttriggerSleep: (ctx) => {
\t\t\tctx.sleep();
\t\t},
\t},
\toptions: {
\t\tsleepTimeout: 100,
\t},
});
`;

// The shared actor as a standalone module, consumed by the Cloudflare Workers and
// Supabase Functions fixtures that hide the wasm runtime behind their packages.
export function buildPlatformSqliteCounterActorSource(): string {
	return `import { actor } from "rivetkit";

${PLATFORM_SQLITE_COUNTER_ACTOR_BODY}`;
}

// The raw-`setup()` registry source used by the Deno fixture, which drives the
// public wasm runtime API directly (Deno has a native outbound WebSocket).
export function buildPlatformSqliteCounterRegistrySource(
	wasmInitMode: PlatformWasmInitMode,
): string {
	const wasmModuleSource =
		wasmInitMode === "cloudflare-module-import"
			? 'import wasmModule from "@rivetkit/rivetkit-wasm/rivetkit_wasm_bg.wasm";'
			: 'const wasmModule = await Deno.readFile(new URL(import.meta.resolve("@rivetkit/rivetkit-wasm/rivetkit_wasm_bg.wasm")));';

	return `import { actor, setup } from "rivetkit";
import * as wasmBindings from "@rivetkit/rivetkit-wasm";
${wasmModuleSource}

interface RegistryConfig {
\tendpoint: string;
\tnamespace: string;
\trunnerName: string;
\ttoken: string;
\tserverless?: {
\t\tbasePath: string;
\t\tpublicEndpoint: string;
\t};
}

${PLATFORM_SQLITE_COUNTER_ACTOR_BODY}
export function createRegistry(config: RegistryConfig) {
\treturn setup({
\t\truntime: "wasm",
\t\tsqlite: "remote",
\t\twasm: {
\t\t\tbindings: wasmBindings,
\t\t\tinitInput: wasmModule,
\t\t},
\t\tuse: { sqliteCounter },
\t\tendpoint: config.endpoint,
\t\tnamespace: config.namespace,
\t\ttoken: config.token,
\t\tenvoy: {
\t\t\tpoolName: config.runnerName,
\t\t},
\t\t...(config.serverless ? { serverless: config.serverless } : {}),
\t\tnoWelcome: true,
\t});
}
`;
}

export function linkWorkspacePackage(
	app: TempPlatformApp,
	packageName: string,
	packagePath: string,
): void {
	const linkPath = resolve(
		app.path,
		"node_modules",
		...packageName.split("/"),
	);
	mkdirSync(dirname(linkPath), { recursive: true });
	rmSync(linkPath, { force: true, recursive: true });
	symlinkSync(packagePath, linkPath, "dir");
}

export async function getOrStartPlatformTestEngine(): Promise<SharedTestEngine> {
	return getOrStartSharedTestEngine();
}

export async function releasePlatformTestEngine(): Promise<void> {
	await releaseSharedTestEngine();
}

function childOutput(logs: RuntimeLogs): string {
	return [logs.stdout, logs.stderr].filter(Boolean).join("\n");
}

function appendChildLogs(
	logs: RuntimeLogs,
	stream: "stdout" | "stderr",
	label: string,
	chunk: Buffer,
	logEnv: string,
) {
	const text = chunk.toString();
	logs[stream] += text;

	if (process.env[logEnv] === "1") {
		process.stderr.write(`[${label}.${stream.toUpperCase()}] ${text}`);
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

	await new Promise<void>((resolveStop) => {
		const timeout = setTimeout(() => {
			if (child.exitCode === null) {
				child.kill("SIGKILL");
			}
		}, timeoutMs);

		child.once("exit", () => {
			clearTimeout(timeout);
			resolveStop();
		});

		child.kill(signal);
	});
}

export function spawnLoggedChild({
	label,
	command,
	args = [],
	options,
	logEnv = PLATFORM_TEST_LOGS_ENV,
}: SpawnLoggedChildOptions): LoggedChild {
	const logs: RuntimeLogs = { stdout: "", stderr: "" };
	const child = spawn(command, args, {
		...options,
		stdio: ["ignore", "pipe", "pipe"],
	});

	child.stdout?.on("data", (chunk) => {
		appendChildLogs(logs, "stdout", label, chunk, logEnv);
	});
	child.stderr?.on("data", (chunk) => {
		appendChildLogs(logs, "stderr", label, chunk, logEnv);
	});

	return {
		child,
		getOutput: () => childOutput(logs),
		stop: async (signal = "SIGTERM", timeoutMs = 5_000) => {
			await stopProcess(child, signal, timeoutMs);
		},
	};
}

export function buildPinnedPnpmDlxArgs(
	packageName: string,
	packageVersion: string,
	args: string[] = [],
): string[] {
	if (!packageVersion || packageVersion === "latest") {
		throw new Error(
			`platform CLI ${packageName} must use a pinned version`,
		);
	}

	return ["dlx", `${packageName}@${packageVersion}`, ...args];
}

export function spawnPinnedPnpmDlx({
	packageName,
	packageVersion,
	args = [],
	...options
}: SpawnPinnedPnpmDlxOptions): LoggedChild {
	return spawnLoggedChild({
		...options,
		command: "pnpm",
		args: buildPinnedPnpmDlxArgs(packageName, packageVersion, args),
	});
}

export function createTempPlatformApp(
	files: Record<string, string | Uint8Array> = {},
	prefix = "rivetkit-platform-",
): TempPlatformApp {
	const appPath = mkdtempSync(join(tmpdir(), prefix));

	const app: TempPlatformApp = {
		path: appPath,
		writeFile: (relativePath, contents, options) => {
			const filePath = resolve(appPath, relativePath);
			const rootPrefix = `${resolve(appPath)}${sep}`;
			if (
				filePath !== resolve(appPath) &&
				!filePath.startsWith(rootPrefix)
			) {
				throw new Error(
					`temp app file escapes app directory: ${relativePath}`,
				);
			}

			mkdirSync(dirname(filePath), { recursive: true });
			writeFileSync(filePath, contents, options);
		},
		cleanup: () => {
			rmSync(appPath, { force: true, recursive: true });
		},
	};

	for (const [relativePath, contents] of Object.entries(files)) {
		app.writeFile(relativePath, contents);
	}

	return app;
}

async function apiFetch(
	endpoint: string,
	path: string,
	init: RequestInit = {},
): Promise<Response> {
	const headers = new Headers(init.headers);
	headers.set("Authorization", `Bearer ${PLATFORM_TEST_TOKEN}`);

	return fetch(`${endpoint}${path}`, {
		...init,
		headers,
	});
}

export async function createPlatformNamespace(
	engine: SharedTestEngine,
	namespace = `platform-${randomUUID()}`,
): Promise<string> {
	const response = await apiFetch(engine.endpoint, "/namespaces", {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify({
			name: namespace,
			display_name: `Platform test ${namespace}`,
		}),
	});

	if (!response.ok) {
		throw new Error(
			`failed to create platform namespace ${namespace}: ${response.status} ${await response.text()}`,
		);
	}

	return namespace;
}

async function getFirstDatacenter(
	engine: SharedTestEngine,
	namespace: string,
): Promise<string> {
	const response = await apiFetch(
		engine.endpoint,
		`/datacenters?namespace=${encodeURIComponent(namespace)}`,
	);

	if (!response.ok) {
		throw new Error(
			`failed to list platform datacenters: ${response.status} ${await response.text()}`,
		);
	}

	const body = (await response.json()) as {
		datacenters: Array<{ name: string }>;
	};
	const datacenter = body.datacenters[0]?.name;
	if (!datacenter) {
		throw new Error("engine returned no platform datacenters");
	}

	return datacenter;
}

export async function createPlatformServerlessRunner({
	engine,
	namespace = `platform-${randomUUID()}`,
	runnerName = `platform-${randomUUID()}`,
	serverlessUrl,
	headers,
	requestLifespan,
	drainGracePeriod,
	metadata,
	metadataPollInterval,
	drainOnVersionUpgrade,
	maxRunners,
	minRunners,
	runnersMargin,
	slotsPerRunner,
}: PlatformServerlessRunnerOptions): Promise<PlatformServerlessRunner> {
	await createPlatformNamespace(engine, namespace);
	const datacenter = await getFirstDatacenter(engine, namespace);
	const deadline = Date.now() + 30_000;
	const upsertBody = {
		datacenters: {
			[datacenter]: {
				serverless: {
					url: serverlessUrl,
					headers: headers ?? {},
					request_lifespan: requestLifespan ?? 60 * 60,
					drain_grace_period: drainGracePeriod,
					metadata_poll_interval: metadataPollInterval ?? 1_000,
					max_runners: maxRunners ?? 100_000,
					min_runners: minRunners ?? 0,
					runners_margin: runnersMargin ?? 0,
					slots_per_runner: slotsPerRunner ?? 1,
				},
				metadata: metadata ?? {},
				drain_on_version_upgrade: drainOnVersionUpgrade ?? true,
			},
		},
	};

	while (true) {
		const response = await apiFetch(
			engine.endpoint,
			`/runner-configs/${encodeURIComponent(runnerName)}?namespace=${encodeURIComponent(namespace)}`,
			{
				method: "PUT",
				headers: {
					"Content-Type": "application/json",
				},
				body: JSON.stringify(upsertBody),
			},
		);

		if (response.ok) {
			break;
		}

		const responseBody = await response.text();
		if (
			Date.now() < deadline &&
			((response.status === 400 &&
				responseBody.includes('"group":"namespace"') &&
				responseBody.includes('"code":"not_found"')) ||
				(response.status === 500 &&
					responseBody.includes('"group":"core"') &&
					responseBody.includes('"code":"internal_error"')))
		) {
			await new Promise((resolveWait) => setTimeout(resolveWait, 500));
			continue;
		}

		throw new Error(
			`failed to upsert platform serverless runner ${runnerName}: ${response.status} ${responseBody}`,
		);
	}

	const bumpResponse = await apiFetch(
		engine.endpoint,
		`/runner-configs/${encodeURIComponent(runnerName)}?namespace=${encodeURIComponent(namespace)}`,
		{
			method: "PUT",
			headers: {
				"Content-Type": "application/json",
			},
			body: JSON.stringify(upsertBody),
		},
	);
	if (!bumpResponse.ok) {
		throw new Error(
			`failed to bump platform serverless runner ${runnerName}: ${bumpResponse.status} ${await bumpResponse.text()}`,
		);
	}

	return {
		endpoint: engine.endpoint,
		namespace,
		runnerName,
		token: PLATFORM_TEST_TOKEN,
		serverlessUrl,
	};
}

export function createPlatformSqliteCounterClient(
	runner: PlatformServerlessRunner,
): Client<PlatformSqliteCounterRegistry> {
	return createClient<PlatformSqliteCounterRegistry>({
		endpoint: runner.endpoint,
		namespace: runner.namespace,
		poolName: runner.runnerName,
		token: runner.token,
		disableMetadataLookup: true,
	});
}

export async function waitForHttpOk({
	url,
	timeoutMs = 30_000,
	intervalMs = 500,
	child,
	getOutput = () => "",
}: WaitForHttpOkOptions): Promise<void> {
	const deadline = Date.now() + timeoutMs;

	while (Date.now() < deadline) {
		if (child?.exitCode !== null && child?.exitCode !== undefined) {
			throw new Error(
				`platform process exited before health check passed:\n${getOutput()}`,
			);
		}

		try {
			const response = await fetch(url);
			if (response.ok) {
				return;
			}
		} catch {}

		await new Promise((resolveWait) => setTimeout(resolveWait, intervalMs));
	}

	throw new Error(
		`timed out waiting for platform health at ${url}:\n${getOutput()}`,
	);
}
