import { Action, Actor, Client, type State } from "@rivetkit/effect";
import {
	Context,
	Effect,
	type Layer,
	Schema,
	SchemaTransformation,
} from "effect";
import type { RawAccess } from "rivetkit/db";
import { db } from "rivetkit/db";
import { describe, expectTypeOf, it, test } from "@effect/vitest";

class SomeDep extends Context.Service<SomeDep, { readonly x: number }>()(
	"SomeDep",
) {}

const Ping = Action.make("Ping", {
	success: Schema.Number,
	error: Schema.String,
});

const TestActor = Actor.make("TestActor", {
	actions: [Ping],
});

const TestState = {
	schema: Schema.Struct({
		count: Schema.Number,
	}),
	initialValue: () => ({ count: 0 }),
};

const TagsCsv = Schema.String.pipe(
	Schema.decodeTo(
		Schema.Array(Schema.String),
		SchemaTransformation.transform({
			decode: (s: string): ReadonlyArray<string> => s.split(","),
			encode: (arr: ReadonlyArray<string>) => arr.join(","),
		}),
	),
);

const ServiceDependentNumber = Schema.Number.pipe(
	Schema.decodeTo(
		Schema.Number,
		SchemaTransformation.transformOrFail({
			decode: (n: number) =>
				Effect.gen(function* () {
					const dep = yield* SomeDep;
					return n + dep.x;
				}),
			encode: (n: number) =>
				Effect.gen(function* () {
					const dep = yield* SomeDep;
					return n - dep.x;
				}),
		}),
	),
);

class ServiceDependentError extends Schema.TaggedErrorClass<ServiceDependentError>()(
	"ServiceDependentError",
	{
		limit: ServiceDependentNumber,
		message: Schema.String,
	},
) {}

const ServiceDependentAction = Action.make("ServiceDependentAction", {
	payload: { amount: ServiceDependentNumber },
	success: ServiceDependentNumber,
	error: ServiceDependentError,
});

const ServiceDependentActor = Actor.make("ServiceDependentActor", {
	actions: [ServiceDependentAction],
});

const ServiceDependentState = {
	schema: Schema.Struct({
		count: ServiceDependentNumber,
	}),
	initialValue: () => ({ count: 0 }),
};

const TransformedState = {
	schema: Schema.Struct({
		when: Schema.DateFromString,
		url: Schema.URLFromString,
		id: Schema.BigIntFromString,
		bytes: Schema.Uint8ArrayFromBase64,
		tags: TagsCsv,
		history: Schema.Array(
			Schema.Struct({
				at: Schema.DateFromString,
				payload: Schema.Uint8ArrayFromBase64,
			}),
		),
	}),
	initialValue: () => ({
		when: new Date("2024-01-15T10:30:00.000Z"),
		url: new URL("https://rivet.dev/docs"),
		id: 1n,
		bytes: new Uint8Array([1, 2, 3]),
		tags: ["alpha", "beta"],
		history: [
			{
				at: new Date("2024-01-15T10:30:00.000Z"),
				payload: new Uint8Array([4, 5, 6]),
			},
		],
	}),
};

describe("Actor.make", () => {
	test("preserves the name literal", () => {
		expectTypeOf(TestActor.name).toEqualTypeOf<"TestActor">();
	});
});

