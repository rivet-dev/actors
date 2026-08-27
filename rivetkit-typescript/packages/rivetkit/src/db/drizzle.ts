import {
	drizzle,
	type RemoteCallback,
	type SqliteRemoteDatabase,
} from "drizzle-orm/sqlite-proxy";
import type {
	DatabaseProvider,
	DatabaseProviderContext,
	RawAccess,
	SqliteProfilingOptions,
	SqliteTransactionOptions,
	SqliteDatabase,
	SqliteTransactionDatabase,
} from "@/common/database/config";
import { getLogger } from "@/common/log";
import {
	isManualTransactionControl,
	MIGRATION_TRANSACTION_TIMEOUT_MS,
	toSqliteBindings,
	validateTransactionName,
	validateTransactionTimeout,
} from "@/common/database/shared";
import { sha256Hex } from "@/utils/crypto";

export type { SQLiteTable } from "drizzle-orm/sqlite-core";
export {
	alias,
	check,
	foreignKey,
	index,
	integer,
	primaryKey,
	sqliteTable,
	sqliteTableCreator,
	text,
	unique,
	uniqueIndex,
} from "drizzle-orm/sqlite-core";

type DrizzleSchema = Record<string, unknown>;
type DrizzleDatabase<TSchema extends DrizzleSchema> = Omit<
	SqliteRemoteDatabase<TSchema>,
	"transaction"
> &
	Omit<RawAccess, "transaction"> & {
		transaction: <T>(
			callback: (tx: DrizzleDatabase<TSchema>) => Promise<T> | T,
			options?: SqliteTransactionOptions,
		) => Promise<T>;
	};

interface DrizzleMigrationJournalEntry {
	idx: number;
	tag: string;
	when: number;
	breakpoints?: boolean;
}

interface DrizzleMigrations {
	journal: unknown;
	migrations: Record<string, string>;
}

export interface DrizzleDatabaseFactoryConfig<TSchema extends DrizzleSchema> {
	schema?: TSchema;
	migrations?: DrizzleMigrations;
	onMigrate?: (db: DrizzleDatabase<TSchema>) => Promise<void> | void;
	warnOnManualTransactions?: boolean;
	profiling?: SqliteProfilingOptions;
}

interface DrizzleKitConfig {
	out?: string;
	schema?: string;
	dialect?: "sqlite";
	[key: string]: unknown;
}

export function defineConfig<TConfig extends DrizzleKitConfig>(
	config: TConfig,
): TConfig & { dialect: "sqlite" } {
	return {
		dialect: "sqlite",
		...config,
	};
}

export function db<TSchema extends DrizzleSchema = Record<string, never>>({
	schema,
	migrations,
	onMigrate,
	warnOnManualTransactions = true,
	profiling,
}: DrizzleDatabaseFactoryConfig<TSchema> = {}): DatabaseProvider<
	DrizzleDatabase<TSchema>
