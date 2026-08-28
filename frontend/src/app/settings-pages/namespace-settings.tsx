import { faPlus, faTrash, faTriangleExclamation, Icon } from "@rivet-gg/icons";
import {
	useInfiniteQuery,
	usePrefetchInfiniteQuery,
	useQuery,
} from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Suspense } from "react";
import { ProviderDropdown } from "@/app/provider-dropdown";
import { RunnerConfigsTable } from "@/app/runner-config-table";
import { RunnersTable } from "@/app/runners-table";
import { Button, Code, Ping, Skeleton, SmallText } from "@/components";
import {
	ActorRegion,
	useCloudNamespaceDataProvider,
	useDataProvider,
	useEngineCompatDataProvider,
} from "@/components/actors";
import { NoProvidersAlert } from "@/components/actors/no-providers-alert";
import { features } from "@/lib/features";
import {
	CloudApiTokens,
	ConnectionTokens,
	PublishableToken,
	SecretToken,
} from "@/routes/_context/orgs.$organization/projects.$project/ns.$namespace/tokens";
import { McpConnection } from "./mcp-connection";
import { SettingsCard } from "./settings-card";

export function NamespaceSettingsContent() {
	return (
		<div className="space-y-4">
			<NoRunnersAlert />
			<Providers />
			<Runners />
		</div>
	);
}

export function NamespaceAdvancedContent() {
	return (
		<Suspense
			fallback={
				<div className="space-y-3">
					<Skeleton className="w-full h-16 rounded-md" />
					<Skeleton className="w-full h-16 rounded-md" />
					<Skeleton className="w-full h-16 rounded-md" />
				</div>
			}
		>
			<div className="space-y-4">
				<SecretToken />
				<PublishableToken />
				{features.auth ? <ConnectionTokens /> : null}
				{features.auth ? <CloudApiTokens /> : null}
				<McpConnection />
				<DatacenterStatus />
				{features.dangerZone ? <DangerZone /> : null}
			</div>
		</Suspense>
	);
}

function NoRunnersAlert() {
	const dataProvider = useDataProvider();
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

	if (runnerConfigsCount + runnerNamesCount > 0) {
		return null;
	}

	return (
		<div className="flex flex-col items-start">
			<NoProvidersAlert variant="connect" />
		</div>
	);
}

function Providers() {
	const navigate = useNavigate();
	const dataProvider = useEngineCompatDataProvider();
	const {
		isLoading,
		isError,
		data: configs,
		hasNextPage,
		fetchNextPage,
	} = useInfiniteQuery({
		...dataProvider.runnerConfigsQueryOptions(),
		refetchInterval: 5000,
	});

	const { data: totalDatacenterCount } = useInfiniteQuery({
		...dataProvider.datacentersQueryOptions(),
		maxPages: Infinity,
		select: (data) =>
			data.pages.reduce((acc, page) => acc + page.datacenters?.length, 0),
	});

	return (
		<SettingsCard
			title="Providers"
			description="Clouds connected to Rivet for running Rivet Actors."
			action={
				<ProviderDropdown>
					<Button
						variant="outline"
						size="sm"
						startIcon={<Icon icon={faPlus} className="size-3" />}
					>
						Add Provider
					</Button>
				</ProviderDropdown>
			}
		>
			<div className="border border-foreground/10 rounded-md overflow-hidden">
				<RunnerConfigsTable
					isLoading={isLoading}
					isError={isError}
					configs={configs || []}
					fetchNextPage={fetchNextPage}
					hasNextPage={hasNextPage}
					totalDatacenterCount={totalDatacenterCount}
					renderRegion={(regionId, { abbreviated }) => (
						<ActorRegion
							className="w-full items-center flex-1 whitespace-nowrap"
							regionId={regionId}
							showLabel={abbreviated ? "abbreviated" : true}
						/>
					)}
					onEditConfig={(name) =>
						navigate({
							to: ".",
							search: (old) => ({
								...(old as Record<string, unknown>),
								modal: "edit-provider-config",
								config: name,
							}),
						})
					}
					onDeleteConfig={(name) =>
						navigate({
							to: ".",
							search: (old) => ({
								...(old as Record<string, unknown>),
								modal: "delete-provider-config",
								config: name,
							}),
						})
					}
				/>
			</div>
		</SettingsCard>
	);
}

