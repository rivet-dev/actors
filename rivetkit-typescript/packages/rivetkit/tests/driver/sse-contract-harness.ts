import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
	chmod,
	mkdir,
	readFile,
	rename,
	rm,
	writeFile,
} from "node:fs/promises";
import { createServer, type IncomingMessage, type Server } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import getPort from "get-port";

const HARNESS_VERSION = "2.31.0";
const RELEASE_URL =
	"https://github.com/launchdarkly/sse-contract-tests/releases/download";
const HARNESS_TIMEOUT_MS = 330_000;
const HARNESS_KILL_GRACE_MS = 5_000;
const HARNESS_ACQUIRE_TIMEOUT_MS = 60_000;

interface HarnessAsset {
	archive: string;
	binary: string;
	sha256: string;
	binarySha256: string;
}

const HARNESS_ASSETS: Record<string, HarnessAsset> = {
	"darwin-arm64": {
		archive: "sse-contract-tests_Darwin_arm64.tar.gz",
		binary: "sse-contract-tests",
		sha256: "65084d8e2c28ddce72a7770e25c90d9fb774ac4bfe1e62caf9c2c4061418176d",
		binarySha256:
			"955d1baac0b7f56019818c280761933b3c74b17dfb1018808b26cb5de5c86efd",
	},
	"darwin-x64": {
		archive: "sse-contract-tests_Darwin_x86_64.tar.gz",
		binary: "sse-contract-tests",
		sha256: "ddcb50140943552da75cceba531e73fcc63dd9f3eeda2a9fdd69e544af1f6c08",
		binarySha256:
			"e88a77ff3193c969b8a358bafc496776ebd5ec00a9f4def7425a131e86e2c3a6",
	},
	"linux-arm64": {
		archive: "sse-contract-tests_Linux_arm64.tar.gz",
		binary: "sse-contract-tests",
		sha256: "abe663905d0944bc37f7f2b7f8df4871b50f6e907c76bc3aeded73401b42f175",
		binarySha256:
			"c957df214133990f63b92950ab29e9eb004b30f671817d32c58891841643bf52",
	},
	"linux-x64": {
		archive: "sse-contract-tests_Linux_x86_64.tar.gz",
		binary: "sse-contract-tests",
		sha256: "b8963f94dffc34a41a9cf192b3c59d5a16c94e4ba722dca945eed288ff88ec1d",
		binarySha256:
			"7035444fe54def899eb390ea05d35eefefc6913d5fc57f0cea64a47349131612",
	},
};

interface ActorFetch {
	fetch(input: string | URL | Request, init?: RequestInit): Promise<Response>;
}

interface StreamParams {
	callbackUrl: string;
	initialDelayMs?: number | null;
	lastEventId?: string | null;
	streamUrl: string;
}

interface StreamClient {
	abortController: AbortController;
	callbacks: Promise<void>;
	done: Promise<void>;
	nextCallback: number;
}

function sha256(data: Uint8Array): string {
	return createHash("sha256").update(data).digest("hex");
}

function debugAdapter(message: string): void {
	if (process.env.SSE_CONTRACT_DEBUG === "1") {
		process.stderr.write(`[sse-contract-adapter] ${message}\n`);
	}
}

