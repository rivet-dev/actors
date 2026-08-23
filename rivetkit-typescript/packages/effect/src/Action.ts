import { type Effect, Predicate, Schema } from "effect";

const TypeId = "~@rivetkit/effect/Action";

export const isAction = (u: unknown): u is Action<any, any, any, any> =>
	Predicate.hasProperty(u, TypeId);

/**
 * A value-level definition for a non-durable, request-response call.
 */
export interface Action<
	Tag extends string,
	Payload extends Schema.Top = Schema.Void,
	Success extends Schema.Top = Schema.Void,
	Error extends Schema.Top = Schema.Never,
> {
	readonly [TypeId]: typeof TypeId;
	readonly _tag: Tag;
	readonly key: string;
	/**
	 * Raw RivetKit clients omit the argument for no-payload actions, so
	 * the actor wrapper uses this to adapt only those calls to the Effect
	 * JSON Void codec's null representation.
	 */
	readonly hasPayload: boolean;
	readonly payloadSchema: Payload;
	readonly successSchema: Success;
	readonly errorSchema: Error;
}

/**
 * Type-erased view of any `Action`. Useful for collections of actions
 * where the specific schemas don't matter.
 */
export interface Any {
	readonly [TypeId]: typeof TypeId;
	readonly _tag: string;
	readonly key: string;
}

/**
 * Like `Any`, but with the prop fields (`*Schema`) accessible. Used
 * by internal builders that need to read schemas off an action.
 */
export interface AnyWithProps {
	readonly [TypeId]: typeof TypeId;
	readonly _tag: string;
	readonly key: string;
	readonly hasPayload: boolean;
	readonly payloadSchema: Schema.Top;
	readonly successSchema: Schema.Top;
	readonly errorSchema: Schema.Top;
}

// --- Type helpers ---------------------------------------------------

export type Tag<R> =
	R extends Action<infer _Tag, infer _Payload, infer _Success, infer _Error>
		? _Tag
		: never;

export type PayloadSchema<R> =
	R extends Action<infer _Tag, infer _Payload, infer _Success, infer _Error>
		? _Payload
		: never;

export type Payload<R> = PayloadSchema<R>["Type"];

/**
 * The shape accepted by the payload schema's `make` constructor on the
 * client side (i.e. before encoding). Useful for typing the call site.
 */
export type PayloadConstructor<R> =
	R extends Action<infer _Tag, infer _Payload, infer _Success, infer _Error>
		? _Payload["~type.make.in"]
		: never;

export type SuccessSchema<R> =
	R extends Action<infer _Tag, infer _Payload, infer _Success, infer _Error>
		? _Success
		: never;

export type Success<R> = SuccessSchema<R>["Type"];

export type ErrorSchema<R> =
	R extends Action<infer _Tag, infer _Payload, infer _Success, infer _Error>
		? _Error
		: never;

export type Error<R> = ErrorSchema<R>["Type"];

/**
 * The full set of decoding/encoding services required by every schema
 * referenced by the action. Code generators include this in the `R`
 * channel of any effect that handles or invokes the action.
 */
export type Services<R> =
	R extends Action<infer _Tag, infer _Payload, infer _Success, infer _Error>
		?
				| _Payload["DecodingServices"]
				| _Payload["EncodingServices"]
				| _Success["DecodingServices"]
				| _Success["EncodingServices"]
				| _Error["DecodingServices"]
				| _Error["EncodingServices"]
		: never;

/**
 * The subset of `Services` actually needed on the client side: encoding
 * the payload, decoding the success response, decoding the error.
 */
export type ServicesClient<R> =
	R extends Action<infer _Tag, infer _Payload, infer _Success, infer _Error>
		?
				| _Payload["EncodingServices"]
				| _Success["DecodingServices"]
				| _Error["DecodingServices"]
		: never;

/**
 * The subset of `Services` needed on the server side: decoding the
 * payload, encoding the success response, encoding the error.
 */
export type ServicesServer<R> =
	R extends Action<infer _Tag, infer _Payload, infer _Success, infer _Error>
		?
				| _Payload["DecodingServices"]
				| _Success["EncodingServices"]
				| _Error["EncodingServices"]
		: never;

/**
 * Extract the action with the matching tag from a union of actions.
 */
export type ExtractTag<R extends Any, Tag extends string> = R extends {
	readonly _tag: Tag;
}
	? R
	: never;

export type ResultFrom<R extends Any, Services> = R extends Action<
	infer _Tag,
	infer _Payload,
	infer _Success,
	infer _Error
>
	? Effect.Effect<_Success["Type"], _Error["Type"], Services>
	: never;

// --- Implementation -------------------------------------------------

const Proto = {
	[TypeId]: TypeId,
};

const makeProto = <
	const Tag extends string,
	Payload extends Schema.Top,
	Success extends Schema.Top,
	Error extends Schema.Top,
>(options: {
	readonly _tag: Tag;
	readonly hasPayload: boolean;
	readonly payloadSchema: Payload;
	readonly successSchema: Success;
	readonly errorSchema: Error;
}): Action<Tag, Payload, Success, Error> => {
	const self = Object.assign(Object.create(Proto), options);
	self.key = `@rivetkit/effect/Action/${options._tag}`;
	return self;
};

/**
 * Define a Rivet Actor action.
 *
 * @example
 * ```ts
 * import { Schema } from "effect"
 * import { Action } from "@rivetkit/effect"
 *
 * class CounterOverflow extends Schema.TaggedError<CounterOverflow>()(
 *   "CounterOverflow",
 *   { limit: Schema.Number },
 * ) {}
 *
 * export const Increment = Action.make("Increment", {
 *   payload: { amount: Schema.Number },
 *   success: Schema.Number,
 *   error: CounterOverflow,
 * })
 * ```
 */
export const make = <
	const Tag extends string,
	Payload extends Schema.Top | Schema.Struct.Fields = Schema.Void,
	Success extends Schema.Top = Schema.Void,
	Error extends Schema.Top = Schema.Never,
>(
	tag: Tag,
	options?: {
		readonly payload?: Payload;
		readonly success?: Success;
		readonly error?: Error;
	},
): Action<
	Tag,
	Payload extends Schema.Struct.Fields ? Schema.Struct<Payload> : Payload,
	Success,
	Error
> => {
	const successSchema = options?.success ?? Schema.Void;
	const errorSchema = options?.error ?? Schema.Never;
	const hasPayload = options?.payload !== undefined;
	const payloadSchema: Schema.Top = Schema.isSchema(options?.payload)
		? (options?.payload as any)
		: options?.payload
			? Schema.Struct(options?.payload as any)
			: Schema.Void;
	return makeProto({
		_tag: tag,
		hasPayload,
		payloadSchema,
		successSchema,
		errorSchema,
	}) as Action<
		Tag,
		Payload extends Schema.Struct.Fields ? Schema.Struct<Payload> : Payload,
		Success,
		Error
	>;
};
