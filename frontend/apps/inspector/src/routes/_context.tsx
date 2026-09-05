import {
	createFileRoute,
	isRedirect,
	Outlet,
	redirect,
	useNavigate,
	useRouteContext,
} from "@tanstack/react-router";
import { zodValidator } from "@tanstack/zod-adapter";
import z from "zod";
import { getInspectorClientEndpoint } from "@/app/data-providers/inspector-data-provider";
import { useDialog } from "@/components";
import { DataProviderContext } from "@/components/actors";
import { ModalRenderer } from "@/components/modal-renderer";

const searchSchema = z
	.object({
		modal: z
			.enum(["feedback", "create-ns", "create-project", "billing"])
			.or(z.string())
			.optional(),
		utm_source: z.string().optional(),
		actorId: z.string().optional(),
		tab: z.string().optional(),
		n: z.array(z.string()).optional(),
		u: z.string().optional(),
		t: z.string().optional(),
	})
	.and(z.record(z.string(), z.any()));

export const Route = createFileRoute("/_context")({
	component: RouteComponent,
	validateSearch: zodValidator(searchSchema),
	context: ({ location: { search }, context }) => {
		const typedSearch = search as z.infer<typeof searchSchema>;
		return {
			dataProvider: context.getOrCreateInspectorContext({
				url: typedSearch.u || "http://localhost:6420",
				token: typedSearch.t,
			}),
			__type: "inspector" as const,
		};
	},
	beforeLoad: async (route) => {
		if (route.search.u) {
			try {
				const realUrl = await getInspectorClientEndpoint(
					route.search.u,
				);
				if (realUrl !== route.search.u) {
					throw redirect({
						to: route.location.pathname,
						search: {
							...route.search,
							u: realUrl,
						},
					});
				}
			} catch (e) {
				if (isRedirect(e)) {
					throw e;
				}
				// Ignore errors here.
			}
		}
	},
	loaderDeps: (route) => ({ token: route.search.t, url: route.search.u }),
});

function RouteComponent() {
	const dataProvider = useRouteContext({
		from: "/_context",
		select: (context) => context.dataProvider,
	});

	return (
		<DataProviderContext.Provider value={dataProvider}>
			<Outlet />
			<ModalRenderer />
			<Modals />
		</DataProviderContext.Provider>
	);
}

function Modals() {
	const navigate = useNavigate();
	const search = Route.useSearch();

	const CreateActorDialog = useDialog.CreateActor.Dialog;
	const FeedbackDialog = useDialog.Feedback.Dialog;

	return (
		<>
			<CreateActorDialog
				dialogProps={{
					open: search.modal === "create-actor",
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
		</>
	);
}
