import type { ResponseLike } from "@/common/fetch-like";
import { HEADER_ORIGINAL_REQUEST_URL } from "@/common/actor-router-consts";
import { stringifyError } from "@/common/utils";
import { logger } from "./log";
import type { RuntimeBytes, RuntimeHttpResponse } from "./runtime";

const HTTP_BODY_CHUNK_SIZE = 64 * 1024;

// Structural types keep the HTTP adapter independent of the N-API package.
export interface NativeHttpResponseBodyStream {
	cancelled(): Promise<void>;
	write(chunk: Uint8Array): Promise<void>;
	end(): Promise<void>;
	error(message: string): Promise<void>;
}

export interface NativeHttpRequestBodyStream {
	read(): Promise<Uint8Array | null | undefined>;
	cancel(): Promise<void>;
}

interface NativeHttpRequestInit {
	method: string;
	uri: string;
	headers?: Record<string, string>;
	body?: RuntimeBytes;
	bodyStream?: NativeHttpRequestBodyStream;
	signal?: AbortSignal;
	abortController?: AbortController;
}

interface NativeHttpResponseConversion {
	response: RuntimeHttpResponse;
	// Keeps request-scoped actor resources alive until the response body closes.
	bodyCompletion?: Promise<void>;
}

function runtimeBytesToArrayBuffer(value: RuntimeBytes): ArrayBuffer {
	return value.buffer.slice(
		value.byteOffset,
		value.byteOffset + value.byteLength,
	) as ArrayBuffer;
}

export function buildNativeHttpRequest(init: NativeHttpRequestInit): Request {
	const headers = new Headers(init.headers);
	const originalUrl = headers.get(HEADER_ORIGINAL_REQUEST_URL);
	headers.delete(HEADER_ORIGINAL_REQUEST_URL);
	const url =
		originalUrl ??
		(init.uri.startsWith("http")
			? init.uri
			: new URL(init.uri, "http://127.0.0.1").toString());
	const method = init.method.toUpperCase();
	const bodyForbidden = method === "GET" || method === "HEAD";
	const body = bodyForbidden
		? undefined
		: init.bodyStream
			? new ReadableStream<Uint8Array>({
					async pull(controller) {
						try {
							if (init.body && init.body.length > 0) {
								controller.enqueue(new Uint8Array(init.body));
								init.body = undefined;
								return;
							}
							const chunk = await init.bodyStream?.read();
							if (!chunk) {
								controller.close();
							} else {
								controller.enqueue(new Uint8Array(chunk));
							}
						} catch (error) {
							init.abortController?.abort(error);
							controller.error(error);
						}
					},
					async cancel() {
						await init.bodyStream?.cancel();
					},
				})
			: init.body && init.body.length > 0
				? runtimeBytesToArrayBuffer(init.body)
				: undefined;
	const streamInit =
		init.bodyStream && !bodyForbidden ? { duplex: "half" } : {};
	return new Request(url, {
		method,
		headers,
		body,
		signal: init.abortController?.signal ?? init.signal,
		...streamInit,
	} as RequestInit);
}

export async function cancelNativeHttpRequestBody(
	bodyStream?: NativeHttpRequestBodyStream,
) {
	await bodyStream?.cancel();
}

async function writeResponseChunk(
	stream: NativeHttpResponseBodyStream,
	chunk: Uint8Array,
) {
	for (
		let offset = 0;
		offset < chunk.byteLength;
		offset += HTTP_BODY_CHUNK_SIZE
	) {
		await stream.write(
			chunk.subarray(offset, offset + HTTP_BODY_CHUNK_SIZE),
		);
	}
}

async function pumpResponseBody(
	reader: ReadableStreamDefaultReader<Uint8Array>,
	stream: NativeHttpResponseBodyStream,
) {
	// Awaiting each write applies native-side backpressure. Racing the next read
	// against cancellation also stops an otherwise unbounded SSE response when
	// its downstream receiver disappears.
	const cancelled = stream
		.cancelled()
		.then(() => ({ cancelled: true }) as const);
	try {
		for (;;) {
			const read = reader
				.read()
				.then((next) => ({ cancelled: false, next }) as const);
			const result = await Promise.race([read, cancelled]);
			if (result.cancelled) {
				await reader.cancel(
					new Error("native http response stream receiver dropped"),
				);
				return;
			}
			const { next } = result;
			if (next.done) {
				await stream.end();
				return;
			}
			if (next.value?.byteLength) {
				await writeResponseChunk(stream, next.value);
			}
		}
	} catch (error) {
		try {
			await stream.error(stringifyError(error));
		} catch (streamError) {
			logger().debug({
				msg: "failed to report native http response stream error",
				error: streamError,
			});
		}
		try {
			await reader.cancel(error);
		} catch {
			// Reader may already be closed after a native-side disconnect.
		}
	} finally {
		reader.releaseLock();
	}
}

export async function convertNativeHttpResponse(
	response: ResponseLike,
	responseBodyStream?: NativeHttpResponseBodyStream,
): Promise<NativeHttpResponseConversion> {
	const headers = Object.fromEntries(response.headers.entries());
	if (!response.body) {
		return {
			response: {
				status: response.status,
				headers,
				body: new Uint8Array(),
			},
		};
	}

	if (responseBodyStream) {
		const reader = response.body.getReader();
		const bodyCompletion = pumpResponseBody(reader, responseBodyStream);
		return {
			response: {
				status: response.status,
				headers,
				stream: true,
			},
			bodyCompletion,
		};
	}

	return {
		response: {
			status: response.status,
			headers,
			body: new Uint8Array(await response.arrayBuffer()),
		},
	};
}

export const nativeHttpTestInternals = {
	buildRequest: buildNativeHttpRequest,
	cancelRequestBody: cancelNativeHttpRequestBody,
	convertRuntimeHttpResponse: convertNativeHttpResponse,
};
