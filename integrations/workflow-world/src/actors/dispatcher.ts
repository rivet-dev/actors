import { MessageId, QueuePayloadSchema, parseQueueName } from "@workflow/world";
import type { Ctx } from "./shared.js";

type DispatchRow = {
	message_id: string;
	queue_name: string;
	route: "flow" | "step";
	runtime_url: string;
	body: string;
	headers: Record<string, string>;
	attempt: number;
};

type QueueSnapshotRow = {
	messageId: string;
	queueName: string;
	route: string;
	status: string;
	attempt: number;
	nextAt: number;
	startedAt: number | null;
	finishedAt: number | null;
	httpStatus: number | null;
	error: string | null;
};

export type DispatcherVars = {
	/** Per-actor-instance latch: at most one drain loop runs at a time. */
	draining: boolean;
	/** Monotonic signal used to close the enqueue-vs-idle lost-wakeup window. */
	requestedGeneration: number;
	/** Resolves when newly committed work should wake the active drain loop. */
	drainSignal: DispatchSignal;
	/** Message IDs currently being delivered by this actor process. */
	activeMessageIds: Set<string>;
	/** Earliest currently-armed `wake` schedule (ms epoch), or null if none. */
	wakeArmedAt: number | null;
	/** Serializes mutating actor actions with scheduled wake delivery. */
	serialTail: Promise<void>;
};

type DispatchSignal = {
	promise: Promise<void>;
	resolve: () => void;
};

function createDispatchSignal(): DispatchSignal {
	let resolve!: () => void;
	const promise = new Promise<void>((resolvePromise) => {
		resolve = resolvePromise;
	});
	return { promise, resolve };
}

function signalDispatch(vars: DispatcherVars) {
	vars.requestedGeneration++;
	vars.drainSignal.resolve();
	vars.drainSignal = createDispatchSignal();
}

export type DispatchContext = Ctx & {
	key: readonly string[];
	vars: DispatcherVars;
	keepAwake<T>(promise: Promise<T>): Promise<T>;
	schedule: {
		after(
			delayMs: number,
			actionName: string,
			...args: unknown[]
		): Promise<unknown>;
		at(timestampMs: number, actionName: string, ...args: unknown[]): Promise<unknown>;
	};
	armNextWake?: () => Promise<void>;
};

function assertPartitionKey(
	c: { key: readonly string[] },
	partition: string,
) {
	const actorPartition = c.key[0];
	if (!actorPartition || actorPartition !== partition) {
		throw new Error(
			`workflowRun actor key mismatch: expected ${actorPartition ?? "<missing>"}, received ${partition}`,
		);
	}
}

function envInteger(name: string, fallback: number, min: number) {
	const value = Number(process.env[name]);
	if (!Number.isFinite(value)) return fallback;
	return Math.max(min, Math.floor(value));
}

const MAX_DISPATCH_ATTEMPTS = envInteger(
	"RIVET_WORLD_RIVET_MAX_DISPATCH_ATTEMPTS",
	256,
	1,
);
const RETRY_DELAY_MS = envInteger(
	"RIVET_WORLD_RIVET_RETRY_DELAY_MS",
	5_000,
	0,
);
export const STALE_INFLIGHT_MS = envInteger(
	"RIVET_WORLD_RIVET_STALE_INFLIGHT_MS",
	60_000,
	1,
);
const configuredDispatchTimeoutMs = Number(
	process.env.RIVET_WORLD_RIVET_DISPATCH_TIMEOUT_MS,
);
const DISPATCH_TIMEOUT_MS = Number.isFinite(configuredDispatchTimeoutMs)
	? Math.max(1, Math.floor(configuredDispatchTimeoutMs))
	: null;
const MAX_TIMER_MS = 2_147_483_647;
const MAX_CONCURRENT_DISPATCHES = 32;

// Field names that must never reach the log (defense-in-depth — the call sites
// already avoid passing the bearer secret, but redact by key in case a future
// call site passes headers/tokens through).
const REDACTED_LOG_KEYS = new Set([
	"authorization",
	"secret",
	"token",
	"headers",
	"password",
]);

