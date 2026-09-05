import {
	faCheck,
	faSpinnerThird,
	faTriangleExclamation,
	Icon,
} from "@rivet-gg/icons";
import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { formatDistance } from "date-fns";
import { Fragment } from "react";
import {
	Button,
	DiscreteCopyButton,
	Skeleton,
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
	Text,
	WithTooltip,
} from "@/components";
import {
	ErrorDetailsContent,
	useCloudProjectDataProvider,
} from "@/components/actors";

interface ImagesTableProps {
	isLoading?: boolean;
	isError?: boolean;
	hasNextPage?: boolean;
	fetchNextPage?: () => void;
	images: Image[];
	deployments: Deployment[];
	namespace: string;
}

interface Image {
	repository: string;
	tag: string;
	createdAt: string;
}

interface Deployment {
	repository?: string;
	namespace: string;
	tag?: string;
}

export function ImagesTable({
	isLoading,
	isError,
	hasNextPage,
	fetchNextPage,
	images,
	deployments,
	namespace,
}: ImagesTableProps) {
	return (
		<Table>
			<TableHeader>
				<TableRow>
					<TableHead className="pl-8">Tag</TableHead>
					<TableHead className="w-full whitespace-nowrap">
						Deployed To
					</TableHead>
					<TableHead className="min-w-56">Date</TableHead>
					<TableHead />
				</TableRow>
			</TableHeader>
			<TableBody>
				{!isLoading && !isError && images?.length === 0 ? (
					<TableRow>
						<TableCell colSpan={5}>
							<Text className="text-center">
								No images found.
							</Text>
						</TableCell>
					</TableRow>
				) : null}
				{isError ? (
					<TableRow>
						<TableCell colSpan={7}>
							<Text className="text-center">
								An error occurred while fetching images.
							</Text>
						</TableCell>
					</TableRow>
				) : null}
				{isLoading ? (
					<>
						<RowSkeleton />
						<RowSkeleton />
						<RowSkeleton />
						<RowSkeleton />
						<RowSkeleton />
						<RowSkeleton />
						<RowSkeleton />
						<RowSkeleton />
					</>
				) : null}
				{images?.map((image) => (
					<ImageRow
						{...image}
						deployments={deployments.filter(
							(deployment) =>
								deployment.repository === image.repository &&
								deployment.tag === image.tag,
						)}
						namespace={namespace}
						key={`${image.repository}:${image.tag}`}
					/>
				))}

				{!isLoading && hasNextPage ? (
					<TableRow>
						<TableCell colSpan={7} className="text-center py-2">
							<Button
								variant="ghost"
								size="sm"
								isLoading={isLoading}
								onClick={() => fetchNextPage?.()}
								disabled={!hasNextPage}
								className="text-muted-foreground"
							>
								Load more
							</Button>
						</TableCell>
					</TableRow>
				) : null}
			</TableBody>
		</Table>
	);
}

function RowSkeleton() {
	return (
		<TableRow>
			<TableCell>
				<Skeleton className="w-full h-4" />
			</TableCell>
			<TableCell>
				<Skeleton className="w-full h-4" />
			</TableCell>
			<TableCell>
				<Skeleton className="w-full h-4" />
			</TableCell>
		</TableRow>
	);
}

export function ImageRow({
	repository,
	tag,
	createdAt,
	deployments,
	namespace,
}: Image & {
	deployments: Deployment[];
	namespace: string;
}) {
	return (
		<TagRow
			key={tag}
			repository={repository}
			tag={tag}
			createTs={createdAt}
			deployments={deployments}
			namespace={namespace}
		/>
	);
}

function TagRow({
	repository,
	tag,
	createTs,
	deployments,
	namespace,
}: {
	repository: string;
	tag: string;
	createTs: string;
	deployments: Deployment[];
	namespace: string;
}) {
	const navigate = useNavigate();
	const isDeployedToCurrentNamespace = deployments.find(
		(d) =>
			d.namespace === namespace &&
			d.repository === repository &&
			d.tag === tag,
	);

	return (
		<TableRow>
			<TableCell className="size-8">
				<DiscreteCopyButton value={`${repository}:${tag}`}>
					{repository}:{tag}
				</DiscreteCopyButton>
			</TableCell>
			<TableCell>
				{deployments.length > 0 ? (
					deployments.map((deployment, i) => (
						<Fragment key={deployment.namespace}>
							{i > 0 ? ", " : null}
							<DeploymentNamespace
								namespace={deployment.namespace}
							/>
						</Fragment>
					))
				) : (
					<Text className="text-muted-foreground">-</Text>
				)}
			</TableCell>
			<TableCell>
				<CreateTs createTs={createTs} />
			</TableCell>
			<TableCell className="text-right">
				{!isDeployedToCurrentNamespace ? (
					<Button
						variant="outline"
						className="w-full"
						size="sm"
						onClick={() =>
							navigate({
								to: ".",
								search: (old) => ({
									...old,
									modal: "upsert-deployment",
									namespace,
									repository,
									tag,
								}),
							})
						}
					>
						Deploy
					</Button>
				) : (
					<ManagedPoolStatus
						namespace={isDeployedToCurrentNamespace.namespace}
					/>
				)}
			</TableCell>
		</TableRow>
	);
}

function ManagedPoolStatus({ namespace }: { namespace: string }) {
	const provider = useCloudProjectDataProvider();

	const { data } = useQuery({
		...provider.currentProjectManagedPoolQueryOptions({
			namespace,
			pool: "default",
			safe: true,
		}),
		refetchInterval: 5_000,
	});

	if (!data) {
		return null;
	}

	if (data.status === "ready") {
		return (
			<p className="text-center flex items-center justify-center">
				<Icon className="text-green-500 mr-1.5" icon={faCheck} />
				Current
			</p>
		);
	}

	if (data.status === "error") {
		return (
			<WithTooltip
				content={<ErrorDetailsContent error={data.error?.message} />}
				trigger={
					<p className="text-center flex items-center justify-center cursor-pointer">
						<Icon
							className="text-red-500 mr-1.5"
							icon={faTriangleExclamation}
						/>
						Error
					</p>
				}
			/>
		);
	}

	return (
		<p className="text-center flex items-center justify-center">
			<Icon className="mr-1.5 animate-spin" icon={faSpinnerThird} />
			Deploying...
		</p>
	);
}

function CreateTs({ createTs }: { createTs: string }) {
	return (
		<WithTooltip
			content={formatDistance(createTs, new Date(), { addSuffix: true })}
			trigger={
				<div className="text-muted-foreground">
					{new Date(createTs).toLocaleString()}
				</div>
			}
		/>
	);
}

function DeploymentNamespace({ namespace }: { namespace: string }) {
	const provider = useCloudProjectDataProvider();
	const { organization, project } = useParams({ strict: false }) as {
		organization: string;
		project: string;
	};
	const { data } = useQuery(
		provider.currentProjectNamespaceQueryOptions({ namespace }),
	);
	return (
		<Link
			to="/orgs/$organization/projects/$project/ns/$namespace"
			params={{ organization, project, namespace }}
			className="hover:underline"
		>
			{data?.displayName || <Skeleton className="w-6 h-4" />}
		</Link>
	);
}
