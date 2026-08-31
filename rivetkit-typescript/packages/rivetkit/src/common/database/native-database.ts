import { decodeBridgeRivetError } from "@/actor/errors";
import type {
	SqliteBatchStatement,
	SqliteBindings,
	SqliteDatabase,
	SqliteExecuteResult,
	SqliteNativeMetrics,
	SqliteTransactionDatabase,
} from "./config";

type NativeBindNoValues = {
	intValue?: never;
	floatValue?: never;
	textValue?: never;
	blobValue?: never;
};

type NativeBindParam =
	| ({ kind: "null" } & NativeBindNoValues)
	| {
			kind: "int";
			intValue: number;
			floatValue?: never;
			textValue?: never;
			blobValue?: never;
	  }
	| {
			kind: "float";
			intValue?: never;
			floatValue: number;
			textValue?: never;
			blobValue?: never;
	  }
	| {
			kind: "text";
			intValue?: never;
			floatValue?: never;
			textValue: string;
			blobValue?: never;
	  }
	| {
			kind: "blob";
			intValue?: never;
			floatValue?: never;
			textValue?: never;
			blobValue: Buffer;
	  };

interface NativeExecResult {
	columns: string[];
	rows: unknown[][];
}

interface NativeQueryResult {
	columns: string[];
	rows: unknown[][];
}

interface NativeRunResult {
	changes: number;
}

interface NativeExecuteResult {
	columns: string[];
	rows: unknown[][];
	changes: number;
	lastInsertRowId?: number | null;
}

interface NativeBatchStatement {
	sql: string;
	params?: NativeBindParam[] | null;
}

export interface JsNativeDatabaseLike {
	exec(sql: string): Promise<NativeExecResult>;
	execute(
		sql: string,
		params?: NativeBindParam[] | null,
	): Promise<NativeExecuteResult>;
	executeBatch?(
		statements: NativeBatchStatement[],
	): Promise<NativeExecuteResult[]>;
	beginStateTransaction?(
		timeoutMs?: number,
		context?: unknown,
	): Promise<JsNativeTransactionLike>;
	beginTransaction(
		timeoutMs?: number,
		name?: string,
	): Promise<JsNativeTransactionLike>;
	query(
		sql: string,
		params?: NativeBindParam[] | null,
	): Promise<NativeQueryResult>;
	run(
		sql: string,
		params?: NativeBindParam[] | null,
	): Promise<NativeRunResult>;
	metrics?(): SqliteNativeMetrics | null;
	takeLastKvError?(): string | null;
	close(): Promise<void>;
}
export type StateAwareSqliteDatabase = SqliteDatabase & {
	beginStateTransaction(
		timeoutMs?: number,
		context?: unknown,
	): Promise<SqliteTransactionDatabase>;
};

export interface JsNativeTransactionLike {
	exec(sql: string): Promise<NativeExecResult>;
	execute(
		sql: string,
		params?: NativeBindParam[] | null,
	): Promise<NativeExecuteResult>;
	commit(): Promise<void>;
	rollback(): Promise<void>;
}

function shouldAttachNativeKvError(message: string): boolean {
	return /i\/o error|unable to open database file/i.test(message);
}

function enrichNativeDatabaseError(
	database: JsNativeDatabaseLike,
	error: unknown,
): never {
	const bridgeReason =
		typeof error === "string"
			? error
			: error instanceof Error
				? error.message
				: undefined;
	const bridged =
		bridgeReason === undefined
			? undefined
			: decodeBridgeRivetError(bridgeReason);
	if (bridged) {
		throw bridged;
	}

	const kvError = database.takeLastKvError?.();
	if (
		error instanceof Error &&
		kvError &&
		shouldAttachNativeKvError(error.message) &&
		!error.message.includes(kvError)
	) {
		error.message = `${error.message} (native sqlite kv error: ${kvError})`;
	}
	throw error;
}

function toNativeBinding(arg: unknown): NativeBindParam {
	if (arg === null || arg === undefined) {
		return { kind: "null" };
	}
	if (typeof arg === "bigint") {
		return { kind: "int", intValue: Number(arg) };
	}
	if (typeof arg === "number") {
		if (Number.isInteger(arg)) {
			return { kind: "int", intValue: arg };
		}
		return { kind: "float", floatValue: arg };
	}
	if (typeof arg === "string") {
		return { kind: "text", textValue: arg };
	}
	if (typeof arg === "boolean") {
		return { kind: "int", intValue: arg ? 1 : 0 };
	}
	if (arg instanceof Uint8Array) {
		return { kind: "blob", blobValue: Buffer.from(arg) };
	}
	throw new Error(`unsupported bind parameter type: ${typeof arg}`);
}

function extractNamedSqliteParameters(sql: string): string[] {
	const orderedNames: string[] = [];
	const seen = new Set<string>();
	const pattern = /([:@$][A-Za-z_][A-Za-z0-9_]*)/g;
	for (const match of sql.matchAll(pattern)) {
		const name = match[1];
		if (seen.has(name)) {
			continue;
		}
		seen.add(name);
		orderedNames.push(name);
	}
	return orderedNames;
}

