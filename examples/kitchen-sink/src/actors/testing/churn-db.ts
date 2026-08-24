import { actor } from "rivetkit";
import { db } from "rivetkit/db";

// Seeding vessel for the "deep overwrite delta chain" compaction read-hot-shard
// load test (spec H2).
//
// Unlike growDb (append-only, live size == delta total), churnDb repeatedly
// UPDATEs a small, bounded working set in place. Every commit rewrites the same
// few pages, so the live SHARD/PIDX range stays tiny while each commit still
// encodes a fresh DELTA/{txid} blob.
//
// The read-hotspot shape (proven necessary by the 2026-07-10 prod subspace
// scan): the branch's whole contiguous DELTA range must stay SMALL enough to
// live on ONE FDB shard (< ~500 MB) while the chain is DEEP (tens of thousands
// of txids). Then the compactor's stage read of that branch cannot fan out
// across shards. Every slice lands on the same shard's replica procs. The
// earlier 18 GiB "whale" shape was wrong: DD split it across ~450 shards and
// the reads spread, so it never pinned a proc. The fix here is small per-commit
// DELTA (tiny working set, small rows) plus high targetTxids for depth, which
// keeps the total contiguous DELTA under one shard.
//
// It deliberately avoids VACUUM, DELETE, and whole-database scans so the
// database never grows and each churn() call costs O(working set), not
// O(delta chain depth).

interface ChurnInput {
	// Number of rows in the bounded working set that is rewritten every commit.
	// Small on purpose: the whole live database stays within one FDB shard.
	workingSetRows?: number;
	// Payload bytes per row, regenerated server-side via randomblob() on every
	// commit so the pages are always dirtied (no no-op page elision).
	rowBytes?: number;
	// Target cumulative committed-transaction count. churn() is resumable: it
	// persists the running count and stops once this target is reached, so the
	// driver can drive a whale to a fixed delta-chain depth across many calls.
	// Raising the target on a later call resumes churning (this is the "bump"
	// that fires compaction after the compaction-enabled build is deployed).
	targetTxids?: number;
	// Wall-clock budget for a single churn() call in milliseconds. When the
	// budget is exceeded the call returns done=false so the driver can resume.
	budgetMs?: number;
	// Delay in milliseconds between commits. Throttles the per-whale write rate
	// so the depot sqlite_commit path (which waits on FDB durability) does not
	// outrun the storage servers. Each whale appends DELTA/{txid} to one
	// contiguous txid-ordered tail, so unthrottled rapid churn is an
	// append-hotspot whose durability lag exceeds the commit timeout. A small
	// delay keeps the tail shard's durability lag under that bound.
	commitDelayMs?: number;
}

interface StorageStats {
	pageCount: number;
	pageSize: number;
	freelistCount: number;
	sizeBytes: number;
}

interface ChurnResult extends StorageStats {
	done: boolean;
	txidCount: number;
	targetTxids: number;
	commitsThisCall: number;
	elapsedMs: number;
}

// Sub-shard-deep shape: a tiny working set (few dirty pages per commit) over a
// deep chain keeps the whole DELTA range under one FDB shard. At ~12-16 KiB of
// DELTA per commit, 25k txids is ~300-400 MB, i.e. one shard. Verify the
// resulting per-branch DELTA/shard-count with fdb_subspace_sizes.py after
// seeding and tune targetTxids to stay single-shard.
const DEFAULT_WORKING_SET_ROWS = 16;
const DEFAULT_ROW_BYTES = 256;
const DEFAULT_TARGET_TXIDS = 25_000;
const DEFAULT_BUDGET_MS = 120_000;
const DEFAULT_COMMIT_DELAY_MS = 25;

function nonNegativeInt(value: number | undefined, fallback: number): number {
	if (value === undefined) return fallback;
	if (!Number.isFinite(value) || value < 0) {
		throw new Error(`expected a non-negative finite number, got ${value}`);
	}
	return Math.floor(value);
}

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

function finiteInt(value: number | undefined, fallback: number): number {
	if (value === undefined) return fallback;
	if (!Number.isFinite(value) || value <= 0) {
		throw new Error(`expected a positive finite number, got ${value}`);
	}
	return Math.floor(value);
}

async function queryOne<T>(
	database: {
		execute: (sql: string, ...args: unknown[]) => Promise<unknown[]>;
	},
	sql: string,
): Promise<T> {
	const rows = await database.execute(sql);
	if (!rows[0]) throw new Error(`query returned no rows: ${sql}`);
	return rows[0] as T;
}

async function storageStats(database: {
	execute: (sql: string, ...args: unknown[]) => Promise<unknown[]>;
}): Promise<StorageStats> {
	const [pageCount, pageSize, freelistCount] = await Promise.all([
		queryOne<{ page_count: number }>(database, "PRAGMA page_count"),
		queryOne<{ page_size: number }>(database, "PRAGMA page_size"),
		queryOne<{ freelist_count: number }>(database, "PRAGMA freelist_count"),
	]);
	return {
		pageCount: pageCount.page_count,
		pageSize: pageSize.page_size,
		freelistCount: freelistCount.freelist_count,
		sizeBytes: pageCount.page_count * pageSize.page_size,
	};
}

