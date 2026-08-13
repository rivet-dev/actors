import * as DialogPrimitive from "@radix-ui/react-dialog";
import {
	faBuilding,
	faCircleUser,
	faClose,
	faCreditCard,
	faGear,
	faRivet,
	faSliders,
	faSparkles,
	Icon,
	type IconProp,
} from "@rivet-gg/icons";
import { useQuery } from "@tanstack/react-query";
import {
	useMatch,
	useMatchRoute,
	useNavigate,
	useSearch,
} from "@tanstack/react-router";
import { type ReactNode, useEffect, useState } from "react";
import { cn, VisuallyHidden } from "@/components";
import { useHasManagedPool } from "@/components/actors/actor-details-shared";
import {
	useCloudNamespaceDataProvider,
	useCloudProjectDataProvider,
} from "@/components/actors/data-provider";
import { features } from "@/lib/features";
import { BillingUsageGauge } from "./billing/billing-usage-gauge";
import {
	PoolSwitcher,
	poolHeaderText,
	resolvePoolName,
} from "./pool-switcher";
import { BillingPanel } from "./settings-pages/billing-panel";
import { NamespaceComputeContent } from "./settings-pages/namespace-compute";
import {
	NamespaceAdvancedContent,
	NamespaceSettingsContent,
} from "./settings-pages/namespace-settings";
import { OrganizationPanel } from "./settings-pages/organization-panel";
import { ProfilePage } from "./settings-pages/profile-page";
import { ResourcePicker } from "./settings-pages/resource-picker";
import { WhatsNewPanel } from "./settings-pages/whats-new-panel";

export type SettingsTab =
	| "profile"
	| "settings"
	| "compute"
	| "advanced"
	| "billing"
	| "organization"
	| "whats-new";

const NAV_SECTIONS: Array<{
	label: string;
	items: { key: SettingsTab; label: string; icon: IconProp }[];
}> = [
	{
		label: "Account",
		items: [{ key: "profile", label: "Account", icon: faCircleUser }],
	},
	{
		label: "Project",
		items: [{ key: "billing", label: "Billing", icon: faCreditCard }],
	},
	{
		label: "Namespace",
		items: [
			{ key: "settings", label: "Settings", icon: faGear },
			{ key: "compute", label: "Compute", icon: faRivet },
			{ key: "advanced", label: "Advanced", icon: faSliders },
		],
	},
	{
		label: "Organization",
		items: [
			{ key: "organization", label: "Organization", icon: faBuilding },
		],
	},
	{
		label: "Other",
		items: [{ key: "whats-new", label: "What's new", icon: faSparkles }],
	},
];

const TAB_META: Record<SettingsTab, { title: string; description?: string }> = {
	profile: {
		title: "Account",
		description: "Manage your account info.",
	},
	billing: {
		title: "Billing",
		description:
			"Manage your project's billing information and view usage details.",
	},
	settings: {
		title: "Settings",
		description:
			"Connect your RivetKit application to Rivet Cloud. Use your cloud of choice to run Rivet Actors.",
	},
	compute: {
		title: "Compute",
		description: "Manage the Rivet Compute deployment for this namespace.",
	},
	advanced: {
		title: "Advanced",
		description:
			"Tokens, datacenter status, and other low-level controls for this namespace.",
	},
	organization: {
		title: "Organization",
		description: "Manage your organization and its members.",
	},
	"whats-new": {
		title: "What's new",
		description: "Recent changes and announcements.",
	},
};

interface SettingsDrawerProps {
	open: boolean;
	tab: SettingsTab;
	onOpenChange: (open: boolean) => void;
}

// TopBar is a flush `h-12` bar (48px, border included). Add the content view's
// `my-2` gap (8px) so the drawer's outline aligns with the card below.
const TOP_BAR_OUTER_HEIGHT = "56px";

