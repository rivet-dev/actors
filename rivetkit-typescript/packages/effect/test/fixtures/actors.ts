import { Action, Actor, State } from "@rivetkit/effect";
import {
	Context,
	DateTime,
	Effect,
	Layer,
	Option,
	Ref,
	Schema,
	SchemaIssue,
	SchemaTransformation,
} from "effect";
import { db } from "rivetkit/db";

// --- Counter ---

export class CounterOverflowError extends Schema.TaggedErrorClass<CounterOverflowError>()(
	"CounterOverflowError",
	{
		limit: Schema.Number,
		message: Schema.String,
	},
) {}

export class Flags extends Context.Service<Flags>()("Flags", {
	make: Effect.sync(() => new Map<string, boolean | string>()),
}) {
	static readonly layer = Layer.effect(Flags, this.make);
}

/**
 * A non-built-in service used by `Counter` to verify that user-provided
 * services resolve in both the wake-scope build effect and inside
 * individual action handlers.
 */
export class Greeter extends Context.Service<
	Greeter,
	{ readonly greet: (name: string) => string }
>()("test/Greeter") {}

const TagsCsv = Schema.String.pipe(
	Schema.decodeTo(
		Schema.Array(Schema.String),
		SchemaTransformation.transform({
			decode: (s: string): ReadonlyArray<string> => s.split(","),
			encode: (arr: ReadonlyArray<string>) => arr.join(","),
		}),
	),
);

export const Increment = Action.make("Increment", {
	payload: { amount: Schema.Number },
	success: Schema.Number,
	error: CounterOverflowError,
});

export const GetCount = Action.make("GetCount", {
	success: Schema.Number,
});

export const Crash = Action.make("Crash");

export const EchoDate = Action.make("EchoDate", {
	payload: { when: Schema.DateFromString },
	success: Schema.DateFromString,
});

export const Tags = Action.make("Tags", {
	payload: { tags: TagsCsv },
	success: Schema.Number,
});

export const Greet = Action.make("Greet", {
	payload: { name: Schema.String },
	success: Schema.String,
});

export const WakeGreeting = Action.make("WakeGreeting", {
	success: Schema.String,
});

// An action whose handler emits its own user-defined sub-span. The
// tracing test asserts the sub-span lands as a child of the SDK's
// server-side span, which itself is a child of the SDK's client-side
// span — proof that user spans nest correctly under the SDK's wire
// propagation.
export const Compute = Action.make("Compute", {
	payload: { n: Schema.Number },
	success: Schema.Number,
});

// Service that the codec schema below depends on. Yielding it from
// inside a `transformOrFail` puts `Multiplier` into the schema's
// `DecodingServices` / `EncodingServices`, which in turn surfaces in
// `Action.ServicesServer` / `Action.ServicesClient` for any action
// referencing the codec.
export class Multiplier extends Context.Service<
	Multiplier,
	{ readonly factor: number }
>()("test/Multiplier") {}

// A `Number` schema whose decode multiplies by the live factor and whose
// encode divides by it. With the same factor on both ends, values
// round-trip; the test would fail if any codec site failed to resolve
// `Multiplier`.
const ScaledNumber = Schema.Number.pipe(
	Schema.decodeTo(
		Schema.Number,
		SchemaTransformation.transformOrFail({
			decode: (n: number) =>
				Effect.gen(function* () {
					const m = yield* Multiplier;
					return n * m.factor;
				}),
			encode: (n: number) =>
				Effect.gen(function* () {
					const m = yield* Multiplier;
					return n / m.factor;
				}),
		}),
	),
);

export class ScaledOverflowError extends Schema.TaggedErrorClass<ScaledOverflowError>()(
	"ScaledOverflowError",
	{
		limit: ScaledNumber,
		message: Schema.String,
	},
) {}

