// @ts-nocheck
import { actor, queue } from "rivetkit";
import type { registry } from "./registry-static";

export const RUN_SLEEP_TIMEOUT = 1000;

// Actor that tracks tick counts and respects abort signal
export const runWithTicks = actor({
	state: {
		tickCount: 0,
		lastTickAt: 0,
		runCount: 0,
		runStarted: false,
		runExited: false,
	},
	run: async (c) => {
		c.state.runCount += 1;
		c.state.runStarted = true;
		c.log.info("run handler started");

		while (!c.aborted) {
			c.state.tickCount += 1;
			c.state.lastTickAt = Date.now();

			// Wait 50ms between ticks, or exit early if aborted
			await new Promise<void>((resolve) => {
				const timeout = setTimeout(resolve, 50);
				c.abortSignal.addEventListener(
					"abort",
					() => {
						clearTimeout(timeout);
						resolve();
					},
					{ once: true },
				);
			});
		}

		c.state.runExited = true;
		c.log.info("run handler exiting gracefully");
	},
	actions: {
		getState: (c) => ({
			tickCount: c.state.tickCount,
			lastTickAt: c.state.lastTickAt,
			runCount: c.state.runCount,
			runStarted: c.state.runStarted,
			runExited: c.state.runExited,
		}),
		setRunWakeAt: async (c, timestamp: number | null) => {
			await c.run.setWakeAt(timestamp);
		},
	},
	options: {
		sleepTimeout: RUN_SLEEP_TIMEOUT,
	},
});

// Finite run handler used to verify durable wake and cancellation behavior.
export const runWithWakeDeadline = actor({
	onWake: async (c) => {
		const client = c.client<typeof registry>();
		await client.lifecycleObserver
			.getOrCreate([c.key[0]])
			.recordEvent({ actorKey: c.actorId, event: "wake" });
	},
	run: async (c) => {
		const client = c.client<typeof registry>();
		await client.lifecycleObserver
			.getOrCreate([c.key[0]])
			.recordEvent({ actorKey: c.actorId, event: "run" });
	},
	onSleep: async (c) => {
		const client = c.client<typeof registry>();
		await client.lifecycleObserver
			.getOrCreate([c.key[0]])
			.recordEvent({ actorKey: c.actorId, event: "sleep" });
	},
	actions: {
		setRunWakeAt: async (c, timestamp: number | null) => {
			await c.run.setWakeAt(timestamp);
		},
	},
	options: {
		sleepTimeout: RUN_SLEEP_TIMEOUT,
	},
});

// Actor that consumes from a queue in the run handler
export const runWithQueueConsumer = actor({
	state: {
		messagesReceived: [] as Array<{ name: string; body: unknown }>,
		runStarted: false,
		wakeCount: 0,
	},
	queues: {
		messages: queue<unknown>(),
	},
	onWake: (c) => {
		c.state.wakeCount += 1;
	},
	run: async (c) => {
		c.state.runStarted = true;
		c.log.info("run handler started, waiting for messages");

		while (!c.aborted) {
			const message = await c.queue.next({ names: ["messages"] });
			if (message) {
				c.log.info({ msg: "received message", body: message.body });
				c.state.messagesReceived.push({
					name: message.name,
					body: message.body,
				});
			}
		}

		c.log.info("run handler exiting gracefully");
	},
	actions: {
		getState: (c) => ({
			messagesReceived: c.state.messagesReceived,
			runStarted: c.state.runStarted,
			wakeCount: c.state.wakeCount,
		}),
		sendMessage: async (c, body: unknown) => {
			await c.queue.send("messages", body);
			return true;
		},
	},
	options: {
		sleepTimeout: RUN_SLEEP_TIMEOUT,
	},
});