> {
	return {
		sqliteProfiling: profiling,
		createClient: async (ctx) => {
			const override = ctx.overrideDrizzleDatabaseClient
				? await ctx.overrideDrizzleDatabaseClient()
				: undefined;
			if (override) {
				return override as DrizzleDatabase<TSchema>;
			}

			const nativeDatabaseProvider = ctx.nativeDatabaseProvider;
			if (!nativeDatabaseProvider) {
				throw new Error(
					"native SQLite is required, but the current runtime did not provide a native database provider",
				);
			}

			const nativeDb = await nativeDatabaseProvider.open(ctx.actorId);
			let closed = false;
			let manualTransactionWarned = false;
			const ensureOpen = () => {
				if (closed) {
					throw new Error(
						"Database is closed. This usually means a background timer (setInterval, setTimeout) or a stray promise is still running after the actor stopped. Use c.abortSignal to clean up timers before the actor shuts down.",
					);
				}
			};

			const createDrizzleClient = (
				target: SqliteDatabase | SqliteTransactionDatabase,
				transactionScoped = false,
			): DrizzleDatabase<TSchema> => {
				const runSql = async (
					query: string,
					params: unknown[],
					method: "run" | "all" | "values" | "get",
				) => {
					ensureOpen();
					warnForManualTransaction(query, transactionScoped);

					const start = performance.now();
					const kvReadsBefore = ctx.metrics?.totalKvReads ?? 0;
					const kvWritesBefore = ctx.metrics?.totalKvWrites ?? 0;
					try {
						const { rows } = await target.execute(
							query,
							toSqliteBindings(params),
						);
						if (method === "run") {
							return { rows: [] };
						}
						if (method === "get") {
							return { rows: rows[0] };
						}
						return { rows };
					} finally {
						const durationMs = performance.now() - start;
						ctx.metrics?.trackSql(query, durationMs);
						if (ctx.metrics) {
							ctx.log?.debug({
								msg: "sql query",
								query: query.slice(0, 120),
								durationMs,
								kvReads:
									ctx.metrics.totalKvReads - kvReadsBefore,
								kvWrites:
									ctx.metrics.totalKvWrites - kvWritesBefore,
							});
						}
					}
				};

				const callback: RemoteCallback = async (
					query,
					params,
					method,
				) => {
					return await runSql(query, params, method);
				};

				const drizzleDb = drizzle(callback, {
					schema,
				}) as unknown as DrizzleDatabase<TSchema>;
				drizzleDb.execute = async <
					TRow extends Record<string, unknown> = Record<
						string,
						unknown
					>,
				>(
					query: string,
					...args: unknown[]
				): Promise<TRow[]> => {
					return await executeRaw<TRow>(
						target,
						ctx,
						ensureOpen,
						query,
						args,
						() =>
							warnForManualTransaction(query, transactionScoped),
					);
				};
				drizzleDb.transaction = async <T>(
					transactionCallback: (
						tx: DrizzleDatabase<TSchema>,
					) => Promise<T> | T,
					options?: SqliteTransactionOptions,
				): Promise<T> => {
					validateTransactionTimeout(options?.timeout);
					validateTransactionName(options?.name);
					const transaction = await nativeDb.beginTransaction(
						options?.timeout,
						options?.name,
					);
					const tx = createDrizzleClient(transaction, true);
					try {
						const result = await transactionCallback(tx);
						await transaction.commit();
						return result;
					} catch (error) {
						try {
							await transaction.rollback();
						} catch {
							// Preserve the callback or commit error after expiry cleanup.
						}
						throw error;
					}
				};
				drizzleDb.close = async () => {
					if (!closed) {
						closed = true;
						await nativeDb.close();
					}
				};

				return drizzleDb;
			};

			const warnForManualTransaction = (
				query: string,
				transactionScoped: boolean,
			) => {
				if (
					transactionScoped ||
					!warnOnManualTransactions ||
					manualTransactionWarned ||
					hasMultipleStatements(query) ||
					!isManualTransactionControl(query)
				) {
					return;
				}
				manualTransactionWarned = true;
				getLogger("database").warn(
					{ actorId: ctx.actorId },
					"Manual cross-call SQLite transactions can interleave with other actor work. Use db.transaction() for coordinated transactions. Set warnOnManualTransactions: false in your db(...) configuration to disable this warning.",
				);
			};

			return createDrizzleClient(nativeDb);
		},
		onMigrate: async (client) => {
			if (!migrations && !onMigrate) {
				return;
			}
			await withMigrationSavepoint(client, async (leased) => {
				if (migrations) {
					await runMigrations(leased, migrations);
				}
				if (onMigrate) {
					await onMigrate(leased);
				}
			});
		},
	};
}

async function withMigrationSavepoint<TSchema extends DrizzleSchema, T>(
	client: DrizzleDatabase<TSchema>,
	callback: (leased: DrizzleDatabase<TSchema>) => Promise<T> | T,
): Promise<T> {
	return await client.transaction(
		async (leased) => {
			await leased.execute("SAVEPOINT __rivet_on_migrate");
			try {
				const result = await callback(leased);
				await leased.execute("RELEASE SAVEPOINT __rivet_on_migrate");
				return result;
			} catch (error) {
				try {
					await leased.execute(
						"ROLLBACK TO SAVEPOINT __rivet_on_migrate",
					);
				} finally {
					await leased.execute(
						"RELEASE SAVEPOINT __rivet_on_migrate",
					);
				}
				throw error;
			}
		},
		{
			name: "rivetkit-drizzle-migration",
			timeout: MIGRATION_TRANSACTION_TIMEOUT_MS,
		},
	);
}

