import { expect, test } from "vitest";
import { parseEventStream } from "./driver/sse-contract-harness";

test("the SSE parser strips exactly one leading BOM", async () => {
	const events: string[] = [];
	const bytes = new TextEncoder().encode("\uFEFF\uFEFFdata: hidden\n\n");
	const body = new ReadableStream<Uint8Array>({
		start(controller) {
			controller.enqueue(bytes);
			controller.close();
		},
	});

	await parseEventStream(
		body,
		"",
		(event) => events.push(event.data),
		() => {},
	);

	expect(events).toEqual([]);
});

test("the SSE parser retains IDs from blocks without data", async () => {
	const events: Array<{ data: string; id: string }> = [];
	const bytes = new TextEncoder().encode("id: retained\n\n");
	const body = new ReadableStream<Uint8Array>({
		start(controller) {
			controller.enqueue(bytes);
			controller.close();
		},
	});

	const lastEventId = await parseEventStream(
		body,
		"",
		(event) => events.push({ data: event.data, id: event.id }),
		() => {},
	);

	expect(lastEventId).toBe("retained");
	expect(events).toEqual([]);
});

test("the SSE parser handles a large event split across many chunks", async () => {
	const dataLength = 5 * 1024 * 1024;
	const bytes = new TextEncoder().encode(
		`data: ${"x".repeat(dataLength)}\n\n`,
	);
	const body = new ReadableStream<Uint8Array>({
		start(controller) {
			for (
				let offset = 0;
				offset < bytes.byteLength;
				offset += 16 * 1024
			) {
				controller.enqueue(bytes.subarray(offset, offset + 16 * 1024));
			}
			controller.close();
		},
	});
	let parsedLength = 0;

	await parseEventStream(
		body,
		"",
		(event) => {
			parsedLength = event.data.length;
		},
		() => {},
	);

	expect(parsedLength).toBe(dataLength);
}, 10_000);
