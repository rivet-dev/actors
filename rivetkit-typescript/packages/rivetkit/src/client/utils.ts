import invariant from "invariant";
import type { VersionedDataHandler } from "vbare";
import type { z } from "zod/v4";
import { HTTP_RESPONSE_ERROR_VERSIONED } from "@/common/client-protocol-versioned";
import { HttpResponseErrorSchema } from "@/common/client-protocol-zod";
import type { Encoding } from "@/common/encoding";
import { assertUnreachable } from "@/common/utils";
import {
	contentTypeForEncoding,
	decodeCborCompat,
	deserializeWithEncoding,
	serializeWithEncoding,
} from "@/serde";
import { httpUserAgent } from "@/utils";
import { ActorError, HttpRequestError } from "./errors";
import { logger } from "./log";

export interface ParsedCloseReason {
	group: string;
	code: string;
	rayId?: string;
}

/**
 * Parses WebSocket close reason string into structured data.
 *
 * Expected format examples:
 *   - "guard.actor_runner_failed#t1s80so6h3irenp8ymzltfoittcl00"
 *   - "ws.client_closed"
 *
 * Returns undefined if the format is invalid
 */
export function parseWebSocketCloseReason(
	reason: string,
): ParsedCloseReason | undefined {
	const [mainPart, rayId] = reason.split("#");
	const [group, code] = mainPart.split(".");

	if (!group || !code) {
		logger().warn({ msg: "failed to parse close reason", reason });
		return undefined;
	}

	return {
		group,
		code,
		rayId,
	};
}

export type WebSocketMessage = string | Blob | ArrayBuffer | Uint8Array;

export function messageLength(message: WebSocketMessage): number {
	if (message instanceof Blob) {
		return message.size;
	}
	if (message instanceof ArrayBuffer) {
		return message.byteLength;
	}
	if (message instanceof Uint8Array) {
		return message.byteLength;
	}
	if (typeof message === "string") {
		return message.length;
	}
	assertUnreachable(message);
}

export interface HttpRequestOpts<
	RequestBare,
	ResponseBare,
	RequestJson = RequestBare,
	ResponseJson = ResponseBare,
	Request = RequestBare,
	Response = ResponseBare,
> {
	method: string;
	url: string;
	headers: Record<string, string>;
	body?: Request;
	encoding: Encoding;
	skipParseResponse?: boolean;
	signal?: AbortSignal;
	customFetch?: (req: globalThis.Request) => Promise<globalThis.Response>;
	requestVersionedDataHandler: VersionedDataHandler<RequestBare> | undefined;
	requestVersion: number | undefined;
	responseVersionedDataHandler:
		| VersionedDataHandler<ResponseBare>
		| undefined;
	responseVersion: number | undefined;
	requestZodSchema: z.ZodType<RequestJson>;
	responseZodSchema: z.ZodType<ResponseJson>;
	requestToJson: (value: Request) => RequestJson;
	requestToBare: (value: Request) => RequestBare;
	responseFromJson: (value: ResponseJson) => Response;
	responseFromBare: (value: ResponseBare) => Response;
}

export async function sendHttpRequest<
	RequestBare = unknown,
	ResponseBare = unknown,
	RequestJson = RequestBare,
	ResponseJson = ResponseBare,
	Request = RequestBare,
	Response = ResponseBare,
