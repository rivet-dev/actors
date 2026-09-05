import {
	faEllipsisVertical,
	faGear,
	faPlus,
	faUserPlus,
	Icon,
} from "@rivet-gg/icons";
import { useInfiniteQuery, useMutation } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { AnimatePresence, motion } from "framer-motion";
import { useState } from "react";
import * as InviteMemberForm from "@/app/forms/invite-member-form";
import {
	Avatar,
	AvatarFallback,
	AvatarImage,
	Button,
	cn,
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
	H1,
	RelativeTime,
	ScrollArea,
	Skeleton,
	SmallText,
	toast,
	WithTooltip,
} from "@/components";
import { useCloudDataProvider } from "@/components/actors";
import { RouteLayout } from "./route-layout";
import { authClient } from "@/lib/auth";
import { orgConicGradient, paletteForLetter } from "@/lib/org-palette";
import {
	getRecentTimestamp,
	RECENT_PROJECTS_KEY,
} from "@/lib/recently-visited";
import { VisibilitySensor } from "@/components/visibility-sensor";
import { queryClient } from "@/queries/global";
import { LazyBillingPlanBadge } from "./billing/billing-plan-badge";

export function OrgLanding({ organization }: { organization: string }) {
	const navigate = useNavigate();
	const dataProvider = useCloudDataProvider();
	const {
		data: projects = [],
		isLoading,
		hasNextPage,
		fetchNextPage,
		isFetchingNextPage,
	} = useInfiniteQuery(dataProvider.currentOrgProjectsQueryOptions());
	const { data: org } = authClient.useActiveOrganization();
	const { data: session } = authClient.useSession();

	const sorted = [...projects].sort((a, b) => {
		const aTime = getRecentTimestamp(RECENT_PROJECTS_KEY, a.name);
		const bTime = getRecentTimestamp(RECENT_PROJECTS_KEY, b.name);
		if (aTime !== bTime) return bTime - aTime;
		return a.displayName.localeCompare(b.displayName);
	});

	return (
		<div className="flex flex-1 min-h-0 my-2 mr-2 overflow-hidden rounded-xl border border-foreground/10 bg-card">
			<ScrollArea className="h-full w-full">
				<div className="px-6 py-6 max-w-6xl mx-auto space-y-8">
					<header className="flex items-center justify-between gap-4 pb-6 border-b border-foreground/10">
						<div className="flex items-center gap-3 min-w-0">
							<Avatar className="size-10 shrink-0">
								<AvatarImage src={org?.logo ?? undefined} />
								<AvatarFallback
									className="text-white text-base font-semibold"
									style={{
										backgroundImage: orgConicGradient(
											paletteForLetter(org?.name ?? ""),
										),
									}}
								>
									{(org?.name?.[0] ?? "?").toUpperCase()}
								</AvatarFallback>
							</Avatar>
							<H1 className="text-2xl truncate">
								{org?.name ?? "Organization"}
							</H1>
						</div>
						<WithTooltip
							content="Organization settings"
							trigger={
								<Button
									variant="outline"
									size="icon-sm"
									aria-label="Organization settings"
									onClick={() => {
										navigate({
											to: ".",
											search: (old) => ({
												...(old as Record<
													string,
													unknown
												>),
												settings: "organization",
											}),
										});
									}}
								>
									<Icon icon={faGear} />
								</Button>
							}
						/>
					</header>

					<section>
						<header className="flex items-center justify-between gap-4 mb-3">
							<h2 className="text-base font-semibold text-foreground">
								Projects
							</h2>
							{sorted.length > 0 ? (
								<Button
									variant="outline"
									size="sm"
									startIcon={<Icon icon={faPlus} />}
									onClick={() => {
										navigate({
											to: ".",
											search: (old) => ({
												...(old as Record<
													string,
													unknown
												>),
												modal: "create-project",
												organization,
											}),
										});
									}}
								>
									Create Project
								</Button>
							) : null}
						</header>

						{!isLoading && sorted.length === 0 ? (
							<div className="flex flex-col items-center gap-3 rounded-md border border-dashed bg-card/50 px-6 py-10 text-center">
								<H1 className="text-base">No projects yet</H1>
								<SmallText className="text-muted-foreground max-w-md">
									Create a project to start deploying actors
									in this organization.
								</SmallText>
								<Button
									variant="default"
									size="sm"
									startIcon={<Icon icon={faPlus} />}
									onClick={() => {
										navigate({
											to: ".",
											search: (old) => ({
												...(old as Record<
													string,
													unknown
												>),
												modal: "create-project",
												organization,
											}),
										});
									}}
								>
									Create Project
								</Button>
							</div>
						) : (
							<>
								<div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-3">
									{sorted.map((project) => (
										<div
											key={project.id}
											className="group relative min-h-[130px] transition-transform duration-150 hover:-translate-y-0.5 active:translate-y-0"
										>
											<Link
												to="/orgs/$organization/projects/$project"
												params={{
													organization,
													project: project.name,
												}}
												className={cn(
													"flex h-full flex-col items-start gap-2 rounded-lg border border-foreground/10 bg-foreground/[0.02] p-4 text-left transition-[background-color,border-color,box-shadow] duration-150",
													"group-hover:border-foreground/25 group-hover:bg-foreground/[0.06] group-hover:shadow-sm",
													"group-active:shadow-none",
													"focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
													"cursor-pointer",
												)}
											>
												<div className="font-medium text-sm leading-tight truncate pr-14 w-full">
													{project.displayName}
												</div>
												<SmallText className="text-muted-foreground text-xs leading-tight font-mono-console truncate w-full">
													{project.name}
												</SmallText>
												{project.createdAt ? (
													<SmallText className="text-muted-foreground text-[11px] mt-auto pt-1">
														Created{" "}
														<RelativeTime
															time={
																new Date(
																	project.createdAt,
																)
															}
														/>
													</SmallText>
												) : null}
											</Link>
											<button
												type="button"
												onClick={(e) => {
													e.preventDefault();
													e.stopPropagation();
													void navigate({
														to: "/orgs/$organization/projects/$project",
														params: {
															organization,
															project:
																project.name,
														},
														search: {
															settings: "billing",
														},
													});
												}}
												title="Manage billing"
												aria-label="Manage billing"
												className="absolute top-3 right-3 z-10 rounded-full focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
											>
												<LazyBillingPlanBadge
													project={project.name}
													organization={organization}
												/>
											</button>
										</div>
									))}
								</div>
								{hasNextPage && !isFetchingNextPage ? (
									<VisibilitySensor
										onChange={() => fetchNextPage()}
									/>
								) : null}
							</>
						)}
					</section>

					<MembersSection
						organization={organization}
						members={org?.members ?? []}
						currentUserId={session?.user?.id}
					/>
				</div>
			</ScrollArea>
		</div>
	);
}

