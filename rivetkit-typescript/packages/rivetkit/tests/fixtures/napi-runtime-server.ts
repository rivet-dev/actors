import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { getEnginePath } from "@rivetkit/engine-cli";
import { z } from "zod/v4";
import { db } from "../../src/db/mod";
import { actor, event, queue, setup, UserError } from "../../src/mod";
import { buildNativeRegistry } from "../../src/registry/native";

const fixtureDir = dirname(fileURLToPath(import.meta.url));
const repoEngineBinary = resolve(
	fixtureDir,
	"../../../../../target/debug/rivet-engine",
);

const endpoint = process.env.RIVETKIT_TEST_ENDPOINT ?? "http://127.0.0.1:6642";
const connParamsSchema = z.object({
	userId: z.string().min(1),
});
const validatedActionArgsSchema = z.tuple([
	z.object({
		amount: z.number().int().nonnegative(),
	}),
]);
const countChangedSchema = z.object({
	count: z.number().int(),
});
const jobSchema = z.object({
	id: z.string().min(1),
});

function resolveEngineBinaryPath(): string {
	if (existsSync(repoEngineBinary)) {
		return repoEngineBinary;
	}

	return getEnginePath();
}

const integrationActor = actor({
	state: { count: 0 },
	db: db(),
	connParamsSchema,
	actionInputSchemas: {
		validatedAction: validatedActionArgsSchema,
		emitValidatedEvent: z.tuple([countChangedSchema]),
		enqueueValidatedJob: z.tuple([jobSchema]),
	},
	events: {
		countChanged: event({ schema: countChangedSchema }),
	},
	queues: {
		jobs: queue({ message: jobSchema }),
	},
	onBeforeConnect: async () => {},
	actions: {
		ping: async (c) => {
			return c.conn.params.userId;
		},
		getCount: async (c) => {
			return c.state.count;
		},
		logContext: async (c, correlationToken: string) => {
			c.log.warn(
				{ correlation_token: correlationToken },
				"native actor log context",
			);
			return correlationToken;
		},
		validatedAction: async (_c, payload: { amount: number }) => {
			return payload.amount;
		},
		emitValidatedEvent: async (c, payload: { count: number }) => {
			c.broadcast("countChanged", payload);
			return payload.count;
		},
		enqueueValidatedJob: async (c, payload: { id: string }) => {
			await c.queue.send("jobs", payload);
			return payload.id;
		},
		increment: async (c, amount: number) => {
			c.state.count += amount;

			await c.kv.put("count", String(c.state.count));
			await c.db.execute(
				"CREATE TABLE IF NOT EXISTS increments (value INTEGER NOT NULL)",
			);
			await c.db.execute(
				"INSERT INTO increments (value) VALUES (?)",
				c.state.count,
			);

			const rows = await c.db.execute<{ value: number }>(
				"SELECT value FROM increments ORDER BY rowid ASC",
			);
			return {
				count: c.state.count,
				sqliteValues: rows.map(({ value }) => Number(value)),
			};
		},
		snapshot: async (c) => {
			const kvValue = await c.kv.get("count");
			await c.db.execute(
				"CREATE TABLE IF NOT EXISTS increments (value INTEGER NOT NULL)",
			);
			const rows = await c.db.execute<{ value: number }>(
				"SELECT value FROM increments ORDER BY rowid ASC",
			);

			return {
				count: c.state.count,
				kvCount: kvValue ? Number(kvValue) : null,
				sqliteValues: rows.map(({ value }) => Number(value)),
			};
		},
		incrementWithoutSql: async (c, amount: number) => {
			c.state.count += amount;
			await c.kv.put("count", String(c.state.count));
			return {
				count: c.state.count,
			};
		},
		scheduleTrace: async (c, correlationToken: string) => {
			await c.schedule.after(50, "scheduledTrace", correlationToken);
			return correlationToken;
		},
		scheduledTrace: async (c, correlationToken: string) => {
			await c.db.execute("SELECT ? AS trace", correlationToken);
		},
		sqliteFailure: async (c) => {
			await c.db.execute("SELECT value FROM missing_trace_test_table");
		},
		stateSnapshot: async (c) => {
			const kvValue = await c.kv.get("count");
			return {
				count: c.state.count,
				kvCount: kvValue ? Number(kvValue) : null,
			};
		},
		// Interleaves awaits, SQLite, a child actor call and a log so two
		// overlapping invocations of this action have every chance to observe
		// each other's telemetry context.
		isolationProbe: async (c, token: string, fail: boolean) => {
			await new Promise((resolve) => setTimeout(resolve, 20));
			await c.db.execute("SELECT ? AS probe", token);
			c.log.warn({ correlation_token: token }, "isolation probe");
			const client = c.client<any>();
			await client.integrationActor
				.getForId(c.actorId, {
					params: { userId: "internal-integration-test" },
				})
				.getCount();
			await new Promise((resolve) => setTimeout(resolve, 20));
			await c.db.execute("SELECT ? AS probe2", token);
			if (fail) {
				throw new UserError("isolation probe failure", {
					code: "isolation_probe_failed",
				});
			}
			return token;
		},
		getCountViaClient: async (c) => {
			const client = c.client<any>();
			return await client.integrationActor
				.getForId(c.actorId, {
					params: { userId: "internal-integration-test" },
				})
				.getCount();
		},
		throwTypedError: async () => {
			throw new UserError("native typed error", {
				code: "boom",
				metadata: {
					source: "native",
				},
			});
		},
		throwUntypedError: async () => {
			throw new Error("native untyped error");
		},
		goToSleep: async (c) => {
			c.sleep();
			return { ok: true };
		},
	},
});

const registry = setup({
	use: {
		integrationActor,
	},
	endpoint,
	namespace: process.env.RIVET_NAMESPACE ?? "default",
	token: process.env.RIVET_TOKEN ?? "dev",
	envoy: {
		poolName: process.env.RIVETKIT_TEST_POOL_NAME ?? "default",
	},
});

const {
	runtime: nativeRuntime,
	registry: nativeRegistry,
	serveConfig,
} = await buildNativeRegistry(registry.parseConfig());
if (!process.env.RIVETKIT_TEST_ENDPOINT) {
	serveConfig.engineBinaryPath = resolveEngineBinaryPath();
}

const shutdown = () => {
	void nativeRuntime.shutdownRegistry(nativeRegistry);
};
process.once("SIGINT", shutdown);
process.once("SIGTERM", shutdown);
try {
	await nativeRuntime.serveRegistry(nativeRegistry, serveConfig);
} finally {
	process.removeListener("SIGINT", shutdown);
	process.removeListener("SIGTERM", shutdown);
}
