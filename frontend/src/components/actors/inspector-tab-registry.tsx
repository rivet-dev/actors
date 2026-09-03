import { faSpinnerThird, Icon } from "@rivet-gg/icons";
import { useQuery } from "@tanstack/react-query";
import { lazy, type ReactNode, Suspense, useMemo } from "react";
import { useActorInspector } from "./actor-inspector-context";
import type { ActorId } from "./queries";

// Tab components are lazy-loaded so their heavy dependencies (xyflow + recharts
// for the workflow graph, codemirror + shiki for the state/database editors,
// the SQLite wasm module, the console REPL worker) split into per-tab chunks
// instead of bloating the inspector shell. Opening a tab fetches only that
// tab's code. lazy() expects a default export, so map the named ones.
const ActorWorkflowTab = lazy(() =>
	import("./workflow/actor-workflow-tab").then((m) => ({
		default: m.ActorWorkflowTab,
	})),
);
const ActorDatabaseTab = lazy(() =>
	import("./actor-db-tab").then((m) => ({ default: m.ActorDatabaseTab })),
);
const ActorStateTab = lazy(() =>
	import("./actor-state-tab").then((m) => ({ default: m.ActorStateTab })),
);
const ActorQueueTab = lazy(() =>
	import("./actor-queue-tab").then((m) => ({ default: m.ActorQueueTab })),
);
const ActorSchedulesTab = lazy(() =>
	import("./actor-schedules-tab").then((m) => ({
		default: m.ActorSchedulesTab,
	})),
);
const ActorConnectionsTab = lazy(() =>
	import("./actor-connections-tab").then((m) => ({
		default: m.ActorConnectionsTab,
	})),
);
const ActorConsoleFull = lazy(() =>
	import("./console/actor-console").then((m) => ({
		default: m.ActorConsoleFull,
	})),
);
const ActorWorkerContextProvider = lazy(() =>
	import("./worker/actor-worker-context").then((m) => ({
		default: m.ActorWorkerContextProvider,
	})),
);

// Public descriptor crossed over the postMessage bridge by the iframe.
export type InspectorTabDescriptor = {
	id: string;
	label: string;
	icon: string;
	/**
	 * `true` for author-shipped custom tabs (rendered at
	 * `/inspector/custom-tabs/<id>/`); absent or `false` for built-in
	 * tabs the SPA renders. Optional so older SPAs that omit the flag
	 * remain compatible — the dashboard treats them as built-in.
	 */
	isCustom?: boolean;
};

// Capability snapshot the registry consults to decide which tabs are
// available for the currently connected actor. Sourced from the inspector
// context once the WS Init message has populated the relevant queries.
type ActorCapabilities = {
	isWorkflowEnabled: boolean;
	isDatabaseEnabled: boolean;
	isStateEnabled: boolean;
	isQueueSupported: boolean;
	isSchedulesSupported: boolean;
};

interface TabRegistration {
	descriptor: InspectorTabDescriptor;
	available: (caps: ActorCapabilities) => boolean;
	render: (actorId: ActorId) => ReactNode;
}

// Tab list — preserved order matches the dashboard tab strip. Adding a new
// inspector tab here automatically advertises it to the dashboard (in the
// iframe path) and renders it inline (in the legacy path).
export const INSPECTOR_TAB_REGISTRATIONS: readonly TabRegistration[] = [
	{
		descriptor: { id: "workflow", label: "Workflow", icon: "workflow" },
		available: (caps) => caps.isWorkflowEnabled,
		render: (actorId) => <ActorWorkflowTab actorId={actorId} />,
	},
	{
		descriptor: { id: "database", label: "Database", icon: "database" },
		available: (caps) => caps.isDatabaseEnabled,
		render: (actorId) => <ActorDatabaseTab actorId={actorId} />,
	},
	{
		descriptor: { id: "state", label: "State", icon: "state" },
		available: (caps) => caps.isStateEnabled,
		render: (actorId) => <ActorStateTab actorId={actorId} />,
	},
	{
		descriptor: { id: "queue", label: "Queue", icon: "queue" },
		available: (caps) => caps.isQueueSupported,
		render: (actorId) => <ActorQueueTab actorId={actorId} />,
	},
	{
		descriptor: {
			id: "schedules",
			label: "Schedules",
			icon: "calendar",
		},
		available: (caps) => caps.isSchedulesSupported,
		render: (actorId) => <ActorSchedulesTab actorId={actorId} />,
	},
	{
		descriptor: { id: "connections", label: "Connections", icon: "plug" },
		available: () => true,
		render: (actorId) => <ActorConnectionsTab actorId={actorId} />,
	},
	{
		descriptor: { id: "console", label: "Console", icon: "terminal" },
		available: () => true,
		render: (actorId) => (
			<ActorWorkerContextProvider actorId={actorId}>
				<ActorConsoleFull actorId={actorId} />
			</ActorWorkerContextProvider>
		),
	},
] as const;

