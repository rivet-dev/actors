import type { Next } from "hono";
import type { ContentfulStatusCode } from "hono/utils/http-status";
import * as errors from "@/actor/errors";
import { getLogErrorStack } from "@/utils/env-vars";

export function assertUnreachable(x: never): never {
	throw new Error(`Unreachable case: ${x}`);
}

/**
 * Safely stringifies an object, ensuring that the stringified object is under a certain size.
 * @param obj any object to stringify
 * @param maxSize maximum size of the stringified object in bytes
 * @returns stringified object
 */
export function safeStringify(obj: unknown, maxSize: number) {
	let size = 0;

	function replacer(key: string, value: unknown) {
		if (value === null || value === undefined) return value;
		const valueSize =
			typeof value === "string"
				? value.length
				: JSON.stringify(value).length;
		size += key.length + valueSize;

		if (size > maxSize) {
			throw new Error(
				`JSON object exceeds size limit of ${maxSize} bytes.`,
			);
		}

		return value;
	}

	return JSON.stringify(obj, replacer);
}

export interface DeconstructedError {
	__type: "ActorError";
	statusCode: ContentfulStatusCode;
	public: boolean;
	group: string;
	code: string;
	message: string;
	metadata?: unknown;
	rayId?: string;
	actor?: errors.ActorSpecifier;
}

function isCanonicalStructuredRivetError(
	error: unknown,
): error is errors.RivetErrorLike {
	return (
		error instanceof errors.RivetError ||
		(typeof error === "object" &&
			error !== null &&
			"__type" in error &&
			error.__type === "RivetError" &&
			"group" in error &&
			typeof error.group === "string" &&
			"code" in error &&
			typeof error.code === "string" &&
			"message" in error &&
			typeof error.message === "string")
	);
}

/**
 * Deconstructs errors into response fields. Bridge callback errors that cross
 * into rivetkit-core are sanitized there; this only classifies JS-local errors.
 */
export function deconstructError(
	error: unknown,
	exposeInternalError = false,
): DeconstructedError {
	// Build response error information. Only return errors if flagged as public in order to prevent leaking internal behavior.
	let statusCode: ContentfulStatusCode;
	let public_: boolean;
	let group: string;
	let code: string;
	let message: string;
	let metadata: unknown;
	let rayId: string | undefined;
	let actor: errors.ActorSpecifier | undefined;
	// Structured errors from core or from pre-built `RivetError` instances are canonical.
	// Only unstructured errors go through the classifier below.
	if (isCanonicalStructuredRivetError(error)) {
		statusCode = (
			typeof error.statusCode === "number"
				? error.statusCode
				: error.public
					? 400
					: 500
		) as ContentfulStatusCode;
		public_ = error.public ?? false;
		group = error.group;
		code = error.code;
		message = error.message;
		metadata = error.metadata;
		rayId = error.rayId;
		actor = error.actor;
	} else if (errors.ActorError.isActorError(error) && error.public) {
		// Check if error has statusCode (could be ActorError instance or DeconstructedError)
		statusCode = (
			"statusCode" in error && error.statusCode ? error.statusCode : 400
		) as ContentfulStatusCode;
		public_ = true;
		group = error.group;
		code = error.code;
		message = getErrorMessage(error);
		metadata = error.metadata;
		rayId = error.rayId;
		actor = error.actor;
	} else if (exposeInternalError) {
		if (errors.ActorError.isActorError(error)) {
			statusCode = 500;
			public_ = false;
			group = error.group;
			code = error.code;
			message = getErrorMessage(error);
			metadata = error.metadata;
			rayId = error.rayId;
			actor = error.actor;
		} else {
			statusCode = 500;
			public_ = false;
			group = "rivetkit";
			code = errors.INTERNAL_ERROR_CODE;
			message = getErrorMessage(error);
		}
	} else {
		statusCode = 500;
		public_ = false;
		group = "rivetkit";
		code = errors.INTERNAL_ERROR_CODE;
		message = errors.INTERNAL_ERROR_DESCRIPTION;
		if (errors.ActorError.isActorError(error)) {
			actor = error.actor;
		}
		metadata = {
			//url: `https://dashboard.rivet.dev/projects/${actorMetadata.project.slug}/environments/${actorMetadata.environment.slug}/actors?actorId=${actorMetadata.actor.id}`,
		} satisfies errors.InternalErrorMetadata;
	}

	return {
		__type: "ActorError",
		statusCode,
		public: public_,
		group,
		code,
		message,
		metadata,
		rayId,
		actor,
	};
}

export function stringifyError(error: unknown): string {
	if (error instanceof Error) {
		if (typeof process !== "undefined" && getLogErrorStack()) {
			let stack: string | undefined;
			try {
				stack = error.stack;
			} catch {
				stack = undefined;
			}
			return `${error.name}: ${error.message}${stack ? `\n${stack}` : ""}`;
		} else {
			return `${error.name}: ${error.message}`;
		}
	} else if (typeof error === "string") {
		return error;
	} else if (typeof error === "object" && error !== null) {
		try {
			return `${JSON.stringify(error)}`;
		} catch {
			return "[cannot stringify error]";
		}
	} else {
		return `Unknown error: ${getErrorMessage(error)}`;
	}
}

function getErrorMessage(err: unknown): string {
	if (
		err &&
		typeof err === "object" &&
		"message" in err &&
		typeof err.message === "string"
	) {
		return err.message;
	} else {
		return String(err);
	}
}

/** Generates a `Next` handler to pass to middleware in order to be able to call arbitrary middleware. */
export function noopNext(): Next {
	return async () => {};
}
