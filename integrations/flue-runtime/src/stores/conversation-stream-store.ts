import {
	defineSqlConversationStreamStore,
	type ConversationStreamStore,
	type SqlConversationDialect,
	type SqlConversationDialectTx,
} from '@flue/runtime/adapter-kit';
import type { AsyncSqlDb, AsyncSqlValue } from './async-db.js';

export function createAsyncConversationStreamStore(db: AsyncSqlDb): ConversationStreamStore {
	const dialect: SqlConversationDialect = {
		placeholder: () => '?',
		lockClause: '',
		insertIgnorePrefix: 'INSERT OR IGNORE',
		insertIgnoreSuffix: '',
		supportsReturning: true,
		query: (sql: string, params: readonly unknown[]) =>
			db.query(sql, params as readonly AsyncSqlValue[]),
		transaction: <T>(fn: (tx: SqlConversationDialectTx) => Promise<T>) => db.transaction((tx) => fn({
			query: (sql: string, params: readonly unknown[]) =>
				tx.query(sql, params as readonly AsyncSqlValue[]),
		})),
	};
	return defineSqlConversationStreamStore(dialect);
}