export function SettingsDrawer({
	open,
	tab,
	onOpenChange,
}: SettingsDrawerProps) {
	const navigate = useNavigate();
	const matchRoute = useMatchRoute();
	const onNamespace = !!matchRoute({
		to: "/orgs/$organization/projects/$project/ns/$namespace",
		fuzzy: true,
	});
	const [activeTab, setActiveTab] = useState<SettingsTab>(tab);

	useEffect(() => {
		if (open) setActiveTab(tab);
	}, [open, tab]);

	const switchTab = (next: SettingsTab) => {
		setActiveTab(next);
		navigate({
			to: ".",
			search: (old) => ({
				...(old as Record<string, unknown>),
				settings: next,
			}),
		});
	};

	const onProject = !!matchRoute({
		to: "/orgs/$organization/projects/$project",
		fuzzy: true,
	});

	// Engine flavors only expose namespace-scoped settings (no account, billing,
	// or organization), so the nav collapses to the Namespace section.
	const navSections = features.platform
		? NAV_SECTIONS
		: NAV_SECTIONS.filter((section) => section.label === "Namespace");

	const meta = TAB_META[activeTab];
	const titleNode: ReactNode =
		activeTab === "settings" && onNamespace ? (
			<NamespaceSettingsTitle fallback={meta.title} />
		) : activeTab === "compute" && onNamespace ? (
			<ComputeSettingsTitle fallback={meta.title} />
		) : activeTab === "billing" && onProject ? (
			<ProjectBillingTitle fallback={meta.title} />
		) : (
			meta.title
		);

	return (
		<DialogPrimitive.Root
			open={open}
			onOpenChange={onOpenChange}
			modal={false}
		>
			<DialogPrimitive.Portal>
				<DialogPrimitive.Content
					className={cn(
						"fixed left-2 right-2 z-50 flex flex-col overflow-hidden",
						"bg-card border border-border rounded-lg",
						"focus:outline-none",
						"data-[state=open]:animate-in data-[state=closed]:animate-out",
						"data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0",
					)}
					style={{
						top: `calc(${TOP_BAR_OUTER_HEIGHT} + var(--billing-banner-height, 0px))`,
						bottom: "8px",
					}}
					onInteractOutside={(e) => e.preventDefault()}
					onPointerDownOutside={(e) => e.preventDefault()}
					onFocusOutside={(e) => e.preventDefault()}
				>
					<VisuallyHidden>
						<DialogPrimitive.Title>
							{meta.title}
						</DialogPrimitive.Title>
					</VisuallyHidden>

					<div className="flex h-full min-h-0">
						<aside className="w-44 shrink-0 border-r border-border p-3 overflow-y-auto">
							<nav className="flex flex-col gap-4">
								{navSections.map((section) => (
									<div key={section.label}>
										<div className="px-2 pb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
											{section.label}
										</div>
										{section.items.map((item) =>
											item.key === "compute" ? (
												<ComputeNavItem
													key={item.key}
													icon={item.icon}
													label={item.label}
													active={
														activeTab === item.key
													}
													onClick={() =>
														switchTab(item.key)
													}
												/>
											) : (
												<NavItem
													key={item.key}
													icon={item.icon}
													label={item.label}
													active={
														activeTab === item.key
													}
													onClick={() =>
														switchTab(item.key)
													}
													trailing={
														item.key ===
															"billing" &&
														activeTab !==
															"billing" ? (
															<NavBillingGauge />
														) : null
													}
												/>
											),
										)}
									</div>
								))}
							</nav>
						</aside>
						<div className="flex-1 min-w-0 overflow-y-auto [scrollbar-gutter:stable]">
							<TabFrame
								title={titleNode}
								description={meta.description}
							>
								<TabContent tab={activeTab} />
							</TabFrame>
						</div>
					</div>

					<DialogPrimitive.Close
						className="absolute right-3 top-3 rounded-md p-1.5 text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
						aria-label="Close settings"
					>
						<Icon icon={faClose} className="size-4" />
					</DialogPrimitive.Close>
				</DialogPrimitive.Content>
			</DialogPrimitive.Portal>
		</DialogPrimitive.Root>
	);
}

function NavItem({
	icon,
	label,
	active,
	onClick,
	trailing,
}: {
	icon: IconProp;
	label: string;
	active: boolean;
	onClick: () => void;
	trailing?: ReactNode;
}) {
	return (
		<button
			type="button"
			onClick={onClick}
			className={cn(
				"w-full flex items-center gap-2 rounded-md px-2.5 py-1.5 text-sm text-left transition-colors",
				"focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
				active
					? "bg-foreground/[0.06] text-foreground"
					: "text-muted-foreground hover:bg-foreground/[0.04] hover:text-foreground",
			)}
		>
			<Icon
				icon={icon}
				className={cn(
					"!size-3 shrink-0",
					icon === faRivet && "scale-125",
				)}
			/>
			<span className="truncate">{label}</span>
			{trailing}
		</button>
	);
}

