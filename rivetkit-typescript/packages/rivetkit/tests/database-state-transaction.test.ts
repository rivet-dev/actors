import { describe, expect, test } from "vitest";
import type { DatabaseProvider, RawAccess } from "@/common/database/config";
import { ActorContextHandleAdapter } from "@/registry/native";

describe("experimental database state transactions", () => {
	test("does not wrap or mutate custom database clients", async () => {
		class CustomClient implements RawAccess {
			#normalTransactions = 0;

			get normalTransactions(): number {
				return this.#normalTransactions;
			}

			async execute() {
				return [];
			}

			async transaction<T>(callback: (tx: RawAccess) => Promise<T> | T) {
				this.#normalTransactions++;
				return await callback(this);
			}

			async close() {}
		}
		const customClient = new CustomClient();
		Object.freeze(customClient);
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

		expect(context.db).toBe(customClient);
		await expect(context.db.transaction(async () => 42)).resolves.toBe(42);
		expect(context.db.normalTransactions).toBe(1);
	});
});