async function ensureHarnessBinary(): Promise<string> {
	const asset = HARNESS_ASSETS[`${process.platform}-${process.arch}`];
	if (!asset) {
		throw new Error(
			`SSE contract tests do not support ${process.platform}-${process.arch}`,
		);
	}

	const cacheDir = join(
		tmpdir(),
		"rivetkit-sse-contract-tests",
		`v${HARNESS_VERSION}`,
		`${process.platform}-${process.arch}`,
	);
	const archivePath = join(cacheDir, asset.archive);
	const installDir = join(cacheDir, asset.sha256);
	const binaryPath = join(installDir, asset.binary);
	await mkdir(cacheDir, { recursive: true });

	let archive: Uint8Array | undefined;
	try {
		const cached = await readFile(archivePath);
		if (sha256(cached) === asset.sha256) {
			archive = cached;
		}
	} catch {
		// Download below.
	}

	if (!archive) {
		const response = await fetch(
			`${RELEASE_URL}/v${HARNESS_VERSION}/${asset.archive}`,
			{ signal: AbortSignal.timeout(HARNESS_ACQUIRE_TIMEOUT_MS) },
		);
		if (!response.ok) {
			throw new Error(
				`failed to download SSE contract harness: ${response.status} ${response.statusText}`,
			);
		}

		archive = new Uint8Array(await response.arrayBuffer());
		const actualSha256 = sha256(archive);
		if (actualSha256 !== asset.sha256) {
			throw new Error(
				`SSE contract harness checksum mismatch: expected ${asset.sha256}, received ${actualSha256}`,
			);
		}

		const temporaryPath = `${archivePath}.${process.pid}.tmp`;
		await writeFile(temporaryPath, archive);
		await rename(temporaryPath, archivePath);
	}

	try {
		const binary = await readFile(binaryPath);
		if (sha256(binary) === asset.binarySha256) {
			await chmod(binaryPath, 0o755);
			return binaryPath;
		}
	} catch {
		// Replace the installation below.
	}
	await rm(installDir, { recursive: true, force: true });

	const temporaryInstallDir = join(
		cacheDir,
		`.extract-${process.pid}-${randomUUID()}`,
	);
	await mkdir(temporaryInstallDir);
	try {
		await extractArchive(archivePath, temporaryInstallDir);
		const temporaryBinaryPath = join(temporaryInstallDir, asset.binary);
		const binary = await readFile(temporaryBinaryPath);
		const actualBinarySha256 = sha256(binary);
		if (actualBinarySha256 !== asset.binarySha256) {
			throw new Error(
				`SSE contract harness binary checksum mismatch: expected ${asset.binarySha256}, received ${actualBinarySha256}`,
			);
		}
		await chmod(temporaryBinaryPath, 0o755);
	} catch (error) {
		await rm(temporaryInstallDir, { recursive: true, force: true });
		throw error;
	}
	try {
		await rename(temporaryInstallDir, installDir);
	} catch (error) {
		await rm(temporaryInstallDir, { recursive: true, force: true });
		try {
			const binary = await readFile(binaryPath);
			if (sha256(binary) !== asset.binarySha256) {
				throw error;
			}
			await chmod(binaryPath, 0o755);
		} catch {
			throw error;
		}
	}
	await chmod(binaryPath, 0o755);
	return binaryPath;
}

async function extractArchive(
	archivePath: string,
	installDir: string,
): Promise<void> {
	await new Promise<void>((resolve, reject) => {
		const child = spawn("tar", ["-xzf", archivePath, "-C", installDir], {
			stdio: ["ignore", "pipe", "pipe"],
		});
		let stderr = "";
		let timedOut = false;
		let forceKillTimer: ReturnType<typeof setTimeout> | undefined;
		const timeoutTimer = setTimeout(() => {
			timedOut = true;
			child.kill("SIGTERM");
			forceKillTimer = setTimeout(() => {
				child.kill("SIGKILL");
			}, HARNESS_KILL_GRACE_MS);
		}, HARNESS_ACQUIRE_TIMEOUT_MS);
		const clearTimers = () => {
			clearTimeout(timeoutTimer);
			if (forceKillTimer) clearTimeout(forceKillTimer);
		};
		child.stderr.on("data", (chunk) => {
			stderr += chunk.toString();
		});
		child.once("error", (error) => {
			clearTimers();
			reject(error);
		});
		child.once("exit", (code) => {
			clearTimers();
			if (timedOut) {
				reject(
					new Error(
						`timed out extracting SSE contract harness after ${HARNESS_ACQUIRE_TIMEOUT_MS}ms`,
					),
				);
				return;
			}
			if (code === 0) {
				resolve();
			} else {
				reject(
					new Error(
						`failed to extract SSE contract harness (exit ${code}): ${stderr}`,
					),
				);
			}
		});
	});
}