// Every channel of this action — payload, success, error — references
// `ScaledNumber`, so a successful round-trip proves all six codec sites
// (payload encode + decode, success encode + decode, error encode +
// decode) resolved their schema services.
export const Scale = Action.make("Scale", {
	payload: { amount: ScaledNumber },
	success: ScaledNumber,
	error: ScaledOverflowError,
});

export const PersistAndSleep = Action.make("PersistAndSleep", {
	payload: { amount: Schema.Number },
	success: Schema.Number,
});

export const PersistDateAndSleep = Action.make("PersistDateAndSleep", {
	payload: { when: Schema.DateFromString },
	success: Schema.Date,
});

export const PersistTagsAndSleep = Action.make("PersistTagsAndSleep", {
	payload: { tags: TagsCsv },
	success: TagsCsv,
});

export const PersistScaledAndSleep = Action.make("PersistScaledAndSleep", {
	payload: { amount: ScaledNumber },
	success: ScaledNumber,
});

export const GetPersistedState = Action.make("GetPersistedState", {
	success: Schema.Struct({
		count: Schema.Number,
		when: Schema.DateFromString,
		tags: TagsCsv,
		scaled: ScaledNumber,
	}),
});

export const LogEvent = Action.make("LogEvent", {
	payload: { event: Schema.String },
	success: Schema.Number,
});

export const ListEvents = Action.make("ListEvents", {
	success: Schema.Array(Schema.String),
});

export const CountEvents = Action.make("CountEvents", {
	success: Schema.Number,
});

export const SleepDuringAction = Action.make("SleepDuringAction", {
	success: Schema.String,
});

const EncodedTransformedState = Schema.Struct({
	when: Schema.String,
	instant: Schema.String,
	url: Schema.String,
	id: Schema.String,
	bytes: Schema.String,
	tags: Schema.String,
	history: Schema.Array(
		Schema.Struct({
			at: Schema.String,
			payload: Schema.String,
		}),
	),
});

const TransformedStateSchema = Schema.Struct({
	when: Schema.Date,
	instant: Schema.DateTimeUtc,
	url: Schema.URL,
	id: Schema.BigInt,
	bytes: Schema.Uint8Array,
	tags: TagsCsv,
	history: Schema.Array(
		Schema.Struct({
			at: Schema.Date,
			payload: Schema.Uint8Array,
		}),
	),
});

export const GetRawWakeState = Action.make("GetRawWakeState", {
	success: EncodedTransformedState,
});

export const GetDecodedState = Action.make("GetDecodedState", {
	success: TransformedStateSchema,
});

export const SetTransformedStateAndSleep = Action.make(
	"SetTransformedStateAndSleep",
	{
		payload: TransformedStateSchema,
	},
);

export const SetRawWakeStateAndSleep = Action.make("SetRawWakeStateAndSleep", {
	payload: EncodedTransformedState,
});

export const TransformedStateActor = Actor.make("TransformedStateActor", {
	actions: [
		GetRawWakeState,
		GetDecodedState,
		SetTransformedStateAndSleep,
		SetRawWakeStateAndSleep,
	],
});

export const TransformedStateActorLive = TransformedStateActor.toLayer(
	({ rawRivetkitContext, state }) =>
		Effect.gen(function* () {
			const sleep = yield* Actor.Sleep;
			const rawWakeState = rawRivetkitContext.state;

			return TransformedStateActor.of({
				GetRawWakeState: () =>
					Effect.succeed(
						rawWakeState as unknown as typeof EncodedTransformedState.Type,
					),
				GetDecodedState: () => State.get(state).pipe(Effect.orDie),
				SetTransformedStateAndSleep: ({ payload }) =>
					State.set(state, payload).pipe(
						Effect.andThen(sleep),
						Effect.orDie,
					),
				SetRawWakeStateAndSleep: ({ payload }) =>
					Effect.tryPromise(async () => {
						rawRivetkitContext.state =
							payload as unknown as typeof rawRivetkitContext.state;
						await rawRivetkitContext.saveState({
							immediate: true,
						});
						rawRivetkitContext.sleep();
					}).pipe(Effect.orDie),
			});
		}),
	{
		state: {
			schema: TransformedStateSchema,
			initialValue: () => ({
				when: new Date("2024-01-01T00:00:00.000Z"),
				instant: DateTime.makeUnsafe(1_704_067_200_000),
				url: new URL("https://rivet.dev/docs"),
				id: 1n,
				bytes: new Uint8Array([1, 2, 3]),
				tags: ["initial"],
				history: [],
			}),
		},
	},
);

