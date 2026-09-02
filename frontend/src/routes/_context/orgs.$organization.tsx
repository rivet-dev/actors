import { createFileRoute, Outlet, redirect } from "@tanstack/react-router";
import { match } from "ts-pattern";
import { authClient } from "@/lib/auth";

export const Route = createFileRoute("/_context/orgs/$organization")({
	component: RouteComponent,
	context: ({ context, params }) =>
		match(context)
			.with({ __type: "cloud" }, (context) => ({
				dataProvider: context.getOrCreateOrganizationContext(
					context.dataProvider,
					params.organization,
				),
			}))
			.otherwise(() => {
				throw new Error("Invalid context type for this route");
			}),
	beforeLoad: async ({ params }) => {
		const org = await authClient.organization.getFullOrganization({
			query: { organizationSlug: params.organization },
		});

		// Redirect instead of throwing notFound(). notFound() leaves descendant
		// matches stuck in `pending` while their layout components keep
		// rendering, which crashes useCloudDataProvider() /
		// useCloudProjectDataProvider() consumers. The destination must not
		// resolve an organization itself, or an unresolvable slug ping-pongs
		// between the two routes forever.
		if (org.error) {
			if (org.error.status === 403 || org.error.status === 404) {
				throw redirect({ to: "/new-org" });
			}
			throw new Error(org.error.message ?? "Failed to load organization");
		}

		const session = await authClient.getSession();
		if (session.data?.session.activeOrganizationId !== org.data.id) {
			await authClient.organization.setActive({
				organizationId: org.data.id,
			});
		}

		return { org: org.data };
	},
	loader: ({ context }) => ({ dataProvider: context.dataProvider }),
	pendingMinMs: 0,
	pendingMs: 0,
});

function RouteComponent() {
	return <Outlet />;
}
