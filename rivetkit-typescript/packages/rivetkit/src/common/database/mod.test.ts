import { pino } from "pino";
import { beforeEach, describe, expect, test } from "vitest";
import { configureBaseLogger } from "@/common/log";
import type {
	DatabaseProviderContext,
	SqliteBindings,
	SqliteDatabase,
	SqliteExecuteResult,
	SqliteTransactionDatabase,
} from "./config";
import { db, registerNativeStateTransactionOpener } from "./mod";

let logLines: string[];

class FakeSqliteDatabase implements SqliteDatabase {
	failSql = new Map<string, Error>();
	executeCalls: { sql: string; params?: SqliteBindings }[] = [];
	stateTransactionTimeouts: Array<number | undefined> = [];
	transactionTimeouts: Array<number | undefined> = [];
	transactionNames: Array<string | undefined> = [];

	async exec(): Promise<void> {}

	async execute(
		sql: string,
		params?: SqliteBindings,
	): Promise<SqliteExecuteResult> {
		this.record(sql, params);
		return emptyResult();
	}

	async beginTransaction(
		timeoutMs?: number,
		name?: string,
	): Promise<SqliteTransactionDatabase> {
		this.transactionTimeouts.push(timeoutMs);
		this.transactionNames.push(name);
		this.record("BEGIN");
		return {
			exec: async () => {},
			execute: async (sql, params) => {
				this.record(sql, params);
				return emptyResult();
			},
			commit: async () => this.record("COMMIT"),
			rollback: async () => this.record("ROLLBACK"),
		};
	}
	async beginStateTransaction(
		timeoutMs?: number,
	): Promise<SqliteTransactionDatabase> {
		this.stateTransactionTimeouts.push(timeoutMs);
		this.record("BEGIN_STATE");
		return {
			exec: async () => {},
			execute: async (sql, params) => {
				this.record(sql, params);
				return emptyResult();
			},
			commit: async () => this.record("COMMIT"),
			rollback: async () => this.record("ROLLBACK"),
		};
	}

	async executeBatch(
		statements: Array<{ sql: string; params?: SqliteBindings }>,
	): Promise<SqliteExecuteResult[]> {
		const results: SqliteExecuteResult[] = [];
		for (const statement of statements) {
			results.push(await this.execute(statement.sql, statement.params));
		}
		return results;
	}

	async run(sql: string, params?: SqliteBindings): Promise<void> {
		await this.execute(sql, params);
	}

	async query(sql: string, params?: SqliteBindings) {
		const { columns, rows } = await this.execute(sql, params);
		return { columns, rows };
	}

	async close(): Promise<void> {}

	private record(sql: string, params?: SqliteBindings): void {
		this.executeCalls.push({ sql, params });
		const error = this.failSql.get(sql);
		if (error) throw error;
	}
}

function emptyResult(): SqliteExecuteResult {
	return {
		columns: [],
		rows: [],
		changes: 0,
		lastInsertRowId: null,
	};
}

function testProviderContext(
	database: FakeSqliteDatabase,
	includeStateTransactions = false,
): DatabaseProviderContext {
	return {
		actorId: "actor-a",
		kv: {
			batchPut: async () => {},
			batchGet: async (keys) => keys.map(() => null),
			batchDelete: async () => {},
			deleteRange: async () => {},
		},
		nativeDatabaseProvider: includeStateTransactions
			? registerNativeStateTransactionOpener(
					{ open: async () => database },
					async (timeoutMs?: number) =>
						await database.beginStateTransaction(timeoutMs),
				)
			: { open: async () => database },
	};
}

