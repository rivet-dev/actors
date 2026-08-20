import { serve } from "@hono/node-server";
import { actor, setup } from "rivetkit";

const host = process.env.RIVETKIT_TEST_HOST ?? "127.0.0.1";
const port = Number(process.env.RIVETKIT_TEST_PORT);
const endpoint = process.env.RIVETKIT_TEST_ENDPOINT;
const namespace = process.env.RIVET_NAMESPACE;
const token = process.env.RIVET_TOKEN ?? "dev";
const poolName = process.env.RIVETKIT_TEST_POOL_NAME;
const heartbeatMode = process.env.RIVETKIT_HEARTBEAT_MODE ?? "sqlite";

if (!Number.isInteger(port) || port <= 0) {
	throw new Error("RIVETKIT_TEST_PORT must be a positive integer");
}
if (!endpoint) {
	throw new Error("RIVETKIT_TEST_ENDPOINT is required");
}
if (!namespace) {
	throw new Error("RIVET_NAMESPACE is required");
}
if (!poolName) {
	throw new Error("RIVETKIT_TEST_POOL_NAME is required");
}
if (!["none", "sqlite", "kv"].includes(heartbeatMode)) {
	throw new Error("RIVETKIT_HEARTBEAT_MODE must be one of: none, sqlite, kv");
}

interface SqliteDatabase {
	run(sql: string, params?: unknown[]): Promise<void>;
	query(
		sql: string,
		params?: unknown[],
	): Promise<{
		rows: unknown[][];
	}>;
}

interface HeartbeatVars {
	heartbeatTimer?: ReturnType<typeof setInterval>;
	heartbeatInFlight?: boolean;
	heartbeatSeq?: number;
}

const rawSqlDatabaseProvider = {
	createClient: async (ctx: any) => {
		if (!ctx.nativeDatabaseProvider) {
			throw new Error("native SQLite is required");
		}
		return await ctx.nativeDatabaseProvider.open(ctx.actorId);
	},
	onMigrate: async () => {},
};

async function ensureTables(database: SqliteDatabase) {
	await database.run(`
		CREATE TABLE IF NOT EXISTS restart_counter (
			id INTEGER PRIMARY KEY CHECK (id = 1),
			count INTEGER NOT NULL
		)
	`);
	await database.run(`
		CREATE TABLE IF NOT EXISTS restart_counter_events (
			id INTEGER PRIMARY KEY AUTOINCREMENT,
			count INTEGER NOT NULL,
			payload TEXT NOT NULL,
			created_at INTEGER NOT NULL
		)
	`);
	await database.run(`
		CREATE TABLE IF NOT EXISTS restart_heartbeat (
			id INTEGER PRIMARY KEY CHECK (id = 1),
			count INTEGER NOT NULL,
			updated_at INTEGER NOT NULL
		)
	`);
}

