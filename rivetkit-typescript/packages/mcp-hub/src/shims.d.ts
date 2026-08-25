declare module "@modelcontextprotocol/sdk/server/mcp.js" {
	export type ImplementationInfo = {
		name: string;
		version: string;
	};

	export class McpServer {
		constructor(
			info: ImplementationInfo,
			options?: Record<string, unknown>,
		);
		registerTool(...args: unknown[]): unknown;
		registerPrompt(...args: unknown[]): unknown;
		registerResource(...args: unknown[]): unknown;
		connect(transport: unknown): Promise<void>;
		close(): Promise<void>;
		server: Record<string, unknown>;
	}
}

declare module "@modelcontextprotocol/sdk/server/streamableHttp.js" {
	export class StreamableHTTPServerTransport {
		constructor(options?: Record<string, unknown>);
		close(): void;
		connect?(): Promise<void>;
		handleRequest(
			req: unknown,
			res: unknown,
			body?: unknown,
		): Promise<void>;
	}
}

declare module "@modelcontextprotocol/sdk/server/webStandardStreamableHttp.js" {
	export class WebStandardStreamableHTTPServerTransport {
		constructor(options?: Record<string, unknown>);
		handleRequest(
			request: Request,
			options?: Record<string, unknown>,
		): Promise<Response>;
	}
}
