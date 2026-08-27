import {
	faCheck,
	faCircleExclamation,
	faCopy,
	faPenToSquare,
	faTriangleExclamation,
	Icon,
} from "@rivet-gg/icons";
import type { ReactNode } from "react";
import { useMemo, useState } from "react";
import { match, P } from "ts-pattern";
import {
	Badge,
	Button,
	Code,
	CopyTrigger,
	cn,
	Popover,
	PopoverContent,
	PopoverTrigger,
	ScrollArea,
	Tabs,
	TabsContent,
	TabsList,
	TabsTrigger,
	Tooltip,
	TooltipContent,
	TooltipPortal,
	TooltipTrigger,
} from "@/components";
import { CodePreview } from "@/components/code-preview/code-preview";
import {
	getRegionKey,
	REGION_LABEL,
	RegionIcon,
} from "@/components/matchmaker/lobby-region";
import type { RivetActorError } from "@/queries/types";

type Severity = "error" | "warning";

interface ClassifiedError {
	severity: Severity;
	kind:
		| "serverless_http"
		| "serverless_connection"
		| "serverless_destination_blocked"
		| "serverless_invalid_sse"
		| "serverless_stream_ended_early"
		| "downgrade"
		| "internal"
		| "unknown";
	title: string;
	statusCode?: number;
	body?: string;
	fingerprint: string;
}

function classifyRunnerError(error: RivetActorError): ClassifiedError {
	return match(error)
		.returnType<ClassifiedError>()
		.with(P.string, (s) =>
			match(s)
				.returnType<ClassifiedError>()
				.with("downgrade", () => ({
					severity: "warning",
					kind: "downgrade",
					title: "Runner pool downgraded",
					fingerprint: "downgrade",
				}))
				.with("serverless_stream_ended_early", () => ({
					severity: "warning",
					kind: "serverless_stream_ended_early",
					title: "Connection terminated early",
					fingerprint: "stream_ended_early",
				}))
				.with("internal_error", () => ({
					severity: "error",
					kind: "internal",
					title: "Internal runner pool error",
					fingerprint: "internal",
				}))
				.otherwise((other) => ({
					severity: "error",
					kind: "unknown",
					title: other,
					fingerprint: `string:${other}`,
				})),
		)
		.with(
			P.shape({
				serverless_http_error: P.shape({
					status_code: P.number,
					body: P.string,
				}),
			}),
			(e) => {
				const { status_code, body } = e.serverless_http_error;
				return {
					severity: "error",
					kind: "serverless_http",
					title: `Serverless HTTP ${status_code}`,
					statusCode: status_code,
					body,
					fingerprint: `http:${status_code}:${body.slice(0, 64)}`,
				};
			},
		)
		.with(
			P.shape({
				serverless_connection_error: P.shape({ message: P.string }),
			}),
			(e) => ({
				severity: "error",
				kind: "serverless_connection",
				title: "Serverless connection failed",
				body: e.serverless_connection_error.message,
				fingerprint: `conn:${e.serverless_connection_error.message.slice(0, 64)}`,
			}),
		)
		.with(
			P.shape({
				serverless_destination_blocked: P.shape({ reason: P.string }),
			}),
			(e) => ({
				severity: "error",
				kind: "serverless_destination_blocked",
				title: "Serverless URL is not an allowed destination",
				body: e.serverless_destination_blocked.reason,
				fingerprint: `blocked:${e.serverless_destination_blocked.reason.slice(0, 64)}`,
			}),
		)
		.with(
			P.shape({
				serverless_invalid_sse_payload: P.shape({ message: P.string }),
			}),
			(e) => ({
				severity: "warning",
				kind: "serverless_invalid_sse",
				title: "Invalid SSE payload",
				body: e.serverless_invalid_sse_payload.message,
				fingerprint: `sse:${e.serverless_invalid_sse_payload.message.slice(0, 64)}`,
			}),
		)
		.otherwise(() => ({
			severity: "error",
			kind: "unknown",
			title: "Unknown runner pool error",
			fingerprint: "unknown",
		}));
}

interface ErrorGroup {
	classified: ClassifiedError;
	regions: string[];
}

function groupErrors(
	errors: Record<string, RivetActorError | undefined>,
): ErrorGroup[] {
	const map = new Map<string, ErrorGroup>();
	for (const [region, err] of Object.entries(errors)) {
		if (!err) continue;
		const classified = classifyRunnerError(err);
		const existing = map.get(classified.fingerprint);
		if (existing) {
			existing.regions.push(region);
		} else {
			map.set(classified.fingerprint, { classified, regions: [region] });
		}
	}
	return Array.from(map.values());
}

interface RunnerPoolErrorPopoverProps {
	errors: Record<string, RivetActorError | undefined>;
	onEditConfig?: () => void;
	renderRegion?: (regionId: string) => ReactNode;
	className?: string;
	iconOnly?: boolean;
}

