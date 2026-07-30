import { createServer } from "node:http";
import { once } from "node:events";
import getPort from "get-port";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { actor, setup } from "@/mod";
import { Registry, type RegistryDeps } from "@/registry";
import { loadNapiRuntime } from "@/registry/napi-runtime";
import type { RuntimeServeConfig } from "@/registry/runtime";
import { getRivetkitRuntimeMode, parsePortEnv } from "@/utils/env-vars";

describe("getRivetkitRuntimeMode", () => {
	let snapshot: string | undefined;

	beforeEach(() => {
		snapshot = process.env.RIVETKIT_RUNTIME_MODE;
		delete process.env.RIVETKIT_RUNTIME_MODE;
	});
	afterEach(() => {
		if (snapshot === undefined) delete process.env.RIVETKIT_RUNTIME_MODE;
		else process.env.RIVETKIT_RUNTIME_MODE = snapshot;
	});

	test("default (unset) is envoy", () => {
		expect(getRivetkitRuntimeMode()).toBe("envoy");
	});

	test("explicit serverless", () => {
		process.env.RIVETKIT_RUNTIME_MODE = "serverless";
		expect(getRivetkitRuntimeMode()).toBe("serverless");
	});

	test("explicit envoy", () => {
		process.env.RIVETKIT_RUNTIME_MODE = "envoy";
		expect(getRivetkitRuntimeMode()).toBe("envoy");
	});

	test("empty string is rejected", () => {
		process.env.RIVETKIT_RUNTIME_MODE = "";
		expect(() => getRivetkitRuntimeMode()).toThrow(
			/RIVETKIT_RUNTIME_MODE env var must be/,
		);
	});

	test("unrecognized value is rejected", () => {
		process.env.RIVETKIT_RUNTIME_MODE = "potato";
		expect(() => getRivetkitRuntimeMode()).toThrow(
			/RIVETKIT_RUNTIME_MODE env var must be/,
		);
	});
});

describe("parsePortEnv", () => {
	test("undefined input returns undefined", () => {
		expect(parsePortEnv(undefined)).toBeUndefined();
	});

	test("empty string returns undefined", () => {
		expect(parsePortEnv("")).toBeUndefined();
	});

	test("valid integer string parses", () => {
		expect(parsePortEnv("8080")).toBe(8080);
	});

	test("port 1 is accepted (lower bound)", () => {
		expect(parsePortEnv("1")).toBe(1);
	});

	test("port 65535 is accepted (upper bound)", () => {
		expect(parsePortEnv("65535")).toBe(65535);
	});

	test("port 0 is rejected", () => {
		expect(() => parsePortEnv("0")).toThrow(/RIVET_PORT env var must be/);
	});

	test("port 65536 is rejected", () => {
		expect(() => parsePortEnv("65536")).toThrow(
			/RIVET_PORT env var must be/,
		);
	});

	test("non-numeric input is rejected", () => {
		expect(() => parsePortEnv("notaport")).toThrow(
			/RIVET_PORT env var must be/,
		);
	});

	test("partial numeric input is rejected (parseInt would silently succeed)", () => {
		expect(() => parsePortEnv("8080abc")).toThrow(
			/RIVET_PORT env var must be/,
		);
	});

	test("negative input is rejected", () => {
		expect(() => parsePortEnv("-1")).toThrow(/RIVET_PORT env var must be/);
	});
});

const testActor = actor({
	state: {},
	actions: {},
});

const applicationServeConfig: RuntimeServeConfig = {
	version: 4,
	endpoint: "https://api.rivet.dev",
	token: "listener-test",
	namespace: "listener-test",
	poolName: "listener-test",
	serverlessPackageVersion: "0.0.0",
	serverlessValidateEndpoint: false,
	serverlessMaxStartPayloadBytes: 1024 * 1024,
};

async function createNativeApplicationRegistry(): Promise<Registry<any>> {
	const { runtime } = await loadNapiRuntime();
	const handle = runtime.createRegistry();
	const buildConfiguredRegistry = async () => ({
		runtime,
		registry: handle,
		serveConfig: applicationServeConfig,
	});
	return new Registry(
		{
			use: { test: testActor },
			startEngine: false,
			noWelcome: true,
			shutdown: { disableSignalHandlers: true },
		},
		{
			buildConfiguredRegistry:
				buildConfiguredRegistry as RegistryDeps["buildConfiguredRegistry"],
		},
	);
}

