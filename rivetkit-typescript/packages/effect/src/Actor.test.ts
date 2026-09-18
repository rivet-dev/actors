import { assert, describe, it } from "@effect/vitest";
import { Action, Actor, Registry, State } from "@rivetkit/effect";
import { Context, Effect, Layer, Schema } from "effect";
import type * as Rivetkit from "rivetkit";

class Prefix extends Context.Service<Prefix, { readonly value: string }>()(
	"Actor.test/Prefix",
) {}
const PrefixLive = Layer.succeed(Prefix, Prefix.of({ value: "svc" }));

describe("Actor.toWakeHandler", () => {
	it("defaults actions to an empty array", () => {
		const actor = Actor.make("NoActions");

		assert.deepStrictEqual(actor.actions, []);
	});

	it.effect("wraps a plain action handler object", () =>
		Effect.gen(function* () {
			const wake = { Ping: () => Effect.succeed("pong") };
			const wakeHandler = Actor.toWakeHandler(wake);
			const actionHandlers = yield* wakeHandler({} as Actor.WakeOptions);

			assert.strictEqual(actionHandlers, wake);
		}),
	);

	it.effect("runs an Effect that resolves to action handlers", () =>
		Effect.gen(function* () {
			const wake = Effect.gen(function* () {
				const prefix = yield* Prefix;

				return {
					Ping: () => Effect.succeed(`${prefix.value}:pong`),
				};
			});
			const wakeHandler = Actor.toWakeHandler(wake);
			const actionHandlers = yield* wakeHandler({} as Actor.WakeOptions);

			assert.strictEqual(yield* actionHandlers.Ping(), "svc:pong");
		}).pipe(Effect.provide(PrefixLive)),
	);

	it.effect("calls a wake function with wake options", () =>
		Effect.gen(function* () {
			const rawRivetkitContext = {
				key: ["room", "1"],
			} as Rivetkit.WakeContextOf<Rivetkit.AnyActorDefinition>;
			const wakeOptions: Actor.WakeOptions = {
				rawRivetkitContext,
			};
			const wake = (wakeOptions: Actor.WakeOptions) => ({
				GetKey: () =>
					Effect.succeed(
						wakeOptions.rawRivetkitContext.key.join("/"),
					),
			});
			const wakeHandler = Actor.toWakeHandler(wake);
			const actionHandlers = yield* wakeHandler(wakeOptions);

			assert.strictEqual(
				wakeOptions.rawRivetkitContext,
				rawRivetkitContext,
			);
			assert.strictEqual(yield* actionHandlers.GetKey(), "room/1");
		}),
	);

	it.effect("passes actor state through wake options", () =>
		Effect.gen(function* () {
			const cell = { value: { count: 1 } };
			const state = yield* State.make(
				() => Effect.sync(() => cell.value),
				(value: { readonly count: number }) =>
					Effect.sync(() => {
						cell.value = value;
					}),
			);
			type StatefulWakeOptions = Actor.WakeOptions & {
				readonly state: State.State<
					{ readonly count: number },
					never,
					never
				>;
			};
			const wakeOptions: StatefulWakeOptions = {
				rawRivetkitContext:
					{} as Rivetkit.WakeContextOf<Rivetkit.AnyActorDefinition>,
				state,
			};
			const wake = (wakeOptions: StatefulWakeOptions) => ({
				GetCount: () => State.get(wakeOptions.state),
				SetCount: (count: number) =>
					State.set(wakeOptions.state, { count }),
			});
			const wakeHandler = Actor.toWakeHandler(wake);
			const actionHandlers = yield* wakeHandler(wakeOptions);

			assert.deepStrictEqual(yield* actionHandlers.GetCount(), {
				count: 1,
			});

			yield* actionHandlers.SetCount(7);
			assert.deepStrictEqual(cell.value, { count: 7 });
		}),
	);

	it.effect("flattens a wake function returning an Effect", () =>
		Effect.gen(function* () {
			const wakeOptions: Actor.WakeOptions = {
				rawRivetkitContext: {
					key: ["room", "2"],
				} as Rivetkit.WakeContextOf<Rivetkit.AnyActorDefinition>,
			};
			const wake = (options: Actor.WakeOptions) =>
				Effect.gen(function* () {
					const prefix = yield* Prefix;

					return {
						GetKey: () =>
							Effect.succeed(
								`${prefix.value}:${options.rawRivetkitContext.key.join("/")}`,
							),
					};
				});
			const wakeHandler = Actor.toWakeHandler(wake);
			const actionHandlers = yield* wakeHandler(wakeOptions);

			assert.strictEqual(yield* actionHandlers.GetKey(), "svc:room/2");
		}).pipe(Effect.provide(PrefixLive)),
	);

	it.effect("runs an Effect that resolves to a wake function", () =>
		Effect.gen(function* () {
			const wakeOptions: Actor.WakeOptions = {
				rawRivetkitContext: {
					actorId: "actor-1",
				} as Rivetkit.WakeContextOf<Rivetkit.AnyActorDefinition>,
			};
			const wake = Effect.gen(function* () {
				const prefix = yield* Prefix;

				return (options: Actor.WakeOptions) =>
					Effect.succeed({
						GetActorId: () =>
							Effect.succeed(
								`${prefix.value}:${options.rawRivetkitContext.actorId}`,
							),
					});
			});
			const wakeHandler = Actor.toWakeHandler(wake);
			const actionHandlers = yield* wakeHandler(wakeOptions);

			assert.strictEqual(
				yield* actionHandlers.GetActorId(),
				"svc:actor-1",
			);
		}).pipe(Effect.provide(PrefixLive)),
	);

	it.effect("accepts an Effect.fn wake function", () =>
		Effect.gen(function* () {
			const wakeOptions: Actor.WakeOptions = {
				rawRivetkitContext: {
					key: ["effect", "fn"],
				} as Rivetkit.WakeContextOf<Rivetkit.AnyActorDefinition>,
			};
			const wake = Effect.fn("wake")(function* (
				options: Actor.WakeOptions,
			) {
				const prefix = yield* Prefix;

				return {
					GetKey: () =>
						Effect.succeed(
							`${prefix.value}:${options.rawRivetkitContext.key.join("/")}`,
						),
				};
			});
			const wakeHandler = Actor.toWakeHandler(wake);
			const actionHandlers = yield* wakeHandler(wakeOptions);

			assert.strictEqual(yield* actionHandlers.GetKey(), "svc:effect/fn");
		}).pipe(Effect.provide(PrefixLive)),
	);

	it.effect(
		"defers wake functions until the returned handler is invoked",
		() =>
			Effect.gen(function* () {
				let calls = 0;
				const wake = () => {
					calls++;
					return { Count: () => Effect.succeed(calls) };
				};
				const wakeHandler = Actor.toWakeHandler(wake);

				assert.strictEqual(calls, 0);

				const first = yield* wakeHandler({} as Actor.WakeOptions);
				assert.strictEqual(calls, 1);
				assert.strictEqual(yield* first.Count(), 1);

				const second = yield* wakeHandler({} as Actor.WakeOptions);
				assert.strictEqual(calls, 2);
				assert.strictEqual(yield* second.Count(), 2);
			}),
	);
});

