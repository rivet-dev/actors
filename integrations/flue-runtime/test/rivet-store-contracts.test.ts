import { DatabaseSync } from 'node:sqlite';
import { describe, expect, it } from 'vitest';
import {
	defineEventStreamStoreContractTests,
	defineRunStoreContractTests,
	defineStoreContractTests,
} from '@flue/runtime/test-utils';
import { defineConversationStreamStoreContractTests } from '@flue/runtime/test-utils/conversation-stream';
import { defineAttachmentStoreContractTests } from '@flue/runtime/test-utils/attachment-store';
import {
	createAsyncEventStreamStore,
	createAsyncAttachmentStore,
	createAsyncConversationStreamStore,
	createAsyncRunStore,
	createAsyncSqlStores,
	createRivetAsyncSqlDb,
	ensureAsyncSqlSchema,
} from '../src/index.ts';
import type { AsyncSqlDb, RivetRawDb } from '../src/index.ts';

defineStoreContractTests('Rivet async SQL AgentExecutionStore', {
	async create() {
		const db = await createTestDb();
		return createAsyncSqlStores(db).executionStore;
	},
});

defineRunStoreContractTests('Rivet async SQL RunStore', {
	async create() {
		const db = await createTestDb();
		return createAsyncRunStore(db);
	},
});

defineEventStreamStoreContractTests('Rivet async SQL EventStreamStore', {
	async create() {
		const db = await createTestDb();
		return createAsyncEventStreamStore(db);
	},
});

defineConversationStreamStoreContractTests('Rivet async SQL ConversationStreamStore', {
	async create() {
		const db = await createTestDb();
		return {
			stream: createAsyncConversationStreamStore(db),
			executionStore: createAsyncSqlStores(db).executionStore,
		};
	},
});

defineAttachmentStoreContractTests('Rivet async SQL AttachmentStore', {
	async create() {
		const db = await createTestDb();
		return createAsyncAttachmentStore(db);
	},
});

describe('createRivetAsyncSqlDb()', () => {
	it('rolls back every statement in a transaction when the callback throws', async () => {
		const db = await createTestDb();
		await db.query(`CREATE TABLE rollback_probe (id INTEGER PRIMARY KEY, value TEXT NOT NULL)`);

		await expect(
			db.transaction(async (tx) => {
				await tx.query(`INSERT INTO rollback_probe (id, value) VALUES (?, ?)`, [1, 'first']);
				await tx.query(`INSERT INTO rollback_probe (id, value) VALUES (?, ?)`, [2, 'second']);
				throw new Error('boom');
			}),
		).rejects.toThrow('boom');

		const rows = await db.query(`SELECT id FROM rollback_probe ORDER BY id`);
		expect(rows).toEqual([]);
	});

	it('commits every statement in a transaction when the callback resolves', async () => {
		const db = await createTestDb();
		await db.query(`CREATE TABLE commit_probe (id INTEGER PRIMARY KEY, value TEXT NOT NULL)`);

		const result = await db.transaction(async (tx) => {
			await tx.query(`INSERT INTO commit_probe (id, value) VALUES (?, ?)`, [1, 'first']);
			await tx.query(`INSERT INTO commit_probe (id, value) VALUES (?, ?)`, [2, 'second']);
			return 'done';
		});

		expect(result).toBe('done');
		const rows = await db.query(`SELECT id, value FROM commit_probe ORDER BY id`);
		expect(rows).toEqual([
			{ id: 1, value: 'first' },
			{ id: 2, value: 'second' },
		]);
	});

	it('does not interleave concurrent transactions on the shared connection', async () => {
		const db = await createTestDb();
		await db.query(`CREATE TABLE serialize_probe (id INTEGER PRIMARY KEY, value INTEGER NOT NULL)`);

		// Two transactions started concurrently must each see a consistent view:
		// neither commits its partial state into the other's BEGIN..COMMIT window.
		const first = db.transaction(async (tx) => {
			await tx.query(`INSERT INTO serialize_probe (id, value) VALUES (?, ?)`, [1, 10]);
			await tx.query(`INSERT INTO serialize_probe (id, value) VALUES (?, ?)`, [2, 20]);
			const rows = await tx.query(`SELECT COUNT(*) AS n FROM serialize_probe`);
			return Number((rows[0] as { n: number }).n);
		});
		const second = db.transaction(async (tx) => {
			await tx.query(`INSERT INTO serialize_probe (id, value) VALUES (?, ?)`, [3, 30]);
			const rows = await tx.query(`SELECT COUNT(*) AS n FROM serialize_probe`);
			return Number((rows[0] as { n: number }).n);
		});

		const [firstCount, secondCount] = await Promise.all([first, second]);
		expect(firstCount).toBe(2);
		expect(secondCount).toBe(3);
	});
});

// Exercises the production `createRivetAsyncSqlDb` adapter over an `execute`-only
// transport that mirrors Rivet's `c.db` (RawAccess) contract: a single persistent
// connection exposing one variadic `execute(query, ...args)` method. The adapter's
// own BEGIN/COMMIT/ROLLBACK logic — not a stronger test double — is what the
// contract tests run against.
async function createTestDb(): Promise<AsyncSqlDb> {
	const sqlite = new DatabaseSync(':memory:');
	const raw: RivetRawDb = {
		async execute(query, ...args) {
			const rows = sqlite.prepare(query).all(...(args as unknown[])) as Record<string, unknown>[];
			return rows.map((row) => ({ ...row })) as never;
		},
	};
	const db = createRivetAsyncSqlDb(raw);
	await ensureAsyncSqlSchema(db);
	return db;
}