function defaultRenderRegion(regionId: string) {
	const key = getRegionKey(regionId);
	const label = key && REGION_LABEL[key] ? REGION_LABEL[key] : regionId;
	return (
		<>
			<RegionIcon region={key} className="size-3 min-w-3" />
			<span>{label}</span>
		</>
	);
}

export function RunnerPoolErrorPopover({
	errors,
	onEditConfig,
	renderRegion = defaultRenderRegion,
	className,
	iconOnly = false,
}: RunnerPoolErrorPopoverProps) {
	const [open, setOpen] = useState(false);
	const groups = useMemo(() => groupErrors(errors), [errors]);
	const totalRegions = useMemo(
		() => groups.reduce((sum, g) => sum + g.regions.length, 0),
		[groups],
	);

	if (groups.length === 0) return null;

	const topSeverity: Severity = groups.some(
		(g) => g.classified.severity === "error",
	)
		? "error"
		: "warning";

	const summary =
		groups.length === 1
			? `${groups[0].classified.title} · ${formatRegionCount(totalRegions)}`
			: `${formatRegionCount(totalRegions)} failing`;

	const triggerIcon =
		topSeverity === "error" ? faCircleExclamation : faTriangleExclamation;

	return (
		<Popover open={open} onOpenChange={setOpen}>
			{iconOnly ? (
				<Tooltip open={open ? false : undefined}>
					<TooltipTrigger asChild>
						<PopoverTrigger asChild>
							<button
								type="button"
								aria-label={summary}
								className={cn(
									"inline-flex size-5 items-center justify-center rounded-md transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-ring",
									topSeverity === "error"
										? "text-destructive hover:bg-destructive/15"
										: "text-amber-500 hover:bg-amber-500/15",
									className,
								)}
							>
								<Icon icon={triggerIcon} className="size-4" />
							</button>
						</PopoverTrigger>
					</TooltipTrigger>
					<TooltipPortal>
						<TooltipContent>
							<div className="flex flex-col gap-0.5">
								<span className="font-medium">{summary}</span>
								<span className="text-xs text-muted-foreground">
									Click to view details
								</span>
							</div>
						</TooltipContent>
					</TooltipPortal>
				</Tooltip>
			) : (
				<PopoverTrigger asChild>
					<button
						type="button"
						className={cn(
							"inline-flex items-center gap-1.5 rounded-md border px-2 py-1 text-xs font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-ring",
							topSeverity === "error"
								? "border-destructive/40 bg-destructive/10 text-destructive hover:bg-destructive/15"
								: "border-amber-500/40 bg-amber-500/10 text-amber-500 hover:bg-amber-500/15",
							className,
						)}
					>
						<Icon icon={triggerIcon} className="size-3" />
						<span className="truncate max-w-[200px]">
							{summary}
						</span>
					</button>
				</PopoverTrigger>
			)}
			<PopoverContent
				align="start"
				className="w-[420px] p-0"
				onOpenAutoFocus={(e) => e.preventDefault()}
			>
				<ErrorPopoverBody
					groups={groups}
					renderRegion={renderRegion}
					onEditConfig={
						onEditConfig
							? () => {
									setOpen(false);
									onEditConfig();
								}
							: undefined
					}
				/>
			</PopoverContent>
		</Popover>
	);
}

function ErrorPopoverBody({
	groups,
	renderRegion,
	onEditConfig,
}: {
	groups: ErrorGroup[];
	renderRegion: (regionId: string) => ReactNode;
	onEditConfig?: () => void;
}) {
	const [activeFingerprint, setActiveFingerprint] = useState(
		groups[0].classified.fingerprint,
	);

	const showTabs = groups.length > 1;

	return (
		<div className="flex flex-col">
			<div className="flex items-start justify-between gap-2 border-b px-4 py-3">
				<div className="min-w-0">
					<div className="text-sm font-semibold">
						Runner pool errors
					</div>
					<div className="text-xs text-muted-foreground">
						{groups.length === 1
							? `${formatRegionCount(groups[0].regions.length)} affected`
							: `${groups.length} distinct errors across ${formatRegionCount(
									groups.reduce(
										(sum, g) => sum + g.regions.length,
										0,
									),
								)}`}
					</div>
				</div>
			</div>

			{showTabs ? (
				<Tabs
					value={activeFingerprint}
					onValueChange={setActiveFingerprint}
					className="flex flex-col"
				>
					<TabsList className="mx-3 mt-3 h-auto w-[calc(100%-1.5rem)] justify-start gap-1 overflow-x-auto rounded-md border-b-0 bg-muted/40 p-1 [&::-webkit-scrollbar]:hidden">
						{groups.map((g) => (
							<TabsTrigger
								key={g.classified.fingerprint}
								value={g.classified.fingerprint}
								className="h-6 shrink-0 rounded-sm border-0 px-2 py-0 text-xs font-medium data-[state=active]:border-b-transparent data-[state=active]:bg-background data-[state=active]:text-foreground"
							>
								<SeverityDot severity={g.classified.severity} />
								<span className="ml-1.5 truncate max-w-[100px]">
									{g.classified.title}
								</span>
								<Badge
									variant="secondary"
									className="ml-1.5 h-4 px-1 text-[10px]"
								>
									{g.regions.length}
								</Badge>
							</TabsTrigger>
						))}
					</TabsList>
					{groups.map((g) => (
						<TabsContent
							key={g.classified.fingerprint}
							value={g.classified.fingerprint}
							className="mt-0"
						>
							<GroupBody group={g} renderRegion={renderRegion} />
						</TabsContent>
					))}
				</Tabs>
			) : (
				<GroupBody group={groups[0]} renderRegion={renderRegion} />
			)}

			{onEditConfig ? (
				<div className="border-t px-3 py-2">
					<Button
						variant="ghost"
						size="sm"
						className="w-full justify-start"
						startIcon={<Icon icon={faPenToSquare} />}
						onClick={onEditConfig}
					>
						Edit runner config
					</Button>
				</div>
			) : null}
		</div>
	);
}