describe("Actor.make(...).toLayer", () => {
	test("is a function", () => {
		expectTypeOf(TestActor.toLayer).toBeFunction();
	});

	test("accepts a plain action handlers object", () => {
		expectTypeOf(TestActor.toLayer).toBeCallableWith({
			Ping: () => Effect.succeed(0),
		});
	});

	test("accepts an effect of action handlers", () => {
		expectTypeOf(TestActor.toLayer).toBeCallableWith(
			Effect.gen(function* () {
				return {
					Ping: () => Effect.succeed(0),
				};
			}),
		);
	});

	test("accepts a function returning a plain action handlers object", () => {
		expectTypeOf(TestActor.toLayer).toBeCallableWith(
			(_wakeOptions: any) => ({
				Ping: () => Effect.succeed(0),
			}),
		);
	});

	test("wake options omit state without a configured state type", () => {
		TestActor.toLayer((wakeOptions) => {
			// @ts-expect-error: stateless actors do not expose wakeOptions.state
			wakeOptions.state;
			expectTypeOf(
				wakeOptions.rawRivetkitContext.state,
			).toEqualTypeOf<never>();

			return {
				Ping: () => Effect.succeed(0),
			};
		});

		TestActor.toLayer((wakeOptions) => {
			// @ts-expect-error: actors without a state option do not expose wakeOptions.state
			wakeOptions.state;

			expectTypeOf(
				wakeOptions.rawRivetkitContext.state,
			).toEqualTypeOf<never>();

			return {
				Ping: () => Effect.succeed(0),
			};
		}, {});
	});

	test("wake options carry the configured state type", () => {
		TestActor.toLayer(
			(wakeOptions) => {
				expectTypeOf(wakeOptions.state).toEqualTypeOf<
					State.State<{ readonly count: number }, Schema.SchemaError>
				>();

				return {
					Ping: () => Effect.succeed(0),
				};
			},
			{ state: TestState },
		);
	});

	test("state schema services surface in State effects and actor layers", () => {
		const layer = TestActor.toLayer(
			(wakeOptions) => {
				type StateGetServices = Effect.Services<
					typeof wakeOptions.state.get
				>;

				expectTypeOf<SomeDep>().toExtend<StateGetServices>();

				return {
					Ping: () =>
						wakeOptions.state.get.pipe(
							Effect.map((current) => current.count),
							Effect.orDie,
						),
				};
			},
			{ state: ServiceDependentState },
		);
		type LayerServices = Layer.Services<typeof layer>;
		type LayerHasSomeDep = SomeDep extends LayerServices ? true : false;

		expectTypeOf<LayerHasSomeDep>().toEqualTypeOf<true>();
	});

	test("wake options carry the transformed state type", () => {
		TestActor.toLayer(
			(wakeOptions) => {
				expectTypeOf(wakeOptions.state).toEqualTypeOf<
					State.State<
						{
							readonly when: Date;
							readonly url: URL;
							readonly id: bigint;
							readonly bytes: Uint8Array;
							readonly tags: ReadonlyArray<string>;
							readonly history: ReadonlyArray<{
								readonly at: Date;
								readonly payload: Uint8Array;
							}>;
						},
						Schema.SchemaError
					>
				>();

				return {
					Ping: () => Effect.succeed(0),
				};
			},
			{ state: TransformedState },
		);
	});

	test("wake options carry the raw RivetKit context with the encoded configured state type", () => {
		TestActor.toLayer(
			(wakeOptions) => {
				expectTypeOf(
					wakeOptions.rawRivetkitContext.state,
				).toEqualTypeOf<{ readonly count: number }>();

				return {
					Ping: () => Effect.succeed(0),
				};
			},
			{ state: TestState },
		);
	});

	test("wake options carry the raw RivetKit context with the encoded transformed state type", () => {
		TestActor.toLayer(
			(wakeOptions) => {
				expectTypeOf(
					wakeOptions.rawRivetkitContext.state,
				).toEqualTypeOf<{
					readonly when: string;
					readonly url: string;
					readonly id: string;
					readonly bytes: string;
					readonly tags: string;
					readonly history: ReadonlyArray<{
						readonly at: string;
						readonly payload: string;
					}>;
				}>();

				return {
					Ping: () => Effect.succeed(0),
				};
			},
			{ state: TransformedState },
		);
	});

	test("wake options carry the configured database client type", () => {
		TestActor.toLayer(
			(wakeOptions) => {
				expectTypeOf(
					wakeOptions.rawRivetkitContext.db,
				).toEqualTypeOf<RawAccess>();
				expectTypeOf(
					wakeOptions.rawRivetkitContext.db.transaction(
						async () => {},
						{ name: "effect-operation", timeout: 1_000 },
					),
				).toEqualTypeOf<Promise<void>>();

				return {
					Ping: () => Effect.succeed(0),
				};
			},
			{ db: db() },
		);
	});

	test("accepts a function returning an effect of action handlers", () => {
		expectTypeOf(TestActor.toLayer).toBeCallableWith((_wakeOptions: any) =>
			Effect.gen(function* () {
				return {
					Ping: () => Effect.succeed(0),
				};
			}),
		);
	});

	test("accepts an effect that resolves to a wake function", () => {
		expectTypeOf(TestActor.toLayer).toBeCallableWith(
			Effect.gen(function* () {
				// Allow for initialization logic before the per-entity wake function is called

				return (_wakeOptions: any) =>
					Effect.gen(function* () {
						return {
							Ping: () => Effect.succeed(0),
						};
					});
			}),
		);
	});

	test("accepts an Effect.fn returning action handlers", () => {
		expectTypeOf(TestActor.toLayer).toBeCallableWith(
			Effect.fn("wake")(function* (_wakeOptions) {
				return {
					Ping: () => Effect.succeed(0),
				};
			}),
		);
	});

	test("returns a Layer", () => {
		expectTypeOf(TestActor.toLayer).returns.toExtend<Layer.Any>();
	});

	test("action handler's envelope is typed against the action", () => {
		TestActor.toLayer({
			Ping: (envelope) => {
				expectTypeOf(envelope._tag).toEqualTypeOf<"Ping">();
				expectTypeOf(envelope.action).toExtend<Action.Any>();
				return Effect.succeed(0);
			},
		});
	});

	test("action handler return success is type checked", () => {
		// Plain action handlers object.
		expectTypeOf(TestActor.toLayer).toBeCallableWith({
			Ping: () => Effect.succeed(0),
		});

		TestActor.toLayer({
			// @ts-expect-error: Ping must return the declared number success type.
			Ping: () => Effect.succeed("not a number"),
		});

		// Effect of action handlers.
		expectTypeOf(TestActor.toLayer).toBeCallableWith(
			Effect.gen(function* () {
				return {
					Ping: () => Effect.succeed(0),
				};
			}),
		);

		TestActor.toLayer(
			// @ts-expect-error: Ping must return the declared number success type.
			Effect.gen(function* () {
				return {
					Ping: () => Effect.succeed("not a number"),
				};
			}),
		);

		// Function returning a plain action handlers object.
		expectTypeOf(TestActor.toLayer).toBeCallableWith(() => ({
			Ping: () => Effect.succeed(0),
		}));

		// @ts-expect-error: Ping must return the declared number success type.
		TestActor.toLayer(() => ({
			Ping: () => Effect.succeed("not a number"),
		}));

		// Function returning an effect of action handlers.
		expectTypeOf(TestActor.toLayer).toBeCallableWith(() =>
			Effect.gen(function* () {
				return {
					Ping: () => Effect.succeed(0),
				};
			}),
		);

		// @ts-expect-error: Ping must return the declared number success type.
		TestActor.toLayer(() =>
			Effect.gen(function* () {
				return {
					Ping: () => Effect.succeed("not a number"),
				};
			}),
		);

		// Effect that resolves to a wake function.
		expectTypeOf(TestActor.toLayer).toBeCallableWith(
			Effect.gen(function* () {
				return () => ({
					Ping: () => Effect.succeed(0),
				});
			}),
		);

		TestActor.toLayer(
			// @ts-expect-error: Ping must return the declared number success type.
			Effect.gen(function* () {
				return () => ({
					Ping: () => Effect.succeed("not a number"),
				});
			}),
		);

		// Effect.fn returning action handlers.
		expectTypeOf(TestActor.toLayer).toBeCallableWith(
			Effect.fn("wake")(function* () {
				return {
					Ping: () => Effect.succeed(0),
				};
			}),
		);

		TestActor.toLayer(
			// @ts-expect-error: Ping must return the declared number success type.
			Effect.fn("wake")(function* () {
				return {
					Ping: () => Effect.succeed("not a number"),
				};
			}),
		);
	});

	test("action handler return error is type checked", () => {
		// Plain action handlers object.
		expectTypeOf(TestActor.toLayer).toBeCallableWith({
			Ping: () => Effect.succeed(0),
		});

		TestActor.toLayer({
			// @ts-expect-error: Ping can only fail with its declared action error type.
			// @effect-diagnostics effect/missingEffectError:off
			Ping: () => Effect.fail(1),
		});

		// Effect of action handlers.
		expectTypeOf(TestActor.toLayer).toBeCallableWith(
			Effect.gen(function* () {
				return {
					Ping: () => Effect.succeed(0),
				};
			}),
		);

		TestActor.toLayer(
			// @ts-expect-error: Ping can only fail with its declared action error type.
			Effect.gen(function* () {
				return {
					// @effect-diagnostics effect/missingEffectError:off
					Ping: () => Effect.fail(1),
				};
			}),
		);

		// Function returning a plain action handlers object.
		expectTypeOf(TestActor.toLayer).toBeCallableWith(() => ({
			Ping: () => Effect.succeed(0),
		}));

		// @ts-expect-error: Ping can only fail with its declared action error type.
		TestActor.toLayer(() => ({
			// @effect-diagnostics effect/missingEffectError:off
			Ping: () => Effect.fail(1),
		}));

		// Function returning an effect of action handlers.
		expectTypeOf(TestActor.toLayer).toBeCallableWith(() =>
			Effect.gen(function* () {
				return {
					Ping: () => Effect.succeed(0),
				};
			}),
		);

		// @ts-expect-error: Ping can only fail with its declared action error type.
		TestActor.toLayer(() =>
			Effect.gen(function* () {
				return {
					// @effect-diagnostics effect/missingEffectError:off
					Ping: () => Effect.fail(1),
				};
			}),
		);

		// Effect that resolves to a wake function.
		expectTypeOf(TestActor.toLayer).toBeCallableWith(
			Effect.gen(function* () {
				return () => ({
					Ping: () => Effect.succeed(0),
				});
			}),
		);

		TestActor.toLayer(
			// @ts-expect-error: Ping can only fail with its declared action error type.
			Effect.gen(function* () {
				return () => ({
					// @effect-diagnostics effect/missingEffectError:off
					Ping: () => Effect.fail(1),
				});
			}),
		);

		// Effect.fn returning action handlers.
		expectTypeOf(TestActor.toLayer).toBeCallableWith(
			Effect.fn("wake")(function* () {
				return {
					Ping: () => Effect.succeed(0),
				};
			}),
		);

		TestActor.toLayer(
			// @ts-expect-error: Ping can only fail with its declared action error type.
			Effect.fn("wake")(function* () {
				return {
					// @effect-diagnostics effect/missingEffectError:off
					Ping: () => Effect.fail(1),
				};
			}),
		);
	});

	test("missing action handler is rejected", () => {
		// @ts-expect-error: Ping handler is required
		TestActor.toLayer({});
	});

	test.todo("unknown action handler key is rejected", () => {
		TestActor.toLayer({
			Ping: () => Effect.succeed(0),
			// TODO: toLayer should reject unknown action handler keys
			Unknown: () => Effect.void,
		});
	});

	test.todo("wake-effect requirements surface in the Layer", () => {
		const layer = TestActor.toLayer(
			Effect.gen(function* () {
				yield* SomeDep;
				return { Ping: () => Effect.succeed(0) };
			}),
		);
		type Reqs =
			typeof layer extends Layer.Layer<any, any, infer R> ? R : never;
		// @ts-expect-error: TODO - expectTypeOf<T>() no-arg generic form not resolving
		expectTypeOf<SomeDep>().toExtend<Reqs>();
	});
});