function Runners() {
	const {
		isLoading,
		isError,
		data: runners,
		hasNextPage,
		fetchNextPage,
	} = useInfiniteQuery({
		...useEngineCompatDataProvider().runnersQueryOptions(),
		refetchInterval: 5000,
	});
	const {
		isLoading: isLoadingEnvoys,
		isError: isErrorEnvoys,
		data: envoys,
		hasNextPage: hasNextEnvoysPage,
		fetchNextPage: fetchNextEnvoysPage,
	} = useInfiniteQuery({
		...useEngineCompatDataProvider().currentNamespaceEnvoyListQueryOptions(),
		refetchInterval: 5000,
	});

	const allRunners = [...(envoys || []), ...(runners || [])];

	return (
		<SettingsCard
			title="Runners"
			description="Processes connected to Rivet Cloud and ready to start running Rivet Actors."
		>
			<div className="border border-foreground/10 rounded-md overflow-hidden">
				<RunnersTable
					isLoading={isLoading || isLoadingEnvoys}
					isError={isError || isErrorEnvoys}
					runners={allRunners || []}
					fetchNextPage={() => {
						if (hasNextEnvoysPage) fetchNextEnvoysPage();
						if (hasNextPage) fetchNextPage();
					}}
					hasNextPage={hasNextPage || hasNextEnvoysPage}
				/>
			</div>
		</SettingsCard>
	);
}

function DatacenterStatus() {
	const dataProvider = useEngineCompatDataProvider();
	usePrefetchInfiniteQuery({
		...dataProvider.datacentersQueryOptions(),
		maxPages: Infinity,
	});
	const { data } = useInfiniteQuery(dataProvider.datacentersQueryOptions());

	return (
		<SettingsCard
			title="Datacenters status"
			description="Where the Rivet Engine is currently running and ready to run your Rivet Actors."
		>
			<ul className="flex flex-col gap-1">
				{data?.map((region) => (
					<li key={region.name}>
						<div className="inline-flex gap-2 items-center h-full text-sm">
							<Ping
								variant="success"
								className="relative left-0 right-0 top-0"
							/>
							<ActorRegion showLabel regionId={region.name} />{" "}
							<Code className="text-xs">{region.name}</Code>
						</div>
					</li>
				))}
			</ul>
		</SettingsCard>
	);
}

function DangerZone() {
	const dataProvider = useCloudNamespaceDataProvider();
	const navigate = useNavigate();

	// The namespace data provider can briefly resolve to `undefined` when the
	// user switches namespace/project from the nav while this drawer is open.
	// Bail until the new route's loader has populated `dataProvider`.
	if (!dataProvider) {
		return null;
	}

	const { data: project } = useQuery(
		dataProvider.currentProjectQueryOptions(),
	);
	const { data: namespace } = useQuery(
		dataProvider.currentNamespaceQueryOptions(),
	);

	return (
		<SettingsCard
			divided
			title={
				<span className="inline-flex items-center gap-2">
					<Icon
						icon={faTriangleExclamation}
						className="size-3.5 text-destructive"
					/>
					Danger zone
				</span>
			}
		>
			<div className="flex items-start justify-between gap-4 px-5 py-4 border-b border-foreground/10">
				<div className="min-w-0">
					<div className="text-sm font-medium text-foreground">
						Archive namespace
					</div>
					<SmallText className="text-muted-foreground">
						Permanently removes all associated Rivet Actors,
						Runners, and configurations. Cannot be undone.
					</SmallText>
				</div>
				<Button
					variant="destructive-outline"
					size="sm"
					startIcon={<Icon icon={faTrash} />}
					onClick={() =>
						navigate({
							to: ".",
							search: (old) => ({
								...(old as Record<string, unknown>),
								modal: "delete-namespace",
								displayName: namespace?.displayName,
							}),
						})
					}
				>
					Archive
				</Button>
			</div>
			<div className="flex items-start justify-between gap-4 px-5 py-4">
				<div className="min-w-0">
					<div className="text-sm font-medium text-foreground">
						Archive project
					</div>
					<SmallText className="text-muted-foreground">
						Permanently removes all associated Rivet Actors,
						Runners, and configurations. Cannot be undone.
					</SmallText>
				</div>
				<Button
					variant="destructive-outline"
					size="sm"
					startIcon={<Icon icon={faTrash} />}
					onClick={() =>
						navigate({
							to: ".",
							search: (old) => ({
								...(old as Record<string, unknown>),
								modal: "delete-project",
								displayName: project?.displayName,
							}),
						})
					}
				>
					Archive
				</Button>
			</div>
		</SettingsCard>
	);
}
