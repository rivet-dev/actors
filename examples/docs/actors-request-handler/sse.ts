import { actor } from "rivetkit";

const encoder = new TextEncoder();

export const notificationsActor = actor({
	state: {},
	onRequest: () => {
		const stream = new ReadableStream<Uint8Array>({
			async start(controller) {
				controller.enqueue(
					encoder.encode('event: status\ndata: {"ready":true}\n\n'),
				);
				await new Promise((resolve) => setTimeout(resolve, 100));
				controller.enqueue(
					encoder.encode('event: status\ndata: {"ready":false}\n\n'),
				);
				controller.close();
			},
		});

		return new Response(stream, {
			headers: {
				"Content-Type": "text/event-stream; charset=utf-8",
				"Cache-Control": "no-cache, no-transform",
			},
		});
	},
	actions: {},
});
