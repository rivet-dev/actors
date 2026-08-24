import type { Rivet } from "@rivet-gg/cloud";
import type { Virtualizer } from "@tanstack/react-virtual";
import {
	useCallback,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";

interface UseLogScrollOptions {
	logs: Rivet.LogStreamEvent.Log[];
	hasMore: boolean;
	isLoading: boolean;
	isLoadingMore: boolean;
	loadMoreHistory: () => void;
}

/**
 * Owns the log viewport's follow-newest behavior, the freeze-while-not-following
 * rule, and scroll-position anchoring when older history is prepended. Returns
 * everything `DeploymentLogs` needs to wire the virtualized list.
 */
export function useLogScroll({
	logs,
	hasMore,
	isLoading,
	isLoadingMore,
	loadMoreHistory,
}: UseLogScrollOptions) {
	const viewportRef = useRef<HTMLDivElement>(null);
	const virtualizerRef = useRef<Virtualizer<HTMLDivElement, Element>>(null);
	const [follow, setFollow] = useState(true);
	// Track the log count before a load-more so we can restore scroll position.
	const prevLogCountRef = useRef(0);

	// Freeze displayed logs when not following so appended entries don't shift scroll.
	// Always update when following, and also when history is prepended (logs grew
	// from the front, detectable because the previously-first entry moved).
	const frozenLogsRef = useRef(logs);
	const frozenFirstIdRef = useRef<string | undefined>(undefined);
	if (follow) {
		frozenLogsRef.current = logs;
		frozenFirstIdRef.current = logs[0]?.data.insertId;
	} else if (
		logs.length > 0 &&
		logs[0]?.data.insertId !== frozenFirstIdRef.current
	) {
		// First entry changed — history was prepended. Accept the update.
		frozenLogsRef.current = logs;
		frozenFirstIdRef.current = logs[0]?.data.insertId;
	}
	const displayedLogs = follow ? logs : frozenLogsRef.current;

	// When hasMore, index 0 is the sentinel row; real logs start at index 1.
	const sentinelOffset = hasMore ? 1 : 0;
	const totalCount = displayedLogs.length + sentinelOffset;

	useEffect(() => {
		if (
			follow &&
			!isLoading &&
			virtualizerRef.current &&
			displayedLogs.length > 0
		) {
			// https://github.com/TanStack/virtual/issues/537
			const rafId = requestAnimationFrame(() => {
				virtualizerRef.current?.scrollToIndex(totalCount - 1, {
					align: "end",
				});
			});
			return () => cancelAnimationFrame(rafId);
		}
	}, [totalCount, displayedLogs.length, follow, isLoading]);

	// After prepending older history, keep the viewport anchored to the same
	// content by growing scrollTop by the height added above the fold. Measuring
	// the real scroll element (rather than the virtualizer's estimated total size)
	// and applying the correction in a layout effect before paint keeps the scroll
	// position exact with no visible jump.
	const pendingRestoreRef = useRef(false);
	const prevScrollHeightRef = useRef(0);
	const prevScrollTopRef = useRef(0);
	useLayoutEffect(() => {
		if (
			!pendingRestoreRef.current ||
			displayedLogs.length <= prevLogCountRef.current
		) {
			return;
		}
		pendingRestoreRef.current = false;
		const viewport = viewportRef.current;
		if (!viewport) return;
		const addedHeight = viewport.scrollHeight - prevScrollHeightRef.current;
		viewport.scrollTop = prevScrollTopRef.current + addedHeight;
	}, [displayedLogs.length]);

	// Stop following newest and capture the scroll geometry so the layout effect
	// can anchor the viewport after older history is prepended. Shared by the
	// scroll-to-top auto-trigger and the sentinel button click.
	const triggerLoadMore = useCallback(() => {
		if (!hasMore || isLoadingMore) return;
		setFollow(false);
		prevLogCountRef.current = displayedLogs.length;
		const viewport = viewportRef.current;
		prevScrollHeightRef.current = viewport?.scrollHeight ?? 0;
		prevScrollTopRef.current = viewport?.scrollTop ?? 0;
		pendingRestoreRef.current = true;
		loadMoreHistory();
	}, [hasMore, isLoadingMore, displayedLogs.length, loadMoreHistory]);

	const handleScrollChange = useCallback(
		(instance: Virtualizer<HTMLDivElement, Element>) => {
			const isAtBottom = (instance.range?.endIndex ?? 0) >= totalCount - 1;
			if (isAtBottom) {
				return setFollow(true);
			}
			if (instance.scrollDirection === "backward") {
				setFollow(false);
				// Load more when the sentinel row comes into view.
				if ((instance.range?.startIndex ?? 1) === 0) {
					triggerLoadMore();
				}
			}
		},
		[totalCount, triggerLoadMore],
	);

	return {
		displayedLogs,
		follow,
		setFollow,
		viewportRef,
		virtualizerRef,
		triggerLoadMore,
		handleScrollChange,
		totalCount,
		sentinelOffset,
	};
}