/**
 * Structured, single-line JSON logging gated behind RIVET_WORLD_RIVET_DEBUG. A
 * trace id (message id, falling back to partition/run id) ties together all log
 * lines for one dispatch so a run can be followed end to end. Sensitive keys are
 * redacted so the `.well-known` bearer secret can never be logged.
 */
export function debugLog(event: string, data?: Record<string, unknown>) {
	if (process.env.RIVET_WORLD_RIVET_DEBUG !== "1") return;
	const safe: Record<string, unknown> = {};
	for (const [key, value] of Object.entries(data ?? {})) {
		safe[key] = REDACTED_LOG_KEYS.has(key.toLowerCase()) ? "[redacted]" : value;
	}
	const traceId = data?.messageId ?? data?.partition ?? data?.runId;
	console.error(
		JSON.stringify({
			scope: "@rivet-dev/workflow-world",
			event,
			...(traceId == null ? {} : { traceId }),
			...safe,
		}),
	);
}

function normalizeRuntimeUrl(value: string) {
	return value.replace(/\/+$/, "");
}

function queueRoute(queueName: string): "flow" | "step" {
	return parseQueueName(queueName).kind === "workflow" ? "flow" : "step";
}

export async function claimNextDispatch(c: Ctx): Promise<DispatchRow | null> {
	const now = Date.now();
	const rows = await c.db.execute(
		`
		UPDATE dispatcher_queue
		SET status = 'inflight',
			attempt = attempt + 1,
			started_at = ?,
			finished_at = NULL,
			http_status = NULL,
			error = NULL,
			updated_at = ?
		WHERE message_id = (
			SELECT message_id
			FROM dispatcher_queue
			WHERE status = 'ready' AND next_at <= ?
			ORDER BY created_at ASC
			LIMIT 1
		)
		RETURNING message_id, queue_name, route, runtime_url, body, headers, attempt
		`,
		now,
		now,
		now,
	);
	const row = rows[0];
	if (!row) return null;
	debugLog("dispatcher.claim", {
		messageId: row.message_id,
		queueName: row.queue_name,
		attempt: row.attempt,
	});
	return {
		message_id: String(row.message_id),
		queue_name: String(row.queue_name),
		route: row.route === "step" ? "step" : "flow",
		runtime_url: String(row.runtime_url),
		body: String(row.body),
		headers:
			row.headers == null
				? {}
				: (JSON.parse(String(row.headers)) as Record<string, string>),
		attempt: Number(row.attempt),
	};
}

export async function reclaimStaleInflight(
	c: Ctx,
	locallyActiveMessageIds: ReadonlySet<string> = new Set(),
) {
	const now = Date.now();
	const locallyActive = [...locallyActiveMessageIds];
	const excludeLocallyActive =
		locallyActive.length === 0
			? ""
			: `AND message_id NOT IN (${locallyActive.map(() => "?").join(", ")})`;
	// Transient reclaim: the row is going back to 'ready' for redelivery, so it
	// is NOT finished — leave finished_at NULL and don't stamp a terminal error.
	await c.db.execute(
		`
		UPDATE dispatcher_queue
		SET status = 'ready',
			next_at = ?,
			updated_at = ?
		WHERE status = 'inflight'
			AND started_at IS NOT NULL
			AND started_at <= ?
			AND attempt < ?
			${excludeLocallyActive}
		`,
		now,
		now,
		now - STALE_INFLIGHT_MS,
		MAX_DISPATCH_ATTEMPTS,
		...locallyActive,
	);
	await c.db.execute(
		`
		UPDATE dispatcher_queue
		SET status = 'failed',
			finished_at = ?,
			error = 'stale inflight exceeded max attempts',
			updated_at = ?
		WHERE status = 'inflight'
			AND started_at IS NOT NULL
			AND started_at <= ?
			AND attempt >= ?
			${excludeLocallyActive}
		`,
		now,
		now,
		now - STALE_INFLIGHT_MS,
		MAX_DISPATCH_ATTEMPTS,
		...locallyActive,
	);
}

