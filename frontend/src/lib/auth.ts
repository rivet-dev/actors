import { notFound, redirect } from "@tanstack/react-router";
import { adminClient, organizationClient } from "better-auth/client/plugins";
import { createAuthClient } from "better-auth/react";
import { cloudEnv } from "./env";
import { features } from "./features";

const createClient = () =>
	createAuthClient({
		baseURL: cloudEnv().VITE_APP_CLOUD_API_URL,
		fetchOptions: { credentials: "include" },
		plugins: [organizationClient(), adminClient()],
	});

type AuthClient = ReturnType<typeof createClient>;

export const authClient: AuthClient = features.auth
	? createClient()
	: (null as unknown as AuthClient);

const isSafeInternalPath = (path: string | undefined): path is string => {
	if (!path) return false;
	if (!path.startsWith("/")) return false;
	if (path.startsWith("//")) return false;
	if (path.startsWith("/login")) return false;
	if (path.startsWith("/join")) return false;
	if (path.startsWith("/verify-email-pending")) return false;
	if (path.startsWith("/forgot-password")) return false;
	return true;
};

export const redirectToOrganization = async ({
	from,
}: {
	from?: string;
} = {}) => {
	const session = await authClient.getSession();
	if (session.data) {
		if (isSafeInternalPath(from)) {
			throw redirect({ to: from });
		}

		if (session.data.session.activeOrganizationId) {
			const org = await authClient.organization.getFullOrganization({
				query: {
					organizationId: session.data.session.activeOrganizationId,
				},
			});

			if (org.error) {
				throw notFound();
			}

			throw redirect({
				to: "/orgs/$organization",
				params: { organization: org.data.slug },
			});
		}
		const orgs = await authClient.organization.list();

		if (!orgs.data?.[0]) {
			return false;
		}

		await authClient.organization.setActive({
			organizationId: orgs.data[0].id,
		});
		throw redirect({
			to: "/orgs/$organization",
			params: { organization: orgs.data[0].slug },
		});
	}

	return false;
};
