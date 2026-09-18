import { assert, layer } from "@effect/vitest";
import { Registry, RivetError } from "@rivetkit/effect";
import { DateTime, Effect, Fiber, Layer, Schedule } from "effect";
import { TestClock } from "effect/testing";
import { createClient } from "rivetkit/client";
import * as RawRivetErrors from "rivetkit/errors";
import { inject } from "vitest";
import {
	BuildSetRejected,
	BuildSetRejectedLive,
	Counter,
	CounterLive,
	CounterOverflowError,
	FailingActor,
	FailingActorLive,
	FailingWakeCleanup,
	FailingWakeCleanupLive,
	Flags,
	Greeter,
	Multiplier,
	Pinger,
	PingerLive,
	ScaledOverflowError,
	ScheduledNoPayload,
	ScheduledNoPayloadLive,
	Strict,
	StrictLive,
	TransformedStateActor,
	TransformedStateActorLive,
	Unregistered,
	WakeDecodeFail,
	WakeDecodeFailLive,
} from "./fixtures/actors";
import { TestTracer } from "./fixtures/tracer";
import { prepareNamespace, waitForEnvoy } from "./shared-engine";

// Each test file talks to the shared engine spawned in globalSetup
// against a unique namespace + runner pool, so envoy registrations
// from prior files (or prior test runs) cannot pollute this file's
// actor routing. The namespace is created and the pool's runner
// config is upserted before `Registry.test` registers the in-process
// envoy at `.start()`.
const { endpoint, token, namespace, poolName } = await prepareNamespace(
	inject("rivetEngine").endpoint,
);

const GreeterLive = Layer.succeed(
	Greeter,
	Greeter.of({
		greet: (name) => `Hello, ${name}!`,
	}),
);

// `Multiplier` has to be in scope on both sides of the wire: the
// `Counter`'s `Scale` action's codec consumes `Action.ServicesServer`
// during registration, and the test body's `Counter.client` getter
// consumes `Action.ServicesClient` for the same action.
// `provideMerge` keeps it as a layer output so the test effect
// itself sees it too.
const MultiplierLive = Layer.succeed(Multiplier, Multiplier.of({ factor: 2 }));

// Block test execution until the in-process envoy has registered
// against the engine's pool view. `rivetkitRegistry.start()` returns
// before that registration round-trip completes, and the first
// action call against an empty pool would otherwise burn the entire
// per-test timeout waiting on the engine.
const ReadyForEnvoy = Layer.effectDiscard(
	Effect.tryPromise(() => waitForEnvoy(endpoint, namespace, poolName)).pipe(
		Effect.orDie,
	),
);

const TestLayer = ReadyForEnvoy.pipe(
	Layer.provideMerge(
		Registry.test.pipe(
			Layer.provideMerge(
				Layer.mergeAll(
					CounterLive,
					PingerLive,
					FailingActorLive,
					FailingWakeCleanupLive,
					StrictLive,
					WakeDecodeFailLive,
					BuildSetRejectedLive,
					TransformedStateActorLive,
					ScheduledNoPayloadLive,
				),
			),
			Layer.provideMerge(Flags.layer),
			Layer.provide(GreeterLive),
			Layer.provideMerge(MultiplierLive),
			Layer.provideMerge(TestTracer.layer()),
			Layer.provide(
				Registry.layer({
					endpoint,
					token,
					namespace,
					sqlite: "remote",
				}),
			),
		),
	),
);