export async function nextDispatchAt(c: Ctx) {
	const row = (
		await c.db.execute(
			`
			SELECT next_at
			FROM dispatcher_queue
			WHERE status = 'ready'
			ORDER BY next_at ASC
			LIMIT 1
			`,
		)
	)[0];
	return row?.next_at == null ? null : Number(row.next_at);
}

export async function nextInflightRecoveryAt(
	c: Ctx,
	locallyActiveMessageIds: ReadonlySet<string> = new Set(),
) {
	const locallyActive = [...locallyActiveMessageIds];
	const excludeLocallyActive =
		locallyActive.length === 0
			? ""
			: `AND message_id NOT IN (${locallyActive.map(() => "?").join(", ")})`;
	const row = (
		await c.db.execute(
			`
			SELECT MIN(started_at) AS started_at
			FROM dispatcher_queue
			WHERE status = 'inflight' AND started_at IS NOT NULL
				${excludeLocallyActive}
			`,
			...locallyActive,
		)
	)[0];
	const persistedRecoveryAt = row?.started_at == null
		? null
		: Number(row.started_at) + STALE_INFLIGHT_MS;
	// Keep a durable recovery alarm while a request is active, but move it into
	// the future after each check so a legitimate long request cannot hot-loop
	// immediate actor wakes. After a crash this in-memory set disappears, and the
	// next alarm reclaims the persisted inflight row at its original stale time.
	const localRecoveryAt = locallyActive.length > 0
		? Date.now() + STALE_INFLIGHT_MS
		: null;
	return persistedRecoveryAt == null
		? localRecoveryAt
		: localRecoveryAt == null
			? persistedRecoveryAt
			: Math.min(persistedRecoveryAt, localRecoveryAt);
}

export async function markDispatchDone(
	c: Ctx,
	messageId: string,
	attempt: number,
	status: number,
) {
	await c.db.execute(
		`
		UPDATE dispatcher_queue
		SET status = 'done',
			finished_at = ?,
			http_status = ?,
			error = NULL,
			updated_at = ?
		WHERE message_id = ? AND status = 'inflight' AND attempt = ?
		`,
		Date.now(),
		status,
		Date.now(),
		messageId,
		attempt,
	);
}

export async function rescheduleDispatch(
	c: Ctx,
	messageId: string,
	attempt: number,
	delayMs: number,
	status: number | null,
	error: string | null,
) {
	const now = Date.now();
	await c.db.execute(
		`
		UPDATE dispatcher_queue
		SET status = 'ready',
			next_at = ?,
			finished_at = ?,
			http_status = ?,
			error = ?,
			updated_at = ?
		WHERE message_id = ? AND status = 'inflight' AND attempt = ?
		`,
		now + Math.max(0, delayMs),
		now,
		status,
		error,
		now,
		messageId,
		attempt,
	);
}

export async function failDispatch(
	c: Ctx,
	messageId: string,
	attempt: number,
	status: number | null,
	error: string,
) {
	const now = Date.now();
	await c.db.execute(
		`
		UPDATE dispatcher_queue
		SET status = 'failed',
			finished_at = ?,
			http_status = ?,
			error = ?,
			updated_at = ?
		WHERE message_id = ? AND status = 'inflight' AND attempt = ?
		`,
		now,
		status,
		error,
		now,
		messageId,
		attempt,
	);
}

export async function inspectQueue(c: Ctx) {
	const countRows = await c.db.execute(
		`
		SELECT status, COUNT(*) AS count
		FROM dispatcher_queue
		GROUP BY status
		`,
	);
	const rowRows = await c.db.execute(
		`
		SELECT message_id, queue_name, route, status, attempt, next_at,
			started_at, finished_at, http_status, error
		FROM dispatcher_queue
		ORDER BY created_at ASC
		LIMIT 100
		`,
	);
	const counts: Record<string, number> = {};
	for (const row of countRows) {
		counts[String(row.status)] = Number(row.count);
	}
	const rows: QueueSnapshotRow[] = rowRows.map((row) => ({
		messageId: String(row.message_id),
		queueName: String(row.queue_name),
		route: String(row.route),
		status: String(row.status),
		attempt: Number(row.attempt),
		nextAt: Number(row.next_at),
		startedAt: row.started_at == null ? null : Number(row.started_at),
		finishedAt: row.finished_at == null ? null : Number(row.finished_at),
		httpStatus: row.http_status == null ? null : Number(row.http_status),
		error: row.error == null ? null : String(row.error),
	}));
	return {
		counts,
		rows,
		maxAttempts: MAX_DISPATCH_ATTEMPTS,
		retryDelayMs: RETRY_DELAY_MS,
		staleInflightMs: STALE_INFLIGHT_MS,
	};
}

