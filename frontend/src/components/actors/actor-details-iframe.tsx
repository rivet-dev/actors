import {
	faMoon,
	faSpinnerThird,
	faTriangleExclamation,
	Icon,
} from "@rivet-gg/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { z } from "zod";
import {
	Button,
	cn,
	getConfig,
	ShimmerLine,
	Tabs,
	TabsList,
	TabsTrigger,
	WithTooltip,
} from "@/components";
import { useTheme } from "@/lib/theme";
import { useTimeout } from "../hooks/use-timeout";
import { ActorDetailsLegacy } from "./actor-details-legacy";
import {
	CLOUD_TABS,
	SKELETON_INSPECTOR_TABS,
	useHasManagedPool,
	useShowTabLabels,
} from "./actor-details-shared";
import { ActorDetailsSkeleton } from "./actor-details-skeleton";
import {
	actorMetadataQueryOptions,
	actorWakeUpMutationOptions,
	isVersionAtLeast,
} from "./actor-inspector-context";
import {
	resumeAutoWake,
	useAutoWakeSuppression,
} from "./auto-wake-suppression";
import { useFiltersValue } from "./actor-filters-context";
import { useDataProvider } from "./data-provider";
import { resolveInspectorTabIcon } from "./inspector-tab-icons";
import type { InspectorTabDescriptor } from "./inspector-tab-registry";
import type { ActorId, ActorStatus } from "./queries";
import { ActorStatusLabel, QueriedActorError } from "./actor-status-label";
import { useRivetToken } from "./use-rivet-token";

interface ActorDetailsProps {
	actorId: ActorId;
	tab?: string;
	onTabChange?: (tab: string) => void;
}

// Engines below this RivetKit version don't serve the bundled inspector UI
// at `/inspector/ui/`, so the iframe would 404. The dashboard falls back to
// rendering the inspector inline for those actors (the legacy path).
//
// This must be updated to the actual release tag whenever the iframe bundle
// first ships. For development before the first release that includes
// the iframe route, the threshold is high enough to force the legacy path
// in default test setups; override via MSW to verify the iframe path.
export const IFRAME_INSPECTOR_MIN_VERSION = "2.3.0";

// A terminal actor never becomes reachable again, so its inspector can't
// connect and its inspector token is no longer readable.
const isActorTerminal = (status: ActorStatus | undefined) =>
	status === "stopped" || status === "crashed" || status === "crash-loop";

/**
 * Dashboard right panel for a selected actor.
 *
 * Fetches the actor's metadata and dispatches between two implementations:
 *
 *   • engines new enough to serve `/inspector/ui/` → render the inspector
 *     inside an iframe (`ActorDetailsIframePath`); the dashboard owns only
 *     the tab strip and lets the iframe own everything else.
 *   • older engines → render the inspector inline in the dashboard
 *     (`ActorDetailsLegacy`); the dashboard mounts ActorInspectorProvider
 *     and the tab components itself.
 *
 * Both paths render the same tab content components — the only thing that
 * differs is where they're hosted.
 */
