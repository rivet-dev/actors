import { describe, expect, test } from "vitest";
import type { DatabaseProvider, RawAccess } from "@/common/database/config";
import { ActorContextHandleAdapter } from "@/registry/native";

describe("experimental database state transactions", () => {
	test("rejects includeState for custom database providers", async () => {
		const customClient: RawAccess = {
			execute: async () => [],
			transaction: async (callback) => await callback(customClient),
			close: async () => {},
		};
		const provider: DatabaseProvider<RawAccess> = {
			createClient: async () => customClient,
			onMigrate: async () => {},
		};
		const runtimeState = {};
		const context = new ActorContextHandleAdapter(
			{
				actorId: () => "actor-a",
				actorRuntimeState: () => runtimeState,
			} as never,
			{} as never,
			undefined,
			{},
			provider,
		);
		await context.prepare();

		await expect(context.db.transaction(async () => 42)).resolves.toBe(42);
		expect(() =>
			context.db.transaction(async () => {}, {
				experimental: { includeState: true },
			}),
		).toThrow(
			"experimental.includeState is only supported by RivetKit's embedded database provider",
		);
	});
});
