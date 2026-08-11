import type { DeconstructedError } from "@/common/utils";

export const INTERNAL_ERROR_CODE = "internal_error";
export const INTERNAL_ERROR_DESCRIPTION = "An internal error occurred";
export type InternalErrorMetadata = Record<string, never>;

export const USER_ERROR_CODE = "user_error";
export const BRIDGE_RIVET_ERROR_PREFIX = "__RIVET_ERROR_JSON__:";

export interface RivetErrorOptions extends ErrorOptions {
	/** Error data can safely be serialized in a response to the client. */
	public?: boolean;
	/** Metadata associated with this error. */
	metadata?: unknown;
	/** Request identifier used to correlate this error with engine logs. */
	rayId?: string;
	/** Explicit HTTP status override for router responses. */
	statusCode?: number;
	/** Actor context associated with this error. */
	actor?: ActorSpecifier;
}

/** Identifies the actor that was handling work when an error was produced. */
export interface ActorSpecifier {
	actorId: string;
	generation: number;
	key?: string;
}

export interface RivetErrorLike {
	__type?: "ActorError" | "RivetError";
	group: string;
	code: string;
	message: string;
	metadata?: unknown;
	rayId?: string;
	public?: boolean;
	statusCode?: number;
	actor?: ActorSpecifier;
}

export interface BridgeRivetErrorPayload extends RivetErrorLike {}

export interface UserErrorOptions extends ErrorOptions {
	/**
	 * Machine readable code for this error. Useful for catching different types of
	 * errors in try-catch.
	 */
	code?: string;
	/**
	 * Additional metadata related to the error. Useful for understanding context
	 * about the error.
	 */
	metadata?: unknown;
}

function looksLikeRivetErrorOptions(
	value: unknown,
): value is RivetErrorOptions {
	return (
		typeof value === "object" &&
		value !== null &&
		("public" in value ||
			"metadata" in value ||
			"rayId" in value ||
			"statusCode" in value ||
			"actor" in value ||
			"cause" in value)
	);
}

function isTypedErrorTag(value: unknown): value is "ActorError" | "RivetError" {
	return value === "ActorError" || value === "RivetError";
}

function errorMessage(error: unknown, fallback = String(error)): string {
	if (
		error &&
		typeof error === "object" &&
		"message" in error &&
		typeof error.message === "string"
	) {
		return error.message;
	}

	return fallback;
}

export function isRivetErrorLike(
	error: unknown,
): error is RivetError | DeconstructedError | RivetErrorLike {
	return (
		typeof error === "object" &&
		error !== null &&
		"group" in error &&
		typeof error.group === "string" &&
		"code" in error &&
		typeof error.code === "string" &&
		"message" in error &&
		typeof error.message === "string" &&
		(!("rayId" in error) ||
			error.rayId === undefined ||
			typeof error.rayId === "string") &&
		(!("__type" in error) || isTypedErrorTag(error.__type))
	);
}

// Matches the `actor`/`aborted` error that core raises when an in-flight queue
// wait is interrupted because the actor is going to sleep. This is the expected
// way a parked `run` handler unwinds during sleep, not a failure.
export function isActorAbortedError(error: unknown): boolean {
	return (
		isRivetErrorLike(error) &&
		error.group === "actor" &&
		error.code === "aborted"
	);
}

function isActorSpecifier(value: unknown): value is ActorSpecifier {
	return (
		typeof value === "object" &&
		value !== null &&
		"actorId" in value &&
		typeof value.actorId === "string" &&
		"generation" in value &&
		typeof value.generation === "number" &&
		(!("key" in value) ||
			value.key === undefined ||
			typeof value.key === "string")
	);
}

export class RivetError extends Error {
	__type = "RivetError" as const;

	public public: boolean;
	public metadata?: unknown;
	public readonly rayId?: string;
	public statusCode: number;
	public actor?: ActorSpecifier;
	public readonly group: string;
	public readonly code: string;

	public static isRivetError(
		error: unknown,
	): error is RivetError | DeconstructedError {
		return isRivetErrorLike(error);
	}

	public static isActorError(
		error: unknown,
	): error is RivetError | DeconstructedError {
		return isRivetErrorLike(error);
	}

	constructor(
		group: string,
		code: string,
		message: string,
		options?: RivetErrorOptions | unknown,
	) {
		const normalized = looksLikeRivetErrorOptions(options)
			? options
			: { metadata: options };

		super(message, { cause: normalized.cause });
		this.name = "RivetError";
		this.group = group;
		this.code = code;
		this.public = normalized.public ?? false;
		this.metadata = normalized.metadata;
		this.rayId = normalized.rayId;
		this.statusCode = normalized.statusCode ?? (this.public ? 400 : 500);
		this.actor = normalized.actor;
	}

	toString() {
		return this.message;
	}
}

export { RivetError as ActorError };

export class UserError extends RivetError {
	constructor(message: string, options?: UserErrorOptions) {
		super("user", options?.code ?? USER_ERROR_CODE, message, {
			public: true,
			metadata: options?.metadata,
			cause: options?.cause,
		});
	}
}