async function deliverDispatch(
	c: DispatchContext,
	partition: string,
	row: DispatchRow,
) {
	debugLog("dispatcher.deliver", {
		messageId: row.message_id,
		route: row.route,
		runtimeUrl: row.runtime_url,
		attempt: row.attempt,
	});
	const response = await fetch(
		`${normalizeRuntimeUrl(row.runtime_url)}/.well-known/workflow/v1/${row.route}`,
		{
			method: "POST",
			// Workflow handlers may legitimately hold this request for the entire
			// duration of a long-running step. Keep it open by default; operators can
			// opt into a transport timeout when their runtime has a known upper bound.
			...(DISPATCH_TIMEOUT_MS == null
				? {}
				: { signal: AbortSignal.timeout(DISPATCH_TIMEOUT_MS) }),
			duplex: "half",
			headers: {
				...row.headers,
				"content-type": "application/json",
				"x-vqs-queue-name": row.queue_name,
				"x-vqs-message-id": row.message_id,
				"x-vqs-message-attempt": String(row.attempt),
			},
			body: row.body,
		} as RequestInit,
	);
	const text = await response.text();
	debugLog("dispatcher.response", {
		messageId: row.message_id,
		status: response.status,
		body: text.slice(0, 500),
	});
	if (response.ok) {
		let parsed: unknown;
		let parseOk = false;
		try {
			parsed = JSON.parse(text);
			parseOk = true;
		} catch {}
		if (
			parseOk &&
			parsed != null &&
			typeof parsed === "object" &&
			"timeoutSeconds" in parsed
		) {
			const timeoutSeconds = Number(
				(parsed as { timeoutSeconds: unknown }).timeoutSeconds,
			);
			if (Number.isFinite(timeoutSeconds) && timeoutSeconds >= 0) {
				const delayMs = Math.min(timeoutSeconds * 1000, MAX_TIMER_MS);
				await rescheduleDispatch(
					c,
					row.message_id,
					row.attempt,
					delayMs,
					response.status,
					null,
				);
				await scheduleDispatchWake(c, partition, delayMs);
				return;
			}
			// A `timeoutSeconds` field that isn't a valid delay is a malformed
			// runtime response — retry rather than silently acking the message.
			if (row.attempt >= MAX_DISPATCH_ATTEMPTS) {
				await failDispatch(
					c,
					row.message_id,
					row.attempt,
					response.status,
					`malformed timeoutSeconds: ${text.slice(0, 200)}`,
				);
				return;
			}
			await rescheduleDispatch(
				c,
				row.message_id,
				row.attempt,
				RETRY_DELAY_MS,
				response.status,
				`malformed timeoutSeconds: ${text.slice(0, 200)}`,
			);
			await scheduleDispatchWake(c, partition, RETRY_DELAY_MS);
			return;
		}
		// A 2xx means the runtime accepted this delivery, but a crash before this
		// durable update causes redelivery. This is intentional at-least-once
		// delivery: Workflow requires the same message id across attempts for
		// replay ownership recovery. It does not promise exactly-once external
		// side effects; steps must use their stable step id as an idempotency key.
		await markDispatchDone(c, row.message_id, row.attempt, response.status);
		return;
	}

	if (row.attempt >= MAX_DISPATCH_ATTEMPTS) {
		await failDispatch(
			c,
			row.message_id,
			row.attempt,
			response.status,
			`HTTP ${response.status}: ${text}`,
		);
		return;
	}
	await rescheduleDispatch(
		c,
		row.message_id,
		row.attempt,
		RETRY_DELAY_MS,
		response.status,
		`HTTP ${response.status}: ${text}`,
	);
	await scheduleDispatchWake(c, partition, RETRY_DELAY_MS);
}