// Lazily seed the bounded working set. Runs once per database as a single small
// commit; subsequent calls are cheap no-ops once the rows exist.
async function ensureWorkingSet(
	database: {
		execute: (sql: string, ...args: unknown[]) => Promise<unknown[]>;
	},
	workingSetRows: number,
	rowBytes: number,
): Promise<void> {
	const existing = await queryOne<{ n: number }>(
		database,
		"SELECT COUNT(*) AS n FROM churn_rows",
	);
	if (existing.n >= workingSetRows) return;

	await database.execute("BEGIN");
	try {
		for (let id = existing.n + 1; id <= workingSetRows; id += 1) {
			await database.execute(
				"INSERT INTO churn_rows (id, payload, rev) VALUES (?, randomblob(?), 0)",
				id,
				rowBytes,
			);
		}
		await database.execute("COMMIT");
	} catch (err) {
		await database.execute("ROLLBACK").catch(() => undefined);
		throw err;
	}
}

export const churnDb = actor({
	options: {
		// Generous per-action timeout. churn() self-limits with budgetMs and
		// returns done=false before this fires.
		actionTimeout: 300_000,
	},
	db: db({
		onMigrate: async (database) => {
			// Bounded working set rewritten in place on every commit.
			await database.execute(`
				CREATE TABLE IF NOT EXISTS churn_rows (
					id INTEGER PRIMARY KEY,
					payload BLOB NOT NULL,
					rev INTEGER NOT NULL
				)
			`);
			// Single-row durable counter of committed churn transactions. Bumped
			// inside each churn commit so the delta-chain depth is recoverable
			// across calls without scanning history.
			await database.execute(`
				CREATE TABLE IF NOT EXISTS churn_meta (
					id INTEGER PRIMARY KEY CHECK (id = 0),
					txid_count INTEGER NOT NULL
				)
			`);
			await database.execute(
				"INSERT OR IGNORE INTO churn_meta (id, txid_count) VALUES (0, 0)",
			);
		},
	}),
	actions: {
		churn: async (c, input: ChurnInput = {}): Promise<ChurnResult> => {
			const startedAt = performance.now();
			const workingSetRows = finiteInt(
				input.workingSetRows,
				DEFAULT_WORKING_SET_ROWS,
			);
			const rowBytes = finiteInt(input.rowBytes, DEFAULT_ROW_BYTES);
			const targetTxids = finiteInt(
				input.targetTxids,
				DEFAULT_TARGET_TXIDS,
			);
			const budgetMs = finiteInt(input.budgetMs, DEFAULT_BUDGET_MS);
			const commitDelayMs = nonNegativeInt(
				input.commitDelayMs,
				DEFAULT_COMMIT_DELAY_MS,
			);

			await ensureWorkingSet(c.db, workingSetRows, rowBytes);

			let { txid_count: txidCount } = await queryOne<{
				txid_count: number;
			}>(c.db, "SELECT txid_count FROM churn_meta WHERE id = 0");
			let commitsThisCall = 0;

			while (txidCount < targetTxids) {
				if (performance.now() - startedAt >= budgetMs) {
					const stats = await storageStats(c.db);
					return {
						...stats,
						done: false,
						txidCount,
						targetTxids,
						commitsThisCall,
						elapsedMs: Math.round(performance.now() - startedAt),
					};
				}

				// One BEGIN/UPDATE/COMMIT rewrites the entire working set and
				// bumps the counter, producing exactly one uncompacted DELTA in
				// FDB over the same fixed page range.
				await c.db.execute("BEGIN");
				try {
					await c.db.execute(
						"UPDATE churn_rows SET payload = randomblob(?), rev = rev + 1",
						rowBytes,
					);
					await c.db.execute(
						"UPDATE churn_meta SET txid_count = txid_count + 1 WHERE id = 0",
					);
					await c.db.execute("COMMIT");
				} catch (err) {
					await c.db.execute("ROLLBACK").catch(() => undefined);
					throw err;
				}

				txidCount += 1;
				commitsThisCall += 1;

				// Throttle the write rate so the depot commit path does not
				// outrun FDB durability on the whale's append-hotspot tail shard.
				if (commitDelayMs > 0) await sleep(commitDelayMs);
			}

			const stats = await storageStats(c.db);
			return {
				...stats,
				done: true,
				txidCount,
				targetTxids,
				commitsThisCall,
				elapsedMs: Math.round(performance.now() - startedAt),
			};
		},
		stats: async (c): Promise<StorageStats & { txidCount: number }> => {
			const stats = await storageStats(c.db);
			const meta = await queryOne<{ txid_count: number }>(
				c.db,
				"SELECT txid_count FROM churn_meta WHERE id = 0",
			);
			return { ...stats, txidCount: meta.txid_count };
		},
	},
});
