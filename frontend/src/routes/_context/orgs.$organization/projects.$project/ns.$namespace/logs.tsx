import type { Rivet } from "@rivet-gg/cloud";
import { faPause, faPlay, Icon } from "@rivet-gg/icons";
import { useInfiniteQuery, useSuspenseQuery } from "@tanstack/react-query";
import {
	createFileRoute,
	Link,
	redirect,
	useNavigate,
} from "@tanstack/react-router";
import { startTransition, useRef, useState } from "react";
import { z } from "zod";
import { Content } from "@/app/layout";
import {
	PoolSwitcher,
	poolHeaderText,
	resolvePoolName,
} from "@/app/pool-switcher";
import { Button, H1, Skeleton } from "@/components";
import {
	useCloudNamespaceDataProvider,
	useDataProvider,
} from "@/components/actors";
import { RegionSelect } from "@/components/actors/region-select";
import {
	DeploymentLogs,
	DeploymentLogsExportMenu,
} from "@/components/deployment-logs";
import { features } from "@/lib/features";

export const Route = createFileRoute(
	"/_context/orgs/$organization/projects/$project/ns/$namespace/logs",
)({
	validateSearch: z.object({
		search: z.string().optional(),
		pool: z.string().optional(),
	}),
	component: RouteComponent,
	beforeLoad: ({ params }) => {
		if (!features.compute) {
			throw redirect({
				to: "/orgs/$organization/projects/$project/ns/$namespace",
				params,
			});
		}
	},
	loaderDeps: ({ search }) => ({ pool: search.pool }),
	loader: async ({ context, deps }) => {
		const dataProvider = context.dataProvider;
		// Prefetch the pool resolved from `?pool=` so a deep link to a non-default
		// pool doesn't refetch client-side after the loader settles.
		const pools = await context.queryClient.ensureQueryData(
			dataProvider.currentNamespaceManagedPoolsQueryOptions(),
		);
		await context.queryClient.prefetchQuery(
			dataProvider.currentNamespaceManagedPoolQueryOptions({
				pool: resolvePoolName(pools, deps.pool),
				safe: true,
			}),
		);
	},
	pendingComponent: DataLoadingPlaceholder,
});

function RouteComponent() {
	const params = Route.useParams();
	const { namespace, project } = params;
	const dataProvider = useCloudNamespaceDataProvider();
	const navigate = useNavigate();

	const { data: pools = [] } = useSuspenseQuery(
		dataProvider.currentNamespaceManagedPoolsQueryOptions(),
	);
	const { pool: poolParam, search: initialSearch } = Route.useSearch();
	const selectedPool = resolvePoolName(pools, poolParam);

	const { data: pool } = useSuspenseQuery(
		dataProvider.currentNamespaceManagedPoolQueryOptions({
			pool: selectedPool,
			safe: true,
		}),
	);

	const { data: datacenters = [] } = useInfiniteQuery(
		useDataProvider().datacentersQueryOptions(),
	);

	// Width all region labels to the longest datacenter name so messages line up.
	const regionLabelLength = datacenters.reduce(
		(max, dc) => Math.max(max, dc.name.length),
		0,
	);

	const [search, setSearch] = useState(initialSearch ?? "");
	const [isPaused, setIsPaused] = useState(false);
	const [region, setRegion] = useState<string>("all");
	const logsRef = useRef<Rivet.LogStreamEvent.Log[]>([]);

	if (!pool) {
		return (
			<Content>
				<div className="h-full flex flex-col items-center justify-center gap-3">
					<p className="text-muted-foreground">
						Logs are only accessible on namespaces running on Rivet
						Compute.
					</p>
					<Link
						to="/orgs/$organization/projects/$project/ns/$namespace"
						params={params}
					>
						<Button variant="outline" size="sm">
							Go back
						</Button>
					</Link>
				</div>
			</Content>
		);
	}

	return (
		<Content>
			<div className="flex flex-col h-full">
				<div className="pt-2 px-6 mx-auto w-full">
					<div className="flex justify-between items-center px-0 py-4">
						<div className="flex items-center gap-1.5">
							<H1>{poolHeaderText(pools, "Logs", "Logs")}</H1>
							{pools.length > 1 ? (
								<PoolSwitcher
									pools={pools}
									value={selectedPool}
									onChange={(name) =>
										navigate({
											to: ".",
											search: (old) => ({
												...old,
												pool: name,
											}),
										})
									}
								/>
							) : null}
						</div>
					</div>

					<p className="mb-6 text-muted-foreground">
						Monitor real-time logs from your deployments here.
					</p>
				</div>

				<div className="w-full border-t flex-1 flex flex-col min-h-0">
					<div className="flex items-stretch border-b	px-6 shrink-0">
						<div className="border-r flex flex-1">
							<input
								type="text"
								className="bg-transparent outline-none text-xs placeholder:text-muted-foreground font-sans flex-1 py-2"
								placeholder="Search logs..."
								spellCheck={false}
								defaultValue={initialSearch}
								onChange={(e) =>
									startTransition(() =>
										setSearch(e.target.value),
									)
								}
							/>
						</div>
						<RegionSelect
							onValueChange={setRegion}
							value={region}
							showAuto={false}
							showAllRegions={true}
							className="bg-transparent max-w-64 border-0 rounded-none"
						/>
						<div className="border-l flex items-center pl-4" />
						<Button
							variant="ghost"
							size="icon-sm"
							className="h-full rounded-none"
							onClick={() => setIsPaused((p) => !p)}
						>
							<Icon icon={isPaused ? faPlay : faPause} />
						</Button>
						<DeploymentLogsExportMenu
							logsRef={logsRef}
							filename={`deployment-logs-${namespace}.txt`}
							className="mx-1 h-full rounded-none"
						/>
					</div>

					{pool ? (
						<div className="flex-1 min-h-0 overflow-hidden">
							<DeploymentLogs
								project={project}
								namespace={namespace}
								pool={selectedPool}
								filter={search || undefined}
								region={region === "all" ? undefined : region}
								paused={isPaused}
								logsRef={logsRef}
								regionLabelLength={regionLabelLength}
							/>
						</div>
					) : (
						<div className="h-full flex flex-1 flex-col items-center justify-center">
							<p>No logs available.</p>
							<p className="text-muted-foreground text-xs mt-1">
								No active runner pool found. Logs will appear
								here once a deployment is active and running.
							</p>
							<p className="text-muted-foreground text-xs mt-1">
								If you just started a deployment, please allow a
								few moments for the logs to become available.
							</p>
						</div>
					)}
				</div>
			</div>
		</Content>
	);
}

function DataLoadingPlaceholder() {
	return (
		<div className="bg-card h-full border my-2 mr-2 rounded-lg">
			<div className="mt-2 flex justify-between items-center px-6 py-4 max-w-5xl mx-auto">
				<Skeleton className="w-48 h-8" />
			</div>
			<hr className="mb-4" />
			<div className="p-4 px-6 max-w-5xl mx-auto">
				<Skeleton className="w-full h-96 rounded-md" />
			</div>
		</div>
	);
}
