import { createFileRoute, redirect } from "@tanstack/react-router";
import { toast } from "sonner";
import { z } from "zod";
import { Login } from "@/app/login";
import { Logo } from "@/app/logo";
import { authClient, redirectToOrganization } from "@/lib/auth";
import { features } from "@/lib/features";

export const Route = createFileRoute("/login")({
	component: RouteComponent,
	validateSearch: z.object({
		emailVerified: z.coerce.number().optional(),
		from: z.string().optional(),
	}),
	beforeLoad: async ({ search }) => {
		if (search.emailVerified) {
			toast.success("Email verified successfully. You can now sign in.", {
				position: "top-center",
			});
			throw redirect({ to: ".", search: { emailVerified: undefined } });
		}

		if (features.auth) {
			// Bypass the session cookie cache so a just-signed-out user is not
			// bounced back into the app by a stale cached session.
			const session = await authClient.getSession({
				query: { disableCookieCache: true },
			});
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
				<Login />
			</div>
		</div>
	);
}
