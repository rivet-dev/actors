import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { ClientConfigSchema } from "@/client/config";
import { RemoteEngineControlClient } from "@/engine-client/mod";

describe.sequential("RemoteEngineControlClient poolName selection", () => {
	beforeEach(() => {
		vi.restoreAllMocks();
	});

	afterEach(() => {
		vi.unstubAllGlobals();
	});

	function makeDriver(): RemoteEngineControlClient {
		return new RemoteEngineControlClient(
			ClientConfigSchema.parse({
				endpoint: "https://api.rivet.dev",
				namespace: "default",
				poolName: "config-pool",
				disableMetadataLookup: true,
			}),
		);
	}

	function stubActorFetch(): Request[] {
		const requests: Request[] = [];
		vi.stubGlobal(
			"fetch",
			vi.fn(async (input: Request) => {
				requests.push(input);
				return new Response(
					JSON.stringify({
						actor: { actor_id: "act-1", name: "counter", key: "a" },
						created: true,
					}),
					{ headers: { "content-type": "application/json" } },
				);
			}),
		);
		return requests;
	}

	test("createActor uses per-call poolName as runner_name_selector", async () => {
		const requests = stubActorFetch();
		const driver = makeDriver();

		await driver.createActor({
			name: "counter",
			key: ["a"],
			poolName: "call-pool",
		});

		expect(requests).toHaveLength(1);
		const body = await requests[0]!.json();
		expect(body.runner_name_selector).toBe("call-pool");
	});

	test("createActor falls back to configured poolName", async () => {
		const requests = stubActorFetch();
		const driver = makeDriver();

		await driver.createActor({ name: "counter", key: ["a"] });

		expect(requests).toHaveLength(1);
		const body = await requests[0]!.json();
		expect(body.runner_name_selector).toBe("config-pool");
	});

	test("getOrCreateWithKey uses per-call poolName as runner_name_selector", async () => {
		const requests = stubActorFetch();
		const driver = makeDriver();

		await driver.getOrCreateWithKey({
			name: "counter",
			key: ["a"],
			poolName: "call-pool",
		});

		expect(requests).toHaveLength(1);
		const body = await requests[0]!.json();
		expect(body.runner_name_selector).toBe("call-pool");
	});

	test("getOrCreateWithKey falls back to configured poolName", async () => {
		const requests = stubActorFetch();
		const driver = makeDriver();

		await driver.getOrCreateWithKey({ name: "counter", key: ["a"] });

		expect(requests).toHaveLength(1);
		const body = await requests[0]!.json();
		expect(body.runner_name_selector).toBe("config-pool");
	});

	test("getOrCreate gateway URL uses per-call poolName for rvt-runner", async () => {
		const driver = makeDriver();

		const url = await driver.buildGatewayUrl({
			getOrCreateForKey: {
				name: "room",
				key: ["a"],
				poolName: "call-pool",
			},
		});

		expect(new URL(url).searchParams.get("rvt-runner")).toBe("call-pool");
	});

	test("getOrCreate gateway URL falls back to configured poolName for rvt-runner", async () => {
		const driver = makeDriver();

		const url = await driver.buildGatewayUrl({
			getOrCreateForKey: { name: "room", key: ["a"] },
		});

		expect(new URL(url).searchParams.get("rvt-runner")).toBe("config-pool");
	});
});
