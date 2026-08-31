import {
	faChevronRight,
	faClaude,
	faCursor,
	faGemini,
	faPlug,
	faVscode,
	Icon,
	type IconProp,
} from "@rivet-gg/icons";
import { useParams } from "@tanstack/react-router";
import { useState } from "react";
import {
	CodeFrame,
	CodeGroup,
	CodePreview,
	getConfig,
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components";
import { useEngineCompatDataProvider } from "@/components/actors";
import { getMcpUrl } from "@/lib/env";
import { features } from "@/lib/features";
import {
	type HostedTarget,
	hostedUrl,
	SCOPE_ORDER,
	SCOPES,
	type Scope,
} from "./mcp-scope";
import { SettingsCard } from "./settings-card";

const DOCS_URL = "https://rivet.dev/mcp";

const DESCRIPTION =
	"Let AI tools like Claude Code and Cursor read and manage your actors.";

type Language = "json" | "bash";

interface ClientTab {
	title: string;
	icon: IconProp;
	language: Language;
	code: string;
}

function json(value: unknown) {
	return JSON.stringify(value, null, 2);
}

function hostedTabs(url: string): ClientTab[] {
	return [
		{
			title: "Claude Code",
			icon: faClaude,
			language: "bash",
			code: `claude mcp add --transport http rivet "${url}"`,
		},
		{
			title: "Cursor",
			icon: faCursor,
			language: "json",
			code: json({ mcpServers: { rivet: { url } } }),
		},
		{
			title: "VS Code",
			icon: faVscode,
			language: "bash",
			code: `code --add-mcp '${JSON.stringify({ name: "rivet", type: "http", url })}'`,
		},
		{
			title: "Gemini CLI",
			icon: faGemini,
			language: "json",
			code: json({ mcpServers: { rivet: { httpUrl: url } } }),
		},
		{
			title: "Other",
			icon: faPlug,
			language: "json",
			code: json({ mcpServers: { rivet: { type: "http", url } } }),
		},
	];
}

function localTabs(endpoint: string, namespace: string): ClientTab[] {
	const command = "npx";
	const args = ["-y", "@rivet-dev/mcp", "--target", "local"];
	const env = { RIVET_ENDPOINT: endpoint, RIVET_NAMESPACE: namespace };
	const server = { command, args, env };

	return [
		{
			title: "Claude Code",
			icon: faClaude,
			language: "bash",
			code: `claude mcp add rivet \\
  --env RIVET_ENDPOINT=${endpoint} \\
  --env RIVET_NAMESPACE=${namespace} \\
  -- ${command} ${args.join(" ")}`,
		},
		{
			title: "Cursor",
			icon: faCursor,
			language: "json",
			code: json({ mcpServers: { rivet: server } }),
		},
		{
			title: "VS Code",
			icon: faVscode,
			language: "bash",
			code: `code --add-mcp '${JSON.stringify({ name: "rivet", ...server })}'`,
		},
		{
			title: "Gemini CLI",
			icon: faGemini,
			language: "json",
			code: json({ mcpServers: { rivet: server } }),
		},
		{
			title: "Other",
			icon: faPlug,
			language: "json",
			code: json({ mcpServers: { rivet: server } }),
		},
	];
}

function DocsFooter() {
	return (
		<a href={DOCS_URL} target="_blank" rel="noopener noreferrer">
			<span className="cursor-pointer hover:underline">
				See MCP Documentation{" "}
				<Icon icon={faChevronRight} className="text-xs" />
			</span>
		</a>
	);
}

function ClientTabs({ tabs }: { tabs: ClientTab[] }) {
	return (
		<CodeGroup>
			{tabs.map((tab) => (
				<CodeFrame
					key={tab.title}
					language={tab.language}
					title={tab.title}
					icon={tab.icon}
					code={() => tab.code}
					footer={<DocsFooter />}
				>
					<CodePreview code={tab.code} language={tab.language} />
				</CodeFrame>
			))}
		</CodeGroup>
	);
}

function ScopeSelect({
	value,
	onValueChange,
}: {
	value: Scope;
	onValueChange: (value: Scope) => void;
}) {
	return (
		<Select
			value={value}
			onValueChange={(next) => onValueChange(next as Scope)}
		>
			<SelectTrigger className="w-48">
				<SelectValue />
			</SelectTrigger>
			<SelectContent>
				{SCOPE_ORDER.map((scope) => (
					<SelectItem key={scope} value={scope}>
						{SCOPES[scope].label}
					</SelectItem>
				))}
			</SelectContent>
		</Select>
	);
}

function HostedMcp() {
	const params = useParams({ strict: false }) as Partial<HostedTarget>;
	const [scope, setScope] = useState<Scope>("namespace");

	if (!params.organization || !params.project || !params.namespace) {
		return null;
	}

	const target: HostedTarget = {
		organization: params.organization,
		project: params.project,
		namespace: params.namespace,
	};
	return (
		<SettingsCard
			title="MCP"
			description={`${DESCRIPTION} This connection can reach ${SCOPES[scope].reach}.`}
			action={<ScopeSelect value={scope} onValueChange={setScope} />}
		>
			<ClientTabs
				tabs={hostedTabs(hostedUrl(getMcpUrl(), target, scope))}
			/>
		</SettingsCard>
	);
}

function LocalMcp() {
	const namespace = useEngineCompatDataProvider().engineNamespace;
	const endpoint = getConfig().apiUrl;

	return (
		<SettingsCard
			title="MCP"
			description={`${DESCRIPTION} Runs on your machine.`}
		>
			<ClientTabs tabs={localTabs(endpoint, namespace)} />
		</SettingsCard>
	);
}

export function McpConnection() {
	if (!features.mcp) return null;
	return features.platform ? <HostedMcp /> : <LocalMcp />;
}
