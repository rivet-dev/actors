import type { Rivet } from "@rivet-gg/cloud";
import {
	createFileRoute,
	redirect,
	useNavigate,
	useSearch,
} from "@tanstack/react-router";
import { NamespaceLandingPending } from "@/app/actors-grid";
import { peekCloudNamespaceContext } from "@/app/data-providers/cache";
import {
	ConnectProviderSheet,
	isConnectProviderModal,
} from "@/app/dialogs/connect-provider-sheet";
import { EditRunnerConfigSheet } from "@/app/dialogs/edit-runner-config-sheet";
import { GettingStarted } from "@/app/getting-started";
import { SidebarlessHeader } from "@/app/layout";
import { NotFoundCard } from "@/app/not-found-card";
import { OnboardingSkeleton } from "@/app/onboarding-skeleton";
import { RouteError } from "@/app/route-error";
import { RouteLayout } from "@/app/route-layout";
import { useDialog } from "@/app/use-dialog";
import { ls } from "@/components";
import { CreateActorSheet } from "@/components/actors/dialogs/create-actor-sheet";
import {
	deriveOnboardingState,
	peekDisplaysOnboarding,
	type RunnerConfigsInfiniteData,
	type RunnerNamesInfiniteData,
} from "@/lib/data";
import { posthog } from "@/lib/posthog";
import {
	RECENT_NAMESPACES_KEY,
	recordRecentVisit,
} from "@/lib/recently-visited";
import { queryClient } from "@/queries/global";

export const Route = createFileRoute(
	"/_context/orgs/$organization/projects/$project/ns/$namespace",
)({
	component: RouteComponent,
	beforeLoad: async ({ context, params, search }) => {
		if (context.__type !== "cloud") {
			throw new Error("Invalid context type for this route");
		}

		recordRecentVisit(RECENT_NAMESPACES_KEY, params.namespace);

		let ns: Rivet.NamespacesGetResponse.Namespace;
		try {
			ns = await context.queryClient.ensureQueryData(
				context.dataProvider.currentProjectNamespaceQueryOptions({
					namespace: params.namespace,
				}),
			);
		} catch (error) {
			// Treat both true 404s and the engine's 400 + body
			// { group: "namespace", code: "not_found" } as "namespace gone" so
			// stale URLs (e.g. recently-visited entries pointing at a deleted
			// namespace) bounce back to the project picker instead of rendering
			// the error UI.
			const e = error as
				| {
						statusCode?: number;
						body?: { group?: unknown; code?: unknown };
				  }
				| null
				| undefined;
			const isNotFound =
				e?.statusCode === 404 ||
				(e?.statusCode === 400 &&
					e.body?.group === "namespace" &&
					e.body?.code === "not_found");
			if (isNotFound) {
				throw redirect({
					to: "/orgs/$organization/projects/$project",
					params,
				});
			}
			throw error;
		}

		if (search.skipOnboarding) {
			ls.onboarding.skipWelcome(params.project, params.namespace);
			posthog.capture("onboarding_skipped", {
				project: params.project,
				namespace: params.namespace,
			});
			throw redirect({ to: ".", search: {} });
		}
		if (search.onboardingSuccess) {
			throw redirect({ to: ".", search: {} });
		}

		return {
			dataProvider: context.getOrCreateCloudNamespaceContext(
				context.dataProvider,
				params.namespace,
				ns.access.engineNamespaceName,
				ns.access.engineNamespaceId,
			),
		};
	},
	loaderDeps(opts) {
		return {
			skipOnboarding: opts.search.skipOnboarding,
			backendOnboardingSuccess: opts.search.backendOnboardingSuccess,
			onboardingSuccess: opts.search.onboardingSuccess,
		};
	},
	async loader({ params, deps, context }) {
		const isSkipped =
			ls.onboarding.getSkipWelcome(params.project, params.namespace) ||
			deps.skipOnboarding;

		const namespace = await context.queryClient.fetchQuery(
			context.dataProvider.currentProjectNamespaceQueryOptions({
				namespace: params.namespace,
			}),
		);

		if (namespace.displayName !== "Production" || isSkipped === true) {
			return {
				dataProvider: context.dataProvider,
				displayOnboarding: false,
				displayFrontendOnboarding: false,
				provider: undefined,
			};
		}

		const runnerNamesOpts = context.dataProvider.runnerNamesQueryOptions();
		const runnerConfigsOpts =
			context.dataProvider.runnerConfigsQueryOptions();

		let runnerNames =
			context.queryClient.getQueryData<RunnerNamesInfiniteData>(
				runnerNamesOpts.queryKey,
			);
		let runnerConfigs =
			context.queryClient.getQueryData<RunnerConfigsInfiniteData>(
				runnerConfigsOpts.queryKey,
			);

		const cachedHasConfigs =
			Object.keys(runnerConfigs?.pages[0]?.runnerConfigs ?? {}).length >
			0;
		const cachedHasNames = (runnerNames?.pages[0]?.names?.length ?? 0) > 0;

		// Cache-first: only skip the slow blocking runner-config fetch when the
		// cache already proves the backend is configured. An absent or empty
		// cache still pays the fetch so we never wrongly show onboarding. This
		// keeps the slowness to the first cold load of the Production namespace.
		if (!cachedHasConfigs && !cachedHasNames) {
			const [names, configs] = await Promise.all([
				context.queryClient.fetchInfiniteQuery(runnerNamesOpts),
				context.queryClient.fetchInfiniteQuery(runnerConfigsOpts),
			]);
			runnerNames = names as RunnerNamesInfiniteData;
			runnerConfigs = configs as RunnerConfigsInfiniteData;
		}

		const actorCount = await context.queryClient.fetchQuery(
			context.dataProvider.actorsCountQueryOptions(),
		);

		return {
			dataProvider: context.dataProvider,
			...deriveOnboardingState({
				runnerNames,
				runnerConfigs,
				actorCount,
			}),
		};
	},
	notFoundComponent: () => <NotFoundCard />,
	errorComponent: RouteError,
	pendingMinMs: 0,
	pendingMs: 0,
	pendingComponent: NamespacePending,
});

