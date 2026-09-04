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
import { CodeFrame, CodeGroup, CodePreview } from "@/components";

export const MCP_DOCS_URL = "https://rivet.dev/mcp";

export const MCP_DESCRIPTION =
	"Let AI tools like Claude Code and Cursor read and manage your actors.";

type Language = "json" | "bash";

export interface ClientTab {
	title: string;
	icon: IconProp;
	language: Language;
	code: string;
}

function json(value: unknown) {
	return JSON.stringify(value, null, 2);
}

export function hostedMcpCommand(url: string) {
	return `claude mcp add --transport http rivet "${url}"`;
}

export function hostedTabs(url: string): ClientTab[] {
	return [
		{
			title: "Claude Code",
			icon: faClaude,
			language: "bash",
			code: hostedMcpCommand(url),
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

const LOCAL_COMMAND = "npx";
const LOCAL_ARGS = ["-y", "@rivet-dev/mcp", "--target", "local"];

export function localMcpCommand(endpoint: string, namespace: string) {
	return `claude mcp add rivet \\
  --env RIVET_ENDPOINT=${endpoint} \\
  --env RIVET_NAMESPACE=${namespace} \\
  -- ${LOCAL_COMMAND} ${LOCAL_ARGS.join(" ")}`;
}

export function localTabs(endpoint: string, namespace: string): ClientTab[] {
	const command = LOCAL_COMMAND;
	const args = LOCAL_ARGS;
	const env = { RIVET_ENDPOINT: endpoint, RIVET_NAMESPACE: namespace };
	const server = { command, args, env };

	return [
		{
			title: "Claude Code",
			icon: faClaude,
			language: "bash",
			code: localMcpCommand(endpoint, namespace),
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
		<a href={MCP_DOCS_URL} target="_blank" rel="noopener noreferrer">
			<span className="cursor-pointer hover:underline">
				See MCP Documentation{" "}
				<Icon icon={faChevronRight} className="text-xs" />
			</span>
		</a>
	);
}

export function ClientTabs({ tabs }: { tabs: ClientTab[] }) {
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