describe("registry.listen() end-to-end", () => {
	let registry: Registry<any> | undefined;
	let listenPromise: Promise<void> | undefined;
	let runtimeMode: string | undefined;

	beforeEach(() => {
		runtimeMode = process.env.RIVETKIT_RUNTIME_MODE;
	});

	afterEach(async () => {
		if (registry) {
			await registry.shutdown();
			registry = undefined;
		}
		if (listenPromise) {
			await listenPromise.catch(() => undefined);
			listenPromise = undefined;
		}
		if (runtimeMode === undefined) delete process.env.RIVETKIT_RUNTIME_MODE;
		else process.env.RIVETKIT_RUNTIME_MODE = runtimeMode;
	}, 30_000);

	test("binds the requested port and serves /api/rivet/metadata", async () => {
		const port = await getPort({ host: "127.0.0.1" });
		registry = setup({
			use: { test: testActor },
			startEngine: false,
			endpoint: "http://127.0.0.1:65535",
			token: "dev",
			namespace: "default",
			noWelcome: true,
			shutdown: { disableSignalHandlers: true },
		}) as Registry<any>;

		listenPromise = registry.listen({ port, host: "127.0.0.1" });

		const baseUrl = `http://127.0.0.1:${port}`;
		const response = await waitForResponse(
			`${baseUrl}/api/rivet/metadata`,
			15_000,
		);
		expect(response.status).toBe(200);
		const body = await response.json();
		expect(body.runtime).toBe("rivetkit");
		expect(body.actorNames).toBeDefined();
		expect(body.actorNames).toHaveProperty("test");
	}, 30_000);

	test("/api/rivet/health returns ok", async () => {
		const port = await getPort({ host: "127.0.0.1" });
		registry = setup({
			use: { test: testActor },
			startEngine: false,
			endpoint: "http://127.0.0.1:65535",
			token: "dev",
			namespace: "default",
			noWelcome: true,
			shutdown: { disableSignalHandlers: true },
		}) as Registry<any>;

		listenPromise = registry.listen({ port, host: "127.0.0.1" });

		const response = await waitForResponse(
			`http://127.0.0.1:${port}/api/rivet/health`,
			15_000,
		);
		expect(response.status).toBe(200);
		const body = await response.json();
		expect(body.runtime).toBe("rivetkit");
		expect(body.status).toBeDefined();
	}, 30_000);

	test("native application listener forwards requests", async () => {
		const port = await getPort({ host: "127.0.0.1" });
		const paths: string[] = [];
		const { runtime } = await loadNapiRuntime();
		const handle = runtime.createRegistry();
		const applicationListener = runtime.serveApplicationListener(
			handle,
			{
				port,
				host: "127.0.0.1",
				application: async (request) => {
					const path = new URL(request.url).pathname;
					paths.push(path);
					return {
						status: 201,
						headers: { "x-application": "yes" },
						body: new TextEncoder().encode(
							path === "/echo"
								? new TextDecoder().decode(request.body)
								: `application:${path}`,
						),
					};
				},
			},
			applicationServeConfig,
		);

		try {
			const baseUrl = `http://127.0.0.1:${port}`;
			const health = await waitForResponse(`${baseUrl}/health`, 15_000);
			expect(health.status).toBe(201);
			expect(await health.text()).toBe("application:/health");

			const echo = await fetch(`${baseUrl}/echo`, {
				method: "POST",
				body: "hello",
			});
			expect(echo.status).toBe(201);
			expect(echo.headers.get("x-application")).toBe("yes");
			expect(await echo.text()).toBe("hello");
			expect(paths).toEqual(["/health", "/echo"]);
		} finally {
			await runtime.shutdownRegistry(handle);
			await applicationListener;
		}
	}, 30_000);

	test("native application listener streams response chunks before completion", async () => {
		const port = await getPort({ host: "127.0.0.1" });
		process.env.RIVETKIT_RUNTIME_MODE = "serverless";
		registry = await createNativeApplicationRegistry();
		let releaseSecondChunk: (() => void) | undefined;
		const secondChunkGate = new Promise<void>((resolve) => {
			releaseSecondChunk = resolve;
		});
		listenPromise = registry.listen({
			port,
			host: "127.0.0.1",
			application: {
				fetch: () =>
					new Response(
						new ReadableStream({
							async start(controller) {
								controller.enqueue(
									new TextEncoder().encode("first\n"),
								);
								await secondChunkGate;
								controller.enqueue(
									new TextEncoder().encode("second\n"),
								);
								controller.close();
							},
						}),
						{
							headers: { "content-type": "text/event-stream" },
						},
					),
			},
		});

		try {
			const response = await waitForResponse(
				`http://127.0.0.1:${port}/events`,
				15_000,
			);
			expect(response.headers.get("content-type")).toBe(
				"text/event-stream",
			);
			const reader = response.body?.getReader();
			expect(reader).toBeDefined();
			const first = await reader?.read();
			expect(new TextDecoder().decode(first?.value)).toBe("first\n");
			expect(first?.done).toBe(false);

			releaseSecondChunk?.();
			const second = await reader?.read();
			expect(new TextDecoder().decode(second?.value)).toBe("second\n");
			expect(second?.done).toBe(false);
			expect((await reader?.read())?.done).toBe(true);
		} finally {
			releaseSecondChunk?.();
		}
	}, 30_000);

	test("native application listener relays an upstream fetch stream", async () => {
		const port = await getPort({ host: "127.0.0.1" });
		const upstreamPort = await getPort({ host: "127.0.0.1" });
		const upstream = createServer((_request, response) => {
			response.writeHead(200, {
				"content-type": "text/event-stream",
				"cache-control": "no-cache",
			});
			response.write("event: control\ndata: {\"upToDate\":true}\n\n");
		});
		upstream.listen(upstreamPort, "127.0.0.1");
		await once(upstream, "listening");
		process.env.RIVETKIT_RUNTIME_MODE = "serverless";
		registry = await createNativeApplicationRegistry();
		listenPromise = registry.listen({
			port,
			host: "127.0.0.1",
			application: {
				fetch: () => fetch(`http://127.0.0.1:${upstreamPort}/events`),
			},
		});

		try {
			const response = await waitForResponse(
				`http://127.0.0.1:${port}/events`,
				15_000,
			);
			const reader = response.body?.getReader();
			const first = await reader?.read();
			expect(new TextDecoder().decode(first?.value)).toBe(
				'event: control\ndata: {"upToDate":true}\n\n',
			);
			expect(first?.done).toBe(false);
		} finally {
			upstream.closeAllConnections();
			upstream.close();
		}
	}, 30_000);

	test("native application listener cancels active streams during shutdown", async () => {
		const port = await getPort({ host: "127.0.0.1" });
		process.env.RIVETKIT_RUNTIME_MODE = "serverless";
		registry = await createNativeApplicationRegistry();
		registry.config.shutdown = {
			disableSignalHandlers: true,
			gracePeriodMs: 5_000,
		};
		let resolveRequestAborted: (() => void) | undefined;
		const requestAborted = new Promise<void>((resolve) => {
			resolveRequestAborted = resolve;
		});
		let resolveBodyCancelled: (() => void) | undefined;
		const bodyCancelled = new Promise<void>((resolve) => {
			resolveBodyCancelled = resolve;
		});
		listenPromise = registry.listen({
			port,
			host: "127.0.0.1",
			application: {
				fetch: (request) => {
					request.signal.addEventListener(
						"abort",
						() => resolveRequestAborted?.(),
						{ once: true },
					);
					return new Response(
						new ReadableStream({
							start(controller) {
								controller.enqueue(
									new TextEncoder().encode("first\n"),
								);
							},
							cancel() {
								resolveBodyCancelled?.();
							},
						}),
						{
							headers: { "content-type": "text/event-stream" },
						},
					);
				},
			},
		});

		const response = await waitForResponse(
			`http://127.0.0.1:${port}/events`,
			15_000,
		);
		const reader = response.body?.getReader();
		expect(new TextDecoder().decode((await reader?.read())?.value)).toBe(
			"first\n",
		);

		await expect(
			Promise.race([
				registry.shutdown(),
				new Promise((_, reject) =>
					setTimeout(
						() => reject(new Error("registry shutdown timed out")),
						2_000,
					),
				),
			]),
		).resolves.toBeUndefined();
		await expect(requestAborted).resolves.toBeUndefined();
		await expect(bodyCancelled).resolves.toBeUndefined();
		await listenPromise;
	}, 30_000);
});

/**
 * Poll the URL until it responds (the listener takes a moment to bind and
 * build the serverless runtime on first request).
 */
async function waitForResponse(
	url: string,
	timeoutMs: number,
): Promise<Response> {
	const deadline = Date.now() + timeoutMs;
	let lastError: unknown;
	while (Date.now() < deadline) {
		try {
			const response = await fetch(url);
			return response;
		} catch (error) {
			lastError = error;
			await new Promise((resolve) => setTimeout(resolve, 100));
		}
	}
	throw new Error(`timed out waiting for ${url}: ${String(lastError)}`);
}