async function scheduleDispatchWake(
	c: DispatchContext,
	partition: string,
	delayMs: number,
) {
	if (c.armNextWake) {
		await c.armNextWake();
		return;
	}
	const clampedDelayMs = Math.min(Math.max(0, delayMs), MAX_TIMER_MS);
	debugLog("dispatcher.schedule", { partition, delayMs: clampedDelayMs });
	if (clampedDelayMs === 0) {
		startDispatchLoop(c, partition);
		return;
	}
	// Coalesce superseded wakes: `c.schedule` has no cancel, so we avoid arming a
	// redundant later-or-equal wake when one is already pending (it will re-scan
	// and re-arm). This bounds schedule growth.
	const target = Date.now() + clampedDelayMs;
	if (c.vars.wakeArmedAt != null && c.vars.wakeArmedAt <= target) return;
	await c.schedule.after(clampedDelayMs, "wake");
	c.vars.wakeArmedAt = target;
}

export async function drainDispatchQueue(c: DispatchContext, partition: string) {
	// NOTE: do not gate this loop on the originating action's `c.aborted`. The
	// loop is launched via `c.keepAwake(...)` from whatever action (enqueue /
	// wake / onWake) first observed work, but it must outlive that action: the
	// action's context aborts as soon as it returns its response, which would
	// otherwise strand any rows enqueued after the first delivery. `keepAwake`
	// keeps the *actor* alive for the promise; genuine actor shutdown surfaces as
	// a thrown db error that unwinds the loop and clears the latch.
	const active = new Set<Promise<void>>();
	while (true) {
		if (active.size >= MAX_CONCURRENT_DISPATCHES) {
			await Promise.race(active);
			continue;
		}
		await reclaimStaleInflight(c, c.vars.activeMessageIds);
		const drainSignal = c.vars.drainSignal.promise;
		const row = await claimNextDispatch(c);
		if (!row) {
			if (active.size > 0) {
				// Workflow turns can enqueue their continuations before the current HTTP
				// delivery returns. Wake on either event so those continuations can run
				// concurrently instead of head-of-line blocking the active turn.
				await Promise.race([drainSignal, Promise.race(active)]);
				continue;
			}
			const [nextReadyAt, nextRecoveryAt] = await Promise.all([
				nextDispatchAt(c),
				nextInflightRecoveryAt(c, c.vars.activeMessageIds),
			]);
			const nextAt =
				nextReadyAt == null
					? nextRecoveryAt
					: nextRecoveryAt == null
						? nextReadyAt
						: Math.min(nextReadyAt, nextRecoveryAt);
			debugLog("dispatcher.loop.idle", { partition, nextAt });
			if (nextAt == null) return;
			if (nextAt <= Date.now()) continue;
			await scheduleDispatchWake(c, partition, nextAt - Date.now());
			return;
		}
		// A claimed row must always have a durable future wake before external I/O.
		// If the process dies during fetch, that wake reclaims the expired claim.
		await scheduleDispatchWake(c, partition, STALE_INFLIGHT_MS);
		let delivery!: Promise<void>;
		c.vars.activeMessageIds.add(row.message_id);
		delivery = (async () => {
			try {
				await deliverDispatch(c, partition, row);
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error);
				if (row.attempt >= MAX_DISPATCH_ATTEMPTS) {
					await failDispatch(c, row.message_id, row.attempt, null, message);
				} else {
					await rescheduleDispatch(
						c,
						row.message_id,
						row.attempt,
						RETRY_DELAY_MS,
						null,
						message,
					);
					await scheduleDispatchWake(c, partition, RETRY_DELAY_MS);
				}
			} finally {
				active.delete(delivery);
				c.vars.activeMessageIds.delete(row.message_id);
			}
		})();
		active.add(delivery);
	}
}