export const Counter = Actor.make("Counter", {
	actions: [
		Increment,
		GetCount,
		Crash,
		EchoDate,
		Tags,
		Greet,
		WakeGreeting,
		Compute,
		Scale,
		PersistAndSleep,
		PersistDateAndSleep,
		PersistTagsAndSleep,
		PersistScaledAndSleep,
		GetPersistedState,
		LogEvent,
		ListEvents,
		CountEvents,
		SleepDuringAction,
	],
});

export const CounterLive = Counter.toLayer(
	({ rawRivetkitContext, state }) =>
		Effect.gen(function* () {
			const count = yield* Ref.make(0);
			const flags = yield* Flags;
			flags.set("on wake", true);
			const greeter = yield* Greeter;
			const wakeGreeting = greeter.greet("on wake");

			const sleep = yield* Actor.Sleep;
			// `rawRivetkitContext`'s `db` widens to `any` against
			// `RunContextOf<AnyActorDefinition>`. The provider configured on
			// `Counter.toLayer` below is the `rivetkit/db` raw-access factory,
			// so re-narrow to `RawAccess` for typed `execute` calls inside
			// handler closures.
			const db = rawRivetkitContext.db;
			// `Flags` is a process-wide Map shared across all tests in the
			// suite, so the finalizer flag must be namespaced by actor key
			// to keep cross-test wake/sleep cycles from leaking into each
			// other's assertions.
			const address = yield* Actor.CurrentAddress;
			const finalizerFlag = `finalizer:${address.key.join("/")}`;

			yield* Effect.addFinalizer(() =>
				Effect.sync(() => {
					flags.set(finalizerFlag, true);
				}),
			);

			return Counter.of({
				Increment: ({ payload }) =>
					Effect.gen(function* () {
						const next = yield* Ref.updateAndGet(
							count,
							(n) => n + payload.amount,
						);
						if (next > 20) {
							return yield* new CounterOverflowError({
								limit: 20,
								message: `count ${next} would exceed limit 20`,
							});
						}
						return next;
					}),
				GetCount: () => Ref.get(count),
				Crash: () => Effect.die("kaboom"),
				EchoDate: ({ payload }) => Effect.succeed(payload.when),
				Tags: ({ payload }) => Effect.succeed(payload.tags.length),
				// Per-handler yield of a non-built-in service. Resolved on
				// every call against the snapshotted Runner context.
				Greet: ({ payload }) =>
					Effect.gen(function* () {
						const g = yield* Greeter;
						return g.greet(payload.name);
					}),
				WakeGreeting: () => Effect.succeed(wakeGreeting),
				// User-defined sub-span. The SDK already wraps the handler
				// in a server-side span; the inner `withSpan("step.double")`
				// nests under it, demonstrating that hand-written spans
				// inside a handler join the caller's trace transparently.
				Compute: ({ payload }) =>
					Effect.succeed(payload.n * 2).pipe(
						Effect.withSpan("step.double"),
					),
				Scale: ({ payload }) =>
					Effect.gen(function* () {
						if (payload.amount > 30) {
							return yield* new ScaledOverflowError({
								limit: 30,
								message: `amount ${payload.amount} would exceed limit 30`,
							});
						}
						// +100 makes the round-trip non-tautological: the
						// test asserts on a value the client never sent, so
						// the success path can't pass without the success
						// and payload codec sites firing on both sides.
						return payload.amount + 100;
					}),
				PersistAndSleep: ({ payload }) =>
					Effect.gen(function* () {
						const { count } = yield* State.updateAndGet(
							state,
							(s) => ({
								...s,
								count: s.count + payload.amount,
							}),
						).pipe(Effect.orDie);
						yield* sleep;
						return count;
					}),
				PersistDateAndSleep: ({ payload }) =>
					Effect.gen(function* () {
						const { when } = yield* State.updateAndGet(
							state,
							(s) => ({
								...s,
								when: payload.when,
							}),
						).pipe(Effect.orDie);
						yield* sleep;
						return when;
					}),
				PersistTagsAndSleep: ({ payload }) =>
					Effect.gen(function* () {
						const { tags } = yield* State.updateAndGet(
							state,
							(s) => ({
								...s,
								tags: payload.tags,
							}),
						).pipe(Effect.orDie);
						yield* sleep;
						return tags;
					}),
				PersistScaledAndSleep: ({ payload }) =>
					Effect.gen(function* () {
						const { scaled } = yield* State.updateAndGet(
							state,
							(s) => ({
								...s,
								scaled: payload.amount,
							}),
						).pipe(Effect.orDie);
						yield* sleep;
						return scaled;
					}),
				GetPersistedState: () => State.get(state).pipe(Effect.orDie),
				// Per-actor SQLite is provisioned via the `db:` option on
				// `Counter.toLayer` below. The build effect destructures `db`
				// from `rawRivetkitContext`, so handlers reach SQLite
				// through the captured client without going through `c.db`.
				LogEvent: ({ payload }) =>
					Effect.tryPromise(async () => {
						await db.execute(
							"INSERT INTO events (event, created_at) VALUES (?, ?)",
							payload.event,
							Date.now(),
						);
						const rows = await db.execute<{ count: number }>(
							"SELECT COUNT(*) as count FROM events",
						);
						return rows[0]?.count ?? 0;
					}).pipe(Effect.orDie),
				ListEvents: () =>
					Effect.tryPromise(async () => {
						const rows = await db.execute<{ event: string }>(
							"SELECT event FROM events ORDER BY id ASC",
						);
						return rows.map((r) => r.event);
					}).pipe(Effect.orDie),
				CountEvents: () =>
					Effect.tryPromise(async () => {
						const rows = await db.execute<{ count: number }>(
							"SELECT COUNT(*) as count FROM events",
						);
						return rows[0]?.count ?? 0;
					}).pipe(Effect.orDie),
				SleepDuringAction: () =>
					Effect.gen(function* () {
						const key = address.key.join("/");
						yield* Effect.sync(() => {
							flags.set(
								`sleep-during-action-started:${key}`,
								true,
							);
						});
						yield* sleep;
						return yield* Effect.never.pipe(
							Effect.onInterrupt(() =>
								Effect.sync(() => {
									flags.set(
										`sleep-during-action-interrupted:${key}`,
										true,
									);
								}),
							),
						);
					}),
			});
		}),
	{
		state: {
			schema: Schema.Struct({
				count: Schema.Number,
				when: Schema.DateFromString,
				tags: TagsCsv,
				// `scaled` is encoded/decoded through `ScaledNumber`, which
				// yields `Multiplier` inside the transform. The Registry's state
				// encode (write) and decode (wake) sites must resolve the
				// service against the snapshotted Runner context, the same way
				// action codec sites do.
				scaled: ScaledNumber,
			}),
			initialValue: () => ({
				count: 0,
				when: new Date(),
				tags: ["default"],
				scaled: 0,
			}),
		},
		// Migration runs once before the wake-scope build effect, so the
		// destructured `db` is already pointed at a migrated database
		// when handlers capture it.
		db: db({
			onMigrate: async (client) => {
				await client.execute(`
					CREATE TABLE IF NOT EXISTS events (
						id INTEGER PRIMARY KEY AUTOINCREMENT,
						event TEXT NOT NULL,
						created_at INTEGER NOT NULL
					)
				`);
			},
		}),
	},
);
// --- ScheduledNoPayload ---

