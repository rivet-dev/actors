import { createFileRoute, redirect, useNavigate } from "@tanstack/react-router";
import { match } from "ts-pattern";
import { NamespaceLandingPending } from "@/app/actors-grid";
import {
	ConnectProviderSheet,
	isConnectProviderModal,
} from "@/app/dialogs/connect-provider-sheet";
import { EditRunnerConfigSheet } from "@/app/dialogs/edit-runner-config-sheet";
import { GettingStarted } from "@/app/getting-started";
import { SidebarlessHeader } from "@/app/layout";
import { NotFoundCard } from "@/app/not-found-card";
import { OnboardingSkeleton } from "@/app/onboarding-skeleton";
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

export const Route = createFileRoute("/_context/ns/$namespace")({
	context: ({ context, params }) =>
		match(context)
			.with({ __type: "engine" }, (ctx) => ({
				dataProvider: context.getOrCreateEngineNamespaceContext(
					ctx.dataProvider,
					params.namespace,
				),
			}))
			.otherwise(() => {
				throw new Error("Invalid context type for this route");
			}),
	beforeLoad: ({ params, search }) => {
		recordRecentVisit(RECENT_NAMESPACES_KEY, params.namespace);

		const s = search as unknown as {
			skipOnboarding?: boolean;
			onboardingSuccess?: boolean;
		};
		if (s.skipOnboarding) {
			ls.onboarding.skipWelcomeEngine(params.namespace);
			posthog.capture("onboarding_skipped", {
				namespace: params.namespace,
			});
			throw redirect({ to: ".", search: {} });
		}
		if (s.onboardingSuccess) {
			throw redirect({ to: ".", search: {} });
		}
	},
	loaderDeps(opts) {
		const s = opts.search as unknown as {
			skipOnboarding?: boolean;
			backendOnboardingSuccess?: boolean;
			onboardingSuccess?: boolean;
		};
		return {
			skipOnboarding: s.skipOnboarding,
			backendOnboardingSuccess: s.backendOnboardingSuccess,
			onboardingSuccess: s.onboardingSuccess,
		};
	},
	async loader({ params, deps, context }) {
		const d = deps as {
			skipOnboarding?: boolean;
			backendOnboardingSuccess?: boolean;
			onboardingSuccess?: boolean;
		};
		const isSkipped =
			ls.onboarding.getSkipWelcomeEngine(params.namespace) ||
			d.skipOnboarding;

		if (isSkipped === true) {
			return {
				dataProvider: context.dataProvider,
				displayOnboarding: false,
				displayFrontendOnboarding: false,
				provider: undefined,
			};
		}

		// Onboarding only applies to the default namespace. Every other namespace
		// renders immediately without paying for the runner-config fetch.
		const namespace = await context.queryClient.fetchQuery(
			context.dataProvider.currentNamespaceQueryOptions(),
		);
		if (namespace?.displayName !== "Default") {
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
		// keeps the slowness to the first cold load of the default namespace.
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
	component: RouteComponent,
	notFoundComponent: () => <NotFoundCard />,
	pendingMinMs: 0,
	pendingMs: 0,
	pendingComponent: NamespacePending,
});

function NamespacePending() {
	const { namespace } = Route.useParams();
	const { dataProvider } = Route.useRouteContext();
	const search = Route.useSearch() as { skipOnboarding?: boolean };

	const displaysOnboarding = peekDisplaysOnboarding({
		queryClient,
		dataProvider,
		onboardingDisplayName: "Default",
		isSkipped:
			ls.onboarding.getSkipWelcomeEngine(namespace) ||
			search.skipOnboarding === true,
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
	const { namespace } = Route.useParams();

	if (displayOnboarding || displayFrontendOnboarding) {
		return (
			<>
				<SidebarlessHeader />
				<GettingStarted
					key={namespace}
					displayFrontendOnboarding={displayFrontendOnboarding}
					provider={provider || runnerProvider}
				/>
				<Modals />
			</>
		);
	}

	return (
		<>
			<Modals />
			<RouteLayout />
		</>
	);
}

function Modals() {
	const navigate = useNavigate();
	const search = Route.useSearch();

	const DeleteConfigDialog = useDialog.DeleteConfig.Dialog;

	return (
		<>
			<CreateActorSheet
				open={search.modal === "create-actor"}
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
				open={search.modal === "create-agent-os"}
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
			<ConnectProviderSheet
				modal={search.modal}
				open={isConnectProviderModal(search.modal)}
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
				name={search.config}
				dc={search.dc}
				open={search.modal === "edit-provider-config"}
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
				name={search.config}
				dialogProps={{
					open: search.modal === "delete-provider-config",

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
