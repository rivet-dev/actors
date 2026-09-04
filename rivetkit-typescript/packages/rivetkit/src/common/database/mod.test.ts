import { pino } from "pino";
import { beforeEach, describe, expect, test } from "vitest";
import { configureBaseLogger } from "@/common/log";
import type {
	DatabaseProviderContext,
	SqliteBindings,
	SqliteDatabase,
	SqliteExecuteResult,
	SqliteTransactionDatabase,
	SynchronousSqliteTransactionDatabase,
} from "./config";
import { db, registerNativeStateTransactionOpener } from "./mod";

let logLines: string[];

class FakeSqliteDatabase implements SqliteDatabase {
	failSql = new Map<string, Error>();
	executeCalls: { sql: string; params?: SqliteBindings }[] = [];
	stateTransactionTimeouts: Array<number | undefined> = [];
	transactionTimeouts: Array<number | undefined> = [];
	transactionNames: Array<string | undefined> = [];

	async exec(
		sql: string,
		callback?: (row: unknown[], columns: string[]) => void,
	): Promise<void> {
		this.execSync(sql, callback);
	}

	execSync(
		sql: string,
		callback?: (row: unknown[], columns: string[]) => void,
	): void {
		this.record(sql);
		callback?.([1], ["value"]);
	}

	async execute(
		sql: string,
		params?: SqliteBindings,
	): Promise<SqliteExecuteResult> {
		this.record(sql, params);
		return emptyResult();
	}

	executeSync(sql: string, params?: SqliteBindings): SqliteExecuteResult {
		this.record(sql, params);
		return emptyResult();
	}

	async beginTransaction(
		timeoutMs?: number,
		name?: string,
	): Promise<SqliteTransactionDatabase> {
		return this.beginTransactionSync(timeoutMs, name);
	}

	beginTransactionSync(
		timeoutMs?: number,
		name?: string,
	): SynchronousSqliteTransactionDatabase {
		this.transactionTimeouts.push(timeoutMs);
		this.transactionNames.push(name);
		this.record("BEGIN");
		return {
			exec: async () => {},
			execSync: () => {},
			execute: async (sql, params) => {
				this.record(sql, params);
				return emptyResult();
			},
			executeSync: (sql, params) => {
				this.record(sql, params);
				return emptyResult();
			},
			commit: async () => this.record("COMMIT"),
			commitSync: () => this.record("COMMIT"),
			rollback: async () => this.record("ROLLBACK"),
			rollbackSync: () => this.record("ROLLBACK"),
		};
	}
	async beginStateTransaction(
		timeoutMs?: number,
	): Promise<SqliteTransactionDatabase> {
		this.stateTransactionTimeouts.push(timeoutMs);
		this.record("BEGIN_STATE");
		return {
			exec: async () => {},
			execSync: () => {},
			execute: async (sql, params) => {
				this.record(sql, params);
				return emptyResult();
			},
			executeSync: (sql, params) => {
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

	test("exposes synchronous raw queries on the built-in client", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const client = await db().createClient(testProviderContext(nativeDb));

		client.executeSync("SELECT ?", 42);
		expect(
			client.executeSync<{ value: number }>("SELECT 1; SELECT 2"),
		).toEqual([{ value: 1 }]);

		expect(nativeDb.executeCalls).toEqual([
			{
				sql: "SELECT ?",
				params: [42],
			},
			{
				sql: "SELECT 1; SELECT 2",
				params: undefined,
			},
		]);
	});

	test("commits synchronous transaction work before returning", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const client = await db().createClient(testProviderContext(nativeDb));

		const value = client.transactionSync(
			(tx) => {
				tx.executeSync("INSERT INTO items(value) VALUES (?)", "inside");
				return 42;
			},
			{ name: "sync-insert", timeout: 120_000 },
		);

		expect(value).toBe(42);
		expect(nativeDb.transactionTimeouts).toEqual([120_000]);
		expect(nativeDb.transactionNames).toEqual(["sync-insert"]);
		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN",
			"INSERT INTO items(value) VALUES (?)",
			"COMMIT",
		]);
	});

	test("rolls back synchronous transactions on callback and commit errors", async () => {
		const callbackDb = new FakeSqliteDatabase();
		const callbackClient = await db().createClient(
			testProviderContext(callbackDb),
		);
		expect(() =>
			callbackClient.transactionSync(() => {
				throw new Error("callback failed");
			}),
		).toThrow("callback failed");
		expect(callbackDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN",
			"ROLLBACK",
		]);

		const commitDb = new FakeSqliteDatabase();
		commitDb.failSql.set("COMMIT", new Error("commit failed"));
		const commitClient = await db().createClient(
			testProviderContext(commitDb),
		);
		expect(() => commitClient.transactionSync(() => 1)).toThrow(
			"commit failed",
		);
		expect(commitDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN",
			"COMMIT",
			"ROLLBACK",
		]);
	});

	test("rejects async synchronous-transaction callbacks and outer-client queries", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const client = await db().createClient(testProviderContext(nativeDb));

		expect(() => client.transactionSync(async () => undefined)).toThrow(
			"must not return a promise",
		);
		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN",
			"ROLLBACK",
		]);

		nativeDb.executeCalls = [];
		let outerQuery: Promise<Record<string, unknown>[]> | undefined;
		client.transactionSync((tx) => {
			expect(Object.keys(tx)).toEqual(["executeSync"]);
			expect(() => client.executeSync("SELECT 1")).toThrow(
				"transaction callback's tx value",
			);
			outerQuery = client.execute("SELECT 1");
			expect(() => client.transactionSync(() => undefined)).toThrow(
				"Nested synchronous SQLite transactions",
			);
			tx.executeSync("SELECT 2");
		});
		await expect(outerQuery).rejects.toThrow(
			"transaction callback's tx value",
		);
		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN",
			"SELECT 2",
			"COMMIT",
		]);
	});

	test("validates synchronous transaction options before beginning", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const client = await db().createClient(testProviderContext(nativeDb));

		for (const timeout of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
			expect(() =>
				client.transactionSync(() => undefined, { timeout }),
			).toThrow("positive finite");
		}
		expect(() =>
			client.transactionSync(() => undefined, { name: "" }),
		).toThrow("must not be empty");
		expect(nativeDb.executeCalls).toEqual([]);
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
