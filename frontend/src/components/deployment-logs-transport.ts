import type { Rivet } from "@rivet-gg/cloud";
import { cloudEnv } from "@/lib/env";

const MAX_RETRIES = 8;
const BASE_DELAY_MS = 1_000;

// Cloud Run emits this on every container cold-start, prefixed with its own
// timestamp on stderr. When the user hasn't pushed an image yet, surface a
// Rivet-flavored hint instead of leaky provider detail. Matched by suffix
// (after Cloud Run's leading "YYYY/MM/DD HH:MM:SS " prefix) and gated on
// `stream === "stderr"` so we don't grab a user-emitted line that happens to
// contain the same text.
const CLOUD_RUN_HELLO_SUFFIX =
	"Hello from Cloud Run! The container started successfully and is listening for HTTP requests on port 8080";
const RIVET_COMPUTE_HELLO =
	"Hello from Rivet Compute! Waiting for you to deploy an image. See rivet.dev/docs/connect/rivet-compute to learn more.";

export function rewriteLogEntry<T extends { message: string; stream?: string }>(
	data: T,
): T {
	if (
		data.stream === "stderr" &&
		data.message.endsWith(CLOUD_RUN_HELLO_SUFFIX)
	) {
		return { ...data, message: RIVET_COMPUTE_HELLO };
	}
	return data;
}

function parseSseEvent(raw: string): Rivet.LogStreamEvent | null {
	let eventType = "message";
	let data = "";
	for (const line of raw.split("\n")) {
		if (line.startsWith("event:")) eventType = line.slice(6).trim();
		else if (line.startsWith("data:")) data = line.slice(5).trim();
	}
	if (!data) return null;
	try {
		const parsed = JSON.parse(data);
		if (eventType === "log") return { event: "log", data: parsed };
		if (eventType === "error") return { event: "error", data: parsed };
		if (eventType === "end") return { event: "end", data: parsed };
		if (eventType === "connected")
			return { event: "connected", data: parsed };
	} catch {
		// ignore malformed events
	}
	return null;
}

async function* streamLogsWithCredentials(
	baseUrl: string,
	project: string,
	namespace: string,
	pool: string,
	request: {
		region?: string;
		contains?: string;
		abortSignal?: AbortSignal;
	},
): AsyncGenerator<Rivet.LogStreamEvent> {
	const params = new URLSearchParams();
	if (request.region) params.set("region", request.region);
	if (request.contains) params.set("contains", request.contains);
	const qs = params.toString();
	const url = `${baseUrl}/projects/${encodeURIComponent(project)}/namespaces/${encodeURIComponent(namespace)}/managed-pools/${encodeURIComponent(pool)}/logs${qs ? `?${qs}` : ""}`;

	const response = await fetch(url, {
		method: "GET",
		headers: {
			Accept: "text/event-stream",
			"Cache-Control": "no-cache",
		},
		credentials: "include",
		signal: request.abortSignal,
	});

	if (!response.ok) {
		const body = await response.text();
		throw new Error(
			`streamLogs request failed with status ${response.status}: ${body}`,
		);
	}
	if (!response.body) throw new Error("streamLogs: response body is null");

	const reader = response.body.getReader();
	const decoder = new TextDecoder();
	let buffer = "";
	try {
		while (true) {
			const { done, value } = await reader.read();
			if (done) break;
			buffer += decoder.decode(value, { stream: true });
			const parts = buffer.split("\n\n");
			buffer = parts.pop() ?? "";
			for (const part of parts) {
				const event = parseSseEvent(part);
				if (event != null) {
					yield event;
					if (event.event === "end") return;
				}
			}
		}
		if (buffer.trim()) {
			const event = parseSseEvent(buffer);
			if (event != null) yield event;
		}
	} finally {
		reader.releaseLock();
	}
}

async function sleep(ms: number, signal: AbortSignal) {
	return new Promise<void>((resolve) => {
		const timeout = setTimeout(resolve, ms);
		signal.addEventListener(
			"abort",
			() => {
				clearTimeout(timeout);
				resolve();
			},
			{ once: true },
		);
	});
}

export async function streamWithRetry(
	project: string,
	namespace: string,
	pool: string,
	filter: string | undefined,
	region: string | undefined,
	signal: AbortSignal,
	onConnected: () => void,
	onEntry: (entry: Rivet.LogStreamEvent.Log) => void,
): Promise<"exhausted" | "ended" | "aborted" | { error: string }> {
	for (let attempt = 0; attempt <= MAX_RETRIES; attempt++) {
		if (signal.aborted) return "aborted";

		try {
			const stream = streamLogsWithCredentials(
				cloudEnv().VITE_APP_CLOUD_API_URL,
				project,
				namespace,
				pool,
				{
					contains: filter || undefined,
					region: region || undefined,
					abortSignal: signal,
				},
			);

			for await (const event of stream) {
				if (event.event === "connected") {
					onConnected();
				} else if (event.event === "end") {
					return "ended";
				} else if (event.event === "error") {
					return { error: event.data.message };
				} else if (event.event === "log") {
					onEntry({ ...event, data: rewriteLogEntry(event.data) });
				}
			}
		} catch (err) {
			if ((err as Error).name === "AbortError") return "aborted";
			console.error(`Log stream error (attempt ${attempt + 1}):`, err);
		}

		if (attempt < MAX_RETRIES) {
			await sleep(BASE_DELAY_MS * 2 ** attempt, signal);
		}
	}

	return "exhausted";
}
