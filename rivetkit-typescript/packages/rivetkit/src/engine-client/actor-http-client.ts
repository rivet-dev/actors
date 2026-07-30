import type { ClientConfig } from "@/client/config";
import {
	HEADER_RIVET_ACTOR,
	HEADER_RIVET_SKIP_READY_WAIT,
	HEADER_RIVET_TARGET,
	HEADER_RIVET_TOKEN,
} from "@/common/actor-router-consts";
import { type GatewayRequestOptions, shouldSkipReadyWait } from "./driver";

export interface HttpGatewayRequestOptions extends GatewayRequestOptions {
	directActorId?: string;
}

export async function sendHttpRequestToGateway(
	runConfig: ClientConfig,
	gatewayUrl: string,
	actorRequest: Request,
	options: HttpGatewayRequestOptions = {},
): Promise<Response> {
	let bodyToSend: ReadableStream<Uint8Array> | null = null;
	const guardHeaders = buildGuardHeaders(runConfig, actorRequest, options);

	if (actorRequest.method !== "GET" && actorRequest.method !== "HEAD") {
		if (actorRequest.bodyUsed) {
			throw new Error("Request body has already been consumed");
		}

		if (actorRequest.body) {
			bodyToSend = actorRequest.body;
			guardHeaders.delete("transfer-encoding");
			guardHeaders.delete("content-length");
		}
	}

	return fetch(gatewayUrl, {
		method: actorRequest.method,
		headers: guardHeaders,
		body: bodyToSend,
		signal: actorRequest.signal,
		...(bodyToSend ? { duplex: "half" } : {}),
	} as RequestInit);
}

function buildGuardHeaders(
	runConfig: ClientConfig,
	actorRequest: Request,
	options: HttpGatewayRequestOptions,
): Headers {
	const headers = new Headers();
	// Copy all headers from the original request
	actorRequest.headers.forEach((value, key) => {
		headers.set(key, value);
	});
	// Add extra headers from config
	for (const [key, value] of Object.entries(runConfig.headers)) {
		headers.set(key, value as string);
	}
	// Add guard-specific headers
	if (runConfig.token) {
		headers.set(HEADER_RIVET_TOKEN, runConfig.token);
	}
	if (options.directActorId !== undefined) {
		headers.set(HEADER_RIVET_TARGET, "actor");
		headers.set(HEADER_RIVET_ACTOR, options.directActorId);
	}
	if (shouldSkipReadyWait(options)) {
		headers.set(HEADER_RIVET_SKIP_READY_WAIT, "1");
	}
	return headers;
}
