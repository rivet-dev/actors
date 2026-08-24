import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";

export type Delivery = {
	route: string;
	queueName: string | null;
	messageId: string | null;
	attempt: number;
	body: any;
};

export type MockApp = {
	url: string;
	deliveries: Delivery[];
	/** runId -> remaining forced 503 failures before success. */
	failuresBeforeSuccess: Map<string, number>;
	/** runIds that always respond 503. */
	alwaysFail: Set<string>;
	/** runId -> { timeoutSeconds } body to return on next success (long-step). */
	timeoutResponse: Map<string, number>;
	/** runIds whose success body is malformed/non-JSON. */
	malformedSuccess: Set<string>;
	/** Artificial delay (ms) before responding, to widen concurrency windows. */
	responseDelayMs: number;
	/** Peak number of simultaneously in-flight deliveries observed. */
	maxConcurrent: number;
	count(runId: string): number;
	close(): Promise<void>;
};

function runIdOf(body: unknown): string | null {
	if (body && typeof body === "object" && "runId" in body) {
		return String((body as { runId: unknown }).runId);
	}
	return null;
}

/**
 * Stands up a stand-in for the app's `.well-known/workflow/v1/{flow,step}`
 * runtime endpoints with scriptable failure / long-step / malformed behavior so
 * dispatcher-loop tests are deterministic. Mirrors the integration mock but is
 * reusable from native (`setupTest`) actor tests.
 */
export async function startMockApp(): Promise<MockApp> {
	const deliveries: Delivery[] = [];
	const failuresBeforeSuccess = new Map<string, number>();
	const alwaysFail = new Set<string>();
	const timeoutResponse = new Map<string, number>();
	const malformedSuccess = new Set<string>();
	const state = { responseDelayMs: 0, maxConcurrent: 0, inFlight: 0 };

	const server = createServer(async (req, res) => {
		const path = req.url ?? "";
		if (req.method === "GET" && path.includes("__health")) {
			res.writeHead(200, { "content-type": "application/json" });
			res.end(JSON.stringify({ healthy: true }));
			return;
		}
		if (req.method !== "POST" || !path.startsWith("/.well-known/workflow/v1/")) {
			res.writeHead(404);
			res.end("not found");
			return;
		}
		state.inFlight++;
		state.maxConcurrent = Math.max(state.maxConcurrent, state.inFlight);
		const finish = (status: number, payload: string) => {
			const send = () => {
				state.inFlight--;
				res.writeHead(status, { "content-type": "application/json" });
				res.end(payload);
			};
			if (state.responseDelayMs > 0) setTimeout(send, state.responseDelayMs);
			else send();
		};
		const chunks: Buffer[] = [];
		for await (const chunk of req) chunks.push(Buffer.from(chunk));
		const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
		deliveries.push({
			route: path.split("/").at(-1)?.split("?")[0] ?? "",
			queueName: req.headers["x-vqs-queue-name"]
				? String(req.headers["x-vqs-queue-name"])
				: null,
			messageId: req.headers["x-vqs-message-id"]
				? String(req.headers["x-vqs-message-id"])
				: null,
			attempt: Number(req.headers["x-vqs-message-attempt"]),
			body,
		});
		const runId = runIdOf(body);
		if (runId && alwaysFail.has(runId)) {
			finish(503, JSON.stringify({ error: "forced failure" }));
			return;
		}
		if (runId) {
			const remaining = failuresBeforeSuccess.get(runId) ?? 0;
			if (remaining > 0) {
				failuresBeforeSuccess.set(runId, remaining - 1);
				finish(503, JSON.stringify({ error: "forced retry" }));
				return;
			}
		}
		if (runId && timeoutResponse.has(runId)) {
			const seconds = timeoutResponse.get(runId)!;
			timeoutResponse.delete(runId);
			finish(200, JSON.stringify({ timeoutSeconds: seconds }));
			return;
		}
		if (runId && malformedSuccess.has(runId)) {
			finish(200, JSON.stringify({ timeoutSeconds: "not-a-number" }));
			return;
		}
		finish(200, JSON.stringify({ ok: true }));
	});

	await new Promise<void>((resolve) =>
		server.listen(0, "127.0.0.1", () => resolve()),
	);
	const port = (server.address() as AddressInfo).port;

	return {
		url: `http://127.0.0.1:${port}`,
		deliveries,
		failuresBeforeSuccess,
		alwaysFail,
		timeoutResponse,
		malformedSuccess,
		get responseDelayMs() {
			return state.responseDelayMs;
		},
		set responseDelayMs(value: number) {
			state.responseDelayMs = value;
		},
		get maxConcurrent() {
			return state.maxConcurrent;
		},
		count: (runId: string) =>
			deliveries.filter((d) => runIdOf(d.body) === runId).length,
		close: () =>
			new Promise<void>((resolve, reject) =>
				(server as Server).close((err) => (err ? reject(err) : resolve())),
			),
	};
}

export function sleep(ms: number) {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

export async function waitFor(
	label: string,
	predicate: () => boolean | Promise<boolean>,
	timeoutMs = 5_000,
	intervalMs = 25,
) {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		if (await predicate()) return;
		await sleep(intervalMs);
	}
	throw new Error(`Timed out waiting for ${label}`);
}
