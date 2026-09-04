import { faGear, Icon } from "@rivet-gg/icons";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Button, H1, ScrollArea, SmallText, WithTooltip } from "@/components";
import { useEngineNamespaceDataProvider } from "@/components/actors";
import { NoProvidersAlert } from "@/components/actors/no-providers-alert";
import { VisibilitySensor } from "@/components/visibility-sensor";
import { AddComponentButton, AddComponentCard } from "./add-component-card";
import { ActorBuildCard, ActorGridCardSkeleton } from "./actors-grid";

// Engine (OSS / enterprise) namespace landing shown when no Actor name is
// selected. This is the engine counterpart to the cloud `ActorsGrid`; keep the
// two visually in sync (they share `ActorBuildCard` / `ActorGridCardSkeleton`)
// so OSS and platform do not diverge. The cloud-only Deployments section and
// logs link are intentionally omitted here.
export function EngineNamespaceLanding() {
	const navigate = useNavigate();
	const dataProvider = useEngineNamespaceDataProvider();

	const { data: namespace } = useQuery(
		dataProvider.currentNamespaceQueryOptions(),
	);
	const namespaceName = namespace?.displayName ?? "Namespace";

	const {
		data: builds = [],
		isLoading,
		hasNextPage,
		isFetchingNextPage,
		fetchNextPage,
	} = useInfiniteQuery(dataProvider.buildsQueryOptions());

	const { data: runnerNamesCount = 0 } = useInfiniteQuery({
		...dataProvider.runnerNamesQueryOptions(),
		select: (data) => data.pages.flatMap((page) => page.names).length,
	});
	const { data: runnerConfigsCount = 0 } = useInfiniteQuery({
		...dataProvider.runnerConfigsQueryOptions(),
		select: (data) =>
			data.pages.flatMap((page) => Object.keys(page.runnerConfigs))
				.length,
	});
	const hasProviders = runnerNamesCount + runnerConfigsCount > 0;

	const sorted = [...builds].sort((a, b) => a.id.localeCompare(b.id));

	return (
		<div className="flex flex-1 min-h-0 my-2 mr-2 overflow-hidden rounded-xl border border-foreground/10 bg-card">
			<ScrollArea className="h-full w-full">
				<div className="px-6 py-6 max-w-6xl mx-auto space-y-8">
					<header className="flex items-center justify-between gap-4 pb-6 border-b border-foreground/10">
						<H1 className="text-2xl truncate">{namespaceName}</H1>
						<WithTooltip
							content="Namespace settings"
							trigger={
								<Button
									variant="outline"
									size="icon-sm"
									aria-label="Namespace settings"
									onClick={() =>
										navigate({
											to: ".",
											search: (old) => ({
												...(old as Record<
													string,
													unknown
												>),
												settings: "settings",
											}),
										})
									}
								>
									<Icon icon={faGear} />
								</Button>
							}
						/>
					</header>

					<section>
						<header className="flex items-center justify-between gap-4 mb-3">
							<h2 className="text-base font-semibold text-foreground">
								Actors
							</h2>
						</header>

						{isLoading ? (
							<div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-3">
								{Array.from({ length: 8 }).map((_, i) => (
									// biome-ignore lint/suspicious/noArrayIndexKey: skeleton loaders are static
									<ActorGridCardSkeleton key={i} />
								))}
							</div>
						) : builds.length === 0 ? (
							!hasProviders ? (
								<NoProvidersAlert variant="connect" />
							) : (
								<div className="flex flex-col items-center gap-3 rounded-md border border-dashed bg-card/50 px-6 py-10 text-center">
									<h3 className="text-base font-semibold text-foreground">
										No actors yet
									</h3>
									<SmallText className="text-muted-foreground max-w-md">
										Deploy code that registers an actor to
										see it here.
									</SmallText>
									<AddComponentButton />
								</div>
							)
						) : (
							<>
								<div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-3">
									{sorted.map((build) => (
										<ActorBuildCard
											key={build.id}
											build={build}
										/>
									))}
									<AddComponentCard />
									{isFetchingNextPage
										? Array.from({ length: 4 }).map(
												(_, i) => (
													// biome-ignore lint/suspicious/noArrayIndexKey: skeleton loaders are static
													<ActorGridCardSkeleton
														key={`next-${i}`}
													/>
												),
											)
										: null}
								</div>
								{hasNextPage && !isFetchingNextPage ? (
									<VisibilitySensor
										onChange={fetchNextPage}
									/>
								) : null}
							</>
						)}
					</section>
				</div>
			</ScrollArea>
		</div>
	);
}
