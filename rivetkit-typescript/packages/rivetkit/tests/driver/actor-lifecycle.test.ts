import { describe, expect, test, vi } from "vitest";
import { describeDriverMatrix } from "./shared-matrix";
import { setupDriverTest } from "./shared-utils";

const LIFECYCLE_RACE_TIMEOUT_MS = 60_000;

describeDriverMatrix("Actor Lifecycle", (driverTestConfig) => {
	describe.sequential("Actor Lifecycle Tests", () => {
		test("actor stop during start waits for start to complete", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);

			const actorKey = `test-stop-during-start-${Date.now()}`;

			// Create actor - this starts the actor
			const actor = client.startStopRaceActor.getOrCreate([actorKey]);

			// Immediately try to call an action and then destroy
			// This creates a race where the actor might not be fully started yet
			const pingPromise = actor.ping();

			// Get actor ID
			const actorId = await actor.resolve();

			// Destroy immediately while start might still be in progress
			await actor.destroy();

			// The ping should still complete successfully because destroy waits for start
			const result = await pingPromise;
			expect(result).toBe("pong");

			// Verify actor was actually destroyed
			let destroyed = false;
			try {
				await client.startStopRaceActor.getForId(actorId).ping();
			} catch (err: any) {
				destroyed = true;
				expect(err.group).toBe("actor");
				expect(err.code).toBe("not_found");
			}
			expect(destroyed).toBe(true);
		});

		test(
			"actor stop before actor instantiation completes cleans up handler",
			async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);

				const actorKey = `test-stop-before-instantiation-${Date.now()}`;

				// Create multiple actors rapidly to increase chance of race
				const actors = Array.from({ length: 5 }, (_, i) =>
					client.startStopRaceActor.getOrCreate([`${actorKey}-${i}`]),
				);

				// Resolve all actor IDs (this triggers start)
				const ids = await Promise.all(actors.map((a) => a.resolve()));

				// Immediately destroy all actors
				await Promise.all(actors.map((a) => a.destroy()));

				// Verify all actors were cleaned up
				for (const id of ids) {
					let destroyed = false;
					try {
						await client.startStopRaceActor.getForId(id).ping();
					} catch (err: any) {
						destroyed = true;
						expect(err.group).toBe("actor");
						expect(err.code).toBe("not_found");
					}
					expect(destroyed, `actor ${id} should be destroyed`).toBe(
						true,
					);
				}
			},
			LIFECYCLE_RACE_TIMEOUT_MS,
		);

		test("onBeforeActorStart completes before stop proceeds", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);

			const actorKey = `test-before-actor-start-${Date.now()}`;

			// Create actor
			const actor = client.startStopRaceActor.getOrCreate([actorKey]);

			// Call action to ensure actor is starting
			const statePromise = actor.getState();

			// Destroy immediately
			await actor.destroy();

			// State should be initialized because onBeforeActorStart must complete
			const state = await statePromise;
			expect(state.initialized).toBe(true);
			expect(state.startCompleted).toBe(true);
		});

		test(
			"multiple rapid create/destroy cycles handle race correctly",
			async (c) => {
				const { client } = await setupDriverTest(c, driverTestConfig);
				let cleanedUpActors = 0;

				// Perform multiple rapid create/destroy cycles
				for (let i = 0; i < 10; i++) {
					const actorKey = `test-rapid-cycle-${Date.now()}-${i}`;
					const actor = client.startStopRaceActor.getOrCreate([
						actorKey,
					]);

					// Resolve one identity before destroying it. Racing two independent
					// get-or-create queries can create a replacement after the first dies,
					// which tests query routing instead of this actor's startup/stop race.
					const actorId = await actor.resolve();
					await client.startStopRaceActor.getForId(actorId).destroy();
					await expect(
						client.startStopRaceActor.getForId(actorId).ping(),
					).rejects.toMatchObject({
						group: "actor",
						code: "not_found",
					});
					cleanedUpActors += 1;
				}

				// The race is only handled correctly if every rapid cycle tears the actor down completely.
				expect(cleanedUpActors).toBe(10);
			},
			LIFECYCLE_RACE_TIMEOUT_MS,
		);

		test("actor stop called with no actor instance cleans up handler", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);

			const actorKey = `test-cleanup-no-instance-${Date.now()}`;

			// Create and immediately destroy
			const actor = client.startStopRaceActor.getOrCreate([actorKey]);
			await actor.resolve();
			await actor.destroy();

			// Try to recreate with same key - should work without issues
			const newActor = client.startStopRaceActor.getOrCreate([actorKey]);
			const result = await newActor.ping();
			expect(result).toBe("pong");

			// Clean up
			await newActor.destroy();
		});

		test("run-closure-self-initiated-sleep runs onSleep before wake", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const actor = client.runSelfInitiatedSleep.getOrCreate([
				`run-self-sleep-${Date.now()}`,
			]);

			// Poll until the actor has slept, restarted, and exposed the persisted hook state.
			await vi.waitFor(
				async () => {
					const state = await actor.getState();
					expect(state.sleepCount).toBe(1);
					expect(state.marker).toBe("slept");
					expect(state.wakeCount).toBeGreaterThanOrEqual(2);
					expect(state.runCount).toBeGreaterThanOrEqual(2);
				},
				{ timeout: 10_000 },
			);
		});

		test("clean run exit still runs onSleep when stop arrives later", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const observer = client.lifecycleObserver.getOrCreate([
				"run-with-early-exit",
			]);
			await observer.clearEvents();

			const actor = client.runWithEarlyExit.getOrCreate([
				`run-clean-exit-sleep-${Date.now()}`,
			]);
			const actorId = await actor.resolve();

			// Poll until the run handler exits cleanly before the actor reaches its idle sleep path.
			await vi.waitFor(
				async () => {
					const state = await actor.getState();
					expect(state.runStarted).toBe(true);
					expect(state.runExited).toBe(true);
				},
				{ timeout: 5_000 },
			);

			// Poll until the delayed sleep stop runs the hook and a later wake exposes the persisted counts.
			await vi.waitFor(
				async () => {
					const events = await observer.getEvents();
					expect(
						events.filter(
							(event) =>
								event.actorKey === actorId &&
								event.event === "sleep",
						),
					).toHaveLength(1);

					const state = await actor.getState();
					expect(state.sleepCount).toBe(1);
					expect(state.destroyCalled).toBe(false);
					expect(state.wakeCount).toBeGreaterThanOrEqual(2);
				},
				{ timeout: 10_000 },
			);
		});

		test("run-closure-self-initiated-destroy terminates actor", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const observer = client.lifecycleObserver.getOrCreate([
				"self-initiated-destroy",
			]);
			await observer.clearEvents();

			const actorKey = `run-self-destroy-${Date.now()}`;
			const actor = client.runSelfInitiatedDestroy.getOrCreate([
				actorKey,
			]);
			const actorId = await actor.resolve();

			// Poll until the observer records the destroy hook for this actor instance.
			await vi.waitFor(
				async () => {
					const events = await observer.getEvents();
					expect(
						events.some(
							(event) =>
								event.actorKey === actorId &&
								event.event === "destroy",
						),
					).toBe(true);
				},
				{ timeout: 5_000 },
			);

			// Poll until registry teardown finishes and the actor ID is no longer resolvable.
			await vi.waitFor(
				async () => {
					await expect(
						client.runSelfInitiatedDestroy
							.getForId(actorId)
							.getState(),
					).rejects.toMatchObject({
						group: "actor",
						code: "not_found",
					});
				},
				{ timeout: 5_000 },
			);
		});

		test("clean run exit still runs onDestroy when stop arrives later", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const observer = client.lifecycleObserver.getOrCreate([
				"run-with-early-exit",
			]);
			await observer.clearEvents();

			const actor = client.runWithEarlyExit.getOrCreate([
				`run-clean-exit-destroy-${Date.now()}`,
			]);
			const actorId = await actor.resolve();

			// Poll until the run handler has returned so the destroy exercises the delayed Stop path.
			await vi.waitFor(
				async () => {
					const state = await actor.getState();
					expect(state.runStarted).toBe(true);
					expect(state.runExited).toBe(true);
				},
				{ timeout: 5_000 },
			);

			await actor.destroy();

			// Poll until the destroy hook is observed because registry teardown races the test process.
			await vi.waitFor(
				async () => {
					const events = await observer.getEvents();
					expect(
						events.filter(
							(event) =>
								event.actorKey === actorId &&
								event.event === "destroy",
						),
					).toHaveLength(1);
				},
				{ timeout: 5_000 },
			);

			await expect(
				client.runWithEarlyExit.getForId(actorId).getState(),
			).rejects.toMatchObject({
				group: "actor",
				code: "not_found",
			});
		});

		test("sleepGracePeriod bounds run handler shutdown on destroy", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);
			const actor = client.runIgnoresAbortStopTimeout.getOrCreate([
				`run-destroy-timeout-${Date.now()}`,
			]);

			const state = await actor.getState();
			expect(state.wakeCount).toBe(1);

			const startedAt = performance.now();
			await actor.destroy();
			const elapsedMs = performance.now() - startedAt;

			expect(elapsedMs).toBeLessThan(1_500);
		}, 10_000);

		test("onDestroy is called even when actor is destroyed during start", async (c) => {
			const { client } = await setupDriverTest(c, driverTestConfig);

			const actorKey = `test-ondestroy-during-start-${Date.now()}`;

			// Create actor
			const actor = client.startStopRaceActor.getOrCreate([actorKey]);

			// Start and immediately destroy
			const statePromise = actor.getState();
			await actor.destroy();

			const state = await statePromise;
			expect(state.startCompleted).toBe(true);

			const observer = client.lifecycleObserver.getOrCreate(["observer"]);
			// The destroy action requests shutdown; its reply does not wait for
			// onDestroy's separate observer request to complete.
			await vi.waitFor(
				async () => {
					const events = await observer.getEvents();
					expect(events).toContainEqual(
						expect.objectContaining({ actorKey, event: "destroy" }),
					);
				},
				{ timeout: 10_000 },
			);
		});
	});
});
