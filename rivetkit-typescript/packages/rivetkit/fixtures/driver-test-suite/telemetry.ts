import { actor, queue } from "rivetkit";
import { db } from "@/common/database/mod";

const jobSchema = queue<{ id: string }>();

export const telemetryActor = actor({
	state: { count: 0 },
	db: db(),
	queues: {
		jobs: jobSchema,
	},
	actions: {
		getCount: (c) => c.state.count,
		increment: (c, amount: number) => {
			c.state.count += amount;
			return c.state.count;
		},
		sqliteFailure: async (c) => {
			await c.db.execute("SELECT value FROM missing_trace_test_table");
		},
		stateTransaction: async (c, amount: number) => {
			await c.db.transaction(
				async (tx) => {
					await tx.execute("SELECT 1");
					c.state.count += amount;
				},
				{ experimental: { includeState: true } },
			);
			return c.state.count;
		},
	},
});
