import { App } from "@modelcontextprotocol/ext-apps";
import { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import type { ActorId } from "@/components/actors/queries";
import "@/index.css";
import { InspectorApp } from "./main";

type ActorTarget =
	| { actorId: string }
	| { name: string; key?: string[]; method: "get"; skipReadyWait?: boolean }
	| {
			name: string;
			key?: string[];
			method: "getOrCreate";
			pool: string;
			input?: unknown;
			region?: string;
			crashPolicy?: "restart" | "sleep" | "destroy";
			skipReadyWait?: boolean;
	  };

type InspectorGrant = {
	token: string;
	proxyUrl: string;
	expiresAt: string;
	actorId: string;
	dashboardUrl?: string;
};

const app = new App(
	{ name: "Rivet Actor Inspector", version: "0.1.0" },
	{},
	{ strict: true },
);

let currentActor: ActorTarget | undefined;
let currentGrant: InspectorGrant | undefined;

function structuredGrant(
	result: Awaited<ReturnType<typeof app.callServerTool>>,
): InspectorGrant {
	if (result.isError || !result.structuredContent) {
		throw new Error("Could not create the temporary Inspector session");
	}
	const value = result.structuredContent as Record<string, unknown>;
	for (const key of ["token", "proxyUrl", "expiresAt", "actorId"] as const) {
		if (typeof value[key] !== "string")
			throw new Error("Invalid Inspector session response");
	}
	return value as InspectorGrant;
}

async function createSession(actor: ActorTarget): Promise<InspectorGrant> {
	return structuredGrant(
		await app.callServerTool({
			name: "rivet.ui.actor.session.create",
			arguments: { actor },
		}),
	);
}

async function renewSession(token: string): Promise<InspectorGrant> {
	return structuredGrant(
		await app.callServerTool({
			name: "rivet.ui.actor.session.renew",
			arguments: { token },
		}),
	);
}

async function revokeSession(token: string): Promise<void> {
	await app.callServerTool({
		name: "rivet.ui.actor.session.revoke",
		arguments: { token },
	});
}

// `create` mints a new session record rather than rotating the current one, so
// the grant it replaces stays valid until its own TTL and keeps counting
// against the per-principal session limit. `renew` rotates in place and needs
// no revocation. Hosts may fire tool results back to back, so swaps are
// serialized to keep a concurrent pair from both reading the same outgoing
// grant and leaking one of them.
let sessionSwap: Promise<unknown> = Promise.resolve();

function replaceSession(actor: ActorTarget): Promise<InspectorGrant> {
	const swap = sessionSwap.then(async () => {
		const superseded = currentGrant;
		const next = await createSession(actor);
		currentGrant = next;
		if (superseded) await revokeSession(superseded.token).catch(() => {});
		return next;
	});
	sessionSwap = swap.catch(() => {});
	return swap;
}

function McpInspector() {
	const [grant, setGrant] = useState<InspectorGrant>();
	const [error, setError] = useState<string>();

	useEffect(() => {
		const receiveInput = (params: {
			arguments?: Record<string, unknown>;
		}) => {
			const actor = params.arguments?.actor;
			if (actor && typeof actor === "object")
				currentActor = actor as ActorTarget;
		};
		const receiveResult = () => {
			if (!currentActor) return;
			void replaceSession(currentActor)
				.then(setGrant)
				.catch(() =>
					setError("Could not authenticate the embedded Inspector."),
				);
		};
		app.addEventListener("toolinput", receiveInput);
		app.addEventListener("toolresult", receiveResult);
		app.onhostcontextchanged = (context) => {
			document.documentElement.classList.toggle(
				"dark",
				context.theme !== "light",
			);
		};
		app.onteardown = async () => {
			if (currentGrant) await revokeSession(currentGrant.token);
			return {};
		};
		void app
			.connect()
			.catch(() =>
				setError("This host could not initialize the MCP App."),
			);
		return () => {
			app.removeEventListener("toolinput", receiveInput);
			app.removeEventListener("toolresult", receiveResult);
		};
	}, []);

	useEffect(() => {
		if (!grant) return;
		const renewAt = Math.max(
			1_000,
			new Date(grant.expiresAt).getTime() - Date.now() - 30_000,
		);
		const timer = window.setTimeout(() => {
			void renewSession(grant.token)
				.then((next) => {
					currentGrant = next;
					setGrant(next);
				})
				.catch(() =>
					setError(
						"The Inspector session expired. Reopen the Inspector to continue.",
					),
				);
		}, renewAt);
		return () => window.clearTimeout(timer);
	}, [grant]);

	if (error) return <p className="p-4 text-sm text-destructive">{error}</p>;
	if (!grant)
		return (
			<p className="p-4 text-sm text-muted-foreground">
				Connecting to the Rivet Actor Inspector…
			</p>
		);
	return (
		<div className="flex h-full min-h-0 flex-col">
			{grant.dashboardUrl ? (
				<div className="shrink-0 border-b px-3 py-2 text-xs text-muted-foreground">
					Console and custom tabs are available in the{" "}
					<a
						className="font-medium text-foreground underline underline-offset-2"
						href={grant.dashboardUrl}
						target="_blank"
						rel="noreferrer"
					>
						full Rivet Inspector
					</a>
					.
				</div>
			) : null}
			<div className="min-h-0 flex-1">
				<InspectorApp
					key={grant.token}
					actorId={grant.actorId as ActorId}
					credentials={{
						url: grant.proxyUrl,
						inspectorToken: grant.token,
						token: grant.token,
					}}
					activeTab={undefined}
					standalone
				/>
			</div>
		</div>
	);
}

const root = document.getElementById("root");
if (!root) throw new Error("Inspector UI: #root element missing");
ReactDOM.createRoot(root).render(<McpInspector />);
