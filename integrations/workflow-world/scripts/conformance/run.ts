import { spawn, type ChildProcess } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { getEnginePath } from "@rivetkit/engine-cli";

const TOKEN = "dev";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const PNPM_BIN = process.platform === "win32" ? "pnpm.cmd" : "pnpm";

type ManagedProcess = {
	child: ChildProcess;
	output: () => string;
};

type EngineProcess = ManagedProcess & {
	endpoint: string;
	dbRoot: string;
};

function log(message: string) {
	console.log(`[conformance] ${message}`);
}

function sleep(ms: number) {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitFor(
	label: string,
	predicate: () => Promise<boolean>,
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
			const port = address.port;
			server.close(() => resolvePromise(port));
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
	const dbRoot = mkdtempSync(join(tmpdir(), "workflow-world-rivet-conf-"));
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
			display_name: `Conformance ${namespace}`,
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

async function runVitest(
	endpoint: string,
	namespace: string,
	poolName: string,
	args: string[],
) {
	const child = spawn(
		PNPM_BIN,
		["exec", "vitest", "run", "tests/conformance.test.ts", ...args],
		{
			cwd: ROOT,
			env: {
				...process.env,
				RUN_WORKFLOW_CONFORMANCE: "1",
				RIVET_TOKEN: TOKEN,
				RIVET_ENDPOINT: endpoint,
				RIVET_NAMESPACE: namespace,
				RIVET_POOL: poolName,
			},
			stdio: "inherit",
		},
	);
	const code = await new Promise<number | null>((resolvePromise) =>
		child.once("exit", resolvePromise),
	);
	if (code !== 0) {
		throw new Error(`Vitest exited with code ${code}`);
	}
}

async function run() {
	const engine = await startEngine();
	const namespace = `conf-${randomUUID()}`;
	const poolName = `conf-${randomUUID()}`;

	try {
		await createNamespace(engine.endpoint, namespace);
		await upsertRunnerConfig(engine.endpoint, namespace, poolName);
		await runVitest(
			engine.endpoint,
			namespace,
			poolName,
			process.argv.slice(2),
		);
	} finally {
		await stopProcess(engine.child, "engine");
		rmSync(engine.dbRoot, { force: true, recursive: true });
	}
}

run().catch((error) => {
	console.error("[conformance] FAILED");
	console.error(error);
	process.exitCode = 1;
});
