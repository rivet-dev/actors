import * as cbor from "cbor-x";
import { describe, expect, test, vi } from "vitest";
import {
	CURRENT_VERSION as INSPECTOR_PROTOCOL_VERSION,
	TO_CLIENT_VERSIONED,
	TO_SERVER_VERSIONED,
} from "../../src/inspector/client.browser";
import type { WorkflowHistory as TransportWorkflowHistory } from "../../src/common/bare/transport/v1";
import { decodeWorkflowHistoryTransport } from "../../src/common/inspector-transport";
import { describeDriverMatrix } from "./shared-matrix";
import { setupDriverTest, waitFor } from "./shared-utils";

const WORKFLOW_READY_TIMEOUT_MS = 30_000;
const ACTIVE_WORKFLOW_INSPECTOR_TIMEOUT_MS = 45_000;

type WorkflowHistoryResponse = {
	history: {
		nameRegistry: string[];
		entries: unknown[];
		entryMetadata: Record<
			string,
			{
				status: string;
				error: string | null;
				attempts: number;
				lastAttemptAt: number;
				createdAt: number;
				completedAt: number | null;
				rollbackCompletedAt: number | null;
				rollbackError: string | null;
			}
		>;
	} | null;
	workflowState: string | null;
	isWorkflowEnabled: boolean;
};

async function fetchWorkflowHistory(
	gatewayUrl: string,
): Promise<WorkflowHistoryResponse> {
	const response = await fetch(
		buildInspectorUrl(gatewayUrl, "/inspector/workflow-history"),
		{
			headers: { Authorization: "Bearer token" },
		},
	);
	expect(response.status).toBe(200);
	return (await response.json()) as WorkflowHistoryResponse;
}

async function waitForInspectorJson<T>(
	gatewayUrl: string,
	path: string,
	assertReady: (data: T) => void,
	timeoutMs = WORKFLOW_READY_TIMEOUT_MS,
): Promise<T> {
	let ready!: T;

	// Poll because inspector routes can race actor startup and workflow state transitions during native runs.
	await vi.waitFor(
		async () => {
			const response = await fetch(buildInspectorUrl(gatewayUrl, path), {
				headers: { Authorization: "Bearer token" },
			});
			const body = (await response.json()) as
				| T
				| {
						group?: string;
						code?: string;
				  };

			if (
				response.status === 503 &&
				body?.group === "guard" &&
				body?.code === "actor_ready_timeout"
			) {
				throw new Error("actor inspector endpoint is still warming up");
			}

			expect(response.status).toBe(200);
			ready = body as T;
			assertReady(ready);
		},
		{ timeout: timeoutMs, interval: 100 },
	);

	return ready;
}

function buildInspectorUrl(
	gatewayUrl: string,
	path: string,
	searchParams?: Record<string, string>,
): string {
	const url = new URL(gatewayUrl);
	url.pathname = `${url.pathname.replace(/\/$/, "")}${path}`;
	for (const [key, value] of Object.entries(searchParams ?? {})) {
		url.searchParams.set(key, value);
	}
	return url.toString();
}

function buildInspectorWebSocketUrl(gatewayUrl: string): string {
	const url = new URL(
		buildInspectorUrl(gatewayUrl, "/inspector/connect", {
			protocol_version: String(INSPECTOR_PROTOCOL_VERSION),
		}),
	);
	url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
	return url.toString();
}

async function toBinaryPayload(data: Blob | ArrayBuffer | Buffer | string) {
	if (typeof data === "string") {
		return new TextEncoder().encode(data);
	}
	if (data instanceof ArrayBuffer) {
		return new Uint8Array(data);
	}
	if (data instanceof Blob) {
		return new Uint8Array(await data.arrayBuffer());
	}
	return new Uint8Array(data);
}

type InspectorMessage = ReturnType<typeof TO_CLIENT_VERSIONED.deserialize>;

