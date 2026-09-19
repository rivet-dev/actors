import { expectTypeOf, test } from "vitest";
import type { SynchronousRawAccess } from "./mod";
import { db as drizzleDb } from "./drizzle";

test("synchronous SQLite preserves return types and rejects async callbacks", () => {
	const check = (client: SynchronousRawAccess) => {
		expectTypeOf(client.transactionSync(() => 42)).toEqualTypeOf<number>();
		expectTypeOf(
			client.executeSync<{ value: number }>("SELECT 1"),
		).toEqualTypeOf<{ value: number }[]>();
		// @ts-expect-error A synchronous transaction cannot await a callback.
		client.transactionSync(async () => 42);
		// @ts-expect-error A callback returning an existing Promise is also asynchronous.
		client.transactionSync(() => Promise.resolve(42));
		client.transactionSync((tx) => {
			// @ts-expect-error Only synchronous execution is available in the callback.
			tx.execute("SELECT 1");
		});
	};
	type DrizzleClient = Awaited<
		ReturnType<ReturnType<typeof drizzleDb>["createClient"]>
	>;
	expectTypeOf<DrizzleClient>().toExtend<SynchronousRawAccess>();
	void check;
});
