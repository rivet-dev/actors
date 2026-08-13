#!/usr/bin/env -S pnpm exec tsx

// Seeds churnDb actors with deep overwrite delta chains for the compaction
// read-hot-shard load test (spec H2). Each actor rewrites a small bounded
// working set in place until it accumulates a target committed transaction
// (delta) count.
//
// Read-hotspot shape (see churn-db.ts): keep each branch's total DELTA under
// one FDB shard (< ~500 MB) so the compactor stage read cannot fan out across
// shards, while the chain stays deep (25k+ txids). Seed MANY such branches
// (CHURN_COUNT in the hundreds), not a few whales, so many single-shard stage
// reads run concurrently once compaction is enabled. Confirm the shape with
// fdb_subspace_sizes.py: each seeded branch should show meaningful DELTA on a
// single-digit shard count.
//
// The engine under test has hot compaction disabled while seeding, so every
// commit accumulates as an uncompacted delta chain in FDB. This driver only
// issues action calls; the actual SQLite writes happen on the serverful
// kitchen-sink runners the engine routes to (pool "k8s").
//
// Seed only a few of these (COUNT small) alongside the H1 append-only herd
// from seed-large-dbs.ts. After deploying the compaction-enabled build, re-run
// with a slightly higher CHURN_TARGET_TXIDS to "bump" each whale (force one
// commit) and spawn the depot_db_manager -> hot compactor workflows.
//
// Usage (in-cluster Job or laptop):
//   RIVET_ENDPOINT="http://default:<token>@rivet-engine.rivet-engine.svc.cluster.local:6420" \
//   CHURN_COUNT=3 CHURN_TARGET_TXIDS=18000 CHURN_CONCURRENCY=3 \
//   node --import tsx scripts/seed-churn-dbs.ts
//
// All knobs are env vars (flags also accepted):
//   RIVET_ENDPOINT          engine guard endpoint incl. namespace:token userinfo
//   CHURN_COUNT             number of distinct whale DBs to create (default 3)
//   CHURN_TARGET_TXIDS      target committed-txn (delta) count per DB (default 18000)
//   CHURN_WORKING_SET_ROWS  rows in the bounded working set (default 64)
//   CHURN_ROW_BYTES         payload bytes per row (default 8192)
//   CHURN_CONCURRENCY       max DBs churned in parallel (default = count)
//   CHURN_KEY_PREFIX        actor key prefix (default "churn")
//   CHURN_RUN_ID            run tag mixed into keys (default timestamp)
//   CHURN_MAX_CALLS         max churn() calls per DB before giving up (default 1000)

import { createClient } from "rivetkit/client";
import type { registry } from "../src/index.ts";

interface Args {
	endpoint: string;
	count: number;
	targetTxids: number;
	workingSetRows: number;
	rowBytes: number;
	concurrency: number;
	keyPrefix: string;
	runId: string;
	maxCalls: number;
	commitDelayMs: number;
	budgetMs?: number;
}

function envNumNonNeg(name: string, fallback: number): number {
	const raw = process.env[name];
	if (raw === undefined || raw === "") return fallback;
	const value = Number(raw);
	if (!Number.isFinite(value) || value < 0) {
		throw new Error(`${name} must be a non-negative number, got ${raw}`);
	}
	return Math.floor(value);
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
		throw new Error(
			"missing endpoint: set RIVET_ENDPOINT or pass --endpoint",
		);
	}
	const runId =
		flag(argv, "--run-id") ??
		process.env.CHURN_RUN_ID ??
		new Date().toISOString().replace(/[:.]/g, "-");
	const count = Number(flag(argv, "--count")) || envNum("CHURN_COUNT", 3);
	return {
		endpoint,
		count,
		targetTxids:
			Number(flag(argv, "--target-txids")) ||
			envNum("CHURN_TARGET_TXIDS", 25_000),
		workingSetRows:
			Number(flag(argv, "--working-set-rows")) ||
			envNum("CHURN_WORKING_SET_ROWS", 16),
		rowBytes:
			Number(flag(argv, "--row-bytes")) ||
			envNum("CHURN_ROW_BYTES", 256),
		concurrency:
			Number(flag(argv, "--concurrency")) ||
			envNum("CHURN_CONCURRENCY", count),
		keyPrefix:
			flag(argv, "--key-prefix") ??
			process.env.CHURN_KEY_PREFIX ??
			"churn",
		runId,
		maxCalls:
			Number(flag(argv, "--max-calls")) ||
			envNum("CHURN_MAX_CALLS", 1000),
		commitDelayMs: envNumNonNeg("CHURN_COMMIT_DELAY_MS", 25),
		budgetMs: process.env.CHURN_BUDGET_MS
			? envNum("CHURN_BUDGET_MS", 120_000)
			: undefined,
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
	txidCount: number;
	calls: number;
	error?: string;
}

