import type { Rivet } from "@rivet-gg/cloud";
import * as Sentry from "@sentry/react";
import {
	startTransition,
	useCallback,
	useEffect,
	useRef,
	useState,
} from "react";
import { cloudEnv } from "@/lib/env";
import {
	fetchLogsHistory,
	HISTORY_PAGE_SIZE,
	historyToLogEvent,
	INITIAL_HISTORY_SIZE,
} from "./deployment-logs-history";
import { streamWithRetry } from "./deployment-logs-transport";

interface UseDeploymentLogsStreamOptions {
	project: string;
	namespace: string;
	pool: string;
	filter?: string;
	region?: string;
	paused?: boolean;
}

export function useDeploymentLogsStream({
	project,
	namespace,
	pool,
	filter,
	region,
	paused = false,
}: UseDeploymentLogsStreamOptions) {
	const [logs, setLogs] = useState<Rivet.LogStreamEvent.Log[]>([]);
	const [isLoading, setIsLoading] = useState(true);
	const [isLoadingMore, setIsLoadingMore] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [hasMore, setHasMore] = useState(true);
	// Oldest raw timestamp scanned so far (the boundary the next page fetches
	// before). Surfaced for the empty state so the user can see how far back the
	// search has reached when nothing matches the filter yet.
	const [oldestScannedTs, setOldestScannedTs] = useState<string | undefined>(
		undefined,
	);
	const pendingRef = useRef<Rivet.LogStreamEvent.Log[]>([]);
	const pausedRef = useRef(paused);
	// Dedupes entries that appear in both the initial history fetch and
	// the live stream (and across "load more" page boundaries that share
	// a timestamp). `insertId` is the only unique identifier the API
	// exposes per log entry.
	const seenInsertIdsRef = useRef<Set<string>>(new Set());
	// Cursor for the next older history page. The API returns this per page as the
	// oldest raw timestamp it scanned, so paging back works even across windows that
	// contain no entries matching the current filter.
	const nextCursorRef = useRef<string | undefined>(undefined);
	// The current generation's abort controller, owned by the seeding effect and
	// aborted when the filter/region/pool/before changes or the hook unmounts.
	// `loadMoreHistory` reads it so an in-flight older-page fetch is cancelled and
	// its result dropped instead of writing into the reset store.
	const requestAbortRef = useRef<AbortController | null>(null);

	// Advance the history cursor. The ref is read synchronously by load-more; the
	// state mirror drives the empty-state "searched back to" display. Writing both
	// here keeps them from drifting.
	const setCursor = useCallback((next: string | undefined) => {
		nextCursorRef.current = next;
		setOldestScannedTs(next);
	}, []);

	// Move buffered live entries into the visible list. Shared by the stream
	// callback and the unpause effect.
	const flushPending = useCallback(() => {
		if (pendingRef.current.length === 0) return;
		const toFlush = pendingRef.current;
		pendingRef.current = [];
		startTransition(() => {
			setLogs((prev) => [...prev, ...toFlush]);
		});
	}, []);

	// Clear all per-session state before a fresh seed (filter/region/pool/before
	// change or first mount).
	const resetSession = useCallback(() => {
		setLogs([]);
		setIsLoading(true);
		setError(null);
		setHasMore(true);
		setIsLoadingMore(false);
		setCursor(undefined);
		pendingRef.current = [];
		seenInsertIdsRef.current = new Set();
	}, [setCursor]);

	useEffect(() => {
		pausedRef.current = paused;
	}, [paused]);

	useEffect(() => {
		resetSession();

		const controller = new AbortController();
		requestAbortRef.current = controller;

		function onEntry(entry: Rivet.LogStreamEvent.Log) {
			setIsLoading(false);
			const insertId = entry.data.insertId;
			if (seenInsertIdsRef.current.has(insertId)) return;
			seenInsertIdsRef.current.add(insertId);
			pendingRef.current.push(entry);
			if (!pausedRef.current) flushPending();
		}

		async function start() {
			// Seed the view with recent historical logs so it isn't empty on load.
			try {
				const initial = await fetchLogsHistory(
					cloudEnv().VITE_APP_CLOUD_API_URL,
					project,
					namespace,
					pool,
					{
						limit: INITIAL_HISTORY_SIZE,
						region: region || undefined,
						contains: filter || undefined,
						signal: controller.signal,
					},
				);
				if (controller.signal.aborted) return;

				setCursor(initial.nextCursor);
				setHasMore(initial.hasMore);
				if (initial.entries.length > 0) {
					const converted: Rivet.LogStreamEvent.Log[] = [];
					for (const item of initial.entries) {
						seenInsertIdsRef.current.add(item.insertId);
						converted.push(historyToLogEvent(item));
					}
					startTransition(() => {
						setLogs(converted);
					});
				}
			} catch (err) {
				// The fetch was cancelled by the effect cleanup (e.g. filter
				// change or unmount). Not an error worth surfacing.
				if ((err as Error).name === "AbortError") return;
				// Non-fatal. The stream will still start.
				console.warn("Failed to fetch initial log history:", err);
				Sentry.captureException(err, {
					tags: { source: "deployment-logs-initial-history" },
					contexts: {
						logs: { project, namespace, pool, region, filter },
					},
				});
			}

			if (controller.signal.aborted) return;
			setIsLoading(false);

			const result = await streamWithRetry(
				project,
				namespace,
				pool,
				filter,
				region,
				controller.signal,
				() => setIsLoading(false),
				onEntry,
			);

			setIsLoading(false);
			if (result === "exhausted") {
				setError(
					"Failed to connect to log stream after multiple attempts.",
				);
			} else if (typeof result === "object") {
				setError(result.error);
			}
		}

		start().catch((err) => {
			if ((err as Error).name !== "AbortError") {
				console.error("Log stream fatal error:", err);
				setIsLoading(false);
				setError("An unexpected error occurred while streaming logs.");
			}
		});

		return () => controller.abort();
	}, [
		project,
		namespace,
		pool,
		filter,
		region,
		resetSession,
		flushPending,
		setCursor,
	]);

	useEffect(() => {
		if (!paused) flushPending();
	}, [paused, flushPending]);

	const loadMoreHistory = useCallback(async () => {
		if (isLoadingMore || !hasMore) return;
		// Fall back to now when we have no cursor yet, e.g. the initial history
		// fetch failed. `hasMore` stays true until a successful page says otherwise,
		// so a retry still issues a request instead of silently disabling paging.
		const before = nextCursorRef.current ?? new Date().toISOString();
		// Share the seeding effect's controller so a filter/region/pool/before
		// change cancels this fetch and its result is discarded.
		const signal = requestAbortRef.current?.signal;
		setIsLoadingMore(true);
		try {
			const page = await fetchLogsHistory(
				cloudEnv().VITE_APP_CLOUD_API_URL,
				project,
				namespace,
				pool,
				{
					before,
					limit: HISTORY_PAGE_SIZE,
					region: region || undefined,
					contains: filter || undefined,
					signal,
				},
			);
			// The fetch may have resolved just before the filter changed and aborted.
			// Bail before touching the store, which now belongs to a new generation.
			if (signal?.aborted) return;
			setCursor(page.nextCursor);
			setHasMore(page.hasMore);

			// Multiple entries can share a timestamp, so `seenInsertIdsRef` filters
			// out any overlap at the page boundary.
			const fresh = page.entries.filter(
				(item) => !seenInsertIdsRef.current.has(item.insertId),
			);
			for (const item of fresh) {
				seenInsertIdsRef.current.add(item.insertId);
			}
			if (fresh.length > 0) {
				const converted = fresh.map(historyToLogEvent);
				startTransition(() => {
					setLogs((prev) => [...converted, ...prev]);
				});
			}
		} catch (err) {
			// Cancelled by a filter change or unmount; not worth surfacing.
			if ((err as Error).name === "AbortError") return;
			console.error("Failed to load historical logs:", err);
		} finally {
			setIsLoadingMore(false);
		}
	}, [
		isLoadingMore,
		hasMore,
		project,
		namespace,
		pool,
		filter,
		region,
		setCursor,
	]);

	return {
		logs,
		isLoading,
		error,
		streamError: null,
		isLoadingMore,
		hasMore,
		oldestScannedTs,
		loadMoreHistory,
	};
}
