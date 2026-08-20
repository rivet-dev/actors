import { describe, expect, test } from "vitest";
import { actor } from "@/actor/definition";
import { type RegistryConfig, RegistryConfigSchema } from "@/registry/config";
import { buildNativeFactory } from "@/registry/native";
import type { RuntimeServeConfig } from "@/registry/runtime";
import { type WasmBindings, WasmCoreRuntime } from "@/registry/wasm-runtime";
import { decodeCborCompat, encodeCborCompat } from "@/serde";

type HostKind = "supabase-deno" | "cloudflare-workers";
type SmokeCallbacks = Record<string, any>;

const serveConfig: RuntimeServeConfig = {
	version: 4,
	endpoint: "https://api.rivet.dev",
	token: "smoke-token",
	namespace: "smoke-namespace",
	poolName: "smoke-pool",
	serverlessPackageVersion: "0.0.0",
	serverlessValidateEndpoint: true,
	serverlessMaxStartPayloadBytes: 1024,
};

function encodeValue(value: unknown): Buffer {
	return Buffer.from(encodeCborCompat(value));
}

function decodeValue<T>(value: Uint8Array): T {
	return decodeCborCompat<T>(value);
}

class SmokeGate {
	#started!: () => void;
	#released!: () => void;

	readonly started = new Promise<void>((resolve) => {
		this.#started = resolve;
	});
	readonly released = new Promise<void>((resolve) => {
		this.#released = resolve;
	});

	markStarted(): void {
		this.#started();
	}

	release(): void {
		this.#released();
	}
}

class SmokeScenario {
	readonly actionReconnect = new SmokeGate();
	readonly remoteWriteReconnect = new SmokeGate();
	readonly save = new SmokeGate();
	readonly registerTask = new SmokeGate();
	registerTaskCompleted = false;
}

class SmokeHost {
	readonly sockets: Array<{
		url: string;
		protocols: string[];
		binaryType: string;
		reason: string;
	}> = [];
	readonly reconnects: string[] = [];
	readonly sql: Array<{
		method: string;
		sql: string;
		params: unknown;
		reconnects: string[];
	}> = [];
	readonly kv = new Map<string, Buffer>();
	readonly saves: unknown[] = [];

	constructor(readonly kind: HostKind) {}

	openEnvoySocket(config: RuntimeServeConfig, reason: string): void {
		const url = new URL("/envoys/connect", config.endpoint);
		url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
		url.searchParams.set("protocol_version", String(config.version));
		url.searchParams.set("namespace", config.namespace);
		url.searchParams.set("envoy_key", config.token ?? "");
		url.searchParams.set("version", config.serverlessPackageVersion);
		url.searchParams.set("pool_name", config.poolName);

		const protocols = ["rivet"];
		if (config.token) {
			protocols.push(`rivet_token.${config.token}`);
		}

		this.sockets.push({
			url: url.toString(),
			protocols,
			binaryType: "arraybuffer",
			reason,
		});
	}

	reconnect(config: RuntimeServeConfig, reason: string): void {
		this.reconnects.push(reason);
		this.openEnvoySocket(config, reason);
	}
}

class SmokeSql {
	constructor(private readonly host: SmokeHost) {}

	async exec(sql: string) {
		this.host.sql.push({
			method: "exec",
			sql,
			params: null,
			reconnects: [...this.host.reconnects],
		});
		return { columns: [], rows: [] };
	}

	async execute(sql: string, params?: unknown) {
		this.host.sql.push({
			method: "execute",
			sql,
			params,
			reconnects: [...this.host.reconnects],
		});
		return {
			columns: ["value"],
			rows: [["ok"]],
			changes: 1,
			lastInsertRowId: 1,
		};
	}

	async query(sql: string, params?: unknown) {
		this.host.sql.push({
			method: "query",
			sql,
			params,
			reconnects: [...this.host.reconnects],
		});
		return { columns: ["value"], rows: [["ok"]] };
	}

	async run(sql: string, params?: unknown) {
		await this.execute(sql, params);
		return { changes: 1 };
	}

	takeLastKvError(): null {
		return null;
	}

	async close(): Promise<void> {}
}

class SmokeKv {
	constructor(private readonly host: SmokeHost) {}

	async get(key: Buffer): Promise<Buffer | null> {
		return this.host.kv.get(key.toString("hex")) ?? null;
	}

	async put(key: Buffer, value: Buffer): Promise<void> {
		this.host.kv.set(key.toString("hex"), Buffer.from(value));
	}

	async delete(key: Buffer): Promise<void> {
		this.host.kv.delete(key.toString("hex"));
	}

	async deleteRange(): Promise<void> {}

	async listPrefix(): Promise<Array<{ key: Buffer; value: Buffer }>> {
		return [];
	}

	async listRange(): Promise<Array<{ key: Buffer; value: Buffer }>> {
		return [];
	}

	async batchGet(keys: Buffer[]): Promise<Array<Buffer | null>> {
		return keys.map((key) => this.host.kv.get(key.toString("hex")) ?? null);
	}

