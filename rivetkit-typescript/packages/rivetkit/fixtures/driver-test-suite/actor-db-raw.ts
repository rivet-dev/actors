import { actor } from "rivetkit";
import { promiseWithResolvers } from "rivetkit/utils";
import { db } from "@/common/database/mod";
import type { registry } from "./registry-static";
import { scheduleActorSleep } from "./schedule-sleep";

function firstRowValue(row: Record<string, unknown> | undefined): unknown {
	if (!row) {
		return undefined;
	}

	const values = Object.values(row);
	return values.length > 0 ? values[0] : undefined;
}

function toSafeInteger(value: unknown): number {
	if (typeof value === "bigint") {
		return Number(value);
	}
	if (typeof value === "number") {
		return Number.isFinite(value) ? Math.trunc(value) : 0;
	}
	if (typeof value === "string") {
		const parsed = Number.parseInt(value, 10);
		return Number.isFinite(parsed) ? parsed : 0;
	}
	return 0;
}

function normalizeRowIds(rowIds: number[]): number[] {
	const normalized = rowIds
		.map((id) => Math.trunc(id))
		.filter((id) => Number.isFinite(id) && id > 0);
	return Array.from(new Set(normalized));
}

function makePayload(size: number): string {
	const normalizedSize = Math.max(0, Math.trunc(size));
	return "x".repeat(normalizedSize);
}

async function recordLifecycleEvent(
	c: {
		key: string[];
		state: { lifecycleObserverKey: string | null };
		client: <T>() => T;
	},
	event: string,
) {
	const observerKey = c.state.lifecycleObserverKey;
	if (!observerKey) {
		return;
	}

	const client = c.client<typeof registry>();
	const observer = client.lifecycleObserver.getOrCreate([observerKey]);
	await observer.recordEvent({
		actorKey: c.key.join("/"),
		event,
	});
}

