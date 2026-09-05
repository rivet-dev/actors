import { Icon } from "@rivet-gg/icons";
import type { ReactNode } from "react";
import { cn, ShimmerLine, Tabs, TabsList, TabsTrigger } from "@/components";
import { resolveInspectorTabIcon } from "./inspector-tab-icons";

// Default placeholder set rendered while the iframe boots and for empty
// states (no actor selected, actor not found). Matches the bundled iframe's
// eventual tab list so there's no jarring layout swap once `tabs-available`
// arrives. All triggers are disabled.
const PLACEHOLDER_TABS: ReadonlyArray<{
	id: string;
	label: string;
	icon: string;
}> = [
	{ id: "workflow", label: "Workflow", icon: "workflow" },
	{ id: "database", label: "Database", icon: "database" },
	{ id: "state", label: "State", icon: "state" },
	{ id: "queue", label: "Queue", icon: "queue" },
	{ id: "connections", label: "Connections", icon: "plug" },
	{ id: "console", label: "Console", icon: "terminal" },
	{ id: "metadata", label: "Metadata", icon: "tag" },
];

interface Props {
	/**
	 * Optional shimmer line below the strip. Use when the underlying content
	 * is loading; omit for static empty states.
	 */
	shimmer?: boolean;
	/**
	 * Optional content rendered below the strip — typically a centered
	 * empty-state message ("select an actor", "actor not found", etc.).
	 */
	children?: ReactNode;
	className?: string;
}

/**
 * Disabled tab strip used by:
 *
 *   • the dashboard's iframe wrapper while waiting for `tabs-available`
 *   • the empty-state placeholder when no actor is selected
 *   • the not-found view when the actor doesn't exist
 *
 * Looks identical to the live iframe tab strip so the panel doesn't jump
 * once real content takes over.
 */
export function ActorDetailsSkeleton({ shimmer, children, className }: Props) {
	return (
		<Tabs
			value={undefined}
			className={cn("flex-1 min-h-0 min-w-0 flex flex-col", className)}
		>
			<div className="relative flex items-center border-b h-[45px]">
				<TabsList className="flex border-none h-full items-stretch min-w-0 overflow-hidden w-full">
					{PLACEHOLDER_TABS.map((t) => (
						<TabsTrigger
							key={t.id}
							value={t.id}
							disabled
							className="h-full text-xs px-2 py-0 min-w-0 shrink gap-1 opacity-60"
						>
							<Icon
								icon={resolveInspectorTabIcon(t.icon)}
								className="shrink-0"
							/>
							<span className="truncate">{t.label}</span>
						</TabsTrigger>
					))}
				</TabsList>
				{shimmer && (
					<ShimmerLine className="absolute bottom-0 left-0 right-0" />
				)}
			</div>
			{children}
		</Tabs>
	);
}
