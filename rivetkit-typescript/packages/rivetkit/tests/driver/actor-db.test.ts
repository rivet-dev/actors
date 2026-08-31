// @ts-nocheck

import { once } from "node:events";
import { existsSync } from "node:fs";
import { createConnection, type Socket } from "node:net";
import { describe, expect, test, vi } from "vitest";
import {
	decodeServerFrame,
	decodeServerHello,
	encodeClientFrame,
} from "../../src/common/bare/generated/actor-runtime-socket-protocol/v1";
import {
	describeDriverMatrix,
	SQLITE_DRIVER_MATRIX_OPTIONS,
} from "./shared-matrix";
import { setupDriverTest, waitFor } from "./shared-utils";

type DbVariant = "raw";

const CHUNK_SIZE = 4096;
const LARGE_PAYLOAD_SIZE = 32768;
const HIGH_VOLUME_COUNT = 1000;
const SLEEP_WAIT_MS = 150;
const LIFECYCLE_POLL_INTERVAL_MS = 25;
const LIFECYCLE_POLL_ATTEMPTS = 40;
const REAL_TIMER_DB_TIMEOUT_MS = 180_000;
const CHUNK_BOUNDARY_SIZES = [
	CHUNK_SIZE - 1,
	CHUNK_SIZE,
	CHUNK_SIZE + 1,
	2 * CHUNK_SIZE - 1,
	2 * CHUNK_SIZE,
	2 * CHUNK_SIZE + 1,
	4 * CHUNK_SIZE - 1,
	4 * CHUNK_SIZE,
	4 * CHUNK_SIZE + 1,
];
const SHRINK_GROW_INITIAL_ROWS = 16;
const SHRINK_GROW_REGROW_ROWS = 10;
const SHRINK_GROW_INITIAL_PAYLOAD = 4096;
const SHRINK_GROW_REGROW_PAYLOAD = 6144;
const HOT_ROW_COUNT = 10;
const HOT_ROW_UPDATES = 240;
const INTEGRITY_SEED_COUNT = 64;
const INTEGRITY_CHURN_COUNT = 120;
const SLEEP_OBSERVER_TIMEOUT_MS =
	SLEEP_WAIT_MS + 4 * LIFECYCLE_POLL_INTERVAL_MS;

type LifecycleEvent = {
	actorKey: string;
	event: string;
	timestamp: number;
};

const ACTOR_RUNTIME_SOCKET_PROTOCOL_VERSION = Buffer.from([1, 0]);

function writeActorRuntimeSocketFrame(
	socket: Socket,
	payload: Uint8Array = new Uint8Array(),
): void {
	const versionedPayload = Buffer.concat([
		ACTOR_RUNTIME_SOCKET_PROTOCOL_VERSION,
		Buffer.from(payload),
	]);
	const header = Buffer.allocUnsafe(4);
	header.writeUInt32BE(versionedPayload.length);
	socket.write(Buffer.concat([header, versionedPayload]));
}

function createActorRuntimeSocketFrameReader(socket: Socket) {
	const chunks = socket[Symbol.asyncIterator]();
	let buffered = Buffer.alloc(0);

	return async (): Promise<Buffer> => {
		while (buffered.length < 4) {
			const next = await chunks.next();
			if (next.done) throw new Error("Actor Runtime Socket closed");
			buffered = Buffer.concat([buffered, Buffer.from(next.value)]);
		}

		const payloadLength = buffered.readUInt32BE(0);
		while (buffered.length < 4 + payloadLength) {
			const next = await chunks.next();
			if (next.done) throw new Error("Actor Runtime Socket closed");
			buffered = Buffer.concat([buffered, Buffer.from(next.value)]);
		}

		const payload = buffered.subarray(4, 4 + payloadLength);
		buffered = buffered.subarray(4 + payloadLength);
		return payload;
	};
}

async function connectActorRuntimeSocket(path: string): Promise<{
	socket: Socket;
	readFrame: () => Promise<Buffer>;
}> {
	const socket = createConnection(path);
	await once(socket, "connect");
	const readFrame = createActorRuntimeSocketFrameReader(socket);

	writeActorRuntimeSocketFrame(socket);
	const helloPayload = await readFrame();
	expect(helloPayload.subarray(0, 2)).toEqual(
		ACTOR_RUNTIME_SOCKET_PROTOCOL_VERSION,
	);
	expect(decodeServerHello(helloPayload.subarray(2)).tag).toBe("HelloOk");

	return { socket, readFrame };
}

async function sendActorRuntimeSocketRequest(
	socket: Socket,
	readFrame: () => Promise<Buffer>,
	request: Parameters<typeof encodeClientFrame>[0],
) {
	writeActorRuntimeSocketFrame(socket, encodeClientFrame(request));
	const payload = await readFrame();
	expect(payload.subarray(0, 2)).toEqual(
		ACTOR_RUNTIME_SOCKET_PROTOCOL_VERSION,
	);
	const response = decodeServerFrame(payload.subarray(2));
	expect(response.tag).toBe("Response");
	return response;
}

