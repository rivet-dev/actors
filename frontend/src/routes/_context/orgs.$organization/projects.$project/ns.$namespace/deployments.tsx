import {
	queryOptions,
	useQueries,
	useSuspenseInfiniteQuery,
	useSuspenseQuery,
} from "@tanstack/react-query";
import { createFileRoute, Link, redirect } from "@tanstack/react-router";
import { ImagesTable } from "@/app/images-table";
import { Content } from "@/app/layout";
import { Button, H1, H2, Skeleton } from "@/components";
import { useCloudNamespaceDataProvider } from "@/components/actors";
import { features } from "@/lib/features";

export const Route = createFileRoute(
	"/_context/orgs/$organization/projects/$project/ns/$namespace/deployments",
)({
	component: RouteComponent,
	beforeLoad: ({ params }) => {
		if (!features.compute) {
			throw redirect({
				to: "/orgs/$organization/projects/$project/ns/$namespace",
				params,
			});
		}
	},
	loader: async ({ context }) => {
		const dataProvider = context.dataProvider;
		await context.queryClient.prefetchQuery(
			dataProvider.currentNamespaceManagedPoolQueryOptions({
				pool: "default",
				safe: true,
			}),
		);
	},
	loaderDeps() {
		return [];
	},
	pendingComponent: DataLoadingPlaceholder,
});

function RouteComponent() {
	const dataProvider = useCloudNamespaceDataProvider();
	const { data: pool } = useSuspenseQuery(
		dataProvider.currentNamespaceManagedPoolQueryOptions({
			pool: "default",
			safe: true,
		}),
	);

	if (!pool) {
		return (
			<Content>
				<div className="h-full flex flex-col items-center justify-center gap-3">
					<p className="text-muted-foreground">
						Deployments are only accessible on namespaces running on
						Rivet Compute.
					</p>
					<Link
						to="/orgs/$organization/projects/$project/ns/$namespace"
						params={Route.useParams()}
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
			<div className=" ">
				<div className="mb-4 pt-2 max-w-5xl mx-auto">
					<div className="flex justify-between items-center px-6 @6xl:px-0 py-4 ">
						<H1>Deployments</H1>
					</div>
					<p className="max-w-5xl mb-6 px-6 @6xl:px-0 text-muted-foreground">
						Deployments are Docker images that can be deployed as
						your Runners.
					</p>
				</div>

				<hr className="mb-6" />

				<div className="px-4">
					<Deployments />
				</div>
			</div>
		</Content>
	);
}

function Deployments() {
	const { namespace } = Route.useParams();
	const dataProvider = useCloudNamespaceDataProvider();
	const {
		data: images,
		isError,
		isLoading: isLoadingImages,
		fetchNextPage,
		hasNextPage,
	} = useSuspenseInfiniteQuery({
		...dataProvider.currentProjectImagesQueryOptions({ limit: 20 }),
		refetchInterval: 5_000,
	});

	const { data: namespaces } = useSuspenseInfiniteQuery({
		...dataProvider.currentProjectNamespacesQueryOptions(),
		refetchInterval: 5_000,
	});

	const managedPoolQueries = useQueries({
		queries:
			namespaces.map((ns) =>
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
			) ?? [],
	});

	const deployments = managedPoolQueries
		.map((query) => query.data)
		.filter(
			(data): data is Exclude<typeof data, undefined> =>
				data !== undefined,
		);

	const sorted = images.toSorted((a, b) => {
		const aTimestamp = new Date(a.createdAt).getTime();
		const bTimestamp = new Date(b.createdAt).getTime();
		return bTimestamp - aTimestamp;
	});

	return (
		<div className="max-w-7xl mx-auto px-6">
			<div className="border rounded-md">
				<ImagesTable
					images={sorted}
					deployments={deployments}
					isLoading={isLoadingImages}
					namespace={namespace}
					isError={isError}
					fetchNextPage={fetchNextPage}
					hasNextPage={hasNextPage}
				/>
			</div>
		</div>
	);
}

function DataLoadingPlaceholder() {
	return (
		<div className="bg-card h-full border my-2 mr-2 rounded-lg">
			<div className="mt-2 flex justify-between items-center px-6 py-4 max-w-5xl mx-auto">
				<H2 className="mb-2">
					<Skeleton className="w-48 h-8" />
				</H2>
			</div>
			<p className="max-w-5xl mb-6 px-6 text-muted-foreground mx-auto">
				<Skeleton className="w-full h-4" />
			</p>
			<hr className="mb-4" />
			<div className="p-4 px-6 max-w-5xl mx-auto ">
				<Skeleton className="h-8 w-48 mb-2" />
				<Skeleton className="h-6 w-72 mb-6" />
				<div className="flex flex-wrap gap-2 my-4">
					<Skeleton className="w-full h-20 rounded-md" />
					<Skeleton className="w-full h-20 rounded-md" />
					<Skeleton className="w-full h-20 rounded-md" />
				</div>
			</div>
			<div className="p-4 px-6 max-w-5xl mx-auto">
				<Skeleton className="h-8 w-48 mb-2" />
				<Skeleton className="h-6 w-72 mb-6" />
				<div className="flex flex-wrap gap-2 my-4">
					<Skeleton className="w-full h-20 rounded-md" />
					<Skeleton className="w-full h-20 rounded-md" />
					<Skeleton className="w-full h-20 rounded-md" />
				</div>
			</div>
		</div>
	);
}
