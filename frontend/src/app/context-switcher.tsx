import type { Rivet } from "@rivet-gg/cloud";
import { faCheck, faGear, faPlus, faPlusCircle, Icon } from "@rivet-gg/icons";
import {
	useInfiniteQuery,
	usePrefetchInfiniteQuery,
	useQuery,
} from "@tanstack/react-query";
import {
	useMatches,
	useMatchRoute,
	useNavigate,
	useParams,
	useRouter,
	useSearch,
} from "@tanstack/react-router";
import { useMemo, useState } from "react";
import {
	Button,
	Command,
	CommandEmpty,
	CommandGroup,
	CommandInput,
	CommandItem,
	CommandList,
	cn,
	Popover,
	PopoverContent,
	PopoverTrigger,
	Skeleton,
} from "@/components";
import {
	useCloudDataProvider,
	useDataProvider,
	useDataProviderCheck,
	useEngineCompatDataProvider,
	useNamespaceDataProviderReady,
} from "@/components/actors";
import { SafeHover } from "@/components/safe-hover";
import { VisibilitySensor } from "@/components/visibility-sensor";
import { authClient } from "@/lib/auth";
import { features } from "@/lib/features";
import {
	getRecentTimestamp,
	RECENT_NAMESPACES_KEY,
	RECENT_PROJECTS_KEY,
} from "@/lib/recently-visited";
import { LazyBillingPlanBadge } from "./billing/billing-plan-badge";

export function ContextSwitcher({ inline }: { inline?: boolean }) {
	const match = useContextSwitcherMatch();

	if (!match) {
		return null;
	}

	return (
		<ContextSwitcherInner
			inline={inline}
			organization={
				"organization" in match ? match.organization : undefined
			}
		/>
	);
}

function ContextSwitcherInner({
	organization,
	inline,
}: {
	organization?: string;
	inline?: boolean;
}) {
	const [isOpen, setIsOpen] = useState(false);

	if (features.platform) {
		// biome-ignore lint/correctness/useHookAtTopLevel: guaranteed by build condition
		usePrefetchInfiniteQuery({
			// biome-ignore lint/correctness/useHookAtTopLevel: guaranteed by build condition
			...useCloudDataProvider().projectsQueryOptions({
				organization: organization!,
			}),
		});
	}

	// biome-ignore lint/correctness/useHookAtTopLevel: usage is stable inside this function
	const match = useContextSwitcherMatch();

	const dataProvider = useDataProviderCheck();
	// The org index page carries only an org-level data provider (from the
	// `/orgs/$organization` loader), which `useDataProviderCheck` does not
	// match, so let the org-only breadcrumb through without it.
	const isOrgOnly =
		!!match &&
		"organization" in match &&
		!("project" in match) &&
		!("namespace" in match);
	if (!dataProvider && !isOrgOnly) {
		return null;
	}

	// Multitenancy inline case: render per-segment popovers so each chevron
	// opens its own dropdown (project / namespace), matching the v77 + v78
	// mockups. Other cases fall back to the legacy single popover below.
	if (
		inline &&
		match &&
		"project" in match &&
		"namespace" in match &&
		"organization" in match
	) {
		return (
			<div className="flex items-center min-w-0">
				<BreadcrumbSlash />
				<OrgSegmentPopover organization={match.organization} />
				<BreadcrumbSlash />
				<ProjectSegmentPopover
					organization={match.organization}
					currentProject={match.project}
				/>
				<BreadcrumbSlash />
				<NamespaceSegmentPopover
					organization={match.organization}
					currentProject={match.project}
					currentNamespace={match.namespace}
				/>
				<ActorBreadcrumbSegment />
			</div>
		);
	}

	// Project-only landing (e.g. /orgs/$org/projects/$project namespaces grid).
	// Render just the project segment with its own dropdown — the legacy
	// 2-column popover doesn't fit here.
	if (
		inline &&
		match &&
		"project" in match &&
		"organization" in match &&
		!("namespace" in match)
	) {
		return (
			<div className="flex items-center min-w-0">
				<BreadcrumbSlash />
				<OrgSegmentPopover organization={match.organization} />
				<BreadcrumbSlash />
				<ProjectSegmentPopover
					organization={match.organization}
					currentProject={match.project}
				/>
			</div>
		);
	}

	// Org-only landing (e.g. /orgs/$org org dashboard). Render just the org
	// segment so the breadcrumb still shows which organization you are in,
	// matching every other page.
	if (
		inline &&
		match &&
		"organization" in match &&
		!("project" in match) &&
		!("namespace" in match)
	) {
		return (
			<div className="flex items-center min-w-0">
				<BreadcrumbSlash />
				<OrgSegmentPopover organization={match.organization} />
			</div>
		);
	}

	// Engine (OSS) namespace case: there is no organization/project, so the
	// cloud popovers above and the legacy 2-column `Content` (which bails when
	// `params.organization` is absent) don't apply. Render a single namespace
	// segment that lists engine namespaces and navigates to `/ns/$namespace`.
	if (
		inline &&
		match &&
		"namespace" in match &&
		!("organization" in match) &&
		!("project" in match)
	) {
		return (
			<div className="flex items-center min-w-0">
				<BreadcrumbSlash />
				<EngineNamespaceSegmentPopover
					currentNamespace={match.namespace}
				/>
				<ActorBreadcrumbSegment />
			</div>
		);
	}

	return (
		<Popover open={isOpen} onOpenChange={setIsOpen}>
			<PopoverTrigger asChild>
				<Button
					variant={inline ? "ghost" : "outline"}
					className={cn(
						inline && "gap-2",
						"flex h-auto justify-between items-center px-2 py-1.5",
					)}
					endIcon={<UnfoldIcon />}
				>
					<Breadcrumbs inline={inline} />
				</Button>
			</PopoverTrigger>
			{/*
			 * `closeAnimation={false}`: switching project/namespace/actor
			 * navigates to a route with an async `beforeLoad`. The router runs
			 * that navigation inside a React transition, and while it is pending
			 * the old tree is held on screen. Radix keeps the popover mounted
			 * for its close animation, so the held transition freezes the
			 * closing popover and it lingers until the navigation resolves.
			 * Closing without an exit animation lets it unmount immediately.
			 */}
			<PopoverContent
				className="p-0 w-fit max-w-[calc(12rem*3)]"
				align="start"
				closeAnimation={false}
			>
				<Content onClose={() => setIsOpen(false)} />
			</PopoverContent>
		</Popover>
	);
}

