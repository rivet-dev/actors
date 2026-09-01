import {
	HEADER_RIVET_RAY_ID,
	HEADER_TRACEPARENT,
	HEADER_TRACESTATE,
} from "@/common/actor-router-consts";
import type { ActorInvocationTraceContext } from "@/common/actor-telemetry-context";
import {
	actorInvocationTraceHeaders,
	readActiveRayId,
	readActiveTraceHeaders,
} from "@/common/otel-context";

/**
 * Headers that carry a caller's ray ID and trace context into an actor. One
 * rule for every kind of outbound call, so an action call and a queue send
 * made from the same place land in the same trace under the same ray ID.
 *
 * The ray ID is the calling invocation's when the caller is itself inside an
 * actor, else the one placed in OpenTelemetry baggage by the surrounding
 * request handler. The trace context is the application span active in this
 * JavaScript context, else the calling actor's own Core invocation span.
 */
export function outboundTelemetryHeaders(
	invocation: ActorInvocationTraceContext | undefined,
): Record<string, string> {
	const headers: Record<string, string> = {};
	const rayId = invocation?.rayId ?? readActiveRayId();
	if (rayId) {
		headers[HEADER_RIVET_RAY_ID] = rayId;
	}
	const traceHeaders =
		readActiveTraceHeaders() ??
		actorInvocationTraceHeaders(invocation?.span);
	if (traceHeaders) {
		headers[HEADER_TRACEPARENT] = traceHeaders.traceparent;
		if (traceHeaders.tracestate) {
			headers[HEADER_TRACESTATE] = traceHeaders.tracestate;
		}
	}
	return headers;
}

/**
 * Adds outbound telemetry headers to a request whose caller may have set
 * some already. A header the caller set wins. `traceparent` and `tracestate`
 * describe one span between them, so when the caller set either, both stay
 * as the caller set them and neither is added.
 */
export function addOutboundTelemetryHeaders(
	headers: Headers,
	telemetry: Record<string, string>,
): void {
	const callerSetTraceContext =
		headers.has(HEADER_TRACEPARENT) || headers.has(HEADER_TRACESTATE);
	for (const [name, value] of Object.entries(telemetry)) {
		const isTraceContext =
			name === HEADER_TRACEPARENT || name === HEADER_TRACESTATE;
		if (isTraceContext ? callerSetTraceContext : headers.has(name)) {
			continue;
		}
		headers.set(name, value);
	}
}