function getNamedSqliteBinding(
	bindings: Record<string, unknown>,
	name: string,
): unknown {
	if (name in bindings) {
		return bindings[name];
	}

	const bareName = name.slice(1);
	if (bareName in bindings) {
		return bindings[bareName];
	}

	for (const prefix of [":", "@", "$"] as const) {
		const candidate = `${prefix}${bareName}`;
		if (candidate in bindings) {
			return bindings[candidate];
		}
	}

	return undefined;
}

function toNativeBindings(
	sql: string,
	params?: SqliteBindings,
): NativeBindParam[] | null {
	if (params === undefined) {
		return null;
	}

	if (Array.isArray(params)) {
		return params.map((arg) => toNativeBinding(arg));
	}

	const orderedNames = extractNamedSqliteParameters(sql);
	if (orderedNames.length === 0) {
		return Object.values(params).map((arg) => toNativeBinding(arg));
	}

	return orderedNames.map((name) => {
		const value = getNamedSqliteBinding(params, name);
		if (value === undefined) {
			throw new Error(`missing bind parameter: ${name}`);
		}
		return toNativeBinding(value);
	});
}

function toNativeBatchStatement(
	statement: SqliteBatchStatement,
): NativeBatchStatement {
	return {
		sql: statement.sql,
		params: toNativeBindings(statement.sql, statement.params),
	};
}

function normalizeNativeMetrics(
	metrics: SqliteNativeMetrics | null | undefined,
): SqliteNativeMetrics | null {
	if (!metrics) return null;
	const raw = metrics as unknown as Record<string, unknown>;
	const numberField = (camel: string, snake: string) =>
		Number(raw[camel] ?? raw[snake] ?? 0);

	return {
		requestBuildNs: numberField("requestBuildNs", "request_build_ns"),
		serializeNs: numberField("serializeNs", "serialize_ns"),
		transportNs: numberField("transportNs", "transport_ns"),
		stateUpdateNs: numberField("stateUpdateNs", "state_update_ns"),
		totalNs: numberField("totalNs", "total_ns"),
		commitCount: numberField("commitCount", "commit_count"),
		pageCacheEntries: numberField("pageCacheEntries", "page_cache_entries"),
		pageCacheWeightedSize: numberField(
			"pageCacheWeightedSize",
			"page_cache_weighted_size",
		),
		pageCacheCapacityPages: numberField(
			"pageCacheCapacityPages",
			"page_cache_capacity_pages",
		),
		writeBufferDirtyPages: numberField(
			"writeBufferDirtyPages",
			"write_buffer_dirty_pages",
		),
		dbSizePages: numberField("dbSizePages", "db_size_pages"),
	};
}

class NativeCloseGate {
	#active = 0;
	#closed = false;
	#waiters: (() => void)[] = [];

	enter(): () => void {
		if (this.#closed) {
			throw new Error(
				"Database is closed. This usually means a background timer (setInterval, setTimeout) or a stray promise is still running after the actor stopped. Use c.abortSignal to clean up timers before the actor shuts down.",
			);
		}

		this.#active++;
		let released = false;
		return () => {
			if (released) {
				return;
			}
			released = true;
			this.#active--;
			if (this.#active === 0) {
				const waiters = this.#waiters.splice(0);
				for (const waiter of waiters) {
					waiter();
				}
			}
		};
	}

