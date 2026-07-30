import { createServer } from "node:http";
import { describe, expect, test } from "vitest";
import { describeDriverMatrix } from "./shared-matrix";
import { setupDriverTest } from "./shared-utils";
import { parseEventStream, runSseContractTests } from "./sse-contract-harness";

function delay(ms: number): Promise<"timeout"> {
	return new Promise((resolve) => setTimeout(() => resolve("timeout"), ms));
}

describeDriverMatrix(
	"Streaming Http",
	(driverTestConfig) => {
		describe("streaming http", () => {
			test("streams response chunks before the body completes", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const actor = client.rawHttpActor.getOrCreate([
					"stream-response",
				]);

				const response = await actor.fetch("api/stream");
				expect(response.ok).toBe(true);
				expect(response.headers.get("content-type")).toContain(
					"text/event-stream",
				);

				const reader = response.body?.getReader();
				expect(reader).toBeDefined();
				if (!reader) throw new Error("response body is missing");
				const decoder = new TextDecoder();
				const first = await reader.read();
				expect(first.done).toBe(false);
				expect(decoder.decode(first.value)).toBe("data: first\n\n");

				const secondRead = reader.read();
				const earlySecond = await Promise.race([secondRead, delay(50)]);
				expect(earlySecond).toBe("timeout");

				const second = await secondRead;
				expect(second.done).toBe(false);
				expect(decoder.decode(second.value)).toBe("data: second\n\n");
				expect(await reader.read()).toEqual({
					done: true,
					value: undefined,
				});
			});

			test("keeps the request connection alive through the response stream", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const actorKey = ["stream-response-lifecycle"];
				const actor = client.rawHttpActor.getOrCreate(actorKey, {
					params: { streamLifecycle: true },
				});
				const observer = client.rawHttpActor.getOrCreate(actorKey);

				const response = await actor.fetch("api/stream-lifecycle");
				const reader = response.body?.getReader();
				expect(reader).toBeDefined();
				if (!reader) throw new Error("response body is missing");
				expect((await reader.read()).done).toBe(false);

				const activeState = (await (
					await observer.fetch("api/state")
				).json()) as {
					streamDisconnectedBeforeFinish: boolean;
					streamResponseFinished: boolean;
				};
				expect(activeState.streamResponseFinished).toBe(false);
				expect(activeState.streamDisconnectedBeforeFinish).toBe(false);

				expect(await reader.read()).toEqual({
					done: true,
					value: undefined,
				});
				const finishedState = (await (
					await observer.fetch("api/state")
				).json()) as {
					streamDisconnectedBeforeFinish: boolean;
					streamResponseFinished: boolean;
				};
				expect(finishedState.streamResponseFinished).toBe(true);
				expect(finishedState.streamDisconnectedBeforeFinish).toBe(
					false,
				);
			});

			test("aborts Request.signal when the client drops a started response stream", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const actor = client.rawHttpActor.getOrCreate([
					"response-stream-request-abort",
				]);

				const response = await actor.fetch(
					"api/wait-for-response-abort",
				);
				const reader = response.body?.getReader();
				expect(reader).toBeDefined();
				if (!reader) throw new Error("response body is missing");
				expect((await reader.read()).done).toBe(false);

				await reader.cancel();

				const deadline = Date.now() + 2_000;
				for (;;) {
					const state = (await (
						await actor.fetch("api/state")
					).json()) as {
						responseAbortObserved: boolean;
						responseAbortStarted: boolean;
					};
					expect(state.responseAbortStarted).toBe(true);
					if (state.responseAbortObserved) break;
					if (Date.now() >= deadline) {
						throw new Error(
							"actor did not abort Request.signal after the response client disconnected",
						);
					}
					await delay(25);
				}
			});

			test("exposes gateway-chunked request bodies as Request streams", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const actor = client.rawHttpActor.getOrCreate([
					"stream-upload",
				]);
				const requestBody = new Uint8Array(80 * 1024);
				requestBody.fill(1, 0, 40 * 1024);
				requestBody.fill(2, 40 * 1024);

				const response = await actor.fetch("api/upload-stream", {
					method: "POST",
					body: Buffer.from(requestBody),
				});

				expect(response.ok).toBe(true);
				const body = (await response.json()) as {
					chunkCount: number;
					contentLength: string | null;
					sizes: number[];
					totalBytes: number;
				};
				expect(body.totalBytes, JSON.stringify(body)).toBe(
					requestBody.byteLength,
				);
				expect(body.chunkCount).toBeGreaterThanOrEqual(2);
				expect(Math.max(...body.sizes)).toBeLessThanOrEqual(64 * 1024);
			});

			test("allows handlers to cancel unread streamed uploads", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const actor = client.rawHttpActor.getOrCreate([
					"cancel-stream-upload",
				]);
				const requestBody = new Uint8Array(2 * 1024 * 1024);

				const response = await actor.fetch("api/cancel-upload", {
					method: "POST",
					body: Buffer.from(requestBody),
				});

				expect(response.ok).toBe(true);
				expect(await response.text()).toBe("upload cancelled");
			});

			test("streams a large proxied SSE event", async (c) => {
				const eventData = "x".repeat(7 * 1024 * 1024);
				const upstream = createServer((_request, response) => {
					response.writeHead(200, {
						"content-type": "text/event-stream",
					});
					response.end(`data: ${eventData}\n\n`);
				});
				await new Promise<void>((resolve, reject) => {
					upstream.once("error", reject);
					upstream.listen(0, "127.0.0.1", resolve);
				});

				try {
					const address = upstream.address();
					if (!address || typeof address === "string") {
						throw new Error("upstream did not bind a TCP port");
					}
					const { client } = await setupDriverTest(
						c,
						driverTestConfig,
					);
					const actor = client.rawHttpActor.getOrCreate([
						"large-sse-response",
					]);
					const target = encodeURIComponent(
						`http://127.0.0.1:${address.port}`,
					);

					const response = await actor.fetch(
						`api/sse-proxy?target=${target}`,
					);
					if (!response.body) {
						throw new Error("proxied response has no body");
					}
					let parsedLength = 0;
					await parseEventStream(
						response.body,
						"",
						(event) => {
							parsedLength = event.data.length;
						},
						() => {},
					);

					expect(parsedLength).toBe(eventData.length);
				} finally {
					await new Promise<void>((resolve, reject) => {
						upstream.close((error) => {
							if (error) reject(error);
							else resolve();
						});
					});
				}
			}, 30_000);

			test("aborts Request.signal even when the handler does not read the body", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const actor = client.rawHttpActor.getOrCreate([
					"request-cancellation",
				]);
				const abortController = new AbortController();
				const request = actor.fetch("api/wait-for-request-abort", {
					method: "POST",
					body: Buffer.from("ignored"),
					signal: abortController.signal,
				});

				const startDeadline = Date.now() + 2_000;
				for (;;) {
					const state = (await (
						await actor.fetch("api/state")
					).json()) as {
						requestAbortStarted: boolean;
					};
					if (state.requestAbortStarted) break;
					if (Date.now() >= startDeadline) {
						throw new Error(
							"actor did not start the abortable request",
						);
					}
					await delay(25);
				}
				abortController.abort();
				await expect(request).rejects.toThrow();

				const deadline = Date.now() + 2_000;
				for (;;) {
					const state = (await (
						await actor.fetch("api/state")
					).json()) as {
						requestAbortObserved: boolean;
					};
					if (state.requestAbortObserved) break;
					if (Date.now() >= deadline) {
						throw new Error(
							"actor did not observe the aborted Request.signal",
						);
					}
					await delay(25);
				}
			});

			test("passes the LaunchDarkly SSE contract suite", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const actor = client.rawHttpActor.getOrCreate(["sse-contract"]);

				const output = await runSseContractTests(actor);
				expect(output).toContain("All tests passed");
			}, 360_000);
		});
	},
	{
		runtimes: ["native"],
		encodings: ["bare"],
		sqliteBackends: ["remote"],
	},
);
