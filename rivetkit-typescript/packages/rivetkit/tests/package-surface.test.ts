import type {
	ActionContextOf,
	ActorContextOf,
	BeforeActionResponseContextOf,
	BeforeConnectContextOf,
	ConnContextOf,
	ConnectContextOf,
	ConnInitContextOf,
	CreateConnStateContextOf,
	CreateContextOf,
	CreateVarsContextOf,
	DestroyContextOf,
	DisconnectContextOf,
	MigrateContextOf,
	RequestContextOf,
	RunContextOf,
	SleepContextOf,
	StateChangeContextOf,
	WakeContextOf,
	WebSocketContextOf,
} from "rivetkit";
import * as rivetkit from "rivetkit";
import type {
	AnyDatabaseProvider,
	DatabaseProvider,
	RawAccess,
} from "rivetkit/db";
import { db as rawDb } from "rivetkit/db";
import { defineConfig, db as drizzleDb } from "rivetkit/db/drizzle";
import {
	decodeWorkflowHistoryTransport as decodePublicWorkflowHistory,
	encodeWorkflowHistoryTransport as encodePublicWorkflowHistory,
	WorkflowEntryStatus,
} from "rivetkit/experimental/inspector/workflow";
import { decodeWorkflowHistoryTransport } from "rivetkit/inspector";
import {
	CURRENT_VERSION,
	TO_CLIENT_VERSIONED,
	TO_SERVER_VERSIONED,
	type TransportWorkflowHistory,
} from "rivetkit/inspector/client";
import { setupTest } from "rivetkit/test";
import { jsonParseCompat, jsonStringifyCompat } from "rivetkit/utils";
import { describe, expect, test } from "vitest";
import packageJson from "../package.json" with { type: "json" };

const contextTypeSmokeActor = rivetkit.actor({
	createState: (_ctx, input: { initialCount: number }) => ({
		count: input.initialCount,
	}),
	createVars: () => ({ lastSeenAt: Date.now() }),
	createConnState: (_ctx, params: { userId: string }) => ({
		userId: params.userId,
	}),
	actions: {
		increment: (ctx, amount: number) => (ctx.state.count += amount),
	},
	run: async (ctx) => {
		ctx.state.count += 1;
	},
	onRequest: () => new Response("ok"),
	onWebSocket: () => {},
});

type RestoredContextTypeSmoke = [
	ActorContextOf<typeof contextTypeSmokeActor>,
	ActionContextOf<typeof contextTypeSmokeActor>,
	BeforeActionResponseContextOf<typeof contextTypeSmokeActor>,
	BeforeConnectContextOf<typeof contextTypeSmokeActor>,
	ConnectContextOf<typeof contextTypeSmokeActor>,
	ConnContextOf<typeof contextTypeSmokeActor>,
	ConnInitContextOf<typeof contextTypeSmokeActor>,
	CreateConnStateContextOf<typeof contextTypeSmokeActor>,
	CreateContextOf<typeof contextTypeSmokeActor>,
	CreateVarsContextOf<typeof contextTypeSmokeActor>,
	DestroyContextOf<typeof contextTypeSmokeActor>,
	DisconnectContextOf<typeof contextTypeSmokeActor>,
	MigrateContextOf<typeof contextTypeSmokeActor>,
	RequestContextOf<typeof contextTypeSmokeActor>,
	RunContextOf<typeof contextTypeSmokeActor>,
	SleepContextOf<typeof contextTypeSmokeActor>,
	StateChangeContextOf<typeof contextTypeSmokeActor>,
	WakeContextOf<typeof contextTypeSmokeActor>,
	WebSocketContextOf<typeof contextTypeSmokeActor>,
];

type RestoredDatabaseSurfaceTypes = [
	DatabaseProvider<RawAccess>,
	AnyDatabaseProvider,
];

describe("package surface", () => {
	test("restores supported package entrypoints", () => {
		expect(packageJson.exports).toHaveProperty("./test");
		expect(packageJson.exports).toHaveProperty(
			"./experimental/inspector/workflow",
		);
		expect(packageJson.exports).toHaveProperty("./inspector");
		expect(packageJson.exports).toHaveProperty("./inspector/client");
		expect(packageJson.exports).toHaveProperty("./db");
		expect(packageJson.exports).toHaveProperty("./db/drizzle");
	});

	test("restored package entrypoints resolve", () => {
		expect(setupTest).toBeTypeOf("function");
		expect(decodePublicWorkflowHistory).toBeTypeOf("function");
		expect(encodePublicWorkflowHistory).toBeTypeOf("function");
		expect(WorkflowEntryStatus.COMPLETED).toBe("COMPLETED");
		expect(decodeWorkflowHistoryTransport).toBeTypeOf("function");
		expect(rawDb).toBeTypeOf("function");
		expect(drizzleDb).toBeTypeOf("function");
		expect(defineConfig).toBeTypeOf("function");
		expect(CURRENT_VERSION).toBe(6);
		expect(TO_CLIENT_VERSIONED).toBeDefined();
		expect(TO_SERVER_VERSIONED).toBeDefined();
		expect(jsonParseCompat(jsonStringifyCompat(1n))).toBe(1n);

		const history: TransportWorkflowHistory | null = null;
		expect(history).toBeNull();
	});

	test("restores root ContextOf helper exports", () => {
		const contextTypes: RestoredContextTypeSmoke | null = null;
		expect(contextTypes).toBeNull();
	});

	test("restores database provider type exports", () => {
		const databaseTypes: RestoredDatabaseSurfaceTypes | null = null;
		expect(databaseTypes).toBeNull();
	});

	test("keeps database helpers on dedicated subpaths", () => {
		expect(rivetkit).not.toHaveProperty("db");
		expect(rivetkit).not.toHaveProperty("defineConfig");
	});

	test("does not advertise deleted topology entrypoints", () => {
		expect(packageJson.exports).not.toHaveProperty(
			"./topologies/coordinate",
		);
		expect(packageJson.exports).not.toHaveProperty(
			"./topologies/partition",
		);
		expect(packageJson.scripts.build).not.toContain("src/topologies/");
	});

	test("does not keep obviously dead package metadata", () => {
		expect(packageJson.files).toContain("schemas");
		expect(packageJson.files).not.toContain("deno.json");
		expect(packageJson.files).not.toContain("bun.json");

		expect(packageJson.dependencies).not.toHaveProperty(
			"@hono/standard-validator",
		);
		expect(packageJson.dependencies).not.toHaveProperty(
			"@rivetkit/fast-json-patch",
		);
		expect(packageJson.dependencies).not.toHaveProperty(
			"@rivetkit/on-change",
		);
		expect(packageJson.dependencies).not.toHaveProperty("nanoevents");

		expect(packageJson.devDependencies).not.toHaveProperty("@types/ws");
		expect(packageJson.devDependencies).not.toHaveProperty("@vitest/ui");
		expect(packageJson.devDependencies).not.toHaveProperty("cli-table3");
		expect(packageJson.devDependencies).not.toHaveProperty("commander");
		expect(packageJson.devDependencies).not.toHaveProperty("local-pkg");
		expect(packageJson.devDependencies).not.toHaveProperty(
			"zod-to-json-schema",
		);
	});

	test("keeps intentionally removed helper entrypoints deleted", () => {
		expect(packageJson.exports).not.toHaveProperty("./driver-helpers");
		expect(packageJson.exports).not.toHaveProperty(
			"./driver-helpers/websocket",
		);
		expect(packageJson.exports).not.toHaveProperty("./dynamic");
		expect(packageJson.exports).not.toHaveProperty("./sandbox");
	});
});