	async close(callback: () => Promise<void>): Promise<void> {
		if (this.#closed) {
			return;
		}
		this.#closed = true;
		if (this.#active > 0) {
			await new Promise<void>((resolve) => this.#waiters.push(resolve));
		}
		await callback();
	}
}

export function wrapJsNativeDatabase(
	database: JsNativeDatabaseLike,
): StateAwareSqliteDatabase {
	const gate = new NativeCloseGate();
	let closePromise: Promise<void> | undefined;
	let lastInsertRowId: number | null = null;

	const executeNative = async (
		sql: string,
		params?: SqliteBindings,
	): Promise<SqliteExecuteResult> => {
		const lastInsertRowIdColumn = lastInsertRowIdColumnName(sql);
		if (lastInsertRowIdColumn) {
			return {
				columns: [lastInsertRowIdColumn],
				rows: [[lastInsertRowId ?? 0]],
				changes: 0,
				lastInsertRowId,
			};
		}

		const release = gate.enter();
		try {
			const nativeParams = toNativeBindings(sql, params);
			const result = await database.execute(sql, nativeParams);
			if (result.lastInsertRowId !== undefined) {
				lastInsertRowId = result.lastInsertRowId;
			}
			return result;
		} catch (error) {
			enrichNativeDatabaseError(database, error);
		} finally {
			release();
		}
	};

	return {
		async exec(
			sql: string,
			callback?: (row: unknown[], columns: string[]) => void,
		): Promise<void> {
			const release = gate.enter();
			let result: NativeExecResult;
			try {
				result = await database.exec(sql);
			} catch (error) {
				enrichNativeDatabaseError(database, error);
			} finally {
				release();
			}
			if (!callback) {
				return;
			}
			for (const row of result.rows) {
				callback(row, result.columns);
			}
		},
		async execute(
			sql: string,
			params?: SqliteBindings,
		): Promise<SqliteExecuteResult> {
			return await executeNative(sql, params);
		},
		async executeBatch(
			statements: SqliteBatchStatement[],
		): Promise<SqliteExecuteResult[]> {
			const nativeStatements = statements.map(toNativeBatchStatement);
			const release = gate.enter();
			let transaction: JsNativeTransactionLike | undefined;
			try {
				const results = database.executeBatch
					? await database.executeBatch(nativeStatements)
					: await (async () => {
							transaction = await database.beginTransaction();
							const transactionResults: NativeExecuteResult[] =
								[];
							for (const statement of nativeStatements) {
								transactionResults.push(
									await transaction.execute(
										statement.sql,
										statement.params,
									),
								);
							}
							await transaction.commit();
							transaction = undefined;
							return transactionResults;
						})();
				for (const result of results) {
					if (result.lastInsertRowId !== undefined) {
						lastInsertRowId = result.lastInsertRowId;
					}
				}
				return results;
			} catch (error) {
				if (transaction) {
					try {
						await transaction.rollback();
					} catch {
						// The original batch error is more actionable than cleanup failure.
					}
				}
				enrichNativeDatabaseError(database, error);
			} finally {
				release();
			}
		},
		async beginTransaction(
			timeoutMs?: number,
			name?: string,
		): Promise<SqliteTransactionDatabase> {
			const release = gate.enter();
			let transaction: JsNativeTransactionLike;
			try {
				transaction = await database.beginTransaction(timeoutMs, name);
			} catch (error) {
				enrichNativeDatabaseError(database, error);
			} finally {
				release();
			}
			return wrapTransaction(database, transaction, gate, (result) => {
				if (result.lastInsertRowId !== undefined) {
					lastInsertRowId = result.lastInsertRowId;
				}
			});
		},
		async beginStateTransaction(
			timeoutMs?: number,
			context?: unknown,
		): Promise<SqliteTransactionDatabase> {
			if (!database.beginStateTransaction) {
				throw new Error("actor state transactions are not configured");
			}
			const release = gate.enter();
			let transaction: JsNativeTransactionLike;
			try {
				transaction = await database.beginStateTransaction(
					timeoutMs,
					context,
				);
			} catch (error) {
				enrichNativeDatabaseError(database, error);
			} finally {
				release();
			}
			return wrapTransaction(database, transaction, gate, (result) => {
				if (result.lastInsertRowId !== undefined) {
					lastInsertRowId = result.lastInsertRowId;
				}
			});
		},
		async run(sql: string, params?: SqliteBindings): Promise<void> {
			await executeNative(sql, params);
		},
		async query(sql: string, params?: SqliteBindings) {
			const { columns, rows } = await executeNative(sql, params);
			return { columns, rows };
		},
		nativeMetrics(): SqliteNativeMetrics | null {
			return normalizeNativeMetrics(database.metrics?.());
		},
		async close(): Promise<void> {
			closePromise ??= gate.close(() => database.close());
			await closePromise;
		},
	};
}

function wrapTransaction(
	database: JsNativeDatabaseLike,
	transaction: JsNativeTransactionLike,
	gate: NativeCloseGate,
	onExecute: (result: NativeExecuteResult) => void,
): SqliteTransactionDatabase {
	return {
		async exec(sql, callback) {
			const release = gate.enter();
			let result: NativeExecResult;
			try {
				result = await transaction.exec(sql);
			} catch (error) {
				enrichNativeDatabaseError(database, error);
			} finally {
				release();
			}
			if (callback) {
				for (const row of result.rows) callback(row, result.columns);
			}
		},
		async execute(sql, params) {
			const release = gate.enter();
			try {
				const result = await transaction.execute(
					sql,
					toNativeBindings(sql, params),
				);
				onExecute(result);
				return result;
			} catch (error) {
				enrichNativeDatabaseError(database, error);
			} finally {
				release();
			}
		},
		async commit() {
			const release = gate.enter();
			try {
				await transaction.commit();
			} catch (error) {
				enrichNativeDatabaseError(database, error);
			} finally {
				release();
			}
		},
		async rollback() {
			const release = gate.enter();
			try {
				await transaction.rollback();
			} catch (error) {
				enrichNativeDatabaseError(database, error);
			} finally {
				release();
			}
		},
	};
}

function lastInsertRowIdColumnName(sql: string): string | undefined {
	const match = sql.match(
		/^\s*SELECT\s+last_insert_rowid\s*\(\s*\)\s*(?:AS\s+("[^"]+"|`[^`]+`|\[[^\]]+\]|\w+))?\s*;?\s*$/i,
	);
	if (!match) {
		return undefined;
	}

	const alias = match[1];
	if (!alias) {
		return "last_insert_rowid()";
	}
	if (
		(alias.startsWith('"') && alias.endsWith('"')) ||
		(alias.startsWith("`") && alias.endsWith("`")) ||
		(alias.startsWith("[") && alias.endsWith("]"))
	) {
		return alias.slice(1, -1);
	}
	return alias;
}