	async batchPut(
		entries: Array<{ key: Buffer; value: Buffer }>,
	): Promise<void> {
		for (const entry of entries) {
			this.host.kv.set(
				entry.key.toString("hex"),
				Buffer.from(entry.value),
			);
		}
	}

	async batchDelete(keys: Buffer[]): Promise<void> {
		for (const key of keys) {
			this.host.kv.delete(key.toString("hex"));
		}
	}
}

class SmokeActorContext {
	stateBytes = Buffer.alloc(0);
	readonly runtimeBag = {};
	readonly registeredTasks: Array<Promise<void>> = [];
	readonly kvHandle: SmokeKv;
	readonly sqlHandle: SmokeSql;
	readonly abortController = new AbortController();

	constructor(
		private readonly host: SmokeHost,
		private readonly scenario: SmokeScenario,
	) {
		this.kvHandle = new SmokeKv(host);
		this.sqlHandle = new SmokeSql(host);
	}

	state(): Buffer {
		return this.stateBytes;
	}

	beginOnStateChange(): void {}

	endOnStateChange(): void {}

	requestSave(opts?: unknown): void {
		this.host.saves.push(opts);
	}

	async requestSaveAndWait(opts?: unknown): Promise<void> {
		this.host.saves.push(opts);
		this.scenario.save.markStarted();
		await this.scenario.save.released;
	}

	registerTask(promise: Promise<unknown>): void {
		this.registeredTasks.push(Promise.resolve(promise).then(() => {}));
	}

	async drainRegisteredTasks(): Promise<void> {
		while (this.registeredTasks.length > 0) {
			const tasks = this.registeredTasks.splice(0);
			await Promise.all(tasks);
		}
	}

	takePendingHibernationChanges(): string[] {
		return [];
	}

	dirtyHibernatableConns(): unknown[] {
		return [];
	}

	runtimeState(): object {
		return this.runtimeBag;
	}

	actorId(): string {
		return `${this.host.kind}-actor`;
	}

	name(): string {
		return "smoke";
	}

	key(): Array<{ kind: string; stringValue: string }> {
		return [{ kind: "string", stringValue: this.host.kind }];
	}

	region(): string {
		return "local";
	}

	conns(): unknown[] {
		return [];
	}

	abortSignal(): AbortSignal {
		return this.abortController.signal;
	}

	kv(): SmokeKv {
		return this.kvHandle;
	}

	sql(): SmokeSql {
		return this.sqlHandle;
	}
}

class FakeCancellationToken {
	#cancelled = false;
	#callbacks: Array<() => void> = [];

	aborted(): boolean {
		return this.#cancelled;
	}

	cancel(): void {
		this.#cancelled = true;
		for (const callback of this.#callbacks) {
			callback();
		}
	}

	onCancelled(callback: () => void): void {
		this.#callbacks.push(callback);
	}
}

class FakeActorFactory {
	constructor(
		readonly callbacks: SmokeCallbacks,
		readonly config: Record<string, unknown>,
	) {}
}

function fakeWasmBindings(
	host: SmokeHost,
	scenario: SmokeScenario,
): WasmBindings {
	class FakeCoreRegistry {
		registered = new Map<string, FakeActorFactory>();
		activeCtx?: SmokeActorContext;

		register(name: string, factory: FakeActorFactory): void {
			this.registered.set(name, factory);
		}

		async serve(config: RuntimeServeConfig): Promise<void> {
			host.openEnvoySocket(config, "initial");
			const factory = this.registered.get("smoke");
			if (!factory) {
				throw new Error("smoke actor was not registered");
			}

			expect(factory.config).toMatchObject({
				hasDatabase: true,
				remoteSqlite: true,
			});

			const ctx = new SmokeActorContext(host, scenario);
			this.activeCtx = ctx;
			const initialState = await factory.callbacks.createState(null, {
				ctx,
				input: encodeValue({ host: host.kind }),
			});
			ctx.stateBytes = Buffer.from(initialState);

			let actionSettled = false;
			const actionPromise = factory.callbacks.actions.smoke(null, {
				ctx,
				conn: null,
				name: "smoke",
				args: encodeValue([host.kind]),
				cancelToken: new FakeCancellationToken(),
			});
			void actionPromise.then(
				() => {
					actionSettled = true;
				},
				() => {
					actionSettled = true;
				},
			);

			await scenario.actionReconnect.started;
			host.reconnect(config, "during-action");
			scenario.actionReconnect.release();

			await scenario.remoteWriteReconnect.started;
			host.reconnect(config, "during-remote-write-sql");
			scenario.remoteWriteReconnect.release();

			await scenario.save.started;
			await Promise.resolve();
			expect(actionSettled).toBe(false);
			scenario.save.release();

			const output = decodeValue<{
				stateCount: number;
				kvValue: string;
				sqlRows: number;
			}>(await actionPromise);
			expect(output).toEqual({
				stateCount: 1,
				kvValue: host.kind,
				sqlRows: 1,
			});

			const delta = await factory.callbacks.serializeState(null, {
				ctx,
				reason: "save",
			});
			expect(decodeValue<{ count: number }>(delta.state)).toEqual({
				count: 1,
			});
		}

		async shutdown(): Promise<void> {
			await this.activeCtx?.drainRegisteredTasks();
		}
	}

	return {
		CoreRegistry: FakeCoreRegistry,
		ActorFactory: FakeActorFactory,
		CancellationToken: FakeCancellationToken,
		ActorContext: class {},
		ConnHandle: class {},
		WebSocketHandle: class {},
		bridgeRivetErrorPrefix: () => "__RIVET_ERROR_JSON__:",
		roundTripBytes: (bytes: Uint8Array) => bytes,
		uint8ArrayFromBytes: (bytes: Uint8Array) => bytes,
		awaitPromise: async <T>(promise: Promise<T>) => await promise,
		default: async () => {},
	} as unknown as WasmBindings;
}

