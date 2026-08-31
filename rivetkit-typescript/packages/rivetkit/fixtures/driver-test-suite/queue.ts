// @ts-nocheck
import { actor, queue } from "rivetkit";
import type { registry } from "./registry-static";

const queueSchemas = {
	greeting: queue<{ hello: string }>(),
	self: queue<{ value: number }>(),
	a: queue<number>(),
	b: queue<number>(),
	c: queue<number>(),
	one: queue<string>(),
	two: queue<string>(),
	missing: queue<unknown>(),
	abort: queue<unknown>(),
	tasks: queue<{ value: number }, { echo: { value: number } }>(),
	timeout: queue<{ value: number }, { ok: true }>(),
	nowait: queue<{ value: string }>(),
	twice: queue<{ value: string }, { ok: true }>(),
} as const;

type QueueName = keyof typeof queueSchemas;

export const queueActor = actor({
	state: {},
	queues: queueSchemas,
	actions: {
		receiveOne: async (c, name: QueueName, opts?: { timeout?: number }) => {
			const message = await c.queue.next({
				names: [name],
				timeout: opts?.timeout,
			});
			if (!message) {
				return null;
			}
			return { name: message.name, body: message.body };
		},
		receiveMany: async (
			c,
			names: QueueName[],
			opts?: { count?: number; timeout?: number },
		) => {
			const messages = await c.queue.nextBatch({
				names,
				count: opts?.count,
				timeout: opts?.timeout,
			});
			return messages.map((message) => ({
				name: message.name,
				body: message.body,
			}));
		},
		receiveRequest: async (
			c,
			request: {
				names?: QueueName[];
				count?: number;
				timeout?: number;
			},
		) => {
			const messages = await c.queue.nextBatch(request);
			return messages.map((message) => ({
				name: message.name,
				body: message.body,
			}));
		},
		tryReceiveMany: async (
			c,
			request: {
				names?: QueueName[];
				count?: number;
			},
		) => {
			const messages = await c.queue.tryNextBatch(request);
			return messages.map((message) => ({
				name: message.name,
				body: message.body,
			}));
		},
		receiveWithIterator: async (c, name: QueueName) => {
			for await (const message of c.queue.iter({ names: [name] })) {
				return { name: message.name, body: message.body };
			}
			return null;
		},
		receiveWithAsyncIterator: async (c) => {
			for await (const message of c.queue.iter()) {
				return { name: message.name, body: message.body };
			}
			return null;
		},
		sendToSelf: async (c, name: QueueName, body: unknown) => {
			const client = c.client<typeof registry>();
			const handle = client.queueActor.getForId(c.actorId);
			await handle.send(name, body);
			return true;
		},
		waitForAbort: async (c) => {
			setTimeout(() => {
				c.destroy();
			}, 10);
			await c.queue.next({ names: ["abort"], timeout: 10_000 });
			return true;
		},
		waitForSignalAbort: async (c) => {
			const controller = new AbortController();
			controller.abort();
			try {
				await c.queue.next({
					names: ["abort"],
					timeout: 10_000,
					signal: controller.signal,
				});
				return { ok: false };
			} catch (error) {
				const actorError = error as { group?: string; code?: string };
				return { group: actorError.group, code: actorError.code };
			}
		},
		waitForActorAbortWithSignal: async (c) => {
			const controller = new AbortController();
			setTimeout(() => {
				c.destroy();
			}, 10);
			try {
				await c.queue.next({
					names: ["abort"],
					timeout: 10_000,
					signal: controller.signal,
				});
				return { ok: false };
			} catch (error) {
				const actorError = error as { group?: string; code?: string };
				return { group: actorError.group, code: actorError.code };
			}
		},
		iterWithSignalAbort: async (c) => {
			const controller = new AbortController();
			controller.abort();
			try {
				for await (const _message of c.queue.iter({
					names: ["abort"],
					signal: controller.signal,
				})) {
					return { ok: false };
				}
				return { ok: true };
			} catch (error) {
				const actorError = error as { group?: string; code?: string };
				if (
					actorError.group === "actor" &&
					actorError.code === "aborted"
				) {
					return { ok: true };
				}
				throw error;
			}
		},
		receiveAndComplete: async (c, name: "tasks") => {
			const message = await c.queue.next({
				names: [name],
				completable: true,
			});
			if (!message) {
				return null;
			}
			await message.complete({ echo: message.body });
			return { name: message.name, body: message.body };
		},
		receiveWithoutComplete: async (c, name: "tasks") => {
			const message = await c.queue.next({
				names: [name],
				completable: true,
			});
			if (!message) {
				return null;
			}
			return { name: message.name, body: message.body };
		},
		receiveManualThenNextWithoutComplete: async (c, name: "tasks") => {
			const message = await c.queue.next({
				names: [name],
				completable: true,
			});
			if (!message) {
				return { ok: false, reason: "no_message" };
			}

			try {
				await c.queue.next({ names: [name], timeout: 0 });
				c.destroy();
				return { ok: false, reason: "next_succeeded" };
			} catch (error) {
				c.destroy();
				const actorError = error as { group?: string; code?: string };
				return { group: actorError.group, code: actorError.code };
			}
		},
		receiveCompleteByIdentityThenNext: async (c, name: "tasks") => {
			const first = await c.queue.next({
				names: [name],
				completable: true,
			});
			if (!first) {
				return { ok: false, reason: "no_first_message" };
			}

			await c.queue.complete(
				{ id: first.id, name: first.name },
				{ echo: first.body },
			);
			const second = await c.queue.next({
				names: [name],
				timeout: 1_000,
				completable: true,
			});
			if (!second) {
				return { ok: false, reason: "no_second_message" };
			}
			await second.complete({ echo: second.body });
			return { ok: true, body: second.body };
		},
		receiveAndCompleteTwice: async (c, name: "twice") => {
			const message = await c.queue.next({
				names: [name],
				completable: true,
			});
			if (!message) {
				return null;
			}
			await message.complete({ ok: true });
			try {
				await message.complete({ ok: true });
				return { ok: false };
			} catch (error) {
				const actorError = error as { group?: string; code?: string };
				return { group: actorError.group, code: actorError.code };
			}
		},
		receiveWithoutCompleteMethod: async (c, name: "nowait") => {
			const message = await c.queue.next({
				names: [name],
				completable: true,
			});
			return {
				hasComplete:
					message !== undefined &&
					typeof message.complete === "function",
			};
		},
	},
});