// Thin diagonal slash divider between breadcrumb segments, matching the
// Supabase dashboard nav. Rendered between the logo and each segment.
function BreadcrumbSlash({ className }: { className?: string }) {
	return (
		<span
			className={cn(
				"mx-0.5 shrink-0 text-muted-foreground/40",
				className,
			)}
			aria-hidden
		>
			<svg
				viewBox="0 0 24 24"
				width="16"
				height="16"
				stroke="currentColor"
				strokeWidth="1"
				strokeLinecap="round"
				strokeLinejoin="round"
				fill="none"
				shapeRendering="geometricPrecision"
			>
				<path d="M16 3.549L7.12 20.600" />
			</svg>
		</span>
	);
}

// Up/down "unfold" chevron used on each segment's switcher trigger.
function UnfoldIcon({ className }: { className?: string }) {
	return (
		<svg
			viewBox="0 0 24 24"
			width="14"
			height="14"
			stroke="currentColor"
			strokeWidth="2"
			strokeLinecap="round"
			strokeLinejoin="round"
			fill="none"
			className={cn("size-3 opacity-60", className)}
			aria-hidden
		>
			<path d="m7 15 5 5 5-5" />
			<path d="m7 9 5-5 5 5" />
		</svg>
	);
}

function OrgSegmentPopover({ organization }: { organization: string }) {
	const [open, setOpen] = useState(false);
	const navigate = useNavigate();
	const { data: orgData } = useQuery(
		useCloudDataProvider().organizationQueryOptions({ org: organization }),
	);
	const label = orgData?.name ?? organization;

	return (
		<Popover open={open} onOpenChange={setOpen}>
			<div className="flex items-center">
				<Button
					variant="ghost"
					onClick={() => {
						void navigate({
							to: "/orgs/$organization",
							params: { organization },
							search: (old) => ({
								...old,
								n: undefined,
								actorId: undefined,
								actorKey: undefined,
								settings: undefined,
							}),
						});
					}}
					className="flex h-auto items-center gap-2 px-2 py-1 text-sm font-medium text-foreground rounded-lg hover:bg-foreground/[0.06]"
				>
					<span className="truncate">{label}</span>
				</Button>
				<PopoverTrigger asChild>
					<Button
						variant="ghost"
						aria-label="Open organization switcher"
						className="flex h-auto items-center self-stretch px-1.5 py-1 text-foreground rounded-lg hover:bg-foreground/[0.06] data-[state=open]:bg-foreground/[0.06]"
					>
						<UnfoldIcon />
					</Button>
				</PopoverTrigger>
			</div>
			<PopoverContent
				className="p-0 w-56"
				align="start"
				closeAnimation={false}
			>
				<OrgList
					currentOrganization={organization}
					onClose={() => setOpen(false)}
				/>
			</PopoverContent>
		</Popover>
	);
}

function OrgList({
	currentOrganization,
	onClose,
}: {
	currentOrganization: string;
	onClose?: () => void;
}) {
	const { data: organizations, isPending } =
		authClient.useListOrganizations();
	const navigate = useNavigate();
	const router = useRouter();

	return (
		<div className="w-full">
			<Command loop>
				<CommandInput placeholder="Find organization..." />
				<CommandList
					className="relative p-1 w-full"
					defaultValue={currentOrganization}
				>
					<CommandGroup heading="Organizations" className="w-full">
						{isPending ? (
							<>
								<ListItemSkeleton />
								<ListItemSkeleton />
								<ListItemSkeleton />
							</>
						) : null}

						{organizations?.map((org) => {
							const isCurrent = org.slug === currentOrganization;
							return (
								<CommandItem
									key={org.id}
									value={org.slug}
									keywords={[org.name, org.slug]}
									className="static w-full"
									onSelect={async () => {
										onClose?.();
										await navigate({
											to: "/orgs/$organization",
											params: { organization: org.slug },
										});
										// Force a fresh load of the new org's
										// matches so a prior errored route does
										// not keep rendering its errorComponent.
										await router.invalidate();
									}}
								>
									<Icon
										icon={faCheck}
										className={cn(
											"mr-2 size-3 shrink-0 text-primary",
											isCurrent
												? "opacity-100"
												: "opacity-0",
										)}
									/>
									<span className="truncate flex-1">
										{org.name}
									</span>
								</CommandItem>
							);
						})}

						<CommandItem
							keywords={["create", "new", "organization"]}
							className="text-primary"
							onSelect={() => {
								onClose?.();
								return navigate({ to: "/new-org" });
							}}
						>
							<Icon
								icon={faPlus}
								className="mr-2 size-3 text-primary"
							/>
							New Organization
						</CommandItem>
					</CommandGroup>
				</CommandList>
			</Command>
		</div>
	);
}