interface ComputeNavItemProps {
	icon: IconProp;
	label: string;
	active: boolean;
	onClick: () => void;
}

// The Compute nav item only shows when the current namespace actively uses
// Rivet Compute. Guarded with `shouldThrow: false` plus a `loaderData` check
// because the drawer can render while the namespace match tree is
// mid-transition; the inner managed-pool hooks read the namespace data
// provider from loader data and would crash on undefined. The outer/inner
// split keeps hook order stable while the route match changes.
function ComputeNavItem(props: ComputeNavItemProps) {
	const match = useMatch({
		from: "/_context/orgs/$organization/projects/$project/ns/$namespace",
		shouldThrow: false,
	});
	if (!features.compute || !match || !match.loaderData) return null;
	return <ComputeNavItemInner {...props} />;
}

function ComputeNavItemInner(props: ComputeNavItemProps) {
	const hasManagedPool = useHasManagedPool();
	if (!hasManagedPool) return null;
	return <NavItem {...props} />;
}

// Renders the usage gauge next to the Billing nav item. Guarded with
// `shouldThrow: false` plus a `loaderData` check because the drawer can render
// while the project match tree is mid-transition; without it the inner billing
// hooks read undefined loader data and crash. Returns null off a project route
// so the project-scoped hooks never run.
function NavBillingGauge() {
	const match = useMatch({
		from: "/_context/orgs/$organization/projects/$project",
		shouldThrow: false,
	});
	if (!match || !match.loaderData) return null;
	return (
		<span className="ml-auto flex items-center shrink-0">
			<BillingUsageGauge />
		</span>
	);
}

function TabContent({ tab }: { tab: SettingsTab }) {
	switch (tab) {
		case "profile":
			return <ProfilePage />;
		case "billing":
			return <BillingPanel />;
		case "settings":
			return <SettingsTabBody />;
		case "compute":
			return <ComputeTabBody />;
		case "advanced":
			return <AdvancedTabBody />;
		case "organization":
			return <OrganizationPanel />;
		case "whats-new":
			return <WhatsNewPanel />;
	}
}

function TabFrame({
	title,
	description,
	children,
}: {
	title: ReactNode;
	description?: string;
	children: React.ReactNode;
}) {
	return (
		<div className="mx-auto w-full max-w-4xl px-6 py-5">
			<div className="mb-5">
				<h2 className="text-base font-semibold text-foreground">
					{title}
				</h2>
				{description ? (
					<p className="mt-0.5 text-xs text-muted-foreground">
						{description}
					</p>
				) : null}
			</div>
			{children}
		</div>
	);
}

function NamespaceSettingsTitle({ fallback }: { fallback: string }) {
	// Guard with `shouldThrow: false` because the drawer can render this title
	// while the active match tree is mid-transition. The `loaderData` check
	// also handles the in-between state where the match has entered the tree
	// but its loader hasn't resolved — without it, the inner hook reads
	// undefined loader data and crashes when it touches `dataProvider`.
	const match = useMatch({
		from: "/_context/orgs/$organization/projects/$project/ns/$namespace",
		shouldThrow: false,
	});
	if (!match || !match.loaderData) return <>{fallback}</>;
	return <NamespaceSettingsTitleInner fallback={fallback} />;
}

function NamespaceSettingsTitleInner({ fallback }: { fallback: string }) {
	const dataProvider = useCloudNamespaceDataProvider();
	if (!dataProvider) return <>{fallback}</>;
	const { data } = useQuery(dataProvider.currentNamespaceQueryOptions());
	const displayName = data?.displayName;
	return <>{displayName ? `${displayName} settings` : fallback}</>;
}

