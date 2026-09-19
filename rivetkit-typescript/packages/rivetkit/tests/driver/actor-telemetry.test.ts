import { context, propagation, trace } from "@opentelemetry/api";
import { NodeTracerProvider } from "@opentelemetry/sdk-trace-node";
import getPort from "get-port";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import type { registry } from "../../fixtures/driver-test-suite/registry-static";
import { type Client, createClient } from "../../src/client/mod";
import { RAY_BAGGAGE_KEY } from "../../src/common/otel-context";
import { startOtlpCollector } from "../fixtures/otlp-collector";
import { describeDriverMatrix } from "./shared-matrix";
import type { DriverDeployOutput, DriverTestConfig } from "./shared-types";

new NodeTracerProvider().register();

const OTLP_STATUS_OK = 1;
const OTLP_STATUS_ERROR = 2;
const OTLP_SPAN_KIND_SERVER = 2;

interface ExportedSpan {
	name: string;
	traceId: string;
	spanId: string;
	parentSpanId?: string;
	traceState?: string;
	kind?: number;
	statusCode: number;
	endTimeUnixNano: bigint;
	attributes: Record<string, string | undefined>;
	links: Array<{ traceId: string; spanId: string }>;
}

function otlpStatusCode(code: number | string | undefined): number {
	if (typeof code === "number") return code;
	if (code === "STATUS_CODE_OK") return OTLP_STATUS_OK;
	if (code === "STATUS_CODE_ERROR") return OTLP_STATUS_ERROR;
	return 0;
}

/** Flattens OTLP/JSON export bodies into the spans they carry. */
function exportedSpans(exports: Buffer[]): ExportedSpan[] {
	type OtlpAttribute = {
		key: string;
		value: { stringValue?: string; intValue?: string | number };
	};
	type OtlpSpan = Omit<
		ExportedSpan,
		"attributes" | "endTimeUnixNano" | "statusCode"
	> & {
		attributes?: OtlpAttribute[];
		endTimeUnixNano?: string | number;
		status?: { code?: number | string };
		links?: Array<{ traceId: string; spanId: string }>;
	};
	type OtlpPayload = {
		resourceSpans?: Array<{ scopeSpans?: Array<{ spans?: OtlpSpan[] }> }>;
	};
	return exports.flatMap((body) => {
		const payload = JSON.parse(body.toString("utf8")) as OtlpPayload;
		return (payload.resourceSpans ?? []).flatMap((resource) =>
			(resource.scopeSpans ?? []).flatMap((scope) =>
				(scope.spans ?? []).map((span) => ({
					name: span.name,
					traceId: span.traceId,
					spanId: span.spanId,
					parentSpanId: span.parentSpanId || undefined,
					traceState: span.traceState,
					kind: span.kind,
					statusCode: otlpStatusCode(span.status?.code),
					endTimeUnixNano: BigInt(span.endTimeUnixNano ?? 0),
					attributes: Object.fromEntries(
						(span.attributes ?? []).map((attribute) => [
							attribute.key,
							attribute.value.stringValue ??
								(attribute.value.intValue === undefined
									? undefined
									: String(attribute.value.intValue)),
						]),
					),
					links: (span.links ?? []).map((link) => ({
						traceId: link.traceId,
						spanId: link.spanId,
					})),
				})),
			),
		);
	});
}

/**
 * Polls until the exported spans satisfy `ready`, then returns them. Parent
 * and child spans can land in different export batches, so callers that
 * assert parentage must wait for both.
 */
async function waitForSpans(
	exports: Buffer[],
	description: string,
	ready: (spans: ExportedSpan[]) => boolean,
	timeoutMs = 10_000,
): Promise<ExportedSpan[]> {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		const spans = exportedSpans(exports);
		if (ready(spans)) {
			return spans;
		}
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
	const arrived = exportedSpans(exports)
		.map((span) => `${span.name} (${span.spanId} < ${span.parentSpanId})`)
		.join(", ");
	throw new Error(
		`timed out waiting for ${description}; exported: ${arrived}`,
	);
}

function findInvocation(
	spans: ExportedSpan[],
	actionName: string,
): ExportedSpan | undefined {
	return spans.find(
		(span) =>
			span.attributes["rivet.invocation.type"] !== undefined &&
			span.attributes["rivet.action.name"] === actionName,
	);
}

function isSqliteSpan(span: ExportedSpan): boolean {
	return span.name === "rivet.sqlite.execute";
}

function tracedEnv(tracesEndpoint: string): Record<string, string> {
	return {
		OTEL_EXPORTER_OTLP_TRACES_ENDPOINT: tracesEndpoint,
		OTEL_EXPORTER_OTLP_TRACES_PROTOCOL: "http/json",
		OTEL_TRACES_SAMPLER: "always_on",
		OTEL_BSP_SCHEDULE_DELAY: "10",
	};
}