async function readJson(request: IncomingMessage): Promise<unknown> {
	const chunks: Buffer[] = [];
	for await (const chunk of request) {
		chunks.push(Buffer.from(chunk));
	}
	return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

function listen(server: Server, port: number): Promise<void> {
	return new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(port, "127.0.0.1", () => {
			server.off("error", reject);
			resolve();
		});
	});
}

function close(server: Server): Promise<void> {
	return new Promise((resolve, reject) => {
		server.close((error) => {
			if (error) reject(error);
			else resolve();
		});
	});
}

function abortableDelay(ms: number, signal: AbortSignal): Promise<void> {
	return new Promise((resolve) => {
		const timeout = setTimeout(resolve, ms);
		signal.addEventListener(
			"abort",
			() => {
				clearTimeout(timeout);
				resolve();
			},
			{ once: true },
		);
	});
}

interface ParsedEvent {
	data: string;
	id: string;
	type: string;
}

export async function parseEventStream(
	body: ReadableStream<Uint8Array>,
	initialLastEventId: string,
	onEvent: (event: ParsedEvent) => void,
	onRetry: (delayMs: number) => void,
	signal?: AbortSignal,
): Promise<string> {
	const decoder = new TextDecoder("utf-8", { ignoreBOM: true });
	const reader = body.getReader();
	let data: string[] = [];
	let eventType = "";
	let lastEventId = initialLastEventId;
	let pendingLastEventId: string | undefined;
	let lineParts: string[] = [];
	let skipLeadingLf = false;
	let firstCharacter = true;
	const abortReader = () => {
		void reader.cancel(signal?.reason).catch(() => {
			// The body may already be closed by the HTTP client.
		});
	};
	if (signal?.aborted) {
		abortReader();
	} else {
		signal?.addEventListener("abort", abortReader, { once: true });
	}

	const finishLine = (line: string) => {
		if (line === "") {
			if (pendingLastEventId !== undefined) {
				lastEventId = pendingLastEventId;
			}
			if (data.length > 0) {
				onEvent({
					data: data.join("\n"),
					id: lastEventId,
					type: eventType || "message",
				});
			}
			data = [];
			eventType = "";
			pendingLastEventId = undefined;
			return;
		}

		if (line.startsWith(":")) {
			return;
		}

		const colon = line.indexOf(":");
		const field = colon === -1 ? line : line.slice(0, colon);
		let value = colon === -1 ? "" : line.slice(colon + 1);
		if (value.startsWith(" ")) {
			value = value.slice(1);
		}

		switch (field) {
			case "data":
				data.push(value);
				break;
			case "event":
				eventType = value;
				break;
			case "id":
				if (!value.includes("\0")) {
					pendingLastEventId = value;
				}
				break;
			case "retry":
				if (/^\d+$/.test(value)) {
					onRetry(Number(value));
				}
				break;
		}
	};

	const processText = (decoded: string) => {
		let offset = 0;
		if (firstCharacter && decoded.length > 0) {
			firstCharacter = false;
			if (decoded[0] === "\uFEFF") {
				offset = 1;
			}
		}
		if (skipLeadingLf && offset < decoded.length) {
			skipLeadingLf = false;
			if (decoded[offset] === "\n") {
				offset++;
			}
		}

		while (offset < decoded.length) {
			const cr = decoded.indexOf("\r", offset);
			const lf = decoded.indexOf("\n", offset);
			const lineEnd = cr === -1 ? lf : lf === -1 ? cr : Math.min(cr, lf);
			if (lineEnd === -1) {
				lineParts.push(decoded.slice(offset));
				return;
			}

			lineParts.push(decoded.slice(offset, lineEnd));
			finishLine(lineParts.join(""));
			lineParts = [];
			const terminator = decoded[lineEnd];
			offset = lineEnd + 1;
			if (terminator === "\r") {
				if (offset < decoded.length) {
					if (decoded[offset] === "\n") {
						offset++;
					}
				} else {
					skipLeadingLf = true;
				}
			}
		}
	};

	try {
		for (;;) {
			const next = await reader.read();
			if (next.done) {
				processText(decoder.decode());
				return lastEventId;
			}
			processText(decoder.decode(next.value, { stream: true }));
		}
	} finally {
		signal?.removeEventListener("abort", abortReader);
		reader.releaseLock();
	}
}

