import {
	Cause,
	Effect,
	Exit,
	type Fiber,
	Option,
	Record,
	Schema,
	Tracer,
} from "effect";
import * as Rivetkit from "rivetkit";
import type * as Action from "../Action.ts";
import type { ActionHandlersFrom, ActionRequest, Actor } from "../Actor.ts";
import type * as Client from "../Client.ts";
import * as ActionErrorEnvelope from "./ActionErrorEnvelope.ts";
import { makeActorLogAnnotations } from "./logging.ts";
import { readTraceMeta, rpcSystem } from "./tracing.ts";
import { hasStringProperty } from "./utils.ts";

export type Instance<ActionHandlers> = {
	readonly actionHandlers: ActionHandlers;
	readonly runFork: <A, E>(
		effect: Effect.Effect<A, E, any>,
		options?: Effect.RunOptions,
	) => Fiber.Fiber<A, E>;
};

export const make = <
	Name extends string,
	Actions extends Action.AnyWithProps,
	ActionHandlers extends ActionHandlersFrom<Actions>,
	ActorDefinition extends Rivetkit.AnyActorDefinition,
>({
	actor,
	getInstance,
}: {
	readonly actor: Actor<Name, Actions>;
	readonly getInstance: (
		actorId: string,
	) => Instance<ActionHandlers> | undefined;
}) =>
	Record.fromIterableWith(actor.actions, (action) => {
		const decodePayload = Schema.decodeUnknownEffect(
			Schema.toCodecJson(action.payloadSchema),
		);
		const encodeSuccess = Schema.encodeEffect(
			Schema.toCodecJson(action.successSchema),
		);
		const encodeError = Schema.encodeEffect(
			Schema.toCodecJson(action.errorSchema),
		);

		return [
			action._tag,
			async (
				c: Rivetkit.ActionContextOf<ActorDefinition>,
				payload: Action.Payload<typeof action>,
				meta?: Client.ActionMeta, // TODO: Find better type
			) => {
				// Always wrap in a server-side span so the handler has a
				// live `currentSpan` even when the caller didn't ship trace
				// context (e.g., a non-Effect-SDK client). When trace context
				// is present, reattach it as the parent so the server span
				// joins the caller's trace.
				const rpcMethod = `${actor.name}/${action._tag}`;
				const traceMeta = readTraceMeta(meta);

				const instance = getInstance(c.actorId);
				if (!instance) {
					if (c.abortSignal.aborted) throw makeActorAbortedError();
					throw new Error("actor instance missing");
				}

				const actionEffect = Effect.gen(function* () {
					// The handler map is keyed by the same action
					// definitions being registered here, but
					// TypeScript loses that relationship once the
					// actions are widened into the RivetKit actions
					// record.
					const actionHandler = instance.actionHandlers[
						action._tag as keyof ActionHandlers
					] as (
						envelope: ActionRequest<typeof action>,
					) => Action.ResultFrom<typeof action, any>;
					// RivetKit appends schedule metadata after positional arguments.
					// For actions without payloads, this metadata occupies the payload
					// slot. Effect's JSON Void codec expects that slot to contain null.
					const actionExpectsVoid = !action.hasPayload;
					const payloadWasOmitted = payload === undefined;
					const scheduleMetadataIsPayload =
						actionExpectsVoid &&
						meta === undefined &&
						isScheduledFireInfo(payload);
					const shouldDecodeAsVoid =
						(actionExpectsVoid && payloadWasOmitted) ||
						scheduleMetadataIsPayload;
					const payloadForDecode = shouldDecodeAsVoid ? null : payload;
					const decodedPayload = yield* decodePayload(
						payloadForDecode,
					).pipe(
						Effect.mapError(
							() =>
								new Rivetkit.RivetError(
									"request",
									"invalid",
									`Invalid payload for action ${actor.name}/${action._tag}`,
								),
						),
					);
					// The payload was decoded with this action's schema,
					// so this is the runtime boundary that restores the
					// typed envelope expected by the user handler.
					const actionRequest = {
						_tag: action._tag,
						action,
						payload: decodedPayload,
					} as ActionRequest<typeof action>;

					const resultExit = yield* Effect.exit(
						actionHandler(actionRequest),
					);

					if (Exit.isSuccess(resultExit)) {
						return yield* encodeSuccess(resultExit.value).pipe(
							Effect.orDie,
						);
					}

					const expectedError = Exit.findErrorOption(resultExit);

					if (Option.isSome(expectedError)) {
						const encodedError = yield* encodeError(
							expectedError.value,
						).pipe(Effect.orDie);

						return yield* Effect.fail(
							new Rivetkit.UserError(
								hasStringProperty("message")(encodedError)
									? encodedError.message
									: `${action._tag} failed`,
								{
									code: hasStringProperty("_tag")(
										encodedError,
									)
										? encodedError._tag
										: undefined,
									metadata:
										ActionErrorEnvelope.make(encodedError),
								},
							),
						);
					}

					return yield* Effect.failCause(resultExit.cause);
				}).pipe(
					Effect.withSpan(rpcMethod, {
						parent: traceMeta
							? Tracer.externalSpan(traceMeta)
							: undefined,
						kind: "server",
						attributes: {
							"rpc.system.name": rpcSystem,
							"rpc.method": rpcMethod,
						},
					}),
					Effect.annotateLogs(makeActorLogAnnotations(c)),
				);
				const fiber = instance.runFork(actionEffect, {
					signal: c.abortSignal,
				});
				const exit = await new Promise<Exit.Exit<unknown, unknown>>(
					(resolve) => fiber.addObserver(resolve),
				);

				if (Exit.isSuccess(exit)) return exit.value;
				// Action fibers can be interrupted by a caller abort signal
				// or by the actor instance scope closing during sleep, destroy,
				// or shutdown. Surface those lifecycle exits as RivetKit's
				// structured action-aborted error instead of an internal error.
				if (Cause.hasInterruptsOnly(exit.cause)) {
					throw makeActorAbortedError();
				}
				const expectedError = Exit.findErrorOption(exit);
				if (Option.isSome(expectedError)) {
					throw expectedError.value;
				}
				throw Cause.squash(exit.cause);
			},
		];
	});

const ScheduledFireInfoSchema: Schema.Schema<Rivetkit.ScheduledFireInfo> =
	Schema.Struct({
		kind: Schema.Literals(["at", "cron", "every"]),
		id: Schema.String,
		name: Schema.optionalKey(Schema.UndefinedOr(Schema.String)),
		scheduledAt: Schema.Finite,
		firedAt: Schema.Finite,
	});

const isScheduledFireInfo = Schema.is(ScheduledFireInfoSchema);

const makeActorAbortedError = () =>
	new Rivetkit.RivetError("actor", "aborted", "Actor aborted", {
		public: true,
	});
