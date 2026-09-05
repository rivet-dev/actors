import { context, isSpanContextValid, trace } from "@opentelemetry/api";
import { formatTraceparent } from "@/common/actor-telemetry-context";

/** W3C headers derived from the active JavaScript OTel context. */
export interface ActiveTraceHeaders {
	/** W3C Trace Context identifying the active trace and span. */
	readonly traceparent: string;
	/** Optional vendor trace state associated with the active span. */
	readonly tracestate?: string;
}

/** Returns the active W3C trace context, when an OTel provider has installed one. */
export function readActiveTraceHeaders(): ActiveTraceHeaders | undefined {
	const spanContext = trace.getSpanContext(context.active());
	if (!spanContext || !isSpanContextValid(spanContext)) return undefined;

	const tracestate = spanContext.traceState?.serialize();
	return {
		traceparent: formatTraceparent(
			spanContext.traceId,
			spanContext.spanId,
			spanContext.traceFlags,
		),
		...(tracestate ? { tracestate } : {}),
	};
}