layer(TestLayer)("end-to-end", (it) => {
	it.effect("round-trips an action with payload and success", () =>
		Effect.gen(function* () {
			const counter = (yield* Counter.client).getOrCreate("t-roundtrip");
			assert.strictEqual(yield* counter.Increment({ amount: 5 }), 5);
		}),
	);

	it.effect("preserves in-wake state across calls on the same key", () =>
		Effect.gen(function* () {
			const counter = (yield* Counter.client).getOrCreate(["t-state"]);
			yield* counter.Increment({ amount: 3 });
			yield* counter.Increment({ amount: 4 });
			const total = yield* counter.GetCount();
			assert.strictEqual(total, 7);
		}),
	);

	it.effect(
		"accepts raw client no-arg calls for actions without payloads",
		() =>
			Effect.gen(function* () {
				const client = yield* Effect.acquireRelease(
					Effect.sync(() =>
						createClient({ endpoint, token, namespace }),
					),
					(client) => Effect.promise(() => client.dispose()),
				);
				const counter = client.Counter.getOrCreate("t-raw-no-arg");
				assert.strictEqual(
					yield* Effect.promise(() => counter.GetCount()),
					0,
				);
			}),
	);

	it.effect("runs scheduled actions without payloads", () =>
		Effect.gen(function* () {
			const key = "t-scheduled-no-payload";
			const actor = (yield* ScheduledNoPayload.client).getOrCreate(key);
			const flags = yield* Flags;
			const firedFlag = `scheduled-no-payload:${key}`;

			yield* actor.ScheduleExpiration({ delay: 25 });

			const fired = yield* Effect.sync(() => flags.get(firedFlag)).pipe(
				Effect.repeat({
					until: (value) => value === true,
					schedule: Schedule.spaced("100 millis"),
				}),
				Effect.timeout("5 seconds"),
				TestClock.withLive,
			);
			assert.strictEqual(fired, true);
			assert.strictEqual(yield* actor.GetExpirationCount(), 1);

			const client = yield* Effect.acquireRelease(
				Effect.sync(() => createClient({ endpoint, token, namespace })),
				(client) => Effect.promise(() => client.dispose()),
			);
			const rawActor = client.ScheduledNoPayload.getOrCreate(
				"t-invalid-no-payload",
			);
			const error = yield* Effect.promise(async () => {
				try {
					await Reflect.apply(rawActor.Expire, rawActor, [
						{ unexpected: true },
					]);
					throw new Error("expected arbitrary payload to fail");
				} catch (error) {
					return RawRivetErrors.toRivetError(error);
				}
			});

			assert.strictEqual(error.group, "request");
			assert.strictEqual(error.code, "invalid");
		}),
	);

	it.effect("rejects malformed raw action payloads as request.invalid", () =>
		Effect.gen(function* () {
			const client = yield* Effect.acquireRelease(
				Effect.sync(() => createClient({ endpoint, token, namespace })),
				(client) => Effect.promise(() => client.dispose()),
			);
			const counter = client.Counter.getOrCreate("t-raw-invalid-payload");

			const error = yield* Effect.promise(async () => {
				try {
					await counter.Increment({ amount: "not-a-number" } as any);
					throw new Error("expected malformed payload to fail");
				} catch (error) {
					return RawRivetErrors.toRivetError(error);
				}
			});

			assert.strictEqual(error.group, "request");
			assert.strictEqual(error.code, "invalid");
		}),
	);

	it.effect("isolates in-wake state across keys", () =>
		Effect.gen(function* () {
			const client = yield* Counter.client;
			const a = client.getOrCreate(["t-iso-a"]);
			const b = client.getOrCreate(["t-iso-b"]);
			yield* a.Increment({ amount: 2 });
			yield* a.Increment({ amount: 3 });
			yield* b.Increment({ amount: 1 });
			assert.strictEqual(yield* a.GetCount(), 5);
			assert.strictEqual(yield* b.GetCount(), 1);
		}),
	);

	it.effect("persists state across a sleep/wake cycle", () =>
		Effect.gen(function* () {
			const key = "t-persist-state";
			const counter = (yield* Counter.client).getOrCreate([key]);
			const flags = yield* Flags;
			const flagName = `finalizer:${key}`;

			const beforeSleep = yield* counter.PersistAndSleep({
				amount: 11,
			});
			assert.strictEqual(beforeSleep, 11);

			const finalizerFired = yield* Effect.sync(() =>
				flags.get(flagName),
			).pipe(
				Effect.repeat({
					until: (v) => v === true,
					schedule: Schedule.spaced("100 millis"),
				}),
				TestClock.withLive,
			);
			assert.strictEqual(finalizerFired, true);

			const persistedAfterWake = yield* counter.GetPersistedState();
			assert.strictEqual(persistedAfterWake.count, 11);
		}),
	);

	it.effect("persists state with a non-trivial schema (Date)", () =>
		Effect.gen(function* () {
			const key = "t-persist-state-date";
			const counter = (yield* Counter.client).getOrCreate([key]);
			const flags = yield* Flags;
			const flagName = `finalizer:${key}`;

			const when = new Date("2024-01-15T10:30:00.000Z");
			const beforeSleep = yield* counter.PersistDateAndSleep({
				when,
			});
			assert.strictEqual(beforeSleep.toISOString(), when.toISOString());

			const finalizerFired = yield* Effect.sync(() =>
				flags.get(flagName),
			).pipe(
				Effect.repeat({
					until: (v) => v === true,
					schedule: Schedule.spaced("100 millis"),
				}),
				TestClock.withLive,
			);
			assert.strictEqual(finalizerFired, true);

			const persistedAfterWake = yield* counter.GetPersistedState();
			assert.strictEqual(
				persistedAfterWake.when.toISOString(),
				when.toISOString(),
			);
		}),
	);

	it.effect("persists state with a custom Schema.transform", () =>
		Effect.gen(function* () {
			const key = "t-persist-state-transform";
			const counter = (yield* Counter.client).getOrCreate([key]);
			const flags = yield* Flags;
			const flagName = `finalizer:${key}`;

			const tags = ["alpha", "beta", "gamma"];
			const beforeSleep = yield* counter.PersistTagsAndSleep({
				tags,
			});
			assert.deepEqual(beforeSleep, tags);

			const finalizerFired = yield* Effect.sync(() =>
				flags.get(flagName),
			).pipe(
				Effect.repeat({
					until: (v) => v === true,
					schedule: Schedule.spaced("100 millis"),
				}),
				TestClock.withLive,
			);
			assert.strictEqual(finalizerFired, true);

			const persistedAfterWake = yield* counter.GetPersistedState();
			assert.deepEqual(persistedAfterWake.tags, tags);
		}),
	);

	it.effect("persists state through a service-dependent transform", () =>
		Effect.gen(function* () {
			const key = "t-persist-state-scaled";
			const counter = (yield* Counter.client).getOrCreate([key]);
			const flags = yield* Flags;
			const flagName = `finalizer:${key}`;

			// 14 is the decoded (in-memory) value. With `factor: 2`,
			// the state schema's encode (write) divides 14 -> 7 and
			// its decode (read on wake) multiplies 7 -> 14. Both sites
			// run server-side against the Runner's services snapshot;
			// an unresolved `Multiplier` at either would corrupt the
			// round-trip.
			const beforeSleep = yield* counter.PersistScaledAndSleep({
				amount: 14,
			});
			assert.strictEqual(beforeSleep, 14);

			const finalizerFired = yield* Effect.sync(() =>
				flags.get(flagName),
			).pipe(
				Effect.repeat({
					until: (v) => v === true,
					schedule: Schedule.spaced("100 millis"),
				}),
				TestClock.withLive,
			);
			assert.strictEqual(finalizerFired, true);

			const persistedAfterWake = yield* counter.GetPersistedState();
			assert.strictEqual(persistedAfterWake.scaled, 14);
		}),
	);

	it.effect("handler can catch a State.set schema-encode failure", () =>
		Effect.gen(function* () {
			const strict = (yield* Strict.client).getOrCreate([
				"t-strict-handled",
			]);
			// A passing value writes through and reports "ok".
			assert.strictEqual(yield* strict.StrictSet({ value: 5 }), "ok");
			// A failing value (negative — rejected by the state schema's
			// `isGreaterThanOrEqualTo(0)` check on encode) surfaces as a
			// typed `SchemaError` through `State.set`; the handler
			// catches it via `Effect.match` and reports "rejected".
			// Before `State<A, E, R>` carried `E`, this failure would
			// have died as a defect and the handler had no way to
			// observe it.
			assert.strictEqual(
				yield* strict.StrictSet({ value: -5 }),
				"rejected",
			);
			// And the prior write of 5 stuck (the rejected -5 never
			// touched `c.state`).
			assert.strictEqual(yield* strict.StrictGet(), 5);
		}),
	);

	it.effect(
		"unhandled State.set schema-encode failure surfaces as RivetError",
		() =>
			Effect.gen(function* () {
				const strict = (yield* Strict.client).getOrCreate([
					"t-strict-unhandled",
				]);
				const exit = yield* strict
					.StrictSetUnhandled({ value: -5 })
					.pipe(Effect.flip, Effect.exit);
				assert.isTrue(exit._tag === "Success");
				if (exit._tag === "Success") {
					assert.instanceOf(exit.value, RivetError.RivetError);
				}
			}),
	);

	it.effect(
		"surfaces an expected handler error back into the original error",
		() =>
			Effect.gen(function* () {
				const counter = (yield* Counter.client).getOrCreate([
					"t-expected-error",
				]);
				const exit = yield* counter
					.Increment({ amount: 100 })
					.pipe(Effect.flip, Effect.exit);
				assert.isTrue(exit._tag === "Success");
				if (exit._tag === "Success") {
					assert.instanceOf(exit.value, CounterOverflowError);
					assert.instanceOf(exit.value, Error);
					assert.strictEqual(
						exit.value.constructor,
						CounterOverflowError,
					);
					assert.strictEqual(exit.value._tag, "CounterOverflowError");
					assert.strictEqual(exit.value.limit, 20);
					assert.match(exit.value.message, /exceed limit 20/);
				}
			}),
	);

	it.effect("surfaces an unexpected handler error as a RivetError", () =>
		Effect.gen(function* () {
			const counter = (yield* Counter.client).getOrCreate(["t-boom"]);
			const exit = yield* counter.Crash().pipe(Effect.flip, Effect.exit);
			assert.isTrue(exit._tag === "Success");
			if (exit._tag === "Success") {
				assert.instanceOf(exit.value, RivetError.RivetError);
			}
		}),
	);

	it.effect("interrupts action effects when actor scope closes", () =>
		Effect.gen(function* () {
			const key = "t-action-scope-close";
			const flags = yield* Flags;
			const counter = (yield* Counter.client).getOrCreate([key]);
			const actionFiber = yield* counter
				.SleepDuringAction()
				.pipe(
					Effect.flip,
					Effect.forkChild({ startImmediately: true }),
				);

			const started = yield* Effect.sync(() =>
				flags.get(`sleep-during-action-started:${key}`),
			).pipe(
				Effect.repeat({
					until: (v) => v === true,
					schedule: Schedule.spaced("100 millis"),
				}),
				TestClock.withLive,
			);
			assert.strictEqual(started, true);

			const finalizerFired = yield* Effect.sync(() =>
				flags.get(`finalizer:${key}`),
			).pipe(
				Effect.repeat({
					until: (v) => v === true,
					schedule: Schedule.spaced("100 millis"),
				}),
				TestClock.withLive,
			);
			assert.strictEqual(finalizerFired, true);

			const interrupted = yield* Effect.sync(() =>
				flags.get(`sleep-during-action-interrupted:${key}`),
			).pipe(
				Effect.repeat({
					until: (v) => v === true,
					schedule: Schedule.spaced("100 millis"),
				}),
				TestClock.withLive,
			);
			assert.strictEqual(interrupted, true);

			const error = yield* Fiber.join(actionFiber);
			assert.instanceOf(error, RivetError.RivetError);
			assert.strictEqual(error.group, "actor");
			assert.strictEqual(error.code, "aborted");
		}),
	);

	it.effect("round-trips a non-trivial schema (Date)", () =>
		Effect.gen(function* () {
			const counter = (yield* Counter.client).getOrCreate(["t-date"]);
			const when = new Date("2024-01-15T10:30:00.000Z");
			const result = yield* counter.EchoDate({ when });
			assert.instanceOf(result, Date);
			assert.strictEqual(result.toISOString(), when.toISOString());
		}),
	);

	it.effect("round-trips a custom Schema.transform", () =>
		Effect.gen(function* () {
			const counter = (yield* Counter.client).getOrCreate([
				"t-transform",
			]);
			// `tags` rides the wire as the encoded CSV string and decodes
			// back to a string array on the server. If the transform
			// didn't fire, `payload.tags.length` would be the byte length
			// of the CSV ("alpha,beta,gamma" = 16) instead of 3.
			const count = yield* counter.Tags({
				tags: ["alpha", "beta", "gamma"],
			});
			assert.strictEqual(count, 3);
		}),
	);

	it.effect(
		"exposes transformed actor state as encoded raw wake context state",
		() =>
			Effect.gen(function* () {
				const actor = (yield* TransformedStateActor.client).getOrCreate(
					["t-raw-transformed-state"],
				);
				const when = new Date("2024-04-05T06:07:08.000Z");
				const instant = DateTime.makeUnsafe(1_712_298_428_000);
				const at = new Date("2024-04-06T07:08:09.000Z");
				const bytes = new Uint8Array([9, 8, 7]);
				const payload = new Uint8Array([6, 5, 4]);
				const url = new URL(
					"https://rivet.dev/docs/actors?section=state",
				);
				const id = 9_007_199_254_740_993n;

				yield* actor.SetTransformedStateAndSleep({
					when,
					instant,
					url,
					id,
					bytes,
					tags: ["alpha", "beta", "gamma"],
					history: [{ at, payload }],
				});

				const raw = yield* actor.GetRawWakeState().pipe(
					Effect.repeat({
						until: (state) => state.id === id.toString(),
						schedule: Schedule.spaced("100 millis"),
					}),
					TestClock.withLive,
				);

				assert.deepEqual(raw, {
					when: when.toISOString(),
					instant: DateTime.formatIso(instant),
					url: url.toString(),
					id: id.toString(),
					bytes: Buffer.from(bytes).toString("base64"),
					tags: "alpha,beta,gamma",
					history: [
						{
							at: at.toISOString(),
							payload: Buffer.from(payload).toString("base64"),
						},
					],
				});
			}),
	);

	it.effect(
		"wake options state decodes transformed state written through raw wake context state",
		() =>
			Effect.gen(function* () {
				const actor = (yield* TransformedStateActor.client).getOrCreate(
					["t-raw-set-transformed-state"],
				);
				const when = "2024-05-06T07:08:09.000Z";
				const instant = "2024-05-06T07:08:09.123Z";
				const at = "2024-05-07T08:09:10.000Z";
				const url = "https://rivet.dev/docs/actors/state?source=raw";
				const id = "9007199254740995";
				const bytes = Buffer.from(new Uint8Array([1, 3, 5])).toString(
					"base64",
				);
				const payload = Buffer.from(new Uint8Array([2, 4, 6])).toString(
					"base64",
				);

				yield* actor.SetRawWakeStateAndSleep({
					when,
					instant,
					url,
					id,
					bytes,
					tags: "raw,encoded,state",
					history: [{ at, payload }],
				});

				const decoded = yield* actor.GetDecodedState().pipe(
					Effect.repeat({
						until: (state) => state.id === BigInt(id),
						schedule: Schedule.spaced("100 millis"),
					}),
					TestClock.withLive,
				);

				assert.strictEqual(decoded.when.toISOString(), when);
				assert.strictEqual(
					DateTime.toEpochMillis(decoded.instant),
					Date.parse(instant),
				);
				assert.strictEqual(decoded.url.toString(), url);
				assert.strictEqual(decoded.id, BigInt(id));
				assert.deepEqual(
					Array.from(decoded.bytes),
					Array.from(Buffer.from(bytes, "base64")),
				);
				assert.deepEqual(decoded.tags, ["raw", "encoded", "state"]);
				assert.strictEqual(decoded.history[0]?.at.toISOString(), at);
				assert.deepEqual(
					Array.from(decoded.history[0]?.payload ?? []),
					Array.from(Buffer.from(payload, "base64")),
				);
			}),
	);

	it.effect("resolves a non-built-in service", () =>
		Effect.gen(function* () {
			const counter = (yield* Counter.client).getOrCreate([
				"t-service-wake",
			]);
			// `WakeGreeting` returns the string captured when `Greeter`
			// was resolved inside the wake-scope build effect.
			const greeting = yield* counter.WakeGreeting();
			assert.strictEqual(greeting, "Hello, on wake!");
		}),
	);

	it.effect(
		"resolves a non-built-in service yielded by an action handler",
		() =>
			Effect.gen(function* () {
				const counter = (yield* Counter.client).getOrCreate([
					"t-service-handler",
				]);
				// `Greet`'s handler yields `Greeter` per call; the
				// snapshotted Runner context must satisfy that R.
				const greeting = yield* counter.Greet({ name: "Effect" });
				assert.strictEqual(greeting, "Hello, Effect!");
			}),
	);

	it.effect("registers and serves multiple actors", () =>
		Effect.gen(function* () {
			const counter = (yield* Counter.client).getOrCreate(["t-multi"]);
			const pinger = (yield* Pinger.client).getOrCreate(["t-multi"]);

			const incremented = yield* counter.Increment({ amount: 7 });
			const pong = yield* pinger.Ping();

			assert.strictEqual(incremented, 7);
			assert.strictEqual(pong, "pong");
		}),
	);

	it.effect(
		"surfaces a call to an actor with no registered handler as a RivetError",
		() =>
			Effect.gen(function* () {
				// `Unregistered` is defined in the fixtures module but its
				// `*Live` layer is intentionally not provided, so the engine
				// has no runner that can serve the actor. The engine retries
				// wake until the actor stops before becoming ready and then
				// surfaces the retry-exhaustion guard error.
				const ghost = (yield* Unregistered.client).getOrCreate([
					"t-unregistered",
				]);
				const exit = yield* ghost.Echo().pipe(Effect.flip, Effect.exit);
				assert.isTrue(exit._tag === "Success");
				if (exit._tag === "Success") {
					assert.instanceOf(exit.value, RivetError.RivetError);
					assert.instanceOf(
						exit.value.reason,
						RivetError.GuardActorWakeRetriesExceeded,
					);
					assert.strictEqual(
						(
							exit.value
								.reason as RivetError.GuardActorWakeRetriesExceeded
						).code,
						"actor_wake_retries_exceeded",
					);
				}
			}),
	);

	it.effect("fires the wake-scope finalizer on sleep", () =>
		Effect.gen(function* () {
			const key = "t-wake-finalizer";
			const counter = (yield* Counter.client).getOrCreate([key]);
			// `Flags` is shared across all tests in the suite, so the
			// `Counter` build effect namespaces its finalizer flag by
			// actor key.
			const flagName = `finalizer:${key}`;

			const flags = yield* Flags;
			assert.strictEqual(flags.get(flagName), undefined);

			yield* counter.PersistAndSleep({ amount: 1 });

			// `c.sleep()` is a non-blocking signal: the action returns
			// before the engine tears the wake scope down. Poll the
			// flag until the wake-scope finalizer has run. `TestClock.withLive`
			// swaps in the real Clock so the schedule's interval elapses
			// in wall time (the suite otherwise runs under TestClock).
			const finalizerFired = yield* Effect.sync(() =>
				flags.get(flagName),
			).pipe(
				Effect.repeat({
					until: (v) => v === true,
					schedule: Schedule.spaced("100 millis"),
				}),
				TestClock.withLive,
			);
			assert.strictEqual(finalizerFired, true);
		}),
	);

	it.effect("surfaces an error thrown inside an actor's build effect", () =>
		Effect.gen(function* () {
			// `getOrCreate` only builds a typed proxy on the client and
			// rivetkit's wake is lazy on first action, so the build
			// defect surfaces on `.Ping()`, not here.
			const failing = (yield* FailingActor.client).getOrCreate([
				"t-build-error",
			]);
			const exit = yield* failing.Ping().pipe(Effect.flip, Effect.exit);
			assert.isTrue(exit._tag === "Success");
			if (exit._tag === "Success") {
				assert.instanceOf(exit.value, RivetError.RivetError);
			}
		}),
	);

	it.effect(
		"closes the wake scope when wake fails after registering scoped resources",
		() =>
			Effect.gen(function* () {
				const key = "t-failed-wake-cleanup";
				const flags = yield* Flags;
				const actor = (yield* FailingWakeCleanup.client).getOrCreate([
					key,
				]);

				const exit = yield* actor.Ping().pipe(Effect.flip, Effect.exit);
				assert.isTrue(exit._tag === "Success");
				if (exit._tag === "Success") {
					assert.instanceOf(exit.value, RivetError.RivetError);
				}

				assert.strictEqual(
					flags.get(`failed-wake-started:${key}`),
					true,
				);
				assert.strictEqual(
					flags.get(`failed-wake-finalizer:${key}`),
					true,
				);
				assert.strictEqual(
					flags.get(`failed-wake-fiber-interrupted:${key}`),
					true,
				);
			}),
	);

	it.effect(
		"wake options state decode failure inside build effect surfaces as RivetError",
		() =>
			Effect.gen(function* () {
				const failing = (yield* WakeDecodeFail.client).getOrCreate([
					"t-wake-decode-fail",
				]);
				const exit = yield* failing
					.Ping()
					.pipe(Effect.flip, Effect.exit);
				assert.isTrue(exit._tag === "Success");
				if (exit._tag === "Success") {
					assert.instanceOf(exit.value, RivetError.RivetError);
				}
			}),
	);

	it.effect("build effect can catch a State.set schema-encode failure", () =>
		Effect.gen(function* () {
			const a = (yield* BuildSetRejected.client).getOrCreate([
				"t-build-set-rejected",
			]);
			assert.strictEqual(yield* a.BuildOutcome(), "rejected");
		}),
	);

	it.effect(
		"runs encoding/decoding services for an action's payload, success, and error",
		() =>
			Effect.gen(function* () {
				const counter = (yield* Counter.client).getOrCreate([
					"t-codec-services",
				]);

				// Success path. With `factor: 2` provided on both sides:
				// payload encode 10 -> 5 (client divides), payload decode
				// 5 -> 10 (server multiplies), handler returns 110, success
				// encode 110 -> 55 (server divides), success decode 55 -> 110
				// (client multiplies). A wrong final value would mean one
				// of those four codec sites failed to resolve `Multiplier`.
				assert.strictEqual(yield* counter.Scale({ amount: 10 }), 110);

				// Error path. The handler short-circuits with a
				// `ScaledOverflowError({ limit: 30 })`. The error's `limit`
				// flows through the same service-dependent schema: server
				// encode 30 -> 15, client decode 15 -> 30. A factor mismatch
				// or an unprovided service on either side would surface as
				// a numeric mismatch on `exit.value.limit`.
				const exit = yield* counter
					.Scale({ amount: 40 })
					.pipe(Effect.flip, Effect.exit);
				assert.isTrue(exit._tag === "Success");
				if (exit._tag === "Success") {
					assert.instanceOf(exit.value, ScaledOverflowError);
					assert.strictEqual(exit.value.limit, 30);
					assert.match(exit.value.message, /exceed limit 30/);
				}
			}),
	);

	it.effect("propagates Effect tracing spans end-to-end", () =>
		Effect.gen(function* () {
			const tracer = yield* TestTracer;
			yield* tracer.clear;
			const counter = (yield* Counter.client).getOrCreate(["t-tracing"]);
			// Wrapping the call in `Effect.withSpan("client-call")`
			// makes that span the active parent. The SDK then opens
			// `Counter/Compute` (kind=client) under it, ships the IDs
			// over the wire, and on the server opens another
			// `Counter/Compute` (kind=server) parented to the client
			// span via `externalSpan`. The handler itself wraps its
			// work in `Effect.withSpan("step.double")`, which nests
			// under the SDK's server span — proving user-defined
			// sub-spans join the propagated trace.
			const clientTraceId = yield* Effect.gen(function* () {
				const clientSpan = yield* Effect.currentSpan;
				const doubled = yield* counter.Compute({ n: 21 });
				assert.strictEqual(doubled, 42);
				return clientSpan.traceId;
			}).pipe(Effect.withSpan("client-call"));

			const spans = yield* tracer.spans;
			const onTrace = spans.filter((s) => s.traceId === clientTraceId);
			assert.deepStrictEqual(
				onTrace.map((s) => s.name),
				[
					"client-call",
					"Counter/Compute",
					"Counter/Compute",
					"step.double",
				],
			);
			// Each span (after the root) is parented to the prior one,
			// proving the chain is intact across the wire boundary.
			for (let i = 1; i < onTrace.length; i++) {
				const parent = onTrace[i].parent;
				assert.strictEqual(parent._tag, "Some");
				if (parent._tag === "Some") {
					assert.strictEqual(
						parent.value.spanId,
						onTrace[i - 1].spanId,
					);
				}
			}
		}),
	);

	it.effect("writes through the db captured", () =>
		Effect.gen(function* () {
			const counter = (yield* Counter.client).getOrCreate(["t-db-write"]);
			const afterFirst = yield* counter.LogEvent({ event: "alpha" });
			const afterSecond = yield* counter.LogEvent({ event: "beta" });
			assert.strictEqual(afterFirst, 1);
			assert.strictEqual(afterSecond, 2);
		}),
	);

	it.effect("reads rows back through the captured db", () =>
		Effect.gen(function* () {
			const counter = (yield* Counter.client).getOrCreate(["t-db-list"]);
			yield* counter.LogEvent({ event: "one" });
			yield* counter.LogEvent({ event: "two" });
			yield* counter.LogEvent({ event: "three" });
			const events = yield* counter.ListEvents();
			assert.deepStrictEqual(events, ["one", "two", "three"]);
		}),
	);

	it.effect("isolates db state across actor keys", () =>
		Effect.gen(function* () {
			const client = yield* Counter.client;
			const a = client.getOrCreate(["t-db-iso-a"]);
			const b = client.getOrCreate(["t-db-iso-b"]);
			yield* a.LogEvent({ event: "a1" });
			yield* a.LogEvent({ event: "a2" });
			yield* b.LogEvent({ event: "b1" });
			assert.strictEqual(yield* a.CountEvents(), 2);
			assert.strictEqual(yield* b.CountEvents(), 1);
			assert.deepStrictEqual(yield* a.ListEvents(), ["a1", "a2"]);
			assert.deepStrictEqual(yield* b.ListEvents(), ["b1"]);
		}),
	);

	it.effect("persists db rows across a sleep/wake cycle", () =>
		Effect.gen(function* () {
			const key = "t-db-persist";
			const counter = (yield* Counter.client).getOrCreate([key]);
			yield* counter.LogEvent({ event: "before-sleep" });

			const flags = yield* Flags;
			const flagName = `finalizer:${key}`;

			yield* counter.PersistAndSleep({ amount: 1 });
			const finalizerFired = yield* Effect.sync(() =>
				flags.get(flagName),
			).pipe(
				Effect.repeat({
					until: (v) => v === true,
					schedule: Schedule.spaced("100 millis"),
				}),
				TestClock.withLive,
			);
			assert.strictEqual(finalizerFired, true);

			yield* counter.LogEvent({ event: "after-wake" });
			assert.deepStrictEqual(yield* counter.ListEvents(), [
				"before-sleep",
				"after-wake",
			]);
		}),
	);
});