describe("Actor.make(...).of", () => {
	test("preserves the action handlers object type", () => {
		const handlers = {
			Ping: () => Effect.succeed(0),
		};

		expectTypeOf(TestActor.of(handlers)).toEqualTypeOf<typeof handlers>();
	});

	test("action handler's envelope is typed against the action", () => {
		TestActor.of({
			Ping: (envelope) => {
				expectTypeOf(envelope._tag).toEqualTypeOf<"Ping">();
				expectTypeOf(envelope.action).toEqualTypeOf<typeof Ping>();
				return Effect.succeed(0);
			},
		});
	});

	test("action handler return success is type checked", () => {
		expectTypeOf(TestActor.of).toBeCallableWith({
			Ping: () => Effect.succeed(0),
		});

		TestActor.of({
			// @ts-expect-error: Ping must return the declared number success type.
			Ping: () => Effect.succeed("not a number"),
		});
	});

	test("action handler return error is type checked", () => {
		expectTypeOf(TestActor.of).toBeCallableWith({
			Ping: () => Effect.succeed(0),
		});

		TestActor.of({
			// @ts-expect-error: Ping can only fail with its declared action error type.
			// @effect-diagnostics effect/missingEffectError:off
			Ping: () => Effect.fail(1),
		});
	});
});

describe("Actor.make(...).client", () => {
	test("yields a typed Accessor", () => {
		expectTypeOf(TestActor.client).toEqualTypeOf<
			Effect.Effect<
				Actor.Accessor<(typeof TestActor.actions)[number]>,
				never,
				Client.Client
			>
		>();
	});

	it.effect("handle calls require client-side schema services", () =>
		Effect.gen(function* () {
			const actor = (yield* ServiceDependentActor.client).getOrCreate(
				"t-service-dependent",
			);
			const actionEffect = actor.ServiceDependentAction({ amount: 10 });
			type ActionClientServices = Effect.Services<typeof actionEffect>;

			expectTypeOf<SomeDep>().toExtend<ActionClientServices>();
		}).pipe(Effect.provide(Client.layer())),
	);
});
