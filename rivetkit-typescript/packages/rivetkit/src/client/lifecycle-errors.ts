import { ActorError, HttpRequestError } from "./errors";

export interface LifecycleBoundaryInfo {
	kind: "request_retry" | "reconnect_only";
	source: "actor_error" | "transport_error";
	group?: string;
	code?: string;
	message: string;
	legacy: boolean;
}

function* walkErrorChain(
	error: unknown,
	maxDepth = 8,
): Generator<unknown, void, undefined> {
	let current = error;
	let depth = 0;

	while (current !== undefined && current !== null && depth < maxDepth) {
		yield current;

		if (
			typeof current === "object" &&
			"cause" in current &&
			(current as { cause?: unknown }).cause !== current
		) {
			current = (current as { cause?: unknown }).cause;
			depth += 1;
			continue;
		}

		break;
	}
}

function buildLifecycleBoundaryInfo(
	kind: LifecycleBoundaryInfo["kind"],
	source: LifecycleBoundaryInfo["source"],
	message: string,
	opts?: {
		group?: string;
		code?: string;
		legacy?: boolean;
	},
): LifecycleBoundaryInfo {
	return {
		kind,
		source,
		group: opts?.group,
		code: opts?.code,
		message,
		legacy: opts?.legacy ?? false,
	};
}

function classifyActorError(
	error: ActorError,
): LifecycleBoundaryInfo | undefined {
	if (
		error.group === "actor" &&
		error.code === "stopping" &&
		error.message.includes("database accessed after actor stopped")
	) {
		return undefined;
	}

	if (error.group === "actor" && error.code === "restarting") {
		return buildLifecycleBoundaryInfo(
			"request_retry",
			"actor_error",
			error.message,
			{
				group: error.group,
				code: error.code,
			},
		);
	}

	if (
		error.group === "guard" &&
		isRetryableGuardGatewayHttpError(error.code)
	) {
		return buildLifecycleBoundaryInfo(
			"request_retry",
			"actor_error",
			error.message,
			{
				group: error.group,
				code: error.code,
			},
		);
	}

	// TODO(RVT-6193): Remove this legacy match after structured restart errors
	// are authoritative everywhere.
	if (
		error.group === "actor" &&
		error.code === "internal_error" &&
		error.message === "Actor is stopping"
	) {
		return buildLifecycleBoundaryInfo(
			"request_retry",
			"actor_error",
			error.message,
			{
				group: error.group,
				code: error.code,
				legacy: true,
			},
		);
	}

	// TODO(RVT-6193): Remove this legacy match after connection admission uses
	// actor.restarting consistently.
	if (
		error.group === "actor" &&
		error.code === "stopping" &&
		error.message ===
			"Actor stopping: Cannot accept new connections while actor is stopping"
	) {
		return buildLifecycleBoundaryInfo(
			"request_retry",
			"actor_error",
			error.message,
			{
				group: error.group,
				code: error.code,
				legacy: true,
			},
		);
	}

	if (error.group === "actor" && error.code === "stopped") {
		return buildLifecycleBoundaryInfo(
			"reconnect_only",
			"actor_error",
			error.message,
			{
				group: error.group,
				code: error.code,
				legacy: true,
			},
		);
	}

	if (error.group === "ws" && error.code === "going_away") {
		return buildLifecycleBoundaryInfo(
			"reconnect_only",
			"actor_error",
			error.message,
			{
				group: error.group,
				code: error.code,
				legacy: true,
			},
		);
	}

	if (error.group === "client" && error.code === "get_params_failed") {
		return buildLifecycleBoundaryInfo(
			"reconnect_only",
			"actor_error",
			error.message,
			{
				group: error.group,
				code: error.code,
			},
		);
	}

	return undefined;
}

function isRetryableGuardGatewayHttpError(code: string): boolean {
	return (
		code === "service_unavailable" ||
		code === "actor_wake_retries_exceeded" ||
		code === "actor_stopped_while_waiting" ||
		code === "tunnel_request_aborted" ||
		code === "tunnel_message_timeout" ||
		code === "tunnel_response_closed" ||
		code === "gateway_response_start_timeout"
	);
}

function classifyTransportError(
	error: Error,
): LifecycleBoundaryInfo | undefined {
	if (error.message.includes("database accessed after actor stopped")) {
		return undefined;
	}

	// TODO(RVT-6193): Remove this exact string match after the runner surfaces
	// structured restart errors end to end.
	if (/^Actor [A-Za-z0-9-]+ stopped$/.test(error.message)) {
		return buildLifecycleBoundaryInfo(
			"request_retry",
			"transport_error",
			error.message,
			{ legacy: true },
		);
	}

	// TODO(RVT-6193): Remove these exact string matches after transport
	// shutdown paths are normalized to structured lifecycle errors.
	if (
		error.message === "WebSocket connection closed during shutdown" ||
		error.message === "envoy shut down" ||
		error.message === "envoy shutting down"
	) {
		return buildLifecycleBoundaryInfo(
			"reconnect_only",
			"transport_error",
			error.message,
			{ legacy: true },
		);
	}

	return undefined;
}

export function classifyLifecycleBoundaryError(
	error: unknown,
): LifecycleBoundaryInfo | undefined {
	for (const current of walkErrorChain(error)) {
		if (current instanceof ActorError) {
			const classified = classifyActorError(current);
			if (classified) {
				return classified;
			}
			continue;
		}

		if (current instanceof HttpRequestError || current instanceof Error) {
			const classified = classifyTransportError(current);
			if (classified) {
				return classified;
			}
		}
	}

	return undefined;
}

export function isRetryableLifecycleRequestError(error: unknown): boolean {
	return classifyLifecycleBoundaryError(error)?.kind === "request_retry";
}

export function isRetryableLifecycleReconnectSignal(error: unknown): boolean {
	const classified = classifyLifecycleBoundaryError(error);
	return (
		classified?.kind === "reconnect_only" ||
		classified?.kind === "request_retry"
	);
}

function throwIfAborted(signal?: AbortSignal) {
	if (signal?.aborted) {
		throw signal.reason ?? new Error("Operation aborted");
	}
}

async function waitWithSignal(ms: number, signal?: AbortSignal) {
	throwIfAborted(signal);

	await new Promise<void>((resolve, reject) => {
		const timeout = setTimeout(() => {
			cleanup();
			resolve();
		}, ms);

		const onAbort = () => {
			clearTimeout(timeout);
			cleanup();
			reject(signal?.reason ?? new Error("Operation aborted"));
		};

		const cleanup = () => {
			signal?.removeEventListener("abort", onAbort);
		};

		signal?.addEventListener("abort", onAbort, { once: true });
	});
}

export async function retryOnLifecycleBoundary<T>(
	run: () => Promise<T>,
	opts?: {
		maxAttempts?: number;
		initialDelayMs?: number;
		maxDelayMs?: number;
		signal?: AbortSignal;
	},
): Promise<T> {
	const maxAttempts = opts?.maxAttempts ?? 5;
	const initialDelayMs = opts?.initialDelayMs ?? 25;
	const maxDelayMs = opts?.maxDelayMs ?? 200;

	let lastError: unknown;
	for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
		throwIfAborted(opts?.signal);

		try {
			return await run();
		} catch (error) {
			if (!isRetryableLifecycleRequestError(error)) {
				throw error;
			}

			lastError = error;
			if (attempt === maxAttempts - 1) {
				break;
			}

			const delayMs = Math.min(initialDelayMs * 2 ** attempt, maxDelayMs);
			await waitWithSignal(delayMs, opts?.signal);
		}
	}

	throw lastError;
}
