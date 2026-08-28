import type {
	DatabaseProvider,
	RawAccess,
	SqliteDatabase,
	SqliteProfilingOptions,
} from "./config";
import { isSqliteBindingObject, toSqliteBindings } from "./shared";

export type { RawAccess } from "./config";

export interface DatabaseFactoryConfig {
	onMigrate?: (db: RawAccess) => Promise<void> | void;
	/**
	 * @experimental This entire configuration surface is subject to change.
	 */
	profiling?: SqliteProfilingOptions;
}

function hasMultipleStatements(query: string): boolean {
	const trimmed = query.trim().replace(/;+$/, "").trimEnd();
	return trimmed.includes(";");
}

export function db({
	onMigrate,
	profiling,
}: DatabaseFactoryConfig = {}): DatabaseProvider<RawAccess> {
	return {
		sqliteProfiling: profiling,
		createClient: async (ctx) => {
			const nativeDatabaseProvider = ctx.nativeDatabaseProvider;
			if (!nativeDatabaseProvider) {
				throw new Error(
					"native SQLite is required, but the current runtime did not provide a native database provider",
				);
			}

			const db = await nativeDatabaseProvider.open(ctx.actorId);
			let closed = false;
			const ensureOpen = () => {
				if (closed) {
					throw new Error(
						"Database is closed. This usually means a background timer (setInterval, setTimeout) or a stray promise is still running after the actor stopped. Use c.abortSignal to clean up timers before the actor shuts down.",
					);
				}
			};

			const client: RawAccess = {
				execute: async <
					TRow extends Record<string, unknown> = Record<
						string,
						unknown
					>,
				>(
					query: string,
					...args: unknown[]
				): Promise<TRow[]> => {
					ensureOpen();

					const kvReadsBefore = ctx.metrics?.totalKvReads ?? 0;
					const kvWritesBefore = ctx.metrics?.totalKvWrites ?? 0;
					const start = performance.now();

					try {
						if (args.length > 0) {
							const bindings =
								args.length === 1 &&
								isSqliteBindingObject(args[0])
									? toSqliteBindings(args[0])
									: toSqliteBindings(args);
							const { rows, columns } = await db.execute(
								query,
								bindings,
							);
							return rows.map((row) =>
								rowToObject<TRow>(row, columns),
							);
						}

						if (!hasMultipleStatements(query)) {
							const { rows, columns } = await db.execute(query);
							return rows.map((row) =>
								rowToObject<TRow>(row, columns),
							);
						}

						return await execMultiStatement<TRow>(db, query);
					} finally {
						const durationMs = performance.now() - start;
						ctx.metrics?.trackSql(query, durationMs);
						if (ctx.metrics) {
							const kvReads =
								ctx.metrics.totalKvReads - kvReadsBefore;
							const kvWrites =
								ctx.metrics.totalKvWrites - kvWritesBefore;
							ctx.log?.debug({
								msg: "sql query",
								query: query.slice(0, 120),
								durationMs,
								kvReads,
								kvWrites,
							});
						}
					}
				},
				close: async () => {
					if (!closed) {
						closed = true;
						await db.close();
					}
				},
				nativeMetrics: () => db.nativeMetrics?.() ?? null,
			};
			return client;
		},
		onMigrate: async (client) => {
			if (onMigrate) {
				await withMigrationSavepoint(client, () => onMigrate(client));
			}
		},
	};
}

function rowToObject<TRow extends Record<string, unknown>>(
	row: unknown[],
	columns: string[],
): TRow {
	const rowObj: Record<string, unknown> = {};
	for (let i = 0; i < columns.length; i++) {
		rowObj[columns[i]] = row[i];
	}
	return rowObj as TRow;
}

async function execMultiStatement<TRow extends Record<string, unknown>>(
	db: SqliteDatabase,
	query: string,
): Promise<TRow[]> {
	const results: Record<string, unknown>[] = [];
	let columnNames: string[] | null = null;
	await db.exec(query, (row: unknown[], columns: string[]) => {
		if (!columnNames) {
			columnNames = columns;
		}
		results.push(rowToObject(row, columnNames));
	});
	return results as TRow[];
}

async function withMigrationSavepoint<T>(
	client: RawAccess,
	callback: () => Promise<T> | T,
): Promise<T> {
	await client.execute("SAVEPOINT __rivet_on_migrate");
	try {
		const result = await callback();
		await client.execute("RELEASE SAVEPOINT __rivet_on_migrate");
		return result;
	} catch (error) {
		try {
			await client.execute("ROLLBACK TO SAVEPOINT __rivet_on_migrate");
		} finally {
			await client.execute("RELEASE SAVEPOINT __rivet_on_migrate");
		}
		throw error;
	}
}
