import {
	faArrowUpRight,
	faBook,
	faCog,
	faDiscord,
	faGift,
	faGithub,
	faLogs,
	faMessageSmile,
	faRocket,
	faWallet,
	Icon,
} from "@rivet-gg/icons";
import { useInfiniteQuery, useSuspenseQuery } from "@tanstack/react-query";
import {
	Link,
	useMatches,
	useMatchRoute,
	useNavigate,
} from "@tanstack/react-router";
import {
	type ComponentProps,
	createContext,
	type PropsWithChildren,
	type ReactNode,
	type RefObject,
	Suspense,
	useContext,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import type { ImperativePanelGroupHandle } from "react-resizable-panels";
import {
	Button,
	type ButtonProps,
	cn,
	type ImperativePanelHandle,
	Ping,
	ResizableHandle,
	ResizablePanel,
	ResizablePanelGroup,
	ScrollArea,
	Skeleton,
} from "@/components";
import {
	ActorRegion,
	useCloudNamespaceDataProvider,
	useDataProvider,
	useDataProviderCheck,
	useEngineCompatDataProvider,
} from "@/components/actors";
import { useRootLayoutOptional } from "@/components/actors/root-layout-context";
import type { HeaderLinkProps } from "@/components/header/header-link";
import { features } from "@/lib/features";
import { ensureTrailingSlash } from "@/lib/utils";
import type { RivetActorError } from "@/queries/types";
import { TEST_IDS } from "@/utils/test-ids";
import { ActorBuildsList } from "./actor-builds-list";
import { BillingPlanBadge } from "./billing/billing-plan-badge";
import { BillingUsageGauge } from "./billing/billing-usage-gauge";
import { Changelog } from "./changelog";
import { ContextSwitcher } from "./context-switcher";
import { HelpDropdown } from "./help-dropdown";
import { LogoMark } from "./logo";
import { NamespaceSelect } from "./namespace-select";
import { RunnerPoolErrorPopover } from "./runner-pool-error-popover";
import { TopBarActions } from "./top-bar-actions";
import { UserDropdown } from "./user-dropdown";

interface RootProps {
	children: ReactNode;
}

const Root = ({ children }: RootProps) => {
	return <div className={cn("flex h-screen flex-col")}>{children}</div>;
};

const Main = ({
	children,
	ref,
}: RootProps & { ref?: RefObject<ImperativePanelHandle> }) => {
	return (
		<ResizablePanel ref={ref} minSize={50}>
			<main
				className="bg-background flex flex-1 flex-col h-full min-h-0 min-w-0 relative"
				data-testid={TEST_IDS.Layout.Main}
			>
				{children}
			</main>
		</ResizablePanel>
	);
};

interface SidebarDimensions {
	minWidth: number;
	maxWidth: number;
}

const SidebarDimensionsContext = createContext<SidebarDimensions>({
	minWidth: 0,
	maxWidth: 20,
});
const SIDEBAR_MIN_WIDTH = 195; /* in px */
const SIDEBAR_MAX_WIDTH = 280; /* in px */

const VisibleInFull = ({ children }: PropsWithChildren) => {
	const groupRef = useRef<ImperativePanelGroupHandle>(null);

	const [sidebarDimensions, setSidebarDimensions] =
		useState<SidebarDimensions>({ minWidth: 0, maxWidth: 20 });

	useLayoutEffect(() => {
		const panelGroup = document.querySelector<HTMLDivElement>(
			'[data-panel-group-id="root"]',
		);
		const resizeHandles = panelGroup?.querySelectorAll<HTMLDivElement>(
			"[data-panel-resize-handle-id]",
		);

		if (!panelGroup || !resizeHandles || resizeHandles?.length === 0) {
			return;
		}

		const observer = new ResizeObserver(() => {
			let width = panelGroup.offsetWidth;

			resizeHandles.forEach((resizeHandle) => {
				width -= resizeHandle.offsetWidth;
			});

			setSidebarDimensions({
				minWidth: (SIDEBAR_MIN_WIDTH / width) * 100,
				maxWidth: (SIDEBAR_MAX_WIDTH / width) * 100,
			});
		});
		observer.observe(panelGroup);
		resizeHandles.forEach((resizeHandle) => {
			observer.observe(resizeHandle);
		});

		return () => {
			observer.unobserve(panelGroup);
			resizeHandles.forEach((resizeHandle) => {
				observer.unobserve(resizeHandle);
			});
			observer.disconnect();
		};
	}, []);

	return (
		// biome-ignore lint/correctness/useUniqueElementIds: id its not html element id
		<ResizablePanelGroup
			ref={groupRef}
			direction="horizontal"
			className="relative min-h-screen h-screen"
			id="root"
		>
			<SidebarDimensionsContext.Provider value={sidebarDimensions}>
				{children}
			</SidebarDimensionsContext.Provider>
		</ResizablePanelGroup>
	);
};

export const Logo = () => {
	return (
		<Link to="/" className="flex items-center gap-5 ps-3 pt-5 pb-4">
			<img
				src={`${ensureTrailingSlash(import.meta.env.BASE_URL || "")}logo.svg`}
				alt="Rivet.gg"
				className="h-6"
			/>
		</Link>
	);
};

const Sidebar = ({
	ref,
	...props
}: {
	ref?: RefObject<ImperativePanelHandle | null>;
} & ComponentProps<typeof ResizablePanel>) => {
	const { minWidth: sidebarMinWidth, maxWidth: sidebarMaxWidth } = useContext(
		SidebarDimensionsContext,
	);
	const matchRoute = useMatchRoute();
	return (
		<>
			<ResizablePanel
				ref={ref}
				minSize={sidebarMinWidth}
				maxSize={sidebarMaxWidth}
				className="bg-background"
				collapsible
				{...props}
			>
				<div
					className="flex-col gap-2 size-full flex"
					data-testid={TEST_IDS.Layout.Sidebar}
				>
					<Logo />
					<div className="flex flex-1 flex-col gap-2 px-2 min-h-0">
						{features.platform ? (
							<CloudSidebar />
						) : (
							<>
								<Breadcrumbs />
								<ScrollArea>
									<EngineSubnav />
								</ScrollArea>
							</>
						)}
					</div>
					<div>
						<div className="border-t my-0.5 mx-2.5" />

						{features.platform ? (
							<>
								<div className="flex gap-0.5 my-2 px-2.5 flex-col">
									{features.billing &&
									matchRoute({
										to: "/orgs/$organization/projects/$project/ns/$namespace",
										fuzzy: true,
										pending: false,
									}) ? (
										<HeaderButton asChild>
											<Link
												from="/orgs/$organization/projects/$project/ns/$namespace"
												to="/orgs/$organization/projects/$project/ns/$namespace/billing"
												className="font-normal justify-between flex w-full"
											>
												<p>
													<Icon
														icon={faWallet}
														className="me-1.5"
													/>
													Billing
												</p>
												<div className="flex gap-1">
													<BillingUsageGauge />
													<BillingPlanBadge />
												</div>
											</Link>
										</HeaderButton>
									) : features.billing &&
										matchRoute({
											to: "/orgs/$organization/projects/$project",
											fuzzy: true,
											pending: false,
										}) ? (
										<HeaderButton asChild>
											<Link
												from="/orgs/$organization/projects/$project"
												to="/orgs/$organization/projects/$project/billing"
												className="font-normal justify-between flex w-full"
											>
												<p>
													<Icon
														icon={faWallet}
														className="me-1.5"
													/>
													Billing
												</p>
												<div className="flex gap-1">
													<BillingUsageGauge />
													<BillingPlanBadge />
												</div>
											</Link>
										</HeaderButton>
									) : null}
									{features.support ? (
										<HelpDropdown>
											<HeaderButton
												startIcon={
													<Icon
														icon={faMessageSmile}
														className="size-5 opacity-80 group-hover:opacity-100 transition-opacity"
													/>
												}
											>
												Support
											</HeaderButton>
										</HelpDropdown>
									) : null}
									{features.branding ? (
										<Changelog>
											<HeaderButton
												startIcon={
													<Icon
														icon={faGift}
														className="size-5 opacity-80 group-hover:opacity-100 transition-opacity"
													/>
												}
											>
												<a
													href="https://www.rivet.dev/changelog"
													target="_blank"
													rel="noopener"
												>
													What's new?
													<Ping
														className="relative -right-1"
														data-changelog-ping
													/>
												</a>
											</HeaderButton>
										</Changelog>
									) : null}
								</div>
								<div className="border-t my-0.5 mx-2.5" />
								{features.auth ? (
									<div className=" px-1 pt-2 pb-4 flex flex-col">
										<UserDropdown />
									</div>
								) : null}
							</>
						) : (
							<div className="flex gap-0.5 my-2 px-2.5 flex-col">
								{features.branding ? (
									<Changelog>
										<HeaderButton
											startIcon={
												<Icon
													icon={faGift}
													className="size-5 opacity-80 group-hover:opacity-100 transition-opacity"
												/>
											}
										>
											<a
												href="https://www.rivet.dev/changelog"
												target="_blank"
												rel="noopener"
											>
												What's new?
												<Ping
													className="relative -right-1"
													data-changelog-ping
												/>
											</a>
										</HeaderButton>
									</Changelog>
								) : null}
								<HeaderButton
									asChild
									startIcon={
										<Icon
											icon={faMessageSmile}
											className="size-5 opacity-80 group-hover:opacity-100 transition-opacity"
										/>
									}
								>
									<Link
										to="."
										search={(old) => ({
											...old,
											modal: "feedback",
										})}
									>
										Feedback
									</Link>
								</HeaderButton>
								<HeaderButton
									asChild
									startIcon={
										<Icon
											icon={faBook}
											className="size-5 opacity-80 group-hover:opacity-100 transition-opacity"
										/>
									}
									endIcon={
										<Icon
											icon={faArrowUpRight}
											className="ms-1"
										/>
									}
								>
									<a
										href="https://www.rivet.dev/docs"
										target="_blank"
										rel="noopener noreferrer"
									>
										Documentation
									</a>
								</HeaderButton>
								<HeaderButton
									asChild
									startIcon={
										<Icon
											icon={faDiscord}
											className="size-5 opacity-80 group-hover:opacity-100 transition-opacity"
										/>
									}
									endIcon={
										<Icon
											icon={faArrowUpRight}
											className="ms-1"
										/>
									}
								>
									<a
										href="http://www.rivet.dev/discord"
										target="_blank"
										rel="noopener noreferrer"
									>
										Discord
									</a>
								</HeaderButton>
								<HeaderButton
									asChild
									startIcon={
										<Icon
											icon={faGithub}
											className="size-5 opacity-80 group-hover:opacity-100 transition-opacity"
										/>
									}
									endIcon={
										<Icon
											icon={faArrowUpRight}
											className="ms-1"
										/>
									}
								>
									<a
										href="http://github.com/rivet-dev/rivet"
										target="_blank"
										rel="noopener noreferrer"
									>
										GitHub
									</a>
								</HeaderButton>
							</div>
						)}
					</div>
				</div>
			</ResizablePanel>
			<ResizableHandle className="my-8 after:rounded-t-full after:rounded-b-full bg-transparent" />
		</>
	);
};

const Header = () => {
	return null;
};

const Footer = () => {
	return null;
};

export { Root, Main, Header, Footer, VisibleInFull, Sidebar };

const Breadcrumbs = (): ReactNode => {
	const matchRoute = useMatchRoute();
	const nsMatch = matchRoute({
		to: "/ns/$namespace",
		fuzzy: true,
	});

	if (nsMatch === false) {
		return null;
	}

	return (
		<Suspense
			fallback={
				<div className="flex items-center gap-2 ms-2 h-10">
					<Skeleton className="h-5 w-24" />
				</div>
			}
		>
			<NamespaceBreadcrumbs namespaceNameId={nsMatch.namespace} />
		</Suspense>
	);
};

const NamespaceBreadcrumbs = ({
	namespaceNameId,
}: {
	namespaceNameId: string;
}) => {
	const navigate = useNavigate();
	const leafFullPath = useMatches({
		select: (matches) => matches[matches.length - 1]?.fullPath,
	});
	const namespaceBase = "/ns/$namespace";
	const namespaceTo = (
		typeof leafFullPath === "string" &&
		leafFullPath.startsWith(namespaceBase)
			? leafFullPath
			: namespaceBase
	) as "/ns/$namespace";

	return (
		<div className="flex items-center gap-2">
			<NamespaceSelect
				className="text-sm py-1.5 h-auto [&>[data-icon]]:size-3"
				showCreate
				value={namespaceNameId}
				onValueChange={(value) =>
					navigate({
						to: namespaceTo,
						params: {
							namespace: value,
						},
					})
				}
				onCreateClick={() =>
					navigate({
						to: ".",
						search: (old) => ({
							...old,
							modal: "create-ns",
						}),
					})
				}
			/>
		</div>
	);
};

const EngineSubnav = () => {
	const matchRoute = useMatchRoute();
	const nsMatch = matchRoute({
		to: "/ns/$namespace",
		fuzzy: true,
	});

	if (nsMatch === false) {
		return null;
	}

	return (
		<div className="flex gap-0.5 flex-col w-full">
			<div className="w-full pt-1.5">
				<div className="flex gap-0.5 mb-2 flex-col">
					<HeaderLink
						to="/ns/$namespace"
						className="font-normal"
						params={nsMatch}
						search={(s) => ({ ...s, settings: "settings" })}
						icon={faCog}
					>
						Settings
					</HeaderLink>
				</div>

				<div className="border-t my-2" />
				<span className="block text-muted-foreground text-xs px-2 py-1 transition-colors mb-0.5">
					Actors
				</span>
				<ActorBuildsList />
			</div>
		</div>
	);
};

function HeaderLink({ icon, children, className, ...props }: HeaderLinkProps) {
	return (
		<HeaderButton
			asChild
			variant="ghost"
			{...props}
			className={cn(
				"font-medium px-1 text-muted-foreground hover:bg-foreground/[0.04] data-active:text-foreground data-active:bg-foreground/[0.06]",
				className,
			)}
			startIcon={
				icon ? (
					<Icon
						className={cn(
							"size-5 opacity-80 group-hover:opacity-100 transition-opacity",
						)}
						icon={icon}
					/>
				) : undefined
			}
		>
			<Link to={props.to} activeOptions={{ exact: true }}>
				{children}
			</Link>
		</HeaderButton>
	);
}

function RunnerConfigErrorIndicator() {
	const dataProvider = useEngineCompatDataProvider();
	const { data: errors } = useInfiniteQuery({
		...dataProvider.runnerConfigsQueryOptions(),
		select(data) {
			const map: Record<string, RivetActorError | undefined> = {};
			for (const page of data.pages) {
				for (const config of Object.values(page.runnerConfigs)) {
					for (const [dc, dcConfig] of Object.entries(
						config.datacenters,
					)) {
						if (dcConfig.runnerPoolError && !map[dc]) {
							map[dc] = dcConfig.runnerPoolError;
						}
					}
				}
			}
			return Object.keys(map).length > 0 ? map : null;
		},
	});

	if (!errors) return null;

	return (
		<RunnerPoolErrorPopover
			iconOnly
			errors={errors}
			renderRegion={(regionId) => (
				<ActorRegion regionId={regionId} showLabel="abbreviated" />
			)}
		/>
	);
}

function HeaderButton({ children, className, ...props }: ButtonProps) {
	return (
		<Button
			variant="ghost"
			{...props}
			className={cn(
				"text-muted-foreground px-1 aria-current-page:text-foreground relative h-auto py-1 justify-start",
				className,
			)}
		>
			{children}
		</Button>
	);
}

function CloudSidebar(): ReactNode {
	return (
		<>
			<ContextSwitcher />

			<ScrollArea>
				<CloudSidebarContent />
			</ScrollArea>
		</>
	);
}

function CloudSidebarContent() {
	const dataProvider = useCloudNamespaceDataProvider();

	if (dataProvider) {
		return <CloudSidebarContentInner />;
	}

	return null;
}

function CloudSidebarContentInner() {
	const hasDataProvider = useDataProviderCheck();
	const hasQuery = !!useDataProvider().buildsQueryOptions;
	const matchRoute = useMatchRoute();

	return (
		<div className="flex gap-0.5 flex-col">
			{hasDataProvider && hasQuery ? (
				<div className="w-full pt-1.5">
					<div className="flex gap-0.5 mb-2 flex-col">
						{matchRoute({
							to: "/orgs/$organization/projects/$project/ns/$namespace",
							fuzzy: true,
						}) ? (
							<HeaderLink
								to="/orgs/$organization/projects/$project/ns/$namespace"
								className="flex-1 font-normal items-center gap-1"
								search={(s) => ({ ...s, settings: "settings" })}
								icon={faCog}
							>
								<span className="flex-1">Settings</span>
								<RunnerConfigErrorIndicator />
							</HeaderLink>
						) : matchRoute({
								to: "/orgs/$organization/projects/$project",
								fuzzy: true,
							}) ? (
							<HeaderLink
								to="/orgs/$organization/projects/$project/settings"
								className="font-normal"
								icon={faCog}
							>
								Settings
							</HeaderLink>
						) : null}
					</div>

					<Suspense>
						<DeploymentsLink />
					</Suspense>

					<div className="border-t my-2" />
					<span className="block text-muted-foreground text-xs px-2 py-1 transition-colors mb-0.5">
						Actors
					</span>
					<ActorBuildsList />
				</div>
			) : null}
		</div>
	);
}

function DeploymentsLink() {
	if (!features.compute) {
		return null;
	}
	// biome-ignore lint/correctness/useHookAtTopLevel: guarded by build constant
	const provider = useCloudNamespaceDataProvider();

	// biome-ignore lint/correctness/useHookAtTopLevel: guarded by build constant
	const { data } = useSuspenseQuery(
		provider.currentNamespaceHasManagedPoolQueryOptions(),
	);

	if (!data) {
		return null;
	}

	return (
		<>
			<HeaderLink
				to="/orgs/$organization/projects/$project/ns/$namespace/deployments"
				className="font-normal"
				icon={faRocket}
			>
				Deployments
			</HeaderLink>
			<HeaderLink
				to="/orgs/$organization/projects/$project/ns/$namespace/logs"
				className="font-normal pl-6"
				icon={faLogs}
			>
				Logs
			</HeaderLink>
		</>
	);
}

export const Content = ({
	className,
	children,
}: {
	className?: string;
	children: ReactNode;
}) => {
	const isInRootLayout = !!useRootLayoutOptional();
	const { isSidebarCollapsed } = useRootLayoutOptional() || {};
	return (
		<div
			className={cn(
				" h-full overflow-auto @container transition-colors",
				!isSidebarCollapsed &&
					isInRootLayout &&
					"border my-2 bg-card rounded-lg mr-2",
				!isInRootLayout && "h-screen",
				className,
			)}
		>
			{children}
		</div>
	);
};

// Sidebarless onboarding/new-project pages share the same chrome as the
// main dashboard TopBar (Logo + breadcrumb + Feedback / Help / Docs / Org
// dropdown). The markup is inlined rather than imported from `./top-bar`
// to avoid any module-evaluation order issue with this large layout file.
export const SidebarlessHeader = () => {
	return (
		<header className="z-20 flex items-center gap-2 h-12 px-3 shrink-0 border-b border-border bg-background">
			<Link to="/" className="flex items-center shrink-0 pr-1">
				<LogoMark className="h-6 w-6" />
			</Link>
			<ContextSwitcher inline />
			<TopBarActions />
		</header>
	);
};
