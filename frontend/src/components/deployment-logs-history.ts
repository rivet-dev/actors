import type { Rivet } from "@rivet-gg/cloud";
import { rewriteLogEntry } from "./deployment-logs-transport";

export const HISTORY_PAGE_SIZE = 100;
export const INITIAL_HISTORY_SIZE = 50;

export async function fetchLogsHistory(
	baseUrl: string,
	project: string,
	namespace: string,
	pool: string,
	params: {
		before?: string;
		limit?: number;
		region?: string;
		contains?: string;
		signal?: AbortSignal;
	},
): Promise<Rivet.LogHistoryPaginatedResponse> {
	const qs = new URLSearchParams();
	if (params.before) qs.set("before", params.before);
	if (params.limit) qs.set("limit", String(params.limit));
	if (params.region) qs.set("region", params.region);
	if (params.contains) qs.set("contains", params.contains);
	const query = qs.toString();
	const url = `${baseUrl}/projects/${encodeURIComponent(project)}/namespaces/${encodeURIComponent(namespace)}/managed-pools/${encodeURIComponent(pool)}/logs/history-paginated${query ? `?${query}` : ""}`;

	const response = await fetch(url, {
		method: "GET",
		headers: { Accept: "application/json" },
		credentials: "include",
		signal: params.signal,
	});

	if (!response.ok) {
		const body = await response.text();
		throw new Error(
			`fetchLogsHistory failed with status ${response.status}: ${body}`,
		);
	}

	return response.json();
}

export function historyToLogEvent(
	item: Rivet.LogHistoryPaginatedResponse.Entries.Item,
): Rivet.LogStreamEvent.Log {
	return { event: "log", data: rewriteLogEntry(item) };
}