export function startDispatchLoop(c: DispatchContext, partition: string) {
	signalDispatch(c.vars);
	// Single-consumer latch (SPEC §3.3/§5): actions run concurrently per actor
	// (Fact 1a), so concurrent enqueue/wake calls must not each spawn a drain
	// loop — that would create competing claim loops. One loop may keep multiple
	// claimed HTTP deliveries in flight because Workflow continuations can depend
	// on another delivery for the same run. The
	// check-and-set below has no `await` between read and write, so it is atomic
	// across concurrent JS turns; losers no-op and rely on the running loop to
	// pick up the rows they enqueued.
	if (c.vars.draining) {
		debugLog("dispatcher.start.latched", { partition });
		return;
	}
	const launch = () => {
		const generation = c.vars.requestedGeneration;
		c.vars.draining = true;
		debugLog("dispatcher.start", { partition, generation });
		void c
			.keepAwake(
				(async () => {
					try {
						await drainDispatchQueue(c, partition);
					} finally {
						c.vars.draining = false;
						// An enqueue can commit after the drain's final empty query but
						// before the latch clears. A newer generation forces one more pass.
						if (c.vars.requestedGeneration !== generation) launch();
					}
				})(),
			)
			.catch((error) => {
				debugLog("dispatcher.loop.error", {
					partition,
					error: error instanceof Error ? error.message : String(error),
				});
			});
	};
	launch();
}

export const createDispatcherVars = (): DispatcherVars => ({
	draining: false,
	requestedGeneration: 0,
	drainSignal: createDispatchSignal(),
	activeMessageIds: new Set(),
	wakeArmedAt: null,
	serialTail: Promise.resolve(),
});

export async function insertDispatch(
	c: Ctx,
	messageId: string,
	queueName: string,
	runtimeUrl: string,
	body: string,
	headers?: Record<string, string>,
	delaySeconds?: number,
	idempotencyKey?: string,
) {
	MessageId.parse(messageId);
	QueuePayloadSchema.parse(JSON.parse(body));
	const now = Date.now();
	await c.db.execute(
		`INSERT OR IGNORE INTO dispatcher_queue (
			message_id, queue_name, route, runtime_url, body, headers,
			idem_key, status, attempt, next_at, created_at, updated_at
		) VALUES (?, ?, ?, ?, ?, ?, ?, 'ready', 0, ?, ?, ?)`,
		messageId,
		queueName,
		queueRoute(queueName),
		normalizeRuntimeUrl(runtimeUrl),
		body,
		headers == null ? null : JSON.stringify(headers),
		idempotencyKey ?? null,
		now + Math.max(0, delaySeconds ?? 0) * 1000,
		now,
		now,
	);
	const stored = await c.db.execute(
		`SELECT message_id FROM dispatcher_queue
		 WHERE message_id = ? OR (? IS NOT NULL AND idem_key = ?) LIMIT 1`,
		messageId,
		idempotencyKey ?? null,
		idempotencyKey ?? null,
	);
	return { messageId: String(stored[0]?.message_id ?? messageId) };
}

export async function enqueueDispatch(
	c: DispatchContext,
	partition: string,
	messageId: string,
	queueName: string,
	runtimeUrl: string,
	body: string,
	headers?: Record<string, string>,
	delaySeconds?: number,
	idempotencyKey?: string,
) {
	assertPartitionKey(c, partition);
	// Pre-arm before the durable insert. If the process dies after the insert but
	// before the normal drain starts, the actor wake still discovers the row.
	await c.schedule.after(0, "wake");
	const stored = await insertDispatch(
		c,
		messageId,
		queueName,
		runtimeUrl,
		body,
		headers,
		delaySeconds,
		idempotencyKey,
	);
	startDispatchLoop(c, partition);
	return stored;
}

export async function forceStaleInflightForTesting(
	c: DispatchContext,
	partition: string,
	messageId: string,
	ageMs = STALE_INFLIGHT_MS + 1_000,
) {
	assertPartitionKey(c, partition);
	if (process.env.RIVET_WORLD_RIVET_TESTING !== "1") {
		throw new Error("forceStaleInflightForTesting requires test mode");
	}
	const now = Date.now();
	await c.db.execute(
		`UPDATE dispatcher_queue SET status = 'inflight', started_at = ?,
		 finished_at = NULL, updated_at = ? WHERE message_id = ?`,
		now - Math.max(0, ageMs),
		now,
		messageId,
	);
	await reclaimStaleInflight(c);
	startDispatchLoop(c, partition);
}
