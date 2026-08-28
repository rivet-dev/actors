#!/usr/bin/env -S pnpm exec tsx

// Runs PRAGMA integrity_check against every seeded growDb actor database.
//
// This is the correctness gate for depot compaction changes. A fold or install
// bug shows up as pages that resolve to the wrong version rather than as an
// error, so the only way to see it is to read the whole database back through
// the depot VFS and let SQLite validate its own b-trees.
//
// Key construction mirrors seed-large-dbs.ts, so SEED_COUNT / SEED_KEY_PREFIX /
// SEED_RUN_ID must match the seed run being verified.
//
//   RIVET_ENDPOINT="http://default:dev@127.0.0.1:6420" RIVET_POOL=k8s \
//   SEED_COUNT=24 SEED_RUN_ID=bias1 node --import tsx scripts/integrity-sweep.ts

import { createClient } from "rivetkit/client";
import type { registry } from "../src/index.ts";

function envNum(name: string, fallback: number): number {
	const raw = process.env[name];
	if (raw === undefined || raw === "") return fallback;
	const value = Number(raw);
	if (!Number.isFinite(value) || value <= 0) {
		throw new Error(`${name} must be a positive number, got ${raw}`);
	}
	return Math.floor(value);
}

async function main(): Promise<void> {
	const endpoint = process.env.RIVET_ENDPOINT;
	if (!endpoint) throw new Error("RIVET_ENDPOINT is required");
	const count = envNum("SEED_COUNT", 24);
	const concurrency = envNum("SWEEP_CONCURRENCY", 4);
	const keyPrefix = process.env.SEED_KEY_PREFIX ?? "grow";
	const runId = process.env.SEED_RUN_ID ?? "bias1";

	const client = createClient<typeof registry>(endpoint);
	// A serverful runner only holds so many live actors, and this sweep leaves each
	// one resident, so a large herd has to be swept in batches with a fresh runner
	// between them. SEED_START is the first index of the batch.
	const start = Number(process.env.SEED_START ?? 0);
	const keys = Array.from(
		{ length: count },
		(_, i) => `${keyPrefix}-${runId}-${String(start + i).padStart(6, "0")}`,
	);

	let next = 0;
	let failed = 0;
	const results: unknown[] = [];

	async function worker(): Promise<void> {
		while (true) {
			const index = next++;
			if (index >= keys.length) return;
			const key = keys[index]!;
			const startedAt = Date.now();
			try {
				const out = await client.growDb.getOrCreate(key).integrityCheck();
				if (!out.ok) failed += 1;
				const record = {
					event: "db_checked",
					key,
					ok: out.ok,
					result: out.result.slice(0, 400),
					rows: out.rows,
					pageCount: out.pageCount,
					sizeMib: Math.round((out.sizeBytes / (1024 * 1024)) * 10) / 10,
					elapsedMs: Date.now() - startedAt,
				};
				results.push(record);
				console.log(JSON.stringify(record));
			} catch (err) {
				failed += 1;
				const record = {
					event: "db_error",
					key,
					ok: false,
					error: err instanceof Error ? err.message : String(err),
					elapsedMs: Date.now() - startedAt,
				};
				results.push(record);
				console.log(JSON.stringify(record));
			}
		}
	}

	await Promise.all(
		Array.from({ length: Math.min(concurrency, keys.length) }, () => worker()),
	);

	console.log(
		JSON.stringify({
			event: "sweep_end",
			checked: results.length,
			failed,
			ok: failed === 0,
		}),
	);
	if (failed > 0) process.exitCode = 1;
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
