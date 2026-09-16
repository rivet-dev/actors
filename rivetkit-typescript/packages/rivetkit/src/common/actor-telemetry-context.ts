/** Correlation owned by the currently executing Core actor invocation. */
export interface ActorInvocationTraceContext {
	/** Ray ID the invocation received, absent when nothing upstream supplied one. */
	readonly rayId?: string;
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
	/** Optional vendor trace state inherited by the Core invocation. */
	readonly tracestate?: string;
}
