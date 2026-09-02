import { redirect } from "@tanstack/react-router";
import { oauthProviderClient } from "@better-auth/oauth-provider/client";
import { adminClient, organizationClient } from "better-auth/client/plugins";
import { createAuthClient } from "better-auth/react";
import { cloudEnv } from "./env";
import { features } from "./features";

const createClient = () =>
	createAuthClient({
		baseURL: cloudEnv().VITE_APP_CLOUD_API_URL,
		fetchOptions: { credentials: "include" },
		plugins: [organizationClient(), adminClient(), oauthProviderClient()],
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

		const activeOrganizationId = session.data.session.activeOrganizationId;
		const orgs = await authClient.organization.list();
		const org =
			orgs.data?.find((o) => o.id === activeOrganizationId) ??
			orgs.data?.[0];

		if (!org) {
			return false;
		}

		if (org.id !== activeOrganizationId) {
			await authClient.organization.setActive({
				organizationId: org.id,
			});
		}

		throw redirect({
			to: "/orgs/$organization",
			params: { organization: org.slug },
		});
	}

	return false;
};