function isActorStoppingDbError(error: unknown): boolean {
	return (
		error instanceof Error &&
		error.message.includes(
			"Actor stopping: database accessed after actor stopped",
		)
	);
}

async function runWithActorStoppingRetry(
	_driverTestConfig: DriverTestConfig,
	fn: () => Promise<void>,
): Promise<void> {
	// Wait for the actor to leave the `stopping` window. The driver does not
	// surface a "ready again" signal, so we poll the user function and only
	// retry on the specific `actor stopping: database accessed` error. Any
	// other failure short-circuits.
	await vi.waitFor(
		async () => {
			try {
				await fn();
			} catch (error) {
				if (isActorStoppingDbError(error)) {
					throw error;
				}
				throw new (class extends Error {
					override name = "AbortRetry";
				})(error instanceof Error ? error.message : String(error));
			}
		},
		{ timeout: SLEEP_WAIT_MS * 4, interval: 100 },
	);
}

async function expectIntegrityCheckOk(
	_driverTestConfig: DriverTestConfig,
	integrityCheck: () => Promise<string>,
): Promise<void> {
	// Same lifecycle window as `runWithActorStoppingRetry`: the integrity
	// check is read-only against the SQLite db, so polling it does not hold
	// the actor awake.
	await vi.waitFor(
		async () => {
			try {
				expect((await integrityCheck()).toLowerCase()).toBe("ok");
			} catch (error) {
				if (isActorStoppingDbError(error)) {
					throw error;
				}
				throw error;
			}
		},
		{ timeout: SLEEP_WAIT_MS * 8, interval: 100 },
	);
}

function getDbActor(
	client: Awaited<ReturnType<typeof setupDriverTest>>["client"],
	_variant: DbVariant,
) {
	return client.dbActorRaw;
}

async function waitForDbLifecycleEvent(
	driverTestConfig: DriverTestConfig,
	observer: {
		getEvents(): Promise<LifecycleEvent[]>;
	},
	actorKey: string,
	event: string,
	notBefore: number,
): Promise<LifecycleEvent> {
	for (let i = 0; i < LIFECYCLE_POLL_ATTEMPTS; i++) {
		const events = await observer.getEvents();
		const match = events.find(
			(entry) =>
				entry.actorKey === actorKey &&
				entry.event === event &&
				entry.timestamp >= notBefore,
		);
		if (match) {
			return match;
		}

		await waitFor(driverTestConfig, LIFECYCLE_POLL_INTERVAL_MS);
	}

	throw new Error(`timed out waiting for ${event} event from ${actorKey}`);
}

