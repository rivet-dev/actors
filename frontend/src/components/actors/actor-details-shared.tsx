import type { Rivet } from "@rivet-gg/cloud";
import { useQuery } from "@tanstack/react-query";
import { type ReactNode, useLayoutEffect, useRef, useState } from "react";
import {
	DeploymentLogs,
	DeploymentLogsExportMenu,
} from "@/components/deployment-logs";
import { features } from "@/lib/features";
import { ActorConfigTab } from "./actor-config-tab";
import {
	useCloudNamespaceDataProvider,
	useDataProvider,
} from "./data-provider";
import type { InspectorTabDescriptor } from "./inspector-tab-registry";
import type { ActorId } from "./queries";

// Shared helpers used by both `ActorDetailsIframePath` (the iframe wrapper)
// and `ActorDetailsLegacy` (the dashboard-rendered fallback). The two paths
// differ in where the inspector runs, but they share the surrounding tab
// strip chrome: the same responsive-label logic, the same cloud-tab
// catalog, the same skeleton tab list while waiting for capabilities.

export const TAB_LABEL_THRESHOLD = 300; /* px */

/**
 * Hides the tab labels (icon-only) when the strip is narrower than
 * `TAB_LABEL_THRESHOLD`. Returns a ref to attach to the strip container.
 */
export function useShowTabLabels() {
	const ref = useRef<HTMLDivElement>(null);
	const [showLabels, setShowLabels] = useState(true);
	useLayoutEffect(() => {
		const el = ref.current;
		if (!el) return;
		const observer = new ResizeObserver(() => {
			setShowLabels(el.offsetWidth >= TAB_LABEL_THRESHOLD);
		});
		observer.observe(el);
		return () => observer.disconnect();
	}, []);
	return { ref, showLabels };
}

/**
 * Cloud-only: returns true when the current namespace has a managed pool
 * (i.e. deployment-logs are available). Always false in self-hosted mode.
 */
export function useHasManagedPool(): boolean {
	if (!features.compute) return false;
	// biome-ignore lint/correctness/useHookAtTopLevel: gated by build constant
	const provider = useCloudNamespaceDataProvider();
	// biome-ignore lint/correctness/useHookAtTopLevel: gated by build constant
	const { data } = useQuery({
		...provider.currentNamespaceHasManagedPoolQueryOptions(),
		staleTime: Infinity,
	});
	return data ?? false;
}

/**
 * Renders the deployment-logs tab. Sourced as its own component so it can read
 * the current project/namespace from the cloud namespace data provider. The
 * managed-pools logs endpoint is keyed by both, and this tab only ever renders
 * in cloud mode (gated by `hasManagedPool`), so the provider is always present.
 */
function DeploymentLogsTab({ actorId }: { actorId: ActorId }) {
	const provider = useCloudNamespaceDataProvider();
	const { data: actor } = useQuery(
		useDataProvider().actorQueryOptions(actorId),
	);
	const logsRef = useRef<Rivet.LogStreamEvent.Log[]>([]);
	// The actor's pool is its runner name selector; fall back to the default
	// pool until the actor loads or when it has no explicit selector.
	const pool = actor?.runnerNameSelector || "default";
	return (
		<div className="flex flex-col h-full">
			<div className="flex items-center justify-end border-b shrink-0 px-2 py-1">
				<DeploymentLogsExportMenu
					logsRef={logsRef}
					filename={`actor-logs-${actorId}.txt`}
					className="h-6 text-xs"
				/>
			</div>
			<div className="flex-1 min-h-0 overflow-hidden">
				<DeploymentLogs
					project={provider.project}
					namespace={provider.cloudNamespace}
					pool={pool}
					filter={`actor_id=${actorId}`}
					logsRef={logsRef}
				/>
			</div>
		</div>
	);
}

/** Dashboard-side tab (not part of the inspector iframe contract). */
export interface CloudTabSpec {
	id: string;
	label: string;
	icon: string;
	render: (actorId: ActorId) => ReactNode;
	shouldShow: (ctx: { hasManagedPool: boolean }) => boolean;
}

/**
 * Tabs the dashboard renders itself, next to the inspector tabs. These do NOT
 * live in the iframe. Two reasons a tab belongs here rather than in the
 * inspector bundle:
 *
 *   • it talks to an API unreachable from the engine origin (cloud-api, e.g.
 *     deployment logs), or
 *   • it owns actor lifecycle actions (sleep / reschedule / stop) that set
 *     auto-wake suppression. Suppression is read by `ActorsActorDetails` to
 *     decide whether to mount the inspector connection, so the control that
 *     sets it must run in the dashboard realm — not inside the cross-origin
 *     iframe, where the store would be a separate instance.
 *
 * The Metadata tab is the latter: it renders engine-API data and hosts the
 * lifecycle buttons, so it stays dashboard-side in both the iframe and legacy
 * paths.
 */
export const CLOUD_TABS: readonly CloudTabSpec[] = [
	{
		id: "deployment-logs",
		label: "Logs",
		icon: "logs",
		shouldShow: (ctx) => ctx.hasManagedPool,
		render: (actorId) => <DeploymentLogsTab actorId={actorId} />,
	},
	{
		id: "metadata",
		label: "Metadata",
		icon: "tag",
		shouldShow: () => true,
		render: (actorId) => <ActorConfigTab actorId={actorId} />,
	},
];

/**
 * Skeleton tab list rendered before the iframe's `tabs-available` arrives
 * (or before the legacy path knows the actor's capabilities). Mirrors what
 * the bundled inspector will eventually advertise so there's no layout
 * shift once real data lands.
 */
export const SKELETON_INSPECTOR_TABS: readonly InspectorTabDescriptor[] = [
	{ id: "workflow", label: "Workflow", icon: "workflow" },
	{ id: "database", label: "Database", icon: "database" },
	{ id: "state", label: "State", icon: "state" },
	{ id: "queue", label: "Queue", icon: "queue" },
	{ id: "schedules", label: "Schedules", icon: "calendar" },
	{ id: "connections", label: "Connections", icon: "plug" },
	{ id: "console", label: "Console", icon: "terminal" },
];