describe("db", () => {
	beforeEach(() => {
		logLines = [];
		configureBaseLogger(
			pino(
				{ level: "warn", base: {}, timestamp: false },
				{ write: (line: string) => logLines.push(line) },
			),
		);
	});

	test("runs onMigrate through the shared transaction handle", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const provider = db({
			onMigrate: async (client) => {
				await client.execute(
					"CREATE TABLE items(id INTEGER PRIMARY KEY)",
				);
			},
		});
		const client = await provider.createClient(
			testProviderContext(nativeDb),
		);
		await provider.onMigrate(client);

		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN",
			"SAVEPOINT __rivet_on_migrate",
			"CREATE TABLE items(id INTEGER PRIMARY KEY)",
			"RELEASE SAVEPOINT __rivet_on_migrate",
			"COMMIT",
		]);
		expect(nativeDb.transactionTimeouts).toEqual([300_000]);
		expect(nativeDb.transactionNames).toEqual(["rivetkit-migration"]);
	});

	test("exposes profiling controls to the actor runtime", () => {
		const provider = db({
			profiling: {
				enabled: false,
				maxTrackedStatementFingerprints: 12,
				baselineSampleRate: 0.25,
			},
		});

		expect(provider.sqliteProfiling).toEqual({
			enabled: false,
			maxTrackedStatementFingerprints: 12,
			baselineSampleRate: 0.25,
		});
	});

	test("rolls back migrations when onMigrate fails", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const provider = db({
			onMigrate: async () => {
				throw new Error("migration failed");
			},
		});
		const client = await provider.createClient(
			testProviderContext(nativeDb),
		);
		await expect(provider.onMigrate(client)).rejects.toThrow(
			"migration failed",
		);
		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN",
			"SAVEPOINT __rivet_on_migrate",
			"ROLLBACK TO SAVEPOINT __rivet_on_migrate",
			"RELEASE SAVEPOINT __rivet_on_migrate",
			"ROLLBACK",
		]);
	});

	test("commits transaction work and forwards timeout", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const client = await db().createClient(testProviderContext(nativeDb));
		const value = await client.transaction(
			async (tx) => {
				await tx.execute(
					"INSERT INTO items(value) VALUES (?)",
					"inside",
				);
				return 42;
			},
			{ name: "insert-item", timeout: 120_000 },
		);
		expect(value).toBe(42);
		expect(nativeDb.transactionTimeouts).toEqual([120_000]);
		expect(nativeDb.transactionNames).toEqual(["insert-item"]);
		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN",
			"INSERT INTO items(value) VALUES (?)",
			"COMMIT",
		]);
	});

	test("validates transaction names", async () => {
		const client = await db().createClient(
			testProviderContext(new FakeSqliteDatabase()),
		);
		await expect(
			client.transaction(async () => {}, { name: "" }),
		).rejects.toThrow("must not be empty");
	});

	test("defers the configured transaction name byte limit to the runtime", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const client = await db().createClient(testProviderContext(nativeDb));
		const name = "x".repeat(129);

		await client.transaction(async () => {}, { name });

		expect(nativeDb.transactionNames).toEqual([name]);
	});

	test("rolls back a transaction when the callback throws", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const client = await db().createClient(testProviderContext(nativeDb));
		await expect(
			client.transaction(async (tx) => {
				await tx.execute(
					"INSERT INTO items(value) VALUES (?)",
					"inside",
				);
				throw new Error("callback failed");
			}),
		).rejects.toThrow("callback failed");
		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN",
			"INSERT INTO items(value) VALUES (?)",
			"ROLLBACK",
		]);
	});

	test("uses the state-aware transaction bridge when explicitly requested", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const client = await db().createClient(
			testProviderContext(nativeDb, true),
		);

		await client.transaction(
			async (tx) => {
				await tx.execute(
					"INSERT INTO items(value) VALUES (?)",
					"inside",
				);
			},
			{
				timeout: 120_000,
				experimental: { includeState: true },
			},
		);

		expect(nativeDb.transactionTimeouts).toEqual([]);
		expect(nativeDb.stateTransactionTimeouts).toEqual([120_000]);
		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN_STATE",
			"INSERT INTO items(value) VALUES (?)",
			"COMMIT",
		]);
	});

	test("rolls back state-aware transactions when the callback throws", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const client = await db().createClient(
			testProviderContext(nativeDb, true),
		);

		await expect(
			client.transaction(
				async (tx) => {
					await tx.execute(
						"INSERT INTO items(value) VALUES (?)",
						"inside",
					);
					throw new Error("callback failed");
				},
				{ experimental: { includeState: true } },
			),
		).rejects.toThrow("callback failed");
		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN_STATE",
			"INSERT INTO items(value) VALUES (?)",
			"ROLLBACK",
		]);
	});

	test("rejects nested state-aware transactions", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const client = await db().createClient(
			testProviderContext(nativeDb, true),
		);

		await expect(
			client.transaction(
				async (tx) => {
					await tx.transaction(async () => {}, {
						experimental: { includeState: true },
					});
				},
				{ experimental: { includeState: true } },
			),
		).rejects.toThrow("not supported for nested transactions");
		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN_STATE",
			"ROLLBACK",
		]);
	});

	test("validates transaction timeouts", async () => {
		const client = await db().createClient(
			testProviderContext(new FakeSqliteDatabase()),
		);
		for (const timeout of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
			await expect(
				client.transaction(async () => {}, { timeout }),
			).rejects.toThrow("positive finite");
		}
	});

	test("warns once for manual cross-call transactions and names the opt-out", async () => {
		const client = await db().createClient(
			testProviderContext(new FakeSqliteDatabase()),
		);
		await client.execute("BEGIN");
		await client.execute("COMMIT");
		expect(logLines).toHaveLength(1);
		expect(JSON.parse(logLines[0] ?? "{}").msg).toContain(
			"Set warnOnManualTransactions: false",
		);
	});

	test("can suppress the manual transaction warning", async () => {
		const client = await db({
			warnOnManualTransactions: false,
		}).createClient(testProviderContext(new FakeSqliteDatabase()));
		await client.execute("BEGIN");
		expect(logLines).toHaveLength(0);
	});
});