function ProjectSegmentPopover({
	organization,
	currentProject,
}: {
	organization: string;
	currentProject: string;
}) {
	const [open, setOpen] = useState(false);
	const navigate = useNavigate();
	const { data: projectData } = useQuery(
		useCloudDataProvider().currentOrgProjectQueryOptions({
			project: currentProject,
		}),
	);
	const { data: projects } = useInfiniteQuery(
		useCloudDataProvider().currentOrgProjectsQueryOptions(),
	);
	const label =
		projectData?.displayName ??
		projects?.find((project) => project.name === currentProject)
			?.displayName ??
		currentProject;

	return (
		<Popover open={open} onOpenChange={setOpen}>
			<div className="flex items-center">
				<Button
					variant="ghost"
					onClick={() => {
						authClient.organization.setActive({
							organizationSlug: organization,
						});
						void navigate({
							to: "/orgs/$organization/projects/$project",
							params: { organization, project: currentProject },
							search: (old) => ({
								...old,
								n: undefined,
								actorId: undefined,
								actorKey: undefined,
								settings: undefined,
							}),
						});
					}}
					className="flex h-auto items-center gap-2 px-1 py-1 text-sm font-medium text-foreground rounded-lg hover:bg-foreground/[0.06]"
				>
					<span className="truncate">{label}</span>
					<LazyBillingPlanBadge
						project={currentProject}
						organization={organization}
						eager
					/>
				</Button>
				<PopoverTrigger asChild>
					<Button
						variant="ghost"
						aria-label="Open project switcher"
						className="flex h-auto items-center self-stretch px-1 py-1 text-foreground rounded-lg hover:bg-foreground/[0.06] data-[state=open]:bg-foreground/[0.06]"
					>
						<UnfoldIcon />
					</Button>
				</PopoverTrigger>
			</div>
			<PopoverContent
				className="p-0 w-56"
				align="start"
				closeAnimation={false}
			>
				<ProjectList
					organization={organization}
					currentProject={currentProject}
					onClose={() => setOpen(false)}
				/>
			</PopoverContent>
		</Popover>
	);
}

function NamespaceSegmentPopover({
	organization,
	currentProject,
	currentNamespace,
}: {
	organization: string;
	currentProject: string;
	currentNamespace: string;
}) {
	const [open, setOpen] = useState(false);
	const navigate = useNavigate();
	const { data: nsData } = useQuery(
		useCloudDataProvider().currentOrgProjectNamespaceQueryOptions({
			project: currentProject,
			namespace: currentNamespace,
		}),
	);
	const { data: namespaces } = useInfiniteQuery(
		useCloudDataProvider().currentOrgProjectNamespacesQueryOptions({
			project: currentProject,
		}),
	);
	const label =
		nsData?.displayName ??
		namespaces?.find((namespace) => namespace.name === currentNamespace)
			?.displayName ??
		currentNamespace;

	return (
		<Popover open={open} onOpenChange={setOpen}>
			<div className="flex items-center">
				<Button
					variant="ghost"
					onClick={() => {
						authClient.organization.setActive({
							organizationSlug: organization,
						});
						void navigate({
							to: "/orgs/$organization/projects/$project/ns/$namespace",
							params: {
								organization,
								project: currentProject,
								namespace: currentNamespace,
							},
							search: (old) => ({
								...old,
								n: undefined,
								actorId: undefined,
								actorKey: undefined,
								settings: undefined,
							}),
						});
					}}
					className="flex h-auto items-center px-2 py-1 text-sm font-medium text-foreground rounded-lg hover:bg-foreground/[0.06]"
				>
					<span className="truncate">{label}</span>
				</Button>
				<PopoverTrigger asChild>
					<Button
						variant="ghost"
						aria-label="Open namespace switcher"
						className="flex h-auto items-center self-stretch px-1.5 py-1 text-foreground rounded-lg hover:bg-foreground/[0.06] data-[state=open]:bg-foreground/[0.06]"
					>
						<UnfoldIcon />
					</Button>
				</PopoverTrigger>
			</div>
			<PopoverContent
				className="p-0 w-56"
				align="start"
				closeAnimation={false}
			>
				<NamespaceList
					organization={organization}
					project={currentProject}
					currentNamespace={currentNamespace}
					onClose={() => setOpen(false)}
				/>
			</PopoverContent>
		</Popover>
	);
}

function EngineNamespaceSegmentPopover({
	currentNamespace,
}: {
	currentNamespace: string;
}) {
	const [open, setOpen] = useState(false);
	const navigate = useNavigate();
	const { data: nsData } = useQuery(
		useEngineCompatDataProvider().namespaceQueryOptions(currentNamespace),
	);
	const { data: namespaces } = useInfiniteQuery(
		useEngineCompatDataProvider().namespacesQueryOptions(),
	);
	const label =
		nsData?.displayName ??
		namespaces?.find((namespace) => namespace.name === currentNamespace)
			?.displayName ??
		currentNamespace;

	return (
		<Popover open={open} onOpenChange={setOpen}>
			<div className="flex items-center">
				<Button
					variant="ghost"
					onClick={() => {
						void navigate({
							to: "/ns/$namespace",
							params: { namespace: currentNamespace },
							search: (old) => ({
								...old,
								n: undefined,
								actorId: undefined,
								actorKey: undefined,
								settings: undefined,
							}),
						});
					}}
					className="flex h-auto items-center px-2 py-1 text-sm font-medium text-foreground rounded-lg hover:bg-foreground/[0.06]"
				>
					<span className="truncate">{label}</span>
				</Button>
				<PopoverTrigger asChild>
					<Button
						variant="ghost"
						aria-label="Open namespace switcher"
						className="flex h-auto items-center self-stretch px-1.5 py-1 text-foreground rounded-lg hover:bg-foreground/[0.06] data-[state=open]:bg-foreground/[0.06]"
					>
						<UnfoldIcon />
					</Button>
				</PopoverTrigger>
			</div>
			<PopoverContent
				className="p-0 w-56"
				align="start"
				closeAnimation={false}
			>
				<EngineNamespaceList
					currentNamespace={currentNamespace}
					onClose={() => setOpen(false)}
				/>
			</PopoverContent>
		</Popover>
	);
}

