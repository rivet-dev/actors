import { DatabaseSync } from "node:sqlite";
import type { Ctx } from "../../src/actors/shared.ts";

/**
 * A node:sqlite-backed implementation of the actor `Ctx.db` surface used by the
 * extracted SQL helpers (dispatcher queue ops, event materialization, hook and
 * coordinator index transitions). Lets those helpers be unit-tested without an
 * engine, against real SQLite semantics (RETURNING, partial unique indexes,
 * BLOB round-trips).
 */
export type TestDb = Ctx["db"];

export type MigrateFn = (database: {
	execute(sql: string, ...args: unknown[]): Promise<unknown>;
}) => Promise<void> | void;

export type TestCtx = Ctx & { db: TestDb; raw: DatabaseSync; close(): void };

function makeExecute(raw: DatabaseSync) {
	return async (sql: string, ...args: unknown[]) => {
		const stmt = raw.prepare(sql);
		// node:sqlite `.all()` works for SELECT, RETURNING, and DML alike
		// (returning [] when there are no result rows).
		return stmt.all(...(args as never[])) as Record<string, unknown>[];
	};
}

export async function makeCtx(onMigrate: MigrateFn): Promise<TestCtx> {
	const raw = new DatabaseSync(":memory:");
	const execute = makeExecute(raw);
	await onMigrate({ execute });
	let transactionTail = Promise.resolve();
	const db: TestDb = {
		execute,
		transaction: async (callback) => {
			const previous = transactionTail;
			let release!: () => void;
			transactionTail = new Promise<void>((resolve) => {
				release = resolve;
			});
			await previous;
			try {
				raw.exec("BEGIN IMMEDIATE");
				try {
					const result = await callback(db);
					raw.exec("COMMIT");
					return result;
				} catch (error) {
					raw.exec("ROLLBACK");
					throw error;
				}
			} finally {
				release();
			}
		},
	};
	return {
		db,
		raw,
		close: () => raw.close(),
	};
}
