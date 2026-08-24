import type { QueryClient } from "@tanstack/react-query";
import { createRootRouteWithContext, Outlet } from "@tanstack/react-router";
import { TanStackRouterDevtools } from "@tanstack/react-router-devtools";
import type {
	CloudContext,
	CloudNamespaceContext,
	EngineContext,
	EngineNamespaceContext,
	OrganizationContext,
	ProjectContext,
} from "@/app/data-providers/cache";
import { DevToolbar } from "@/app/dev-toolbar";
import { ImpersonationBanner } from "@/app/impersonation-banner";
import { FullscreenLoading } from "@/components";
import { features } from "@/lib/features";

function RootRoute() {
	return (
		<>
			<Outlet />
			<DevToolbar />
			{import.meta.env.DEV ? (
				<TanStackRouterDevtools position="bottom-right" />
			) : null}
		</>
	);
}

function CloudRoute() {
	return (
		<>
			<Outlet />
			<ImpersonationBanner />
			<DevToolbar />
			{import.meta.env.DEV ? (
				<TanStackRouterDevtools position="bottom-right" />
			) : null}
		</>
	);
}

interface RootRouteContext {
	queryClient: QueryClient;
	getOrCreateCloudContext: () => CloudContext;
	getOrCreateEngineContext: (
		engineToken: (() => string) | string | (() => Promise<string>),
	) => EngineContext;
	getOrCreateOrganizationContext: (
		parent: CloudContext,
		organization: string,
	) => OrganizationContext;
	getOrCreateProjectContext: (
		parent: CloudContext & OrganizationContext,
		organization: string,
		project: string,
	) => ProjectContext;
	getOrCreateCloudNamespaceContext: (
		parent: CloudContext & OrganizationContext & ProjectContext,
		namespace: string,
		engineNamespaceName: string,
		engineNamespaceId: string,
	) => CloudNamespaceContext;
	getOrCreateEngineNamespaceContext: (
		parent: EngineContext,
		namespace: string,
	) => EngineNamespaceContext;
}

export const Route = createRootRouteWithContext<RootRouteContext>()({
	component: features.auth && features.platform ? CloudRoute : RootRoute,
	pendingComponent: FullscreenLoading,
});