function EngineNamespaceList({
	currentNamespace,
	onClose,
}: {
	currentNamespace: string;
	onClose?: () => void;
}) {
	const { data, hasNextPage, isLoading, isFetchingNextPage, fetchNextPage } =
		useInfiniteQuery(
			useEngineCompatDataProvider().namespacesQueryOptions(),
		);
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

	// Sort a copy (recently visited first) in a memo so we neither mutate the
	// query cache array nor re-sort on every render.
	const sortedNamespaces = useMemo(
		() =>
			[...(data ?? [])].sort((a, b) => {
				const aTime = getRecentTimestamp(RECENT_NAMESPACES_KEY, a.name);
				const bTime = getRecentTimestamp(RECENT_NAMESPACES_KEY, b.name);
				return bTime - aTime;
			}),
		[data],
	);

	return (
		<div className="w-full">
			<Command loop>
				<CommandInput placeholder="Find namespace..." />
				<CommandList
					className="relative p-1 w-full"
					defaultValue={currentNamespace}
				>
					<CommandGroup heading="Namespaces" className="w-full">
						{!isLoading && (data?.length ?? 0) === 0 ? (
							<CommandEmpty>
								No namespaces found.
								<Button
									className="mt-1"
									variant="outline"
									size="sm"
									startIcon={<Icon icon={faPlus} />}
									onClick={() => {
										onClose?.();
										return navigate({
											to: ".",
											search: (old) => ({
												...old,
												modal: "create-ns",
											}),
										});
									}}
								>
									New Namespace
								</Button>
							</CommandEmpty>
						) : null}

						{sortedNamespaces.map((ns) => {
							const isCurrent = ns.name === currentNamespace;
							return (
								<CommandItem
									key={ns.id}
									value={ns.name}
									keywords={[ns.displayName, ns.name]}
									className="group static w-full"
									onSelect={() => {
										onClose?.();
										return navigate({
											to: namespaceTo,
											params: { namespace: ns.name },
											search: (old) => ({ ...old }),
										});
									}}
								>
									<Icon
										icon={faCheck}
										className={cn(
											"mr-2 size-3 shrink-0 text-primary",
											isCurrent
												? "opacity-100"
												: "opacity-0",
										)}
									/>
									<span className="truncate flex-1">
										{ns.displayName}
									</span>
									<button
										type="button"
										aria-label={`Settings for ${ns.displayName}`}
										title="Namespace settings"
										onPointerDown={(e) => {
											// Keep cmdk's row onSelect from firing so the
											// gear is its own navigation, not a row switch.
											e.stopPropagation();
										}}
										onClick={(e) => {
											e.stopPropagation();
											e.preventDefault();
											onClose?.();
											void navigate({
												to: "/ns/$namespace",
												params: {
													namespace: ns.name,
												},
												search: {
													settings: "settings",
												},
											});
										}}
										// No opacity transition: the gear reveals in step
										// with the row's instant highlight (the row has no
										// transition). Kept identical to the cloud
										// `NamespaceList` gear.
										className={cn(
											"relative z-10 ml-2 -my-1 size-6 rounded inline-flex items-center justify-center shrink-0",
											"text-muted-foreground hover:text-foreground hover:bg-foreground/[0.08]",
											"opacity-0",
											"group-hover:opacity-100 group-data-[selected=true]:opacity-100 focus-visible:opacity-100",
											"focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
										)}
									>
										<Icon
											icon={faGear}
											className="size-3"
										/>
									</button>
								</CommandItem>
							);
						})}
						{isLoading || isFetchingNextPage ? (
							<>
								<ListItemSkeleton />
								<ListItemSkeleton />
								<ListItemSkeleton />
							</>
						) : null}

						<CommandItem
							keywords={["create", "new", "namespace"]}
							className="text-primary"
							onSelect={() => {
								onClose?.();
								return navigate({
									to: ".",
									search: (old) => ({
										...old,
										modal: "create-ns",
									}),
								});
							}}
						>
							<Icon
								icon={faPlus}
								className="mr-2 size-3 text-primary"
							/>
							New Namespace
						</CommandItem>

						{hasNextPage && !isFetchingNextPage ? (
							<VisibilitySensor onChange={fetchNextPage} />
						) : null}
					</CommandGroup>
				</CommandList>
			</Command>
		</div>
	);
}

const useContextSwitcherMatch = ():
	| {
			project: string;
			namespace: string;
			organization: string;
	  }
	| { organization: string; project: string }
	| { organization: string }
	| { namespace: string }
	| false => {
	const match = useMatchRoute();

	const matchNamespace = match({
		to: "/orgs/$organization/projects/$project/ns/$namespace",
		fuzzy: true,
	});

	if (matchNamespace) {
		return matchNamespace;
	}

	const matchProject = match({
		to: "/orgs/$organization/projects/$project",
		fuzzy: true,
	});

	if (matchProject) {
		return matchProject;
	}

	const matchEngineNamespace = match({
		to: "/ns/$namespace",
		fuzzy: true,
	});

	if (matchEngineNamespace) {
		return matchEngineNamespace;
	}

	const matchOrganization = match({
		to: "/orgs/$organization",
		fuzzy: true,
	});

	if (matchOrganization) {
		return matchOrganization;
	}

	return false;
};

