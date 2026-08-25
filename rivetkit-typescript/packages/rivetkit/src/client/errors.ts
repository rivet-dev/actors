import {
	type ActorSpecifier,
	INTERNAL_ERROR_CODE,
	RivetError,
	type RivetErrorLike,
	UserError,
} from "@/actor/errors";

export class ActorClientError extends Error {}

export class ManagerError extends ActorClientError {
	constructor(error: string, opts?: ErrorOptions) {
		super(`Manager error: ${error}`, opts);
	}
}

export class MalformedResponseMessage extends ActorClientError {
	constructor(cause?: unknown) {
		super(`Malformed response message: ${cause}`, { cause });
	}
}

export { RivetError, RivetError as ActorError, UserError };
export type ActorSchedulingError = RivetError;
export type { ActorSpecifier, RivetErrorLike };

export class HttpRequestError extends ActorClientError {
	constructor(message: string, opts?: { cause?: unknown }) {
		super(`HTTP request error: ${message}`, { cause: opts?.cause });
	}
}

export class ActorConnDisposed extends ActorClientError {
	constructor() {
		super("Attempting to interact with a disposed actor connection.");
	}
}

/**
 * Checks if an error code indicates a scheduling error that may have more details.
 */
export function isSchedulingError(group: string, code: string): boolean {
	return (
		group === "guard" &&
		(code === "actor_ready_timeout" || code === "actor_runner_failed")
	);
}

export function actorSchedulingError(
	group: string,
	code: string,
	actorId: string,
	details: unknown,
	rayId?: string,
): RivetError {
	return new RivetError(
		group,
		code,
		`Actor failed to start (${actorId}): ${JSON.stringify(details)}`,
		{ metadata: { actorId, details }, rayId },
	);
}

export function internalClientError(
	message: string,
	opts?: ErrorOptions,
): RivetError {
	return new RivetError("rivetkit", INTERNAL_ERROR_CODE, message, {
		cause: opts?.cause,
	});
}
