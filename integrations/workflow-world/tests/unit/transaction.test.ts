import { beforeEach, afterEach, describe, expect, test } from "vitest";
import {
	withActorTransaction,
	type Ctx,
} from "../../src/actors/shared.ts";
import { makeCtx, type TestCtx } from "../helpers/db.ts";

type TxCtx = Ctx & { raw: TestCtx["raw"]; close: () => void };

async function makeTxCtx(): Promise<TxCtx> {
	const base = await makeCtx(async (db) => {
		await db.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
	});
	return { db: base.db, raw: base.raw, close: base.close };
}

async function count(ctx: TxCtx) {
	return Number((await ctx.db.execute("SELECT COUNT(*) AS n FROM t"))[0].n);
}

describe("withActorTransaction", () => {
	let ctx: TxCtx;
	beforeEach(async () => {
		ctx = await makeTxCtx();
	});
	afterEach(() => ctx.close());

	test("commits all writes when fn succeeds", async () => {
		await withActorTransaction(ctx, async (tx) => {
			await tx.db.execute("INSERT INTO t (v) VALUES ('a')");
			await tx.db.execute("INSERT INTO t (v) VALUES ('b')");
		});
		expect(await count(ctx)).toBe(2);
	});

	test("rolls back ALL writes when fn throws mid-action (atomicity)", async () => {
		await expect(
			withActorTransaction(ctx, async (tx) => {
				await tx.db.execute("INSERT INTO t (v) VALUES ('a')");
				// Simulate a crash/failure after the first write but before the
				// second — without a transaction, 'a' would leak (partial state).
				throw new Error("boom");
			}),
		).rejects.toThrow("boom");
		expect(await count(ctx)).toBe(0);
	});

	test("serializes overlapping transactions (no nested BEGIN on one connection)", async () => {
		// Fire two transactions concurrently on the same actor database.
		const results = await Promise.allSettled([
			withActorTransaction(ctx, async (tx) => {
				await tx.db.execute("INSERT INTO t (v) VALUES ('x')");
				await new Promise((r) => setTimeout(r, 10));
				await tx.db.execute("INSERT INTO t (v) VALUES ('y')");
			}),
			withActorTransaction(ctx, async (tx) => {
				await tx.db.execute("INSERT INTO t (v) VALUES ('z')");
			}),
		]);
		expect(results.every((r) => r.status === "fulfilled")).toBe(true);
		expect(await count(ctx)).toBe(3);
	});

	test("a failed transaction does not poison the next one", async () => {
		await expect(
			withActorTransaction(ctx, async (tx) => {
				await tx.db.execute("INSERT INTO t (v) VALUES ('a')");
				throw new Error("fail");
			}),
		).rejects.toThrow();
		await withActorTransaction(ctx, async (tx) => {
			await tx.db.execute("INSERT INTO t (v) VALUES ('b')");
		});
		expect(await count(ctx)).toBe(1);
	});
});