describe("Actor.toLayer options", () => {
	const Optioned = Actor.make("Optioned", {
		actions: [Action.make("Noop")],
	});
	const noop = { Noop: () => Effect.void };

	/**
	 * Builds an actor layer against a throwaway registry and hands back
	 * the options the underlying RivetKit actor was actually created
	 * with, after RivetKit has parsed them and filled in its defaults.
	 */
	const builtOptions = (
		actorsLayer: Layer.Layer<never, never, Registry.Registry>,
	) =>
		Effect.gen(function* () {
			const registry = yield* Registry.Registry;
			const built = registry.rivetkitActors.get("Optioned");

			assert.ok(built, "expected Optioned to be registered");

			return built.config.options;
		}).pipe(
			Effect.provide(
				actorsLayer.pipe(
					Layer.provideMerge(Registry.layer({ noWelcome: true })),
				),
			),
		);

	it.effect("forwards instance options to the RivetKit actor", () =>
		Effect.gen(function* () {
			const options = yield* builtOptions(
				Optioned.toLayer(noop, {
					actionTimeout: 1_234,
					sleepTimeout: 5_678,
					maxQueueMessageSize: 128 * 1024,
				}),
			);

			assert.strictEqual(options.actionTimeout, 1_234);
			assert.strictEqual(options.sleepTimeout, 5_678);
			assert.strictEqual(options.maxQueueMessageSize, 128 * 1024);
		}),
	);

	it.effect("keeps forwarding the Inspector display options", () =>
		Effect.gen(function* () {
			const options = yield* builtOptions(
				Optioned.toLayer(noop, { name: "Display", icon: "rocket" }),
			);

			assert.strictEqual(options.name, "Display");
			assert.strictEqual(options.icon, "rocket");
		}),
	);

	it.effect("leaves RivetKit's own defaults in place when unset", () =>
		Effect.gen(function* () {
			const options = yield* builtOptions(Optioned.toLayer(noop, {}));

			assert.isNumber(options.actionTimeout);
			assert.isNumber(options.sleepTimeout);
		}),
	);

	it.effect("does not forward the SDK-only options", () =>
		Effect.gen(function* () {
			const options = yield* builtOptions(
				Optioned.toLayer(noop, {
					state: {
						schema: Schema.Number,
						initialValue: () => 0,
					},
				}),
			);

			assert.isFalse("state" in options);
			assert.isFalse("db" in options);
		}),
	);
});