async function runMigrations<TSchema extends DrizzleSchema>(
	db: DrizzleDatabase<TSchema>,
	migrations: DrizzleMigrations,
) {
	const journal = parseMigrationJournal(migrations.journal);

	await db.execute(`
		CREATE TABLE IF NOT EXISTS __drizzle_migrations (
			id INTEGER PRIMARY KEY AUTOINCREMENT,
			hash TEXT NOT NULL,
			created_at NUMERIC
		)
	`);

	const rows = await db.execute<{ created_at: number }>(
		"SELECT created_at FROM __drizzle_migrations ORDER BY created_at DESC LIMIT 1",
	);
	const lastMigration = rows[0]?.created_at ?? 0;

	for (const entry of journal.entries) {
		if (lastMigration >= entry.when) {
			continue;
		}

		const key = `m${entry.idx.toString().padStart(4, "0")}`;
		const migration = migrations.migrations[key];
		if (migration === undefined) {
			throw new Error(
				`missing Drizzle migration "${key}" for journal entry "${entry.tag}"`,
			);
		}

		const statements = migration
			.split("--> statement-breakpoint")
			.map((statement) => statement.trim())
			.filter(Boolean);
		for (const statement of statements) {
			await db.execute(statement);
		}

		await db.execute(
			"INSERT INTO __drizzle_migrations (hash, created_at) VALUES (?, ?)",
			await sha256Hex(migration),
			entry.when,
		);
	}
}

function parseMigrationJournal(journal: unknown): {
	entries: DrizzleMigrationJournalEntry[];
} {
	if (
		!journal ||
		typeof journal !== "object" ||
		!("entries" in journal) ||
		!Array.isArray(journal.entries)
	) {
		throw new Error("invalid Drizzle migration journal");
	}

	return journal as { entries: DrizzleMigrationJournalEntry[] };
}

function hasMultipleStatements(query: string): boolean {
	const trimmed = query.trim().replace(/;+$/, "").trimEnd();
	return trimmed.includes(";");
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

async function executeRaw<TRow extends Record<string, unknown>>(
	db: SqliteDatabase | SqliteTransactionDatabase,
	ctx: DatabaseProviderContext,
	ensureOpen: () => void,
	query: string,
	args: unknown[],
	warnForManualTransaction: () => void,
): Promise<TRow[]> {
	ensureOpen();
	warnForManualTransaction();

	const start = performance.now();
	const kvReadsBefore = ctx.metrics?.totalKvReads ?? 0;
	const kvWritesBefore = ctx.metrics?.totalKvWrites ?? 0;
	try {
		if (args.length > 0) {
			const { rows, columns } = await db.execute(
				query,
				toSqliteBindings(args),
			);
			return rows.map((row) => rowToObject<TRow>(row, columns));
		}

		if (!hasMultipleStatements(query)) {
			const { rows, columns } = await db.execute(query, undefined);
			return rows.map((row) => rowToObject<TRow>(row, columns));
		}

		const results: Record<string, unknown>[] = [];
		let columnNames: string[] | null = null;
		await db.exec(query, (row, columns) => {
			if (!columnNames) {
				columnNames = columns;
			}
			results.push(rowToObject(row, columnNames));
		});
		return results as TRow[];
	} finally {
		const durationMs = performance.now() - start;
		ctx.metrics?.trackSql(query, durationMs);
		if (ctx.metrics) {
			ctx.log?.debug({
				msg: "sql query",
				query: query.slice(0, 120),
				durationMs,
				kvReads: ctx.metrics.totalKvReads - kvReadsBefore,
				kvWrites: ctx.metrics.totalKvWrites - kvWritesBefore,
			});
		}
	}
}
