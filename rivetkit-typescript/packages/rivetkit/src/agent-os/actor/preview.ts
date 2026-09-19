import crypto from "node:crypto";
import type { RequestContext } from "@/actor/config";
import type {
	DatabaseProvider,
	SynchronousRawAccess,
} from "@/common/database/config";
import type { AgentOsActorConfig } from "../config";
import type {
	AgentOsActionContext,
	AgentOsActorState,
	AgentOsActorVars,
} from "../types";
import { ensureVm } from "./index";

// Generate a 32-character lowercase alphanumeric token (a-z0-9).
// 36^32 ~= 1.6e49 possible tokens, brute-force infeasible.
export function generateToken(): string {
	const bytes = crypto.randomBytes(32);
	const alphabet = "abcdefghijklmnopqrstuvwxyz0123456789";
	let token = "";
	for (let i = 0; i < 32; i++) {
		token += alphabet[bytes[i]! % alphabet.length];
	}
	return token;
}

// CORS headers added to all preview proxy responses.
const CORS_HEADERS: Record<string, string> = {
	"Access-Control-Allow-Origin": "*",
	"Access-Control-Allow-Methods": "GET, POST, PUT, DELETE, OPTIONS",
	"Access-Control-Allow-Headers": "*",
};

function addCorsHeaders(response: Response): Response {
	const headers = new Headers(response.headers);
	for (const [key, value] of Object.entries(CORS_HEADERS)) {
		headers.set(key, value);
	}
	return new Response(response.body, {
		status: response.status,
		statusText: response.statusText,
		headers,
	});
}

type AgentOsRequestContext<TConnParams> = RequestContext<
	AgentOsActorState,
	TConnParams,
	undefined,
	AgentOsActorVars,
	undefined,
	DatabaseProvider<SynchronousRawAccess>
>;

export function buildOnRequestHandler<TConnParams>(
	config: AgentOsActorConfig<TConnParams>,
) {
	return async (
		c: AgentOsRequestContext<TConnParams>,
		request: Request,
	): Promise<Response> => {
		const url = new URL(request.url);
		const pathname = url.pathname;

		// Expect paths like /fetch/{token} or /fetch/{token}/remaining/path.
		const match = pathname.match(/^\/fetch\/([a-z0-9]+)(\/.*)?$/);
		if (!match) {
			return new Response("Not Found", { status: 404 });
		}

		// Handle OPTIONS preflight before token validation.
		if (request.method === "OPTIONS") {
			return new Response(null, { status: 204, headers: CORS_HEADERS });
		}

		const token = match[1]!;
		const remainingPath = match[2] ?? "/";

		// Validate token from SQLite.
		const now = Date.now();
		const rows: { port: number }[] = await c.db.execute(
			`SELECT port FROM agent_os_preview_tokens WHERE token = ? AND expires_at > ?`,
			token,
			now,
		);

		if (rows.length === 0) {
			c.log.warn({ msg: "agent-os preview auth failed", token });
			return addCorsHeaders(new Response("Forbidden", { status: 403 }));
		}

		const port = rows[0]?.port;

		// Boot the VM if needed.
		const agentOs = await ensureVm(
			c as AgentOsActionContext<TConnParams>,
			config,
		);

		// Build the request to proxy through the VM's virtual network.
		const vmUrl = `http://localhost:${port}${remainingPath}${url.search}`;
		const vmRequest = new Request(vmUrl, {
			method: request.method,
			headers: request.headers,
			body: request.body,
			duplex: "half",
		} as RequestInit);

		const vmResponse = await agentOs.fetch(port, vmRequest);

		c.log.info({
			msg: "agent-os preview request proxied",
			port,
			method: request.method,
			path: remainingPath,
			status: vmResponse.status,
		});

		return addCorsHeaders(vmResponse);
	};
}

export function buildPreviewActions<TConnParams>(
	config: AgentOsActorConfig<TConnParams>,
) {
	return {
		createSignedPreviewUrl: async (
			c: AgentOsActionContext<TConnParams>,
			port: number,
			expiresInSeconds?: number,
		): Promise<{
			path: string;
			token: string;
			port: number;
			expiresAt: number;
		}> => {
			await ensureVm(c, config);

			const effectiveExpires =
				expiresInSeconds ?? config.preview.defaultExpiresInSeconds;
			const maxExpires = config.preview.maxExpiresInSeconds;

			if (effectiveExpires < 1 || effectiveExpires > maxExpires) {
				throw new Error(
					`expiresInSeconds must be between 1 and ${maxExpires}`,
				);
			}

			const token = generateToken();
			const now = Date.now();
			const expiresAt = now + effectiveExpires * 1000;

			// Insert token and lazy-delete expired tokens.
			await c.db.execute(
				`INSERT INTO agent_os_preview_tokens (token, port, created_at, expires_at)
				 VALUES (?, ?, ?, ?)`,
				token,
				port,
				now,
				expiresAt,
			);
			await c.db.execute(
				`DELETE FROM agent_os_preview_tokens WHERE expires_at <= ?`,
				now,
			);

			// Path relative to the actor's gateway URL. Full URL is
			// `${gatewayUrl}/request/fetch/${token}` where gatewayUrl
			// comes from the client's getGatewayUrl().
			const path = `/request/fetch/${token}`;

			c.log.info({
				msg: "agent-os preview token created",
				port,
				expiresInSeconds: effectiveExpires,
			});

			return { path, token, port, expiresAt };
		},

		expireSignedPreviewUrl: async (
			c: AgentOsActionContext<TConnParams>,
			token: string,
		): Promise<void> => {
			await c.db.execute(
				`DELETE FROM agent_os_preview_tokens WHERE token = ?`,
				token,
			);

			c.log.info({ msg: "agent-os preview token expired", token });
		},
	};
}
