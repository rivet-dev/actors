import {
	type Context,
	context,
	createTraceState,
	isSpanContextValid,
	trace,
} from "@opentelemetry/api";
import type { RuntimeActorInvocationSpanContext } from "./runtime";

/**
 * Runs `run` with the Core invocation span as the active OpenTelemetry span,
 * so application spans started inside an actor callback nest under it. With
 * no span, or an invalid one, `run` executes unchanged.
 */
export function runWithActorInvocationSpan<T>(
	invocation: RuntimeActorInvocationSpanContext | undefined,
	run: () => T,
): T {
	if (!invocation) return run();

	let parent: Context;
	try {
		const spanContext = {
			traceId: invocation.traceId,
			spanId: invocation.spanId,
			traceFlags: invocation.traceFlags,
			traceState: invocation.tracestate
				? createTraceState(invocation.tracestate)
				: undefined,
			isRemote: false,
		};
		if (!isSpanContextValid(spanContext)) return run();
		parent = trace.setSpanContext(context.active(), spanContext);
	} catch {
		// Invalid telemetry must not prevent the action from running.
		return run();
	}

	return context.with(parent, run);
}
