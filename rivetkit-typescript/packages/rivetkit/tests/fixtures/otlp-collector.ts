import { createServer } from "node:http";

export interface OtlpCollector {
	readonly endpoint: string;
	spans(): Buffer[];
	close(): Promise<void>;
}

export interface OtlpCollectorOptions {
	/**
	 * Delay before each export is answered. Models a collector that accepts the
	 * connection and then stalls, which backs up the exporter's queue rather
	 * than failing its requests outright.
	 */
	readonly responseDelayMs?: number;
}

export async function startOtlpCollector(
	port: number,
	options: OtlpCollectorOptions = {},
): Promise<OtlpCollector> {
	const exports: Buffer[] = [];
	const pending = new Set<NodeJS.Timeout>();
	const server = createServer((request, response) => {
		const chunks: Buffer[] = [];
		request.on("data", (chunk: Buffer) => chunks.push(chunk));
		request.on("end", () => {
			exports.push(Buffer.concat(chunks));
			const reply = () => {
				response.writeHead(200, { "content-type": "application/json" });
				response.end();
			};
			if (!options.responseDelayMs) {
				reply();
				return;
			}
			const timer = setTimeout(() => {
				pending.delete(timer);
				reply();
			}, options.responseDelayMs);
			pending.add(timer);
		});
	});

	await new Promise<void>((resolve) =>
		server.listen(port, "127.0.0.1", resolve),
	);

	return {
		endpoint: `http://127.0.0.1:${port}/v1/traces`,
		spans: () => exports,
		close: () =>
			new Promise<void>((resolve, reject) => {
				for (const timer of pending) clearTimeout(timer);
				pending.clear();
				server.closeAllConnections();
				server.close((error) => (error ? reject(error) : resolve()));
			}),
	};
}