async function startTestService(actor: ActorFetch): Promise<{
	close: () => Promise<void>;
	url: string;
}> {
	const port = await getPort();
	const streams = new Map<string, StreamClient>();
	let nextStream = 1;

	const sendCallback = (
		client: StreamClient,
		callbackUrl: string,
		body: unknown,
	) => {
		const callbackNumber = client.nextCallback++;
		client.callbacks = client.callbacks.then(async () => {
			const callbackBody = JSON.stringify(body);
			debugAdapter(
				`posting callback ${callbackNumber} (${callbackBody.length} bytes)`,
			);
			const response = await fetch(`${callbackUrl}/${callbackNumber}`, {
				method: "POST",
				headers: { "content-type": "application/json" },
				body: callbackBody,
			});
			if (!response.ok) {
				throw new Error(
					`SSE contract callback failed: ${response.status} ${await response.text()}`,
				);
			}
			debugAdapter(`posted callback ${callbackNumber}`);
		});
	};

	const runStream = async (
		client: StreamClient,
		params: StreamParams,
	): Promise<void> => {
		let retryDelayMs = params.initialDelayMs ?? 1_000;
		let lastEventId = params.lastEventId ?? "";

		while (!client.abortController.signal.aborted) {
			try {
				const headers = new Headers({
					accept: "text/event-stream",
					"cache-control": "no-cache",
				});
				if (lastEventId) {
					headers.set("last-event-id", lastEventId);
				}

				const response = await actor.fetch(
					`api/sse-proxy?target=${encodeURIComponent(params.streamUrl)}`,
					{
						headers,
						signal: client.abortController.signal,
					},
				);
				if (response.status === 204) {
					return;
				}
				if (!response.ok || !response.body) {
					throw new Error(
						`SSE request failed with ${response.status}`,
					);
				}
				if (
					!response.headers
						.get("content-type")
						?.toLowerCase()
						.startsWith("text/event-stream")
				) {
					throw new Error(
						`invalid SSE content type: ${response.headers.get("content-type")}`,
					);
				}
				debugAdapter(`connected to ${params.streamUrl}`);

				lastEventId = await parseEventStream(
					response.body,
					lastEventId,
					(event) => {
						debugAdapter(
							`parsed event (${event.data.length} bytes)`,
						);
						lastEventId = event.id;
						sendCallback(client, params.callbackUrl, {
							kind: "event",
							event: {
								type: event.type,
								data: event.data,
								...(event.id ? { id: event.id } : {}),
							},
						});
					},
					(delayMs) => {
						retryDelayMs = delayMs;
					},
					client.abortController.signal,
				);
				if (!client.abortController.signal.aborted) {
					sendCallback(client, params.callbackUrl, {
						kind: "error",
						comment: "SSE stream disconnected",
					});
				}
			} catch (error) {
				debugAdapter(
					`stream failed: ${error instanceof Error ? error.stack : String(error)}`,
				);
				if (!client.abortController.signal.aborted) {
					sendCallback(client, params.callbackUrl, {
						kind: "error",
						comment:
							error instanceof Error
								? error.message
								: String(error),
					});
				}
			}

			await abortableDelay(retryDelayMs, client.abortController.signal);
		}
	};

	const server = createServer(async (request, response) => {
		try {
			const url = new URL(request.url ?? "/", `http://127.0.0.1:${port}`);

			if (request.method === "GET" && url.pathname === "/") {
				response.writeHead(200, { "content-type": "application/json" });
				response.end(
					JSON.stringify({
						capabilities: [
							"bom",
							"last-event-id",
							"server-directed-shutdown-request",
						],
					}),
				);
				return;
			}

			if (request.method === "POST" && url.pathname === "/") {
				const params = (await readJson(request)) as StreamParams;
				if (!params.streamUrl || !params.callbackUrl) {
					response.writeHead(400).end();
					return;
				}

				const id = String(nextStream++);
				const abortController = new AbortController();
				const client: StreamClient = {
					abortController,
					callbacks: Promise.resolve(),
					done: Promise.resolve(),
					nextCallback: 1,
				};
				client.done = runStream(client, params);
				streams.set(id, client);

				response.writeHead(201, {
					location: `http://127.0.0.1:${port}/streams/${id}`,
				});
				response.end();
				return;
			}

			const streamMatch = /^\/streams\/(\d+)$/.exec(url.pathname);
			const streamId = streamMatch?.[1];
			const client = streamId ? streams.get(streamId) : undefined;
			if (!streamId || !client) {
				response.writeHead(404).end();
				return;
			}

			if (request.method === "POST") {
				response.writeHead(400).end();
				return;
			}

			if (request.method === "DELETE") {
				debugAdapter(`deleting stream ${streamId}`);
				client.abortController.abort();
				await client.done;
				await client.callbacks;
				debugAdapter(`deleted stream ${streamId}`);
				streams.delete(streamId);
				response.writeHead(204).end();
				return;
			}

			response.writeHead(405).end();
		} catch (error) {
			response.writeHead(500, { "content-type": "text/plain" });
			response.end(error instanceof Error ? error.stack : String(error));
		}
	});

	await listen(server, port);
	return {
		url: `http://127.0.0.1:${port}`,
		close: async () => {
			for (const client of streams.values()) {
				client.abortController.abort();
			}
			await Promise.allSettled(
				[...streams.values()].flatMap((client) => [
					client.done,
					client.callbacks,
				]),
			);
			await close(server);
		},
	};
}

