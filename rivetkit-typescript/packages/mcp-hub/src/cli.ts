#!/usr/bin/env node
import http from "node:http";
import { StreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/streamableHttp.js";

import { createDocsMcpServer } from "./index";

const port = Number(process.env.PORT ?? 7332);
const mountPath = process.env.MCP_PATH ?? "/mcp";

async function main() {
	const { server } = await createDocsMcpServer();

	const httpServer = http.createServer(async (req, res) => {
		if (!req.url) {
			res.statusCode = 400;
			res.end("Missing request URL");
			return;
		}

		const url = new URL(
			req.url,
			`http://${req.headers.host ?? "localhost"}`,
		);
		if (url.pathname !== mountPath) {
			res.statusCode = 404;
			res.end("Not Found");
			return;
		}

		const transport = new StreamableHTTPServerTransport({});

		res.on("close", () => {
			if (typeof transport.close === "function") {
				transport.close();
			}
		});

		try {
			await server.connect(transport);
			const body = await readBody(req);
			await transport.handleRequest(req, res, body);
		} catch (error) {
			console.error("MCP request failed", error);
			if (!res.headersSent) {
				res.statusCode = 500;
				res.setHeader(
					"Content-Type",
					"application/json; charset=utf-8",
				);
				res.end(
					JSON.stringify({
						jsonrpc: "2.0",
						error: {
							code: -32000,
							message: "Internal server error",
						},
						id: null,
					}),
				);
			}
		}
	});

	httpServer.listen(port, () => {
		console.log(
			`Docs MCP server listening on http://localhost:${port}${mountPath}`,
		);
	});
}

async function readBody(req: http.IncomingMessage): Promise<unknown> {
	if (req.method !== "POST") {
		return undefined;
	}

	const chunks: Buffer[] = [];
	for await (const chunk of req) {
		chunks.push(Buffer.from(chunk));
	}

	if (chunks.length === 0) {
		return undefined;
	}

	const raw = Buffer.concat(chunks).toString("utf-8");
	try {
		return JSON.parse(raw);
	} catch {
		return undefined;
	}
}

main().catch((error) => {
	console.error("Failed to start docs MCP server", error);
	process.exit(1);
});
