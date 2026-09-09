import { actor, queue } from "rivetkit";
import { db } from "@/common/database/mod";

const jobSchema = queue<{ id: string }>();

export const telemetryActor = actor({
	state: { count: 0 },
	db: db(),
	queues: {
		jobs: jobSchema,
	},
	onRequest: async (c, request) => {
		await c.db.execute("SELECT ? AS path", new URL(request.url).pathname);
		return new Response("ok", { status: 200 });
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
		insertAfterReply: (c, token: string) => {
			c.waitUntil(
				c.queue
					.next({ names: ["jobs"], timeout: 10_000 })
					.then((message) => {
						if (!message)
							throw new Error("deferred work was not released");
						return c.db.execute("SELECT ? AS deferred", token);
					}),
			);
			return "replied";
		},
		scheduleTrace: async (c, correlationToken: string) => {
			await c.schedule.after(50, "scheduledTrace", correlationToken);
			return correlationToken;
		},
		scheduledTrace: async (c, correlationToken: string) => {
			await c.db.execute("SELECT ? AS trace", correlationToken);
		},
		consumeJob: async (c) => {
			const message = await c.queue.next({
				names: ["jobs"],
				timeout: 5_000,
			});
			return message?.body ?? null;
		},
	},
});

export const telemetryRunConsumerActor = actor({
	state: {},
	queues: {
		runJobs: jobSchema,
	},
	run: async (c) => {
		while (!c.aborted) {
			await c.queue.waitForNames(["runJobs"], {
				signal: c.abortSignal,
			});
		}
	},
	actions: {},
});