function GroupBody({
	group,
	renderRegion,
}: {
	group: ErrorGroup;
	renderRegion: (regionId: string) => ReactNode;
}) {
	const { classified, regions } = group;

	return (
		<div className="flex flex-col gap-3 px-4 py-3">
			<div className="flex flex-wrap items-center gap-1.5">
				<span className="text-xs text-muted-foreground">
					{regions.length === 1 ? "Region:" : "Regions:"}
				</span>
				{regions.map((r) => (
					<Badge
						key={r}
						variant="secondary"
						className="h-5 gap-1 px-1.5 text-[11px] font-normal"
					>
						{renderRegion(r)}
					</Badge>
				))}
			</div>

			{classified.body ? (
				<ErrorBody
					body={classified.body}
					label={
						classified.kind === "serverless_http"
							? "Response body"
							: "Details"
					}
				/>
			) : (
				<p className="text-xs text-muted-foreground">
					{describeKind(classified.kind)}
				</p>
			)}
		</div>
	);
}

function ErrorBody({ body, label }: { body: string; label: string }) {
	const [copied, setCopied] = useState(false);
	const formatted = useMemo(() => formatBody(body), [body]);

	const handleCopied = () => {
		setCopied(true);
		setTimeout(() => setCopied(false), 1500);
	};

	return (
		<div className="relative rounded-md border bg-muted/30">
			<div className="flex items-center justify-between border-b px-2 py-1">
				<span className="text-[10px] uppercase tracking-wider text-muted-foreground">
					{label}
				</span>
				<CopyTrigger value={formatted.code} onClick={handleCopied}>
					<Button
						type="button"
						variant="ghost"
						size="sm"
						className="h-5 gap-1 px-1.5 text-[11px]"
					>
						<Icon
							icon={copied ? faCheck : faCopy}
							className="size-3"
						/>
						{copied ? "Copied" : "Copy"}
					</Button>
				</CopyTrigger>
			</div>
			<ScrollArea className="max-h-48">
				{formatted.language === "json" ? (
					<CodePreview
						language="json"
						className="text-[11px] leading-relaxed p-1"
						code={formatted.code}
					/>
				) : (
					<Code className="block whitespace-pre-wrap break-words p-2 text-[11px] leading-relaxed">
						{formatted.code}
					</Code>
				)}
			</ScrollArea>
		</div>
	);
}

function formatBody(body: string): { code: string; language: "json" | "text" } {
	const trimmed = body.trim();
	if (
		(trimmed.startsWith("{") && trimmed.endsWith("}")) ||
		(trimmed.startsWith("[") && trimmed.endsWith("]"))
	) {
		try {
			return {
				code: JSON.stringify(JSON.parse(trimmed), null, 2),
				language: "json",
			};
		} catch {
			// Fall through.
		}
	}
	return { code: body, language: "text" };
}

function SeverityDot({ severity }: { severity: Severity }) {
	return (
		<span
			className={cn(
				"inline-block size-1.5 rounded-full",
				severity === "error" ? "bg-destructive" : "bg-amber-500",
			)}
		/>
	);
}

function formatRegionCount(n: number) {
	return n === 1 ? "1 region" : `${n} regions`;
}

function describeKind(kind: ClassifiedError["kind"]): string {
	switch (kind) {
		case "downgrade":
			return "Runner pool was downgraded to an unsupported version. Revert to a higher version.";
		case "serverless_stream_ended_early":
			return "Connection terminated before the runner stopped. Check the request lifespan limits on your serverless provider.";
		case "serverless_destination_blocked":
			return "The configured serverless URL points at a destination Rivet is not allowed to reach. Use a publicly routable URL, or allow the address range in the engine's outbound configuration.";
		case "internal":
			return "An internal error occurred in the runner pool.";
		default:
			return "Unknown error.";
	}
}
