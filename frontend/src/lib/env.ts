import z from "zod";
import { getApiEndpoint } from "../components/lib/config";

export const commonEnvSchema = z.object({
	// Engine API endpoint - transformed via getApiEndpoint() for local development support
	VITE_APP_API_URL: z.string().transform((url) => {
		return getApiEndpoint(url);
	}),
	VITE_APP_ASSETS_URL: z.string().url(),
	VITE_APP_POSTHOG_API_KEY: z.string().optional(),
	VITE_APP_POSTHOG_API_HOST: z.string().url().optional(),
	VITE_APP_SENTRY_DSN: z.string().url().optional(),
	VITE_APP_SENTRY_PROJECT_ID: z.coerce.number().optional(),
	VITE_APP_SENTRY_ENV: z.string().optional(),
	// AVAILABLE ONLY IN CI
	SENTRY_AUTH_TOKEN: z.string().optional(),
	SENTRY_PROJECT: z.string().optional(),
	APP_TYPE: z.enum(["engine", "cloud", "inspector"]).optional(),
	VITE_DEPLOYMENT_TYPE: z.enum(["staging", "production"]).optional(),
	BASE_URL: z.string().optional(),
});

export const commonEnv = () => commonEnvSchema.parse(import.meta.env);

export const engineEnv = () => commonEnvSchema.parse(import.meta.env);

export const cloudEnvSchema = commonEnvSchema.merge(
	z.object({
		// Cloud API endpoint - direct URL without transformation, used for cloud-specific operations
		VITE_APP_CLOUD_API_URL: z.string().url(),
		// Hosted MCP endpoint. Unset on Rivet Cloud; self-hosted deployments
		// point this at their own MCP service.
		VITE_APP_MCP_URL: z.string().url().optional(),
		VITE_APP_SENTRY_TUNNEL: z.string().optional(),
		VITE_APP_TURNSTILE_SITE_KEY: z.string().optional(),
	}),
);

export const cloudEnv = () => cloudEnvSchema.parse(import.meta.env);

export const getMcpUrl = () =>
	cloudEnv().VITE_APP_MCP_URL ?? "https://mcp.rivet.dev/mcp";

export const getRivetRunUrl = (engineNsName: string) => {
	return cloudEnv().VITE_DEPLOYMENT_TYPE === "production"
		? `https://${engineNsName}.rivet.run/`
		: `https://${engineNsName}.staging.rivet.run/`;
};
