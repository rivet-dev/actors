import { actor } from "rivetkit";
import { hookTokenDb } from "./db.js";
import { one } from "./shared.js";

export const PENDING_RESERVATION_TTL_MS = 60_000;

type HookTokenContext = {
	key: readonly unknown[];
	db: {
		execute(
			sql: string,
			...args: unknown[]
		): Promise<Record<string, unknown>[]>;
		transaction<T>(
			callback: (db: HookTokenContext["db"]) => Promise<T> | T,
		): Promise<T>;
	};
	client<T>(): T;
};

type HookTokenRow = {
	generation: number;
	runId?: string;
	hookId?: string;
	status: "empty" | "pending" | "confirmed";
	createdAt?: number;
	updatedAt: number;
	expiresAt?: number;
};

function assertTokenKey(c: { key: readonly unknown[] }, token: string) {
	if (String(c.key[0] ?? "") !== token) {
		throw new Error("hookToken actor key must match token");
	}
}

async function readState(c: { db: HookTokenContext["db"] }): Promise<HookTokenRow> {
	const row = await one(
		c,
		`SELECT generation, run_id, hook_id, status, created_at, updated_at, expires_at
		 FROM hook_token_state WHERE singleton = 1`,
	);
	if (!row) throw new Error("hookToken state is missing");
	return {
		generation: Number(row.generation),
		runId: row.run_id == null ? undefined : String(row.run_id),
		hookId: row.hook_id == null ? undefined : String(row.hook_id),
		status: String(row.status) as HookTokenRow["status"],
		createdAt: row.created_at == null ? undefined : Number(row.created_at),
		updatedAt: Number(row.updated_at),
		expiresAt: row.expires_at == null ? undefined : Number(row.expires_at),
	};
}

async function verifyPendingOwner(
	c: HookTokenContext,
	token: string,
	state: HookTokenRow,
): Promise<HookTokenRow | null> {
	if (
		state.status !== "pending" ||
		state.runId == null ||
		state.hookId == null
	) {
		return state.status === "confirmed" ? state : null;
	}

	let ownsHook: boolean;
	try {
		ownsHook = await c
			.client<any>()
			.workflowRun.getOrCreate([state.runId])
			.ownsHook(state.hookId, token);
	} catch {
		// Canonical ownership could not be checked. A pending reservation must
		// never become externally visible or be stolen on an inconclusive result.
		return null;
	}

	const now = Date.now();
	if (ownsHook) {
		await c.db.execute(
			`UPDATE hook_token_state
			 SET status = 'confirmed', updated_at = ?, expires_at = NULL
			 WHERE singleton = 1 AND generation = ? AND run_id = ? AND hook_id = ?
			   AND status = 'pending'`,
			now,
			state.generation,
			state.runId,
			state.hookId,
		);
		const current = await readState(c);
		return current.generation === state.generation &&
			current.status === "confirmed" &&
			current.runId === state.runId &&
			current.hookId === state.hookId
			? current
			: null;
	}

	if ((state.expiresAt ?? Number.POSITIVE_INFINITY) > now) return null;
	await c.db.execute(
		`UPDATE hook_token_state
		 SET generation = generation + 1, run_id = NULL, hook_id = NULL,
		     status = 'empty', created_at = NULL, updated_at = ?, expires_at = NULL
		 WHERE singleton = 1 AND generation = ? AND run_id = ? AND hook_id = ?
		   AND status = 'pending'`,
		now,
		state.generation,
		state.runId,
		state.hookId,
	);
	return null;
}

export const hookToken = actor({
	db: hookTokenDb,
	actions: {
		reserve: async (c, token: string, runId: string, hookId: string) => {
			const ctx = c as unknown as HookTokenContext;
			assertTokenKey(ctx, token);
			let state = await readState(ctx);
			if (
				state.status === "pending" &&
				(state.runId !== runId || state.hookId !== hookId) &&
				(state.expiresAt ?? Number.POSITIVE_INFINITY) <= Date.now()
			) {
				await verifyPendingOwner(ctx, token, state);
				state = await readState(ctx);
			}
			if (
				state.status !== "empty" &&
				state.runId === runId &&
				state.hookId === hookId
			) {
				return { ok: true, generation: state.generation };
			}
			if (state.status !== "empty") {
				return {
					ok: false,
					generation: state.generation,
					runId: state.runId,
					hookId: state.hookId,
				};
			}

			const now = Date.now();
			await ctx.db.execute(
				`UPDATE hook_token_state
				 SET generation = generation + 1, run_id = ?, hook_id = ?,
				     status = 'pending', created_at = ?, updated_at = ?, expires_at = ?
				 WHERE singleton = 1 AND status = 'empty'`,
				runId,
				hookId,
				now,
				now,
				now + PENDING_RESERVATION_TTL_MS,
			);
			const reserved = await readState(ctx);
			return {
				ok:
					reserved.status === "pending" &&
					reserved.runId === runId &&
					reserved.hookId === hookId,
				generation: reserved.generation,
			};
		},
		confirm: async (
			c,
			token: string,
			generation: number,
			runId: string,
			hookId: string,
		) => {
			const ctx = c as unknown as HookTokenContext;
			assertTokenKey(ctx, token);
			await ctx.db.execute(
				`UPDATE hook_token_state
				 SET status = 'confirmed', updated_at = ?, expires_at = NULL
				 WHERE singleton = 1 AND generation = ? AND run_id = ? AND hook_id = ?
				   AND status IN ('pending', 'confirmed')`,
				Date.now(),
				generation,
				runId,
				hookId,
			);
			const state = await readState(ctx);
			return {
				ok:
					state.generation === generation &&
					state.status === "confirmed" &&
					state.runId === runId &&
					state.hookId === hookId,
			};
		},
		get: async (c, token: string) => {
			const ctx = c as unknown as HookTokenContext;
			assertTokenKey(ctx, token);
			let state = await readState(ctx);
			if (state.status === "pending") {
				await verifyPendingOwner(ctx, token, state);
				state = await readState(ctx);
			}
			if (state.status !== "confirmed") return null;
			return {
				runId: state.runId as string,
				hookId: state.hookId as string,
				generation: state.generation,
			};
		},
		release: async (
			c,
			token: string,
			generation: number,
			runId: string,
			hookId?: string,
		) => {
			const ctx = c as unknown as HookTokenContext;
			assertTokenKey(ctx, token);
			await ctx.db.execute(
				`UPDATE hook_token_state
				 SET generation = generation + 1, run_id = NULL, hook_id = NULL,
				     status = 'empty', created_at = NULL, updated_at = ?, expires_at = NULL
				 WHERE singleton = 1 AND generation = ? AND run_id = ?
				   AND (? IS NULL OR hook_id = ?) AND status IN ('pending', 'confirmed')`,
				Date.now(),
				generation,
				runId,
				hookId ?? null,
				hookId ?? null,
			);
			const state = await readState(ctx);
			return {
				ok:
					state.status === "empty" && state.generation >= generation + 1,
			};
		},
	},
});
