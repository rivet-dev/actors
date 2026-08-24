import { Hono } from "hono";
import { getRun, start } from "workflow/api";

import { processOrder } from "../workflows/order.ts";

const app = new Hono()
	.post("/orders/:id", async (c) => {
		const run = await start(processOrder, [c.req.param("id")]);
		return c.json({ runId: run.runId });
	})
	.get("/orders/:runId", async (c) => {
		const run = getRun(c.req.param("runId"));
		return c.json({ status: await run.status });
	});

export default app;
