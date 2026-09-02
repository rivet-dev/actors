import { describe, expect, it } from "vitest";
import {
	getAgentInstructionsPrompt,
	getComputeAddendum,
	getOnboardingTargetCopy,
} from "./agent-prompts";

const agentPromptOptions = {
	providerStr: "Rivet Compute",
	publishableToken: "pk_test",
	secretToken: "secret_test",
	runnerName: "default",
	serverless: true,
	providerDocUrl: "https://rivet.dev/docs/deploy/rivet-compute",
	namespace: "onboarding-test",
	cliDeploy: true,
} as const;

const computePromptOptions = {
	cloudToken: "cloud_api_test",
	publishableToken: "pk_test",
	namespace: "onboarding-test",
	apiUrl: "https://api.staging.rivet.dev",
	cloudApiUrl: "https://cloud-api.staging.rivet.dev",
	rivetRunUrl: "https://onboarding-test.staging.rivet.run/",
} as const;

describe("onboarding product prompts", () => {
	it.each([
		["actor", "https://rivet.dev/docs/actors/quickstart/backend"],
		["workflows", "https://rivet.dev/workflows/docs/quickstart/"],
		["dynamic-apps", "https://rivet.dev/dynamic-apps/docs/quickstart/"],
	] as const)("selects the %s quickstart", (target, expectedUrl) => {
		expect(getOnboardingTargetCopy(target).quickstartUrl).toBe(expectedUrl);
	});

	it("preserves the Rivet Actor prompt as the default", () => {
		const prompt = getAgentInstructionsPrompt(agentPromptOptions);

		expect(prompt).toContain("# RivetKit Setup & Deploy");
		expect(prompt).toContain("npm install rivetkit");
		expect(prompt).toContain("registry.listen");
	});

	it("preserves the existing Rivet Actor setup prompt for agentOS", () => {
		const prompt = getAgentInstructionsPrompt({
			...agentPromptOptions,
			target: "agent-os",
		});

		expect(prompt).toContain("# RivetKit Setup & Deploy");
		expect(prompt).toContain("npm install rivetkit");
		expect(prompt).toContain("registry.listen");
	});

	it("uses Workflows-specific setup guidance", () => {
		const prompt = getAgentInstructionsPrompt({
			...agentPromptOptions,
			target: "workflows",
		});

		expect(prompt).toContain("# Rivet Workflows Setup & Deploy");
		expect(prompt).toContain("@rivet-dev/workflows");
		expect(prompt).toContain(
			"https://rivet.dev/workflows/docs/quickstart/",
		);
		expect(prompt).toContain('ctx.step("stable-step-name"');
	});

	it("uses the Dynamic Apps host and deployment APIs", () => {
		const prompt = getAgentInstructionsPrompt({
			...agentPromptOptions,
			target: "dynamic-apps",
		});

		expect(prompt).toContain("@rivet-dev/dynamic-apps");
		expect(prompt).toContain("appsRouter.fetch");
		expect(prompt).toContain("deployApp({ appId, files })");
		expect(prompt).toContain(
			"https://rivet.dev/dynamic-apps/docs/quickstart/",
		);
		expect(prompt).not.toContain("Start the app with `registry.start()`");
		expect(prompt).not.toContain("Drive actors via the inspector HTTP API");
	});

	it("replaces Actor verification with an app URL for Dynamic Apps on Compute", () => {
		const prompt = getComputeAddendum({
			...computePromptOptions,
			target: "dynamic-apps",
		});

		expect(prompt).toContain("RIVET_CLOUD_TOKEN");
		expect(prompt).toContain("apps/onboarding/");
		expect(prompt).toContain("deployApp");
		expect(prompt).not.toContain("Keep registry.start()");
		expect(prompt).not.toContain("/actors?namespace=");
	});

	it("preserves Rivet Actor Compute deployment guidance by default", () => {
		const prompt = getComputeAddendum(computePromptOptions);

		expect(prompt).toContain("Keep registry.start()");
		expect(prompt).toContain("/actors?namespace=");
		expect(prompt).toContain("Verify actors work end-to-end");
	});

	it("uses Workflows-specific Compute deployment and verification", () => {
		const prompt = getComputeAddendum({
			...computePromptOptions,
			target: "workflows",
		});

		expect(prompt).toContain("# Rivet Workflows Compute Deployment Steps");
		expect(prompt).toContain("@rivet-dev/workflows");
		expect(prompt).toContain(
			"https://rivet.dev/workflows/docs/quickstart/",
		);
		expect(prompt).toContain("ctx.step");
		expect(prompt).toContain(
			'--token "cloud_api_test" --namespace onboarding-test --env PORT=3000',
		);
		expect(prompt).toContain("rivetkit/client");
		expect(prompt).toContain("getOrCreate");
		expect(prompt).toContain(
			"https://onboarding-test.staging.rivet.run/api/rivet",
		);
		expect(prompt).not.toContain("Keep registry.start()");
		expect(prompt).not.toContain("/actors?namespace=");
		expect(prompt).not.toContain("/gateway/<ACTOR_ID>/health");
		expect(prompt).not.toContain("Verify actors work end-to-end");
	});
	it.each([
		"actor",
		"workflows",
		"dynamic-apps",
	] as const)("includes the MCP connection section for %s when MCP is available", (target) => {
		const prompt = getComputeAddendum({
			...computePromptOptions,
			target,
			mcp: {
				command:
					'claude mcp add --transport http rivet "https://mcp.rivet.dev/mcp?organization=acme"',
				requiresUserApproval: true,
			},
		});

		expect(prompt).toContain("## Connect the Rivet MCP server");
		expect(prompt).toContain(
			'claude mcp add --transport http rivet "https://mcp.rivet.dev/mcp?organization=acme"',
		);
		expect(prompt).toContain("Ask the user to run this");
	});

	it("tells the agent to run the local MCP server itself", () => {
		const prompt = getComputeAddendum({
			...computePromptOptions,
			mcp: {
				command: "claude mcp add rivet -- npx -y @rivet-dev/mcp",
				requiresUserApproval: false,
			},
		});

		expect(prompt).toContain("Run this in the project root");
		expect(prompt).not.toContain("Ask the user to run this");
	});

	// Mirrors how `useComputeInstructionsCode` concatenates the two prompts.
	const composeComputePrompt = (
		target: "actor" | "workflows" | "dynamic-apps",
	) =>
		`${getAgentInstructionsPrompt({ ...agentPromptOptions, target })}\n\n---\n\n${getComputeAddendum({ ...computePromptOptions, target })}`;

	it.each([
		"actor",
		"workflows",
		"dynamic-apps",
	] as const)("defers to the Compute addendum for the %s deploy step", (target) => {
		const prompt = composeComputePrompt(target);

		expect(prompt).toContain("Compute Deployment Steps");
		expect(prompt).toContain(
			"Follow that section instead of deploying by hand",
		);
		expect(prompt).not.toContain("Deploy the Hono host as an HTTP service");
		expect(prompt).not.toContain("paste their deployment's public URL");
	});

	it.each([
		"actor",
		"workflows",
		"dynamic-apps",
	] as const)("omits the MCP connection section for %s when MCP is unavailable", (target) => {
		const prompt = getComputeAddendum({
			...computePromptOptions,
			target,
		});

		expect(prompt).not.toContain("Connect the Rivet MCP server");
		expect(prompt).not.toContain("claude mcp add");
	});
});
