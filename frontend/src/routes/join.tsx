import { createFileRoute } from "@tanstack/react-router";
import { Logo } from "@/app/logo";
import { SignUp } from "@/app/sign-up";
import { authClient, redirectToOrganization } from "@/lib/auth";
import { features } from "@/lib/features";

export const Route = createFileRoute("/join")({
	component: RouteComponent,
	beforeLoad: async ({ search }) => {
		if (features.auth) {
			const session = await authClient.getSession();
			if (session.data) {
				await redirectToOrganization({
					from:
						"from" in search ? (search.from as string) : undefined,
				});
			}
		}
	},
});

function RouteComponent() {
	return (
		<div className="flex min-h-screen flex-col items-center justify-center bg-background py-4">
			<div className="flex flex-col items-center gap-6 w-full">
				<Logo className="h-10 mb-4" />
				<SignUp />
			</div>
		</div>
	);
}
