import { Icon } from "@rivet-gg/icons";
import { useMemo } from "react";
import {
	cn,
	getConfig,
	Tabs,
	TabsList,
	TabsTrigger,
	WithTooltip,
} from "@/components";
import { ActorInspectorProvider } from "./actor-inspector-context";
import {
	CLOUD_TABS,
	SKELETON_INSPECTOR_TABS,
	useHasManagedPool,
	useShowTabLabels,
} from "./actor-details-shared";
import { ActorDetailsSkeleton } from "./actor-details-skeleton";
import {
	InspectorTabContent,
	useAvailableInspectorTabs,
} from "./inspector-tab-registry";
import { resolveInspectorTabIcon } from "./inspector-tab-icons";
import type { ActorId } from "./queries";
import { useRivetToken } from "./use-rivet-token";

interface Props {
	actorId: ActorId;
	tab?: string;
	onTabChange?: (tab: string) => void;
	inspectorToken: string;
	rivetkitVersion: string;
}

/**
 * Legacy actor-details renderer used for actors on engines that don't serve
 * the iframe-ui bundle (i.e. pre-`IFRAME_INSPECTOR_MIN_VERSION`). The
 * dashboard mounts `ActorInspectorProvider` itself and renders the tab
 * strip + tab content inline — same visuals as the iframe path, just
 * without the iframe.
 *
 * This path will be removed once no supported engine version is too old to
 * serve the iframe bundle.
 */
export function ActorDetailsLegacy({
	actorId,
	tab,
	onTabChange,
	inspectorToken,
	rivetkitVersion,
}: Props) {
	const engineUrl = getConfig().apiUrl;
	const rivetToken = useRivetToken();

	const credentials = useMemo(
		() => ({
			url: engineUrl,
			inspectorToken,
			token: rivetToken,
		}),
		[engineUrl, inspectorToken, rivetToken],
	);

	return (
		<ActorInspectorProvider
			actorId={actorId}
			credentials={credentials}
			initialVersion={rivetkitVersion}
		>
			<LegacyTabShell
				actorId={actorId}
				tab={tab}
				onTabChange={onTabChange}
			/>
		</ActorInspectorProvider>
	);
}

function LegacyTabShell({
	actorId,
	tab,
	onTabChange,
}: {
	actorId: ActorId;
	tab?: string;
	onTabChange?: (tab: string) => void;
}) {
	const availableTabs = useAvailableInspectorTabs(actorId);
	const hasManagedPool = useHasManagedPool();
	const { ref: tabListRef, showLabels } = useShowTabLabels();

	const inSkeletonMode = availableTabs === null;
	const displayedInspectorTabs = availableTabs ?? SKELETON_INSPECTOR_TABS;

	const visibleCloudTabs = useMemo(
		() => CLOUD_TABS.filter((t) => t.shouldShow({ hasManagedPool })),
		[hasManagedPool],
	);

	const displayedTabs = useMemo(() => {
		// Dashboard tabs are authoritative: drop the inspector copy of any id a
		// dashboard tab claims (e.g. "metadata") so the two sets can't collide
		// into a duplicate tab.
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
	const activeCloudTab =
		activeTabSpec?.kind === "cloud"
			? CLOUD_TABS.find((t) => t.id === activeTabSpec.id)
			: undefined;

	if (inSkeletonMode) {
		return <ActorDetailsSkeleton shimmer />;
	}

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
						<TabsList className="flex border-none h-full items-stretch min-w-0 overflow-hidden w-full">
							{displayedTabs.map((t) => (
								<WithTooltip
									key={t.id}
									delayDuration={0}
									disabled={showLabels}
									trigger={
										<TabsTrigger
											value={t.id}
											className={cn(
												"h-full text-xs px-2.5 py-0 min-w-0 shrink gap-1 isolate before:absolute before:inset-x-0.5 before:inset-y-1 before:-z-10 before:rounded-md before:transition-colors hover:before:bg-foreground/[0.06]",
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
			</div>
			<div className="relative flex-1 min-h-0 bg-card">
				{activeInspectorTabId && (
					<div className="absolute inset-0 flex">
						<InspectorTabContent
							actorId={actorId}
							activeTab={activeInspectorTabId}
						/>
					</div>
				)}
				{activeCloudTab && (
					<div className="absolute inset-0">
						{activeCloudTab.render(actorId)}
					</div>
				)}
			</div>
		</Tabs>
	);
}