function ComputeSettingsTitle({ fallback }: { fallback: string }) {
	// Same mid-transition guard as NamespaceSettingsTitle: the drawer can render
	// this title while the namespace match tree is still resolving. The
	// `dataProvider` check lives here (not in the inner component) so the inner
	// component's hooks stay unconditional across namespace switches, matching
	// the NamespaceComputeContent outer/inner split.
	const match = useMatch({
		from: "/_context/orgs/$organization/projects/$project/ns/$namespace",
		shouldThrow: false,
	});
	const dataProvider = useCloudNamespaceDataProvider();
	if (!match || !match.loaderData || !dataProvider) {
		return <>{fallback}</>;
	}
	return <ComputeSettingsTitleInner fallback={fallback} />;
}

function ComputeSettingsTitleInner({ fallback }: { fallback: string }) {
	const dataProvider = useCloudNamespaceDataProvider();
	const navigate = useNavigate();
	const { pool: poolParam } = useSearch({ strict: false });
	const { data: pools = [] } = useQuery(
		dataProvider.currentNamespaceManagedPoolsQueryOptions(),
	);
	const selectedPool = resolvePoolName(pools, poolParam);

	if (pools.length <= 1) {
		return <>{fallback}</>;
	}

	return (
		<span className="inline-flex items-center gap-1.5">
			{poolHeaderText(pools, "Config", fallback)}
			<PoolSwitcher
				pools={pools}
				value={selectedPool}
				onChange={(name) =>
					navigate({
						to: ".",
						search: (old) => ({
							...(old as Record<string, unknown>),
							pool: name,
						}),
					})
				}
			/>
		</span>
	);
}

function ProjectBillingTitle({ fallback }: { fallback: string }) {
	const match = useMatch({
		from: "/_context/orgs/$organization/projects/$project",
		shouldThrow: false,
	});
	if (!match || !match.loaderData) return <>{fallback}</>;
	return <ProjectBillingTitleInner fallback={fallback} />;
}

function ProjectBillingTitleInner({ fallback }: { fallback: string }) {
	const dataProvider = useCloudProjectDataProvider();
	if (!dataProvider) return <>{fallback}</>;
	const { data } = useQuery(dataProvider.currentProjectQueryOptions());
	const displayName = data?.displayName;
	return <>{displayName ? `${displayName} Billing` : fallback}</>;
}

// Engine flavors scope settings to the route's namespace, so there is no
// project/namespace resource picker. Cloud flavors pick a namespace first.
function SettingsTabBody() {
	if (!features.platform) {
		return <EngineNamespaceSettings />;
	}
	return <CloudSettingsTabBody />;
}

function CloudSettingsTabBody() {
	// `useMatch` with `shouldThrow: false` returns `undefined` when the
	// namespace route is not in the active match tree. This is stricter than
	// `useMatchRoute` (which can flicker during transitions) and lets us bail
	// before any namespace-only hook gets called.
	const namespaceMatch = useMatch({
		from: "/_context/orgs/$organization/projects/$project/ns/$namespace",
		shouldThrow: false,
	});

	if (!namespaceMatch) {
		return (
			<ResourcePicker
				title="Pick a namespace"
				description="Settings are scoped to a namespace. Choose one to manage providers, runners, and tokens."
				settings="settings"
				target="namespace"
			/>
		);
	}
	// During navigation from the resource picker the match enters the tree
	// before its loader resolves. Namespace-only hooks crash on undefined
	// loader data, so wait for it to land.
	if (!namespaceMatch.loaderData) {
		return <NamespaceSettingsSkeleton />;
	}
	return <NamespaceSettingsContent />;
}

// The `/ns/$namespace` route fixes the namespace, so engine settings render
// their content directly once the route loader has resolved (no resource
// picker). The readiness check guards the brief window during a namespace
// switch where the match is in the tree but its loader has not landed yet.
function EngineNamespaceSettings() {
	return useEngineNamespaceReady() ? (
		<NamespaceSettingsContent />
	) : (
		<NamespaceSettingsSkeleton />
	);
}

function EngineNamespaceAdvanced() {
	return useEngineNamespaceReady() ? (
		<NamespaceAdvancedContent />
	) : (
		<NamespaceSettingsSkeleton />
	);
}

function useEngineNamespaceReady() {
	const match = useMatch({
		from: "/_context/ns/$namespace",
		shouldThrow: false,
	});
	// Check the resolved `dataProvider` rather than truthiness of `loaderData`
	// so an in-between empty object during a namespace switch is not treated as
	// ready (the namespace-scoped content hooks need the provider populated).
	return match?.loaderData?.dataProvider != null;
}

