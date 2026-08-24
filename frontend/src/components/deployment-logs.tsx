import type { Rivet } from "@rivet-gg/cloud";
import {
	faArrowDown,
	faCopy,
	faDownload,
	faTriangleExclamation,
	Icon,
} from "@rivet-gg/icons";
import { useCallback, useEffect } from "react";
import { ErrorDetails } from "@/components/actors";
import { VirtualScrollArea } from "@/components/virtual-scroll-area";
import { AnsiText } from "./lib/ansi";
import { cn } from "./lib/utils";
import { Button } from "./ui/button";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import { ScrollArea } from "./ui/scroll-area";
import { Skeleton } from "./ui/skeleton";
import { useDeploymentLogsStream } from "./use-deployment-logs-stream";
import { useLogScroll } from "./use-log-scroll";

const SKELETON_KEYS = Array(40)
	.fill("")
	.map((_, i) => i.toString(36));

interface DeploymentLogsProps {
	project?: string;
	namespace?: string;
	pool: string;
	filter?: string;
	region?: string;
	paused?: boolean;
	logsRef?: React.MutableRefObject<Rivet.LogStreamEvent.Log[]>;
	regionLabelLength?: number;
}

interface LogRowData {
	className?: string;
	entry?: Rivet.LogStreamEvent.Log;
	isSentinel?: boolean;
	isLoadingMore?: boolean;
	onLoadMore?: () => void;
	regionColumnWidth?: string;
}

function LogRow({
	entry,
	isSentinel,
	isLoadingMore,
	onLoadMore,
	regionColumnWidth,
	...props
}: LogRowData) {
	if (isSentinel) {
		// A clickable row rather than a passive hint: scrolling up auto-triggers
		// a load, but when the list is short or a page returned no matching rows
		// there is nothing to scroll, so the button is the reliable trigger.
		return (
			<button
				type="button"
				{...props}
				onClick={onLoadMore}
				disabled={isLoadingMore}
				className={cn(
					"w-full text-left px-4 py-1 border-b italic text-muted-foreground/60 hover:text-muted-foreground disabled:hover:text-muted-foreground/60 disabled:cursor-default",
					props.className,
				)}
			>
				{isLoadingMore ? "Loading older logs…" : "Load older logs"}
			</button>
		);
	}

	if (!entry) return null;

	return (
		<div
			{...props}
			className={cn("font-mono grid grid-cols-subgrid", props.className)}
		>
			<div
				className={cn(
					"grid gap-3 whitespace-pre-wrap break-words px-4 py-1 border-b",
					{
						"text-red-400": entry.data.severity === "error",
						"text-muted-foreground":
							entry.data.severity !== "error",
					},
				)}
				style={{
					gridTemplateColumns: `max-content ${regionColumnWidth ?? "16ch"} 3fr`,
				}}
			>
				<span className="text-neutral-500 shrink-0 select-none">
					{entry.data.timestamp}
				</span>
				{entry.data.region ? (
					<span className="text-neutral-600 shrink-0 select-none">
						[{entry.data.region}]
					</span>
				) : (
					<span />
				)}
				<span className="flex-1">
					<AnsiText text={entry.data.message} />
				</span>
			</div>
		</div>
	);
}

interface DeploymentLogsExportMenuProps {
	/** Ref filled by `DeploymentLogs` via its `logsRef` prop. */
	logsRef: React.MutableRefObject<Rivet.LogStreamEvent.Log[]>;
	/** Download filename, e.g. `deployment-logs-my-namespace.txt`. */
	filename: string;
	className?: string;
}

/**
 * Export dropdown (download / copy) for the log entries currently loaded by a
 * `DeploymentLogs` instance. Reads through `logsRef` so opening the menu never
 * re-renders with the log stream.
 */
export function DeploymentLogsExportMenu({
	logsRef,
	filename,
	className,
}: DeploymentLogsExportMenuProps) {
	const getLogsText = useCallback(
		() =>
			logsRef.current
				.map((e) =>
					[
						e.data.timestamp,
						e.data.region ?? "",
						e.data.message,
					].join("\t"),
				)
				.join("\n"),
		[logsRef],
	);

	const handleDownload = useCallback(() => {
		const blob = new Blob([getLogsText()], { type: "text/plain" });
		const url = URL.createObjectURL(blob);
		const a = document.createElement("a");
		a.href = url;
		a.download = filename;
		a.click();
		URL.revokeObjectURL(url);
	}, [getLogsText, filename]);

	const handleCopy = useCallback(() => {
		navigator.clipboard.writeText(getLogsText());
	}, [getLogsText]);

	return (
		<DropdownMenu>
			<DropdownMenuTrigger asChild>
				<Button variant="ghost" size="sm" className={className}>
					Export
				</Button>
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end">
				<DropdownMenuItem
					indicator={<Icon icon={faDownload} />}
					onClick={handleDownload}
				>
					Download
				</DropdownMenuItem>
				<DropdownMenuItem
					indicator={<Icon icon={faCopy} />}
					onClick={handleCopy}
				>
					Copy
				</DropdownMenuItem>
			</DropdownMenuContent>
		</DropdownMenu>
	);
}

