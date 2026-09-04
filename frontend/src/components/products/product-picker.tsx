import type { ReactNode } from "react";
import { Badge } from "@/components/ui/badge";
import {
	getOnboardingTargetCopy,
	type OnboardingTarget,
} from "@/content/agent-prompts";
import { features } from "@/lib/features";
import { publicUrl } from "@/lib/utils";

type Product = {
	target: OnboardingTarget;
	label: string;
	description: string;
	markFileName: string;
	badge?: string;
	isAvailable: () => boolean;
};

const PRODUCTS: Product[] = [
	{
		target: "actor",
		label: "Actors",
		description: "The primitive for realtime, stateful workloads",
		markFileName: "actors-mark.svg",
		isAvailable: () => true,
	},
	{
		target: "agent-os",
		label: "agentOS",
		description: "Hand every agent a computer of its own",
		markFileName: "agentos-mark.svg",
		isAvailable: () => features.agentOs,
	},
	{
		target: "workflows",
		label: "Workflows",
		description: "Write multi-step operations that survive restarts",
		markFileName: "workflows-mark.svg",
		isAvailable: () => true,
	},
	{
		target: "dynamic-apps",
		label: "Dynamic Apps",
		description: "Deploy AI-generated apps for your users",
		markFileName: "dynamic-apps-mark.svg",
		badge: "Preview",
		isAvailable: () => true,
	},
];

export function getAvailableProducts() {
	return PRODUCTS.filter((p) => p.isAvailable());
}

export function getProductDocsUrl(target: OnboardingTarget) {
	return getOnboardingTargetCopy(target).quickstartUrl;
}

export function ProductMark({ fileName }: { fileName: string }) {
	return (
		<img
			src={publicUrl(`images/brand/${fileName}`)}
			alt=""
			aria-hidden="true"
			className="size-8"
			draggable={false}
		/>
	);
}

export function ProductCard({
	icon,
	label,
	description,
	badge,
	onSelect,
}: {
	icon: ReactNode;
	label: string;
	description: string;
	badge?: string;
	onSelect: () => void;
}) {
	return (
		<button
			type="button"
			onClick={onSelect}
			className="flex items-start gap-3 rounded-lg border border-border px-4 py-3 text-left transition-colors cursor-pointer hover:border-primary hover:bg-primary/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
		>
			<span className="mt-0.5 shrink-0">{icon}</span>
			<div className="min-w-0">
				<div className="flex items-center gap-2">
					<p className="text-sm font-medium whitespace-nowrap">
						{label}
					</p>
					{badge ? (
						<Badge
							variant="outline"
							className="shrink-0 text-[10px] leading-none py-0.5 px-1.5 font-medium"
						>
							{badge}
						</Badge>
					) : null}
				</div>
				<p className="text-xs text-muted-foreground">{description}</p>
			</div>
		</button>
	);
}

export const PRODUCT_COMPOSABILITY_NOTE =
	"Rivet is composable. Start with one product and add the rest to the same project whenever you need them.";

export function ProductPicker({
	onSelect,
	ariaLabel = "Select a product",
}: {
	onSelect: (target: OnboardingTarget) => void;
	ariaLabel?: string;
}) {
	return (
		<div>
			<fieldset
				aria-label={ariaLabel}
				className="m-0 min-w-0 border-0 p-0 grid grid-cols-1 gap-2 sm:grid-cols-2"
			>
				{getAvailableProducts().map((product) => (
					<ProductCard
						key={product.target}
						icon={<ProductMark fileName={product.markFileName} />}
						label={product.label}
						description={product.description}
						badge={product.badge}
						onSelect={() => onSelect(product.target)}
					/>
				))}
			</fieldset>
			<p className="mt-2 text-xs text-muted-foreground">
				{PRODUCT_COMPOSABILITY_NOTE}
			</p>
		</div>
	);
}
