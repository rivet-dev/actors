import { faGear, faLogs, Icon } from "@rivet-gg/icons";
import {
	queryOptions,
	useInfiniteQuery,
	useQueries,
	useQuery,
} from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { ImagesTable } from "@/app/images-table";
import {
	Button,
	cn,
	DiscreteCopyButton,
	H1,
	ScrollArea,
	Skeleton,
	SmallText,
	WithTooltip,
} from "@/components";
import {
	useCloudNamespaceDataProvider,
	useDataProvider,
} from "@/components/actors";
import { NoProvidersAlert } from "@/components/actors/no-providers-alert";
import { ActorIcon } from "@/components/lazy-icon";
import { VisibilitySensor } from "@/components/visibility-sensor";
import { features } from "@/lib/features";
import { getRivetRunUrl } from "../lib/env";
import { AddComponentButton, AddComponentCard } from "./add-component-card";
import { RouteLayout } from "./route-layout";

function _GridCard({
	children,
	className,
	asChild,
	onClick,
}: {
	children: ReactNode;
	className?: string;
	asChild?: boolean;
	onClick?: () => void;
}) {
	const Wrapper = asChild ? "div" : "button";
	return (
		<Wrapper
			onClick={onClick}
			className={cn(
				"group relative flex flex-col items-start gap-2 rounded-lg border border-foreground/10 bg-foreground/[0.02] p-4 text-left transition-colors",
				"hover:border-foreground/20 hover:bg-foreground/[0.05]",
				"focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
				"min-h-[110px]",
				className,
			)}
			type={asChild ? undefined : "button"}
		>
			{children}
		</Wrapper>
	);
}

export function ActorGridCardSkeleton() {
	return (
		<div className="flex min-h-[110px] flex-col items-start gap-2 rounded-lg border border-foreground/10 bg-foreground/[0.02] p-4">
			<Skeleton className="h-9 w-9 rounded-md" />
			<Skeleton className="h-4 w-24" />
			<Skeleton className="h-3 w-16" />
		</div>
	);
}

// Shared so the cloud `ActorsGrid` and the engine namespace landing render
// identical Actor cards. Keep both landings using this so OSS and platform do
// not visually diverge.
export function ActorBuildCard({
	build,
}: {
	// Shape produced by `buildsQueryOptions().select` (id + the `names` map
	// value), shared by the cloud and engine grids.
	build: { id: string; name: { metadata?: Record<string, unknown> } };
}) {
	const meta = build.name.metadata;
	const iconValue = typeof meta?.icon === "string" ? meta.icon : null;
	const displayName = typeof meta?.name === "string" ? meta.name : build.id;

	return (
		<Link
			to="."
			search={(old) => ({
				...old,
				actorId: undefined,
				actorKey: undefined,
				n: [build.id],
			})}
			className={cn(
				"group relative flex flex-col items-start gap-2 rounded-lg border border-foreground/10 bg-foreground/[0.02] p-4 text-left transition-colors",
				"hover:border-foreground/20 hover:bg-foreground/[0.05]",
				"focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
				"min-h-[110px] cursor-pointer",
			)}
		>
			<div className="flex h-9 w-9 items-center justify-center rounded-md bg-foreground/[0.06] text-foreground/80">
				<ActorIcon iconValue={iconValue} className="text-lg" />
			</div>
			<div className="font-medium text-sm leading-tight">
				{displayName}
			</div>
			{displayName !== build.id ? (
				<SmallText className="text-muted-foreground text-xs leading-tight">
					{build.id}
				</SmallText>
			) : null}
		</Link>
	);
}