export const ActorsActorDetails = memo(function ActorsActorDetails({
	actorId,
	tab,
	onTabChange,
}: ActorDetailsProps) {
	const engineUrl = getConfig().apiUrl;
	const rivetToken = useRivetToken();

	// When the user explicitly sleeps the actor, hold off mounting the inspector
	// connection. Its /metadata polling and websocket count as actor activity and
	// would immediately wake it back up. Reschedule does not drop the connection:
	// the actor reallocates back to running on its own and the inspector
	// reconnects, so only `sleep` suppresses here.
	const suppression = useAutoWakeSuppression(actorId);
	const isSuppressed = suppression === "sleep";

	// Opening the inspector connection (/metadata polling and websocket) wakes a
	// sleeping actor, so "Auto-wake actors on select" gates that connection.
	const filters = useFiltersValue();
	const autoWakeOnSelect = filters.wakeOnSelect?.value.includes("1") ?? true;

	// Per-mount opt-in to wake one sleeping actor without changing the setting.
	// Reset per actor so each selection re-evaluates the setting.
	const [manualWake, setManualWake] = useState(false);
	useEffect(() => {
		setManualWake(false);
	}, [actorId]);

	// Hitting /health wakes the actor directly, giving the button pending/error
	// state, then manualWake mounts the inspector. Needs only the rivet token.
	const wakeMutation = useMutation({
		...actorWakeUpMutationOptions(),
		onSuccess: () => setManualWake(true),
	});

	const { data: status } = useQuery({
		...useDataProvider().actorStatusQueryOptions(actorId),
		refetchInterval: 1000,
	});

	// Hold the inspector while a sleeping actor stays asleep. Status is undefined
	// during the initial load, so hold then too rather than racing the connection
	// open before the actor's state is known.
	const wakeBlocked = !autoWakeOnSelect && !manualWake;
	const showSleepingPrompt = wakeBlocked && status === "sleeping";
	const holdForSleep =
		wakeBlocked && (status === "sleeping" || status === undefined);
	const holdInspector = isSuppressed || holdForSleep;

	// Resume auto-wake on the rising edge into running, clearing suppression set
	// by an explicit sleep once the actor is back up. A rising edge is used
	// instead of "is running" so a status refetch returning running while already
	// running does not prematurely clear suppression set milliseconds earlier.
	const prevStatusRef = useRef<ActorStatus | undefined>(undefined);
	useEffect(() => {
		const prevStatus = prevStatusRef.current;
		prevStatusRef.current = status;
		if (status === "running" && prevStatus !== "running") {
			resumeAutoWake(actorId);
		}
	}, [status, actorId]);

	const inspectorTokenQuery = useQuery({
		...useDataProvider().actorInspectorTokenQueryOptions(actorId),
		enabled: !holdInspector,
	});

	const credentials = useMemo(
		() =>
			inspectorTokenQuery.data
				? {
						url: engineUrl,
						inspectorToken: inspectorTokenQuery.data,
						token: rivetToken,
					}
				: null,
		[engineUrl, inspectorTokenQuery.data, rivetToken],
	);

	const metadataQuery = useQuery({
		...actorMetadataQueryOptions({
			actorId,
			// biome-ignore lint/style/noNonNullAssertion: gated by enabled
			credentials: credentials!,
		}),
		enabled: !!credentials && !holdInspector,
		// The host only reads the version to pick the inspector renderer, which
		// never changes for a running actor generation. The shared query
		// options poll every second (the iframe SPA relies on that for its own
		// liveness), but a second poll from the host is pure duplication. Poll
		// until the version resolves once, then stop. Actor liveness is tracked
		// by the status query above, not here.
		refetchInterval: (query) => (query.state.data ? false : 2_000),
	});

	const [forceLegacy, setForceLegacy] = useState(false);
	useEffect(() => {
		setForceLegacy(false);
	}, [actorId]);

	// Don't mount the iframe or legacy inspector while sleep is suppressed: an
	// "Updating actor…" overlay stands in until the actor settles back into a
	// state where reconnecting won't wake it.
	if (isSuppressed) {
		return (
			<div className="flex flex-col h-full flex-1 min-h-0 items-center justify-center bg-card gap-2 text-sm text-muted-foreground">
				<Icon icon={faSpinnerThird} className="animate-spin" />
				<span>Updating actor…</span>
			</div>
		);
	}

	if (showSleepingPrompt) {
		return (
			<div className="flex flex-col h-full flex-1 min-h-0 items-center justify-center bg-card gap-3 px-6 text-center">
				<Icon icon={faMoon} className="text-lg text-muted-foreground" />
				<div className="text-sm font-medium">Actor is sleeping</div>
				<div className="max-w-sm text-xs text-muted-foreground">
					Auto-wake on select is off, so opening this actor won't wake
					it. Wake it to connect the inspector.
				</div>
				<Button
					size="sm"
					variant="outline"
					isLoading={wakeMutation.isPending}
					startIcon={<Icon icon={faMoon} />}
					onClick={() =>
						wakeMutation.mutate({
							actorId,
							credentials: {
								url: engineUrl,
								token: rivetToken,
							},
						})
					}
				>
					Wake actor
				</Button>
				{wakeMutation.isError && (
					<div className="max-w-sm text-xs text-destructive">
						Couldn't wake the actor. Check that it's reachable and
						try again.
					</div>
				)}
			</div>
		);
	}

	// The inspector token is required by both inspector renderers and by the
	// cloud tabs' lifecycle actions, so wait for credentials before mounting
	// anything. A terminal actor is the exception: its token lives in the
	// actor's own KV, which is unreadable once it's gone, so waiting would sit
	// on a skeleton forever and never surface why. Neither the terminal state
	// nor the Metadata tab needs the token.
	if (!credentials && !isActorTerminal(status)) {
		return (
			<div className="flex flex-col h-full flex-1 min-h-0">
				<ActorDetailsSkeleton shimmer />
			</div>
		);
	}

	// The actor's `/metadata` inspector endpoint is unreachable while the actor
	// is still booting, so `metadataQuery.data` can stay pending for a while.
	// The dashboard-owned cloud tabs (Metadata, Logs) render from engine
	// control-plane data and must be reachable in exactly that window, so the
	// tab chrome mounts as soon as credentials exist. Until metadata resolves
	// we don't know the runtime version, so default to the iframe path (the
	// case for every runtime >= IFRAME_INSPECTOR_MIN_VERSION). The iframe shows
	// "Connecting…" for the inspector tabs while the cloud tabs stay usable. If
	// metadata later reveals an older runtime, `useIframe` flips to the legacy
	// renderer.
	const iframeCapable =
		!metadataQuery.data ||
		isVersionAtLeast(
			metadataQuery.data.version,
			IFRAME_INSPECTOR_MIN_VERSION,
		);
	const useIframe = !forceLegacy && iframeCapable;

	if (useIframe) {
		return (
			<ActorDetailsIframePath
				actorId={actorId}
				tab={tab}
				onTabChange={onTabChange}
				inspectorToken={credentials?.inspectorToken ?? ""}
				rivetToken={rivetToken}
				status={status}
				onFallbackToLegacy={() => setForceLegacy(true)}
			/>
		);
	}

	// Reaching here means the runtime is too old for the iframe (or the user
	// forced legacy). The legacy renderer needs both the token and the resolved
	// version, so keep the skeleton until they land.
	if (!credentials || !metadataQuery.data) {
		return (
			<div className="flex flex-col h-full flex-1 min-h-0">
				<ActorDetailsSkeleton shimmer />
			</div>
		);
	}

	const legacy = (
		<ActorDetailsLegacy
			actorId={actorId}
			tab={tab}
			onTabChange={onTabChange}
			inspectorToken={credentials.inspectorToken}
			rivetkitVersion={metadataQuery.data.version}
		/>
	);

	// When the user opted into the legacy inspector after the iframe failed,
	// keep a persistent banner so the degraded mode (and any quirks that come
	// with it) is expected rather than surprising.
	if (forceLegacy) {
		return (
			<div className="flex flex-col h-full flex-1 min-h-0">
				<div className="flex items-center gap-2 border-b border-warning bg-warning/10 px-3 py-1.5 text-xs text-foreground">
					<Icon
						icon={faTriangleExclamation}
						className="shrink-0 text-warning"
					/>
					<span className="min-w-0 flex-1">
						You're using the legacy inspector. It's an older
						version, so some things may look or behave differently.
					</span>
					<Button
						variant="ghost"
						size="xs"
						onClick={() => setForceLegacy(false)}
					>
						Switch back
					</Button>
				</div>
				<div className="flex-1 min-h-0">{legacy}</div>
			</div>
		);
	}

	return legacy;
});