// Actor that exits the run handler after a short delay to test crash behavior
export const runWithEarlyExit = actor({
	state: {
		runStarted: false,
		runExited: false,
		destroyCalled: false,
		sleepCount: 0,
		wakeCount: 0,
	},
	onWake: (c) => {
		c.state.wakeCount += 1;
	},
	onSleep: async (c) => {
		c.state.sleepCount += 1;
		const client = c.client<typeof registry>();
		await client.lifecycleObserver
			.getOrCreate(["run-with-early-exit"])
			.recordEvent({
				actorKey: c.actorId,
				event: "sleep",
			});
	},
	run: async (c) => {
		c.state.runStarted = true;
		c.log.info("run handler started, will exit after delay");
		// Wait a bit so we can observe the runStarted state before exit
		await new Promise((resolve) => setTimeout(resolve, 200));
		c.state.runExited = true;
		c.log.info("run handler exiting early");
		// Exit without respecting abort signal
	},
	onDestroy: async (c) => {
		c.state.destroyCalled = true;
		const client = c.client<typeof registry>();
		await client.lifecycleObserver
			.getOrCreate(["run-with-early-exit"])
			.recordEvent({
				actorKey: c.actorId,
				event: "destroy",
			});
	},
	actions: {
		getState: (c) => ({
			runStarted: c.state.runStarted,
			runExited: c.state.runExited,
			destroyCalled: c.state.destroyCalled,
			sleepCount: c.state.sleepCount,
			wakeCount: c.state.wakeCount,
		}),
		destroy: (c) => {
			c.destroy();
		},
	},
	options: {
		sleepTimeout: RUN_SLEEP_TIMEOUT,
	},
});

// Actor that throws an error in the run handler to test crash behavior
export const runWithError = actor({
	state: {
		runStarted: false,
		destroyCalled: false,
		sleepCount: 0,
		wakeCount: 0,
	},
	onWake: (c) => {
		c.state.wakeCount += 1;
	},
	onSleep: (c) => {
		c.state.sleepCount += 1;
	},
	run: async (c) => {
		c.state.runStarted = true;
		c.log.info("run handler started, will throw error");
		await new Promise((resolve) => setTimeout(resolve, 200));
		throw new Error("intentional error in run handler");
	},
	onDestroy: (c) => {
		c.state.destroyCalled = true;
	},
	actions: {
		getState: (c) => ({
			runStarted: c.state.runStarted,
			destroyCalled: c.state.destroyCalled,
			sleepCount: c.state.sleepCount,
			wakeCount: c.state.wakeCount,
		}),
	},
	options: {
		sleepTimeout: RUN_SLEEP_TIMEOUT,
	},
});

export const runSelfInitiatedSleep = actor({
	state: {
		runCount: 0,
		wakeCount: 0,
		sleepCount: 0,
		marker: "new",
	},
	onWake: (c) => {
		c.state.wakeCount += 1;
	},
	onSleep: (c) => {
		c.state.sleepCount += 1;
		c.state.marker = "slept";
	},
	run: (c) => {
		c.state.runCount += 1;
		if (c.state.runCount === 1) {
			c.state.marker = "sleep-requested";
			c.sleep();
		}
	},
	actions: {
		getState: (c) => ({
			runCount: c.state.runCount,
			wakeCount: c.state.wakeCount,
			sleepCount: c.state.sleepCount,
			marker: c.state.marker,
		}),
	},
	options: {
		sleepTimeout: RUN_SLEEP_TIMEOUT,
	},
});

export const runSelfInitiatedDestroy = actor({
	state: {
		runCount: 0,
		destroyRequested: false,
	},
	run: (c) => {
		c.state.runCount += 1;
		if (!c.state.destroyRequested) {
			c.state.destroyRequested = true;
			c.destroy();
		}
	},
	onDestroy: async (c) => {
		const client = c.client<typeof registry>();
		await client.lifecycleObserver
			.getOrCreate(["self-initiated-destroy"])
			.recordEvent({
				actorKey: c.actorId,
				event: "destroy",
			});
	},
	actions: {
		getState: (c) => ({
			runCount: c.state.runCount,
			destroyRequested: c.state.destroyRequested,
		}),
	},
});

export const runIgnoresAbortStopTimeout = actor({
	state: {
		wakeCount: 0,
		destroyCount: 0,
	},
	onWake: (c) => {
		c.state.wakeCount += 1;
	},
	onDestroy: (c) => {
		c.state.destroyCount += 1;
	},
	run: async () => {
		await new Promise(() => {});
	},
	actions: {
		getState: (c) => ({
			wakeCount: c.state.wakeCount,
			destroyCount: c.state.destroyCount,
		}),
		destroy: (c) => {
			c.destroy();
		},
	},
	options: {
		sleepTimeout: 50,
		sleepGracePeriod: 100,
	},
});

// Actor without a run handler for comparison
export const runWithoutHandler = actor({
	state: {
		wakeCount: 0,
	},
	onWake: (c) => {
		c.state.wakeCount += 1;
	},
	actions: {
		getState: (c) => ({
			wakeCount: c.state.wakeCount,
		}),
	},
	options: {
		sleepTimeout: RUN_SLEEP_TIMEOUT,
	},
});