export const ScheduleExpiration = Action.make("ScheduleExpiration", {
	payload: { delay: Schema.Number },
	success: Schema.String,
});

export const Expire = Action.make("Expire");

export const GetExpirationCount = Action.make("GetExpirationCount", {
	success: Schema.Number,
});

export const ScheduledNoPayload = Actor.make("ScheduledNoPayload", {
	actions: [ScheduleExpiration, Expire, GetExpirationCount],
});

export const ScheduledNoPayloadLive = ScheduledNoPayload.toLayer(
	({ rawRivetkitContext }) =>
		Effect.gen(function* () {
			const expirationCount = yield* Ref.make(0);
			const flags = yield* Flags;
			const address = yield* Actor.CurrentAddress;
			const firedFlag = `scheduled-no-payload:${address.key.join("/")}`;

			return ScheduledNoPayload.of({
				ScheduleExpiration: ({ payload }) =>
					Effect.promise(() =>
						rawRivetkitContext.schedule.after(
							payload.delay,
							"Expire",
						),
					),
				Expire: () =>
					Ref.update(expirationCount, (count) => count + 1).pipe(
						Effect.tap(() =>
							Effect.sync(() => {
								flags.set(firedFlag, true);
							}),
						),
						Effect.asVoid,
					),
				GetExpirationCount: () => Ref.get(expirationCount),
			});
		}),
);

