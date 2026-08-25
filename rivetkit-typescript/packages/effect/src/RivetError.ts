import { Duration, Option, Predicate, Record, Schema } from "effect";
import * as RivetkitErrors from "rivetkit/errors";

const ReasonTypeId = "~@rivetkit/effect/RivetError/Reason" as const;
const TypeId = "~@rivetkit/effect/RivetError" as const;

export class Forbidden extends Schema.TaggedErrorClass<Forbidden>(
	`${ReasonTypeId}/Forbidden`,
)("Forbidden", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

export class ActorNotFound extends Schema.TaggedErrorClass<ActorNotFound>(
	`${ReasonTypeId}/ActorNotFound`,
)("ActorNotFound", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

export class ActorStopping extends Schema.TaggedErrorClass<ActorStopping>(
	`${ReasonTypeId}/ActorStopping`,
)("ActorStopping", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
}

export class ActorRestarting extends Schema.TaggedErrorClass<ActorRestarting>(
	`${ReasonTypeId}/ActorRestarting`,
)("ActorRestarting", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
	get retryAfter(): Duration.Duration | undefined {
		if (!Predicate.isReadonlyObject(this.metadata)) return undefined;
		return Record.get(this.metadata, "retryAfterMs").pipe(
			Option.filter(Predicate.isNumber),
			Option.map(Duration.millis),
			Option.getOrUndefined,
		);
	}
}

export class ActionNotFound extends Schema.TaggedErrorClass<ActionNotFound>(
	`${ReasonTypeId}/ActionNotFound`,
)("ActionNotFound", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

export class ActionTimedOut extends Schema.TaggedErrorClass<ActionTimedOut>(
	`${ReasonTypeId}/ActionTimedOut`,
)("ActionTimedOut", { cause: Schema.instanceOf(RivetkitErrors.RivetError) }) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
}

export class ActionAborted extends Schema.TaggedErrorClass<ActionAborted>(
	`${ReasonTypeId}/ActionAborted`,
)("ActionAborted", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

export class ActorOverloaded extends Schema.TaggedErrorClass<ActorOverloaded>(
	`${ReasonTypeId}/ActorOverloaded`,
)("ActorOverloaded", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
}

export class IncomingMessageTooLong extends Schema.TaggedErrorClass<IncomingMessageTooLong>(
	`${ReasonTypeId}/IncomingMessageTooLong`,
)("IncomingMessageTooLong", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

export class OutgoingMessageTooLong extends Schema.TaggedErrorClass<OutgoingMessageTooLong>(
	`${ReasonTypeId}/OutgoingMessageTooLong`,
)("OutgoingMessageTooLong", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

export class InvalidEncoding extends Schema.TaggedErrorClass<InvalidEncoding>(
	`${ReasonTypeId}/InvalidEncoding`,
)("InvalidEncoding", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

export class InvalidRequest extends Schema.TaggedErrorClass<InvalidRequest>(
	`${ReasonTypeId}/InvalidRequest`,
)("InvalidRequest", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

export class GuardActorReadyTimeout extends Schema.TaggedErrorClass<GuardActorReadyTimeout>(
	`${ReasonTypeId}/GuardActorReadyTimeout`,
)("GuardActorReadyTimeout", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
}

export class GuardActorWakeRetriesExceeded extends Schema.TaggedErrorClass<GuardActorWakeRetriesExceeded>(
	`${ReasonTypeId}/GuardActorWakeRetriesExceeded`,
)("GuardActorWakeRetriesExceeded", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
}

export class GuardActorRunnerFailed extends Schema.TaggedErrorClass<GuardActorRunnerFailed>(
	`${ReasonTypeId}/GuardActorRunnerFailed`,
)("GuardActorRunnerFailed", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

export class GuardServiceUnavailable extends Schema.TaggedErrorClass<GuardServiceUnavailable>(
	`${ReasonTypeId}/GuardServiceUnavailable`,
)("GuardServiceUnavailable", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
}

export class GuardActorStoppedWhileWaiting extends Schema.TaggedErrorClass<GuardActorStoppedWhileWaiting>(
	`${ReasonTypeId}/GuardActorStoppedWhileWaiting`,
)("GuardActorStoppedWhileWaiting", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
}

export class GuardTunnelRequestAborted extends Schema.TaggedErrorClass<GuardTunnelRequestAborted>(
	`${ReasonTypeId}/GuardTunnelRequestAborted`,
)("GuardTunnelRequestAborted", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
}

export class GuardTunnelMessageTimeout extends Schema.TaggedErrorClass<GuardTunnelMessageTimeout>(
	`${ReasonTypeId}/GuardTunnelMessageTimeout`,
)("GuardTunnelMessageTimeout", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
}

export class GuardTunnelResponseClosed extends Schema.TaggedErrorClass<GuardTunnelResponseClosed>(
	`${ReasonTypeId}/GuardTunnelResponseClosed`,
)("GuardTunnelResponseClosed", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
}

export class GuardGatewayResponseStartTimeout extends Schema.TaggedErrorClass<GuardGatewayResponseStartTimeout>(
	`${ReasonTypeId}/GuardGatewayResponseStartTimeout`,
)("GuardGatewayResponseStartTimeout", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return true;
	}
}

export class InternalError extends Schema.TaggedErrorClass<InternalError>(
	`${ReasonTypeId}/InternalError`,
)("InternalError", {
	cause: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

export class ActionErrorDecodeFailed extends Schema.TaggedErrorClass<ActionErrorDecodeFailed>(
	`${ReasonTypeId}/ActionErrorDecodeFailed`,
)("ActionErrorDecodeFailed", {
	cause: Schema.instanceOf(Schema.SchemaError),
	rivetError: Schema.instanceOf(RivetkitErrors.RivetError),
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return `Failed to decode action error ${this.rivetError.group}.${this.rivetError.code}`;
	}
	get group() {
		return this.rivetError.group;
	}
	get code() {
		return this.rivetError.code;
	}
	get metadata() {
		return this.rivetError.metadata;
	}
	get actor() {
		return this.rivetError.actor;
	}
	get statusCode() {
		return this.rivetError.statusCode;
	}
	get public() {
		return this.rivetError.public;
	}
	get rayId() {
		return this.rivetError.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

/**
 * Open-ended user error reason. Used when the actor threw `UserError` but
 * the failing action did not declare a matching schema in its `error`
 * field — so we can't surface it as a typed domain error in the Effect
 * error channel.
 *
 * Actions that declare their user errors via `Action.make({ error: ... })`
 * receive those errors **typed** in the error channel; this reason is
 * the catch-all for everything else.
 */
export class UnknownUserError extends Schema.TaggedErrorClass<UnknownUserError>(
	`${ReasonTypeId}/UnknownUserError`,
)("UnknownUserError", { cause: Schema.instanceOf(RivetkitErrors.RivetError) }) {
	readonly [ReasonTypeId] = ReasonTypeId;
	override get message() {
		return this.cause.message;
	}
	get group() {
		return this.cause.group;
	}
	get code() {
		return this.cause.code;
	}
	get metadata() {
		return this.cause.metadata;
	}
	get actor() {
		return this.cause.actor;
	}
	get statusCode() {
		return this.cause.statusCode;
	}
	get public() {
		return this.cause.public;
	}
	get rayId() {
		return this.cause.rayId;
	}
	get isRetryable(): boolean {
		return false;
	}
}

/**
 * Forward-compatible catch-all for `(group, code)` pairs the SDK does
 * not recognize yet, and for malformed non-Rivet failures. Known wire
 * fields are mirrored when present, while `cause` preserves the raw input.
 */
export class UnknownError extends Schema.TaggedErrorClass<UnknownError>(
	`${ReasonTypeId}/UnknownError`,
)("UnknownError", {
	message: Schema.String,
	cause: Schema.Unknown,
}) {
	readonly [ReasonTypeId] = ReasonTypeId;
	get group() {
		return this.cause instanceof RivetkitErrors.RivetError
			? this.cause.group
			: undefined;
	}
	get code() {
		return this.cause instanceof RivetkitErrors.RivetError
			? this.cause.code
			: undefined;
	}
	get metadata() {
		return this.cause instanceof RivetkitErrors.RivetError
			? this.cause.metadata
			: undefined;
	}
	get actor() {
		return this.cause instanceof RivetkitErrors.RivetError
			? this.cause.actor
			: undefined;
	}
	get statusCode() {
		return this.cause instanceof RivetkitErrors.RivetError
			? this.cause.statusCode
			: undefined;
	}
	get public() {
		return this.cause instanceof RivetkitErrors.RivetError
			? this.cause.public
			: undefined;
	}
	get rayId() {
		return this.cause instanceof RivetkitErrors.RivetError
			? this.cause.rayId
			: undefined;
	}
	get isRetryable(): boolean {
		return false;
	}
}

export type RivetErrorReason =
	| Forbidden
	| ActorNotFound
	| ActorStopping
	| ActorRestarting
	| ActionNotFound
	| ActionTimedOut
	| ActionAborted
	| ActorOverloaded
	| IncomingMessageTooLong
	| OutgoingMessageTooLong
	| InvalidEncoding
	| InvalidRequest
	| GuardActorReadyTimeout
	| GuardActorWakeRetriesExceeded
	| GuardActorRunnerFailed
	| GuardServiceUnavailable
	| GuardActorStoppedWhileWaiting
	| GuardTunnelRequestAborted
	| GuardTunnelMessageTimeout
	| GuardTunnelResponseClosed
	| GuardGatewayResponseStartTimeout
	| InternalError
	| UnknownUserError
	| ActionErrorDecodeFailed
	| UnknownError;

export const RivetErrorReason: Schema.Union<
	[
		typeof Forbidden,
		typeof ActorNotFound,
		typeof ActorStopping,
		typeof ActorRestarting,
		typeof ActionNotFound,
		typeof ActionTimedOut,
		typeof ActionAborted,
		typeof ActorOverloaded,
		typeof IncomingMessageTooLong,
		typeof OutgoingMessageTooLong,
		typeof InvalidEncoding,
		typeof InvalidRequest,
		typeof GuardActorReadyTimeout,
		typeof GuardActorWakeRetriesExceeded,
		typeof GuardActorRunnerFailed,
		typeof GuardServiceUnavailable,
		typeof GuardActorStoppedWhileWaiting,
		typeof GuardTunnelRequestAborted,
		typeof GuardTunnelMessageTimeout,
		typeof GuardTunnelResponseClosed,
		typeof GuardGatewayResponseStartTimeout,
		typeof InternalError,
		typeof ActionErrorDecodeFailed,
		typeof UnknownUserError,
		typeof UnknownError,
	]
> = Schema.Union([
	Forbidden,
	ActorNotFound,
	ActorStopping,
	ActorRestarting,
	ActionNotFound,
	ActionTimedOut,
	ActionAborted,
	ActorOverloaded,
	IncomingMessageTooLong,
	OutgoingMessageTooLong,
	InvalidEncoding,
	InvalidRequest,
	GuardActorReadyTimeout,
	GuardActorWakeRetriesExceeded,
	GuardActorRunnerFailed,
	GuardServiceUnavailable,
	GuardActorStoppedWhileWaiting,
	GuardTunnelRequestAborted,
	GuardTunnelMessageTimeout,
	GuardTunnelResponseClosed,
	GuardGatewayResponseStartTimeout,
	InternalError,
	ActionErrorDecodeFailed,
	UnknownUserError,
	UnknownError,
]);

export const isRivetErrorReason = (u: unknown): u is RivetErrorReason =>
	Predicate.hasProperty(u, ReasonTypeId);

/**
 * The infrastructure-failure error surfaced by `@rivetkit/effect`
 * calls. Wraps a discriminated `reason` of all known failure
 * modes.
 *
 * Recover with `Effect.catchReason` / `Effect.catchReasons` /
 * `Effect.unwrapReason`:
 *
 * ```ts
 * program.pipe(
 *   Effect.catchReasons("RivetError", {
 *     Forbidden: () => Effect.fail(new MyAuthError()),
 *     ConnectionLost: () => Effect.logWarning("reconnecting"),
 *   }),
 * )
 * ```
 *
 * User-defined errors declared on an action via `Action.make({ error })`
 * arrive in the typed error channel separately and do NOT flow through
 * `RivetError`.
 */
export class RivetError extends Schema.TaggedErrorClass<RivetError>(
	"@rivetkit/effect/RivetError",
)("RivetError", {
	reason: RivetErrorReason,
}) {
	/** Marks this value as the top-level Rivet error wrapper for runtime guards. */
	readonly [TypeId] = TypeId;

	/** Exposes the structured Rivet error reason as the JavaScript error cause. */
	override readonly cause = this.reason;

	/** Uses the reason message when present, otherwise falls back to the reason tag. */
	override get message() {
		return this.reason.message || this.reason._tag;
	}

	/** Delegates to the underlying reason's `group` if present. */
	get group(): string | undefined {
		return "group" in this.reason ? this.reason.group : undefined;
	}

	/** Delegates to the underlying reason's `code` if present. */
	get code(): string | undefined {
		return "code" in this.reason ? this.reason.code : undefined;
	}

	/** Delegates to the underlying reason's `metadata` if present. */
	get metadata(): unknown {
		return "metadata" in this.reason ? this.reason.metadata : undefined;
	}

	/** Delegates to the underlying reason's `actor` if present. */
	get actor() {
		return "actor" in this.reason ? this.reason.actor : undefined;
	}

	/** Delegates to the underlying reason's `statusCode` if present. */
	get statusCode(): number | undefined {
		return "statusCode" in this.reason ? this.reason.statusCode : undefined;
	}

	/** Delegates to the underlying reason's `public` if present. */
	get public(): boolean | undefined {
		return "public" in this.reason ? this.reason.public : undefined;
	}

	/** Delegates to the underlying reason's `rayId` if present. */
	get rayId(): string | undefined {
		return "rayId" in this.reason ? this.reason.rayId : undefined;
	}

	/** Delegates to the underlying reason's `isRetryable` getter. */
	get isRetryable(): boolean {
		return this.reason.isRetryable;
	}

	/** Delegates to the underlying reason's `retryAfter` if present. */
	get retryAfter(): Duration.Duration | undefined {
		return "retryAfter" in this.reason ? this.reason.retryAfter : undefined;
	}
}

export const isRivetError = (u: unknown): u is RivetError =>
	Predicate.hasProperty(u, TypeId);

type MakeRivetErrorReason = (
	error: RivetkitErrors.RivetError,
) => RivetErrorReason;

const reasonByCode: { [key: string]: MakeRivetErrorReason | undefined } = {
	"auth.forbidden": (error) => new Forbidden({ cause: error }),
	"actor.not_found": (error) => new ActorNotFound({ cause: error }),
	"actor.stopping": (error) => new ActorStopping({ cause: error }),
	"actor.restarting": (error) => new ActorRestarting({ cause: error }),
	"actor.action_not_found": (error) => new ActionNotFound({ cause: error }),
	"actor.action_timed_out": (error) => new ActionTimedOut({ cause: error }),
	"actor.aborted": (error) => new ActionAborted({ cause: error }),
	"actor.overloaded": (error) => new ActorOverloaded({ cause: error }),
	[`actor.${RivetkitErrors.INTERNAL_ERROR_CODE}`]: (error) =>
		new InternalError({ cause: error }),
	[`core.${RivetkitErrors.INTERNAL_ERROR_CODE}`]: (error) =>
		new InternalError({ cause: error }),
	[`rivetkit.${RivetkitErrors.INTERNAL_ERROR_CODE}`]: (error) =>
		new InternalError({ cause: error }),
	"message.incoming_too_long": (error) =>
		new IncomingMessageTooLong({ cause: error }),
	"message.outgoing_too_long": (error) =>
		new OutgoingMessageTooLong({ cause: error }),
	"encoding.invalid": (error) => new InvalidEncoding({ cause: error }),
	"request.invalid": (error) => new InvalidRequest({ cause: error }),
	"guard.actor_ready_timeout": (error) =>
		new GuardActorReadyTimeout({ cause: error }),
	"guard.actor_wake_retries_exceeded": (error) =>
		new GuardActorWakeRetriesExceeded({ cause: error }),
	"guard.actor_runner_failed": (error) =>
		new GuardActorRunnerFailed({ cause: error }),
	"guard.service_unavailable": (error) =>
		new GuardServiceUnavailable({ cause: error }),
	"guard.actor_stopped_while_waiting": (error) =>
		new GuardActorStoppedWhileWaiting({ cause: error }),
	"guard.tunnel_request_aborted": (error) =>
		new GuardTunnelRequestAborted({ cause: error }),
	"guard.tunnel_message_timeout": (error) =>
		new GuardTunnelMessageTimeout({ cause: error }),
	"guard.tunnel_response_closed": (error) =>
		new GuardTunnelResponseClosed({ cause: error }),
	"guard.gateway_response_start_timeout": (error) =>
		new GuardGatewayResponseStartTimeout({ cause: error }),
};

const reasonFromRivetkitRivetError = (
	error: RivetkitErrors.RivetError,
): RivetErrorReason => {
	const makeReason = reasonByCode[`${error.group}.${error.code}`];
	if (makeReason) return makeReason(error);

	if (error.group === "user") return new UnknownUserError({ cause: error });

	return new UnknownError({
		message: error.message,
		cause: error,
	});
};

export const fromRivetkitRivetError = (
	error: RivetkitErrors.RivetError,
): RivetError => {
	return new RivetError({ reason: reasonFromRivetkitRivetError(error) });
};

export const fromUnknown = (cause: unknown): RivetError => {
	if (isRivetError(cause)) return cause;
	if (RivetkitErrors.isRivetErrorLike(cause)) {
		return fromRivetkitRivetError(RivetkitErrors.toRivetError(cause));
	}

	return new RivetError({
		reason: new UnknownError({
			message: cause instanceof Error ? cause.message : String(cause),
			cause,
		}),
	});
};