async function waitForInspectorMessage(
	ws: WebSocket,
	timeoutMs = 10_000,
	predicate?: (message: InspectorMessage) => boolean,
) {
	return await new Promise<InspectorMessage>((resolve, reject) => {
		const timeout = setTimeout(() => {
			cleanup();
			reject(new Error("Inspector websocket message timed out"));
		}, timeoutMs);

		const cleanup = () => {
			clearTimeout(timeout);
			ws.removeEventListener("message", onMessage);
			ws.removeEventListener("error", onError);
		};

		const onError = () => {
			cleanup();
			reject(new Error("Inspector websocket errored"));
		};

		const onMessage = async (event: MessageEvent) => {
			try {
				const payload = await toBinaryPayload(
					event.data as Blob | ArrayBuffer | Buffer | string,
				);
				const decoded = TO_CLIENT_VERSIONED.deserialize(
					payload,
					INSPECTOR_PROTOCOL_VERSION,
				);
				if (predicate && !predicate(decoded)) {
					return;
				}
				cleanup();
				resolve(decoded);
			} catch (error) {
				cleanup();
				reject(error);
			}
		};

		ws.addEventListener("message", onMessage);
		ws.addEventListener("error", onError);
	});
}

async function waitForInspectorMessageWithTag<
	T extends InspectorMessage["body"]["tag"],
>(
	ws: WebSocket,
	tag: T,
	timeoutMs = 10_000,
): Promise<Extract<InspectorMessage, { body: { tag: T } }>> {
	const message = await waitForInspectorMessage(
		ws,
		timeoutMs,
		(candidate) => candidate.body.tag === tag,
	);
	return message as Extract<InspectorMessage, { body: { tag: T } }>;
}

async function waitForInspectorStateUpdated(
	ws: WebSocket,
	expectedState: unknown,
	timeoutMs = 10_000,
) {
	const message = await waitForInspectorMessage(
		ws,
		timeoutMs,
		(candidate) =>
			candidate.body.tag === "StateUpdated" &&
			JSON.stringify(
				cbor.decode(new Uint8Array(candidate.body.val.state)),
			) === JSON.stringify(expectedState),
	);
	return message as Extract<
		InspectorMessage,
		{ body: { tag: "StateUpdated" } }
	>;
}

async function waitForInspectorOpen(ws: WebSocket, timeoutMs = 10_000) {
	await new Promise<void>((resolve, reject) => {
		const timeout = setTimeout(() => {
			cleanup();
			reject(new Error("Inspector websocket open timed out"));
		}, timeoutMs);

		const cleanup = () => {
			clearTimeout(timeout);
			ws.removeEventListener("open", onOpen);
			ws.removeEventListener("error", onError);
		};

		const onOpen = () => {
			cleanup();
			resolve();
		};

		const onError = () => {
			cleanup();
			reject(new Error("Inspector websocket failed to open"));
		};

		ws.addEventListener("open", onOpen);
		ws.addEventListener("error", onError);
	});
}

function isActorStoppingDbError(error: unknown): boolean {
	return (
		error instanceof Error &&
		error.message.includes(
			"Actor stopping: database accessed after actor stopped",
		)
	);
}