export async function runSseContractTests(actor: ActorFetch): Promise<string> {
	const [binary, callbackPort] = await Promise.all([
		ensureHarnessBinary(),
		getPort(),
	]);
	const service = await startTestService(actor);

	try {
		return await new Promise<string>((resolve, reject) => {
			const child = spawn(
				binary,
				[
					"--url",
					service.url,
					"--host",
					"127.0.0.1",
					"--port",
					String(callbackPort),
					...(process.env.SSE_CONTRACT_RUN
						? ["--run", process.env.SSE_CONTRACT_RUN]
						: []),
					...(process.env.SSE_CONTRACT_DEBUG === "1"
						? ["--debug-all"]
						: []),
				],
				{ stdio: ["ignore", "pipe", "pipe"] },
			);
			let stdout = "";
			let stderr = "";
			let timedOut = false;
			let settled = false;
			let forceKillTimer: ReturnType<typeof setTimeout> | undefined;
			const timeoutTimer = setTimeout(() => {
				timedOut = true;
				child.kill("SIGTERM");
				forceKillTimer = setTimeout(() => {
					child.kill("SIGKILL");
				}, HARNESS_KILL_GRACE_MS);
			}, HARNESS_TIMEOUT_MS);
			const settle = (result: { output: string } | { error: Error }) => {
				if (settled) return;
				settled = true;
				clearTimeout(timeoutTimer);
				if (forceKillTimer) clearTimeout(forceKillTimer);
				if ("output" in result) {
					resolve(result.output);
				} else {
					reject(result.error);
				}
			};
			child.stdout.on("data", (chunk) => {
				const text = chunk.toString();
				stdout += text;
				if (process.env.SSE_CONTRACT_DEBUG === "1") {
					process.stdout.write(text);
				}
			});
			child.stderr.on("data", (chunk) => {
				const text = chunk.toString();
				stderr += text;
				if (process.env.SSE_CONTRACT_DEBUG === "1") {
					process.stderr.write(text);
				}
			});
			child.once("error", (error) => {
				settle({ error });
			});
			child.once("exit", (code, signal) => {
				if (timedOut) {
					settle({
						error: new Error(
							`SSE contract tests timed out after ${HARNESS_TIMEOUT_MS}ms (signal ${signal ?? "none"})\n${stdout}\n${stderr}`,
						),
					});
					return;
				}
				if (code === 0) {
					settle({ output: stdout });
				} else {
					settle({
						error: new Error(
							`SSE contract tests failed (exit ${code})\n${stdout}\n${stderr}`,
						),
					});
				}
			});
		});
	} finally {
		await service.close();
	}
}
