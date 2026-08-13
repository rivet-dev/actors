// Curated agentOS registry catalog for the onboarding selection steps.
//
// The canonical registry lives at website/src/data/registry.ts, but it is
// website-only (the dashboard has no access to it). This is a curated subset of
// the entries relevant to onboarding. A future unification into
// frontend/packages/shared-data (where deploy.ts already lives) would remove the
// drift; out of scope for now.

export type CatalogStatus = "available" | "coming-soon";

export interface AgentEntry {
	slug: string;
	title: string;
	description: string;
	status: CatalogStatus;
	/** Present only when status is "available". */
	package?: string;
}

export interface SoftwareEntry {
	slug: string;
	title: string;
	package: string;
	description: string;
}

export interface SandboxProviderEntry {
	slug: string;
	title: string;
	description: string;
	/**
	 * Only `docker` is verified against an in-repo example
	 * (examples/agent-os/src/sandbox/server.ts). Other providers follow the
	 * `sandbox-agent/<slug>` subpath + same-named factory pattern and should be
	 * confirmed before relying on the generated import.
	 */
	verified?: boolean;
}

// Coding agents. Pi, Claude Code, Codex, and OpenCode all run in agentOS.
export const AGENTS: AgentEntry[] = [
	{
		slug: "pi",
		title: "Pi",
		description: "Lightweight, fast coding agent.",
		status: "available",
		package: "@rivet-dev/agent-os-pi",
	},
	{
		slug: "claude-code",
		title: "Claude Code",
		description:
			"Anthropic's agent with full tool, file, and shell access.",
		status: "available",
		// Package not yet documented (docs page 404s); follows the
		// agent-os-<slug> naming style. Verify before relying on the install.
		package: "@rivet-dev/agent-os-claude-code",
	},
	{
		slug: "codex",
		title: "Codex",
		description: "OpenAI's Codex coding agent.",
		status: "available",
		// Note the `-agent` suffix; the bare `@rivet-dev/agent-os-codex` is the
		// Codex CLI software package, not the agent.
		package: "@rivet-dev/agent-os-codex-agent",
	},
	{
		slug: "opencode",
		title: "OpenCode",
		description: "Open-source coding agent.",
		status: "available",
		package: "@rivet-dev/agent-os-opencode",
	},
];

// Software packages baked into the build. `common` is on by default.
export const SOFTWARE: SoftwareEntry[] = [
	{
		slug: "common",
		title: "Common",
		package: "@rivet-dev/agent-os-common",
		description:
			"coreutils, sed, grep, gawk, findutils, diffutils, tar, gzip.",
	},
	{
		slug: "build-essential",
		title: "Build Essential",
		package: "@rivet-dev/agent-os-build-essential",
		description: "common + make + git + curl.",
	},
	{
		slug: "git",
		title: "git",
		package: "@rivet-dev/agent-os-git",
		description: "Version control.",
	},
	{
		slug: "jq",
		title: "jq",
		package: "@rivet-dev/agent-os-jq",
		description: "Lightweight JSON processor.",
	},
	{
		slug: "ripgrep",
		title: "ripgrep",
		package: "@rivet-dev/agent-os-ripgrep",
		description: "Fast recursive search (rg).",
	},
	{
		slug: "fd",
		title: "fd",
		package: "@rivet-dev/agent-os-fd",
		description: "Fast file finder.",
	},
	{
		slug: "tree",
		title: "tree",
		package: "@rivet-dev/agent-os-tree",
		description: "Display directory structure as a tree.",
	},
	{
		slug: "coreutils",
		title: "Coreutils",
		package: "@rivet-dev/agent-os-coreutils",
		description: "Essential POSIX commands (when not using Common).",
	},
];

export const DEFAULT_AGENT = "pi";
export const DEFAULT_PACKAGES = ["common"];

// Sandbox mounting providers (sandbox-agent). Mounted at /sandbox.
export const SANDBOX_PROVIDERS: SandboxProviderEntry[] = [
	{
		slug: "docker",
		title: "Docker",
		description: "Run sandboxes in local Docker containers.",
		verified: true,
	},
	{
		slug: "e2b",
		title: "E2B",
		description: "Secure, ephemeral cloud sandboxes.",
	},
	{
		slug: "local",
		title: "Local",
		description: "Run sandboxes directly on the local machine.",
	},
	{
		slug: "daytona",
		title: "Daytona",
		description: "Managed development environments.",
	},
	{
		slug: "modal",
		title: "Modal",
		description: "Serverless cloud sandboxes.",
	},
	{
		slug: "vercel",
		title: "Vercel",
		description: "Vercel's edge and serverless platform.",
	},
	{
		slug: "computesdk",
		title: "ComputeSDK",
		description: "The ComputeSDK compute provider.",
	},
	{
		slug: "sprites",
		title: "Sprites",
		description: "Sprites' cloud sandbox infrastructure.",
	},
];

export const DEFAULT_SANDBOX_PROVIDER = "docker";