function Breadcrumbs({ inline }: { inline?: boolean }) {
	const match = useContextSwitcherMatch();

	if (match && "project" in match && "namespace" in match) {
		return (
			<div
				className={cn(
					"flex items-center min-w-0",
					inline && "flex-row gap-2 max-w-full",
					!inline && "flex-col w-full",
				)}
			>
				<div
					className={cn(
						!inline && "text-xs min-w-0 w-full",
						"text-left text-muted-foreground flex",
						inline && "shrink-0",
					)}
				>
					<ProjectBreadcrumb
						project={match.project}
						className={cn(
							inline
								? "whitespace-nowrap"
								: "truncate min-w-0 max-w-full block",
							inline && "h-auto",
							!inline && "h-4",
						)}
					/>
				</div>
				{inline ? <BreadcrumbSlash /> : null}
				<div
					className={cn(
						!inline && "min-w-0 w-full",
						inline && "shrink-0",
					)}
				>
					<NamespaceBreadcrumb
						className={cn(
							"text-left block",
							inline ? "whitespace-nowrap" : "truncate",
						)}
						namespace={match.namespace}
						project={match.project}
					/>
				</div>
				{inline ? <ActorBreadcrumbSegment /> : null}
			</div>
		);
	}

	if (match && "project" in match) {
		return <ProjectBreadcrumb project={match.project} />;
	}

	if (match && "namespace" in match) {
		return (
			<EngineNamespaceBreadcrumb
				className="text-left truncate block"
				namespace={match.namespace}
			/>
		);
	}

	return null;
}

function ProjectBreadcrumb({
	project,
	className,
}: {
	project: string;
	className?: string;
}) {
	const { isLoading, data } = useQuery(
		useCloudDataProvider().currentOrgProjectQueryOptions({ project }),
	);
	if (isLoading) {
		return <Skeleton className={cn("h-5 w-32", className)} />;
	}

	return (
		<span className={className}>
			{data?.displayName || "Unknown Project"}
		</span>
	);
}

function NamespaceBreadcrumb({
	namespace,
	project,
	className,
}: {
	namespace: string;
	project: string;
	className?: string;
}) {
	const { isLoading, data } = useQuery(
		useCloudDataProvider().currentOrgProjectNamespaceQueryOptions({
			project,
			namespace,
		}),
	);
	if (isLoading) {
		return <Skeleton className="h-5 w-32" />;
	}

	return (
		<span className={className}>
			{data?.displayName || "Unknown Namespace"}
		</span>
	);
}

function ActorBreadcrumbSegment() {
	// biome-ignore lint/correctness/useHookAtTopLevel: guarded by the parent only rendering on namespace match
	const search = useSearch({ strict: false }) as { n?: string[] };
	const buildId = search.n?.[0];
	// `n` is a search param, so it is set before the namespace route's loader
	// resolves. The popover below reads that loader's data provider, so hold the
	// segment until it lands rather than rendering off the search param alone.
	const isDataProviderReady = useNamespaceDataProviderReady();

	if (!buildId || !isDataProviderReady) return null;

	return (
		<>
			<BreadcrumbSlash />
			<ActorSegmentPopover currentBuildId={buildId} />
		</>
	);
}

function ActorSegmentPopover({ currentBuildId }: { currentBuildId: string }) {
	const [open, setOpen] = useState(false);
	const navigate = useNavigate();
	const { data: builds = [] } = useInfiniteQuery(
		useEngineCompatDataProvider().buildsQueryOptions(),
	);

	const currentBuild = builds.find((b) => b.id === currentBuildId);
	const currentMeta = currentBuild?.name?.metadata as
		| Record<string, unknown>
		| undefined;
	const currentLabel =
		typeof currentMeta?.name === "string"
			? currentMeta.name
			: currentBuildId;

	const sorted = [...builds].sort((a, b) => {
		const an = (a.name?.metadata as Record<string, unknown> | undefined)
			?.name;
		const bn = (b.name?.metadata as Record<string, unknown> | undefined)
			?.name;
		const aLabel = typeof an === "string" ? an : a.id;
		const bLabel = typeof bn === "string" ? bn : b.id;
		return aLabel.localeCompare(bLabel);
	});

	return (
		<Popover open={open} onOpenChange={setOpen}>
			<PopoverTrigger asChild>
				<Button
					variant="ghost"
					className="flex h-auto items-center gap-1.5 px-2 py-1 text-sm font-medium text-foreground rounded-lg hover:bg-foreground/[0.06] data-[state=open]:bg-foreground/[0.06]"
					endIcon={<UnfoldIcon />}
				>
					<span className="truncate">{currentLabel}</span>
				</Button>
			</PopoverTrigger>
			<PopoverContent
				className="p-0 w-56"
				align="start"
				closeAnimation={false}
			>
				<div className="w-full">
					<Command loop>
						<CommandInput placeholder="Find actor..." />
						<CommandList
							className="relative p-1 w-full"
							defaultValue={currentBuildId}
						>
							<CommandGroup heading="Actors" className="w-full">
								{sorted.length === 0 ? (
									<CommandEmpty>No actors yet.</CommandEmpty>
								) : null}
								{sorted.map((build) => {
									const meta = build.name?.metadata as
										| Record<string, unknown>
										| undefined;
									const label =
										typeof meta?.name === "string"
											? meta.name
											: build.id;
									const isCurrent =
										build.id === currentBuildId;
									return (
										<CommandItem
											key={build.id}
											value={build.id}
											keywords={[label, build.id]}
											className="static w-full"
											onSelect={() => {
												setOpen(false);
												return navigate({
													to: ".",
													search: (old) => ({
														...(old as Record<
															string,
															unknown
														>),
														actorId: undefined,
														actorKey: undefined,
														settings: undefined,
														n: [build.id],
													}),
												});
											}}
										>
											<Icon
												icon={faCheck}
												className={cn(
													"mr-2 size-3 shrink-0 text-primary",
													isCurrent
														? "opacity-100"
														: "opacity-0",
												)}
											/>
											<span className="truncate w-full">
												{label}
											</span>
										</CommandItem>
									);
								})}
							</CommandGroup>
						</CommandList>
					</Command>
				</div>
			</PopoverContent>
		</Popover>
	);
}