interface TracedRuntime {
	runtime: DriverDeployOutput;
	client: Client<typeof registry>;
	stop(): Promise<void>;
}

async function startTracedRuntime(
	config: DriverTestConfig,
	tracesEndpoint: string,
	extraEnv: Record<string, string> = {},
): Promise<TracedRuntime> {
	const runtime = await config.start({
		env: { ...tracedEnv(tracesEndpoint), ...extraEnv },
	});
	const client = createClient<typeof registry>({
		endpoint: runtime.endpoint,
		namespace: runtime.namespace,
		poolName: runtime.runnerName,
		encoding: config.encoding,
		disableMetadataLookup: true,
	});
	return {
		runtime,
		client,
		stop: async () => {
			await client.dispose();
			await runtime.cleanup();
		},
	};
}

/** Runs `run` with `rayId` in OpenTelemetry baggage under `rivet.ray.id`. */
function withRayBaggage<T>(rayId: string, run: () => Promise<T>): Promise<T> {
	const baggage = propagation.createBaggage({
		[RAY_BAGGAGE_KEY]: { value: rayId },
	});
	return context.with(propagation.setBaggage(context.active(), baggage), run);
}

function randomTraceId(): string {
	return crypto.randomUUID().replaceAll("-", "");
}

function randomSpanId(): string {
	return randomTraceId().slice(0, 16);
}

