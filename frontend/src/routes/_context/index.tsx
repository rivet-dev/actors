import { createFileRoute, isRedirect, redirect } from "@tanstack/react-router";
import CreateNamespacesFrameContent from "@/app/dialogs/create-namespace-frame";
import { Logo } from "@/app/logo";
import { Card } from "@/components";
import { redirectToOrganization } from "@/lib/auth";
import { features } from "@/lib/features";

export const Route = createFileRoute("/_context/")({
	component: () => (features.platform ? <CloudRoute /> : <EngineRoute />),
	beforeLoad: async ({ context, search }) => {
		if (features.platform) {
			if (!(await redirectToOrganization(search))) {
				throw redirect({ to: "/new-org" });
			}
			return;
		}

		const ctx = context as Extract<typeof context, { __type: "engine" }>;
		try {
			const result = await ctx.queryClient.fetchInfiniteQuery(
				ctx.dataProvider.namespacesQueryOptions(),
			);

			const firstNamespace = result.pages[0]?.namespaces[0];
			if (!firstNamespace) {
				throw redirect({
					to: "/ns/$namespace",
					params: { namespace: "default" },
					search: true,
				});
			}
			throw redirect({
				to: "/ns/$namespace",
				params: { namespace: firstNamespace.name },
				search: true,
			});
		} catch (e) {
			if (isRedirect(e)) {
				throw e;
			}

			// Ignore errors here, they will be handled in the UI
			return;
		}
	},
});

function CloudRoute() {
	return null;
}

function EngineRoute() {
	return (
		<div className="flex min-h-screen flex-col items-center justify-center bg-background py-4">
			<div className="flex flex-col items-center gap-6 w-full">
				<Logo className="h-10 mb-4" />
				<Card className="w-full sm:w-96">
					<CreateNamespacesFrameContent />
				</Card>
			</div>
		</div>
	);
}
