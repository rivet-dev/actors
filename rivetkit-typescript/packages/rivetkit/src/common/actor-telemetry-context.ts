/** Correlation owned by the currently executing Core actor invocation. */
export interface ActorInvocationTraceContext {
	/** Rivet request correlation identifier for the current invocation. */
	readonly rayId: string;
	/** Core-owned invocation span context, absent when tracing is disabled. */
	readonly span?: ActorInvocationSpanContext;
}

/** W3C span context of the Core invocation span. */
export interface ActorInvocationSpanContext {
	/** W3C trace identifier. */
	readonly traceId: string;
	/** W3C span identifier for the Core invocation. */
	readonly spanId: string;
	/** OpenTelemetry trace flags encoded as an integer. */
	readonly traceFlags: number;
	/** Serialized W3C Trace Context for the Core invocation. */
	readonly traceparent: string;
	/** Optional vendor trace state inherited by the Core invocation. */
	readonly tracestate?: string;
}

/** Formats a W3C `traceparent` header from its span identifiers. */
export function formatTraceparent(
	traceId: string,
	spanId: string,
	traceFlags: number,
): string {
	return `00-${traceId}-${spanId}-${traceFlags.toString(16).padStart(2, "0")}`;
}