export const dbActorRaw = actor({
	state: {
		disconnectInsertEnabled: false,
		disconnectInsertDelayMs: 0,
		lifecycleObserverKey: null as string | null,
		transactionHolding: false,
		atomicStateValue: null as string | null,
		baselineCloneProbe: null as ArrayBuffer | null,
	},
	db: db({
		onMigrate: async (db) => {
			await db.execute(`
				CREATE TABLE IF NOT EXISTS test_data (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					value TEXT NOT NULL,
					payload TEXT NOT NULL DEFAULT '',
					created_at INTEGER NOT NULL
				)
			`);
		},
	}),
	createVars: () =>
		({
			actorAwakeHeld: false,
			actorAwakeRelease: undefined,
			sqlOperationSubmitted: false,
			transactionRelease: undefined,
			stateTransactionStarted: undefined,
			stateTransactionRelease: undefined,
			stateChangeValues: [] as Array<string | null>,
		}) as {
			actorAwakeHeld: boolean;
			actorAwakeRelease: PromiseWithResolvers<void> | undefined;
			sqlOperationSubmitted: boolean;
			transactionRelease: PromiseWithResolvers<void> | undefined;
			stateTransactionStarted: PromiseWithResolvers<void> | undefined;
			stateTransactionRelease: PromiseWithResolvers<void> | undefined;
			stateChangeValues: Array<string | null>;
		},
	createConnState: () => ({ atomicStateValue: null as string | null }),
	onStateChange: (c, state) => {
		c.vars.stateChangeValues.push(state.atomicStateValue);
	},
	onWake: async (c) => {
		await recordLifecycleEvent(c, "wake");
	},
	onSleep: async (c) => {
		await recordLifecycleEvent(c, "sleep");
	},
	onDisconnect: async (c) => {
		if (!c.state.disconnectInsertEnabled) {
			return;
		}

		if (c.state.disconnectInsertDelayMs > 0) {
			await new Promise<void>((resolve) =>
				setTimeout(resolve, c.state.disconnectInsertDelayMs),
			);
		}

		await c.db.execute(
			"INSERT INTO test_data (value, payload, created_at) VALUES (?, ?, ?)",
			"__disconnect__",
			"",
			Date.now(),
		);
	},
	actions: {
		getActorRuntimeSocketPath: async (c) => {
			return (await c.actorRuntimeSocket()).path;
		},
		holdActorAwake: async (c) => {
			c.vars.actorAwakeRelease = promiseWithResolvers((reason) =>
				c.log.warn({
					msg: "actor wake hold rejected",
					reason,
				}),
			);
			c.vars.actorAwakeHeld = true;
			try {
				await c.vars.actorAwakeRelease.promise;
			} finally {
				c.vars.actorAwakeHeld = false;
				c.vars.actorAwakeRelease = undefined;
			}
		},
		releaseActorAwake: (c) => c.vars.actorAwakeRelease?.resolve(),
		isActorAwakeHeld: (c) => c.vars.actorAwakeHeld,
		holdTransaction: async (c) => {
			await c.db.transaction(async (tx) => {
				await tx.execute("SELECT 1");
				c.vars.transactionRelease = promiseWithResolvers((reason) =>
					c.log.warn({
						msg: "held transaction release rejected",
						reason,
					}),
				);
				c.state.transactionHolding = true;
				try {
					await c.vars.transactionRelease.promise;
				} finally {
					c.state.transactionHolding = false;
					c.vars.transactionRelease = undefined;
				}
			});
		},
		releaseHeldTransaction: (c) => c.vars.transactionRelease?.resolve(),
		isTransactionHolding: (c) => c.state.transactionHolding,
		configureDisconnectInsert: (c, enabled: boolean, delayMs: number) => {
			c.state.disconnectInsertEnabled = enabled;
			c.state.disconnectInsertDelayMs = Math.max(0, Math.floor(delayMs));
		},
		configureLifecycleObserver: (c, observerKey: string | null) => {
			c.state.lifecycleObserverKey = observerKey;
		},
		getDisconnectInsertCount: async (c) => {
			const results = await c.db.execute<{ count: number }>(
				`SELECT COUNT(*) as count FROM test_data WHERE value = '__disconnect__'`,
			);
			return results[0]?.count ?? 0;
		},
		reset: async (c) => {
			await c.db.execute(`DELETE FROM test_data`);
		},
		insertValue: async (c, value: string) => {
			await c.db.execute(
				"INSERT INTO test_data (value, payload, created_at) VALUES (?, ?, ?)",
				value,
				"",
				Date.now(),
			);
			const results = await c.db.execute<{ id: number }>(
				`SELECT last_insert_rowid() as id`,
			);
			return { id: results[0].id };
		},
		insertValueAndReadBack: async (c, value: string) => {
			await c.db.execute(
				"INSERT INTO test_data (value, payload, created_at) VALUES (?, ?, ?)",
				value,
				"",
				Date.now(),
			);
			const inserted = await c.db.execute<{
				id: number;
				value: string;
				hex_value: string;
				sqlite_length: number;
			}>(
				`SELECT id, value, hex(value) as hex_value, length(value) as sqlite_length FROM test_data ORDER BY id DESC LIMIT 1`,
			);
			return inserted[0] ?? null;
		},
		getValues: async (c) => {
			const results = await c.db.execute<{
				id: number;
				value: string;
				payload: string;
				created_at: number;
			}>(`SELECT * FROM test_data ORDER BY id`);
			return results;
		},
		getValue: async (c, id: number) => {
			const results = await c.db.execute<{ value: string }>(
				"SELECT value FROM test_data WHERE id = ?",
				id,
			);
			return results[0]?.value ?? null;
		},
		getCount: async (c) => {
			const results = await c.db.execute<{ count: number }>(
				`SELECT COUNT(*) as count FROM test_data`,
			);
			return results[0].count;
		},
		getCountWithSubmissionBarrier: async (c) => {
			const pending = c.db.execute<{ count: number }>(
				`SELECT COUNT(*) as count FROM test_data`,
			);
			c.vars.sqlOperationSubmitted = true;
			try {
				const results = await pending;
				return results[0].count;
			} finally {
				c.vars.sqlOperationSubmitted = false;
			}
		},
		isSqlOperationSubmitted: (c) => c.vars.sqlOperationSubmitted,
		rawSelectCount: async (c) => {
			const results = await c.db.execute<{ count: number }>(
				`SELECT COUNT(*) as count FROM test_data`,
			);
			return results[0].count;
		},
		insertMany: async (c, count: number) => {
			if (count <= 0) {
				return { count: 0 };
			}
			const now = Date.now();
			const values: string[] = [];
			for (let i = 0; i < count; i++) {
				values.push(`('User ${i}', '', ${now})`);
			}
			await c.db.execute(
				`INSERT INTO test_data (value, payload, created_at) VALUES ${values.join(", ")}`,
			);
			return { count };
		},
		updateValue: async (c, id: number, value: string) => {
			await c.db.execute(
				"UPDATE test_data SET value = ? WHERE id = ?",
				value,
				id,
			);
			return { success: true };
		},
		deleteValue: async (c, id: number) => {
			await c.db.execute("DELETE FROM test_data WHERE id = ?", id);
		},
		transactionCommit: async (c, value: string) => {
			await c.db.transaction(async (tx) => {
				await tx.execute(
					"INSERT INTO test_data (value, payload, created_at) VALUES (?, '', ?)",
					value,
					Date.now(),
				);
			});
		},
		transactionRollback: async (c, value: string) => {
			try {
				await c.db.transaction(async (tx) => {
					await tx.execute(
						"INSERT INTO test_data (value, payload, created_at) VALUES (?, '', ?)",
						value,
						Date.now(),
					);
					throw new Error("expected rollback");
				});
			} catch (error) {
				if (
					!(error instanceof Error) ||
					error.message !== "expected rollback"
				) {
					throw error;
				}
			}
		},
		stateTransactionCommit: async (c, value: string) => {
			await c.db.transaction(
				async (tx) => {
					await tx.execute(
						"INSERT INTO test_data (value, payload, created_at) VALUES (?, '', ?)",
						value,
						Date.now(),
					);
					c.state.atomicStateValue = value;
				},
				{ experimental: { includeState: true } },
			);
			return {
				state: c.state.atomicStateValue,
				count: await c.db.execute<{ count: number }>(
					"SELECT COUNT(*) AS count FROM test_data",
				),
			};
		},
		stateTransactionRollback: async (c, value: string) => {
			try {
				await c.db.transaction(
					async (tx) => {
						await tx.execute(
							"INSERT INTO test_data (value, payload, created_at) VALUES (?, '', ?)",
							value,
							Date.now(),
						);
						c.state.atomicStateValue = value;
						throw new Error("expected state transaction rollback");
					},
					{ experimental: { includeState: true } },
				);
			} catch (error) {
				if (
					!(error instanceof Error) ||
					error.message !== "expected state transaction rollback"
				) {
					throw error;
				}
			}
			return {
				state: c.state.atomicStateValue,
				count: await c.db.execute<{ count: number }>(
					"SELECT COUNT(*) AS count FROM test_data",
				),
			};
		},
		stateTransactionHoldAndRollback: async (c, value: string) => {
			c.vars.stateTransactionStarted = promiseWithResolvers<void>();
			c.vars.stateTransactionRelease = promiseWithResolvers<void>();
			try {
				await c.db.transaction(
					async (tx) => {
						await tx.execute("SELECT 1");
						c.state.atomicStateValue = value;
						c.vars.stateTransactionStarted?.resolve();
						await c.vars.stateTransactionRelease?.promise;
						throw new Error(
							"expected held state transaction rollback",
						);
					},
					{ experimental: { includeState: true } },
				);
			} catch (error) {
				if (
					!(error instanceof Error) ||
					error.message !== "expected held state transaction rollback"
				) {
					throw error;
				}
			}
			return c.state.atomicStateValue;
		},
		stateTransactionRejectsMultiStatement: async (c) => {
			try {
				await c.db.transaction(
					async (tx) => {
						await tx.execute("SELECT 1; SELECT 2");
					},
					{ experimental: { includeState: true } },
				);
			} catch (error) {
				return error instanceof Error ? error.message : String(error);
			}
			throw new Error("expected multi-statement transaction to fail");
		},
		waitForStateTransaction: async (c) => {
			while (!c.vars.stateTransactionStarted) {
				await new Promise((resolve) => setTimeout(resolve, 0));
			}
			await c.vars.stateTransactionStarted.promise;
		},
		mutateStateDuringTransaction: async (c, value: string) => {
			try {
				c.state.atomicStateValue = value;
			} finally {
				c.vars.stateTransactionRelease?.resolve();
			}
		},
		stateTransactionSetValue: async (c, value: string) => {
			await c.db.transaction(
				async (tx) => {
					await tx.execute("SELECT 1");
					c.state.atomicStateValue = value;
				},
				{ experimental: { includeState: true } },
			);
			return c.state.atomicStateValue;
		},
		releaseStateTransaction: (c) => {
			c.vars.stateTransactionRelease?.resolve();
		},
		stateTransactionPreservesPredirtyState: async (c) => {
			c.state.atomicStateValue = "before";
			try {
				await c.db.transaction(
					async (tx) => {
						await tx.execute("SELECT 1");
						c.state.atomicStateValue = "inside";
						throw new Error("expected pre-dirty rollback");
					},
					{ experimental: { includeState: true } },
				);
			} catch (error) {
				if (
					!(error instanceof Error) ||
					error.message !== "expected pre-dirty rollback"
				) {
					throw error;
				}
			}
			return c.state.atomicStateValue;
		},
		stateTransactionRejectsRetainedActorStateAfterRollback: async (c) => {
			const retainedState = c.state;
			try {
				await c.db.transaction(
					async (tx) => {
						await tx.execute("SELECT 1");
						retainedState.atomicStateValue = "inside";
						throw new Error(
							"expected retained actor state rollback",
						);
					},
					{ experimental: { includeState: true } },
				);
			} catch (error) {
				if (
					!(error instanceof Error) ||
					error.message !== "expected retained actor state rollback"
				) {
					throw error;
				}
			}

			let staleWriteErrorCode: string | undefined;
			try {
				retainedState.atomicStateValue = "stale";
			} catch (error) {
				staleWriteErrorCode = (error as { code?: string }).code;
			}
			return {
				staleWriteErrorCode,
				state: c.state.atomicStateValue,
			};
		},
		stateTransactionRejectsRetainedConnStateAfterRollback: async (c) => {
			const conn = c.conns.values().next().value;
			if (!conn) {
				throw new Error("expected an active connection");
			}
			const retainedState = conn.state;
			try {
				await c.db.transaction(
					async (tx) => {
						await tx.execute("SELECT 1");
						retainedState.atomicStateValue = "inside";
						throw new Error(
							"expected retained connection state rollback",
						);
					},
					{ experimental: { includeState: true } },
				);
			} catch (error) {
				if (
					!(error instanceof Error) ||
					error.message !==
						"expected retained connection state rollback"
				) {
					throw error;
				}
			}

			let staleWriteErrorCode: string | undefined;
			try {
				retainedState.atomicStateValue = "stale";
			} catch (error) {
				staleWriteErrorCode = (error as { code?: string }).code;
			}
			return {
				staleWriteErrorCode,
				state: conn.state.atomicStateValue,
			};
		},
		stateTransactionRecoversFromBaselineCloneFailure: async (c) => {
			const detached = new ArrayBuffer(1);
			structuredClone(detached, { transfer: [detached] });
			c.state.baselineCloneProbe = detached;
			try {
				await c.db.transaction(async () => {}, {
					experimental: { includeState: true },
				});
			} catch {
				// The detached buffer intentionally makes baseline cloning fail.
			}
			c.state.baselineCloneProbe = null;
			await c.db.transaction(
				async (tx) => {
					await tx.execute("SELECT 1");
					c.state.atomicStateValue = "after";
				},
				{ experimental: { includeState: true } },
			);
			return c.state.atomicStateValue;
		},
		stateTransactionHidesRolledBackStateFromOnStateChange: async (c) => {
			c.state.atomicStateValue = "before";
			await new Promise<void>((resolve) => setImmediate(resolve));
			c.vars.stateChangeValues = [];
			try {
				await c.db.transaction(
					async (tx) => {
						await tx.execute("SELECT 1");
						c.state.atomicStateValue = "inside";
						await new Promise<void>((resolve) =>
							setImmediate(resolve),
						);
						throw new Error("expected onStateChange rollback");
					},
					{ experimental: { includeState: true } },
				);
			} catch (error) {
				if (
					!(error instanceof Error) ||
					error.message !== "expected onStateChange rollback"
				) {
					throw error;
				}
			}
			await new Promise<void>((resolve) => setImmediate(resolve));
			return {
				state: c.state.atomicStateValue,
				observed: c.vars.stateChangeValues,
			};
		},
		stateTransactionRejectsImmediateSave: async (c) => {
			let errorCode: string | undefined;
			let errorMessage: string | undefined;
			try {
				await c.db.transaction(
					async (tx) => {
						await tx.execute("SELECT 1");
						c.state.atomicStateValue = "inside";
						await Promise.race([
							c.saveState({ immediate: true }),
							new Promise<never>((_, reject) =>
								setTimeout(
									() =>
										reject(
											new Error(
												"immediate save timed out",
											),
										),
									250,
								),
							),
						]);
					},
					{ experimental: { includeState: true } },
				);
			} catch (error) {
				errorCode = (error as { code?: string }).code;
				errorMessage =
					error instanceof Error ? error.message : String(error);
			}

			await c.db.transaction(
				async (tx) => {
					await tx.execute("SELECT 1");
					c.state.atomicStateValue = "after";
				},
				{ experimental: { includeState: true } },
			);
			return { errorCode, errorMessage, state: c.state.atomicStateValue };
		},
		stateTransactionRejectsOuterClientReentry: async (c) => {
			await c.db.transaction(
				async () => {
					await c.db.transaction(async () => {}, {
						experimental: { includeState: true },
					});
				},
				{ experimental: { includeState: true } },
			);
		},
		transactionWithTimeout: async (c, timeout: number) => {
			await c.db.transaction(
				async (tx) => {
					await tx.execute("SELECT 1");
				},
				{ timeout },
			);
		},
		transactionExec: async (c) => {
			await c.db.transaction(async (tx) => {
				await tx.execute(`
					INSERT INTO test_data (value, payload, created_at) VALUES ('exec-first', '', ${Date.now()});
					INSERT INTO test_data (value, payload, created_at) VALUES ('exec-second', '', ${Date.now()});
				`);
			});
			return (
				await c.db.execute<{ value: string }>(
					"SELECT value FROM test_data ORDER BY id",
				)
			).map((row) => row.value);
		},
		transactionAutomaticRollback: async (c) => {
			await c.db.execute(`
				CREATE TABLE IF NOT EXISTS transaction_rollback_probe (
					id INTEGER PRIMARY KEY,
					value TEXT NOT NULL
				);
				DELETE FROM transaction_rollback_probe;
			`);
			try {
				await c.db.transaction(async (tx) => {
					await tx.execute(
						"INSERT INTO transaction_rollback_probe(id, value) VALUES (1, 'first')",
					);
					await tx.execute(
						"INSERT OR ROLLBACK INTO transaction_rollback_probe(id, value) VALUES (1, 'duplicate')",
					);
				});
			} catch {
				// SQLite's ROLLBACK conflict policy has already ended the transaction.
			}
			await c.db.execute(
				"INSERT INTO transaction_rollback_probe(id, value) VALUES (2, 'after')",
			);
			return await c.db.execute<{ id: number; value: string }>(
				"SELECT id, value FROM transaction_rollback_probe ORDER BY id",
			);
		},
		concurrentTransactions: async (c) => {
			let markFirstTransactionStarted: (() => void) | undefined;
			const firstTransactionStarted = new Promise<void>((resolve) => {
				markFirstTransactionStarted = resolve;
			});
			const firstTransaction = c.db.transaction(async (tx) => {
				await tx.execute(
					"INSERT INTO test_data (value, payload, created_at) VALUES ('first-start', '', ?)",
					Date.now(),
				);
				markFirstTransactionStarted?.();
				await new Promise((resolve) => setTimeout(resolve, 20));
				await tx.execute(
					"INSERT INTO test_data (value, payload, created_at) VALUES ('first-end', '', ?)",
					Date.now(),
				);
			});
			await firstTransactionStarted;
			await Promise.all([
				firstTransaction,
				c.db.transaction(async (tx) => {
					await tx.execute(
						"INSERT INTO test_data (value, payload, created_at) VALUES ('second', '', ?)",
						Date.now(),
					);
				}),
				c.db.transaction(async (tx) => {
					await tx.execute(
						"INSERT INTO test_data (value, payload, created_at) VALUES ('third', '', ?)",
						Date.now(),
					);
				}),
			]);
			return (
				await c.db.execute<{ value: string }>(
					"SELECT value FROM test_data ORDER BY id",
				)
			).map((row) => row.value);
		},
		transactionParksOrdinarySql: async (c) => {
			let markTransactionStarted: (() => void) | undefined;
			const transactionStarted = new Promise<void>((resolve) => {
				markTransactionStarted = resolve;
			});
			await Promise.all([
				c.db.transaction(async (tx) => {
					await tx.execute(
						"INSERT INTO test_data (value, payload, created_at) VALUES ('tx-start', '', ?)",
						Date.now(),
					);
					markTransactionStarted?.();
					await new Promise((resolve) => setTimeout(resolve, 20));
					await tx.execute(
						"INSERT INTO test_data (value, payload, created_at) VALUES ('tx-end', '', ?)",
						Date.now(),
					);
				}),
				transactionStarted.then(() =>
					c.db.execute(
						"INSERT INTO test_data (value, payload, created_at) VALUES ('ordinary', '', ?)",
						Date.now(),
					),
				),
			]);
			return (
				await c.db.execute<{ value: string }>(
					"SELECT value FROM test_data ORDER BY id",
				)
			).map((row) => row.value);
		},
		transactionDeadlockDiagnostic: async (c, nested: boolean) => {
			try {
				await c.db.transaction(
					async (tx) => {
						if (nested) {
							await tx.transaction(async () => {});
						} else {
							await c.db.execute("SELECT 1");
						}
					},
					{ timeout: 50 },
				);
				return "unexpected success";
			} catch (error) {
				return error instanceof Error ? error.message : String(error);
			}
		},
		transactionExpiryDiagnostics: async (c) => {
			let leakedTransaction: { execute: typeof c.db.execute } | undefined;
			let parked:
				| Promise<
						| { status: "executed" }
						| { status: "rejected"; message: string }
				  >
				| undefined;
			let ownerMessage = "unexpected success";
			try {
				await c.db.transaction(
					async (tx) => {
						leakedTransaction = tx;
						await tx.execute(
							"INSERT INTO test_data (value, payload, created_at) VALUES ('rolled-back-owner', '', ?)",
							Date.now(),
						);
						parked = c.db
							.execute(
								"INSERT INTO test_data (value, payload, created_at) VALUES ('must-not-run', '', ?)",
								Date.now(),
							)
							.then(
								() => ({ status: "executed" as const }),
								(error) => ({
									status: "rejected" as const,
									message:
										error instanceof Error
											? error.message
											: String(error),
								}),
							);
						await new Promise((resolve) => setTimeout(resolve, 80));
						await tx.execute("SELECT 1");
					},
					{ timeout: 50 },
				);
			} catch (error) {
				ownerMessage =
					error instanceof Error ? error.message : String(error);
			}

			const parkedResult = await parked;
			if (!parkedResult) {
				throw new Error("parked operation was not submitted");
			}
			let leakedMessage = "unexpected success";
			if (!leakedTransaction) {
				throw new Error("transaction handle was not captured");
			}
			try {
				await leakedTransaction.execute("SELECT 1");
			} catch (error) {
				leakedMessage =
					error instanceof Error ? error.message : String(error);
			}
			const values = (
				await c.db.execute<{ value: string }>(
					"SELECT value FROM test_data WHERE value IN ('rolled-back-owner', 'must-not-run') ORDER BY value",
				)
			).map((row) => row.value);
			return { ownerMessage, parkedResult, leakedMessage, values };
		},
		terminalTransactionDiagnostic: async (c) => {
			let completedTransaction:
				| { execute: typeof c.db.execute }
				| undefined;
			await c.db.transaction(async (tx) => {
				completedTransaction = tx;
				await tx.execute("SELECT 1");
			});
			if (!completedTransaction) {
				throw new Error("transaction handle was not captured");
			}
			try {
				await completedTransaction.execute("SELECT 1");
				return "unexpected success";
			} catch (error) {
				return error instanceof Error ? error.message : String(error);
			}
		},
		manualTransactionCompatibility: async (c) => {
			await c.db.execute("BEGIN");
			try {
				await c.db.execute(
					"INSERT INTO test_data (value, payload, created_at) VALUES ('manual-compatible', '', ?)",
					Date.now(),
				);
				await c.db.execute("COMMIT");
			} catch (error) {
				await c.db.execute("ROLLBACK");
				throw error;
			}
			return (
				await c.db.execute<{ count: number }>(
					"SELECT count(*) AS count FROM test_data WHERE value = 'manual-compatible'",
				)
			)[0]?.count;
		},
		insertPayloadOfSize: async (c, size: number) => {
			const payload = "x".repeat(size);
			await c.db.execute(
				"INSERT INTO test_data (value, payload, created_at) VALUES (?, ?, ?)",
				"payload",
				payload,
				Date.now(),
			);
			const results = await c.db.execute<{ id: number }>(
				`SELECT last_insert_rowid() as id`,
			);
			return { id: results[0].id, size };
		},
		getPayloadSize: async (c, id: number) => {
			const results = await c.db.execute<{ size: number }>(
				"SELECT length(payload) as size FROM test_data WHERE id = ?",
				id,
			);
			return results[0]?.size ?? 0;
		},
		insertPayloadRows: async (c, count: number, payloadSize: number) => {
			const normalizedCount = Math.max(0, Math.trunc(count));
			if (normalizedCount === 0) {
				return { count: 0 };
			}

			const payload = makePayload(payloadSize);
			const now = Date.now();
			for (let i = 0; i < normalizedCount; i++) {
				await c.db.execute(
					"INSERT INTO test_data (value, payload, created_at) VALUES (?, ?, ?)",
					`bulk-${i}`,
					payload,
					now,
				);
			}

			return { count: normalizedCount };
		},
		roundRobinUpdateValues: async (
			c,
			rowIds: number[],
			iterations: number,
		) => {
			const normalizedRowIds = normalizeRowIds(rowIds);
			const normalizedIterations = Math.max(0, Math.trunc(iterations));
			if (normalizedRowIds.length === 0 || normalizedIterations === 0) {
				const emptyRows: Array<{ id: number; value: string }> = [];
				return emptyRows;
			}

			await c.db.transaction(async (tx) => {
				for (let i = 0; i < normalizedIterations; i++) {
					const rowId =
						normalizedRowIds[i % normalizedRowIds.length] ?? 0;
					await tx.execute(
						"UPDATE test_data SET value = ? WHERE id = ?",
						`v-${i}`,
						rowId,
					);
				}
			});

			return await c.db.execute<{ id: number; value: string }>(
				`SELECT id, value FROM test_data WHERE id IN (${normalizedRowIds.join(",")}) ORDER BY id`,
			);
		},
		getPageCount: async (c) => {
			const rows =
				await c.db.execute<Record<string, unknown>>(
					"PRAGMA page_count",
				);
			return toSafeInteger(firstRowValue(rows[0]));
		},
		vacuum: async (c) => {
			await c.db.execute("VACUUM");
		},
		integrityCheck: async (c) => {
			const rows = await c.db.execute<Record<string, unknown>>(
				"PRAGMA integrity_check",
			);
			const value = firstRowValue(rows[0]);
			return String(value ?? "");
		},
		runMixedWorkload: async (c, seedCount: number, churnCount: number) => {
			const normalizedSeedCount = Math.max(1, Math.trunc(seedCount));
			const normalizedChurnCount = Math.max(0, Math.trunc(churnCount));
			const now = Date.now();

			await c.db.transaction(async (tx) => {
				for (let i = 0; i < normalizedSeedCount; i++) {
					const payload = makePayload(1024 + (i % 5) * 128);
					await tx.execute(
						"INSERT OR REPLACE INTO test_data (id, value, payload, created_at) VALUES (?, ?, ?, ?)",
						i + 1,
						`seed-${i}`,
						payload,
						now,
					);
				}

				for (let i = 0; i < normalizedChurnCount; i++) {
					const id = (i % normalizedSeedCount) + 1;
					if (i % 9 === 0) {
						await tx.execute(
							"DELETE FROM test_data WHERE id = ?",
							id,
						);
					} else {
						const payload = makePayload(768 + (i % 7) * 96);
						await tx.execute(
							"INSERT OR REPLACE INTO test_data (id, value, payload, created_at) VALUES (?, ?, ?, ?)",
							id,
							`upd-${i}`,
							payload,
							now + i,
						);
					}
				}
			});
		},
		repeatUpdate: async (c, id: number, count: number) => {
			let value = "";
			if (count <= 0) {
				return { value };
			}
			const statements: string[] = ["BEGIN"];
			for (let i = 0; i < count; i++) {
				value = `Updated ${i}`;
				statements.push(
					`UPDATE test_data SET value = '${value}' WHERE id = ${id}`,
				);
			}
			statements.push("COMMIT");
			await c.db.execute(statements.join("; "));
			return { value };
		},
		multiStatementInsert: async (c, value: string) => {
			await c.db.execute(
				`BEGIN; INSERT INTO test_data (value, payload, created_at) VALUES ('${value}', '', ${Date.now()}); UPDATE test_data SET value = '${value}-updated' WHERE id = last_insert_rowid(); COMMIT;`,
			);
			const results = await c.db.execute<{ value: string }>(
				`SELECT value FROM test_data ORDER BY id DESC LIMIT 1`,
			);
			return results[0]?.value ?? null;
		},
		triggerSleep: (c) => {
			scheduleActorSleep(c);
		},
		destroy: (c) => {
			c.destroy();
		},
		ping: () => "pong",
	},
	options: {
		actionTimeout: 120_000,
		enableActorRuntimeSocket: true,
		sleepTimeout: 100,
	},
});

export const dbActorManualWarningsDisabled = actor({
	db: db({ warnOnManualTransactions: false }),
	actions: {
		manualTransactionCompatibility: async (c, marker: string) => {
			await c.db.execute("BEGIN");
			try {
				await c.db.execute("SELECT 1");
				await c.db.execute("COMMIT");
			} catch (error) {
				await c.db.execute("ROLLBACK");
				throw error;
			}
			console.log(marker);
			return true;
		},
	},
});

export const dbActorRuntimeSocketDisabled = actor({
	db: db(),
	actions: {
		getActorRuntimeSocketPath: async (c) => {
			return (await c.actorRuntimeSocket()).path;
		},
	},
});

export const actorRuntimeSocketWithoutDb = actor({
	actions: {
		getActorRuntimeSocketPath: async (c) => {
			return (await c.actorRuntimeSocket()).path;
		},
		destroy: (c) => c.destroy(),
	},
	options: {
		enableActorRuntimeSocket: true,
	},
});

export const dbRemoteLifecycleProbe = actor({
	db: db(),
	actions: {
		ping: () => "pong",
		triggerSleep: (c) => {
			scheduleActorSleep(c);
		},
	},
	options: {
		sleepTimeout: 100,
	},
});