// --- Strict ---

// Catches the `SchemaError` from `State.set` and reports the outcome.
// Proves a handler can react to a schema failure that originates inside
// the State layer — the new behavior since `State<A, E, R>` carries `E`.
export const StrictSet = Action.make("StrictSet", {
	payload: { value: Schema.Number },
	success: Schema.Literals(["ok", "rejected"]),
});

// Lets the `SchemaError` propagate. The registry's catch-encode-die
// path converts it to a `RivetError` on the wire — same shape an
// unhandled defect would have produced before this change.
export const StrictSetUnhandled = Action.make("StrictSetUnhandled", {
	payload: { value: Schema.Number },
	success: Schema.Number,
});

export const StrictGet = Action.make("StrictGet", {
	success: Schema.Number,
});

export const Strict = Actor.make("Strict", {
	actions: [StrictSet, StrictSetUnhandled, StrictGet],
});

export const StrictLive = Strict.toLayer(
	({ state }) =>
		Effect.gen(function* () {
			return Strict.of({
				StrictSet: ({ payload }) =>
					State.set(state, payload.value).pipe(
						Effect.match({
							onFailure: () => "rejected" as const,
							onSuccess: () => "ok" as const,
						}),
					),
				StrictSetUnhandled: ({ payload }) =>
					State.set(state, payload.value).pipe(
						Effect.as(payload.value),
						Effect.orDie,
					),
				StrictGet: () => State.get(state).pipe(Effect.orDie),
			});
		}),
	{
		state: {
			// State schema that rejects negative values. Used to exercise the
			// typed-error channel on `State` writes: encoding a negative through
			// `State.set` fails with `SchemaError`, which now flows through the
			// handler effect instead of dying as a defect.
			schema: Schema.Number.pipe(
				Schema.check(Schema.isGreaterThanOrEqualTo(0)),
			),
			initialValue: () => 0,
		},
	},
);

// --- Pinger ---

// Minimal second actor used solely to assert that the registry serves
// more than one actor side-by-side.
export const Ping = Action.make("Ping", { success: Schema.String });

export const Pinger = Actor.make("Pinger", { actions: [Ping] });

export const PingerLive = Pinger.toLayer({
	Ping: () => Effect.succeed("pong"),
});