function NamespacePending() {
	const { organization, project, namespace } = Route.useParams();
	const { skipOnboarding } = Route.useSearch();

	const displaysOnboarding = peekDisplaysOnboarding({
		queryClient,
		dataProvider: peekCloudNamespaceContext(
			organization,
			project,
			namespace,
		),
		onboardingDisplayName: "Production",
		isSkipped:
			ls.onboarding.getSkipWelcome(project, namespace) ||
			skipOnboarding === true,
	});

	if (displaysOnboarding) {
		return <OnboardingSkeleton header={<SidebarlessHeader />} />;
	}

	return <NamespaceLandingPending />;
}

function RouteComponent() {
	const {
		displayOnboarding,
		displayFrontendOnboarding,
		provider: runnerProvider,
	} = Route.useLoaderData();
	const { provider } = Route.useSearch();
	const { project, namespace } = Route.useParams();

	if (displayOnboarding || displayFrontendOnboarding) {
		return (
			<div className="h-screen flex flex-col overflow-hidden">
				<SidebarlessHeader />
				<GettingStarted
					key={`${project}-${namespace}`}
					displayFrontendOnboarding={displayFrontendOnboarding}
					provider={provider || runnerProvider}
				/>

				<CloudNamespaceModals />
			</div>
		);
	}

	return (
		<>
			<RouteLayout />
			<CloudNamespaceModals />
		</>
	);
}

function CloudNamespaceModals() {
	const navigate = useNavigate();
	const search = useSearch({ strict: false });
	const CreateNamespaceDialog = useDialog.CreateNamespace.Dialog;
	const DeleteConfigDialog = useDialog.DeleteConfig.Dialog;
	const DeleteNamespaceDialog = useDialog.DeleteNamespace.Dialog;
	const DeleteProjectDialog = useDialog.DeleteProject.Dialog;
	const UpsertDeploymentDialog = useDialog.UpsertDeployment.Dialog;

	return (
		<>
			<CreateActorSheet
				open={search?.modal === "create-actor"}
				onOpenChange={(value) => {
					if (!value) {
						return navigate({
							to: ".",
							search: (old) => ({
								...old,
								modal: undefined,
							}),
						});
					}
				}}
			/>
			<CreateActorSheet
				variant="agent-os"
				open={search?.modal === "create-agent-os"}
				onOpenChange={(value) => {
					if (!value) {
						return navigate({
							to: ".",
							search: (old) => ({
								...old,
								modal: undefined,
							}),
						});
					}
				}}
			/>
			<UpsertDeploymentDialog
				namespace={search?.namespace}
				defaultImage={
					search?.repository && search?.tag
						? { repository: search.repository, tag: search.tag }
						: undefined
				}
				dialogProps={{
					open: search?.modal === "upsert-deployment",
					onOpenChange: (value) => {
						if (!value) {
							return navigate({
								to: ".",
								search: (old) => ({
									...old,
									modal: undefined,
								}),
							});
						}
					},
				}}
			/>
			<CreateNamespaceDialog
				project={search?.project}
				dialogProps={{
					open: search?.modal === "create-ns",
					onOpenChange: (value) => {
						if (!value) {
							return navigate({
								to: ".",
								search: (old) => ({
									...old,
									modal: undefined,
								}),
							});
						}
					},
				}}
			/>
			<ConnectProviderSheet
				modal={search?.modal}
				open={isConnectProviderModal(search?.modal)}
				onOpenChange={(value) => {
					if (!value) {
						return navigate({
							to: ".",
							search: (old) => ({
								...old,
								modal: undefined,
							}),
						});
					}
				}}
			/>
			<EditRunnerConfigSheet
				name={search?.config}
				dc={search?.dc}
				open={search?.modal === "edit-provider-config"}
				onOpenChange={(value) => {
					if (!value) {
						return navigate({
							to: ".",
							search: (old) => ({
								...old,
								modal: undefined,
							}),
						});
					}
				}}
			/>
			<DeleteConfigDialog
				name={search?.config}
				dialogProps={{
					open: search?.modal === "delete-provider-config",
					onOpenChange: (value) => {
						if (!value) {
							return navigate({
								to: ".",
								search: (old) => ({
									...old,
									modal: undefined,
								}),
							});
						}
					},
				}}
			/>
			<DeleteNamespaceDialog
				displayName={search?.displayName}
				dialogProps={{
					open: search?.modal === "delete-namespace",
					onOpenChange: (value) => {
						if (!value) {
							return navigate({
								to: ".",
								search: (old) => ({
									...old,
									modal: undefined,
								}),
							});
						}
					},
				}}
			/>
			<DeleteProjectDialog
				displayName={search?.displayName}
				dialogProps={{
					open: search?.modal === "delete-project",
					onOpenChange: (value) => {
						if (!value) {
							return navigate({
								to: ".",
								search: (old) => ({
									...old,
									modal: undefined,
								}),
							});
						}
					},
				}}
			/>
		</>
	);
}