OrgLanding.Skeleton = function OrgLandingSkeleton() {
	return (
		<div className="flex flex-1 min-h-0 my-2 mr-2 overflow-hidden rounded-xl border border-foreground/10 bg-card">
			<ScrollArea className="h-full w-full">
				<div className="px-6 py-6 max-w-6xl mx-auto space-y-8">
					<header className="flex items-center justify-between gap-4 pb-6 border-b border-foreground/10">
						<div className="flex items-center gap-3 min-w-0">
							<Skeleton className="size-10 rounded-full shrink-0" />
							<Skeleton className="h-8 w-48" />
						</div>
						<Skeleton className="size-8 rounded-md" />
					</header>

					<section>
						<header className="flex items-center justify-between gap-4 mb-3">
							<Skeleton className="h-5 w-20" />
							<Skeleton className="h-8 w-32 rounded-md" />
						</header>
						<div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-3">
							{Array.from({ length: 4 }).map((_, i) => (
								<div
									// biome-ignore lint/suspicious/noArrayIndexKey: static skeleton cards
									key={i}
									className="flex min-h-[130px] flex-col items-start gap-2 rounded-lg border border-foreground/10 bg-foreground/[0.02] p-4"
								>
									<Skeleton className="h-4 w-28" />
									<Skeleton className="h-3 w-20" />
									<Skeleton className="h-3 w-16 mt-auto" />
								</div>
							))}
						</div>
					</section>

					<section>
						<header className="flex items-center justify-between gap-4 mb-3">
							<Skeleton className="h-5 w-20" />
							<Skeleton className="h-8 w-32 rounded-md" />
						</header>
						<div className="rounded-md border border-foreground/10 bg-card overflow-hidden">
							<div className="grid grid-cols-[1fr_120px_28px] gap-4 items-center px-3 py-2 text-[11px] font-medium uppercase tracking-wide text-muted-foreground border-b border-foreground/10 bg-foreground/[0.02]">
								<div>Member</div>
								<div>Role</div>
								<div />
							</div>
							{Array.from({ length: 3 }).map((_, i) => (
								<div
									// biome-ignore lint/suspicious/noArrayIndexKey: static skeleton rows
									key={i}
									className="grid grid-cols-[1fr_120px_28px] gap-4 items-center px-3 py-2.5 border-b border-foreground/10 last:border-b-0"
								>
									<div className="flex items-center gap-2 min-w-0">
										<Skeleton className="size-6 rounded-full shrink-0" />
										<Skeleton className="h-4 w-32" />
									</div>
									<Skeleton className="h-5 w-16 rounded-full" />
									<div />
								</div>
							))}
						</div>
					</section>
				</div>
			</ScrollArea>
		</div>
	);
};