describeDriverMatrix(
	"Actor Db",
	(driverTestConfig) => {
		const variants: DbVariant[] = ["raw"];
		const dbTestTimeout = driverTestConfig.useRealTimers
			? REAL_TIMER_DB_TIMEOUT_MS
			: undefined;
		const lifecycleTestTimeout = driverTestConfig.useRealTimers
			? REAL_TIMER_DB_TIMEOUT_MS
			: undefined;

		for (const variant of variants) {
			describe(`Actor Database (${variant}) Tests`, () => {
				test.skipIf(
					driverTestConfig.runtime !== "native" ||
						driverTestConfig.sqliteBackend !== "local",
				)(
					"requires opt-in and provisions the default embedded database",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const disabled =
							client.dbActorRuntimeSocketDisabled.getOrCreate([
								`runtime-socket-disabled-${crypto.randomUUID()}`,
							]);
						await expect(
							disabled.getActorRuntimeSocketPath(),
						).rejects.toMatchObject({
							group: "actor_runtime_socket",
							code: "not_enabled",
						});

						const withoutDb =
							client.actorRuntimeSocketWithoutDb.getOrCreate([
								`runtime-socket-without-db-${crypto.randomUUID()}`,
							]);
						const path =
							await withoutDb.getActorRuntimeSocketPath();
						expect(path).toBeTruthy();
						expect(existsSync(path)).toBe(true);
						await withoutDb.destroy();
						await vi.waitFor(() =>
							expect(existsSync(path)).toBe(false),
						);
					},
					dbTestTimeout,
				);

				test.skipIf(
					driverTestConfig.runtime !== "native" ||
						driverTestConfig.sqliteBackend !== "remote",
				)(
					"rejects Actor Runtime Socket provisioning for a remote SQLite backend",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`runtime-socket-remote-${crypto.randomUUID()}`,
						]);
						await expect(
							actor.getActorRuntimeSocketPath(),
						).rejects.toMatchObject({
							group: "actor_runtime_socket",
							code: "database_unavailable",
						});
					},
					dbTestTimeout,
				);

				test.skipIf(driverTestConfig.runtime !== "wasm")(
					"rejects Actor Runtime Socket provisioning outside the native runtime",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-runtime-socket-wasm-${crypto.randomUUID()}`,
						]);
						await expect(
							actor.getActorRuntimeSocketPath(),
						).rejects.toThrow(
							"only available on Unix native runtimes",
						);
					},
					dbTestTimeout,
				);

				test.skipIf(
					driverTestConfig.runtime !== "native" ||
						driverTestConfig.sqliteBackend !== "local",
				)(
					"provisions the Actor Runtime Socket for the current generation",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actorKey = [
							`db-${variant}-runtime-socket-${crypto.randomUUID()}`,
						];
						const actor = getDbActor(client, variant).getOrCreate(
							actorKey,
						);
						const firstPath =
							await actor.getActorRuntimeSocketPath();
						const { socket, readFrame } =
							await connectActorRuntimeSocket(firstPath);
						const actorAwake = actor.holdActorAwake();
						// Keep this long raw-socket scenario from crossing the actor's normal
						// idle-sleep boundary while no action response is outstanding.
						await vi.waitFor(async () => {
							expect(await actor.isActorAwakeHeld()).toBe(true);
						});

						const begin = await sendActorRuntimeSocketRequest(
							socket,
							readFrame,
							{
								tag: "Request",
								val: {
									requestId: 1,
									leaseKey: null,
									payload: {
										tag: "SqliteBegin",
										val: {
											leaseKey: "driver-transaction",
											timeoutMs: null,
										},
									},
								},
							},
						);
						expect(begin).toMatchObject({
							tag: "Response",
							val: {
								requestId: 1,
								payload: { tag: "SqliteBeginOk" },
							},
						});

						const insert = await sendActorRuntimeSocketRequest(
							socket,
							readFrame,
							{
								tag: "Request",
								val: {
									requestId: 2,
									leaseKey: "driver-transaction",
									payload: {
										tag: "SqliteQuery",
										val: {
											sql: "INSERT INTO test_data (value, payload, created_at) VALUES (?, ?, ?)",
											params: [
												{
													tag: "SqlText",
													val: "transaction",
												},
												{ tag: "SqlText", val: "" },
												{
													tag: "SqlInteger",
													val: BigInt(Date.now()),
												},
											],
										},
									},
								},
							},
						);
						expect(insert).toMatchObject({
							tag: "Response",
							val: {
								requestId: 2,
								payload: { tag: "SqliteQueryOk" },
							},
						});

						const actorCount =
							actor.getCountWithSubmissionBarrier();
						// Poll the fixture barrier set immediately after c.db.execute returns its
						// pending promise, proving the operation reached coordinator admission.
						await vi.waitFor(async () => {
							expect(await actor.isSqlOperationSubmitted()).toBe(
								true,
							);
						});

						const commit = await sendActorRuntimeSocketRequest(
							socket,
							readFrame,
							{
								tag: "Request",
								val: {
									requestId: 3,
									leaseKey: null,
									payload: {
										tag: "SqliteCommit",
										val: { leaseKey: "driver-transaction" },
									},
								},
							},
						);
						expect(commit).toMatchObject({
							tag: "Response",
							val: {
								requestId: 3,
								payload: { tag: "SqliteCommitOk" },
							},
						});
						expect(await actorCount).toBe(1);

						const independentInsert =
							await sendActorRuntimeSocketRequest(
								socket,
								readFrame,
								{
									tag: "Request",
									val: {
										requestId: 4,
										leaseKey: null,
										payload: {
											tag: "SqliteQuery",
											val: {
												sql: "INSERT INTO test_data (value, payload, created_at) VALUES (?, ?, ?)",
												params: [
													{
														tag: "SqlText",
														val: "independent",
													},
													{ tag: "SqlText", val: "" },
													{
														tag: "SqlInteger",
														val: BigInt(Date.now()),
													},
												],
											},
										},
									},
								},
							);
						expect(independentInsert).toMatchObject({
							tag: "Response",
							val: {
								requestId: 4,
								payload: { tag: "SqliteQueryOk" },
							},
						});
						expect(await actor.getCount()).toBe(2);

						const heldTransaction = actor.holdTransaction();
						// Poll until the actor transaction owns the shared coordinator before sending socket SQL.
						await vi.waitFor(async () => {
							expect(await actor.isTransactionHolding()).toBe(
								true,
							);
						});
						const socketSql = sendActorRuntimeSocketRequest(
							socket,
							readFrame,
							{
								tag: "Request",
								val: {
									requestId: 5,
									leaseKey: null,
									payload: {
										tag: "SqliteQuery",
										val: {
											sql: "SELECT count(*) FROM test_data",
											params: [],
										},
									},
								},
							},
						);
						await actor.releaseHeldTransaction();
						await heldTransaction;
						expect(await socketSql).toMatchObject({
							tag: "Response",
							val: {
								requestId: 5,
								payload: { tag: "SqliteQueryOk" },
							},
						});

						const expiringBegin =
							await sendActorRuntimeSocketRequest(
								socket,
								readFrame,
								{
									tag: "Request",
									val: {
										requestId: 6,
										leaseKey: null,
										payload: {
											tag: "SqliteBegin",
											val: {
												leaseKey: "expiring",
												timeoutMs: BigInt(50),
											},
										},
									},
								},
							);
						expect(expiringBegin).toMatchObject({
							tag: "Response",
							val: {
								requestId: 6,
								payload: { tag: "SqliteBeginOk" },
							},
						});
						await new Promise((resolve) => setTimeout(resolve, 80));
						for (const requestId of [7, 8]) {
							const expired = await sendActorRuntimeSocketRequest(
								socket,
								readFrame,
								{
									tag: "Request",
									val: {
										requestId,
										leaseKey: "expiring",
										payload: {
											tag: "SqliteQuery",
											val: {
												sql: "SELECT 1",
												params: [],
											},
										},
									},
								},
							);
							expect(expired).toMatchObject({
								tag: "Response",
								val: {
									requestId,
									payload: {
										tag: "LeaseExpired",
										val: { timeoutMs: 50n },
									},
								},
							});
							if (expired.tag === "Response") {
								expect(
									expired.val.payload.val.message,
								).toContain("deadlock-safety backstop");
							}
						}

						await sendActorRuntimeSocketRequest(socket, readFrame, {
							tag: "Request",
							val: {
								requestId: 9,
								leaseKey: null,
								payload: {
									tag: "SqliteBegin",
									val: {
										leaseKey: "disconnect",
										timeoutMs: null,
									},
								},
							},
						});
						await sendActorRuntimeSocketRequest(socket, readFrame, {
							tag: "Request",
							val: {
								requestId: 10,
								leaseKey: "disconnect",
								payload: {
									tag: "SqliteQuery",
									val: {
										sql: "INSERT INTO test_data (value, payload, created_at) VALUES ('disconnect-rollback', '', ?)",
										params: [
											{
												tag: "SqlInteger",
												val: BigInt(Date.now()),
											},
										],
									},
								},
							},
						});

						socket.destroy();
						await once(socket, "close");
						await actor.releaseActorAwake();
						await actorAwake;
						expect(await actor.getCount()).toBe(2);
						await actor.destroy();
						// Poll until destroy cleanup removes the old generation socket.
						await vi.waitFor(() =>
							expect(existsSync(firstPath)).toBe(false),
						);

						const nextActor = getDbActor(
							client,
							variant,
						).getOrCreate(actorKey);
						const nextPath =
							await nextActor.getActorRuntimeSocketPath();
						expect(nextPath).not.toBe(firstPath);
						expect(existsSync(nextPath)).toBe(true);

						await nextActor.triggerSleep();
						// Poll until sleep cleanup removes the old generation socket.
						await vi.waitFor(() =>
							expect(existsSync(nextPath)).toBe(false),
						);
						const afterSleepPath =
							await nextActor.getActorRuntimeSocketPath();
						expect(afterSleepPath).not.toBe(nextPath);
						expect(existsSync(afterSleepPath)).toBe(true);
					},
					dbTestTimeout,
				);

				test(
					"bootstraps schema on startup",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-bootstrap-${crypto.randomUUID()}`,
						]);

						const count = await actor.getCount();
						expect(count).toBe(0);
					},
					dbTestTimeout,
				);

				test(
					"supports CRUD, raw SQL, and multi-statement exec",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-crud-${crypto.randomUUID()}`,
						]);

						await actor.ready;
						await actor.reset();

						const first = await actor.insertValue("alpha");
						const second = await actor.insertValue("beta");

						const values = await actor.getValues();
						expect(values.length).toBeGreaterThanOrEqual(2);
						expect(
							values.some(
								(row: { value: string }) =>
									row.value === "alpha",
							),
						).toBeTruthy();
						expect(
							values.some(
								(row: { value: string }) =>
									row.value === "beta",
							),
						).toBeTruthy();

						await actor.updateValue(first.id, "alpha-updated");
						const updated = await actor.getValue(first.id);
						expect(updated).toBe("alpha-updated");

						await actor.deleteValue(second.id);
						const count = await actor.getCount();
						if (driverTestConfig.useRealTimers) {
							expect(count).toBeGreaterThanOrEqual(1);
						} else {
							expect(count).toBe(1);
						}

						const rawCount = await actor.rawSelectCount();
						if (driverTestConfig.useRealTimers) {
							expect(rawCount).toBeGreaterThanOrEqual(1);
						} else {
							expect(rawCount).toBe(1);
						}

						const multiValue =
							await actor.multiStatementInsert("gamma");
						expect(multiValue).toBe("gamma-updated");
					},
					dbTestTimeout,
				);

				test(
					"handles transactions",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-tx-${crypto.randomUUID()}`,
						]);

						await actor.reset();
						await actor.transactionCommit("commit");
						expect(await actor.getCount()).toBe(1);

						await actor.transactionRollback("rollback");
						expect(await actor.getCount()).toBe(1);
						await actor.transactionWithTimeout(120_000);
						expect(await actor.transactionExec()).toEqual([
							"commit",
							"exec-first",
							"exec-second",
						]);
						expect(
							await actor.transactionAutomaticRollback(),
						).toEqual([{ id: 2, value: "after" }]);
					},
					dbTestTimeout,
				);

				test(
					"commits and rolls back state-aware transactions through the runtime bridge",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actorKey = `db-${variant}-state-tx-${crypto.randomUUID()}`;
						const observerKey = `${actorKey}-observer`;
						const observer = client.lifecycleObserver.getOrCreate([
							observerKey,
						]);
						await observer.clearEvents();
						const actor = getDbActor(client, variant).getOrCreate([
							actorKey,
						]);

						await actor.reset();
						await actor.configureLifecycleObserver(observerKey);
						expect(
							await actor.stateTransactionCommit("committed"),
						).toEqual({
							state: "committed",
							count: [{ count: 1 }],
						});
						expect(
							await actor.stateTransactionRollback("rolled-back"),
						).toEqual({
							state: "committed",
							count: [{ count: 1 }],
						});

						// The in-process Wasm driver hosts a single envoy process, so
						// explicitly sleeping its actor also tears down the test host.
						// Native exercises the persisted reload; both runtimes exercise
						// the commit and rollback bridges above.
						if (
							driverTestConfig.runtime === "native" &&
							!driverTestConfig.skip?.sleep
						) {
							const sleepRequestedAt = Date.now();
							await actor.triggerSleep();
							await waitForDbLifecycleEvent(
								driverTestConfig,
								observer,
								actorKey,
								"sleep",
								sleepRequestedAt,
							);
							expect(
								await actor.stateTransactionRollback(
									"rolled-back-after-wake",
								),
							).toEqual({
								state: "committed",
								count: [{ count: 1 }],
							});
						}
					},
					dbTestTimeout,
				);

				test(
					"isolates state-aware transactions from concurrent state mutations",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-state-tx-isolation-${crypto.randomUUID()}`,
						]);
						await actor.reset();

						const rollback =
							actor.stateTransactionHoldAndRollback("held");
						await actor.waitForStateTransaction();
						await expect(
							actor.mutateStateDuringTransaction("concurrent"),
						).rejects.toMatchObject({
							group: "actor",
							code: "state_transaction_conflict",
						});
						expect(await rollback).toBeNull();
					},
					dbTestTimeout,
				);

				test(
					"queues state transactions from separate actions",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-state-tx-queued-${crypto.randomUUID()}`,
						]);
						await actor.reset();

						const rollback =
							actor.stateTransactionHoldAndRollback("first");
						await actor.waitForStateTransaction();
						const commit = actor.stateTransactionSetValue("second");
						await waitFor(driverTestConfig, 50);
						await actor.releaseStateTransaction();

						expect(await rollback).toBeNull();
						expect(await commit).toBe("second");
					},
					dbTestTimeout,
				);

				test(
					"rejects multi-statement state-aware transactions locally",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-state-tx-multi-${crypto.randomUUID()}`,
						]);

						expect(
							await actor.stateTransactionRejectsMultiStatement(),
						).toContain("single-statement execute calls");
					},
					dbTestTimeout,
				);

				test(
					"preserves unsaved state from before a state transaction rollback",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-state-tx-predirty-${crypto.randomUUID()}`,
						]);
						await actor.reset();

						expect(
							await actor.stateTransactionPreservesPredirtyState(),
						).toBe("before");
					},
					dbTestTimeout,
				);

				test(
					"rejects state transaction reentry through the outer database client",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-state-tx-reentry-${crypto.randomUUID()}`,
						]);

						await expect(
							actor.stateTransactionRejectsOuterClientReentry(),
						).rejects.toMatchObject({
							group: "actor",
							code: "state_transaction_conflict",
						});
					},
					dbTestTimeout,
				);

				test(
					"rejects retained actor state proxies after rollback",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-state-tx-stale-actor-${crypto.randomUUID()}`,
						]);
						await actor.reset();

						expect(
							await actor.stateTransactionRejectsRetainedActorStateAfterRollback(),
						).toEqual({
							staleWriteErrorCode: "state_transaction_conflict",
							state: null,
						});
					},
					dbTestTimeout,
				);

				test(
					"rejects retained connection state proxies after rollback",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-state-tx-stale-conn-${crypto.randomUUID()}`,
						]);
						const connection = actor.connect();
						try {
							await vi.waitFor(() => {
								expect(connection.isConnected).toBe(true);
							});
							expect(
								await actor.stateTransactionRejectsRetainedConnStateAfterRollback(),
							).toEqual({
								staleWriteErrorCode:
									"state_transaction_conflict",
								state: null,
							});
						} finally {
							await connection.dispose();
						}
					},
					dbTestTimeout,
				);

				test(
					"releases transaction admission when baseline capture fails",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-state-tx-clone-failure-${crypto.randomUUID()}`,
						]);

						await expect(
							actor.stateTransactionRecoversFromBaselineCloneFailure(),
						).resolves.toBe("after");
					},
					dbTestTimeout,
				);

				test(
					"does not expose rolled-back state through onStateChange",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-state-tx-on-change-${crypto.randomUUID()}`,
						]);

						expect(
							await actor.stateTransactionHidesRolledBackStateFromOnStateChange(),
						).toEqual({ state: "before", observed: ["before"] });
					},
					dbTestTimeout,
				);

				test(
					"rejects immediate saves inside state-aware transactions without deadlocking",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-state-tx-immediate-save-${crypto.randomUUID()}`,
						]);

						expect(
							await actor.stateTransactionRejectsImmediateSave(),
						).toMatchObject({
							errorCode: "state_transaction_conflict",
							state: "after",
						});
					},
					dbTestTimeout,
				);

				test(
					"queues concurrent transactions without interleaving",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-tx-fifo-${crypto.randomUUID()}`,
						]);
						await actor.reset();
						const values = await actor.concurrentTransactions();
						expect(values.slice(0, 2)).toEqual([
							"first-start",
							"first-end",
						]);
						// FIFO begins at coordinator admission. Calls crossing an async
						// NAPI/Wasm boundary may be admitted in either source-call order;
						// both must remain wholly behind the active transaction.
						expect(values.slice(2).sort()).toEqual([
							"second",
							"third",
						]);
					},
					dbTestTimeout,
				);

				test(
					"routes Drizzle query builders through transaction handles",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor =
							client.dbActorDrizzleMigration.getOrCreate([
								`db-drizzle-query-builder-${crypto.randomUUID()}`,
							]);
						expect(await actor.queryBuilderTransaction()).toEqual({
							inside: ["query-builder"],
							after: [],
						});
					},
					dbTestTimeout,
				);

				test(
					"parks ordinary actor SQL behind a transaction",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-tx-ordinary-${crypto.randomUUID()}`,
						]);
						await actor.reset();
						expect(
							await actor.transactionParksOrdinarySql(),
						).toEqual(["tx-start", "tx-end", "ordinary"]);
					},
					dbTestTimeout,
				);

				test(
					"times out nested and outer-db transaction deadlocks with diagnostics",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-tx-deadlock-${crypto.randomUUID()}`,
						]);
						for (const nested of [false, true]) {
							const message =
								await actor.transactionDeadlockDiagnostic(
									nested,
								);
							expect(message).toContain("safety backstop");
							expect(message).toContain("timeout");
							expect(message).toContain("nested transaction");
							expect(message).toContain("outer `db`");
						}
					},
					dbTestTimeout,
				);

				test(
					"expires transaction handles and rejects parked SQL without execution",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-tx-expiry-${crypto.randomUUID()}`,
						]);
						await actor.reset();
						const result =
							await actor.transactionExpiryDiagnostics();
						expect(result.ownerMessage).toContain(
							"expired after 50 ms",
						);
						expect(result.leakedMessage).toContain(
							"expired after 50 ms",
						);
						expect(result.parkedResult.status).toBe("rejected");
						if (result.parkedResult.status === "rejected") {
							expect(result.parkedResult.message).toContain(
								"expired after 50 ms",
							);
						}
						expect(result.values).toEqual([]);
					},
					dbTestTimeout,
				);

				test(
					"rejects completed transaction handles",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-tx-terminal-${crypto.randomUUID()}`,
						]);
						expect(
							await actor.terminalTransactionDiagnostic(),
						).toContain("already committed");
					},
					dbTestTimeout,
				);

				test(
					"preserves manual cross-call transaction compatibility",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-tx-manual-${crypto.randomUUID()}`,
						]);
						expect(
							await actor.manualTransactionCompatibility(),
						).toBe(1);
					},
					dbTestTimeout,
				);

				test(
					"warns once per database for manual cross-call transactions",
					async (c) => {
						const { client, getRuntimeOutput } =
							await setupDriverTest(c, driverTestConfig);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-tx-manual-warning-${crypto.randomUUID()}`,
						]);
						const warning =
							"Manual cross-call SQLite transactions can interleave";
						const warningCount = () =>
							getRuntimeOutput().split(warning).length - 1;
						const baseline = warningCount();
						await actor.manualTransactionCompatibility();
						await actor.manualTransactionCompatibility();
						// Wait until the child runtime flushes the once-per-database warning.
						await vi.waitFor(() =>
							expect(warningCount() - baseline).toBe(1),
						);
						expect(getRuntimeOutput()).toContain(
							"Set warnOnManualTransactions: false",
						);
					},
					dbTestTimeout,
				);

				test(
					"suppresses the manual transaction warning when configured",
					async (c) => {
						const { client, getRuntimeOutput } =
							await setupDriverTest(c, driverTestConfig);
						const warning =
							"Manual cross-call SQLite transactions can interleave";
						const baseline =
							getRuntimeOutput().split(warning).length - 1;
						const marker = `manual-warning-disabled-${crypto.randomUUID()}`;
						const actor =
							client.dbActorManualWarningsDisabled.getOrCreate([
								marker,
							]);
						expect(
							await actor.manualTransactionCompatibility(marker),
						).toBe(true);
						// The marker proves stdout has flushed past the point where a warning would run.
						await vi.waitFor(() =>
							expect(getRuntimeOutput()).toContain(marker),
						);
						expect(
							getRuntimeOutput().split(warning).length - 1,
						).toBe(baseline);
					},
					dbTestTimeout,
				);

				test(
					"commits Drizzle migrations through the coordinator",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor =
							client.dbActorDrizzleMigration.getOrCreate([
								`db-drizzle-migration-${crypto.randomUUID()}`,
							]);
						expect(await actor.migrationCommitted()).toBe(true);
					},
					dbTestTimeout,
				);

				test(
					"rolls back a failed Drizzle migration before retry",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const key = [
							`db-drizzle-rollback-${crypto.randomUUID()}`,
						];
						let rolledBack: boolean;
						try {
							rolledBack =
								await client.dbActorDrizzleMigrationRollback
									.getOrCreate(key)
									.migrationRolledBack();
						} catch (error) {
							expect(String(error)).toContain(
								"intentional drizzle migration failure",
							);
							rolledBack =
								await client.dbActorDrizzleMigrationRollback
									.getOrCreate(key)
									.migrationRolledBack();
						}
						expect(rolledBack).toBe(true);
					},
					dbTestTimeout,
				);

				test(
					"persists across sleep and wake cycles",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actorKey = `db-${variant}-sleep-${crypto.randomUUID()}`;
						const observerKey = `${actorKey}-observer`;
						const observer = client.lifecycleObserver.getOrCreate([
							observerKey,
						]);
						await observer.clearEvents();

						const actor = getDbActor(client, variant).getOrCreate([
							actorKey,
						]);

						await actor.reset();
						await actor.configureLifecycleObserver(observerKey);
						await actor.insertValue("sleepy");
						const baselineCount = await actor.getCount();
						expect(baselineCount).toBeGreaterThan(0);

						for (let i = 0; i < 3; i++) {
							const sleepRequestedAt = Date.now();
							await actor.triggerSleep();
							const sleepEvent = await waitForDbLifecycleEvent(
								driverTestConfig,
								observer,
								actorKey,
								"sleep",
								sleepRequestedAt,
							);
							expect(
								sleepEvent.timestamp - sleepRequestedAt,
							).toBeLessThanOrEqual(SLEEP_OBSERVER_TIMEOUT_MS);

							const countAfterWake = await actor.getCount();
							expect(countAfterWake).toBe(baselineCount);
						}
					},
					dbTestTimeout,
				);

				test.skipIf(driverTestConfig.skip?.sleep)(
					"preserves committed rows across a hard crash and restart",
					async (c) => {
						const {
							client,
							hardCrashActor,
							hardCrashPreservesData,
						} = await setupDriverTest(c, driverTestConfig);
						if (!hardCrashPreservesData) {
							return;
						}
						if (!hardCrashActor) {
							throw new Error(
								"hardCrashActor test helper is unavailable for this driver",
							);
						}

						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-hard-crash-${crypto.randomUUID()}`,
						]);

						await actor.reset();
						await actor.configureLifecycleObserver(null);
						await actor.insertValue("before-crash");
						expect(await actor.getCount()).toBe(1);

						const actorId = await actor.resolve();
						await hardCrashActor(actorId);

						const countAfterCrash = await actor.getCount();
						expect(countAfterCrash).toBe(1);
						const values = await actor.getValues();
						expect(
							values.some((row) => row.value === "before-crash"),
						).toBe(true);

						await actor.insertValue("after-crash");
						expect(await actor.getCount()).toBe(2);
					},
					lifecycleTestTimeout,
				);

				test(
					"completes onDisconnect DB writes before sleeping",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const key = `db-${variant}-disconnect-${crypto.randomUUID()}`;

						const actor = getDbActor(client, variant).getOrCreate([
							key,
						]);
						await actor.reset();
						await actor.configureDisconnectInsert(true, 250);

						await waitFor(driverTestConfig, SLEEP_WAIT_MS + 250);
						await actor.configureDisconnectInsert(false, 0);

						// Poll for the disconnect insert to complete.
						// Native SQLite routes writes through a WebSocket KV
						// channel, which adds latency that can push the
						// onDisconnect DB write past the fixed wait window
						// under concurrent test load.
						let count = 0;
						for (let i = 0; i < LIFECYCLE_POLL_ATTEMPTS; i++) {
							count = await actor.getDisconnectInsertCount();
							if (count >= 1) {
								break;
							}
							await waitFor(
								driverTestConfig,
								LIFECYCLE_POLL_INTERVAL_MS,
							);
						}

						expect(count).toBe(1);
					},
					dbTestTimeout,
				);

				test(
					"handles high-volume inserts",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-high-volume-${crypto.randomUUID()}`,
						]);

						await actor.reset();
						await actor.insertMany(HIGH_VOLUME_COUNT);
						const count = await actor.getCount();
						if (driverTestConfig.useRealTimers) {
							expect(count).toBeGreaterThanOrEqual(
								HIGH_VOLUME_COUNT,
							);
						} else {
							expect(count).toBe(HIGH_VOLUME_COUNT);
						}
					},
					dbTestTimeout,
				);

				test(
					"handles payloads across chunk boundaries",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-chunk-${crypto.randomUUID()}`,
						]);

						await actor.reset();
						for (const size of CHUNK_BOUNDARY_SIZES) {
							const { id } =
								await actor.insertPayloadOfSize(size);
							const storedSize = await actor.getPayloadSize(id);
							expect(storedSize).toBe(size);
						}
					},
					dbTestTimeout,
				);

				test(
					"handles large payloads",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-large-${crypto.randomUUID()}`,
						]);

						await actor.reset();
						const { id } =
							await actor.insertPayloadOfSize(LARGE_PAYLOAD_SIZE);
						const storedSize = await actor.getPayloadSize(id);
						expect(storedSize).toBe(LARGE_PAYLOAD_SIZE);
					},
					dbTestTimeout,
				);

				test(
					"supports shrink and regrow workloads with vacuum",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-shrink-regrow-${crypto.randomUUID()}`,
						]);

						await actor.reset();
						await actor.vacuum();
						const baselinePages = await actor.getPageCount();

						await actor.insertPayloadRows(
							SHRINK_GROW_INITIAL_ROWS,
							SHRINK_GROW_INITIAL_PAYLOAD,
						);
						const grownPages = await actor.getPageCount();

						await actor.reset();
						await actor.vacuum();
						const shrunkPages = await actor.getPageCount();

						await actor.insertPayloadRows(
							SHRINK_GROW_REGROW_ROWS,
							SHRINK_GROW_REGROW_PAYLOAD,
						);
						const regrownPages = await actor.getPageCount();

						expect(grownPages).toBeGreaterThanOrEqual(
							baselinePages,
						);
						expect(shrunkPages).toBeLessThanOrEqual(grownPages);
						expect(regrownPages).toBeGreaterThan(shrunkPages);
					},
					dbTestTimeout,
				);

				test(
					"handles repeated updates to the same row",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-updates-${crypto.randomUUID()}`,
						]);

						await actor.reset();
						const { id } = await actor.insertValue("base");
						const result = await actor.repeatUpdate(id, 50);
						expect(result.value).toBe("Updated 49");
						const value = await actor.getValue(id);
						expect(value).toBe("Updated 49");

						const hotRowIds: number[] = [];
						for (let i = 0; i < HOT_ROW_COUNT; i++) {
							const row = await actor.insertValue(`init-${i}`);
							hotRowIds.push(row.id);
						}

						const updatedRows = await actor.roundRobinUpdateValues(
							hotRowIds,
							HOT_ROW_UPDATES,
						);
						expect(updatedRows).toHaveLength(HOT_ROW_COUNT);
						for (const row of updatedRows) {
							expect(row.value).toMatch(/^v-\d+$/);
						}
					},
					dbTestTimeout,
				);

				test(
					"passes integrity checks after mixed workload and sleep",
					async (c) => {
						const { client } = await setupDriverTest(
							c,
							driverTestConfig,
						);
						const actor = getDbActor(client, variant).getOrCreate([
							`db-${variant}-integrity-${crypto.randomUUID()}`,
						]);

						await actor.reset();
						await runWithActorStoppingRetry(
							driverTestConfig,
							async () =>
								await actor.runMixedWorkload(
									INTEGRITY_SEED_COUNT,
									INTEGRITY_CHURN_COUNT,
								),
						);
						await expectIntegrityCheckOk(
							driverTestConfig,
							async () => await actor.integrityCheck(),
						);

						await actor.triggerSleep();
						await expectIntegrityCheckOk(
							driverTestConfig,
							async () => await actor.integrityCheck(),
						);
					},
					dbTestTimeout,
				);
			});
		}

		describe("Actor Database Lifecycle Tests", () => {
			test(
				"handles parallel actor lifecycle churn",
				async (c) => {
					const { client } = await setupDriverTest(
						c,
						driverTestConfig,
					);

					const actorHandles = Array.from({ length: 12 }, (_, i) =>
						client.dbLifecycle.getOrCreate([
							`db-lifecycle-stress-${i}-${crypto.randomUUID()}`,
						]),
					);

					await Promise.all(
						actorHandles.map((handle, i) =>
							handle.insertValue(`phase-1-${i}`),
						),
					);
					await Promise.all(
						actorHandles.map((handle) => handle.triggerSleep()),
					);
					await waitFor(driverTestConfig, SLEEP_WAIT_MS + 100);
					await Promise.all(
						actorHandles.map((handle, i) =>
							handle.insertValue(`phase-2-${i}`),
						),
					);

					const survivors = actorHandles.slice(0, 6);
					const destroyed = actorHandles.slice(6);

					await Promise.all(
						destroyed.map((handle) => handle.triggerDestroy()),
					);
					await Promise.all(
						survivors.map((handle) => handle.triggerSleep()),
					);
					await waitFor(driverTestConfig, SLEEP_WAIT_MS + 100);
					await Promise.all(survivors.map((handle) => handle.ping()));

					const survivorCounts = await Promise.all(
						survivors.map((handle) => handle.getCount()),
					);
					for (const count of survivorCounts) {
						if (driverTestConfig.useRealTimers) {
							expect(count).toBeGreaterThanOrEqual(2);
						} else {
							expect(count).toBe(2);
						}
					}
				},
				lifecycleTestTimeout,
			);
		});
	},
	SQLITE_DRIVER_MATRIX_OPTIONS,
);
