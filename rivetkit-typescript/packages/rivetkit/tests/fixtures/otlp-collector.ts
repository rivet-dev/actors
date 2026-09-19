import { createServer } from "node:http";

export interface OtlpCollector {
	readonly endpoint: string;
	exports(): Buffer[];
	close(): Promise<void>;
}

export async function startOtlpCollector(port: number): Promise<OtlpCollector> {
	const exports: Buffer[] = [];
	const server = createServer((request, response) => {
		const chunks: Buffer[] = [];
		request.on("data", (chunk: Buffer) => chunks.push(chunk));
		request.on("end", () => {
			exports.push(Buffer.concat(chunks));
			response.writeHead(200, { "content-type": "application/json" });
			response.end();
		});
	});

	await new Promise<void>((resolve) =>
		server.listen(port, "127.0.0.1", resolve),
	);

	return {
		endpoint: `http://127.0.0.1:${port}/v1/traces`,
		exports: () => exports,
		close: () =>
			new Promise<void>((resolve, reject) => {
				server.close((error) => (error ? reject(error) : resolve()));
			}),
	};
}
