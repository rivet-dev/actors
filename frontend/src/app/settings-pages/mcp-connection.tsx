import { useParams } from "@tanstack/react-router";
import { useState } from "react";
import {
	getConfig,
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components";
import { useEngineCompatDataProvider } from "@/components/actors";
import {
	ClientTabs,
	hostedTabs,
	localTabs,
	MCP_DESCRIPTION,
} from "@/components/mcp/client-tabs";
import {
	type HostedTarget,
	hostedUrl,
	SCOPE_ORDER,
	SCOPES,
	type Scope,
} from "@/components/mcp/scope";
import { getMcpUrl } from "@/lib/env";
import { features } from "@/lib/features";
import { SettingsCard } from "./settings-card";

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
			description={`${MCP_DESCRIPTION} This connection can reach ${SCOPES[scope].reach}.`}
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
			description={`${MCP_DESCRIPTION} Runs on your machine.`}
		>
			<ClientTabs tabs={localTabs(endpoint, namespace)} />
		</SettingsCard>
	);
}

export function McpConnection() {
	if (!features.mcp) return null;
	return features.platform ? <HostedMcp /> : <LocalMcp />;
}