function smokeRegistryConfig(
	definition: ReturnType<typeof actor>,
): RegistryConfig {
	return RegistryConfigSchema.parse({
		use: { smoke: definition },
		endpoint: serveConfig.endpoint,
		token: serveConfig.token,
		namespace: serveConfig.namespace,
		noWelcome: true,
		startEngine: false,
		test: {
			enabled: true,
			sqliteBackend: "remote",
		},
	});
}

async function runHostSmoke(kind: HostKind): Promise<SmokeHost> {
	const host = new SmokeHost(kind);
	const scenario = new SmokeScenario();
	const runtime = new WasmCoreRuntime(fakeWasmBindings(host, scenario));
	const registry = runtime.createRegistry();
	const definition = actor({
		state: { count: 0 },
		db: {
			createClient: async () => ({
				execute: async () => [],
				close: async () => {},
			}),
			onMigrate: async () => {},
		},
		actions: {
			smoke: async (c, label: string) => {
				c.state.count += 1;
				await c.kv.put("host", label);
				const kvValue = await c.kv.get("host");

				scenario.actionReconnect.markStarted();
				await scenario.actionReconnect.released;

				await c.db.execute(
					"INSERT INTO smoke_events (host) VALUES (?)",
					[label],
				);

				scenario.remoteWriteReconnect.markStarted();
				await scenario.remoteWriteReconnect.released;

				await c.db.execute(
					"UPDATE smoke_events SET host = ? WHERE id = ?",
					[label, 1],
				);
				const rows = await c.db.execute(
					"SELECT host FROM smoke_events WHERE host = ?",
					[label],
				);
				await c.saveState({ immediate: true });
				void (
					c as unknown as {
						internalKeepAwake<T>(run: () => Promise<T>): Promise<T>;
					}
				).internalKeepAwake(async () => {
					scenario.registerTask.markStarted();
					await scenario.registerTask.released;
					scenario.registerTaskCompleted = true;
				});

				return {
					stateCount: c.state.count,
					kvValue,
					sqlRows: rows.rows.length,
				};
			},
		},
	});
	const config = smokeRegistryConfig(definition);

	runtime.registerActor(
		registry,
		"smoke",
		buildNativeFactory(runtime, config, definition),
	);
	await runtime.serveRegistry(registry, serveConfig);
	await scenario.registerTask.started;

	let shutdownSettled = false;
	const shutdownPromise = runtime.shutdownRegistry(registry).then(() => {
		shutdownSettled = true;
	});
	await Promise.resolve();
	expect(shutdownSettled).toBe(false);
	expect(scenario.registerTaskCompleted).toBe(false);

	scenario.registerTask.release();
	await shutdownPromise;
	expect(shutdownSettled).toBe(true);
	expect(scenario.registerTaskCompleted).toBe(true);

	return host;
}

describe("wasm edge host smoke coverage", () => {
	test.each([
		["supabase-deno" as const],
		["cloudflare-workers" as const],
	])("%s loads through the wasm runtime interface", async (kind) => {
		const host = await runHostSmoke(kind);
		const initial = host.sockets[0];
		const parsedUrl = new URL(initial.url);

		expect(initial.protocols).toEqual([
			"rivet",
			`rivet_token.${serveConfig.token}`,
		]);
		expect(initial.binaryType).toBe("arraybuffer");
		expect(parsedUrl.protocol).toBe("wss:");
		expect(parsedUrl.searchParams.get("protocol_version")).toBe("4");
		expect(parsedUrl.searchParams.get("namespace")).toBe(
			serveConfig.namespace,
		);
		expect(parsedUrl.searchParams.get("envoy_key")).toBe(serveConfig.token);
		expect(parsedUrl.searchParams.get("pool_name")).toBe(
			serveConfig.poolName,
		);

		expect(host.sockets.map((socket) => socket.reason)).toEqual([
			"initial",
			"during-action",
			"during-remote-write-sql",
		]);
		expect(host.sql.map((entry) => entry.method)).toEqual([
			"execute",
			"execute",
			"execute",
		]);
		expect(host.sql[1].reconnects).toContain("during-remote-write-sql");
		expect(host.saves).toContainEqual({ immediate: true });
	});
});