describeDriverMatrix(
	"Actor Telemetry",
	(driverTestConfig) => {
		test("keeps actor behavior intact when the trace exporter is unavailable", async () => {
			const unavailable = `http://127.0.0.1:${await getPort({ host: "127.0.0.1" })}/v1/traces`;
			const traced = await startTracedRuntime(
				driverTestConfig,
				unavailable,
			);
			try {
				const handle = traced.client.telemetryActor.getOrCreate([
					`telemetry-down-${crypto.randomUUID()}`,
				]);
				expect(await handle.increment(4)).toBe(4);
			} finally {
				await traced.stop();
			}
		}, 60_000);

		/**
		 * One traced runtime and actor shared by every test that asserts on
		 * exported spans. Each test filters the shared export by the ids it
		 * created, so order between tests does not matter.
		 */
		describe("exported spans", () => {
			let collector: Awaited<ReturnType<typeof startOtlpCollector>>;
			let traced: TracedRuntime;
			let handle: ReturnType<
				Client<typeof registry>["telemetryActor"]["getOrCreate"]
			>;
			let traceExports: Buffer[];

			beforeAll(async () => {
				collector = await startOtlpCollector(
					await getPort({ host: "127.0.0.1" }),
				);
				traceExports = collector.exports();
				traced = await startTracedRuntime(
					driverTestConfig,
					collector.endpoint,
				);
				handle = traced.client.telemetryActor.getOrCreate([
					`telemetry-${crypto.randomUUID()}`,
				]);
				await handle.getCount();
			}, 60_000);

			afterAll(async () => {
				await traced?.stop();
				await collector?.close();
			}, 30_000);

			test("parents a failed SQLite statement under its invocation", async () => {
				await expect(handle.sqliteFailure()).rejects.toMatchObject({
					code: expect.any(String),
				});
				const spans = await waitForSpans(
					traceExports,
					"the sqliteFailure invocation and its failed sqlite span",
					(exported) => {
						const invocation = findInvocation(
							exported,
							"sqliteFailure",
						);
						return (
							invocation !== undefined &&
							exported.some(
								(span) =>
									isSqliteSpan(span) &&
									span.parentSpanId === invocation.spanId,
							)
						);
					},
				);
				const invocation = findInvocation(spans, "sqliteFailure");
				expect(invocation?.statusCode).toBe(OTLP_STATUS_ERROR);
				const failed = spans.find(
					(span) =>
						isSqliteSpan(span) &&
						span.parentSpanId === invocation?.spanId,
				);
				expect(failed?.statusCode).toBe(OTLP_STATUS_ERROR);
				expect(failed?.attributes).toMatchObject({
					"rivet.operation.system": "sqlite",
					"rivet.operation.name": "execute",
				});
			});

			test("parents a state transaction under its invocation", async () => {
				const before = await handle.getCount();
				expect(await handle.stateTransaction(3)).toBe(before + 3);
				const transactionSteps = [
					"rivet.sqlite.transaction.begin",
					"rivet.sqlite.transaction.execute",
					"rivet.sqlite.transaction.commit",
				];
				const spans = await waitForSpans(
					traceExports,
					"the stateTransaction invocation and its transaction spans",
					(exported) => {
						const invocation = findInvocation(
							exported,
							"stateTransaction",
						);
						return (
							invocation !== undefined &&
							transactionSteps.every((name) =>
								exported.some(
									(span) =>
										span.name === name &&
										span.parentSpanId === invocation.spanId,
								),
							)
						);
					},
				);
				const invocation = findInvocation(spans, "stateTransaction");
				expect(invocation?.statusCode).toBe(OTLP_STATUS_OK);
				const commit = spans.find(
					(span) =>
						span.name === "rivet.sqlite.transaction.commit" &&
						span.parentSpanId === invocation?.spanId,
				);
				expect(commit?.statusCode).toBe(OTLP_STATUS_OK);
				expect(commit?.attributes).toMatchObject({
					"rivet.operation.system": "sqlite",
					"rivet.operation.name": "transaction.commit",
				});
			});

			test("keeps the invocation span open for waitUntil work", async () => {
				const token = crypto.randomUUID();
				expect(await handle.insertAfterReply(token)).toBe("replied");
				await handle.send("jobs", { id: token });
				const spans = await waitForSpans(
					traceExports,
					"the insertAfterReply invocation and its deferred sqlite span",
					(exported) => {
						const invocation = findInvocation(
							exported,
							"insertAfterReply",
						);
						return (
							invocation !== undefined &&
							exported.some(
								(span) =>
									isSqliteSpan(span) &&
									span.parentSpanId === invocation.spanId,
							)
						);
					},
				);
				const invocation = findInvocation(spans, "insertAfterReply");
				const deferred = spans.find(
					(span) =>
						isSqliteSpan(span) &&
						span.parentSpanId === invocation?.spanId,
				);
				expect(deferred?.statusCode).toBe(OTLP_STATUS_OK);
				expect(
					invocation !== undefined &&
						deferred !== undefined &&
						invocation.endTimeUnixNano >= deferred.endTimeUnixNano,
				).toBe(true);
			});

			test("carries the caller's ray and trace context into the invocation", async () => {
				const rayId = `caller-${crypto.randomUUID().slice(0, 8)}`;
				const traceId = randomTraceId();
				const spanId = randomSpanId();
				const callerSpan = trace.wrapSpanContext({
					traceId,
					spanId,
					traceFlags: 1,
				});
				await withRayBaggage(rayId, () =>
					context.with(
						trace.setSpan(context.active(), callerSpan),
						() => handle.getCount(),
					),
				);
				const spans = await waitForSpans(
					traceExports,
					"the getCount invocation carrying the caller's ray",
					(exported) =>
						exported.some(
							(span) => span.attributes["rivet.ray.id"] === rayId,
						),
				);
				const invocation = spans.find(
					(span) => span.attributes["rivet.ray.id"] === rayId,
				);
				expect(invocation?.name).toBe("telemetryActor/getCount");
				expect(invocation?.kind).toBe(OTLP_SPAN_KIND_SERVER);
				expect(invocation?.statusCode).toBe(OTLP_STATUS_OK);
				expect(invocation?.attributes).toMatchObject({
					"rivet.invocation.type": "action",
					"rivet.action.name": "getCount",
					"rivet.actor.name": "telemetryActor",
					"rivet.actor.id": await handle.resolve(),
				});
				expect(invocation?.traceId).toBe(traceId);
				expect(invocation?.parentSpanId).toBe(spanId);

				const invalidRayId = "a".repeat(31);
				expect(
					await withRayBaggage(invalidRayId, () =>
						handle.increment(0),
					),
				).toBeTypeOf("number");
				const afterInvalid = await waitForSpans(
					traceExports,
					"the increment invocation sent with an invalid ray",
					(exported) =>
						findInvocation(exported, "increment") !== undefined,
				);
				expect(
					afterInvalid.some(
						(span) =>
							span.attributes["rivet.ray.id"] === invalidRayId,
					),
				).toBe(false);
			});

			test("links a scheduled invocation to the invocation that scheduled it", async () => {
				const token = crypto.randomUUID();
				expect(await handle.scheduleTrace(token)).toBe(token);
				const spans = await waitForSpans(
					traceExports,
					"the scheduleTrace and scheduledTrace invocations",
					(exported) =>
						findInvocation(exported, "scheduleTrace") !==
							undefined &&
						findInvocation(exported, "scheduledTrace") !==
							undefined,
					15_000,
				);
				const definer = findInvocation(spans, "scheduleTrace");
				const scheduled = findInvocation(spans, "scheduledTrace");
				expect(scheduled?.statusCode).toBe(OTLP_STATUS_OK);
				expect(scheduled?.attributes).toMatchObject({
					"rivet.invocation.type": "scheduled",
					"rivet.ray.id": definer?.attributes["rivet.ray.id"],
				});
				expect(scheduled?.traceId).not.toBe(definer?.traceId);
				expect(scheduled?.links).toEqual([
					{ traceId: definer?.traceId, spanId: definer?.spanId },
				]);
			});

			test("traces a raw request under its explicit caller context", async () => {
				const rayId = `explicit-${crypto.randomUUID().slice(0, 8)}`;
				const traceId = randomTraceId();
				const spanId = randomSpanId();
				const activeSpan = trace.wrapSpanContext({
					traceId: randomTraceId(),
					spanId: randomSpanId(),
					traceFlags: 1,
				});
				const response = await context.with(
					trace.setSpan(context.active(), activeSpan),
					() =>
						handle.fetch("hello", {
							headers: {
								"x-rivet-ray-id": rayId,
								traceparent: `00-${traceId}-${spanId}-01`,
							},
						}),
				);
				expect(response.status).toBe(200);
				const isRequest = (span: ExportedSpan) =>
					span.attributes["rivet.invocation.type"] === "request" &&
					span.attributes["rivet.ray.id"] === rayId;
				const spans = await waitForSpans(
					traceExports,
					"the onRequest invocation and its sqlite span",
					(exported) => {
						const request = exported.find(isRequest);
						return (
							request !== undefined &&
							exported.some(
								(span) =>
									isSqliteSpan(span) &&
									span.parentSpanId === request.spanId,
							)
						);
					},
				);
				const request = spans.find(isRequest);
				expect(request?.name).toBe("telemetryActor/onRequest");
				expect(request?.kind).toBe(OTLP_SPAN_KIND_SERVER);
				expect(request?.statusCode).toBe(OTLP_STATUS_OK);
				expect(request?.attributes).toMatchObject({
					"http.request.method": "GET",
					"http.response.status_code": "200",
				});
				expect(request?.traceId).toBe(traceId);
				expect(request?.parentSpanId).toBe(spanId);
			});

			test("links a queue receipt to the send that produced it", async () => {
				const rayId = `queue-${crypto.randomUUID().slice(0, 8)}`;
				const runRayId = `run-${crypto.randomUUID().slice(0, 8)}`;
				const jobId = `job-${crypto.randomUUID()}`;
				const consumer =
					traced.client.telemetryRunConsumerActor.getOrCreate([
						`telemetry-run-consumer-${crypto.randomUUID()}`,
					]);
				await withRayBaggage(rayId, () =>
					handle.send("jobs", { id: jobId }),
				);
				expect(await handle.consumeJob()).toEqual({ id: jobId });
				await withRayBaggage(runRayId, () =>
					consumer.send("runJobs", { id: "job-run" }),
				);

				const sendWithRay = (spans: ExportedSpan[], ray: string) =>
					spans.find(
						(span) =>
							span.attributes["rivet.invocation.type"] ===
								"queue_send" &&
							span.attributes["rivet.ray.id"] === ray,
					);
				const receiptUnder = (
					spans: ExportedSpan[],
					actorName: string,
					parentSpanId: string | undefined,
				) =>
					spans.find(
						(span) =>
							span.name === `${actorName}/queue.receive` &&
							span.parentSpanId === parentSpanId,
					);
				const spans = await waitForSpans(
					traceExports,
					"both queue sends, the consuming action, and both receipts",
					(exported) => {
						const action = findInvocation(exported, "consumeJob");
						return (
							sendWithRay(exported, rayId) !== undefined &&
							sendWithRay(exported, runRayId) !== undefined &&
							action !== undefined &&
							receiptUnder(
								exported,
								"telemetryActor",
								action.spanId,
							) !== undefined &&
							receiptUnder(
								exported,
								"telemetryRunConsumerActor",
								undefined,
							) !== undefined
						);
					},
				);

				const send = sendWithRay(spans, rayId);
				expect(send?.name).toBe("telemetryActor/queue.send");
				expect(send?.attributes).toMatchObject({
					"rivet.invocation.type": "queue_send",
					"rivet.queue.name": "jobs",
				});
				const action = findInvocation(spans, "consumeJob");
				const receipt = receiptUnder(
					spans,
					"telemetryActor",
					action?.spanId,
				);
				expect(receipt?.attributes).toMatchObject({
					"rivet.queue.name": "jobs",
					"rivet.ray.id": action?.attributes["rivet.ray.id"],
				});
				expect(receipt?.links).toEqual([
					{ traceId: send?.traceId, spanId: send?.spanId },
				]);

				const runSend = sendWithRay(spans, runRayId);
				const runReceipt = receiptUnder(
					spans,
					"telemetryRunConsumerActor",
					undefined,
				);
				expect(runReceipt?.attributes).toMatchObject({
					"rivet.queue.name": "runJobs",
					"rivet.ray.id": runRayId,
				});
				expect(runReceipt?.links).toEqual([
					{ traceId: runSend?.traceId, spanId: runSend?.spanId },
				]);
			});
		});
	},
	{
		runtimes: ["native"],
		sqliteBackends: ["local"],
		encodings: ["bare"],
	},
);