export function OrgLandingPending() {
	return (
		<RouteLayout>
			<OrgLanding.Skeleton />
		</RouteLayout>
	);
}

function MembersSection({
	organization,
	members,
	currentUserId,
}: {
	organization: string;
	members: Array<{
		id: string;
		userId: string;
		role?: string;
		user?: {
			name?: string | null;
			email?: string | null;
			image?: string | null;
		};
	}>;
	currentUserId: string | undefined;
}) {
	const navigate = useNavigate();
	const [showInvite, setShowInvite] = useState(false);
	const visible = members.slice(0, 5);
	const remaining = Math.max(0, members.length - visible.length);

	const { data: org } = authClient.useActiveOrganization();
	const organizationId = org?.id;

	const openMembers = () => {
		navigate({
			to: ".",
			search: (old) => ({
				...(old as Record<string, unknown>),
				modal: "org-members",
				organization,
			}),
		});
	};

	const { mutateAsync: inviteMember } = useMutation({
		mutationFn: async (email: string) => {
			if (!organizationId) throw new Error("No active organization");
			const result = await authClient.organization.inviteMember({
				email,
				role: "member",
				organizationId,
			});
			if (result.error) throw result.error;
		},
		onSuccess: () => {
			toast.success("Invitation sent.");
			void queryClient.invalidateQueries({
				queryKey: ["organizations"],
			});
		},
	});

	return (
		<section>
			<header className="flex items-center justify-between gap-4 mb-3">
				<h2 className="text-base font-semibold text-foreground">
					Members
				</h2>
				<Button
					variant="outline"
					size="sm"
					startIcon={<Icon icon={faUserPlus} />}
					onClick={() => setShowInvite((v) => !v)}
				>
					Invite member
				</Button>
			</header>
			<AnimatePresence initial={false}>
				{showInvite ? (
					<motion.div
						key="invite-form"
						initial={{ height: 0, opacity: 0, marginBottom: 0 }}
						animate={{
							height: "auto",
							opacity: 1,
							marginBottom: 12,
						}}
						exit={{ height: 0, opacity: 0, marginBottom: 0 }}
						transition={{
							height: { duration: 0.22, ease: "easeInOut" },
							marginBottom: { duration: 0.22, ease: "easeInOut" },
							opacity: { duration: 0.15, ease: "easeInOut" },
						}}
						className="overflow-hidden"
					>
						<div className="rounded-lg border border-foreground/10 bg-card p-3">
							<InviteMemberForm.Form
								defaultValues={{ email: "" }}
								mode="onSubmit"
								revalidateMode="onSubmit"
								onSubmit={async ({ email }, form) => {
									try {
										await inviteMember(email);
										form.reset();
										setShowInvite(false);
									} catch {
										form.setError("root", {
											message:
												"Failed to send invitation.",
										});
									}
								}}
							>
								<div className="grid grid-cols-[1fr_auto] gap-2 items-start">
									<InviteMemberForm.EmailField />
									<InviteMemberForm.Submit>
										Invite
									</InviteMemberForm.Submit>
								</div>
							</InviteMemberForm.Form>
						</div>
					</motion.div>
				) : null}
			</AnimatePresence>
			<div className="rounded-md border border-foreground/10 bg-card overflow-hidden">
				{visible.length === 0 ? (
					<div className="px-3 py-4 text-center">
						<SmallText className="text-muted-foreground">
							No members yet.
						</SmallText>
					</div>
				) : (
					<>
						<div className="grid grid-cols-[1fr_120px_28px] gap-4 items-center px-3 py-2 text-[11px] font-medium uppercase tracking-wide text-muted-foreground border-b border-foreground/10 bg-foreground/[0.02]">
							<div>Member</div>
							<div>Role</div>
							<div />
						</div>
						{visible.map((member) => {
							const user = member.user;
							const initial =
								(user?.name ??
									user?.email ??
									"?")[0]?.toUpperCase() ?? "?";
							const isSelf = member.userId === currentUserId;
							const role = member.role ?? "member";
							return (
								<div
									key={member.id}
									className="group grid grid-cols-[1fr_120px_28px] gap-4 items-center px-3 py-2.5 text-xs border-b border-foreground/10 last:border-b-0 transition-colors hover:bg-foreground/[0.025]"
								>
									<div className="flex items-center gap-2 min-w-0">
										<Avatar className="size-6 shrink-0">
											<AvatarImage
												src={user?.image ?? undefined}
											/>
											<AvatarFallback>
												{initial}
											</AvatarFallback>
										</Avatar>
										<div className="min-w-0">
											<div className="flex items-center gap-1.5">
												<span className="font-medium text-foreground truncate">
													{user?.name ??
														user?.email ??
														"Unknown"}
												</span>
												{isSelf ? (
													<span className="inline-flex items-center rounded-full border border-primary/20 bg-primary/10 px-1.5 py-0 text-[10px] font-medium text-primary">
														You
													</span>
												) : null}
											</div>
											{user?.email &&
											user.email !== user.name ? (
												<div className="text-muted-foreground truncate text-[11px]">
													{user.email}
												</div>
											) : null}
										</div>
									</div>
									<div>
										<RolePill role={role} />
									</div>
									<div className="opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
										<MemberRowMenu
											memberId={member.id}
											userId={member.userId}
											role={role}
											isSelf={isSelf}
										/>
									</div>
								</div>
							);
						})}
					</>
				)}
				{remaining > 0 ? (
					<button
						type="button"
						onClick={openMembers}
						className="w-full px-3 py-2.5 text-xs text-left text-muted-foreground hover:bg-foreground/[0.04] transition-colors border-t border-foreground/10"
					>
						+{remaining} more
					</button>
				) : null}
			</div>
		</section>
	);
}

