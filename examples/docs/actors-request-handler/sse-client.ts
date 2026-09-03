import { createClient } from "rivetkit/client";
import type { registry } from "./sse-server";

const client = createClient<typeof registry>("http://localhost:6420");
const notifications = client.notifications.getOrCreate(["status"]);
const response = await notifications.fetch("/", {
	headers: { Accept: "text/event-stream" },
});

if (!response.ok || !response.body) {
	throw new Error(`SSE request failed with status ${response.status}`);
}

const reader = response.body.getReader();
const decoder = new TextDecoder();
let pending = "";

for (;;) {
	const chunk = await reader.read();
	if (chunk.done) break;

	pending += decoder.decode(chunk.value, { stream: true });
	for (;;) {
		const boundary = pending.indexOf("\n\n");
		if (boundary === -1) break;
		const event = pending.slice(0, boundary);
		pending = pending.slice(boundary + 2);
		console.log(event);
	}
}
