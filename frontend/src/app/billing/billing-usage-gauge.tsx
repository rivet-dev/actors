import { faExclamationTriangle, Icon } from "@rivet-gg/icons";
import { useQuery } from "@tanstack/react-query";
import { cn, WithTooltip } from "@/components";
import { useCloudProjectDataProvider } from "@/components/actors";
import { useHighestUsagePercent } from "./hooks";

const progressColors = {
	low: {
		stroke: "stroke-emerald-500",
		background: "bg-emerald-950/50",
		border: "border-emerald-900/30",
	},
	medium: {
		stroke: "stroke-warning",
		background: "bg-warning/10",
		border: "border-warning/60",
	},
	warning: {
		stroke: "stroke-amber-500",
		background: "bg-amber-950/50",
		border: "border-amber-900/30",
	},
	high: {
		stroke: "stroke-destructive",
		background: "bg-destructive/20",
		border: "border-destructive/60",
	},
};

const radius = 9;
const circumference = 2 * Math.PI * radius;

export function BillingUsageGauge() {
	const progress = useHighestUsagePercent();

	const dataProvider = useCloudProjectDataProvider();
	const { data: billingData } = useQuery({
		...dataProvider.currentProjectBillingDetailsQueryOptions(),
	});

	const plan = billingData?.billing.activePlan || "free";

	if (
		(plan !== "free" && progress < 80) ||
		(plan === "free" && progress < 50)
	) {
		return null;
	}

	const strokeDashoffset = circumference - (progress / 100) * circumference;

	const progressVariant =
		progress < 50
			? "low"
			: progress < 80
				? "medium"
				: progress < 100
					? "warning"
					: "high";

	return (
		<WithTooltip
			content={
				progress >= 100
					? plan === "free"
						? "You have reached your included usage limit. Upgrade your plan."
						: "You have reached 100% of your included usage."
					: `You have used ${progress}% of your included usage.`
			}
			trigger={
				<div
					className={cn(
						// Sized and margined to match `Badge`, which sits next to it
						// in the billing row. Badge is 22px tall (text-xs leading +
						// py-0.5 + border), so `size-6` made the circle read as
						// oversized against it.
						"rounded-full border size-[22px] my-px flex items-center justify-center",
						progressColors[progressVariant].border,
						progressColors[progressVariant].background,
					)}
				>
					{progress >= 100 ? (
						<Icon
							icon={faExclamationTriangle}
							className="text-destructive text-xs"
						/>
					) : (
						<svg
							className="size-full"
							viewBox="0 0 24 24"
							fill="none"
							strokeWidth="3"
							strokeLinecap="round"
							aria-label="Usage Gauge"
							role="img"
						>
							<circle cx="12" cy="12" r={radius} />
							<circle
								cx="12"
								cy="12"
								r={radius}
								className={
									progressColors[progressVariant].stroke
								}
								strokeDasharray={circumference}
								strokeDashoffset={strokeDashoffset}
								transform="rotate(-90 12 12)"
							/>
						</svg>
					)}
				</div>
			}
		/>
	);
}
