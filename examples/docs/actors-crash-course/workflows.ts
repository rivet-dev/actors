import { queue, workflow } from "@rivet-dev/workflows";

const worker = workflow({
	state: { processed: 0 },
	queues: {
		tasks: queue<{ url: string }>(),
	},
	run: async (ctx) => {
		await ctx.loop("task-loop", async (loopCtx) => {
			const message = await loopCtx.queue.next("wait-task");

			await loopCtx.step("process-task", async (loopCtx) => {
				await processTask(message.body.url);
				loopCtx.state.processed += 1;
			});
		});
	},
});

async function processTask(url: string): Promise<void> {
	const res = await fetch(url, { method: "POST" });
	if (!res.ok) throw new Error(`Task failed: ${res.status}`);
}
