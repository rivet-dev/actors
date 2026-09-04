import { faCopy, Icon } from "@rivet-gg/icons";
import { deployOptions, type Provider } from "@rivetkit/shared-data";
import { useSuspenseQuery } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { toast } from "sonner";
import { Badge, Button, CodeFrame, CodePreview, getConfig } from "@/components";
import {
	useCloudNamespaceDataProvider,
	useEngineCompatDataProvider,
} from "@/components/actors";
import {
	hostedMcpCommand,
	localMcpCommand,
} from "@/components/mcp/client-tabs";
import { type HostedTarget, hostedUrl } from "@/components/mcp/scope";
import {
	getAgentInstructionsPrompt,
	getComputeAddendum,
	type McpSetup,
	type OnboardingTarget,
} from "@/content/agent-prompts";
import { cloudEnv, getMcpUrl, getRivetRunUrl } from "@/lib/env";
import { features } from "@/lib/features";
import { usePublishableToken } from "@/queries/accessors";
import { useRivetDsn } from "./env-variables";

// Providers that can only run serverless (function) deployments. Every other
// platform defaults to a long-lived runner (serverful) but can still toggle to
// serverless.
const SERVERLESS_ONLY_PROVIDERS = new Set<Provider>([
	"vercel",
	"cloudflare-workers",
	"supabase-functions",
	"gcp-cloud-run",
]);

export function isServerlessOnlyProvider(provider: unknown): boolean {
	return SERVERLESS_ONLY_PROVIDERS.has(provider as Provider);
}

export function defaultRuntimeModeForProvider(
	provider: unknown,
): "serverless" | "serverful" {
	return isServerlessOnlyProvider(provider) ? "serverless" : "serverful";
}

export function useAgentInstructionsCode({
	provider,
	runnerName = "default",
	endpoint,
	mode,
	target = "actor",
}: {
	provider?: Provider;
	runnerName?: string;
	endpoint?: string;
	mode?: "serverless" | "serverful";
	target?: OnboardingTarget;
} = {}) {
	const providerDetails = provider
		? deployOptions.find((p) => p.name === provider)
		: undefined;
	const providerStr =
		providerDetails?.displayName ?? provider ?? "your chosen provider";
	const providerDocUrl = providerDetails?.href
		? `https://rivet.dev${providerDetails.href}`
		: undefined;
	// Follow the user's runner/serverless selection. Rivet Compute always
	// deploys serverless (it sets the mode on deploy); every other provider uses
	// the chosen mode, falling back to the provider's default.
	const serverless =
		provider === "rivet" ||
		(mode ?? defaultRuntimeModeForProvider(provider)) === "serverless";
	const publishableToken = useRivetDsn({ kind: "publishable", endpoint });
	const secretToken = useRivetDsn({ kind: "secret", endpoint });
	const mcp = useMcpSetup();
	const namespace = useEngineCompatDataProvider().engineNamespace;

	return getAgentInstructionsPrompt({
		providerStr,
		publishableToken,
		secretToken,
		runnerName,
		serverless,
		providerDocUrl,
		namespace,
		// The `--namespace` deploy flag only applies to Rivet Compute's
		// `@rivetkit/cli deploy` flow.
		cliDeploy: provider === "rivet",
		target,
		mcp,
	});
}

// The MCP setup the copy-prompt should instruct the agent to perform. The hosted
// connection needs the user to approve an OAuth window, so the agent has to hand
// that step back; the local stdio server it can wire up itself.
function useMcpSetup(): McpSetup | undefined {
	const params = useParams({ strict: false }) as Partial<HostedTarget>;
	const dataProvider = useEngineCompatDataProvider();

	if (!features.mcp) return undefined;

	if (features.platform) {
		if (!params.organization || !params.project || !params.namespace) {
			return undefined;
		}
		const url = hostedUrl(
			getMcpUrl(),
			{
				organization: params.organization,
				project: params.project,
				namespace: params.namespace,
			},
			"namespace",
		);
		return {
			command: hostedMcpCommand(url),
			requiresUserApproval: true,
		};
	}

	return {
		command: localMcpCommand(
			getConfig().apiUrl,
			dataProvider.engineNamespace,
		),
		requiresUserApproval: false,
	};
}

// Builds the Rivet Compute copy-prompt (generic instructions + compute addendum)
// and exposes the cloud token and namespace so callers can also render a manual
// `@rivetkit/cli deploy` command.
export function useComputeInstructionsCode(target: OnboardingTarget = "actor") {
	const agentInstructions = useAgentInstructionsCode({
		provider: "rivet",
		target,
	});
	const dataProvider = useCloudNamespaceDataProvider();
	const { data: cloudToken } = useSuspenseQuery(
		dataProvider.createApiTokenQueryOptions({ name: "Onboarding" }),
	);
	const publishableRawToken = usePublishableToken();
	const namespace = dataProvider.engineNamespace;
	const mcp = useMcpSetup();

	const computeAddendum = getComputeAddendum({
		cloudToken,
		publishableToken: publishableRawToken ?? "<PUBLISHABLE_TOKEN>",
		namespace,
		apiUrl: cloudEnv().VITE_APP_API_URL,
		cloudApiUrl: cloudEnv().VITE_APP_CLOUD_API_URL,
		rivetRunUrl: getRivetRunUrl(namespace),
		target,
		mcp,
	});

	return {
		code: `${agentInstructions}\n\n---\n\n${computeAddendum}`,
		cloudToken,
		namespace,
	};
}

export function CommandBox({ command }: { command: string }) {
	return (
		<CodeFrame
			language="bash"
			code={() => command}
			hideFooter
			className="group my-0"
		>
			<CodePreview code={command} language="bash" className="text-left" />
		</CodeFrame>
	);
}

// Compact "Copy prompt" button that copies the agent prompt to the clipboard
// instead of dumping the whole prompt inline.
export function AgentPromptBanner({
	code,
	containsSecret = false,
	title = "Use your coding agent",
	description = "Have your coding agent complete these steps to deploy to Rivet Compute.",
}: {
	code: string;
	containsSecret?: boolean;
	title?: string;
	description?: string;
}) {
	return (
		<button
			type="button"
			onClick={() => {
				navigator.clipboard.writeText(code);
				toast.success(
					containsSecret
						? "Copied to clipboard — includes a secret deploy token, paste only into your agent"
						: "Copied to clipboard",
				);
			}}
			className="relative w-full flex flex-col items-stretch justify-between gap-4 rounded-lg px-4 py-4 border border-primary group cursor-pointer text-left sm:flex-row sm:items-center"
		>
			<Badge className="absolute -top-2.5 left-4 z-10 bg-background">
				Recommended
			</Badge>
			<div className="min-w-0">
				<p className="font-medium mb-1">{title}</p>
				<p className="text-sm text-muted-foreground">{description}</p>
				{containsSecret ? (
					<p className="mt-1 text-xs text-muted-foreground">
						Includes a secret deploy token. Paste only into your
						coding agent.
					</p>
				) : null}
			</div>
			<Button
				asChild
				variant="outline"
				className="w-full shrink-0 sm:w-auto"
			>
				<div>
					<Icon icon={faCopy} className="me-2 text-primary" />
					Copy prompt
				</div>
			</Button>
		</button>
	);
}