export function ActorsGrid({ namespaceLabel }: { namespaceLabel?: string }) {
	const dataProvider = useDataProvider();
	const nsDataProvider = useCloudNamespaceDataProvider();
	const { organization, project, namespace } = useParams({
		strict: false,
	}) as {
		organization: string;
		project: string;
		namespace: string;
	};
	const navigate = useNavigate();
	const { data, isLoading, hasNextPage, fetchNextPage, isFetchingNextPage } =
		useInfiniteQuery(dataProvider.buildsQueryOptions());

	const { data: managedPool } = useQuery({
		...nsDataProvider.currentProjectManagedPoolQueryOptions({
			namespace,
			pool: "default",
			safe: true,
		}),
		enabled: features.compute,
	});
	const hasCompute = features.compute && managedPool != null;
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
	const hasRunners = runnerNamesCount + runnerConfigsCount > 0;

	const builds = data ?? [];
	const sorted = builds.toSorted((a, b) => a.id.localeCompare(b.id));

	const namespaceName = namespaceLabel ?? "Namespace";

	return (
		<div className="flex flex-1 min-h-0 my-2 mr-2 overflow-hidden rounded-xl border border-foreground/10 bg-card">
			<ScrollArea className="h-full w-full">
				<div className="px-6 py-6 max-w-6xl mx-auto space-y-8">
					<header className="flex items-center justify-between gap-4 pb-6 border-b border-foreground/10">
						<H1 className="text-2xl truncate">{namespaceName}</H1>
						<div className="flex items-center gap-2">
							{hasCompute ? (
								<Link
									to="/orgs/$organization/projects/$project/ns/$namespace/logs"
									params={{
										organization,
										project,
										namespace,
									}}
								>
									<Button
										variant="outline"
										size="sm"
										startIcon={<Icon icon={faLogs} />}
									>
										Logs
									</Button>
								</Link>
							) : null}
							<WithTooltip
								content="Namespace settings"
								trigger={
									<Button
										variant="outline"
										size="icon-sm"
										aria-label="Namespace settings"
										onClick={() => {
											void navigate({
												to: ".",
												search: (old) => ({
													...old,
													settings: "settings",
												}),
											});
										}}
									>
										<Icon icon={faGear} />
									</Button>
								}
							/>
						</div>
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
							!hasRunners ? (
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

					<DeploymentsSection />
				</div>
			</ScrollArea>
		</div>
	);
}

ActorsGrid.Skeleton = function ActorsGridSkeleton() {
	return (
		<div className="flex flex-1 min-h-0 my-2 mr-2 overflow-hidden rounded-xl border border-foreground/10 bg-card">
			<ScrollArea className="h-full w-full">
				<div className="px-6 py-6 max-w-6xl mx-auto space-y-8">
					<header className="flex items-center justify-between gap-4 pb-6 border-b border-foreground/10">
						<Skeleton className="h-8 w-48" />
						<Skeleton className="size-8 rounded-md" />
					</header>

					<section>
						<header className="flex items-center justify-between gap-4 mb-3">
							<Skeleton className="h-5 w-20" />
							<Skeleton className="h-8 w-32 rounded-md" />
						</header>

						<div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-3">
							{Array.from({ length: 8 }).map((_, i) => (
								// biome-ignore lint/suspicious/noArrayIndexKey: static skeleton cards
								<ActorGridCardSkeleton key={i} />
							))}
						</div>
					</section>
				</div>
			</ScrollArea>
		</div>
	);
};

export function NamespaceLandingPending() {
	return (
		<RouteLayout>
			<ActorsGrid.Skeleton />
		</RouteLayout>
	);
}

function DeploymentsSection() {
	const { organization, project, namespace } = useParams({
		strict: false,
	}) as {
		organization: string;
		project: string;
		namespace: string;
	};
	const dataProvider = useCloudNamespaceDataProvider();

	// Check if this namespace has a managed pool. If not, don't show deployments.
	// Once we know there's no pool, stop refetching to avoid 404 spam.
	const { data: managedPool, isLoading: isLoadingPool } = useQuery({
		...dataProvider.currentProjectManagedPoolQueryOptions({
			namespace,
			pool: "default",
			safe: true,
		}),
		enabled: features.compute,
		refetchInterval: (query) => (query.state.data === null ? false : 5_000),
		refetchOnWindowFocus: (query) => query.state.data !== null,
	});

	const hasPool = features.compute && managedPool != null;

	const {
		data: images,
		isError,
		isLoading: isLoadingImages,
	} = useInfiniteQuery({
		...dataProvider.currentProjectImagesQueryOptions({ limit: 5 }),
		enabled: hasPool,
		refetchInterval: 5_000,
	});

	const { data: namespaces } = useInfiniteQuery({
		...dataProvider.currentProjectNamespacesQueryOptions(),
		enabled: hasPool,
		refetchInterval: 5_000,
	});

	const managedPoolQueries = useQueries({
		queries: hasPool
			? (namespaces ?? []).map((ns) =>
					queryOptions({
						...dataProvider.currentProjectManagedPoolQueryOptions({
							namespace: ns.name,
							pool: "default",
							safe: true,
						}),
						select: (data) => ({
							...data,
							namespace: ns.name,
							...data?.config?.image,
						}),
						refetchInterval: 5_000,
					}),
				)
			: [],
	});

	const deployments = managedPoolQueries
		.map((query) => query.data)
		.filter(
			(data): data is Exclude<typeof data, undefined> =>
				data !== undefined,
		);

	const { data: nsData } = useQuery(
		dataProvider.currentProjectNamespaceQueryOptions({ namespace }),
	);

	const isDeployed =
		managedPool?.status === "ready" && managedPool?.config?.image != null;
	const deploymentUrl =
		isDeployed && nsData?.access?.engineNamespaceName
			? getRivetRunUrl(nsData.access.engineNamespaceName)
			: null;

	if (isLoadingPool || !hasPool) {
		return null;
	}

	const allImages = images ?? [];

	const sorted = allImages.toSorted((a, b) => {
		const aTimestamp = new Date(a.createdAt).getTime();
		const bTimestamp = new Date(b.createdAt).getTime();
		return bTimestamp - aTimestamp;
	});

	const hasMore = sorted.length >= 5;

	return (
		<section>
			<header className="mb-3">
				<h2 className="text-base font-semibold text-foreground">
					Deployments
				</h2>
			</header>
			{deploymentUrl ? (
				<div className="mb-3 flex items-center gap-2 text-sm">
					<span className="text-muted-foreground">
						Deployment URL
					</span>
					<DiscreteCopyButton
						value={deploymentUrl}
						className="font-mono text-xs text-muted-foreground"
					>
						{deploymentUrl}
					</DiscreteCopyButton>
				</div>
			) : null}
			<div className="border rounded-md">
				<ImagesTable
					images={sorted}
					deployments={deployments}
					isLoading={isLoadingImages}
					namespace={namespace}
					isError={isError}
				/>
				{hasMore ? (
					<Link
						to="/orgs/$organization/projects/$project/ns/$namespace/deployments"
						params={{ organization, project, namespace }}
						className="block border-t border-foreground/10 py-2 text-center text-sm text-muted-foreground hover:text-foreground transition-colors"
					>
						View all
					</Link>
				) : null}
			</div>
		</section>
	);
}
