import {
	createDispatcherVars,
	startDispatchLoop,
	type DispatchContext,
} from "../../src/actors/dispatcher.ts";
import { migrateWorkflowDb } from "../../src/actors/db.ts";
import { makeCtx, type TestCtx } from "./db.ts";

/**
 * A deterministic stand-in for the dispatcher actor's runtime context. It wires
 * the *real* drain loop (`startDispatchLoop` / `drainDispatchQueue` and the
 * delivery/retry/dead-letter logic) to a node:sqlite-backed queue and a real
 * `setTimeout`-driven schedule, so loop behavior is exercised faithfully without
 * the `setupTest` runner's autonomous-loop provisioning flakiness.
 *
 * `schedule.after` mirrors the actor: it fires a `wake` that clears the
 * coalescing flag and restarts the loop — exactly the production wiring.
 */
export type DispatcherDriver = {
	ctx: DispatchContext;
	/** Simulate an actor crash + restart: drop in-flight timers and rebuild the
	 * context with fresh vars (latch cleared) over the SAME persisted db. */
	restart(): DispatchContext;
	close(): void;
};

export async function makeDispatcherDriver(
	partition: string,
): Promise<DispatcherDriver> {
	const base: TestCtx = await makeCtx(migrateWorkflowDb);
	let timers = new Set<ReturnType<typeof setTimeout>>();

	const build = (): DispatchContext => {
		const ctx = {
			key: [partition],
			db: base.db,
			vars: createDispatcherVars(),
			keepAwake: <T>(p: Promise<T>) => p,
			schedule: {
				after: (delayMs: number, _action: string, ...args: unknown[]) => {
					const t = setTimeout(() => {
						timers.delete(t);
						ctx.vars.wakeArmedAt = null;
						startDispatchLoop(ctx, String(args[0] ?? partition));
					}, delayMs);
					timers.add(t);
					return Promise.resolve();
				},
				at: (timestampMs: number, action: string, ...args: unknown[]) =>
					ctx.schedule.after(Math.max(0, timestampMs - Date.now()), action, ...args),
			},
		} as unknown as DispatchContext;
		return ctx;
	};

	const driver: DispatcherDriver = {
		ctx: build(),
		restart() {
			for (const t of timers) clearTimeout(t);
			timers = new Set();
			driver.ctx = build();
			return driver.ctx;
		},
		close: () => {
			for (const t of timers) clearTimeout(t);
			timers.clear();
			base.close();
		},
	};
	return driver;
}
