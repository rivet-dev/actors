import { actor } from "rivetkit";
import { db } from "rivetkit/db";

// Seeding vessel for the "large uncompacted SQLite DB" load test.
//
// The old engine build under test has hot compaction disabled, so every
// commit accumulates as an uncompacted delta chain in FDB that never folds.
// This actor grows a persistent SQLite database with append-only inserts and
// one commit per batch, producing both large page volume and many deltas.
//
// It deliberately avoids VACUUM, DELETE, and whole-database scans (no
// integrity_check) so the database only ever grows and each grow() call costs
// O(added bytes), not O(current size).

interface GrowInput {
	// Target total on-disk size in mebibytes. grow() is idempotent to this
	// target: calling it again after the target is reached is a cheap no-op.
	targetMb?: number;
	// Rows inserted per committed batch. Each batch is one BEGIN/COMMIT, so
	// this controls commit (delta) granularity.
	batchRows?: number;
	// Payload bytes per row, generated server-side via randomblob().
	rowBytes?: number;
	// Wall-clock budget for a single grow() call in milliseconds. When the
	// budget is exceeded the call returns done=false so the driver can resume
	// with another call. Keeps a single action under the action timeout.
	budgetMs?: number;
}

interface StorageStats {
	pageCount: number;
	pageSize: number;
	freelistCount: number;
	sizeBytes: number;
}

interface GrowResult extends StorageStats {
	done: boolean;
	targetBytes: number;
	batchesThisCall: number;
	rowsThisCall: number;
	elapsedMs: number;
}

const DEFAULT_TARGET_MB = 64;
// 64 rows x 8 KiB payload dirties ~130 SQLite pages per commit, comfortably
// under the depot MAX_COMMIT_DIRTY_PAGES (320) cap and giving one uncompacted
// FDB delta per batch.
const DEFAULT_BATCH_ROWS = 64;
const DEFAULT_ROW_BYTES = 8 * 1024;
const DEFAULT_BUDGET_MS = 120_000;
const MEBIBYTE = 1024 * 1024;

function finiteInt(value: number | undefined, fallback: number): number {
	if (value === undefined) return fallback;
	if (!Number.isFinite(value) || value <= 0) {
		throw new Error(`expected a positive finite number, got ${value}`);
	}
	return Math.floor(value);
}

async function queryOne<T>(
	database: { execute: (sql: string, ...args: unknown[]) => Promise<unknown[]> },
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

export const growDb = actor({
	options: {
		// Generous per-action timeout. grow() self-limits with budgetMs and
		// returns done=false before this fires.
		actionTimeout: 300_000,
	},
	db: db({
		onMigrate: async (database) => {
			await database.execute(`
				CREATE TABLE IF NOT EXISTS grow_rows (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					batch INTEGER NOT NULL,
					payload BLOB NOT NULL,
					created_at INTEGER NOT NULL
				)
			`);
		},
	}),
	actions: {
		grow: async (c, input: GrowInput = {}): Promise<GrowResult> => {
			const startedAt = performance.now();
			const targetMb = finiteInt(input.targetMb, DEFAULT_TARGET_MB);
			const batchRows = finiteInt(input.batchRows, DEFAULT_BATCH_ROWS);
			const rowBytes = finiteInt(input.rowBytes, DEFAULT_ROW_BYTES);
			const budgetMs = finiteInt(input.budgetMs, DEFAULT_BUDGET_MS);
			const targetBytes = targetMb * MEBIBYTE;

			const placeholders = new Array(batchRows)
				.fill("(?, randomblob(?), ?)")
				.join(", ");

			let batchesThisCall = 0;
			let rowsThisCall = 0;
			let stats = await storageStats(c.db);

			while (stats.sizeBytes < targetBytes) {
				if (performance.now() - startedAt >= budgetMs) {
					return {
						...stats,
						done: false,
						targetBytes,
						batchesThisCall,
						rowsThisCall,
						elapsedMs: Math.round(performance.now() - startedAt),
					};
				}

				const now = Date.now();
				const args: unknown[] = [];
				for (let i = 0; i < batchRows; i += 1) {
					args.push(batchesThisCall, rowBytes, now);
				}

				// One commit per batch so each iteration produces exactly one
				// uncompacted delta in FDB.
				await c.db.execute("BEGIN");
				try {
					await c.db.execute(
						`INSERT INTO grow_rows (batch, payload, created_at) VALUES ${placeholders}`,
						...args,
					);
					await c.db.execute("COMMIT");
				} catch (err) {
					await c.db.execute("ROLLBACK").catch(() => undefined);
					throw err;
				}

				batchesThisCall += 1;
				rowsThisCall += batchRows;
				stats = await storageStats(c.db);
			}

			return {
				...stats,
				done: true,
				targetBytes,
				batchesThisCall,
				rowsThisCall,
				elapsedMs: Math.round(performance.now() - startedAt),
			};
		},
		stats: async (c): Promise<StorageStats> => storageStats(c.db),
	},
});
