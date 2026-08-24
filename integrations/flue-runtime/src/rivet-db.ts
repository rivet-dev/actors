import type { AsyncSqlDb, AsyncSqlRow, AsyncSqlValue } from './stores/index.js';

export interface RivetRawDb {
	execute<TRow extends Record<string, unknown> = Record<string, unknown>>(
		query: string,
		...args: unknown[]
	): Promise<TRow[]>;
	transaction?<T>(callback: (tx: RivetRawDb) => Promise<T> | T): Promise<T>;
}

const SAVEPOINT_NAME = '__flue_tx';

export function createRivetAsyncSqlDb(raw: RivetRawDb): AsyncSqlDb {
	let tail: Promise<void> = Promise.resolve();

	const queryDirect = async (
		text: string,
		params: readonly AsyncSqlValue[] = [],
	): Promise<AsyncSqlRow[]> => raw.execute(text, ...params.map(normalizeBinding));

	return {
		async query(text, params = []) {
			// Wait for any in-flight transaction to release the tail, but do not chain
			// onto it: a plain query is a single statement and callers await
			// sequentially over the one connection, so it cannot interleave inside a
			// concurrent transaction's savepoint critical section in practice.
			await tail;
			return queryDirect(text, params);
		},
		async transaction(fn) {
			// Serialize transactions through a promise tail. Rivet runs each actor
			// single-threaded over one persistent SQLite connection, so concurrent
			// callers would otherwise interleave their savepoints on the shared
			// connection and corrupt each other's critical sections.
			//
			// Production Rivet clients expose transaction(), which owns the native
			// writer lease and coordinates with other actor work. The savepoint below
			// is only a fallback for the node:sqlite contract harness. A single fixed
			// name is safe because the tail guarantees no two fallback savepoints are
			// open at once and this design does not support re-entrant transactions.
			const previous = tail;
			let release!: () => void;
			tail = new Promise<void>((resolve) => {
				release = resolve;
			});
			await previous;
			try {
				if (raw.transaction) {
					return await raw.transaction((tx) =>
						fn({
							query: (text, params = []) =>
								tx.execute(text, ...params.map(normalizeBinding)),
						}),
					);
				}

				// The node:sqlite contract harness only exposes execute(). Use a
				// savepoint there; production Rivet databases take the coordinated
				// transaction() branch above.
				await queryDirect(`SAVEPOINT ${SAVEPOINT_NAME}`);
				try {
					const result = await fn({ query: queryDirect });
					await queryDirect(`RELEASE SAVEPOINT ${SAVEPOINT_NAME}`);
					return result;
				} catch (error) {
					// Undo the critical section and discard the savepoint, then
					// always surface the original error. A rollback/release failure
					// here would only mask the real cause; the ambient transaction
					// is torn down by Rivet when the action throws regardless.
					await queryDirect(`ROLLBACK TO SAVEPOINT ${SAVEPOINT_NAME}`).catch(() => {});
					await queryDirect(`RELEASE SAVEPOINT ${SAVEPOINT_NAME}`).catch(() => {});
					throw error;
				}
			} finally {
				release();
			}
		},
	};
}

function normalizeBinding(value: AsyncSqlValue): AsyncSqlValue {
	return typeof value === 'boolean' ? (value ? 1 : 0) : value;
}