function stringifyError(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

function logRuntimeEvent(event: string, fields: Record<string, unknown>) {
	console.log(
		JSON.stringify({
			event,
			ts: Date.now(),
			...fields,
		}),
	);
}

async function runHeartbeatSql(database: SqliteDatabase): Promise<number> {
	await ensureTables(database);
	await database.run("BEGIN");
	try {
		await database.run(
			`
				INSERT INTO restart_heartbeat (id, count, updated_at)
				VALUES (1, 1, ?)
				ON CONFLICT(id) DO UPDATE SET
					count = count + 1,
					updated_at = excluded.updated_at
			`,
			[Date.now()],
		);
		await database.run("COMMIT");

		const rows = await database.query(
			"SELECT count FROM restart_heartbeat WHERE id = ?",
			[1],
		);
		return Number(rows.rows[0]?.[0] ?? 0);
	} catch (error) {
		try {
			await database.run("ROLLBACK");
		} catch (rollbackError) {
			logRuntimeEvent("heartbeat_rollback_err", {
				error: stringifyError(rollbackError),
			});
		}
		throw error;
	}
}

async function runHeartbeatKv(kv: {
	get(key: string): Promise<string | null>;
	put(key: string, value: string): Promise<void>;
}): Promise<number> {
	const current = Number((await kv.get("heartbeat_count")) ?? "0");
	const next = current + 1;
	await kv.put("heartbeat_count", String(next));
	return next;
}

const sqliteCounter = actor({
	state: {},
	vars: {} as HeartbeatVars,
	db: rawSqlDatabaseProvider,
	onRequest: (ctx, request) => {
		const url = new URL(request.url);
		if (url.pathname !== "/health") {
			return new Response("not found", { status: 404 });
		}

		logRuntimeEvent("gateway_health_request", {
			actorId: ctx.actorId,
			key: ctx.key,
		});
		return new Response(
			JSON.stringify({
				ok: true,
				actorId: ctx.actorId,
				key: ctx.key,
			}),
			{
				headers: { "Content-Type": "application/json" },
			},
		);
	},
	onWebSocket: (ctx, websocket) => {
		logRuntimeEvent("gateway_websocket_open", {
			actorId: ctx.actorId,
			key: ctx.key,
			path: ctx.request ? new URL(ctx.request.url).pathname : "unknown",
		});
		websocket.addEventListener("message", (event: { data: unknown }) => {
			const data =
				typeof event.data === "string"
					? event.data
					: String(event.data);
			try {
				const message = JSON.parse(data) as {
					type?: string;
					sentAt?: number;
				};
				if (message.type === "ping") {
					websocket.send(
						JSON.stringify({
							type: "pong",
							sentAt: message.sentAt,
							actorId: ctx.actorId,
							key: ctx.key,
						}),
					);
					return;
				}
			} catch {}

			websocket.send(data);
		});
		websocket.addEventListener("close", () => {
			logRuntimeEvent("gateway_websocket_close", {
				actorId: ctx.actorId,
				key: ctx.key,
			});
		});
	},
	onWake: async (ctx) => {
		const vars = ctx.vars as HeartbeatVars;
		if (vars.heartbeatTimer) {
			return;
		}

		const database = ctx.db as SqliteDatabase;
		vars.heartbeatSeq = 0;
		logRuntimeEvent("heartbeat_on_wake", {
			actorId: ctx.actorId,
			key: ctx.key,
			mode: heartbeatMode,
		});
		if (heartbeatMode === "none") {
			return;
		}

		const tick = async () => {
			if (ctx.abortSignal.aborted || vars.heartbeatInFlight) {
				return;
			}

			vars.heartbeatInFlight = true;
			const seq = (vars.heartbeatSeq ?? 0) + 1;
			vars.heartbeatSeq = seq;
			logRuntimeEvent("heartbeat_tick", {
				actorId: ctx.actorId,
				key: ctx.key,
				seq,
				mode: heartbeatMode,
			});

			try {
				if (heartbeatMode === "sqlite") {
					const count = await runHeartbeatSql(database);
					logRuntimeEvent("heartbeat_sql_ok", {
						actorId: ctx.actorId,
						key: ctx.key,
						seq,
						count,
					});
				} else {
					const count = await runHeartbeatKv(ctx.kv);
					logRuntimeEvent("heartbeat_kv_ok", {
						actorId: ctx.actorId,
						key: ctx.key,
						seq,
						count,
					});
				}
			} catch (error) {
				logRuntimeEvent(
					heartbeatMode === "sqlite"
						? "heartbeat_sql_err"
						: "heartbeat_kv_err",
					{
						actorId: ctx.actorId,
						key: ctx.key,
						seq,
						error: stringifyError(error),
					},
				);
			} finally {
				vars.heartbeatInFlight = false;
			}
		};

		vars.heartbeatTimer = setInterval(() => {
			void tick();
		}, 1_000);

		ctx.abortSignal.addEventListener(
			"abort",
			() => {
				if (vars.heartbeatTimer) {
					clearInterval(vars.heartbeatTimer);
					vars.heartbeatTimer = undefined;
				}
				logRuntimeEvent("heartbeat_abort", {
					actorId: ctx.actorId,
					key: ctx.key,
					mode: heartbeatMode,
				});
			},
			{ once: true },
		);

		await tick();
	},
	onSleep: (ctx) => {
		const vars = ctx.vars as HeartbeatVars;
		if (vars.heartbeatTimer) {
			clearInterval(vars.heartbeatTimer);
			vars.heartbeatTimer = undefined;
		}
		logRuntimeEvent("heartbeat_on_sleep", {
			actorId: ctx.actorId,
			key: ctx.key,
			mode: heartbeatMode,
		});
	},
	actions: {
		tick: async (ctx, payloadBytes = 4096) => {
			const database = ctx.db as SqliteDatabase;
			const payload = "x".repeat(Math.max(0, Math.trunc(payloadBytes)));
			const now = Date.now();

			await ensureTables(database);
			await database.run("BEGIN");
			try {
				await database.run(
					`
						INSERT INTO restart_counter (id, count)
						VALUES (1, 1)
						ON CONFLICT(id) DO UPDATE SET count = count + 1
					`,
				);
				const counterRows = await database.query(
					"SELECT count FROM restart_counter WHERE id = ?",
					[1],
				);
				const count = Number(counterRows.rows[0]?.[0] ?? 0);
				await database.run(
					`
						INSERT INTO restart_counter_events (count, payload, created_at)
						VALUES (?, ?, ?)
					`,
					[count, payload, now],
				);
				await database.run(
					`
						DELETE FROM restart_counter_events
						WHERE id IN (
							SELECT id FROM restart_counter_events
							ORDER BY id ASC
							LIMIT max((SELECT COUNT(*) FROM restart_counter_events) - 200, 0)
						)
					`,
				);
				await database.run("COMMIT");

				const eventRows = await database.query(
					"SELECT COUNT(*) AS events FROM restart_counter_events",
				);

				return {
					count,
					events: Number(eventRows.rows[0]?.[0] ?? 0),
				};
			} catch (error) {
				await database.run("ROLLBACK");
				throw error;
			}
		},
		getCount: async (ctx) => {
			const database = ctx.db as SqliteDatabase;
			await ensureTables(database);
			const rows = await database.query(
				"SELECT count FROM restart_counter WHERE id = ?",
				[1],
			);
			return Number(rows.rows[0]?.[0] ?? 0);
		},
		commitDuringEngineRestart: async (
			ctx,
			input: {
				signalUrl: string;
				delayBeforeCommitMs?: number;
				payloadBytes?: number;
			},
		) => {
			const database = ctx.db as SqliteDatabase;
			const payload = "x".repeat(
				Math.max(0, Math.trunc(input.payloadBytes ?? 8192)),
			);

			await ensureTables(database);
			await database.run("BEGIN");
			await database.run(
				`
					INSERT INTO restart_counter (id, count)
					VALUES (1, 1)
					ON CONFLICT(id) DO UPDATE SET count = count + 1
				`,
			);
			await database.run(
				`
					INSERT INTO restart_counter_events (count, payload, created_at)
					VALUES ((SELECT count FROM restart_counter WHERE id = 1), ?, ?)
				`,
				[payload, Date.now()],
			);

			await fetch(input.signalUrl, { method: "POST" });
			await sleep(
				Math.max(0, Math.trunc(input.delayBeforeCommitMs ?? 500)),
			);

			const commitStartedAt = Date.now();
			try {
				await database.run("COMMIT");
				return {
					ok: true,
					commitDurationMs: Date.now() - commitStartedAt,
				};
			} catch (commitError) {
				let rollbackErrorMessage: string | undefined;
				try {
					await database.run("ROLLBACK");
				} catch (rollbackError) {
					rollbackErrorMessage = stringifyError(rollbackError);
				}

				console.error(
					JSON.stringify({
						event: "commitDuringEngineRestartFailed",
						commitDurationMs: Date.now() - commitStartedAt,
						commitError: stringifyError(commitError),
						rollbackError: rollbackErrorMessage,
					}),
				);

				throw new Error(
					`commit failed: ${stringifyError(commitError)}; rollback failed: ${rollbackErrorMessage ?? "no"}`,
				);
			}
		},
	},
	options: {
		sleepTimeout: 300_000,
	},
});

const registry = setup({
	use: {
		sqliteCounter,
	},
	runtime: "native",
	sqlite: "remote",
	endpoint,
	namespace,
	token,
	envoy: {
		poolName,
	},
	serverless: {
		basePath: "/api/rivet",
	},
	noWelcome: true,
	test: {
		enabled: true,
		sqliteBackend: "remote",
	},
});

const server = serve(
	{
		fetch: (request) => registry.handler(request),
		hostname: host,
		port,
	},
	() => {
		console.log(
			JSON.stringify({
				event: "listening",
				url: `http://${host}:${port}/api/rivet`,
			}),
		);
	},
);

function shutdown() {
	server.close(() => {
		process.exit(0);
	});
}

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);
