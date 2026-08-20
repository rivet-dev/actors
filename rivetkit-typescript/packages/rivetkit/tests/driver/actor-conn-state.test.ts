// @ts-nocheck

import { describe, expect, test, vi } from "vitest";
import { describeDriverMatrix } from "./shared-matrix";
import { setupDriverTest } from "./shared-utils";

describeDriverMatrix("Actor Conn State", (driverTestConfig) => {
	describe("Actor Connection State Tests", () => {
		describe("Connection State Initialization", () => {
			test("should retrieve connection state", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				// Connect to the actor
				const connection = client.connStateActor
					.getOrCreate()
					.connect();

				// Get the connection state
				const connState = await connection.getConnectionState();

				// Verify the connection state structure
				expect(connState.id).toBeDefined();
				expect(connState.username).toBeDefined();
				expect(connState.role).toBeDefined();
				expect(connState.counter).toBeDefined();
				expect(connState.createdAt).toBeDefined();

				// Clean up
				await connection.dispose();
			});

			test("should initialize connection state with custom parameters", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				// Connect with custom parameters
				const connection = client.connStateActor
					.getOrCreate([], {
						params: {
							username: "testuser",
							role: "admin",
						},
					})
					.connect();

				// Get the connection state
				const connState = await connection.getConnectionState();

				// Verify the connection state was initialized with custom values
				expect(connState.username).toBe("testuser");
				expect(connState.role).toBe("admin");

				// Clean up
				await connection.dispose();
			});
		});

		describe("State Proxy Nesting", () => {
			test("does not stack proxies on nested connection state", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				const connection = client.connStateActor
					.getOrCreate()
					.connect();

				const result = await connection.spreadUpdateConnState(12);

				// Values read off the state proxy are wrapped exactly once.
				// Extra layers mean each update persisted the previous read's
				// proxies, which makes state traversal cost grow exponentially.
				expect(result.depth).toBe(1);
				expect(result.tags).toEqual(["read", "write"]);

				await connection.dispose();
			});

			test("does not stack proxies on nested actor state", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				const connection = client.connStateActor
					.getOrCreate()
					.connect();

				const result = await connection.spreadUpdateActorState(12);

				expect(result.depth).toBe(1);
				expect(result.tags).toEqual(["read", "write"]);

				await connection.dispose();
			});
		});

		describe("Connection State Management", () => {
			test("should maintain unique state for each connection", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				// Create multiple connections
				const conn1 = client.connStateActor
					.getOrCreate([], {
						params: { username: "user1" },
					})
					.connect();
				await conn1.getConnectionState();

				const conn2 = client.connStateActor
					.getOrCreate([], {
						params: { username: "user2" },
					})
					.connect();
				await conn2.getConnectionState();

				// Update connection state for each connection
				await conn1.incrementConnCounter(5);
				await conn2.incrementConnCounter(10);

				// Get state for each connection
				const state1 = await conn1.getConnectionState();
				const state2 = await conn2.getConnectionState();

				// Verify states are separate
				expect(state1.counter).toBe(5);
				expect(state2.counter).toBe(10);
				expect(state1.username).toBe("user1");
				expect(state2.username).toBe("user2");

				// Clean up
				await conn1.dispose();
				await conn2.dispose();
			});

			test("should track connections in shared state", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				// Create two connections
				const handle = client.connStateActor.getOrCreate();
				const conn1 = handle.connect();
				await conn1.getConnectionState();

				const conn2 = handle.connect();
				await conn2.getConnectionState();

				// Get state1 for reference
				const state1 = await conn1.getConnectionState();

				// Get connection IDs tracked by the actor
				const connectionIds = await conn1.getConnectionIds();

				// There should be at least 2 connections tracked
				expect(connectionIds.length).toBeGreaterThanOrEqual(2);

				// Should include the ID of the first connection
				expect(connectionIds).toContain(state1.id);

				// Clean up
				await conn1.dispose();
				await conn2.dispose();
			});

			test("should identify different connections in the same actor", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				// Create two connections to the same actor
				const handle = client.connStateActor.getOrCreate();
				const conn1 = handle.connect();
				await conn1.getConnectionState();

				const conn2 = handle.connect();
				await conn2.getConnectionState();

				// Get all connection states
				const allStates = await conn1.getAllConnectionStates();

				// Should have at least 2 states
				expect(allStates.length).toBeGreaterThanOrEqual(2);

				// IDs should be unique
				const ids = allStates.map((state: { id: string }) => state.id);
				const uniqueIds = [...new Set(ids)];
				expect(uniqueIds.length).toBe(ids.length);

				// Clean up
				await conn1.dispose();
				await conn2.dispose();
			});
		});

		describe("Connection Lifecycle", () => {
			test("should hide connections from c.conns until createConnState completes", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const handle = client.connPreflightVisibilityActor.getOrCreate([
					"create-state-visibility",
					crypto.randomUUID(),
				]);
				const primary = handle.connect({ label: "primary" });
				await primary.snapshot();

				const pending = handle.connect({
					label: "pending",
					createDelayMs: 300,
				});

				const pendingSnapshot = await pending.snapshot();
				expect([...pendingSnapshot.visibleLabels].sort()).toEqual([
					"pending",
					"primary",
				]);
				expect(pendingSnapshot.createVisibleLabels).toEqual([
					[],
					["primary"],
				]);
				expect(
					pendingSnapshot.connectSnapshots.find(
						(snapshot) => snapshot.label === "pending",
					),
				).toMatchObject({
					ownVisible: true,
				});
				expect(
					[
						...pendingSnapshot.connectSnapshots.find(
							(snapshot) => snapshot.label === "pending",
						).visibleLabels,
					].sort(),
				).toEqual(["pending", "primary"]);

				await pending.dispose();
				await primary.dispose();
			});

			test("should hide connections from c.conns until onBeforeConnect completes", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const handle = client.connPreflightVisibilityActor.getOrCreate([
					"before-connect-visibility",
					crypto.randomUUID(),
				]);
				const primary = handle.connect({ label: "primary" });
				await primary.snapshot();

				const pending = handle.connect({
					label: "pending",
					beforeDelayMs: 300,
				});

				const pendingSnapshot = await pending.snapshot();
				expect([...pendingSnapshot.visibleLabels].sort()).toEqual([
					"pending",
					"primary",
				]);
				expect(pendingSnapshot.beforeVisibleLabels).toEqual([
					[],
					["primary"],
				]);
				expect(pendingSnapshot.createVisibleLabels).toEqual([
					[],
					["primary"],
				]);
				expect(
					pendingSnapshot.connectSnapshots.find(
						(snapshot) => snapshot.label === "pending",
					),
				).toMatchObject({
					ownVisible: true,
				});
				expect(
					[
						...pendingSnapshot.connectSnapshots.find(
							(snapshot) => snapshot.label === "pending",
						).visibleLabels,
					].sort(),
				).toEqual(["pending", "primary"]);

				await pending.dispose();
				await primary.dispose();
			});

			test("should deliver onConnect events to listeners registered before the first await", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				const conn = client.connStateActor
					.getOrCreate([], {
						params: { username: "connect-event-user" },
					})
					.connect();
				let connectEvent:
					| {
							id: string;
							username: string;
					  }
					| undefined;
				let connectConnsEvent:
					| {
							id: string;
							username: string;
					  }
					| undefined;
				// Register these before any await. The client does not replay events that arrive before subscription.
				const unsubscribe = conn.on(
					"connectedFromOnConnect",
					(event) => {
						connectEvent = event;
						unsubscribe();
					},
				);
				const unsubscribeConns = conn.on(
					"connectedFromOnConnectConns",
					(event) => {
						connectConnsEvent = event;
						unsubscribeConns();
					},
				);

				const connState = await conn.getConnectionState();

				// Poll until the onConnect event arrives because connection event delivery crosses the websocket task boundary.
				await vi.waitFor(
					() => {
						expect(connectEvent).toEqual({
							id: connState.id,
							username: "connect-event-user",
						});
						expect(connectConnsEvent).toEqual({
							id: connState.id,
							username: "connect-event-user",
						});
					},
					{ timeout: 10_000, interval: 100 },
				);

				await conn.dispose();
			});

			test("should track connection and disconnection events", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				const debugHandle = client.connStateActor.getOrCreate(
					undefined,
					{
						params: { noCount: true },
					},
				);

				// Create a connection
				const conn = client.connStateActor.getOrCreate().connect();

				// Get the connection state
				const connState = await conn.getConnectionState();

				// Poll until the async connection registration exposes the new connection ID.
				await vi.waitFor(async () => {
					const connectionIds = await debugHandle.getConnectionIds();
					expect(connectionIds).toContain(connState.id);
				});

				// Poll until the actor reports its initial disconnect count after connect bookkeeping settles.
				await vi.waitFor(async () => {
					const disconnects =
						await debugHandle.getDisconnectionCount();
					expect(disconnects).toBe(0);
				});

				// Dispose the connection
				await conn.dispose();

				// Poll until async disconnect bookkeeping lands, which is especially slow over SSE on Workers.
				await vi.waitFor(
					async () => {
						const disconnects =
							await debugHandle.getDisconnectionCount();
						expect(disconnects).toBe(1);
					},
					// SSE takes a long time to disconnect on CF Workers
					{
						timeout: 10_000,
						interval: 100,
					},
				);

				// Create a new connection to check the disconnection count
				const newConn = client.connStateActor.getOrCreate().connect();

				// Poll until the replacement connection is registered after the reconnect handshake finishes.
				await vi.waitFor(async () => {
					const connectionIds = await debugHandle.getConnectionIds();
					expect(connectionIds.length).toBe(1);
				});

				// Clean up
				await newConn.dispose();

				// Poll until the second async disconnect bookkeeping pass updates the observer count.
				await vi.waitFor(
					async () => {
						const disconnects =
							await debugHandle.getDisconnectionCount();
						expect(disconnects).toBe(2);
					},
					// SSE takes a long time to disconnect on CF Workers
					{
						timeout: 10_000,
						interval: 100,
					},
				);
			});

			test("should update connection state", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				// Create a connection
				const conn = client.connStateActor.getOrCreate().connect();

				// Get the initial state
				const initialState = await conn.getConnectionState();
				expect(initialState.username).toBe("anonymous");

				// Update the connection state
				const updatedState = await conn.updateConnection({
					username: "newname",
					role: "moderator",
				});

				// Verify the state was updated
				expect(updatedState.username).toBe("newname");
				expect(updatedState.role).toBe("moderator");

				// Get the state again to verify persistence
				const latestState = await conn.getConnectionState();
				expect(latestState.username).toBe("newname");
				expect(latestState.role).toBe("moderator");

				// Clean up
				await conn.dispose();
			});
		});

		describe("Connection Communication", () => {
			test("should send messages to specific connections", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				// Create two connections
				const handle = client.connStateActor.getOrCreate();
				const conn1 = handle.connect();

				// Get connection states
				const state1 = await conn1.getConnectionState();

				const conn2 = handle.connect();
				const state2 = await conn2.getConnectionState();

				// Set up event listener on second connection
				const receivedMessages: any[] = [];
				conn2.on("directMessage", (data) => {
					receivedMessages.push(data);
				});

				await conn2.getConnectionState();

				const success = await conn1.sendToConnection(
					state2.id,
					"Hello from conn1",
				);
				expect(success).toBe(true);

				// Poll until the forwarded message arrives because conn-to-conn delivery is asynchronous.
				await vi.waitFor(async () => {
					// Verify message was received
					expect(receivedMessages.length).toBe(1);
					expect(receivedMessages[0].from).toBe(state1.id);
					expect(receivedMessages[0].message).toBe(
						"Hello from conn1",
					);
				});

				// Clean up
				await conn1.dispose();
				await conn2.dispose();
			});
		});
	});
});
