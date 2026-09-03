import { actor } from "rivetkit";

const encoder = new TextEncoder();

export const notificationsActor = actor({
	state: {},
	onRequest: (_c, request) => {
		let heartbeat: ReturnType<typeof setInterval> | undefined;
		const stopHeartbeat = () => {
			if (heartbeat !== undefined) {
				clearInterval(heartbeat);
				heartbeat = undefined;
			}
		};

		const stream = new ReadableStream<Uint8Array>({
			start(controller) {
				controller.enqueue(
					encoder.encode(
						'retry: 5000\nid: 1\nevent: ready\ndata: {"connected":true}\n\n',
					),
				);

				// SSE comments keep intermediary proxies from closing an idle stream.
				heartbeat = setInterval(() => {
					controller.enqueue(encoder.encode(": keepalive\n\n"));
				}, 15_000);

				request.signal.addEventListener("abort", stopHeartbeat, {
					once: true,
				});
			},
			cancel: stopHeartbeat,
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
