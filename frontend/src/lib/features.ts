const envValue = import.meta.env.VITE_FEATURE_FLAGS as string | undefined;

const raw = import.meta.env.DEV
	? (localStorage.getItem("FEATURE_FLAGS") ?? envValue)
	: envValue;

// null means all flags are on (env var not set = full cloud build)
const enabled =
	raw === undefined
		? null
		: new Set(
				raw
					.split(",")
					.map((s) => s.trim())
					.filter(Boolean),
			);

function isEnabled(flag: string): boolean {
	return enabled === null || enabled.has(flag);
}

const auth = isEnabled("auth");
// `platform` gates whether the cloud platform stack is available (publishable
// token endpoint, billing, projects, multi-tenancy). The legacy
// `multitenancy` env string is accepted as an alias during the rollover.
const platform = (isEnabled("platform") || isEnabled("multitenancy")) && auth;
const acl = isEnabled("acl") || platform;

export const features = {
	auth,
	acl,
	billing: isEnabled("billing"),
	captcha: isEnabled("captcha") && auth,
	// `compute` gates the Rivet Compute (managed pool) UI: namespace
	// deployments, logs, and the Rivet provider option. Cloud-platform-only
	// because every surface consumes cloud-namespace data providers.
	compute: isEnabled("compute") && platform,
	// `agentOs` gates the agentOS (coding-agent VM) onboarding template. Beta.
	agentOs: isEnabled("agent-os"),
	// `mcp` gates the MCP connection settings. The snippet differs per flavor:
	// platform points at the hosted endpoint, OSS at the local stdio server.
	mcp: isEnabled("mcp"),
	support: isEnabled("support"),
	branding: isEnabled("branding"),
	datacenter: isEnabled("datacenter"),
	dangerZone: isEnabled("danger-zone"),
	platform,
} as const;
