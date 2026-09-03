import { createServer } from "node:http";
import { describe, expect, test } from "vitest";
import { SLEEP_TIMEOUT } from "../../fixtures/driver-test-suite/sleep";
import { describeDriverMatrix } from "./shared-matrix";
import { setupDriverTest, waitFor } from "./shared-utils";
import { parseEventStream, runSseContractTests } from "./sse-contract-harness";

function delay(ms: number): Promise<"timeout"> {
	return new Promise((resolve) => setTimeout(() => resolve("timeout"), ms));
}

describeDriverMatrix(
	"Streaming Http",
	(driverTestConfig) => {
		describe("streaming http", () => {
			test("routes this suite through Gateway 3", async (c) => {
				const { client, metricsEndpoint } = await setupDriverTest(
					c,
					driverTestConfig,
				);
				if (!metricsEndpoint) {
					throw new Error(
						"driver did not expose the engine metrics endpoint",
					);
				}
				const actor = client.rawHttpActor.getOrCreate([
					"gateway3-route-proof",
				]);
				expect((await actor.fetch("api/hello")).ok).toBe(true);

				const metrics = await (await fetch(metricsEndpoint)).text();
				const gatewayRouteMetrics = metrics
					.split("\n")
					.filter((line) =>
						line.includes("pegboard_gateway_route_total{"),
					);
				const selectedGateway3 = gatewayRouteMetrics.find(
					(line) =>
						line.includes('gateway="gateway3"') &&
						line.includes('envoy_protocol="streaming"') &&
						line.includes('request_kind="http_other"') &&
						line.includes('mode="on"') &&
						line.includes('decision="sampled_in"'),
				);
				expect(
					selectedGateway3,
					gatewayRouteMetrics.join("\n"),
				).toBeDefined();
			});

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
				expect(response.headers.get("cache-control")).toBe(
					"no-cache, no-transform",
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

			test("fails the response body when the handler errors after headers", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const actor = client.rawHttpActor.getOrCreate([
					"error-response-stream",
				]);

				const response = await actor.fetch("api/error-response-stream");
				expect(response.status).toBe(200);
				const reader = response.body?.getReader();
				if (!reader) throw new Error("response body is missing");
				const first = await reader.read();
				expect(new TextDecoder().decode(first.value)).toBe(
					"data: ready\n\n",
				);
				await expect(reader.read()).rejects.toThrow();
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

			test("terminates an idle SSE body when the Envoy tunnel disconnects", async (c) => {
				const { client, hardCrashRuntime } = await setupDriverTest(
					c,
					driverTestConfig,
				);
				if (!hardCrashRuntime) {
					throw new Error(
						"native driver did not expose runtime crash control",
					);
				}
				const actor = client.rawHttpActor.getOrCreate([
					"envoy-disconnect-after-headers",
				]);
				const response = await actor.fetch("api/infinite-response");
				const reader = response.body?.getReader();
				if (!reader) throw new Error("response body is missing");
				expect((await reader.read()).done).toBe(false);

				await hardCrashRuntime();
				const terminal = reader.read().then(
					(result) => (result.done ? "terminal" : "data"),
					() => "terminal",
				);
				expect(await Promise.race([terminal, delay(8_000)])).toBe(
					"terminal",
				);
			}, 15_000);

			test("fails a request boundedly when the Envoy disconnects before headers", async (c) => {
				const { client, hardCrashRuntime } = await setupDriverTest(
					c,
					driverTestConfig,
				);
				if (!hardCrashRuntime) {
					throw new Error(
						"native driver did not expose runtime crash control",
					);
				}
				const actor = client.rawHttpActor.getOrCreate([
					"envoy-disconnect-before-headers",
				]);
				const request = actor.fetch("api/never-start-response");
				await delay(100);
				await hardCrashRuntime();
				const terminal = request.then(
					() => "terminal",
					() => "terminal",
				);
				expect(await Promise.race([terminal, delay(8_000)])).toBe(
					"terminal",
				);
			}, 15_000);

			test("routes skipReadyWait without crossing actor generations", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const actor = client.sleepWithRawHttp.getOrCreate();
				expect((await actor.getCounts()).startCount).toBe(1);
				await waitFor(driverTestConfig, SLEEP_TIMEOUT + 250);

				const response = await actor.fetch("long-request?duration=1", {
					skipReadyWait: true,
				});
				let expectedRequestCount: number;
				if (response.ok) {
					expect(await response.json()).toEqual({ completed: true });
					expectedRequestCount = 1;
				} else {
					expect(response.status).toBe(503);
					const errorCode = response.headers.get("x-rivet-error");
					expect([
						"envoy.actor_not_found",
						"envoy.actor_generation_mismatch",
					]).toContain(errorCode);
					expect(await response.json()).toMatchObject({
						group: "envoy",
						code: errorCode?.slice("envoy.".length),
					});
					expectedRequestCount = 0;
				}

				const counts = await actor.getCounts();
				expect(counts.requestCount).toBe(expectedRequestCount);
			}, 15_000);

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

			test("resumes a multi-window upload after a slow actor consumes it", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const actor = client.rawHttpActor.getOrCreate(["slow-upload"]);
				const requestBody = new Uint8Array(3 * 1024 * 1024 + 17).fill(
					7,
				);

				const response = await actor.fetch("api/slow-upload", {
					method: "POST",
					body: Buffer.from(requestBody),
				});
				expect(response.ok).toBe(true);
				expect(
					((await response.json()) as { totalBytes: number })
						.totalBytes,
				).toBe(requestBody.byteLength);
			}, 30_000);

			test("resumes a multi-window response after a slow client consumes it", async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				const actor = client.rawHttpActor.getOrCreate([
					"slow-response-client",
				]);
				const response = await actor.fetch("api/large-pull-response");
				const reader = response.body?.getReader();
				if (!reader) throw new Error("response body is missing");

				let totalBytes = 0;
				for (;;) {
					await delay(5);
					const next = await reader.read();
					if (next.done) break;
					expect(next.value.every((byte) => byte === 9)).toBe(true);
					totalBytes += next.value.byteLength;
				}
				expect(totalBytes).toBe(3 * 1024 * 1024 + 17);
			}, 30_000);

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
						upstream.closeAllConnections();
					});
				}
			}, 30_000);

			test("aborts Request.signal even when the handler does not read the body", async (c) => {
				let markStarted: () => void;
				const started = new Promise<void>((resolve) => {
					markStarted = resolve;
				});
				let markAborted: () => void;
				const aborted = new Promise<void>((resolve) => {
					markAborted = resolve;
				});
				const observer = createServer((incoming, response) => {
					if (incoming.url === "/started") {
						markStarted();
					} else if (incoming.url === "/aborted") {
						markAborted();
					}
					response.writeHead(204);
					response.end();
				});
				await new Promise<void>((resolve, reject) => {
					observer.once("error", reject);
					observer.listen(0, "127.0.0.1", resolve);
				});

				try {
					const address = observer.address();
					if (!address || typeof address === "string") {
						throw new Error("observer did not bind a TCP port");
					}
					const target = encodeURIComponent(
						`http://127.0.0.1:${address.port}`,
					);
					const { client } = await setupDriverTest(
						c,
						driverTestConfig,
					);
					const actor = client.rawHttpActor.getOrCreate([
						"request-cancellation",
					]);
					const abortController = new AbortController();
					const request = actor.fetch(
						`api/wait-for-request-abort?target=${target}`,
						{
							method: "POST",
							body: Buffer.from("ignored"),
							signal: abortController.signal,
						},
					);

					expect(
						await Promise.race([
							started.then(() => "started" as const),
							delay(2_000),
						]),
					).toBe("started");
					abortController.abort();
					await expect(request).rejects.toThrow();
					expect(
						await Promise.race([
							aborted.then(() => "aborted" as const),
							delay(2_000),
						]),
					).toBe("aborted");
				} finally {
					await new Promise<void>((resolve, reject) => {
						observer.close((error) => {
							if (error) reject(error);
							else resolve();
						});
						observer.closeAllConnections();
					});
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
		config: { engine: { gateway3: true } },
	},
);
