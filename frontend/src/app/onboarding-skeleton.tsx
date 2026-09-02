import type { ReactNode } from "react";
import { Skeleton } from "@/components";

// Matches the non-agentOS path (select -> local -> deploy); agentOS adds steps
// only after a product is picked, which is past this skeleton.
const STEP_COUNT = 3;
const PRODUCT_CARD_COUNT = 4;

function HeaderSkeleton() {
	return (
		<header className="z-20 flex items-center gap-3 h-12 px-3 shrink-0 border-b border-border bg-background">
			<Skeleton className="size-6 rounded-md" />
			<Skeleton className="h-4 w-48" />
		</header>
	);
}

function ProductCardSkeleton() {
	return (
		<div className="flex items-start gap-3 rounded-lg border border-border px-4 py-3">
			<Skeleton className="size-8 shrink-0 rounded-md" />
			<div className="min-w-0 flex-1 space-y-2">
				<Skeleton className="h-4 w-24" />
				<Skeleton className="h-3 w-full" />
			</div>
		</div>
	);
}

// Mirrors the `GettingStarted` wizard layout (centered card, stepper progress,
// step heading, product grid) so the pending UI matches the screen it resolves
// to instead of flashing the Actors grid skeleton.
export function OnboardingSkeleton({ header }: { header?: ReactNode }) {
	return (
		<div className="h-screen flex flex-col overflow-hidden bg-background">
			{header ?? <HeaderSkeleton />}
			<div className="flex-1 min-h-0 overflow-auto flex items-safe-center justify-center px-4 py-8">
				<div className="relative w-full max-w-[36rem] rounded-xl border bg-card p-6 sm:p-8 shadow-sm">
					<div className="mt-2">
						<div className="mb-6 flex flex-col gap-2">
							<div className="flex gap-1.5">
								{Array.from({ length: STEP_COUNT }).map(
									(_, i) => (
										<div
											// biome-ignore lint/suspicious/noArrayIndexKey: static skeleton bars
											key={i}
											className={
												i === 0
													? "h-1 flex-1 rounded-full bg-primary"
													: "h-1 flex-1 rounded-full bg-muted"
											}
										/>
									),
								)}
							</div>
							<div className="flex min-h-8 items-center">
								<Skeleton className="h-3 w-20" />
							</div>
						</div>

						<Skeleton className="h-6 w-56" />
						<Skeleton className="mt-2.5 h-4 w-3/4" />

						<div className="mt-6 grid grid-cols-1 gap-2 sm:grid-cols-2">
							{Array.from({ length: PRODUCT_CARD_COUNT }).map(
								(_, i) => (
									// biome-ignore lint/suspicious/noArrayIndexKey: static skeleton cards
									<ProductCardSkeleton key={i} />
								),
							)}
						</div>
						<Skeleton className="mt-3 h-3 w-2/3" />
					</div>
				</div>
			</div>
		</div>
	);
}