// ---------------------------------------------------------------------------
// Iframe path: dashboard hosts a single iframe pointed at the actor's
// inspector-ui bundle and owns only the tab strip. All inspector RPCs,
// version-specific decoding, and tab content rendering live inside the
// iframe.
// ---------------------------------------------------------------------------

const iframeMessageSchema = z.discriminatedUnion("type", [
	z.object({ type: z.literal("ready"), v: z.literal(1) }),
	z.object({ type: z.literal("token-refresh-needed"), v: z.literal(1) }),
	z.object({
		type: z.literal("tabs-available"),
		v: z.literal(1),
		tabs: z.array(
			z.object({
				id: z.string(),
				label: z.string(),
				icon: z.string(),
				// Optional for backwards compat with older SPAs.
				isCustom: z.boolean().optional(),
			}),
		),
	}),
]);

const SKELETON_TIMEOUT_MS = 8_000;

function ActorDetailsIframePath({
	actorId,
	tab,
	onTabChange,
	inspectorToken,
	rivetToken,
	status,
	onFallbackToLegacy,
}: ActorDetailsProps & {
	inspectorToken: string;
	rivetToken: string;
	status?: ActorStatus;
	onFallbackToLegacy: () => void;
}) {
	const engineUrl = getConfig().apiUrl;
	const queryClient = useQueryClient();
	const dataProvider = useDataProvider();
	const hasManagedPool = useHasManagedPool();

	const iframeRef = useRef<HTMLIFrameElement>(null);
	const [iframeListening, setIframeListening] = useState(false);
	const [bootTimedOut, setBootTimedOut] = useState(false);
	const [inspectorTabs, setInspectorTabs] = useState<
		InspectorTabDescriptor[]
	>([]);

	useEffect(() => {
		setIframeListening(false);
		setBootTimedOut(false);
		setInspectorTabs([]);
	}, [actorId]);

	const expectedOrigin = useMemo(() => {
		try {
			return new URL(engineUrl).origin;
		} catch {
			return engineUrl;
		}
	}, [engineUrl]);

	// Custom tab ids come from the SPA's `tabs-available` envelope, which
	// the SPA derives from its own tab-config fetch. Single source of truth
	// — the dashboard does NOT make its own tab-config request, so the
	// dashboard and SPA can never disagree about which tabs are custom.
	// Backwards-compatible: descriptors from older SPAs that don't set
	// `isCustom` produce an empty set, which routes every active tab to
	// the SPA (correct fallback — built-ins render there; missing custom
	// tabs render the SPA's null fallback rather than wedging the iframe).
	const customTabIds = useMemo(
		() => new Set(inspectorTabs.filter((t) => t.isCustom).map((t) => t.id)),
		[inspectorTabs],
	);

	const visibleCloudTabs = useMemo(
		() => CLOUD_TABS.filter((t) => t.shouldShow({ hasManagedPool })),
		[hasManagedPool],
	);

	const inSkeletonMode = inspectorTabs.length === 0;
	const displayedInspectorTabs = inSkeletonMode
		? SKELETON_INSPECTOR_TABS
		: inspectorTabs;

	const displayedTabs = useMemo(() => {
		// Dashboard tabs are authoritative: an engine whose inspector bundle
		// still advertises a tab the dashboard now owns (e.g. an older bundle
		// that lists "metadata") would otherwise produce a duplicate. Drop the
		// inspector copy of any id a dashboard tab claims.
		const cloudTabIds = new Set(visibleCloudTabs.map((t) => t.id));
		const inspector = displayedInspectorTabs
			.filter((t) => !cloudTabIds.has(t.id))
			.map((t) => ({
				kind: "inspector" as const,
				...t,
			}));
		const cloud = visibleCloudTabs.map((t) => ({
			kind: "cloud" as const,
			id: t.id,
			label: t.label,
			icon: t.icon,
		}));
		return [...inspector, ...cloud];
	}, [displayedInspectorTabs, visibleCloudTabs]);

	const activeTabSpec = useMemo(() => {
		if (tab) {
			const match = displayedTabs.find((t) => t.id === tab);
			if (match) return match;
		}
		return displayedTabs[0];
	}, [tab, displayedTabs]);

	const activeInspectorTabId =
		activeTabSpec?.kind === "inspector" ? activeTabSpec.id : undefined;

	// A tab is "custom" iff the tab-config response lists it as a non-hidden
	// entry. Anything else (including built-in ids and ids the dashboard
	// hasn't learned about yet) routes through the SPA at /inspector/ui/.
	// This avoids future-proof breakage when a new built-in tab lands and
	// also gracefully handles the in-flight window before tab-config
	// resolves.
	const isActiveCustomTab =
		activeInspectorTabId !== undefined &&
		customTabIds.has(activeInspectorTabId);

	const { theme } = useTheme();

	const themeRef = useRef(theme);
	themeRef.current = theme;

	const actorSegment = useMemo(
		() =>
			rivetToken
				? `${encodeURIComponent(actorId)}@${encodeURIComponent(rivetToken)}`
				: encodeURIComponent(actorId),
		[actorId, rivetToken],
	);

	const runnerInspectorUiUrl = useMemo(
		() => new URL(`/gateway/${actorSegment}/inspector/ui/`, engineUrl).href,
		[actorSegment, engineUrl],
	);

	const src = useMemo(() => {
		if (isActiveCustomTab && activeInspectorTabId) {
			const url = new URL(
				`/gateway/${actorSegment}/inspector/custom-tabs/${encodeURIComponent(activeInspectorTabId)}/`,
				engineUrl,
			);
			url.searchParams.set("actorId", actorId);
			url.searchParams.set("shellOrigin", window.location.origin);
			url.searchParams.set("theme", themeRef.current);
			return url.href;
		}

		// The engine serves the inspector-UI bundle from its embedded memory
		// for every runner, so always load it from the runner gateway.
		const url = new URL(runnerInspectorUiUrl);
		url.searchParams.set("actorId", actorId);
		url.searchParams.set("shellOrigin", window.location.origin);
		url.searchParams.set("theme", themeRef.current);
		return url.href;
	}, [
		actorId,
		engineUrl,
		isActiveCustomTab,
		activeInspectorTabId,
		actorSegment,
		runnerInspectorUiUrl,
	]);

	const { reset } = useTimeout(() => {
		setBootTimedOut(true);
	}, SKELETON_TIMEOUT_MS);

	useEffect(() => {
		setIframeListening(false);
		setBootTimedOut(false);
		reset();
	}, [src]);

	const postInit = useCallback(() => {
		if (!iframeListening) return;
		const iframe = iframeRef.current;
		if (!iframe?.contentWindow) return;
		iframe.contentWindow.postMessage(
			{
				type: "init",
				v: 1,
				actorId,
				authToken: inspectorToken,
				rivetToken: rivetToken || undefined,
				activeTab: activeInspectorTabId,
				theme,
			},
			expectedOrigin,
		);
	}, [
		iframeListening,
		actorId,
		inspectorToken,
		rivetToken,
		expectedOrigin,
		activeInspectorTabId,
		theme,
	]);

	const postSetActiveTab = useCallback(
		(nextTab: string) => {
			if (!iframeListening) return;
			const iframe = iframeRef.current;
			if (!iframe?.contentWindow) return;
			iframe.contentWindow.postMessage(
				{ type: "set-active-tab", v: 1, tab: nextTab },
				expectedOrigin,
			);
		},
		[iframeListening, expectedOrigin],
	);

	useEffect(() => {
		postInit();
	}, [postInit]);

	useEffect(() => {
		if (!activeInspectorTabId) return;
		postSetActiveTab(activeInspectorTabId);
	}, [activeInspectorTabId, postSetActiveTab]);

	useEffect(() => {
		const onMessage = (event: MessageEvent) => {
			if (event.origin !== expectedOrigin) return;
			if (event.source !== iframeRef.current?.contentWindow) return;
			const parsed = iframeMessageSchema.safeParse(event.data);
			if (!parsed.success) return;
			const msg = parsed.data;
			if (msg.type === "ready") {
				setIframeListening(true);
				setBootTimedOut(false);
				return;
			}
			if (msg.type === "tabs-available") {
				setInspectorTabs(msg.tabs);
				return;
			}
			if (msg.type === "token-refresh-needed") {
				queryClient.invalidateQueries({
					queryKey:
						dataProvider.actorInspectorTokenQueryOptions(actorId)
							.queryKey,
				});
				queryClient.invalidateQueries({
					queryKey: ["publishable-token"],
				});
				queryClient.invalidateQueries({
					queryKey: ["engine-admin-token"],
				});
			}
		};
		window.addEventListener("message", onMessage);
		return () => window.removeEventListener("message", onMessage);
	}, [expectedOrigin, dataProvider, actorId, queryClient]);

	const showIframe = activeTabSpec?.kind === "inspector" || !activeTabSpec;
	const activeCloudTab =
		activeTabSpec?.kind === "cloud"
			? CLOUD_TABS.find((t) => t.id === activeTabSpec.id)
			: undefined;

	// A dead actor never becomes reachable, so its inspector WS can't connect
	// and the iframe would sit on "Connecting…" (or hit the 8s boot timeout)
	// forever. Detect the terminal states from the engine status query and
	// reuse the existing status/error UI instead of an indefinite spinner. The
	// Metadata tab stays available with the full lifecycle and restart actions.
	const isTerminal = isActorTerminal(status);

	const { ref: tabListRef, showLabels } = useShowTabLabels();

	return (
		<Tabs
			value={activeTabSpec?.id}
			onValueChange={onTabChange}
			className="flex-1 min-h-0 min-w-0 flex flex-col"
		>
			<div className="relative flex items-stretch border-b h-[45px]">
				<div className="flex flex-1 items-center h-full min-w-0">
					<div
						ref={tabListRef}
						className="flex-1 min-w-0 overflow-hidden h-full"
					>
						<TabsList className="flex border-none h-full items-end min-w-0 overflow-hidden w-full">
							{displayedTabs.map((t) => (
								<WithTooltip
									key={t.id}
									delayDuration={0}
									disabled={showLabels}
									trigger={
										<TabsTrigger
											value={t.id}
											disabled={
												inSkeletonMode &&
												t.kind === "inspector"
											}
											className={cn(
												"text-xs px-2.5 py-1 pb-2 min-w-0 shrink gap-1 isolate before:absolute before:inset-x-0.5 before:top-1 before:bottom-2 before:-z-10 before:rounded-md before:transition-colors hover:before:bg-foreground/[0.06]",
												inSkeletonMode &&
													t.kind === "inspector" &&
													"opacity-60",
											)}
										>
											<Icon
												icon={resolveInspectorTabIcon(
													t.icon,
												)}
												className="shrink-0"
											/>
											<span
												className={
													showLabels
														? "truncate"
														: "hidden"
												}
											>
												{t.label}
											</span>
										</TabsTrigger>
									}
									content={t.label}
								/>
							))}
						</TabsList>
					</div>
				</div>
				{inSkeletonMode && !isTerminal && (
					<ShimmerLine className="absolute bottom-0 left-0 right-0" />
				)}
			</div>
			<div className="relative flex-1 min-h-0">
				{!isTerminal && (
					<iframe
						ref={iframeRef}
						src={src}
						sandbox="allow-scripts allow-same-origin"
						className="absolute inset-0 w-full h-full border-0 bg-card"
						style={{
							visibility:
								showIframe && iframeListening
									? "visible"
									: "hidden",
						}}
						title={`Inspector for actor ${actorId}`}
					/>
				)}
				{showIframe && isTerminal && (
					<div className="absolute inset-0 flex flex-col items-center justify-center bg-card gap-3 px-6 text-center">
						<Icon
							icon={faTriangleExclamation}
							className="text-lg text-warning"
						/>
						<div className="text-sm font-medium">
							<ActorStatusLabel status={status} />
						</div>
						<div className="max-w-sm text-xs text-muted-foreground [&_p]:m-0">
							<QueriedActorError actorId={actorId} />
						</div>
						<Button
							size="sm"
							variant="outline"
							onClick={() => onTabChange?.("metadata")}
						>
							View actor details
						</Button>
					</div>
				)}
				{showIframe && !isTerminal && !iframeListening && (
					<div className="absolute inset-0 flex flex-col items-center justify-center bg-card text-sm text-muted-foreground gap-2 px-4 text-center">
						{bootTimedOut ? (
							<div className="flex max-w-sm flex-col items-center gap-3">
								<p>
									Inspector didn't load. Check that the
									actor is running and reachable.
								</p>
								<div className="flex items-center gap-2">
									<Button
										size="sm"
										onClick={() => {
											if (!iframeRef.current) return;
											// Reassigning src reloads the iframe.
											iframeRef.current.src =
												iframeRef.current.src;
											setBootTimedOut(false);
											reset();
										}}
									>
										Reload
									</Button>
									<WithTooltip
										content="Older, less secure inspector. Use it only if this keeps failing to load."
										trigger={
											<Button
												variant="ghost"
												size="sm"
												className="text-muted-foreground"
												onClick={onFallbackToLegacy}
											>
												Use legacy inspector
											</Button>
										}
									/>
								</div>
							</div>
						) : (
							<span>Connecting to inspector…</span>
						)}
					</div>
				)}
				{activeCloudTab && (
					<div className="absolute inset-0 bg-card">
						{activeCloudTab.render(actorId)}
					</div>
				)}
			</div>
		</Tabs>
	);
}
