import {
	type Context,
	context,
	createTraceState,
	isSpanContextValid,
	type SpanContext,
	trace,
} from "@opentelemetry/api";
import type { ActorInvocationSpanContext } from "./actor-telemetry-context";

export interface ActiveTraceHeaders {
	readonly traceparent: string;
	readonly tracestate?: string;
}

/** W3C headers for the Core invocation span, or nothing when it is absent or invalid. */
export function actorInvocationTraceHeaders(
	invocation: ActorInvocationSpanContext | undefined,
): ActiveTraceHeaders | undefined {
	const spanContext = invocationSpanContext(invocation);
	if (!spanContext) return undefined;
	const tracestate = spanContext.traceState?.serialize();
	return {
		traceparent: `00-${spanContext.traceId}-${spanContext.spanId}-${spanContext.traceFlags.toString(16).padStart(2, "0")}`,
		...(tracestate ? { tracestate } : {}),
	};
}

/**
 * Runs `run` with the Core invocation span as the active OpenTelemetry span,
 * so application spans started inside an actor callback nest under it. With
 * no span, or an invalid one, `run` executes unchanged.
 */
export function runWithActorInvocationSpan<T>(
	invocation: ActorInvocationSpanContext | undefined,
	run: () => T,
): T {
	if (!invocation) return run();

	const spanContext = invocationSpanContext(invocation);
	if (!spanContext) return run();
	const parent: Context = trace.setSpanContext(context.active(), spanContext);

	return context.with(parent, run);
}

function invocationSpanContext(
	invocation: ActorInvocationSpanContext | undefined,
): SpanContext | undefined {
	if (!invocation) return undefined;
	try {
		const spanContext: SpanContext = {
			traceId: invocation.traceId,
			spanId: invocation.spanId,
			traceFlags: invocation.traceFlags,
			traceState: invocation.tracestate
				? createTraceState(invocation.tracestate)
				: undefined,
			isRemote: false,
		};
		return isSpanContextValid(spanContext) ? spanContext : undefined;
	} catch {
		return undefined;
	}
}
