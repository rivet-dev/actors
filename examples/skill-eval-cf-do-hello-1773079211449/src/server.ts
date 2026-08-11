import { Hono } from "hono";
import { createClient } from "rivetkit/client";
import { registry } from "./actors.ts";

const app = new Hono();
const client = createClient<typeof registry>();

app.get("/", async (c) => {
	const handle = client.myActor.getOrCreate([
		new URL(c.req.url).pathname,
	]);
	const greeting = await handle.sayHello();
	return c.text(greeting);
});

app.all("/api/rivet/*", (c) => registry.handler(c.req.raw));

export default app;