async function churnOne(
	client: ReturnType<typeof createClient<typeof registry>>,
	args: Args,
	index: number,
): Promise<DbOutcome> {
	const key = `${args.keyPrefix}-${args.runId}-${String(index).padStart(6, "0")}`;
	const handle = client.churnDb.getOrCreate(key);
	let calls = 0;
	let last = { done: false, sizeBytes: 0, pageCount: 0, txidCount: 0 };
	try {
		while (!last.done && calls < args.maxCalls) {
			const result = await handle.churn({
				targetTxids: args.targetTxids,
				workingSetRows: args.workingSetRows,
				rowBytes: args.rowBytes,
				commitDelayMs: args.commitDelayMs,
				...(args.budgetMs !== undefined
					? { budgetMs: args.budgetMs }
					: {}),
			});
			calls += 1;
			last = {
				done: result.done,
				sizeBytes: result.sizeBytes,
				pageCount: result.pageCount,
				txidCount: result.txidCount,
			};
		}
		return {
			key,
			ok: last.done,
			sizeBytes: last.sizeBytes,
			pageCount: last.pageCount,
			txidCount: last.txidCount,
			calls,
			...(last.done ? {} : { error: `not done after ${calls} calls` }),
		};
	} catch (err) {
		return {
			key,
			ok: false,
			sizeBytes: last.sizeBytes,
			pageCount: last.pageCount,
			txidCount: last.txidCount,
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
			event: "churn_start",
			endpoint: endpointForLog,
			count: args.count,
			targetTxids: args.targetTxids,
			workingSetRows: args.workingSetRows,
			rowBytes: args.rowBytes,
			commitDelayMs: args.commitDelayMs,
			concurrency: args.concurrency,
			runId: args.runId,
		}),
	);

	const outcomes: DbOutcome[] = new Array(args.count);
	let next = 0;
	let completed = 0;
	let totalTxids = 0;
	const startedAt = Date.now();

	async function worker(): Promise<void> {
		while (true) {
			const index = next;
			if (index >= args.count) return;
			next += 1;
			const outcome = await churnOne(client, args, index);
			outcomes[index] = outcome;
			completed += 1;
			totalTxids += outcome.txidCount;
			console.log(
				JSON.stringify({
					event: "churn_db_done",
					key: outcome.key,
					ok: outcome.ok,
					size: fmtBytes(outcome.sizeBytes),
					pageCount: outcome.pageCount,
					txidCount: outcome.txidCount,
					calls: outcome.calls,
					completed,
					total: args.count,
					...(outcome.error ? { error: outcome.error } : {}),
				}),
			);
		}
	}

	await Promise.all(
		new Array(Math.min(args.concurrency, args.count))
			.fill(0)
			.map(() => worker()),
	);

	const ok = outcomes.filter((o) => o.ok).length;
	const failed = outcomes.filter((o) => !o.ok);
	console.log(
		JSON.stringify({
			event: "churn_end",
			ok,
			failed: failed.length,
			total: args.count,
			totalTxids,
			elapsedSec: Math.round((Date.now() - startedAt) / 1000),
			failures: failed
				.slice(0, 20)
				.map((o) => ({ key: o.key, error: o.error })),
		}),
	);

	if (failed.length > 0) process.exitCode = 1;
}

main().catch((err) => {
	console.error(
		JSON.stringify({
			event: "churn_fatal",
			error:
				err instanceof Error ? (err.stack ?? err.message) : String(err),
		}),
	);
	process.exit(1);
});