function EngineNamespaceBreadcrumb({
	namespace,
	className,
}: {
	namespace: string;
	className?: string;
}) {
	const { isLoading, data } = useQuery(
		useEngineCompatDataProvider().namespaceQueryOptions(namespace),
	);
	if (isLoading) {
		return <Skeleton className={cn("h-5 w-32", className)} />;
	}

	return (
		<span className={className}>
			{data?.displayName || "Unknown Namespace"}
		</span>
	);
}

function Content({ onClose }: { onClose?: () => void }) {
	const params = useParams({
		strict: false,
		select: (p) => ({
			organization: p.organization,
			project: p.project,
			namespace: p.namespace,
		}),
	});

	const [currentProjectHover, setCurrentProjectHover] = useState<
		string | null
	>(params.project || null);
	const [currentNamespaceHover, setCurrentNamespaceHover] = useState<
		string | null
	>(params.namespace || null);

	if (!params.organization) {
		return;
	}

	return (
		<div className="flex w-full">
			<ProjectList
				organization={params.organization}
				onHover={(next) => {
					setCurrentProjectHover(next);
					// Reset namespace hover when project changes so we don't
					// render a stale Actors column.
					if (next !== currentProjectHover) {
						setCurrentNamespaceHover(null);
					}
				}}
				onClose={onClose}
			/>

			{currentProjectHover ? (
				<NamespaceList
					organization={params.organization}
					project={currentProjectHover}
					onHover={setCurrentNamespaceHover}
					onClose={onClose}
				/>
			) : null}

			{currentProjectHover && currentNamespaceHover ? (
				<ActorsList
					organization={params.organization}
					project={currentProjectHover}
					namespace={currentNamespaceHover}
					onClose={onClose}
				/>
			) : null}
		</div>
	);
}

function ProjectList({
	organization,
	onClose,
	onHover,
	currentProject,
}: {
	organization: string;
	onClose?: () => void;
	onHover?: (project: string | null) => void;
	currentProject?: string;
}) {
	const { data, hasNextPage, isLoading, isFetchingNextPage, fetchNextPage } =
		useInfiniteQuery(
			useCloudDataProvider().projectsQueryOptions({
				organization: organization,
			}),
		);
	const navigate = useNavigate();
	const paramsProject = useParams({
		strict: false,
		select(params) {
			return params.project;
		},
	});
	const project = currentProject ?? paramsProject;

	return (
		<div className="w-full">
			<Command loop>
				<CommandInput placeholder="Find project..." />
				<CommandList
					className="relative p-1 w-full"
					defaultValue={project}
				>
					<CommandGroup heading="Projects" className="w-full">
						{!isLoading ? (
							<CommandEmpty>
								No projects found.
								<Button
									className="mt-1"
									variant="outline"
									size="sm"
									startIcon={<Icon icon={faPlus} />}
									onClick={() => {
										onHover?.(null);
										onClose?.();
										return navigate({
											to: ".",
											search: (old) => ({
												...old,
												modal: "create-project",
												organization,
											}),
										});
									}}
								>
									New Project
								</Button>
							</CommandEmpty>
						) : null}

						{data
							?.sort((a, b) => {
								const aTime = getRecentTimestamp(
									RECENT_PROJECTS_KEY,
									a.name,
								);
								const bTime = getRecentTimestamp(
									RECENT_PROJECTS_KEY,
									b.name,
								);
								return bTime - aTime;
							})
							.map((p, index) => {
								const Component =
									index < 5
										? PrefetchedProjectListItem
										: ProjectListItem;
								return (
									<Component
										key={p.id}
										{...p}
										isCurrent={project === p.name}
										onHover={() => onHover?.(p.name)}
										organization={organization}
										onClose={onClose}
										onSelect={() => {
											onClose?.();
											authClient.organization.setActive({
												organizationSlug: organization,
											});
											return navigate({
												to: "/orgs/$organization/projects/$project",
												params: {
													organization: organization,
													project: p.name,
												},
												search: (old) => ({ ...old }),
											});
										}}
									/>
								);
							})}
						{isLoading || isFetchingNextPage ? (
							<>
								<ListItemSkeleton />
								<ListItemSkeleton />
								<ListItemSkeleton />
								<ListItemSkeleton />
								<ListItemSkeleton />
							</>
						) : null}

						<CommandItem
							keywords={["create", "new", "project"]}
							className="text-primary"
							onSelect={() => {
								onHover?.(null);
								onClose?.();
								return navigate({
									to: ".",
									search: (old) => ({
										...old,
										modal: "create-project",
										organization,
									}),
								});
							}}
						>
							<Icon
								icon={faPlus}
								className="mr-2 size-3 text-primary"
							/>
							New Project
						</CommandItem>

						{hasNextPage && !isFetchingNextPage ? (
							<VisibilitySensor onChange={fetchNextPage} />
						) : null}
					</CommandGroup>
				</CommandList>
			</Command>
		</div>
	);
}