// --- FailingActor ---

export const FailingActor = Actor.make("FailingBuild", {
	actions: [Ping],
});

export const FailingActorLive = FailingActor.toLayer(
	Effect.die("build effect failed"),
);

// --- FailingWakeCleanup ---

export const FailingWakeCleanup = Actor.make("FailingWakeCleanup", {
	actions: [Ping],
});

export const FailingWakeCleanupLive = FailingWakeCleanup.toLayer(() =>
	Effect.gen(function* () {
		const flags = yield* Flags;
		const address = yield* Actor.CurrentAddress;
		const key = address.key.join("/");

		flags.set(`failed-wake-started:${key}`, true);

		yield* Effect.addFinalizer(() =>
			Effect.sync(() => {
				flags.set(`failed-wake-finalizer:${key}`, true);
			}),
		);

		yield* Effect.never.pipe(
			Effect.onInterrupt(() =>
				Effect.sync(() => {
					flags.set(`failed-wake-fiber-interrupted:${key}`, true);
				}),
			),
			Effect.forkScoped({ startImmediately: true }),
		);

		return yield* Effect.die("wake failed after scoped resources");
	}),
);

// --- Unregistered ---

// Used solely to test the failure shape when calling an actor whose
// `*Live` layer was never provided to the runner. No `UnregisteredLive`
// is exported on purpose — the test relies on this actor being absent
// from the registry at runtime.
export const Echo = Action.make("Echo", { success: Schema.String });

export const Unregistered = Actor.make("Unregistered", { actions: [Echo] });

// --- WakeDecodeFail ---

export const WakeDecodeFail = Actor.make("WakeDecodeFail", {
	actions: [Ping],
});

export const WakeDecodeFailLive = WakeDecodeFail.toLayer(
	() =>
		Effect.gen(function* () {
			return WakeDecodeFail.of({
				Ping: () => Effect.succeed("never reached"),
			});
		}),
	{
		state: {
			// Schema whose encode is permissive (identity) but whose decode rejects
			// negatives. Used to seed invalid persisted actor state so
			// `state` construction rejects on first wake.
			schema: Schema.Number.pipe(
				Schema.decodeTo(
					Schema.Number,
					SchemaTransformation.transformOrFail({
						decode: (n: number) =>
							n >= 0
								? Effect.succeed(n)
								: Effect.fail(
										new SchemaIssue.InvalidValue(
											Option.some(n),
											{
												message:
													"decode rejects negative",
											},
										),
									),
						encode: (n: number) => Effect.succeed(n),
					}),
				),
			),
			// `-1` encodes successfully (encode is identity) so registry setup
			// passes, but the wake-time decode rejects before handlers are built.
			initialValue: () => -1,
		},
	},
);

// --- BuildSetRejected ---

export const BuildOutcome = Action.make("BuildOutcome", {
	success: Schema.Literals(["wrote", "rejected"]),
});

export const BuildSetRejected = Actor.make("BuildSetRejected", {
	actions: [BuildOutcome],
});

export const BuildSetRejectedLive = BuildSetRejected.toLayer(
	({ state }) =>
		Effect.gen(function* () {
			const wrote = yield* State.set(state, -1).pipe(
				Effect.match({
					onFailure: () => false,
					onSuccess: () => true,
				}),
			);
			return BuildSetRejected.of({
				BuildOutcome: () =>
					Effect.succeed(wrote ? "wrote" : "rejected"),
			});
		}),
	{
		state: {
			// Strict schema rejecting negatives on encode. The build effect deliberately
			// calls `State.set` against `state` with a value the schema
			// rejects, catches the resulting `SchemaError` via `Effect.match`, and
			// exposes the outcome via `BuildOutcome`.
			schema: Schema.Number.pipe(
				Schema.check(Schema.isGreaterThanOrEqualTo(0)),
			),
			initialValue: () => 0,
		},
	},
);
