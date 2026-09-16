import { SpanStatusCode, trace } from "@opentelemetry/api";
import { actor } from "rivetkit";
import { db } from "rivetkit/db";

const tracer = trace.getTracer("counter");

export const counter = actor({
	state: {},
	db: db(),
	actions: {
		query: async (c) =>
			tracer.startActiveSpan("counter.query", async (span) => {
				try {
					return await c.db.execute("SELECT 1 AS value");
				} catch (error) {
					span.setStatus({ code: SpanStatusCode.ERROR });
					throw error;
				} finally {
					span.end();
				}
			}),
	},
});