function PrefetchedProjectListItem({
	id,
	name,
	displayName,
	...props
}: Rivet.Project & {
	organization: string;
	isCurrent?: boolean;
	onHover?: () => void;
	onSelect?: () => void;
	onClose?: () => void;
}) {
	usePrefetchInfiniteQuery({
		...useCloudDataProvider().currentOrgProjectNamespacesQueryOptions({
			project: name,
		}),
	});

	return (
		<ProjectListItem
			id={id}
			name={name}
			displayName={displayName}
			{...props}
		/>
	);
}

function ProjectListItem({
	id,
	name,
	displayName,
	organization,
	isCurrent,
	onHover,
	onSelect,
	onClose,
}: Rivet.Project & {
	onHover?: () => void;
	onSelect?: () => void;
	onClose?: () => void;
	organization: string;
	isCurrent?: boolean;
}) {
	const navigate = useNavigate();
	return (
		<SafeHover key={id} offset={40}>
			<CommandItem
				value={name}
				keywords={[displayName, name]}
				className="static w-full"
				onSelect={onSelect}
				onMouseEnter={onHover}
				onFocus={onHover}
			>
				<Icon
					icon={faCheck}
					className={cn(
						"mr-2 size-3 shrink-0 text-primary",
						isCurrent ? "opacity-100" : "opacity-0",
					)}
				/>
				<span className="truncate flex-1">{displayName}</span>
				{features.billing && (
					<button
						type="button"
						aria-label={`Billing for ${displayName}`}
						title="Manage billing"
						onPointerDown={(e) => e.stopPropagation()}
						onClick={(e) => {
							e.stopPropagation();
							e.preventDefault();
							onClose?.();
							authClient.organization.setActive({
								organizationSlug: organization,
							});
							void navigate({
								to: "/orgs/$organization/projects/$project",
								params: { organization, project: name },
								search: { settings: "billing" },
							});
						}}
						// `relative z-10` lifts the badge above SafeHover's
						// click-eating `::before` corridor, the same trick the
						// gear icon uses below.
						className="relative z-10 rounded-full focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
					>
						<LazyBillingPlanBadge
							project={name}
							organization={organization}
						/>
					</button>
				)}
			</CommandItem>
		</SafeHover>
	);
}

function ListItemSkeleton() {
	return (
		<div className="px-2 py-1.5">
			<Skeleton className="h-5 w-32" />
		</div>
	);
}

function NamespaceList({
	organization,
	project,
	onClose,
	onHover,
	currentNamespace,
}: {
	organization: string;
	project: string;
	onClose?: () => void;
	onHover?: (namespace: string | null) => void;
	currentNamespace?: string;
}) {
	const { data, hasNextPage, isLoading, isFetchingNextPage, fetchNextPage } =
		useInfiniteQuery(
			useCloudDataProvider().currentOrgProjectNamespacesQueryOptions({
				project,
			}),
		);
	const navigate = useNavigate();
	const paramsNamespace = useParams({
		strict: false,
		select(params) {
			return params.namespace;
		},
	});
	const leafFullPath = useMatches({
		select: (matches) => matches[matches.length - 1]?.fullPath,
	});
	const namespaceBase = "/orgs/$organization/projects/$project/ns/$namespace";
	const namespaceTo = (
		typeof leafFullPath === "string" &&
		leafFullPath.startsWith(namespaceBase)
			? leafFullPath
			: namespaceBase
	) as "/orgs/$organization/projects/$project/ns/$namespace";
	const namespace = currentNamespace ?? paramsNamespace;

	return (
		<div className="w-full">
			<Command loop>
				<CommandInput placeholder="Find namespace..." />
				<CommandList
					className="relative p-1 w-full"
					defaultValue={namespace}
				>
					<CommandGroup heading="Namespaces" className="w-full">
						{!isLoading ? (
							<CommandEmpty>
								No namespaces found.
								<Button
									className="mt-1"
									variant="outline"
									size="sm"
									startIcon={<Icon icon={faPlus} />}
									onClick={() => {
										onClose?.();
										return navigate({
											to: ".",
											search: (old) => ({
												...old,
												modal: "create-ns",
												project: project,
											}),
										});
									}}
								>
									New Namespace
								</Button>
							</CommandEmpty>
						) : null}

						{data
							?.sort((a, b) => {
								const aTime = getRecentTimestamp(
									RECENT_NAMESPACES_KEY,
									a.name,
								);
								const bTime = getRecentTimestamp(
									RECENT_NAMESPACES_KEY,
									b.name,
								);
								return bTime - aTime;
							})
							.map((ns) => {
								const isCurrent = ns.name === namespace;
								return (
									<SafeHover key={ns.id} offset={40}>
										<CommandItem
											value={ns.name}
											keywords={[ns.displayName, ns.name]}
											className="group static w-full"
											onMouseEnter={() =>
												onHover?.(ns.name)
											}
											onFocus={() => onHover?.(ns.name)}
											onSelect={() => {
												onClose?.();
												authClient.organization.setActive(
													{
														organizationSlug:
															organization,
													},
												);
												return navigate({
													to: namespaceTo,
													params: {
														organization:
															organization,
														project: project,
														namespace: ns.name,
													},
													search: (old) => ({
														...old,
													}),
												});
											}}
										>
											<Icon
												icon={faCheck}
												className={cn(
													"mr-2 size-3 shrink-0 text-primary",
													isCurrent
														? "opacity-100"
														: "opacity-0",
												)}
											/>
											<span className="truncate flex-1">
												{ns.displayName}
											</span>
											<button
												type="button"
												aria-label={`Settings for ${ns.displayName}`}
												title="Namespace settings"
												onPointerDown={(e) => {
													// Stop cmdk's onSelect from firing on the
													// parent CommandItem so the gear is its
													// own navigation, not a row click.
													e.stopPropagation();
												}}
												onClick={(e) => {
													e.stopPropagation();
													e.preventDefault();
													onClose?.();
													authClient.organization.setActive(
														{
															organizationSlug:
																organization,
														},
													);
													void navigate({
														to: "/orgs/$organization/projects/$project/ns/$namespace",
														params: {
															organization,
															project,
															namespace: ns.name,
														},
														search: {
															settings:
																"settings",
														},
													});
												}}
												// `relative z-10` is load-bearing: the SafeHover
												// parent paints a click-eating `::before` corridor
												// at `z-index: 1` that overlaps this column.
												// Without lifting the button above it, the
												// gear is unclickable on hover. No opacity
												// transition: the gear reveals in step with the
												// row's instant highlight; kept in sync with the
												// engine `EngineNamespaceList` gear.
												className={cn(
													"relative z-10 ml-2 -my-1 size-6 rounded inline-flex items-center justify-center shrink-0",
													"text-muted-foreground hover:text-foreground hover:bg-foreground/[0.08]",
													"opacity-0",
													"group-hover:opacity-100 group-data-[selected=true]:opacity-100 focus-visible:opacity-100",
													"focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
												)}
											>
												<Icon
													icon={faGear}
													className="size-3"
												/>
											</button>
										</CommandItem>
									</SafeHover>
								);
							})}
						{isLoading || isFetchingNextPage ? (
							<>
								<ListItemSkeleton />
								<ListItemSkeleton />
								<ListItemSkeleton />
								<ListItemSkeleton />
								<ListItemSkeleton />
							</>
						) : null}

						<CommandItem
							keywords={["create", "new", "namespace"]}
							className="text-primary"
							onSelect={() => {
								onClose?.();
								return navigate({
									to: ".",
									search: (old) => ({
										...old,
										modal: "create-ns",
										project: project,
									}),
								});
							}}
						>
							<Icon
								icon={faPlus}
								className="mr-2 size-3 text-primary"
							/>
							New Namespace
						</CommandItem>

						{hasNextPage ? (
							<VisibilitySensor onChange={fetchNextPage} />
						) : null}
					</CommandGroup>
				</CommandList>
			</Command>
		</div>
	);
}