function NamespaceSettingsSkeleton() {
	return (
		<div className="space-y-4">
			<div className="h-24 rounded-lg border border-foreground/10 bg-card/50" />
			<div className="h-40 rounded-lg border border-foreground/10 bg-card/50" />
			<div className="h-40 rounded-lg border border-foreground/10 bg-card/50" />
		</div>
	);
}

function AdvancedTabBody() {
	if (!features.platform) {
		return <EngineNamespaceAdvanced />;
	}
	return <CloudAdvancedTabBody />;
}

// Rivet Compute is cloud-platform-only, so there is no engine variant. The
// feature check is a backstop; `settingsParamToTab` already keeps the tab
// unreachable on flavors without the compute flag.
function ComputeTabBody() {
	if (!features.compute) {
		return null;
	}
	return <CloudComputeTabBody />;
}

function CloudComputeTabBody() {
	const namespaceMatch = useMatch({
		from: "/_context/orgs/$organization/projects/$project/ns/$namespace",
		shouldThrow: false,
	});

	if (!namespaceMatch) {
		return (
			<ResourcePicker
				title="Pick a namespace"
				description="Compute settings are scoped to a namespace."
				settings="compute"
				target="namespace"
			/>
		);
	}
	if (!namespaceMatch.loaderData) {
		return <NamespaceSettingsSkeleton />;
	}
	return <CloudComputeTabBodyInner />;
}

// Deep links can land on the Compute tab for a namespace that does not use
// Rivet Compute. Redirect to the regular Settings tab once the managed-pool
// check resolves, and show the skeleton until then.
function CloudComputeTabBodyInner() {
	const navigate = useNavigate();
	const provider = useCloudNamespaceDataProvider();
	const { data: hasManagedPool, isPending } = useQuery({
		...provider.currentNamespaceHasManagedPoolQueryOptions(),
		staleTime: Infinity,
	});

	useEffect(() => {
		if (isPending || hasManagedPool) return;
		navigate({
			to: ".",
			search: (old) => ({
				...(old as Record<string, unknown>),
				settings: "settings",
			}),
		});
	}, [isPending, hasManagedPool, navigate]);

	if (!hasManagedPool) {
		return <NamespaceSettingsSkeleton />;
	}
	return <NamespaceComputeContent />;
}

function CloudAdvancedTabBody() {
	const namespaceMatch = useMatch({
		from: "/_context/orgs/$organization/projects/$project/ns/$namespace",
		shouldThrow: false,
	});

	if (!namespaceMatch) {
		return (
			<ResourcePicker
				title="Pick a namespace"
				description="Advanced settings are scoped to a namespace. Choose one to manage tokens and datacenter status."
				settings="advanced"
				target="namespace"
			/>
		);
	}
	if (!namespaceMatch.loaderData) {
		return <NamespaceSettingsSkeleton />;
	}
	return <NamespaceAdvancedContent />;
}

export function settingsParamToTab(
	param: string | undefined,
): SettingsTab | null {
	switch (param) {
		case "profile":
		case "settings":
		case "advanced":
		case "billing":
		case "organization":
		case "whats-new":
			return param;
		// Compute is gated by the compute feature flag; degrade deep links to
		// the regular Settings tab on flavors without it.
		case "compute":
			return features.compute ? "compute" : "settings";
		// Legacy: members lived in its own tab before being merged into
		// Organization. Keep the deep link working.
		case "members":
			return "organization";
		default:
			return null;
	}
}

export function SettingsDrawerHost() {
	const navigate = useNavigate();
	const search = useSearch({ strict: false }) as Record<string, unknown>;
	const param =
		typeof search.settings === "string" ? search.settings : undefined;
	const tab = settingsParamToTab(param);

	return (
		<SettingsDrawer
			open={tab !== null}
			tab={tab ?? "profile"}
			onOpenChange={(open) => {
				if (!open) {
					navigate({
						to: ".",
						search: (old) => ({
							...(old as Record<string, unknown>),
							settings: undefined,
						}),
					});
				}
			}}
		/>
	);
}