>(
	opts: HttpRequestOpts<
		RequestBare,
		ResponseBare,
		RequestJson,
		ResponseJson,
		Request,
		Response
	>,
): Promise<Response> {
	logger().debug({
		msg: "sending http request",
		url: opts.url,
		encoding: opts.encoding,
	});

	// Serialize body
	let contentType: string | undefined;
	let bodyData: string | Uint8Array | undefined;
	if (opts.method === "POST" || opts.method === "PUT") {
		invariant(opts.body !== undefined, "missing body");
		contentType = contentTypeForEncoding(opts.encoding);
		bodyData = serializeWithEncoding<RequestBare, RequestJson, Request>(
			opts.encoding,
			opts.body,
			opts.requestVersionedDataHandler,
			opts.requestVersion,
			opts.requestZodSchema,
			opts.requestToJson,
			opts.requestToBare,
		);
	}

	// Send request
	let response: globalThis.Response;
	try {
		// Make the HTTP request
		response = await (opts.customFetch ?? fetch)(
			new globalThis.Request(opts.url, {
				method: opts.method,
				headers: {
					...opts.headers,
					...(contentType
						? {
								"Content-Type": contentType,
							}
						: {}),
					"User-Agent": httpUserAgent(),
				},
				body: bodyData,
				credentials: "include",
				signal: opts.signal,
			}),
		);
	} catch (error) {
		throw new HttpRequestError(`Request failed: ${error}`, {
			cause: error,
		});
	}

	// Parse response error
	if (!response.ok) {
		const bufferResponse = await response.arrayBuffer();
		const contentType = response.headers.get("content-type");
		const rayId = response.headers.get("x-rivet-ray-id");

		// Determine encoding from Content-Type header, defaulting to provided encoding
		const encoding: Encoding = contentType?.includes("application/json")
			? "json"
			: opts.encoding;

		// Attempt to parse structured error data
		try {
			const responseData = deserializeWithEncoding(
				encoding,
				new Uint8Array(bufferResponse),
				HTTP_RESPONSE_ERROR_VERSIONED,
				HttpResponseErrorSchema,
				// JSON/CBOR: normalize actor generation to the public number shape.
				(json): any => ({
					...json,
					actor: json.actor
						? {
								...json.actor,
								generation: Number(json.actor.generation),
							}
						: undefined,
				}),
				// BARE: decode ArrayBuffer metadata to unknown
				(bare): any => ({
					group: bare.group,
					code: bare.code,
					message: bare.message,
					metadata: bare.metadata
						? decodeCborCompat(new Uint8Array(bare.metadata))
						: undefined,
					actor: bare.actor
						? {
								actorId: bare.actor.actorId,
								generation: Number(bare.actor.generation),
								key: bare.actor.key ?? undefined,
							}
						: undefined,
				}),
			);

			logger().warn({
				msg: "http error response",
				group: responseData.group,
				code: responseData.code,
				message: responseData.message,
				metadata: responseData.metadata,
				rayId,
				actorId: responseData.actor?.actorId,
				generation: responseData.actor?.generation,
				actorKey: responseData.actor?.key,
			});

			throw new ActorError(
				responseData.group,
				responseData.code,
				responseData.message,
				{
					metadata: responseData.metadata,
					rayId: rayId ?? undefined,
					actor: responseData.actor,
				},
			);
		} catch (error) {
			// If it's already an ActorError, re-throw it
			if (error instanceof ActorError) {
				throw error;
			}

			// Otherwise, fall back to generic error with text response
			const textResponse = new TextDecoder("utf-8", {
				fatal: false,
			}).decode(bufferResponse);

			if (rayId) {
				throw new HttpRequestError(
					`${response.statusText} (${response.status}) (Ray ID: ${rayId}):\n${textResponse}`,
				);
			} else {
				throw new HttpRequestError(
					`${response.statusText} (${response.status}):\n${textResponse}`,
				);
			}
		}
	}

	// Some requests don't need the success response to be parsed, so this can speed things up
	if (opts.skipParseResponse) {
		return undefined as Response;
	}

	// Parse the response based on encoding
	try {
		const buffer = new Uint8Array(await response.arrayBuffer());
		return deserializeWithEncoding<ResponseBare, ResponseJson, Response>(
			opts.encoding,
			buffer,
			opts.responseVersionedDataHandler,
			opts.responseZodSchema,
			opts.responseFromJson,
			opts.responseFromBare,
		);
	} catch (error) {
		throw new HttpRequestError(`Failed to parse response: ${error}`, {
			cause: error,
		});
	}
}
