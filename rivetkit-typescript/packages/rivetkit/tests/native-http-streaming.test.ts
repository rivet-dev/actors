import { describe, expect, test } from "vitest";
import { nativeHttpTestInternals } from "../src/registry/native-http";

describe("native http response streaming", () => {
	test("restores the original URL forwarded through actor fetch", () => {
		const request = nativeHttpTestInternals.buildRequest({
			method: "POST",
			uri: "/agents/assistant/local",
			headers: {
				"x-rivet-internal-original-request-url":
					"http://localhost:3583/agents/assistant/local",
				"x-user": "present",
			},
		});

		expect(request.url).toBe(
			"http://localhost:3583/agents/assistant/local",
		);
		expect(request.headers.get("x-user")).toBe("present");
		expect(
			request.headers.get("x-rivet-internal-original-request-url"),
		).toBeNull();
	});

	test("constructs requests with streaming bodies and abort signals", async () => {
		const chunks = [new Uint8Array([1]), new Uint8Array([2])];
		const controller = new AbortController();
		const request = nativeHttpTestInternals.buildRequest({
			method: "POST",
			uri: "/upload",
			body: chunks.shift(),
			bodyStream: {
				async read() {
					return chunks.shift() ?? null;
				},
				async cancel() {},
			},
			signal: controller.signal,
		});

		controller.abort();

		expect(request.signal.aborted).toBe(true);
		expect(new Uint8Array(await request.arrayBuffer())).toEqual(
			new Uint8Array([1, 2]),
		);
	});

	test("preserves native request stream chunks through Request bodies", async () => {
		const chunkSizes = [13_093, 16_384, 32_768, 19_675];
		const chunks = chunkSizes.map((size, index) => {
			const chunk = new Uint8Array(size);
			chunk.fill(index + 1);
			return chunk;
		});
		const request = nativeHttpTestInternals.buildRequest({
			method: "POST",
			uri: "/upload",
			bodyStream: {
				async read() {
					return chunks.shift() ?? null;
				},
				async cancel() {},
			},
		});

		const reader = request.body?.getReader();
		expect(reader).toBeDefined();
		if (!reader) throw new Error("request body is missing");

		const sizes: number[] = [];
		let totalBytes = 0;
		for (;;) {
			const next = await reader.read();
			if (next.done) break;
			sizes.push(next.value.byteLength);
			totalBytes += next.value.byteLength;
		}

		expect(sizes).toEqual(chunkSizes);
		expect(totalBytes).toBe(80 * 1024);
	});

	test("rejects and aborts the Request when the native body stream aborts", async () => {
		const abortController = new AbortController();
		const request = nativeHttpTestInternals.buildRequest({
			method: "POST",
			uri: "/upload",
			bodyStream: {
				async read() {
					throw new Error("ClientDisconnect: upload closed");
				},
				async cancel() {},
			},
			abortController,
		});

		await expect(request.arrayBuffer()).rejects.toThrow(
			"ClientDisconnect: upload closed",
		);
		expect(request.signal.aborted).toBe(true);
	});

	test("explicitly cancels an unread native upload after an early response", async () => {
		let cancelCount = 0;
		const bodyStream = {
			async read() {
				return new Promise<Uint8Array | null>(() => {});
			},
			async cancel() {
				cancelCount++;
			},
		};
		const request = nativeHttpTestInternals.buildRequest({
			method: "POST",
			uri: "/upload",
			bodyStream,
		});

		expect(request.bodyUsed).toBe(false);
		await nativeHttpTestInternals.cancelRequestBody(bodyStream);
		expect(cancelCount).toBe(1);
	});

	test("streams multi-chunk responses through the native body stream", async () => {
		const writes: Uint8Array[] = [];
		let finish!: () => void;
		const finished = new Promise<void>((resolve) => {
			finish = resolve;
		});

		const responseBodyStream = {
			async cancelled() {
				return new Promise<void>(() => {});
			},
			async write(chunk: Uint8Array) {
				writes.push(new Uint8Array(chunk));
			},
			async end() {
				finish();
			},
			async error(message: string) {
				throw new Error(message);
			},
		};
		const largeChunk = new Uint8Array(64 * 1024 + 1);
		largeChunk.fill(7);
		const response = new Response(
			new ReadableStream<Uint8Array>({
				start(controller) {
					controller.enqueue(new Uint8Array([1]));
					controller.enqueue(largeChunk);
					controller.close();
				},
			}),
			{
				status: 202,
				headers: {
					"x-streamed": "yes",
				},
			},
		);

		const { response: runtimeResponse } =
			await nativeHttpTestInternals.convertRuntimeHttpResponse(
				response,
				responseBodyStream,
			);
		await finished;

		expect(runtimeResponse).toEqual({
			status: 202,
			headers: {
				"x-streamed": "yes",
			},
			stream: true,
		});
		expect(writes.map((chunk) => chunk.byteLength)).toEqual([
			1,
			64 * 1024,
			1,
		]);
		expect(writes[0]).toEqual(new Uint8Array([1]));
		expect(writes[1][0]).toBe(7);
		expect(writes[2][0]).toBe(7);
	});

	test("does not impose a cumulative 20 MiB response limit", async () => {
		const responseSize = 20 * 1024 * 1024 + 1;
		let writtenBytes = 0;
		const responseBodyStream = {
			async cancelled() {
				return new Promise<void>(() => {});
			},
			async write(chunk: Uint8Array) {
				writtenBytes += chunk.byteLength;
			},
			async end() {},
			async error(message: string) {
				throw new Error(message);
			},
		};
		const response = new Response(
			new ReadableStream<Uint8Array>({
				start(controller) {
					controller.enqueue(new Uint8Array(responseSize));
					controller.close();
				},
			}),
		);

		const conversion =
			await nativeHttpTestInternals.convertRuntimeHttpResponse(
				response,
				responseBodyStream,
			);
		await conversion.bodyCompletion;

		expect(conversion.response.stream).toBe(true);
		expect(writtenBytes).toBe(responseSize);
	});

	test("returns streamed response headers before the first body chunk", async () => {
		let bodyController!: ReadableStreamDefaultController<Uint8Array>;
		let finish!: () => void;
		const finished = new Promise<void>((resolve) => {
			finish = resolve;
		});
		const responseBodyStream = {
			async cancelled() {
				return new Promise<void>(() => {});
			},
			async write() {},
			async end() {
				finish();
			},
			async error(message: string) {
				throw new Error(message);
			},
		};
		const response = new Response(
			new ReadableStream<Uint8Array>({
				start(controller) {
					bodyController = controller;
				},
			}),
			{
				status: 200,
				headers: {
					"content-type": "text/event-stream",
				},
			},
		);

		const runtimeResponsePromise = nativeHttpTestInternals
			.convertRuntimeHttpResponse(response, responseBodyStream)
			.then(({ response }) => response);
		const headersReturnedBeforeBody = await Promise.race([
			runtimeResponsePromise.then(() => true),
			new Promise<false>((resolve) =>
				setTimeout(() => resolve(false), 10),
			),
		]);

		bodyController.enqueue(new TextEncoder().encode("data: ready\n\n"));
		bodyController.close();
		const runtimeResponse = await runtimeResponsePromise;
		await finished;

		expect(headersReturnedBeforeBody).toBe(true);
		expect(runtimeResponse).toEqual({
			status: 200,
			headers: {
				"content-type": "text/event-stream",
			},
			stream: true,
		});
	});

	test("keeps streaming response lifecycle open until the body finishes", async () => {
		let bodyController!: ReadableStreamDefaultController<Uint8Array>;
		const responseBodyStream = {
			async cancelled() {
				return new Promise<void>(() => {});
			},
			async write() {},
			async end() {},
			async error(message: string) {
				throw new Error(message);
			},
		};
		const response = new Response(
			new ReadableStream<Uint8Array>({
				start(controller) {
					bodyController = controller;
				},
			}),
		);

		const conversion =
			await nativeHttpTestInternals.convertRuntimeHttpResponse(
				response,
				responseBodyStream,
			);
		expect(conversion.response.stream).toBe(true);
		expect(conversion.bodyCompletion).toBeDefined();

		let bodySettled = false;
		void conversion.bodyCompletion?.then(() => {
			bodySettled = true;
		});
		await Promise.resolve();
		expect(bodySettled).toBe(false);

		bodyController.close();
		await conversion.bodyCompletion;
		expect(bodySettled).toBe(true);
	});

	test("cancels an idle streamed response when the native receiver drops", async () => {
		let cancelNative!: () => void;
		const nativeCancelled = new Promise<void>((resolve) => {
			cancelNative = resolve;
		});
		let cancelBody!: (reason?: unknown) => void;
		const bodyCancelled = new Promise<unknown>((resolve) => {
			cancelBody = resolve;
		});
		const responseBodyStream = {
			cancelled: () => nativeCancelled,
			async write() {},
			async end() {},
			async error() {},
		};
		const response = new Response(
			new ReadableStream<Uint8Array>({
				cancel: cancelBody,
			}),
			{
				headers: {
					"content-type": "text/event-stream",
				},
			},
		);

		const conversion =
			await nativeHttpTestInternals.convertRuntimeHttpResponse(
				response,
				responseBodyStream,
			);
		cancelNative();
		const reason = await bodyCancelled;
		await conversion.bodyCompletion;

		expect(conversion.response.stream).toBe(true);
		expect(reason).toBeInstanceOf(Error);
		expect((reason as Error).message).toContain("receiver dropped");
	});
});
