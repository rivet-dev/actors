#!/usr/bin/env -S pnpm exec tsx

// Seeds many large, uncompacted actor SQLite databases for the compaction
// load test. Creates N unique growDb actor keys and grows each to a target
// size with append-only inserts (one commit per batch). Concurrency-limited.
//
// The engine under test has hot compaction disabled, so every commit
// accumulates as an uncompacted delta chain in FDB. This driver only issues
// action calls; the actual SQLite writes happen on the serverful kitchen-sink
// runners that the engine routes to (pool "k8s").
//
// Usage (in-cluster Job or laptop):
//   RIVET_ENDPOINT="http://default:<token>@rivet-engine.rivet-engine.svc.cluster.local:6420" \
//   SEED_COUNT=200 SEED_TARGET_MB=64 SEED_CONCURRENCY=16 \
//   node --import tsx scripts/seed-large-dbs.ts
//
// All knobs are env vars (flags also accepted):
//   RIVET_ENDPOINT        engine guard endpoint incl. namespace:token userinfo
//   SEED_COUNT            number of distinct actor DBs to create (default 100)
//   SEED_TARGET_MB        target size per DB in MiB (default 64)
//   SEED_BATCH_ROWS       rows per committed batch (default 128)
//   SEED_ROW_BYTES        payload bytes per row (default 8192)
//   SEED_CONCURRENCY      max DBs grown in parallel (default 16)
//   SEED_KEY_PREFIX       actor key prefix (default "grow")
//   SEED_RUN_ID           run tag mixed into keys (default timestamp)
//   SEED_MAX_CALLS        max grow() calls per DB before giving up (default 100)

import { createClient } from "rivetkit/client";
import type { registry } from "../src/index.ts";

interface Args {
	endpoint: string;
	count: number;
	targetMb: number;
	batchRows: number;
	rowBytes: number;
	concurrency: number;
	keyPrefix: string;
	runId: string;
	maxCalls: number;
}

function envNum(name: string, fallback: number): number {
	const raw = process.env[name];
	if (raw === undefined || raw === "") return fallback;
	const value = Number(raw);
	if (!Number.isFinite(value) || value <= 0) {
		throw new Error(`${name} must be a positive number, got ${raw}`);
	}
	return Math.floor(value);
}

function flag(argv: string[], name: string): string | undefined {
	const prefix = `${name}=`;
	const inline = argv.find((a) => a.startsWith(prefix));
	if (inline) return inline.slice(prefix.length);
	const idx = argv.indexOf(name);
	if (idx >= 0) return argv[idx + 1];
	return undefined;
}

function parseArgs(argv: string[]): Args {
	const endpoint = flag(argv, "--endpoint") ?? process.env.RIVET_ENDPOINT;
	if (!endpoint) {
		throw new Error("missing endpoint: set RIVET_ENDPOINT or pass --endpoint");
	}
	const runId =
		flag(argv, "--run-id") ??
		process.env.SEED_RUN_ID ??
		new Date().toISOString().replace(/[:.]/g, "-");
	return {
		endpoint,
		count: Number(flag(argv, "--count")) || envNum("SEED_COUNT", 100),
		targetMb: Number(flag(argv, "--target-mb")) || envNum("SEED_TARGET_MB", 64),
		batchRows:
			Number(flag(argv, "--batch-rows")) || envNum("SEED_BATCH_ROWS", 64),
		rowBytes: Number(flag(argv, "--row-bytes")) || envNum("SEED_ROW_BYTES", 8192),
		concurrency:
			Number(flag(argv, "--concurrency")) || envNum("SEED_CONCURRENCY", 16),
		keyPrefix: flag(argv, "--key-prefix") ?? process.env.SEED_KEY_PREFIX ?? "grow",
		runId,
		maxCalls: Number(flag(argv, "--max-calls")) || envNum("SEED_MAX_CALLS", 100),
	};
}

function fmtBytes(bytes: number): string {
	return `${(bytes / 1024 / 1024).toFixed(1)}MiB`;
}

interface DbOutcome {
	key: string;
	ok: boolean;
	sizeBytes: number;
	pageCount: number;
	calls: number;
	error?: string;
}

async function growOne(
	client: ReturnType<typeof createClient<typeof registry>>,
	args: Args,
	index: number,
): Promise<DbOutcome> {
	const key = `${args.keyPrefix}-${args.runId}-${String(index).padStart(6, "0")}`;
	const handle = client.growDb.getOrCreate(key);
	let calls = 0;
	let last = { done: false, sizeBytes: 0, pageCount: 0 };
	try {
		while (!last.done && calls < args.maxCalls) {
			const result = await handle.grow({
				targetMb: args.targetMb,
				batchRows: args.batchRows,
				rowBytes: args.rowBytes,
			});
			calls += 1;
			last = {
				done: result.done,
				sizeBytes: result.sizeBytes,
				pageCount: result.pageCount,
			};
		}
		return {
			key,
			ok: last.done,
			sizeBytes: last.sizeBytes,
			pageCount: last.pageCount,
			calls,
			...(last.done
				? {}
				: { error: `not done after ${calls} calls` }),
		};
	} catch (err) {
		return {
			key,
			ok: false,
			sizeBytes: last.sizeBytes,
			pageCount: last.pageCount,
			calls,
			error: err instanceof Error ? err.message : String(err),
		};
	}
}

async function main(): Promise<void> {
	const args = parseArgs(process.argv.slice(2));
	const client = createClient<typeof registry>(args.endpoint);

	const endpointForLog = args.endpoint.replace(/\/\/[^@]*@/, "//<redacted>@");
	console.log(
		JSON.stringify({
			event: "seed_start",
			endpoint: endpointForLog,
			count: args.count,
			targetMb: args.targetMb,
			batchRows: args.batchRows,
			rowBytes: args.rowBytes,
			concurrency: args.concurrency,
			runId: args.runId,
		}),
	);

	const outcomes: DbOutcome[] = new Array(args.count);
	let next = 0;
	let completed = 0;
	let totalBytes = 0;
	const startedAt = Date.now();

	async function worker(): Promise<void> {
		while (true) {
			const index = next;
			if (index >= args.count) return;
			next += 1;
			const outcome = await growOne(client, args, index);
			outcomes[index] = outcome;
			completed += 1;
			totalBytes += outcome.sizeBytes;
			console.log(
				JSON.stringify({
					event: "seed_db_done",
					key: outcome.key,
					ok: outcome.ok,
					size: fmtBytes(outcome.sizeBytes),
					pageCount: outcome.pageCount,
					calls: outcome.calls,
					completed,
					total: args.count,
					totalBytes: fmtBytes(totalBytes),
					...(outcome.error ? { error: outcome.error } : {}),
				}),
			);
		}
	}

	await Promise.all(
		new Array(Math.min(args.concurrency, args.count)).fill(0).map(() => worker()),
	);

	const ok = outcomes.filter((o) => o.ok).length;
	const failed = outcomes.filter((o) => !o.ok);
	console.log(
		JSON.stringify({
			event: "seed_end",
			ok,
			failed: failed.length,
			total: args.count,
			totalBytes: fmtBytes(totalBytes),
			elapsedSec: Math.round((Date.now() - startedAt) / 1000),
			failures: failed.slice(0, 20).map((o) => ({ key: o.key, error: o.error })),
		}),
	);

	if (failed.length > 0) process.exitCode = 1;
}

main().catch((err) => {
	console.error(
		JSON.stringify({
			event: "seed_fatal",
			error: err instanceof Error ? (err.stack ?? err.message) : String(err),
		}),
	);
	process.exit(1);
});