export function DeploymentLogs({
	project,
	namespace,
	pool,
	filter,
	region,
	paused,
	logsRef,
	regionLabelLength,
}: DeploymentLogsProps) {
	// Region label gets brackets (2 chars) plus a small buffer so the column
	// has visual breathing room and tolerates glyph-width differences from
	// `ch` (measured from "0").
	const regionColumnWidth =
		regionLabelLength && regionLabelLength > 0
			? `${regionLabelLength + 4}ch`
			: "18ch";

	const {
		logs,
		isLoading,
		error,
		streamError,
		isLoadingMore,
		hasMore,
		oldestScannedTs,
		loadMoreHistory,
	} = useDeploymentLogsStream({
		project: project ?? "",
		namespace: namespace ?? "",
		pool,
		filter,
		region,
		paused,
	});

	const {
		displayedLogs,
		follow,
		setFollow,
		viewportRef,
		virtualizerRef,
		triggerLoadMore,
		handleScrollChange,
		totalCount,
		sentinelOffset,
	} = useLogScroll({
		logs,
		hasMore,
		isLoading,
		isLoadingMore,
		loadMoreHistory,
	});

	useEffect(() => {
		if (logsRef) {
			logsRef.current = logs;
		}
	}, [logs, logsRef]);

	if (isLoading) {
		return (
			<div className="h-full flex flex-col ">
				<ScrollArea
					className="w-full h-full"
					viewportProps={{ className: "p-2" }}
				>
					{SKELETON_KEYS.map((key) => (
						<Skeleton
							key={key}
							className="w-full h-6 mb-2 last:mb-0"
						/>
					))}
				</ScrollArea>
			</div>
		);
	}

	if (logs.length === 0) {
		if (error) {
			return (
				<div className="h-full flex-1 flex items-center justify-center">
					<div className="max-w-md flex flex-col items-center justify-center flex-1">
						<Icon
							icon={faTriangleExclamation}
							className="text-red-500 mb-2 text-2xl"
						/>
						<div className="text-center">
							<div className="mb-1">Failed to load logs.</div>
							<ErrorDetails error={error} className="text-sm" />
						</div>
					</div>
				</div>
			);
		}
		return (
			<div className="h-full flex flex-1 flex-col items-center justify-center gap-3">
				<div className="text-center">
					<p>
						{hasMore
							? "No logs available."
							: "No logs found."}
					</p>
					<p className="text-muted-foreground text-xs mt-1">
						{hasMore
							? "Nothing matches this view in recent history. Older logs may exist."
							: "Logs will appear here as they stream in."}
					</p>
					{hasMore && oldestScannedTs ? (
						<p className="text-muted-foreground text-xs mt-1 font-mono">
							Searched back to {oldestScannedTs}
						</p>
					) : null}
				</div>
				{hasMore ? (
					// The viewport is empty and cannot be scrolled, so scroll-to-top
					// can't trigger a load. Offer an explicit way to page backward
					// through older raw history.
					<Button
						variant="outline"
						size="sm"
						startIcon={<Icon icon={faArrowDown} className="rotate-180" />}
						isLoading={isLoadingMore}
						onClick={() => loadMoreHistory()}
					>
						Load older logs
					</Button>
				) : null}
			</div>
		);
	}

	return (
		<div className="h-full font-mono text-xs text-neutral-100 overflow-hidden flex flex-col">
			{streamError ? (
				<div className="flex items-center gap-2 px-4 py-2 bg-destructive/20 text-destructive-foreground text-xs border-b border-destructive/40 shrink-0">
					<Icon icon={faTriangleExclamation} className="shrink-0" />
					<span>Stream error: {streamError}</span>
				</div>
			) : null}
			<div className="relative flex-1 min-h-0">
				<VirtualScrollArea<LogRowData>
					virtualizerRef={virtualizerRef}
					viewportRef={viewportRef}
					onChange={handleScrollChange}
					count={totalCount}
					estimateSize={() => 24}
					// The default hover-only scrollbar gives no visual hint that
					// the log view scrolls at all. Show it whenever logs overflow.
					type="auto"
					className="w-full h-full"
					scrollerProps={{
						className: "w-full",
					}}
					viewportProps={{}}
					getRowData={(index) => {
						if (hasMore && index === 0) {
							return {
								isSentinel: true,
								isLoadingMore,
								onLoadMore: triggerLoadMore,
							};
						}
						return {
							entry: displayedLogs[index - sentinelOffset],
							regionColumnWidth,
						};
					}}
					row={LogRow}
				/>
				{!follow ? (
					<div className="absolute bottom-4 left-1/2 -translate-x-1/2 z-10">
						<button
							type="button"
							className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-primary text-primary-foreground text-xs font-sans font-medium shadow-lg hover:bg-primary/90 transition-colors"
							onClick={() => {
								setFollow(true);
								virtualizerRef.current?.scrollToIndex(
									totalCount - 1,
									{ align: "end" },
								);
							}}
						>
							<Icon icon={faArrowDown} className="size-3" />
							Back to newest
						</button>
					</div>
				) : null}
			</div>
		</div>
	);
}
