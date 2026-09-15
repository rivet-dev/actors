import { faExclamationTriangle, Icon } from "@rivet-gg/icons";
import { useQuery } from "@tanstack/react-query";
import { Link, useMatch } from "@tanstack/react-router";
import { AnimatePresence, motion } from "framer-motion";
import { useLayoutEffect } from "react";
import { Button, cn } from "@/components";
import { useCloudProjectDataProvider } from "@/components/actors";
import { PLAN_LABELS } from "@/content/billing";
import { features } from "@/lib/features";
import { useHighestUsagePercent } from "./hooks";

// Fixed banner height (Tailwind `h-9`). Published as a CSS variable so the
// settings drawer, a fixed overlay anchored under the top bar, can start below
// the banner instead of behind it.
const BANNER_HEIGHT = "2.25rem";

export function BillingLimitAlert() {
	if (!features.billing) return null;
	return <BillingLimitAlertGuard />;
}

// Billing is project-scoped, but this banner renders from the shared route
// layout. `useMatch` with `shouldThrow: false` keeps it out of routes where
// `useCloudProjectDataProvider` would throw, and waiting on `loaderData`
// avoids reading the provider before the project loader resolves.
function BillingLimitAlertGuard() {
	const projectMatch = useMatch({
		from: "/_context/orgs/$organization/projects/$project",
		shouldThrow: false,
	});

	if (!projectMatch?.loaderData) return null;

	return <BillingLimitAlertInner />;
}

function BillingLimitAlertInner() {
	const dataProvider = useCloudProjectDataProvider();
	const { data: billingData } = useQuery({
		...dataProvider.currentProjectBillingDetailsQueryOptions(),
	});

	const usagePercent = useHighestUsagePercent();
	const plan = billingData?.billing.activePlan || "free";
	const hidden = plan !== "free" || usagePercent < 80;

	// Layout effect so the drawer's `top` moves in the same paint the banner
	// mounts in, instead of a frame where the drawer still sits behind it.
	useLayoutEffect(() => {
		if (hidden) return;
		const root = document.documentElement;
		root.style.setProperty("--billing-banner-height", BANNER_HEIGHT);
		return () => {
			root.style.removeProperty("--billing-banner-height");
		};
	}, [hidden]);

	const atLimit = usagePercent >= 100;

	// The usage figure comes from a slow backend scan, so the banner appears well
	// after the page loads. Expanding it in keeps the content below from jumping.
	return (
		<AnimatePresence>
			{!hidden ? (
				<motion.div
					initial={{ height: 0, opacity: 0 }}
					animate={{ height: BANNER_HEIGHT, opacity: 1 }}
					exit={{ height: 0, opacity: 0 }}
					transition={{ duration: 0.25, ease: "easeOut" }}
					className={cn(
						// Above the settings drawer (`z-40`) so it stays visible
						// while the drawer's `top` catches up; below `z-50`
						// popovers, dropdowns, and dialogs.
						"relative z-[45] overflow-hidden border-b",
						atLimit
							? "border-destructive/60 bg-destructive/15"
							: "border-warning/60 bg-warning/10",
					)}
				>
					<div className="flex h-9 items-center gap-2 px-3 text-xs">
						<Icon
							icon={faExclamationTriangle}
							className={cn(
								"shrink-0",
								atLimit ? "text-destructive" : "text-warning",
							)}
						/>
						<p className="text-foreground font-medium">
							{atLimit
								? `${PLAN_LABELS[plan] ?? "Plan"} plan limit reached`
								: "Approaching your plan limit"}
						</p>
						<p className="text-muted-foreground min-w-0 truncate">
							{atLimit
								? "Upgrade your plan to avoid service interruptions."
								: `You have used ${usagePercent}% of your plan's free usage.`}
						</p>
						<Button
							size="sm"
							variant="ghost"
							className="ml-auto h-6 shrink-0 text-xs"
							asChild
						>
							<Link
								from="/orgs/$organization/projects/$project"
								to="/orgs/$organization/projects/$project/billing"
							>
								Upgrade
							</Link>
						</Button>
					</div>
				</motion.div>
			) : null}
		</AnimatePresence>
	);
}