export function toRivetError(
	error: unknown,
	fallback?: Partial<RivetErrorLike>,
): RivetError {
	if (typeof error === "string") {
		const bridged = decodeBridgeRivetError(error);
		if (bridged) {
			return bridged;
		}
	}

	if (error instanceof Error) {
		const bridged = decodeBridgeRivetError(error.message);
		if (bridged) {
			return bridged;
		}
	}

	if (isRivetErrorLike(error)) {
		return new RivetError(error.group, error.code, error.message, {
			public: error.public,
			statusCode: error.statusCode,
			metadata: error.metadata,
			rayId: error.rayId,
			actor: error.actor,
			cause: error instanceof Error ? error.cause : undefined,
		});
	}

	return new RivetError(
		fallback?.group ?? "actor",
		fallback?.code ?? INTERNAL_ERROR_CODE,
		errorMessage(error, fallback?.message ?? "Unknown error"),
		{
			public: fallback?.public,
			statusCode: fallback?.statusCode,
			metadata: fallback?.metadata,
			rayId: fallback?.rayId,
			actor: fallback?.actor,
			cause: error instanceof Error ? error : undefined,
		},
	);
}

export function encodeBridgeRivetError(error: RivetErrorLike): string {
	return `${BRIDGE_RIVET_ERROR_PREFIX}${JSON.stringify({
		group: error.group,
		code: error.code,
		message: error.message,
		metadata: error.metadata,
		rayId: error.rayId,
		public: error.public,
		statusCode: error.statusCode,
		actor: error.actor,
	})}`;
}

export function decodeBridgeRivetErrorPayload(
	value: string,
): BridgeRivetErrorPayload | undefined {
	if (!value.startsWith(BRIDGE_RIVET_ERROR_PREFIX)) {
		return undefined;
	}

	try {
		const payload = JSON.parse(
			value.slice(BRIDGE_RIVET_ERROR_PREFIX.length),
		) as BridgeRivetErrorPayload;
		if (!isRivetErrorLike(payload)) {
			return undefined;
		}
		if (
			payload.actor !== undefined &&
			payload.actor !== null &&
			!isActorSpecifier(payload.actor)
		) {
			return undefined;
		}

		return payload;
	} catch {
		return undefined;
	}
}

export function decodeBridgeRivetError(value: string): RivetError | undefined {
	const payload = decodeBridgeRivetErrorPayload(value);
	if (!payload) {
		return undefined;
	}

	return new RivetError(payload.group, payload.code, payload.message, {
		metadata: payload.metadata,
		rayId: payload.rayId,
		public: payload.public,
		statusCode: payload.statusCode,
		actor: payload.actor ?? undefined,
	});
}

export function isRivetErrorCode(
	error: unknown,
	group: string,
	code: string,
): error is RivetError {
	return (
		isRivetErrorLike(error) && error.group === group && error.code === code
	);
}

export function internalError(
	message: string,
	options?: Partial<RivetErrorOptions> & {
		group?: string;
		code?: string;
	},
): RivetError {
	return new RivetError(
		options?.group ?? "actor",
		options?.code ?? INTERNAL_ERROR_CODE,
		message,
		{
			public: options?.public,
			statusCode: options?.statusCode,
			metadata: options?.metadata,
			rayId: options?.rayId,
			actor: options?.actor,
			cause: options?.cause,
		},
	);
}

export function invalidEncoding(format?: string): RivetError {
	return new RivetError(
		"encoding",
		"invalid",
		`Invalid encoding \`${format}\`. (https://www.rivet.dev/docs/clients/javascript)`,
		{
			public: true,
		},
	);
}

export function invalidRequest(error?: unknown): RivetError {
	return new RivetError(
		"request",
		"invalid",
		`Invalid request: ${errorMessage(error, String(error))}`,
		{
			public: true,
			cause: error instanceof Error ? error : undefined,
		},
	);
}

export function actorNotFound(identifier?: string): RivetError {
	return new RivetError(
		"actor",
		"not_found",
		identifier
			? `Actor not found: ${identifier} (https://www.rivet.dev/docs/clients/javascript)`
			: "Actor not found (https://www.rivet.dev/docs/clients/javascript)",
		{ public: true },
	);
}

export function actorStopping(identifier?: string): RivetError {
	return new RivetError(
		"actor",
		"stopping",
		identifier ? `Actor stopping: ${identifier}` : "Actor stopping",
		{ public: true },
	);
}

export interface ActorRestartingOptions {
	phase?: "stopping" | "sleeping" | "waking" | "runner_shutdown";
	retryAfterMs?: number;
}

export function actorRestarting(opts?: ActorRestartingOptions): RivetError {
	return new RivetError(
		"actor",
		"restarting",
		"Actor is restarting. Retry the request.",
		{
			public: true,
			statusCode: 503,
			metadata: {
				retryable: true,
				...(opts?.phase ? { phase: opts.phase } : {}),
				...(opts?.retryAfterMs !== undefined
					? { retryAfterMs: opts.retryAfterMs }
					: {}),
			},
		},
	);
}

export function forbiddenError(): RivetError {
	return new RivetError("auth", "forbidden", "Forbidden", {
		public: true,
		statusCode: 403,
	});
}

export function unsupportedFeature(feature: string): RivetError {
	return new RivetError(
		"feature",
		"unsupported",
		`Unsupported feature: ${feature}`,
	);
}
