import { describe, expect, test } from "vitest";
import { BRIDGE_RIVET_ERROR_PREFIX } from "@/actor/errors";
import {
	type JsNativeDatabaseLike,
	wrapJsNativeDatabase,
} from "./native-database";

type NativeParams = Parameters<JsNativeDatabaseLike["execute"]>[1];
type NativeExecuteResult = Awaited<ReturnType<JsNativeDatabaseLike["execute"]>>;

function deferred<T>() {
	let resolve!: (value: T) => void;
	let reject!: (error: unknown) => void;
	const promise = new Promise<T>((resolvePromise, rejectPromise) => {
		resolve = resolvePromise;
		reject = rejectPromise;
	});
	return { promise, resolve, reject };
}

class FakeNativeDatabase implements JsNativeDatabaseLike {
	async beginTransaction() {
		this.transactionEvents.push("BEGIN");
		return this.#transaction();
	}

	beginTransactionSync() {
		this.transactionEvents.push("BEGIN_SYNC");
		return this.#transaction();
	}

	#transaction() {
		return {
			exec: async (_sql: string) => this.exec(),
			execSync: (_sql: string) => this.execSync(),
			execute: async (sql: string, params?: NativeParams) =>
				this.execute(sql, params),
			executeSync: (sql: string, params?: NativeParams) =>
				this.executeSync(sql, params),
			commit: async () => {
				this.transactionEvents.push("COMMIT");
			},
			commitSync: () => {
				this.transactionEvents.push("COMMIT_SYNC");
			},
			rollback: async () => {
				this.transactionEvents.push("ROLLBACK");
			},
			rollbackSync: () => {
				this.transactionEvents.push("ROLLBACK_SYNC");
			},
		};
	}
	active = 0;
	maxActive = 0;
	closed = false;
	executeCalls: { sql: string; params?: NativeParams; write: boolean }[] = [];
	transactionEvents: string[] = [];
	#pending: ReturnType<typeof deferred<NativeExecuteResult>>[] = [];

	async exec() {
		return { columns: [], rows: [] };
	}

	execSync() {
		return { columns: ["value"], rows: [[1], [2]] };
	}

	async execute(sql: string, params?: NativeParams) {
		return await this.#startExecute(sql, params, false);
	}

	executeSync(sql: string, params?: NativeParams): NativeExecuteResult {
		this.executeCalls.push({ sql, params, write: false });
		return {
			columns: ["value"],
			rows: [[1]],
			changes: 0,
			lastInsertRowId: null,
		};
	}

	async query(sql: string, params?: NativeParams) {
		const { columns, rows } = await this.execute(sql, params);
		return { columns, rows };
	}

	async run(sql: string, params?: NativeParams) {
		const { changes } = await this.execute(sql, params);
		return { changes };
	}

	takeLastKvError() {
		return null;
	}

	async close() {
		this.closed = true;
	}

	resolveNext(result: Partial<NativeExecuteResult> = {}) {
		const pending = this.#pending.shift();
		if (!pending) {
			throw new Error("no pending native execute call");
		}
		pending.resolve({
			columns: [],
			rows: [],
			changes: 0,
			lastInsertRowId: null,
			...result,
		});
	}

	rejectNext(error: unknown) {
		const pending = this.#pending.shift();
		if (!pending) {
			throw new Error("no pending native execute call");
		}
		pending.reject(error);
	}

	async #startExecute(
		sql: string,
		params: NativeParams,
		write: boolean,
	): Promise<NativeExecuteResult> {
		this.executeCalls.push({ sql, params, write });
		this.active++;
		this.maxActive = Math.max(this.maxActive, this.active);
		const pending = deferred<NativeExecuteResult>();
		this.#pending.push(pending);
		try {
			return await pending.promise;
		} finally {
			this.active--;
		}
	}
}

