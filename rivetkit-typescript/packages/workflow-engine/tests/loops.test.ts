import { beforeEach, describe, expect, it } from "vitest";
import {
	InMemoryDriver,
	Loop,
	loadStorage,
	runWorkflow,
	type WorkflowContextInterface,
} from "../src/testing.js";

const modes = ["yield", "live"] as const;

function isLoopIteration(segment: unknown): segment is { iteration: number } {
	return (
		typeof segment === "object" &&
		segment !== null &&
		"iteration" in segment
	);
}

for (const mode of modes) {
	describe(`Workflow Engine Loops (${mode})`, { sequential: true }, () => {
		let driver: InMemoryDriver;

		beforeEach(() => {
			driver = new InMemoryDriver();
			driver.latency = 0;
		});

		it("should execute a simple loop", async () => {
			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop({
					name: "count-loop",
					state: { count: 0 },
					run: async (ctx, state) => {
						if (state.count >= 3) {
							return Loop.break(state.count);
						}
						await ctx.step(`step-${state.count}`, async () => {});
						return Loop.continue({ count: state.count + 1 });
					},
				});
			};

			const result = await runWorkflow(
				"wf-1",
				workflow,
				undefined,
				driver,
				{ mode },
			).result;

			expect(result.state).toBe("completed");
			expect(result.output).toBe(3);
		});

		it("should run a stateless loop", async () => {
			let iteration = 0;

			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop("stateless", async () => {
					iteration += 1;
					if (iteration >= 3) {
						return Loop.break("done");
					}
					return Loop.continue(undefined);
				});
			};

			const result = await runWorkflow(
				"wf-1",
				workflow,
				undefined,
				driver,
				{ mode },
			).result;

			expect(result.state).toBe("completed");
			expect(result.output).toBe("done");
		});

		it("should treat undefined return as continue in stateless loops", async () => {
			let iteration = 0;

			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop(
					"stateless-implicit-continue",
					async () => {
						iteration += 1;
						if (iteration >= 3) {
							return Loop.break("done");
						}
						return undefined;
					},
				);
			};

			const result = await runWorkflow(
				"wf-1",
				workflow,
				undefined,
				driver,
				{ mode },
			).result;

			expect(result.state).toBe("completed");
			expect(result.output).toBe("done");
		});

		it("should replay void loop output on restart", async () => {
			let callCount = 0;

			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop("void-output", async () => {
					callCount++;
					return Loop.break(undefined);
				});
			};

			const result1 = await runWorkflow(
				"wf-1",
				workflow,
				undefined,
				driver,
				{ mode },
			).result;

			expect(result1.state).toBe("completed");
			expect(result1.output).toBeUndefined();
			expect(callCount).toBe(1);

			const result2 = await runWorkflow(
				"wf-1",
				workflow,
				undefined,
				driver,
				{ mode },
			).result;

			expect(result2.state).toBe("completed");
			expect(result2.output).toBeUndefined();
			expect(callCount).toBe(1);
		});

		it("should resume nested sub-loops across parent loop iterations", async () => {
			const processed: string[] = [];

			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop("command-loop", async (loopCtx) => {
					const message = await loopCtx.queue.next<{
						items: string[];
					}>("next", {
						names: ["work"],
						completable: true,
					});

					let itemIndex = 0;
					await loopCtx.loop("process-items", async (subLoopCtx) => {
						const item = message.body.items[itemIndex];
						if (item === undefined) {
							return Loop.break(undefined);
						}

						await subLoopCtx.step(
							`process-item-${itemIndex}`,
							async () => {
								processed.push(item);
							},
						);
						itemIndex += 1;
						return Loop.continue(undefined);
					});

					await message.complete?.({ ok: true });

					if (processed.length >= 3) {
						return Loop.break([...processed]);
					}

					return Loop.continue(undefined);
				});
			};

			await driver.messageDriver.addMessage({
				id: "msg-1",
				name: "work",
				data: { items: ["a", "b"] },
				sentAt: Date.now(),
			});

			if (mode === "yield") {
				const firstRun = await runWorkflow(
					"wf-1",
					workflow,
					undefined,
					driver,
					{ mode },
				).result;

				expect(firstRun.state).toBe("sleeping");
				expect(processed).toEqual(["a", "b"]);

				await driver.messageDriver.addMessage({
					id: "msg-2",
					name: "work",
					data: { items: ["c"] },
					sentAt: Date.now(),
				});

				const secondRun = await runWorkflow(
					"wf-1",
					workflow,
					undefined,
					driver,
					{ mode },
				).result;

				expect(secondRun.state).toBe("completed");
				expect(secondRun.output).toEqual(["a", "b", "c"]);
				return;
			}

			const handle = runWorkflow("wf-1", workflow, undefined, driver, {
				mode,
			});

			await new Promise((resolve) => setTimeout(resolve, 10));

			await handle.message("work", { items: ["c"] });

			const result = await handle.result;
			expect(result.state).toBe("completed");
			expect(result.output).toEqual(["a", "b", "c"]);
		});

		it("should resume an inner loop that suspends mid-iteration inside a parent loop", async () => {
			// An inner loop whose body suspends (queue.next with no message)
			// advances past iteration 0, persists its iteration, and resumes on
			// the next run. The inner loop resume path starts at the saved
			// iteration and never re-visits iteration 0, so the enclosing
			// parent-loop branch's validateComplete() must still treat the
			// already-completed inner iterations as accounted for.
			const ticks: number[] = [];

			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop({
					name: "outer",
					state: { done: false },
					run: async (outerCtx, outerState) => {
						if (outerState.done) {
							return Loop.break(ticks.length);
						}

						await outerCtx.loop({
							name: "inner",
							state: { count: 0 },
							run: async (innerCtx, innerState) => {
								if (innerState.count >= 2) {
									return Loop.break(undefined);
								}

								const message = await innerCtx.queue.next<{
									n: number;
								}>("tick", { names: ["tick"] });

								await innerCtx.step(
									`record-${innerState.count}`,
									async () => {
										ticks.push(message.body.n);
									},
								);

								return Loop.continue({
									count: innerState.count + 1,
								});
							},
						});

						return Loop.break(ticks.length);
					},
				});
			};

			if (mode === "yield") {
				await driver.messageDriver.addMessage({
					id: "tick-1",
					name: "tick",
					data: { n: 1 },
					sentAt: Date.now(),
				});

				// First run: inner iteration 0 consumes tick-1 and continues;
				// inner iteration 1 finds no message and suspends, persisting
				// the inner loop at iteration 1.
				const firstRun = await runWorkflow(
					"wf-1",
					workflow,
					undefined,
					driver,
					{ mode },
				).result;

				expect(firstRun.state).toBe("sleeping");
				expect(ticks).toEqual([1]);

				await driver.messageDriver.addMessage({
					id: "tick-2",
					name: "tick",
					data: { n: 2 },
					sentAt: Date.now(),
				});

				// Second run: replay resumes the inner loop at iteration 1
				// (iteration 0 is never re-visited), then the parent branch is
				// validated against the full subtree.
				const secondRun = await runWorkflow(
					"wf-1",
					workflow,
					undefined,
					driver,
					{ mode },
				).result;

				expect(secondRun.state).toBe("completed");
				expect(secondRun.output).toBe(2);
				expect(ticks).toEqual([1, 2]);
				return;
			}

			const handle = runWorkflow("wf-1", workflow, undefined, driver, {
				mode,
			});

			await handle.message("tick", { n: 1 });
			await handle.message("tick", { n: 2 });

			const result = await handle.result;
			expect(result.state).toBe("completed");
			expect(result.output).toBe(2);
			expect(ticks).toEqual([1, 2]);
		});

		it("should resume nested joins across parent loop iterations", async () => {
			const processed: string[] = [];

			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop("command-loop", async (loopCtx) => {
					const message = await loopCtx.queue.next<{
						items: string[];
					}>("next", {
						names: ["work"],
						completable: true,
					});

					const branches = Object.fromEntries(
						message.body.items.map((item, index) => [
							`item-${index}`,
							{
								run: async (
									branchCtx: WorkflowContextInterface,
								) =>
									await branchCtx.step(
										`process-item-${index}`,
										async () => {
											processed.push(item);
											return item;
										},
									),
							},
						]),
					);

					await loopCtx.join("process-items", branches);
					await message.complete?.({ ok: true });

					if (processed.length >= 3) {
						return Loop.break([...processed]);
					}

					return Loop.continue(undefined);
				});
			};

			await driver.messageDriver.addMessage({
				id: "msg-1",
				name: "work",
				data: { items: ["a", "b"] },
				sentAt: Date.now(),
			});

			if (mode === "yield") {
				const firstRun = await runWorkflow(
					"wf-1",
					workflow,
					undefined,
					driver,
					{ mode },
				).result;

				expect(firstRun.state).toBe("sleeping");
				expect(processed).toEqual(["a", "b"]);

				await driver.messageDriver.addMessage({
					id: "msg-2",
					name: "work",
					data: { items: ["c"] },
					sentAt: Date.now(),
				});

				const secondRun = await runWorkflow(
					"wf-1",
					workflow,
					undefined,
					driver,
					{ mode },
				).result;

				expect(secondRun.state).toBe("completed");
				expect(secondRun.output).toEqual(["a", "b", "c"]);
				return;
			}

			const handle = runWorkflow("wf-1", workflow, undefined, driver, {
				mode,
			});

			await new Promise((resolve) => setTimeout(resolve, 10));

			await handle.message("work", { items: ["c"] });

			const result = await handle.result;
			expect(result.state).toBe("completed");
			expect(result.output).toEqual(["a", "b", "c"]);
		});

		it("should resume nested races across parent loop iterations", async () => {
			const processed: string[] = [];

			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop("command-loop", async (loopCtx) => {
					const message = await loopCtx.queue.next<{
						items: string[];
					}>("next", {
						names: ["work"],
						completable: true,
					});

					const nested = await loopCtx.race("process-item", [
						{
							name: "fast",
							run: async (raceCtx) =>
								await raceCtx.step("process-fast", async () => {
									const item = message.body.items[0]!;
									processed.push(item);
									return item;
								}),
						},
						{
							name: "slow",
							run: async (raceCtx) => {
								await new Promise<void>((resolve) => {
									if (raceCtx.abortSignal.aborted) {
										resolve();
										return;
									}
									raceCtx.abortSignal.addEventListener(
										"abort",
										() => resolve(),
										{ once: true },
									);
								});
								return "slow";
							},
						},
					]);

					expect(nested.value).toBe(message.body.items[0]);
					await message.complete?.({ ok: true });

					if (processed.length >= 2) {
						return Loop.break([...processed]);
					}

					return Loop.continue(undefined);
				});
			};

			await driver.messageDriver.addMessage({
				id: "msg-1",
				name: "work",
				data: { items: ["a"] },
				sentAt: Date.now(),
			});

			if (mode === "yield") {
				const firstRun = await runWorkflow(
					"wf-1",
					workflow,
					undefined,
					driver,
					{ mode },
				).result;

				expect(firstRun.state).toBe("sleeping");
				expect(processed).toEqual(["a"]);

				await driver.messageDriver.addMessage({
					id: "msg-2",
					name: "work",
					data: { items: ["b"] },
					sentAt: Date.now(),
				});

				const secondRun = await runWorkflow(
					"wf-1",
					workflow,
					undefined,
					driver,
					{ mode },
				).result;

				expect(secondRun.state).toBe("completed");
				expect(secondRun.output).toEqual(["a", "b"]);
				return;
			}

			const handle = runWorkflow("wf-1", workflow, undefined, driver, {
				mode,
			});

			await new Promise((resolve) => setTimeout(resolve, 10));

			await handle.message("work", { items: ["b"] });

			const result = await handle.result;
			expect(result.state).toBe("completed");
			expect(result.output).toEqual(["a", "b"]);
		});

		it("should resume loop from saved state", async () => {
			let iteration = 0;

			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop({
					name: "resume-loop",
					state: { count: 0 },
					historyPruneInterval: 2,
					run: async (_ctx, state) => {
						iteration++;

						if (state.count >= 5) {
							return Loop.break(state.count);
						}

						if (state.count === 2 && iteration === 3) {
							throw new Error("Simulated crash");
						}

						return Loop.continue({ count: state.count + 1 });
					},
				});
			};

			try {
				await runWorkflow("wf-1", workflow, undefined, driver, { mode })
					.result;
			} catch {}

			iteration = 0;

			const result = await runWorkflow(
				"wf-1",
				workflow,
				undefined,
				driver,
				{ mode },
			).result;

			expect(result.state).toBe("completed");
			expect(result.output).toBe(5);
		});

		it("should prune old iterations at each prune interval", async () => {
			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop({
					name: "cleanup-loop",
					state: { count: 0 },
					historyPruneInterval: 2,
					run: async (ctx, state) => {
						await ctx.step(`step-${state.count}`, async () => {});
						if (state.count >= 4) {
							return Loop.break(state.count);
						}
						return Loop.continue({ count: state.count + 1 });
					},
				});
			};

			await runWorkflow("wf-1", workflow, undefined, driver, { mode })
				.result;

			const storage = await loadStorage(driver);
			const iterations = [...storage.history.entries.values()]
				.flatMap((entry) => entry.location)
				.flatMap((segment) =>
					isLoopIteration(segment) ? [segment] : [],
				)
				.map((segment) => segment.iteration);

			const minIteration = Math.min(...iterations);
			expect(minIteration).toBeGreaterThanOrEqual(2);
		});

		it("should not re-delete already-pruned iterations", async () => {
			let deleteRangeCallCount = 0;
			const originalDeleteRange = driver.deleteRange.bind(driver);
			driver.deleteRange = async (start: Uint8Array, end: Uint8Array) => {
				deleteRangeCallCount++;
				return originalDeleteRange(start, end);
			};

			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop({
					name: "efficient-cleanup",
					state: { count: 0 },
					historyPruneInterval: 3,
					run: async (ctx, state) => {
						await ctx.step(`step-${state.count}`, async () => {});
						if (state.count >= 8) {
							return Loop.break(state.count);
						}
						return Loop.continue({ count: state.count + 1 });
					},
				});
			};

			deleteRangeCallCount = 0;
			await runWorkflow("wf-1", workflow, undefined, driver, { mode })
				.result;

			// With historyPruneInterval=3 and 9 iterations (0-8), prune runs at
			// iterations 3 and 6, plus final break at iteration 8.
			// At prune 3: no deletions (3 - 3 = 0, nothing to delete)
			// At prune 6: one range delete for iterations 0-2
			// At break (iteration 9): one range delete for iterations 3-5
			expect(deleteRangeCallCount).toBe(2);
		});

		it("should prune history on break even without reaching historyPruneInterval", async () => {
			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop({
					name: "break-prune",
					state: { count: 0 },
					historyPruneInterval: 3,
					run: async (ctx, state) => {
						await ctx.step(`step-${state.count}`, async () => {});
						if (state.count >= 5) {
							return Loop.break(state.count);
						}
						return Loop.continue({ count: state.count + 1 });
					},
				});
			};

			await runWorkflow("wf-1", workflow, undefined, driver, { mode })
				.result;

			const storage = await loadStorage(driver);
			const iterations = [...storage.history.entries.values()]
				.flatMap((entry) => entry.location)
				.flatMap((segment) =>
					isLoopIteration(segment) ? [segment] : [],
				)
				.map((segment) => segment.iteration);

			// With historyPruneInterval=3 and break at iteration 5 (6 total iterations):
			// At prune 3: no deletions (3 - 3 = 0)
			// At break (6 iterations total): prune iterations 0-2
			// So iterations 3-5 should remain
			if (iterations.length > 0) {
				const minIteration = Math.min(...iterations);
				expect(minIteration).toBeGreaterThanOrEqual(3);
			}
		});

		it("should resume from saved state after crash in later iteration", async () => {
			let firstRun = true;
			let iterationsExecuted: number[] = [];

			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop({
					name: "deferred-crash",
					state: { count: 0 },
					historyPruneInterval: 2,
					run: async (ctx, state) => {
						iterationsExecuted.push(state.count);

						await ctx.step(
							`step-${state.count}`,
							async () => state.count,
						);

						if (state.count >= 5) {
							return Loop.break(state.count);
						}

						// Crash at iteration 3 during first run. State was
						// persisted at the end of iteration 2 after
						// Loop.continue, so state should be saved.
						if (state.count === 3 && firstRun) {
							throw new Error("Crash after state save");
						}

						return Loop.continue({ count: state.count + 1 });
					},
				});
			};

			try {
				await runWorkflow("wf-1", workflow, undefined, driver, { mode })
					.result;
			} catch {}

			// Reset tracking for second run
			firstRun = false;
			iterationsExecuted = [];

			const result = await runWorkflow(
				"wf-1",
				workflow,
				undefined,
				driver,
				{ mode },
			).result;

			expect(result.state).toBe("completed");
			expect(result.output).toBe(5);

			// Should resume from saved state at iteration 3, not from 0
			expect(iterationsExecuted[0]).toBe(3);
		});

		it("should handle loop that breaks before first prune interval", async () => {
			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop({
					name: "early-break",
					state: { count: 0 },
					historyPruneInterval: 10,
					run: async (ctx, state) => {
						await ctx.step(`step-${state.count}`, async () => {});
						if (state.count >= 2) {
							return Loop.break(state.count);
						}
						return Loop.continue({ count: state.count + 1 });
					},
				});
			};

			const result = await runWorkflow(
				"wf-1",
				workflow,
				undefined,
				driver,
				{ mode },
			).result;

			expect(result.state).toBe("completed");
			expect(result.output).toBe(2);

			// No pruning should occur since we never reached historyPruneInterval
			const storage = await loadStorage(driver);
			const iterations = [...storage.history.entries.values()]
				.flatMap((entry) => entry.location)
				.flatMap((segment) =>
					isLoopIteration(segment) ? [segment] : [],
				)
				.map((segment) => segment.iteration);

			// All iterations should still be present
			expect(iterations).toContain(0);
			expect(iterations).toContain(1);
			expect(iterations).toContain(2);
		});

		it("should propagate loop errors", async () => {
			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop({
					name: "error-loop",
					state: { count: 0 },
					run: async () => {
						throw new Error("loop failure");
					},
				});
			};

			await expect(
				runWorkflow("wf-1", workflow, undefined, driver, { mode })
					.result,
			).rejects.toThrow("loop failure");
		});

		it("should handle historyPruneInterval of 1", async () => {
			const workflow = async (ctx: WorkflowContextInterface) => {
				return await ctx.loop({
					name: "frequent-commit",
					state: { count: 0 },
					historyPruneInterval: 1,
					run: async (ctx, state) => {
						await ctx.step(`step-${state.count}`, async () => {});
						if (state.count >= 3) {
							return Loop.break(state.count);
						}
						return Loop.continue({ count: state.count + 1 });
					},
				});
			};

			const result = await runWorkflow(
				"wf-1",
				workflow,
				undefined,
				driver,
				{ mode },
			).result;

			expect(result.state).toBe("completed");
			expect(result.output).toBe(3);

			// With historyPruneInterval=1, only the most recent iteration should remain
			const storage = await loadStorage(driver);
			const iterations = [...storage.history.entries.values()]
				.flatMap((entry) => entry.location)
				.flatMap((segment) =>
					isLoopIteration(segment) ? [segment] : [],
				)
				.map((segment) => segment.iteration);

			if (iterations.length > 0) {
				const minIteration = Math.min(...iterations);
				expect(minIteration).toBeGreaterThanOrEqual(2);
			}
		});
	});
}
