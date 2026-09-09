import {
	HEADER_CONN_PARAMS,
	HEADER_ENCODING,
} from "@/common/actor-router-consts";
import type * as protocol from "@/common/client-protocol";
import {
	CURRENT_VERSION as CLIENT_PROTOCOL_CURRENT_VERSION,
	HTTP_QUEUE_SEND_REQUEST_VERSIONED,
	HTTP_QUEUE_SEND_RESPONSE_VERSIONED,
} from "@/common/client-protocol-versioned";
import {
	type HttpQueueSendRequest as HttpQueueSendRequestJson,
	HttpQueueSendRequestSchema,
	type HttpQueueSendResponse as HttpQueueSendResponseJson,
	HttpQueueSendResponseSchema,
} from "@/common/client-protocol-zod";
import type { Encoding, JsonCompatValue } from "@/common/encoding";
import { decodeCborCompat, encodeCborCompat } from "@/serde";
import { bufferToArrayBuffer } from "@/utils";
import { sendHttpRequest } from "./utils";

export interface QueueSender {
	send(
		name: string,
		body: unknown,
		options: QueueSendWaitOptions,
	): Promise<QueueSendResult>;
	send(
		name: string,
		body: unknown,
		options?: QueueSendNoWaitOptions,
	): Promise<void>;
}

export interface QueueSendWaitOptions {
	wait: true;
	timeout?: number;
	signal?: AbortSignal;
}

export interface QueueSendNoWaitOptions {
	wait?: false;
	timeout?: never;
	signal?: AbortSignal;
}

export type QueueSendOptions = QueueSendWaitOptions | QueueSendNoWaitOptions;

export interface QueueSendResult<TResponse = unknown> {
	status: "completed" | "timedOut";
	response?: TResponse;
}

interface QueueSenderOptions {
	encoding: Encoding;
	params: unknown;
	/** Ray ID and trace context headers, read per send. */
	telemetryHeaders: () => Record<string, string>;
	customFetch: (request: Request) => Promise<Response>;
}

export function createQueueSender(
	senderOptions: QueueSenderOptions,
): QueueSender {
	async function send(
		name: string,
		body: unknown,
		options: QueueSendWaitOptions,
	): Promise<QueueSendResult>;
	async function send(
		name: string,
		body: unknown,
		options?: QueueSendNoWaitOptions,
	): Promise<void>;
	async function send(
		name: string,
		body: unknown,
		options?: QueueSendOptions,
	): Promise<QueueSendResult | void> {
		const wait = options?.wait ?? false;
		const timeout = options?.timeout;

		const result = await sendHttpRequest<
			protocol.HttpQueueSendRequest,
			protocol.HttpQueueSendResponse,
			HttpQueueSendRequestJson,
			HttpQueueSendResponseJson,
			{ body: unknown; wait?: boolean; timeout?: number; name?: string },
			QueueSendResult
		>({
			url: `http://actor/queue/${encodeURIComponent(name)}`,
			method: "POST",
			headers: {
				[HEADER_ENCODING]: senderOptions.encoding,
				...senderOptions.telemetryHeaders(),
				...(senderOptions.params !== undefined
					? {
							[HEADER_CONN_PARAMS]: JSON.stringify(
								senderOptions.params,
							),
						}
					: {}),
			},
			body: { body, wait, timeout },
			encoding: senderOptions.encoding,
			customFetch: senderOptions.customFetch,
			signal: options?.signal,
			requestVersion: CLIENT_PROTOCOL_CURRENT_VERSION,
			requestVersionedDataHandler: HTTP_QUEUE_SEND_REQUEST_VERSIONED,
			responseVersion: CLIENT_PROTOCOL_CURRENT_VERSION,
			responseVersionedDataHandler: HTTP_QUEUE_SEND_RESPONSE_VERSIONED,
			requestZodSchema: HttpQueueSendRequestSchema,
			responseZodSchema: HttpQueueSendResponseSchema,
			requestToJson: (value): HttpQueueSendRequestJson => ({
				...value,
				name,
			}),
			requestToBare: (value): protocol.HttpQueueSendRequest => ({
				name: value.name ?? name,
				body: bufferToArrayBuffer(
					encodeCborCompat(value.body as JsonCompatValue),
				),
				wait: value.wait ?? false,
				timeout:
					value.timeout !== undefined ? BigInt(value.timeout) : null,
			}),
			responseFromJson: (json): QueueSendResult => {
				if (json.response === undefined) {
					return { status: json.status as "completed" | "timedOut" };
				}
				return {
					status: json.status as "completed" | "timedOut",
					response: json.response,
				};
			},
			responseFromBare: (bare): QueueSendResult => {
				if (bare.response === null || bare.response === undefined) {
					return { status: bare.status as "completed" | "timedOut" };
				}
				return {
					status: bare.status as "completed" | "timedOut",
					response: decodeCborCompat(new Uint8Array(bare.response)),
				};
			},
		});

		if (wait) {
			return result;
		}
		return;
	}

	return {
		send,
	};
}
