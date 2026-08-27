import { describe, expect, test } from "vitest";
import type {
	DatabaseProviderContext,
	SqliteBatchStatement,
	SqliteBindings,
	SqliteDatabase,
	SqliteExecuteResult,
	SqliteTransactionDatabase,
} from "@/common/database/config";
import { db } from "./drizzle";

class FakeSqliteDatabase implements SqliteDatabase {
	executeCalls: Array<{ sql: string; params?: SqliteBindings }> = [];
	transactionTimeouts: Array<number | undefined> = [];
	transactionNames: Array<string | undefined> = [];

	async exec(): Promise<void> {}

	async execute(
		sql: string,
		params?: SqliteBindings,
	): Promise<SqliteExecuteResult> {
		this.executeCalls.push({ sql, params });
		return emptyResult();
	}

	async executeBatch(
		statements: SqliteBatchStatement[],
	): Promise<SqliteExecuteResult[]> {
		const transaction = await this.beginTransaction();
		try {
			const results: SqliteExecuteResult[] = [];
			for (const statement of statements) {
				results.push(
					await transaction.execute(statement.sql, statement.params),
				);
			}
			await transaction.commit();
			return results;
		} catch (error) {
			await transaction.rollback();
			throw error;
		}
	}

	async beginTransaction(
		timeoutMs?: number,
		name?: string,
	): Promise<SqliteTransactionDatabase> {
		this.transactionTimeouts.push(timeoutMs);
		this.transactionNames.push(name);
		this.executeCalls.push({ sql: "BEGIN" });
		return {
			exec: async () => {},
			execute: async (sql, params) => {
				this.executeCalls.push({ sql, params });
				return emptyResult();
			},
			commit: async () => {
				this.executeCalls.push({ sql: "COMMIT" });
			},
			rollback: async () => {
				this.executeCalls.push({ sql: "ROLLBACK" });
			},
		};
	}

	async run(sql: string, params?: SqliteBindings): Promise<void> {
		await this.execute(sql, params);
	}

	async query(sql: string, params?: SqliteBindings) {
		const { columns, rows } = await this.execute(sql, params);
		return { columns, rows };
	}

	async close(): Promise<void> {}
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
	database: SqliteDatabase,
): DatabaseProviderContext {
	return {
		actorId: "actor-a",
		kv: {
			batchPut: async () => {},
			batchGet: async (keys) => keys.map(() => null),
			batchDelete: async () => {},
			deleteRange: async () => {},
		},
		nativeDatabaseProvider: { open: async () => database },
	};
}

describe("Drizzle database transactions", () => {
	test("runs migrations in the shared transaction with a generous timeout", async () => {
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

		expect(nativeDb.transactionTimeouts).toEqual([300_000]);
		expect(nativeDb.transactionNames).toEqual([
			"rivetkit-drizzle-migration",
		]);
		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN",
			"SAVEPOINT __rivet_on_migrate",
			"CREATE TABLE items(id INTEGER PRIMARY KEY)",
			"RELEASE SAVEPOINT __rivet_on_migrate",
			"COMMIT",
		]);
	});

	test("routes transaction work through the transaction handle", async () => {
		const nativeDb = new FakeSqliteDatabase();
		const client = await db().createClient(testProviderContext(nativeDb));
		await client.transaction(
			async (tx) => {
				await tx.execute(
					"INSERT INTO items(value) VALUES (?)",
					"inside",
				);
			},
			{ name: "drizzle-insert", timeout: 120_000 },
		);

		expect(nativeDb.transactionTimeouts).toEqual([120_000]);
		expect(nativeDb.transactionNames).toEqual(["drizzle-insert"]);
		expect(nativeDb.executeCalls.map(({ sql }) => sql)).toEqual([
			"BEGIN",
			"INSERT INTO items(value) VALUES (?)",
			"COMMIT",
		]);
	});

	test("validates zero, negative, and non-finite transaction timeouts", async () => {
		const client = await db().createClient(
			testProviderContext(new FakeSqliteDatabase()),
		);
		for (const timeout of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
			await expect(
				client.transaction(async () => {}, { timeout }),
			).rejects.toThrow("positive finite");
		}
	});
});
