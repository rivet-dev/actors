import { Loop, queue, setup, workflow } from "@rivet-dev/workflows";

type WorkMessage = { amount: number };
type ControlMessage = { type: "stop"; reason: string };

const worker = workflow({
	state: {
		phase: "idle" as "idle" | "running" | "stopped",
		processed: 0,
		total: 0,
		stopReason: null as string | null,
	},
	queues: {
		work: queue<WorkMessage>(),
		control: queue<ControlMessage>(),
	},
	run: async (ctx) => {
		await ctx.step("setup", async (ctx) => {
			await fetch("https://api.example.com/workers/init", {
				method: "POST",
			});
			ctx.state.phase = "running";
			ctx.state.stopReason = null;
		});

		const stopReason = await ctx.loop("worker-loop", async (loopCtx) => {
			const message = await loopCtx.queue.next("wait-command", {
				names: ["work", "control"],
			});

			if (message.name === "work") {
				await loopCtx.step("apply-work", async (loopCtx) => {
					await fetch("https://api.example.com/workers/process", {
						method: "POST",
						body: JSON.stringify({ amount: message.body.amount }),
					});
					loopCtx.state.processed += 1;
					loopCtx.state.total += message.body.amount;
				});
				return;
			}

			return Loop.break((message.body as ControlMessage).reason);
		});

		await ctx.step("teardown", async (ctx) => {
			await fetch("https://api.example.com/workers/shutdown", {
				method: "POST",
			});
			ctx.state.phase = "stopped";
			ctx.state.stopReason = stopReason;
		});
	},
});

const registry = setup({ use: { worker } });
