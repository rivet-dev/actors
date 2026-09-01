import { beforeEach, describe, expect, it } from "vitest";
import {
	InMemoryDriver,
	Loop,
	loadStorage,
	RollbackCheckpointError,
	runWorkflow,
	type WorkflowContextInterface,
} from "../src/testing.js";

const modes = ["yield", "live"] as const;

for (const mode of modes) {
	describe(`Workflow Engine Rollback (${mode})`, { sequential: true }, () => {
		let driver: InMemoryDriver;

		beforeEach(() => {
			driver = new InMemoryDriver();
			driver.latency = 0;
		});

		it("should execute rollback steps in reverse order", async () => {
			const rollbacks: string[] = [];

			const workflow = async (ctx: WorkflowContextInterface) => {
				await ctx.rollbackCheckpoint("checkpoint");
				await ctx.step({
					name: "first",
					run: async () => "one",
					rollback: async (_ctx, output) => {
						rollbacks.push(`first:${output}`);
					},
				});
				await ctx.step({
					name: "second",
					run: async () => "two",
					rollback: async (_ctx, output) => {
						rollbacks.push(`second:${output}`);
					},
				});
				throw new Error("boom");
			};

			await expect(
				runWorkflow("wf-1", workflow, undefined, driver, { mode })
					.result,
			).rejects.toThrow("boom");

			expect(rollbacks).toEqual(["second:two", "first:one"]);
		});

		it("should error if rollback checkpoint missing", async () => {
			const workflow = async (ctx: WorkflowContextInterface) => {
				await ctx.step({
					name: "missing-checkpoint",
					run: async () => "value",
					rollback: async () => {
						return;
					},
				});
			};

			await expect(
				runWorkflow("wf-1", workflow, undefined, driver, { mode })
					.result,
			).rejects.toThrow(RollbackCheckpointError);
		});

		it("should resume rollback after eviction", async () => {
			const rollbacks: string[] = [];
			let unblockRollback: (() => void) | undefined;
			let startRollback: (() => void) | undefined;

			const rollbackGate = new Promise<void>((resolve) => {
				unblockRollback = () => resolve();
			});
			const rollbackStarted = new Promise<void>((resolve) => {
				startRollback = () => resolve();
			});

			const workflow = async (ctx: WorkflowContextInterface) => {
				await ctx.rollbackCheckpoint("checkpoint");
				await ctx.step({
					name: "first",
					run: async () => "one",
					rollback: async () => {
						rollbacks.push("first");
					},
				});
				await ctx.step({
					name: "second",
					run: async () => "two",
					rollback: async (rollbackCtx) => {
						if (startRollback) {
							startRollback();
						}
						await rollbackGate;
						if (rollbackCtx.abortSignal.aborted) {
							return;
						}
						rollbacks.push("second");
					},
				});
				await ctx.step({
					name: "third",
					run: async () => "three",
					rollback: async () => {
						rollbacks.push("third");
					},
				});
				throw new Error("boom");
			};

			const handle = runWorkflow("wf-1", workflow, undefined, driver, {
				mode,
			});

			await rollbackStarted;
			handle.evict();

			const result = await handle.result;
			expect(result.state).toBe("rolling_back");
			expect(rollbacks).toEqual(["third"]);

			if (unblockRollback) {
				unblockRollback();
			}

			await expect(
				runWorkflow("wf-1", workflow, undefined, driver, { mode })
					.result,
			).rejects.toThrow("boom");

			expect(rollbacks).toEqual(["third", "second", "first"]);
		});

		it("should not persist loop progress while replaying rollback", async () => {
			const rollbacks: string[] = [];
			let shouldFail = true;

			const workflow = async (ctx: WorkflowContextInterface) => {
				await ctx.rollbackCheckpoint("checkpoint");
				await ctx.step({
					name: "outside",
					run: async () => "outside",
					rollback: async () => {
						rollbacks.push("outside");
					},
				});
				await ctx.loop({
					name: "loop",
					state: 0,
					run: async (loopCtx, state) => {
						await loopCtx.step({
							name: `step-${state}`,
							run: async () => `step-${state}`,
							rollback: async () => {
								rollbacks.push(`loop-${state}`);
							},
						});

						if (state === 0) {
							return Loop.continue(1);
						}

						if (shouldFail) {
							shouldFail = false;
							throw new Error("boom");
						}

						return Loop.continue(2);
					},
				});
			};

			await expect(
				runWorkflow("wf-1", workflow, undefined, driver, { mode })
					.result,
			).rejects.toThrow("boom");

			const storage = await loadStorage(driver);
			const loopEntry = [...storage.history.entries.values()].find(
				(entry) => entry.kind.type === "loop",
			);

			expect(loopEntry?.kind.type).toBe("loop");
			if (loopEntry?.kind.type !== "loop") {
				throw new Error("Expected loop entry");
			}

			expect(loopEntry.kind.data.iteration).toBe(1);
			expect(rollbacks).toContain("outside");
		});

		it("should roll back a completed loop's nested steps", async () => {
			// A loop completes with a defined output, then a later step fails and
			// triggers rollback. The completed loop's nested step rollbacks must
			// still be collected, not skipped by the completed-loop early return.
			const rollbacks: string[] = [];

			const workflow = async (ctx: WorkflowContextInterface) => {
				await ctx.rollbackCheckpoint("checkpoint");
				await ctx.loop({
					name: "loop",
					state: 0,
					run: async (loopCtx, state) => {
						await loopCtx.step({
							name: `step-${state}`,
							run: async () => `step-${state}`,
							rollback: async () => {
								rollbacks.push(`loop-${state}`);
							},
						});
						if (state === 0) {
							return Loop.continue(1);
						}
						return Loop.break("loop-done");
					},
				});
				await ctx.step({
					name: "after",
					run: async () => "after",
					rollback: async () => {
						rollbacks.push("after");
					},
				});
				throw new Error("boom");
			};

			await expect(
				runWorkflow("wf-1", workflow, undefined, driver, { mode })
					.result,
			).rejects.toThrow("boom");

			// Control: the post-loop step's rollback proves rollback ran.
			expect(rollbacks).toContain("after");
			// The completed loop's nested step rollbacks must also run.
			expect(rollbacks).toContain("loop-0");
			expect(rollbacks).toContain("loop-1");
		});

		it("should roll back nested steps of a loop that breaks with no output", async () => {
			// Loop.break(undefined) completes the loop with output === undefined;
			// the rollback replay is keyed on completion status, not output, so
			// nested step handlers must still be registered.
			const rollbacks: string[] = [];

			const workflow = async (ctx: WorkflowContextInterface) => {
				await ctx.rollbackCheckpoint("checkpoint");
				await ctx.loop({
					name: "loop",
					state: 0,
					run: async (loopCtx, state) => {
						await loopCtx.step({
							name: `step-${state}`,
							run: async () => `step-${state}`,
							rollback: async () => {
								rollbacks.push(`loop-${state}`);
							},
						});
						if (state === 0) {
							return Loop.continue(1);
						}
						return Loop.break(undefined);
					},
				});
				await ctx.step({
					name: "after",
					run: async () => "after",
					rollback: async () => {
						rollbacks.push("after");
					},
				});
				throw new Error("boom");
			};

			await expect(
				runWorkflow("wf-1", workflow, undefined, driver, { mode })
					.result,
			).rejects.toThrow("boom");

			expect(rollbacks).toContain("after");
			expect(rollbacks).toContain("loop-0");
			expect(rollbacks).toContain("loop-1");
		});

		it("should roll back retained iterations when loop history is pruned", async () => {
			// Bounded loop history prunes early iterations. The rollback replay
			// must start at the first retained iteration instead of aborting on
			// the pruned one, so retained iterations and post-loop steps still
			// register their rollback handlers.
			const rollbacks: string[] = [];

			const workflow = async (ctx: WorkflowContextInterface) => {
				await ctx.rollbackCheckpoint("checkpoint");
				await ctx.loop({
					name: "loop",
					state: 0,
					historyPruneInterval: 1,
					historySize: 1,
					run: async (loopCtx, state) => {
						await loopCtx.step({
							name: `step-${state}`,
							run: async () => `step-${state}`,
							rollback: async () => {
								rollbacks.push(`loop-${state}`);
							},
						});
						if (state === 0) {
							return Loop.continue(1);
						}
						return Loop.break("done");
					},
				});
				await ctx.step({
					name: "after",
					run: async () => "after",
					rollback: async () => {
						rollbacks.push("after");
					},
				});
				throw new Error("boom");
			};

			await expect(
				runWorkflow("wf-1", workflow, undefined, driver, { mode })
					.result,
			).rejects.toThrow("boom");

			// The retained tail iteration and post-loop step roll back; the
			// pruned iteration 0 cannot (its step output is gone).
			expect(rollbacks).toContain("after");
			expect(rollbacks).toContain("loop-1");
			expect(rollbacks).not.toContain("loop-0");
		});
	});
}
