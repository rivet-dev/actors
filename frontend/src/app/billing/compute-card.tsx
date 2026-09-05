import {
	faArrowUpRight,
	faExclamationTriangle,
	faMemory,
	faMicrochip,
	faServer,
	Icon,
	type IconProp,
} from "@rivet-gg/icons";
import { Link } from "@tanstack/react-router";
import { cn, formatCurrency } from "@/components";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { COMPUTE } from "@/content/billing";

interface ComputeRate {
	icon: IconProp;
	label: string;
	rate: number;
	unit: string;
}

const COMPUTE_RATES: ComputeRate[] = [
	{
		icon: faMicrochip,
		label: "CPU",
		rate: COMPUTE.cpuPerVcpuSecond,
		unit: "per vCPU-second",
	},
	{
		icon: faMemory,
		label: "Memory",
		rate: COMPUTE.memoryPerGibSecond,
		unit: "per GiB-second",
	},
];

/** Renders the per-second compute rate, e.g. `$0.0000330`. */
function formatRate(rate: number): string {
	return `$${rate.toFixed(7)}`;
}

const CAPS_NOTE = `Billed per active second. Up to ${COMPUTE.maxVcpu} vCPU (Free plan is limited to ${COMPUTE.freeMaxVcpu} vCPU and capped at $5/month of compute). One vCPU is half a physical core. You can also bring your own compute and run your actors and applications on AWS, Vercel, Railway, or bare metal, paid directly to your provider.`;

interface ComputeUsageCardProps {
	/** Month-to-date compute cost in dollars. Omitted shows pricing only. */
	monthToDate?: number;
	isLoading?: boolean;
	isError?: boolean;
	/**
	 * Compute spend as a percentage of the Free plan's $5 budget. Drives the
	 * over-limit callout. Omitted / 0 on paid plans, which have no compute cap.
	 */
	budgetPercent?: number;
}

/** Compute pricing card with month-to-date spend and free-budget warning. */
export function ComputeUsageCard({
	monthToDate,
	isLoading,
	isError,
	budgetPercent,
}: ComputeUsageCardProps = {}) {
	const overBudget = budgetPercent !== undefined && budgetPercent >= 100;
	const nearBudget =
		budgetPercent !== undefined &&
		budgetPercent >= 80 &&
		budgetPercent < 100;
	return (
		<Card className="w-full border border-border bg-card shadow-sm">
			<CardHeader className="flex flex-row items-start justify-between space-y-0 pb-4">
				<div className="flex items-start gap-3">
					<div className="flex size-7 items-center justify-center rounded-md border border-border">
						<Icon
							icon={faServer}
							className="size-3.5 text-foreground"
						/>
					</div>
					<div>
						<h3 className="text-base font-semibold text-foreground">
							Compute
						</h3>
						<p className="text-sm text-muted-foreground">
							Run your actors and applications on Rivet Compute,
							billed per active second by configured CPU and
							memory.
						</p>
					</div>
				</div>
				{monthToDate !== undefined && (
					<div className="text-right ml-6">
						<div className="text-2xl font-semibold text-foreground">
							{isLoading ? (
								<Skeleton className="h-8 w-20" />
							) : isError ? (
								"—"
							) : (
								formatCurrency(monthToDate)
							)}
						</div>
						<div className="text-sm text-muted-foreground">
							Month-to-date
						</div>
					</div>
				)}
			</CardHeader>
			<CardContent className="pt-0">
				{(overBudget || nearBudget) && (
					<div
						className={cn(
							"mb-4 flex items-start gap-2 rounded-lg border px-4 py-3 text-sm",
							overBudget
								? "border-destructive/60 bg-destructive/10 text-destructive"
								: "border-warning/60 bg-warning/10 text-foreground",
						)}
					>
						<Icon
							icon={faExclamationTriangle}
							className={cn(
								"mt-0.5 shrink-0",
								overBudget
									? "text-destructive"
									: "text-warning",
							)}
						/>
						<div>
							<p className="font-medium">
								{overBudget
									? "Compute over your free limit"
									: "Approaching your free compute limit"}
							</p>
							<p className="text-muted-foreground">
								{overBudget
									? `You've used ${monthToDate !== undefined ? formatCurrency(monthToDate) : "over"} of your $5.00 monthly compute allowance. Upgrade your plan to avoid service interruptions.`
									: `You've used ${Math.round(budgetPercent ?? 0)}% of your $5.00 monthly compute allowance.`}
							</p>
						</div>
					</div>
				)}
				<div className="rounded-lg border border-border bg-muted/30 divide-y divide-border">
					{COMPUTE_RATES.map((rate) => (
						<div
							key={rate.label}
							className="flex items-center justify-between px-5 py-3.5"
						>
							<div className="flex items-center gap-3">
								<Icon
									icon={rate.icon}
									className="size-4 text-muted-foreground"
								/>
								<span className="text-sm font-medium text-foreground">
									{rate.label}
								</span>
							</div>
							<div className="text-right">
								<span className="text-sm tabular-nums font-medium text-foreground">
									{formatRate(rate.rate)}
								</span>
								<span className="ml-2 text-xs text-muted-foreground">
									{rate.unit}
								</span>
							</div>
						</div>
					))}
				</div>
				<p className="mt-3 text-xs text-muted-foreground leading-relaxed">
					{CAPS_NOTE}
				</p>
			</CardContent>
		</Card>
	);
}

/**
 * Pointer shown on namespace surfaces. Compute is billed per project, so the
 * namespace billing/metrics views direct users to Project Billing rather than
 * showing a (namespace-less) compute figure. The link opens the project billing
 * drawer on the current route.
 */
export function ComputeUsageProjectBillingPointer() {
	return (
		<Card className="w-full border border-border bg-card shadow-sm">
			<CardHeader className="flex flex-row items-start justify-between space-y-0 pb-4">
				<div className="flex items-start gap-3">
					<div className="flex size-7 items-center justify-center rounded-md border border-border">
						<Icon
							icon={faServer}
							className="size-3.5 text-foreground"
						/>
					</div>
					<div>
						<h3 className="text-base font-semibold text-foreground">
							Compute
						</h3>
						<p className="text-sm text-muted-foreground">
							Compute is billed per project, not per namespace.
						</p>
					</div>
				</div>
			</CardHeader>
			<CardContent className="pt-0">
				<Link
					to="."
					search={(prev) => ({ ...prev, settings: "billing" })}
					className="inline-flex items-center gap-1.5 text-sm font-medium text-primary hover:underline"
				>
					See Compute Usage on Project Billing
					<Icon icon={faArrowUpRight} className="size-3" />
				</Link>
			</CardContent>
		</Card>
	);
}