describeDriverMatrix("Actor Inspector", (driverTestConfig) => {
	describe("Actor Inspector HTTP API", () => {
		test("GET /inspector/state returns actor state", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.counter.getOrCreate(["inspector-state"]);

			// Set some state first
			await handle.increment(5);

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/state"),
				{
					headers: { Authorization: "Bearer token" },
				},
			);
			expect(response.status).toBe(200);
			const data = await response.json();
			expect(data).toEqual({
				state: { count: 5 },
				isStateEnabled: true,
			});
		});

		test("PATCH /inspector/state updates actor state", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.counter.getOrCreate(["inspector-set-state"]);

			await handle.increment(5);

			const gatewayUrl = await handle.getGatewayUrl();

			// Replace state
			const patchResponse = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/state"),
				{
					method: "PATCH",
					headers: {
						"Content-Type": "application/json",
						Authorization: "Bearer token",
					},
					body: JSON.stringify({ state: { count: 42 } }),
				},
			);
			expect(patchResponse.status).toBe(200);
			const patchData = await patchResponse.json();
			expect(patchData).toEqual({ ok: true });

			// Verify via action
			const count = await handle.getCount();
			expect(count).toBe(42);
		});

		test("inspector websocket Init disables workflows for non-workflow actors", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.counter.getOrCreate([
				"inspector-init-workflow-disabled",
			]);

			await handle.increment(0);
			const gatewayUrl = await handle.getGatewayUrl();
			const ws = new WebSocket(buildInspectorWebSocketUrl(gatewayUrl), [
				"rivet",
				"rivet_inspector_token.token",
			]);
			ws.binaryType = "arraybuffer";

			try {
				await waitForInspectorOpen(ws);
				const init = await waitForInspectorMessageWithTag(ws, "Init");
				expect(init.body.val.isWorkflowEnabled).toBe(false);

				const replayResponse = waitForInspectorMessageWithTag(
					ws,
					"WorkflowReplayResponse",
				);
				ws.send(
					TO_SERVER_VERSIONED.serialize(
						{
							body: {
								tag: "WorkflowReplayRequest",
								val: { id: 1n, entryId: null },
							},
						},
						INSPECTOR_PROTOCOL_VERSION,
					),
				);
				expect((await replayResponse).body.val.isWorkflowEnabled).toBe(
					false,
				);
			} finally {
				ws.close();
			}
		});

		test("PATCH /inspector/state pushes StateUpdated over inspector websocket", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.counter.getOrCreate([
				"inspector-state-websocket-patch",
			]);

			await handle.increment(5);

			const gatewayUrl = await handle.getGatewayUrl();
			const ws = new WebSocket(buildInspectorWebSocketUrl(gatewayUrl), [
				"rivet",
				"rivet_inspector_token.token",
			]);
			ws.binaryType = "arraybuffer";

			try {
				await waitForInspectorOpen(ws);

				await waitForInspectorMessageWithTag(ws, "Init");

				ws.send(
					TO_SERVER_VERSIONED.serialize(
						{
							body: {
								tag: "PatchStateRequest",
								val: {
									state: new Uint8Array(
										cbor.encode({ count: 42 }),
									).buffer,
								},
							},
						},
						INSPECTOR_PROTOCOL_VERSION,
					),
				);

				const stateUpdated = await waitForInspectorStateUpdated(ws, {
					count: 42,
				});
				expect(
					cbor.decode(new Uint8Array(stateUpdated.body.val.state)),
				).toEqual({ count: 42 });

				ws.send(
					TO_SERVER_VERSIONED.serialize(
						{
							body: {
								tag: "StateRequest",
								val: { id: 1n },
							},
						},
						INSPECTOR_PROTOCOL_VERSION,
					),
				);

				const stateResponse = await waitForInspectorMessageWithTag(
					ws,
					"StateResponse",
				);
				expect(
					cbor.decode(new Uint8Array(stateResponse.body.val.state!)),
				).toEqual({ count: 42 });
			} finally {
				ws.close();
			}
		});

		test("GET /inspector/connections returns connections list", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.counter.getOrCreate([
				"inspector-connections",
			]);

			// Ensure actor exists
			await handle.increment(0);

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/connections"),
				{
					headers: { Authorization: "Bearer token" },
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as {
				connections: unknown[];
			};
			expect(data).toHaveProperty("connections");
			expect(Array.isArray(data.connections)).toBe(true);
		});

		test("GET /inspector/rpcs returns available actions", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.counter.getOrCreate(["inspector-rpcs"]);

			// Ensure actor exists
			await handle.increment(0);

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/rpcs"),
				{
					headers: { Authorization: "Bearer token" },
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as { rpcs: string[] };
			expect(data).toHaveProperty("rpcs");
			expect(data.rpcs).toContain("increment");
			expect(data.rpcs).toContain("getCount");
			expect(data.rpcs).toContain("setCount");
		});

		test("POST /inspector/action/:name executes an action", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.counter.getOrCreate(["inspector-action"]);

			await handle.increment(10);

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/action/increment"),
				{
					method: "POST",
					headers: {
						"Content-Type": "application/json",
						Authorization: "Bearer token",
					},
					body: JSON.stringify({ args: [5] }),
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as { output: number };
			expect(data.output).toBe(15);

			// Verify via normal action
			const count = await handle.getCount();
			expect(count).toBe(15);
		});

		test("GET /inspector/queue returns queue status", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.queueActor.getOrCreate(["inspector-queue"]);

			await handle.send("greeting", { hello: "queue-size" });

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/queue", {
					limit: "10",
				}),
				{
					headers: { Authorization: "Bearer token" },
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as {
				size: number;
				maxSize: number;
				truncated: boolean;
				messages: unknown[];
			};
			expect(data).toHaveProperty("size");
			expect(data).toHaveProperty("maxSize");
			expect(data).toHaveProperty("truncated");
			expect(data).toHaveProperty("messages");
			expect(typeof data.size).toBe("number");
			expect(typeof data.maxSize).toBe("number");
			expect(typeof data.truncated).toBe("boolean");
			expect(Array.isArray(data.messages)).toBe(true);
			expect(data.size).toBeGreaterThan(0);

			const summaryResponse = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/summary"),
				{
					headers: { Authorization: "Bearer token" },
				},
			);
			expect(summaryResponse.status).toBe(200);
			const summary = (await summaryResponse.json()) as {
				queueSize: number;
			};
			expect(summary.queueSize).toBeGreaterThan(0);
		});

		test("POST then DELETE /inspector/queue injects and clears messages", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.queueActor.getOrCreate([
				"inspector-queue-mutate",
			]);
			// Seed a message so the actor exists and the inspector routes are live.
			await handle.send("greeting", { hello: "seed" });

			const gatewayUrl = await handle.getGatewayUrl();

			// Inject a message through the inspector.
			const injectResponse = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/queue"),
				{
					method: "POST",
					headers: {
						"Content-Type": "application/json",
						Authorization: "Bearer token",
					},
					body: JSON.stringify({
						name: "injected",
						body: { hello: "world" },
					}),
				},
			);
			expect(injectResponse.status).toBe(200);
			const injected = (await injectResponse.json()) as {
				id: string;
				name: string;
				createdAtMs: number;
			};
			// id is serialized as a string to avoid bigint truncation.
			expect(typeof injected.id).toBe("string");
			expect(injected.name).toBe("injected");

			// The injected message is visible in the queue.
			const beforeClear = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/queue", {
					limit: "50",
				}),
				{ headers: { Authorization: "Bearer token" } },
			);
			expect(beforeClear.status).toBe(200);
			const beforeData = (await beforeClear.json()) as { size: number };
			expect(beforeData.size).toBeGreaterThan(0);

			// Clear the queue.
			const clearResponse = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/queue"),
				{
					method: "DELETE",
					headers: { Authorization: "Bearer token" },
				},
			);
			expect(clearResponse.status).toBe(200);

			// The queue is now empty.
			const afterClear = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/queue", {
					limit: "50",
				}),
				{ headers: { Authorization: "Bearer token" } },
			);
			expect(afterClear.status).toBe(200);
			const afterData = (await afterClear.json()) as { size: number };
			expect(afterData.size).toBe(0);
		});

		test("POST /inspector/queue rejects an empty name", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.queueActor.getOrCreate([
				"inspector-queue-empty-name",
			]);
			await handle.send("greeting", { hello: "seed" });

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/queue"),
				{
					method: "POST",
					headers: {
						"Content-Type": "application/json",
						Authorization: "Bearer token",
					},
					body: JSON.stringify({
						name: "",
						body: { hello: "world" },
					}),
				},
			);
			expect(response.status).toBe(400);
			const error = (await response.json()) as { code: string };
			expect(error.code).toBe("invalid_request");
		});

		test("POST /inspector/queue rejects malformed JSON", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.queueActor.getOrCreate([
				"inspector-queue-bad-json",
			]);
			await handle.send("greeting", { hello: "seed" });

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/queue"),
				{
					method: "POST",
					headers: {
						"Content-Type": "application/json",
						Authorization: "Bearer token",
					},
					body: "{ not valid json",
				},
			);
			expect(response.status).toBe(400);
			const error = (await response.json()) as { code: string };
			expect(error.code).toBe("invalid_request");
		});

		test("GET /inspector/traces returns trace data", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.counter.getOrCreate(["inspector-traces"]);

			// Perform an action to generate traces
			await handle.increment(1);

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/traces", {
					startMs: "0",
					endMs: String(Date.now() + 60000),
					limit: "100",
				}),
				{
					headers: { Authorization: "Bearer token" },
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as {
				otlp: unknown;
				clamped: boolean;
			};
			expect(data).toHaveProperty("otlp");
			expect(data).toHaveProperty("clamped");
			expect(typeof data.clamped).toBe("boolean");
		});

		test("GET /inspector/workflow-history returns workflow status", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.counter.getOrCreate(["inspector-workflow"]);

			// Ensure actor exists
			await handle.increment(0);

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/workflow-history"),
				{
					headers: { Authorization: "Bearer token" },
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as {
				history: unknown;
				isWorkflowEnabled: boolean;
			};
			expect(data).toHaveProperty("history");
			expect(data).toHaveProperty("isWorkflowEnabled");
			// Counter actor has no workflow, so it should be disabled
			expect(data.isWorkflowEnabled).toBe(false);
			expect(data.history).toBeNull();
		});

		test("GET /inspector/database/schema returns SQLite schema", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.dbActorRaw.getOrCreate([
				`inspector-database-schema-${crypto.randomUUID()}`,
			]);

			await handle.insertValue("Alice");
			await handle.insertValue("Bob");

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/database/schema"),
				{
					headers: { Authorization: "Bearer token" },
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as {
				schema: {
					tables: Array<{
						table: { schema: string; name: string; type: string };
						columns: Array<{ name: string }>;
						records: number;
					}>;
				};
			};

			expect(Array.isArray(data.schema.tables)).toBe(true);
			const testDataTable = data.schema.tables.find(
				(table) => table.table.name === "test_data",
			);
			expect(testDataTable).toBeDefined();
			expect(testDataTable?.table.schema).toBe("main");
			expect(testDataTable?.table.type).toBe("table");
			expect(testDataTable?.records).toBe(2);
			expect(testDataTable?.columns.map((column) => column.name)).toEqual(
				["id", "value", "payload", "created_at"],
			);
		});

		test("GET /inspector/workflow-history returns populated history for active workflows", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.workflowRunningStepActor.getOrCreate([
				"inspector-workflow-active",
				crypto.randomUUID(),
			]);
			const gatewayUrl = await handle.getGatewayUrl();
			const data = await waitForInspectorJson<WorkflowHistoryResponse>(
				gatewayUrl,
				"/inspector/workflow-history",
				(history) => {
					expect(history.isWorkflowEnabled).toBe(true);
					expect(["pending", "running"]).toContain(
						history.workflowState,
					);
					expect(history.history).not.toBeNull();
					expect(
						history.history?.nameRegistry.length,
					).toBeGreaterThan(0);
					expect(history.history?.entries.length).toBeGreaterThan(0);
					expect(
						Object.keys(history.history?.entryMetadata ?? {})
							.length,
					).toBeGreaterThan(0);
				},
				ACTIVE_WORKFLOW_INSPECTOR_TIMEOUT_MS,
			);
			expect(data.isWorkflowEnabled).toBe(true);
			expect(["pending", "running"]).toContain(data.workflowState);
			expect(data.history).not.toBeNull();
			expect(data.history?.nameRegistry.length).toBeGreaterThan(0);
			expect(data.history?.entries.length).toBeGreaterThan(0);
			expect(
				Object.keys(data.history?.entryMetadata ?? {}).length,
			).toBeGreaterThan(0);

			await handle.release();
			// Poll until the released workflow finishes because release() only unblocks the run handler.
			await vi.waitFor(
				async () => {
					expect((await handle.getState()).finishedAt).not.toBeNull();
				},
				{ timeout: WORKFLOW_READY_TIMEOUT_MS, interval: 100 },
			);
		});

		test("inspector websocket workflow history stays raw BARE", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.workflowRunningStepActor.getOrCreate([
				"inspector-workflow-ws-bare",
				crypto.randomUUID(),
			]);
			const gatewayUrl = await handle.getGatewayUrl();
			const ws = new WebSocket(buildInspectorWebSocketUrl(gatewayUrl), [
				"rivet",
				"rivet_inspector_token.token",
			]);
			ws.binaryType = "arraybuffer";

			try {
				await waitForInspectorOpen(ws);
				await waitForInspectorMessageWithTag(ws, "Init");

				let raw: ArrayBuffer | undefined;
				// Poll because history stays null until the run handler records its first entry.
				await vi.waitFor(
					async () => {
						const pending = waitForInspectorMessageWithTag(
							ws,
							"WorkflowHistoryResponse",
						);
						ws.send(
							TO_SERVER_VERSIONED.serialize(
								{
									body: {
										tag: "WorkflowHistoryRequest",
										val: { id: 1n },
									},
								},
								INSPECTOR_PROTOCOL_VERSION,
							),
						);
						const body = (await pending).body.val;
						expect(body.isWorkflowEnabled).toBe(true);
						expect(body.history).not.toBeNull();
						raw = body.history as ArrayBuffer;
					},
					{
						timeout: ACTIVE_WORKFLOW_INSPECTOR_TIMEOUT_MS,
						interval: 250,
					},
				);

				// Decoded outside the poll: CBOR-wrapped bytes are a hard
				// failure, not something that becomes valid on a later attempt.
				const decoded: TransportWorkflowHistory =
					decodeWorkflowHistoryTransport(raw as ArrayBuffer);
				expect(decoded.nameRegistry.length).toBeGreaterThan(0);
				expect(decoded.entries.length).toBeGreaterThan(0);
			} finally {
				ws.close();
			}

			// `Init` carries history from the same bridge call, so assert it too.
			// It arrives once per connection, hence a second socket: history is
			// only guaranteed to be recorded now that the request above saw it.
			const initWs = new WebSocket(
				buildInspectorWebSocketUrl(gatewayUrl),
				["rivet", "rivet_inspector_token.token"],
			);
			initWs.binaryType = "arraybuffer";

			try {
				await waitForInspectorOpen(initWs);
				const init = await waitForInspectorMessageWithTag(
					initWs,
					"Init",
				);
				expect(init.body.val.isWorkflowEnabled).toBe(true);
				expect(init.body.val.workflowHistory).not.toBeNull();
				// Throws if the bytes were CBOR-wrapped instead of BARE.
				const initDecoded = decodeWorkflowHistoryTransport(
					init.body.val.workflowHistory as ArrayBuffer,
				);
				expect(initDecoded.nameRegistry.length).toBeGreaterThan(0);
				expect(initDecoded.entries.length).toBeGreaterThan(0);
			} finally {
				initWs.close();
			}

			await handle.release();
			// Poll until the released workflow finishes because release() only unblocks the run handler.
			await vi.waitFor(
				async () => {
					expect((await handle.getState()).finishedAt).not.toBeNull();
				},
				{ timeout: WORKFLOW_READY_TIMEOUT_MS, interval: 100 },
			);
		});

		test("POST /inspector/workflow/replay replays a completed workflow from the beginning", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.workflowReplayActor.getOrCreate([
				"inspector-workflow-replay",
				crypto.randomUUID(),
			]);

			// Poll until the first run finishes and persists its baseline timeline before replay.
			await vi.waitFor(
				async () => {
					expect(await handle.getTimeline()).toEqual(["one", "two"]);
				},
				{ timeout: WORKFLOW_READY_TIMEOUT_MS, interval: 100 },
			);

			const gatewayUrl = await handle.getGatewayUrl();
			let response: Response | undefined;
			// Poll the replay endpoint because completed workflow history can become visible slightly after the actor action returns.
			await vi.waitFor(
				async () => {
					const replayResponse = await fetch(
						buildInspectorUrl(
							gatewayUrl,
							"/inspector/workflow/replay",
						),
						{
							method: "POST",
							headers: {
								"Content-Type": "application/json",
								Authorization: "Bearer token",
							},
							body: JSON.stringify({}),
						},
					);
					expect(replayResponse.status).toBe(200);
					response = replayResponse;
				},
				{ timeout: WORKFLOW_READY_TIMEOUT_MS, interval: 100 },
			);

			const data = (await response.json()) as {
				history: {
					nameRegistry: string[];
					entries: unknown[];
					entryMetadata: Record<string, unknown>;
				} | null;
				isWorkflowEnabled: boolean;
			};
			expect(data.isWorkflowEnabled).toBe(true);
			expect(data.history).not.toBeNull();

			// Poll until the replayed workflow repopulates the persisted timeline from the beginning.
			await vi.waitFor(
				async () => {
					expect(await handle.getTimeline()).toEqual([
						"one",
						"two",
						"one",
						"two",
					]);
				},
				{ timeout: WORKFLOW_READY_TIMEOUT_MS, interval: 100 },
			);
		});

		test("POST /inspector/database/execute runs read-only queries", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.dbActorRaw.getOrCreate([
				"inspector-database-select",
			]);

			await handle.reset();
			await handle.insertValue("alpha");
			await handle.insertValue("beta");

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/database/execute"),
				{
					method: "POST",
					headers: {
						"Content-Type": "application/json",
						Authorization: "Bearer token",
					},
					body: JSON.stringify({
						sql: "SELECT value FROM test_data ORDER BY id",
					}),
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as {
				rows: Array<{ value: string }>;
			};
			expect(data.rows).toEqual([{ value: "alpha" }, { value: "beta" }]);
		});

		test("GET /inspector/database/rows returns SQLite rows", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.dbActorRaw.getOrCreate([
				`inspector-database-rows-${crypto.randomUUID()}`,
			]);

			await handle.insertValue("Alice");
			let inserted = false;
			for (let attempt = 0; attempt < 40; attempt++) {
				try {
					await handle.insertValue("Bob");
					inserted = true;
					break;
				} catch (error) {
					if (!isActorStoppingDbError(error)) {
						throw error;
					}
					await waitFor(driverTestConfig, 25);
				}
			}
			expect(inserted).toBe(true);

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/database/rows", {
					table: "test_data",
					limit: "1",
					offset: "1",
				}),
				{
					headers: { Authorization: "Bearer token" },
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as {
				rows: Array<{
					id: number;
					value: string;
					payload: string;
					created_at: number;
				}>;
			};

			expect(data.rows).toHaveLength(1);
			expect(data.rows[0]?.id).toBe(2);
			expect(data.rows[0]?.value).toBe("Bob");
			expect(data.rows[0]?.payload).toBe("");
			expect(typeof data.rows[0]?.created_at).toBe("number");
		});

		test("POST /inspector/database/execute supports named properties", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.dbActorRaw.getOrCreate([
				"inspector-database-properties",
			]);

			await handle.reset();
			await handle.insertValue("alpha");
			await handle.insertValue("beta");

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/database/execute"),
				{
					method: "POST",
					headers: {
						"Content-Type": "application/json",
						Authorization: "Bearer token",
					},
					body: JSON.stringify({
						sql: "SELECT value FROM test_data WHERE value = :value",
						properties: { value: "beta" },
					}),
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as {
				rows: Array<{ value: string }>;
			};
			expect(data.rows).toEqual([{ value: "beta" }]);
		});

		test("POST /inspector/workflow/replay rejects workflows that are currently in flight", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.workflowRunningStepActor.getOrCreate([
				"inspector-workflow-replay-in-flight",
				crypto.randomUUID(),
			]);
			const gatewayUrl = await handle.getGatewayUrl();

			// Poll the exact workflow-history endpoint because replay eligibility is asserted against its pending or running state.
			await vi.waitFor(
				async () => {
					const history = await fetchWorkflowHistory(gatewayUrl);
					expect(history.isWorkflowEnabled).toBe(true);
					expect(["pending", "running"]).toContain(
						history.workflowState,
					);
				},
				{ timeout: WORKFLOW_READY_TIMEOUT_MS, interval: 100 },
			);

			let response!: Response;
			// Poll the replay endpoint until it observes the in-flight workflow state and returns the structured 409.
			await vi.waitFor(
				async () => {
					const replayResponse = await fetch(
						buildInspectorUrl(
							gatewayUrl,
							"/inspector/workflow/replay",
						),
						{
							method: "POST",
							headers: {
								"Content-Type": "application/json",
								Authorization: "Bearer token",
							},
							body: JSON.stringify({}),
						},
					);
					expect(replayResponse.status).toBe(409);
					response = replayResponse;
				},
				{ timeout: WORKFLOW_READY_TIMEOUT_MS, interval: 100 },
			);
			expect(response.status).toBe(409);
			const data = (await response.json()) as {
				group: string;
				code: string;
				message: string;
				metadata: unknown;
			};
			expect(data).toEqual({
				group: "actor",
				code: "workflow_in_flight",
				message:
					"Workflow replay is unavailable while the workflow is currently in flight.",
				metadata: null,
			});

			await handle.release();
			// Poll until the released workflow finishes because release() only unblocks the run handler.
			await vi.waitFor(
				async () => {
					expect((await handle.getState()).finishedAt).not.toBeNull();
				},
				{ timeout: WORKFLOW_READY_TIMEOUT_MS, interval: 100 },
			);
		});

		test("POST /inspector/database/execute runs mutations", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.dbActorRaw.getOrCreate([
				"inspector-database-mutation",
			]);

			await handle.reset();

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/database/execute"),
				{
					method: "POST",
					headers: {
						"Content-Type": "application/json",
						Authorization: "Bearer token",
					},
					body: JSON.stringify({
						sql: "INSERT INTO test_data (value, payload, created_at) VALUES (?, '', ?)",
						args: ["from-inspector", Date.now()],
					}),
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as {
				rows: unknown[];
			};
			expect(data.rows).toEqual([]);
			expect(await handle.getCount()).toBe(1);
			const values = await handle.getValues();
			expect(values.at(-1)?.value).toBe("from-inspector");
		});

		test("GET /inspector/summary returns full actor snapshot", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.counter.getOrCreate(["inspector-summary"]);

			await handle.increment(7);

			const gatewayUrl = await handle.getGatewayUrl();
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/summary"),
				{
					headers: { Authorization: "Bearer token" },
				},
			);
			expect(response.status).toBe(200);
			const data = (await response.json()) as {
				state: { count: number };
				connections: unknown[];
				rpcs: string[];
				queueSize: number;
				isStateEnabled: boolean;
				isDatabaseEnabled: boolean;
				isWorkflowEnabled: boolean;
				workflowHistory: unknown;
			};
			expect(data.state).toEqual({ count: 7 });
			expect(Array.isArray(data.connections)).toBe(true);
			expect(data.rpcs).toContain("increment");
			expect(typeof data.queueSize).toBe("number");
			expect(data.isStateEnabled).toBe(true);
			expect(typeof data.isDatabaseEnabled).toBe("boolean");
			expect(data.isWorkflowEnabled).toBe(false);
			expect(data.workflowHistory).toBeNull();
		});

		test("GET /inspector/summary returns populated workflow history for active workflows", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.workflowRunningStepActor.getOrCreate([
				"inspector-summary-workflow",
				crypto.randomUUID(),
			]);
			const gatewayUrl = await handle.getGatewayUrl();
			const data = await waitForInspectorJson<{
				isWorkflowEnabled: boolean;
				workflowState: string | null;
				workflowHistory: {
					nameRegistry: string[];
					entries: unknown[];
					entryMetadata: Record<string, unknown>;
				} | null;
			}>(
				gatewayUrl,
				"/inspector/summary",
				(summary) => {
					expect(summary.isWorkflowEnabled).toBe(true);
					expect(["pending", "running"]).toContain(
						summary.workflowState,
					);
					expect(summary.workflowHistory).not.toBeNull();
					expect(
						summary.workflowHistory?.nameRegistry.length,
					).toBeGreaterThan(0);
					expect(
						summary.workflowHistory?.entries.length,
					).toBeGreaterThan(0);
					expect(
						Object.keys(
							summary.workflowHistory?.entryMetadata ?? {},
						).length,
					).toBeGreaterThan(0);
				},
				ACTIVE_WORKFLOW_INSPECTOR_TIMEOUT_MS,
			);
			expect(data.isWorkflowEnabled).toBe(true);
			expect(["pending", "running"]).toContain(data.workflowState);
			expect(data.workflowHistory).not.toBeNull();
			expect(data.workflowHistory?.nameRegistry.length).toBeGreaterThan(
				0,
			);
			expect(data.workflowHistory?.entries.length).toBeGreaterThan(0);
			expect(
				Object.keys(data.workflowHistory?.entryMetadata ?? {}).length,
			).toBeGreaterThan(0);

			await handle.release();
			// Poll until the released workflow finishes because release() only unblocks the run handler.
			await vi.waitFor(
				async () => {
					expect((await handle.getState()).finishedAt).not.toBeNull();
				},
				{ timeout: WORKFLOW_READY_TIMEOUT_MS, interval: 100 },
			);
		});

		test("inspector endpoints require auth in non-dev mode", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const handle = client.counter.getOrCreate(["inspector-auth"]);

			await handle.increment(0);

			const gatewayUrl = await handle.getGatewayUrl();

			// Request with wrong token should fail
			const response = await fetch(
				buildInspectorUrl(gatewayUrl, "/inspector/state"),
				{
					headers: { Authorization: "Bearer wrong-token" },
				},
			);
			expect(response.status).toBe(401);
		});
	});
});