function ActorsList({
	organization,
	project,
	namespace,
	onClose,
}: {
	organization: string;
	project: string;
	namespace: string;
	onClose?: () => void;
}) {
	const navigate = useNavigate();
	const {
		data: actors,
		isLoading,
		hasNextPage,
		isFetchingNextPage,
		fetchNextPage,
	} = useInfiniteQuery(
		useCloudDataProvider().currentOrgProjectNamespaceActorNamesQueryOptions(
			{
				project,
				namespace,
			},
		),
	);

	const sorted = [...(actors ?? [])].sort((a, b) => {
		const an = (a.name?.metadata as Record<string, unknown> | undefined)
			?.name;
		const bn = (b.name?.metadata as Record<string, unknown> | undefined)
			?.name;
		const aLabel = typeof an === "string" ? an : a.id;
		const bLabel = typeof bn === "string" ? bn : b.id;
		return aLabel.localeCompare(bLabel);
	});

	return (
		<div className="border-l w-48">
			<Command loop>
				<CommandInput placeholder="Find Actor..." />
				<CommandList className="relative p-1 w-full">
					<CommandGroup heading="Actors" className="w-full">
						{!isLoading && sorted.length === 0 ? (
							<CommandEmpty>No actors yet.</CommandEmpty>
						) : null}
						{isLoading ? (
							<>
								<ListItemSkeleton />
								<ListItemSkeleton />
								<ListItemSkeleton />
							</>
						) : null}
						{sorted.map((actor) => {
							const meta = actor.name?.metadata as
								| Record<string, unknown>
								| undefined;
							const label =
								typeof meta?.name === "string"
									? meta.name
									: actor.id;
							return (
								<CommandItem
									key={actor.id}
									value={actor.id}
									keywords={[label, actor.id]}
									className="static w-full"
									onSelect={() => {
										onClose?.();
										authClient.organization.setActive({
											organizationSlug: organization,
										});
										return navigate({
											to: "/orgs/$organization/projects/$project/ns/$namespace",
											params: {
												organization,
												project,
												namespace,
											},
											search: (old) => ({
												...(old as Record<
													string,
													unknown
												>),
												actorId: undefined,
												actorKey: undefined,
												settings: undefined,
												n: [actor.id],
											}),
										});
									}}
								>
									<span className="truncate w-full">
										{label}
									</span>
								</CommandItem>
							);
						})}
						{isFetchingNextPage ? (
							<>
								<ListItemSkeleton />
								<ListItemSkeleton />
							</>
						) : null}
						{hasNextPage && !isFetchingNextPage ? (
							<VisibilitySensor onChange={fetchNextPage} />
						) : null}

						<CommandItem
							keywords={["create", "new", "actor"]}
							onSelect={() => {
								onClose?.();
								authClient.organization.setActive({
									organizationSlug: organization,
								});
								return navigate({
									to: "/orgs/$organization/projects/$project/ns/$namespace",
									params: {
										organization,
										project,
										namespace,
									},
									search: (old) => ({
										...(old as Record<string, unknown>),
										modal: "create-actor",
									}),
								});
							}}
						>
							<Icon icon={faPlusCircle} className="mr-2" />
							Create Actor
						</CommandItem>
					</CommandGroup>
				</CommandList>
			</Command>
		</div>
	);
}