describe("wrapJsNativeDatabase", () => {
	test("admits Promise.all read queries concurrently", async () => {
		const native = new FakeNativeDatabase();
		const db = wrapJsNativeDatabase(native);

		const first = db.query("SELECT 1");
		const second = db.query("SELECT 2");

		expect(native.maxActive).toBe(2);
		native.resolveNext({ columns: ["value"], rows: [[1]] });
		native.resolveNext({ columns: ["value"], rows: [[2]] });

		await expect(first).resolves.toEqual({
			columns: ["value"],
			rows: [[1]],
		});
		await expect(second).resolves.toEqual({
			columns: ["value"],
			rows: [[2]],
		});
	});

	test("normalizes supported sqlite bind values", async () => {
		const native = new FakeNativeDatabase();
		const db = wrapJsNativeDatabase(native);
		const blob = new Uint8Array([1, 2, 3]);

		const query = db.query("SELECT ?, ?, ?, ?, ?, ?, ?", [
			1n,
			true,
			"text",
			1.5,
			null,
			undefined,
			blob,
		]);

		expect(native.executeCalls[0]?.params).toEqual([
			{ kind: "int", intValue: 1 },
			{ kind: "int", intValue: 1 },
			{ kind: "text", textValue: "text" },
			{ kind: "float", floatValue: 1.5 },
			{ kind: "null" },
			{ kind: "null" },
			{ kind: "blob", blobValue: Buffer.from(blob) },
		]);

		native.resolveNext({ columns: ["value"], rows: [[1]] });

		await expect(query).resolves.toEqual({
			columns: ["value"],
			rows: [[1]],
		});
	});

	test("executes synchronously with normalized bindings", () => {
		const native = new FakeNativeDatabase();
		const db = wrapJsNativeDatabase(native);

		const result = db.executeSync?.("SELECT ?, ?", [true, "text"]);
		const execRows: unknown[][] = [];
		db.execSync?.("SELECT 1; SELECT 2", (row) => execRows.push(row));

		expect(native.executeCalls[0]?.params).toEqual([
			{ kind: "int", intValue: 1 },
			{ kind: "text", textValue: "text" },
		]);
		expect(result).toMatchObject({
			columns: ["value"],
			rows: [[1]],
		});
		expect(execRows).toEqual([[1], [2]]);
	});

	test("wraps synchronous transaction lifecycle methods", () => {
		const native = new FakeNativeDatabase();
		const db = wrapJsNativeDatabase(native);

		const committed = db.beginTransactionSync?.(1_000, "commit");
		if (!committed)
			throw new Error("missing synchronous transaction support");
		committed.executeSync("SELECT 1");
		committed.commitSync();
		const rolledBack = db.beginTransactionSync?.(1_000, "rollback");
		if (!rolledBack)
			throw new Error("missing synchronous transaction support");
		rolledBack.rollbackSync();

		expect(native.transactionEvents).toEqual([
			"BEGIN_SYNC",
			"COMMIT_SYNC",
			"BEGIN_SYNC",
			"ROLLBACK_SYNC",
		]);
	});

	test("returns native execute metadata", async () => {
		const native = new FakeNativeDatabase();
		const db = wrapJsNativeDatabase(native);

		const write = db.execute("INSERT INTO test VALUES (1)");
		native.resolveNext({ changes: 1, lastInsertRowId: 7 });
		await expect(write).resolves.toMatchObject({
			changes: 1,
			lastInsertRowId: 7,
		});

		const fallback = db.execute("SELECT last_insert_rowid()");
		await expect(fallback).resolves.toMatchObject({
			rows: [[7]],
		});
	});

	test("close waits for admitted native calls and rejects new work", async () => {
		const native = new FakeNativeDatabase();
		const db = wrapJsNativeDatabase(native);

		const query = db.query("SELECT 1");
		const close = db.close();
		await Promise.resolve();

		expect(native.closed).toBe(false);
		native.resolveNext({ columns: ["value"], rows: [[1]] });

		await query;
		await close;

		expect(native.closed).toBe(true);
		await expect(db.query("SELECT 2")).rejects.toThrow(
			"Database is closed",
		);
	});

	test("decodes sanitized native bridge errors without logging", async () => {
		const native = new FakeNativeDatabase();
		const db = wrapJsNativeDatabase(native);
		const query = db.execute("SELECT broken", [1, "two"]);
		const bridgeReason = `${BRIDGE_RIVET_ERROR_PREFIX}${JSON.stringify({
			group: "rivetkit",
			code: "internal_error",
			message: "An internal error occurred",
			statusCode: 500,
		})}`;

		native.rejectNext(new Error(bridgeReason));

		await expect(query).rejects.toMatchObject({
			name: "RivetError",
			group: "rivetkit",
			code: "internal_error",
			message: "An internal error occurred",
		});

		await query.catch((error) => {
			expect(error.stack).toContain("decodeBridgeRivetError");
			expect(error.stack).toContain("enrichNativeDatabaseError");
		});
	});
});
