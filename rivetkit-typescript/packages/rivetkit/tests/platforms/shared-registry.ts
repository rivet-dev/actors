import {
	actor,
	type RegistryConfigInput,
	setup,
	type WasmRuntimeBindings,
	type WasmRuntimeInitInput,
} from "rivetkit";

interface SqliteDatabase {
	run(sql: string, params?: unknown[]): Promise<void>;
	query(
		sql: string,
		params?: unknown[],
	): Promise<{
		rows: unknown[][];
	}>;
}

const COUNTER_ID = 1;

const rawSqlDatabaseProvider = {
	createClient: async (ctx: any) => {
		if (!ctx.nativeDatabaseProvider) {
			throw new Error("native SQLite is required");
		}
		return await ctx.nativeDatabaseProvider.open(ctx.actorId);
	},
	onMigrate: async () => {},
};

async function ensureCounterTable(db: SqliteDatabase) {
	await db.run(`
		CREATE TABLE IF NOT EXISTS platform_counter (
			id INTEGER PRIMARY KEY CHECK (id = 1),
			count INTEGER NOT NULL
		)
	`);
}

async function ensureLifecycleTable(db: SqliteDatabase) {
	await db.run(`
		CREATE TABLE IF NOT EXISTS platform_counter_lifecycle (
			event TEXT PRIMARY KEY,
			count INTEGER NOT NULL
		)
	`);
}

async function recordLifecycleEvent(db: SqliteDatabase, event: string) {
	await ensureLifecycleTable(db);
	await db.run(
		`
			INSERT INTO platform_counter_lifecycle (event, count)
			VALUES (?, 1)
			ON CONFLICT(event) DO UPDATE SET count = count + 1
		`,
		[event],
	);
}

async function readCounter(db: SqliteDatabase): Promise<number> {
	const result = await db.query(
		"SELECT count FROM platform_counter WHERE id = ?",
		[COUNTER_ID],
	);

	return Number(result.rows[0]?.[0] ?? 0);
}

async function readLifecycleCounts(db: SqliteDatabase): Promise<{
	wakeCount: number;
	sleepCount: number;
}> {
	await ensureLifecycleTable(db);
	const result = await db.query(
		"SELECT event, count FROM platform_counter_lifecycle",
	);
	const counts = new Map(
		result.rows.map((row) => [String(row[0]), Number(row[1])]),
	);

	return {
		wakeCount: counts.get("wake") ?? 0,
		sleepCount: counts.get("sleep") ?? 0,
	};
}

export const sqliteCounterActor = actor({
	db: rawSqlDatabaseProvider,
	onWake: async (ctx) => {
		await recordLifecycleEvent(ctx.db as SqliteDatabase, "wake");
	},
	onSleep: async (ctx) => {
		await recordLifecycleEvent(ctx.db as SqliteDatabase, "sleep");
	},
	actions: {
		increment: async (ctx, amount = 1) => {
			const db = ctx.db as SqliteDatabase;
			await ensureCounterTable(db);
			await db.run(
				`
					INSERT INTO platform_counter (id, count)
					VALUES (?, ?)
					ON CONFLICT(id) DO UPDATE SET count = count + excluded.count
				`,
				[COUNTER_ID, amount],
			);

			return await readCounter(db);
		},
		getCount: async (ctx) => {
			const db = ctx.db as SqliteDatabase;
			await ensureCounterTable(db);

			return await readCounter(db);
		},
		getLifecycleCounts: async (ctx) => {
			return await readLifecycleCounts(ctx.db as SqliteDatabase);
		},
		triggerSleep: (ctx) => {
			ctx.sleep();
		},
	},
	options: {
		sleepTimeout: 100,
	},
});

export const platformSqliteCounterActors = {
	sqliteCounter: sqliteCounterActor,
};

type PlatformSqliteCounterActors = typeof platformSqliteCounterActors;

export type PlatformSqliteCounterRegistryOptions = Omit<
	RegistryConfigInput<PlatformSqliteCounterActors>,
	"runtime" | "sqlite" | "test" | "use" | "wasm"
> & {
	bindings: WasmRuntimeBindings;
	initInput?: WasmRuntimeInitInput;
};

export function createPlatformSqliteCounterRegistry({
	bindings,
	initInput,
	...config
}: PlatformSqliteCounterRegistryOptions) {
	return setup({
		...config,
		runtime: "wasm",
		sqlite: "remote",
		wasm: {
			bindings,
			initInput,
		},
		use: platformSqliteCounterActors,
	});
}

export type PlatformSqliteCounterRegistry = ReturnType<
	typeof createPlatformSqliteCounterRegistry
>;