function RolePill({ role }: { role: string }) {
	const styles = (() => {
		switch (role) {
			case "owner":
				return "border-primary/20 bg-primary/10 text-primary";
			case "admin":
				return "border-foreground/15 bg-foreground/[0.06] text-foreground";
			default:
				return "border-foreground/10 bg-foreground/[0.03] text-muted-foreground";
		}
	})();
	return (
		<span
			className={cn(
				"inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-medium capitalize",
				styles,
			)}
		>
			{role}
		</span>
	);
}

function MemberRowMenu({
	memberId,
	userId,
	role,
	isSelf,
}: {
	memberId: string;
	userId: string;
	role: string;
	isSelf: boolean;
}) {
	const { data: org } = authClient.useActiveOrganization();
	const organizationId = org?.id;

	const invalidate = () =>
		queryClient.invalidateQueries({ queryKey: ["organizations"] });

	const { mutate: setRole, isPending: isUpdatingRole } = useMutation({
		mutationFn: async (nextRole: "owner" | "admin" | "member") => {
			if (!organizationId) throw new Error("No active organization");
			const result = await authClient.organization.updateMemberRole({
				memberId,
				role: nextRole,
				organizationId,
			});
			if (result.error) throw result.error;
		},
		onSuccess: (_data, nextRole) => {
			toast.success(`Member set to ${nextRole}.`);
			void invalidate();
		},
		onError: () => toast.error("Couldn't update member role."),
	});

	const { mutate: remove, isPending: isRemoving } = useMutation({
		mutationFn: async () => {
			if (!organizationId) throw new Error("No active organization");
			const result = await authClient.organization.removeMember({
				memberIdOrEmail: userId,
				organizationId,
			});
			if (result.error) throw result.error;
		},
		onSuccess: () => {
			toast.success("Member removed.");
			void invalidate();
		},
		onError: () => toast.error("Couldn't remove member."),
	});

	const isOwner = role === "owner";
	const isAdmin = role === "admin";
	const disabled = isUpdatingRole || isRemoving;

	if (isSelf) {
		return (
			<Button
				variant="ghost"
				size="icon-sm"
				type="button"
				aria-label="Member actions"
				aria-disabled
				title="You can't change your own role"
				onClick={(e) => e.preventDefault()}
				onPointerDown={(e) => e.preventDefault()}
				className="cursor-not-allowed opacity-50 hover:bg-transparent"
			>
				<Icon icon={faEllipsisVertical} />
			</Button>
		);
	}

	return (
		<DropdownMenu>
			<DropdownMenuTrigger asChild>
				<Button
					variant="ghost"
					size="icon-sm"
					aria-label="Member actions"
					disabled={disabled}
				>
					<Icon icon={faEllipsisVertical} />
				</Button>
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end" className="min-w-[10rem]">
				{!isOwner ? (
					<DropdownMenuItem onSelect={() => setRole("owner")}>
						Promote to owner
					</DropdownMenuItem>
				) : null}
				{!isAdmin && !isOwner ? (
					<DropdownMenuItem onSelect={() => setRole("admin")}>
						Promote to admin
					</DropdownMenuItem>
				) : null}
				{isOwner || isAdmin ? (
					<DropdownMenuItem onSelect={() => setRole("member")}>
						Demote to member
					</DropdownMenuItem>
				) : null}
				<DropdownMenuSeparator />
				<DropdownMenuItem
					onSelect={() => remove()}
					className="text-destructive focus:text-destructive"
				>
					Remove from organization
				</DropdownMenuItem>
			</DropdownMenuContent>
		</DropdownMenu>
	);
}