const availableInspectorRegistrations = __MCP_APP__
	? INSPECTOR_TAB_REGISTRATIONS.filter(
			(registration) => registration.descriptor.id !== "console",
		)
	: INSPECTOR_TAB_REGISTRATIONS;

/**
 * Returns the descriptors of all inspector tabs available for this actor,
 * filtered by the live capability flags from the inspector context. Returns
 * `null` while the inspector hasn't connected yet — callers should treat
 * that as "don't know yet" rather than "no tabs available".
 *
 * Must be called inside an `ActorInspectorProvider`.
 */
export function useAvailableInspectorTabs(
	actorId: ActorId,
): InspectorTabDescriptor[] | null {
	const inspector = useActorInspector();

	const { data: isWorkflowEnabled = false } = useQuery(
		inspector.actorIsWorkflowEnabledQueryOptions(actorId),
	);
	const { data: isDatabaseEnabled = false } = useQuery(
		inspector.actorDatabaseEnabledQueryOptions(actorId),
	);
	const { data: stateData } = useQuery(
		inspector.actorStateQueryOptions(actorId),
	);
	const isStateEnabled = stateData?.isEnabled ?? false;
	const isQueueSupported = inspector.features.queue.supported;
	const isSchedulesSupported = inspector.features.schedules.supported;

	// Tab config arrives with the WS `Init` message (same tick as the
	// capability flags below), so the empty default only shows before the
	// socket connects. No separate fetch is made.
	const { data: tabConfig } = useQuery(
		inspector.actorTabConfigQueryOptions(actorId),
	);

	// Wait for the WS `Init` message, not just metadata success. The
	// capability flags (workflow/state/database enabled) arrive with `Init`;
	// gating on metadata alone emits a list missing those tabs and then
	// re-adds them once `Init` lands, which reads as a flash.
	const { data: capabilitiesKnown = false } = useQuery(
		inspector.actorInspectorInitializedQueryOptions(actorId),
	);

	return useMemo(() => {
		if (!capabilitiesKnown) return null;
		const caps: ActorCapabilities = {
			isWorkflowEnabled,
			isDatabaseEnabled,
			isStateEnabled,
			isQueueSupported,
			isSchedulesSupported,
		};
		const hideSet = new Set(
			(tabConfig?.tabs ?? [])
				.filter((t) => t.hidden === true)
				.map((t) => t.id),
		);
		const builtIns = availableInspectorRegistrations
			.filter((t) => t.available(caps))
			.map((t) => t.descriptor)
			.filter((d) => !hideSet.has(d.id));
		const customs: InspectorTabDescriptor[] = (
			__MCP_APP__ ? [] : (tabConfig?.tabs ?? [])
		)
			.filter((t) => t.hidden !== true && typeof t.label === "string")
			.map((t) => ({
				id: t.id,
				// biome-ignore lint/style/noNonNullAssertion: filter above guarantees label is a string
				label: t.label!,
				// Author-supplied icon id; falls back to a sentinel that
				// resolveInspectorTabIcon will not match, producing the
				// generic faQuestionSquare glyph.
				icon: t.icon ?? "custom-tab",
				// Flag every author-declared tab so the dashboard can
				// route the iframe `src` correctly without needing its
				// own knowledge of the built-in id set.
				isCustom: true,
			}));
		return [...builtIns, ...customs];
	}, [
		capabilitiesKnown,
		isWorkflowEnabled,
		isDatabaseEnabled,
		isStateEnabled,
		isQueueSupported,
		isSchedulesSupported,
		tabConfig,
	]);
}

/**
 * Renders the active inspector tab's content. Returns `null` if the active
 * tab id isn't in the registry (unknown tab, or no tab selected).
 *
 * Must be called inside an `ActorInspectorProvider`.
 */
export function InspectorTabContent({
	actorId,
	activeTab,
}: {
	actorId: ActorId;
	activeTab: string | undefined;
}) {
	const registration = availableInspectorRegistrations.find(
		(t) => t.descriptor.id === activeTab,
	);
	if (!registration) return null;
	return (
		<div className="flex flex-col h-full flex-1 min-h-0">
			<Suspense fallback={<TabLoadingFallback />}>
				{registration.render(actorId)}
			</Suspense>
		</div>
	);
}

function TabLoadingFallback() {
	return (
		<div className="flex flex-1 items-center justify-center h-full">
			<Icon
				icon={faSpinnerThird}
				className="animate-spin text-muted-foreground"
			/>
		</div>
	);
}