export const queueLimitedActor = actor({
	state: {},
	queues: {
		message: queue<number>(),
		oversize: queue<string>(),
	},
	actions: {},
	options: {
		maxQueueSize: 1,
		maxQueueMessageSize: 64,
	},
});

export const MANY_QUEUE_NAMES = Array.from(
	{ length: 32 },
	(_, i) => `cmd.${i}` as const,
);

const manyQueueSchemas = Object.fromEntries(
	MANY_QUEUE_NAMES.map((name) => [
		name,
		queue<{ index: number }, { ok: true; index: number }>(),
	]),
);

export const manyQueueChildActor = actor({
	queues: manyQueueSchemas,
	actions: {
		ping: (c) => ({ label: c.state.label, pong: true }),
		getSnapshot: (c) => c.state,
	},
	createState: (_c, label: string) => ({
		label,
		started: false,
		processed: [] as string[],
	}),
	run: async (c) => {
		c.state.started = true;
		for await (const msg of c.queue.iter({
			names: [...MANY_QUEUE_NAMES],
			completable: true,
		})) {
			c.state.processed.push(msg.name);
			await msg.complete({
				ok: true,
				index: msg.body.index,
			});
		}
	},
});

export const manyQueueActionParentActor = actor({
	state: {
		spawned: [] as string[],
	},
	actions: {
		spawnChild: async (c, key: string) => {
			const client = c.client<typeof registry>();
			await client.manyQueueChildActor.getOrCreate([key], {
				createWithInput: key,
			});
			c.state.spawned.push(key);
			return { key };
		},
		getSpawned: (c) => c.state.spawned,
	},
});

export const manyQueueRunParentActor = actor({
	state: {
		spawned: [] as string[],
	},
	queues: {
		spawn: queue<{ key: string }>(),
	},
	actions: {
		queueSpawn: async (c, key: string) => {
			await c.queue.enqueueAndWait("spawn", { key }, { timeout: 10_000 });
			return { queued: true };
		},
		getSpawned: (c) => c.state.spawned,
	},
	run: async (c) => {
		for await (const msg of c.queue.iter({
			names: ["spawn"],
			completable: true,
		})) {
			const client = c.client<typeof registry>();
			await client.manyQueueChildActor.getOrCreate([msg.body.key], {
				createWithInput: msg.body.key,
			});
			c.state.spawned.push(msg.body.key);
			await msg.complete({ ok: true });
		}
	},
});
