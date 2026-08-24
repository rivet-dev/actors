import * as Sentry from "@sentry/react";
import {
	createFileRoute,
	Outlet,
	redirect,
	useNavigate,
	useSearch,
} from "@tanstack/react-router";
import posthog from "posthog-js";
import { useEffect } from "react";
import z from "zod";
import { SettingsDrawerHost } from "@/app/settings-drawer";
import { useDialog } from "@/app/use-dialog";
import { getConfig, ls } from "@/components";
import { ModalRenderer } from "@/components/modal-renderer";
import { authClient } from "@/lib/auth";
import { features } from "@/lib/features";

const searchSchema = z
	.object({
		modal: z
			.enum([
				"feedback",
				"create-ns",
				"create-project",
				"billing",
				"org-members",
			])
			.or(z.string())
			.optional(),
		settings: z.string().optional(),
		utm_source: z.string().optional(),
		actorId: z.string().optional(),
		tab: z.string().optional(),
		n: z.array(z.string()).optional(),
		u: z.string().optional(),
		t: z.string().optional(),
		from: z.string().optional(),
		project: z.string().optional(),
		pool: z.string().optional(),
	})
	.and(z.record(z.string(), z.any()));

export const Route = createFileRoute("/_context")({
	component: RouteComponent,
	validateSearch: (search) => {
		const validated = searchSchema.parse(search);
		// `pool` is scoped to the pages that actually use it: the Logs route
		// re-declares it in its own validateSearch, and the compute settings tab
		// needs it while open. Drop it everywhere else so the selected pool does
		// not cling to unrelated pages after navigating away.
		if (validated.settings !== "compute") {
			delete (validated as { pool?: string }).pool;
		}
		return validated;
	},
	context: ({ context }) => {
		if (features.platform) {
			return {
				dataProvider: context.getOrCreateCloudContext(),
				__type: "cloud" as const,
			};
		}
		return {
			dataProvider: context.getOrCreateEngineContext(
				() => ls.engineCredentials.get(getConfig().apiUrl) || "",
			),
			__type: "engine" as const,
		};
	},
	beforeLoad: async (route) => {
		if (features.platform) {
			const justVerifiedEmail =
				(route.location.search as { emailVerified?: string })
					?.emailVerified === "1";
			const session = await authClient.getSession(
				justVerifiedEmail
					? { query: { disableCookieCache: true } }
					: undefined,
			);

			if (!session.data) {
				throw redirect({
					to: "/login",
					search: (old) => ({
						...old,
						from: route.location.pathname,
					}),
				});
			}

			if (!session.data.user.emailVerified) {
				throw redirect({ to: "/verify-email-pending" });
			}
		}
	},
	loader: ({ context }) => ({ dataProvider: context.dataProvider }),
});

function IdentifyUser() {
	const { data: session } = authClient.useSession();

	useEffect(() => {
		const user = session?.user;
		if (!user) return;

		Sentry.setUser({ id: user.id, email: user.email });
		posthog.setPersonProperties({ id: user.id, email: user.email });
	}, [session?.user]);

	return null;
}

function RouteComponent() {
	return (
		<>
			{features.auth && <IdentifyUser />}
			<Outlet />
			<ModalRenderer />
			<Modals />
			{!features.platform && <EngineModals />}
			{features.platform && <CloudModals />}
			{/* The host self-gates on the `settings` search param and renders a
			    namespace-only nav on engine flavors, so it is safe to mount for
			    every flavor. */}
			<SettingsDrawerHost />
		</>
	);
}

function Modals() {
	const navigate = useNavigate();
	const search = Route.useSearch();

	const FeedbackDialog = useDialog.Feedback.Dialog;

	return (
		<FeedbackDialog
			dialogProps={{
				open: search.modal === "feedback",
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
	);
}

function EngineModals() {
	const navigate = useNavigate();
	const search = Route.useSearch();
	const CreateNamespaceDialog = useDialog.CreateNamespace.Dialog;
	return (
		<CreateNamespaceDialog
			dialogProps={{
				open: search.modal === "create-ns",
				onOpenChange: (value) => {
					if (!value) {
						return navigate({
							to: ".",
							search: (old) => ({ ...old, modal: undefined }),
						});
					}
				},
			}}
		/>
	);
}

function CloudModals() {
	const navigate = useNavigate();
	const search = useSearch({ strict: false });

	const CreateProjectDialog = useDialog.CreateProject.Dialog;
	const CreateOrganizationDialog = useDialog.CreateOrganization.Dialog;
	const OrgMembersDialog = useDialog.OrgMembers.Dialog;

	return (
		<>
			<CreateProjectDialog
				organization={search?.organization}
				dialogProps={{
					open: search?.modal === "create-project",
					onOpenChange: (value) => {
						if (!value) {
							return navigate({
								to: ".",
								search: (old) => ({ ...old, modal: undefined }),
							});
						}
					},
				}}
			/>
			<CreateOrganizationDialog
				dialogProps={{
					open: search?.modal === "create-organization",
					onOpenChange: (value) => {
						if (!value) {
							return navigate({
								to: ".",
								search: (old) => ({ ...old, modal: undefined }),
							});
						}
					},
				}}
			/>
			<OrgMembersDialog
				dialogProps={{
					open: search?.modal === "org-members",
					onOpenChange: (value) => {
						if (!value) {
							return navigate({
								to: ".",
								search: (old) => ({ ...old, modal: undefined }),
							});
						}
					},
				}}
			/>
		</>
	);
}
