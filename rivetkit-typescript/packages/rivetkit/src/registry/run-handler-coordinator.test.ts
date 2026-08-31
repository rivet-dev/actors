import { describe, expect, test, vi } from "vitest";
import { defineRunHandler, type RunControl } from "@/actor/config";
import { RunHandlerCoordinator } from "./run-handler-coordinator";

class Deferred<T = void> {
	readonly promise: Promise<T>;
	resolve!: (value: T | PromiseLike<T>) => void;
	reject!: (reason?: unknown) => void;

	constructor() {
		this.promise = new Promise<T>((resolve, reject) => {
			this.resolve = resolve;
			this.reject = reject;
		});
	}
}

function createCoordinator() {
	let control: RunControl | undefined;
	const dispose = vi.fn();
	const createInspector = vi.fn((context: { control: RunControl }) => {
		control = context.control;
		return {
			inspector: {
				workflow: {
					getHistory: () => null,
					getState: async () => null,
					onHistoryUpdated: () => () => {},
					replayFromStep: async () => null,
				},
			},
			dispose,
		};
	});
	const run = defineRunHandler(async () => {}, {
		inspectorKind: "workflow",
		createInspector,
	});
	const coordinator = new RunHandlerCoordinator(run);
	const restart = vi.fn();
	const inspector = coordinator.getInspector("actor-1", restart);

	return {
		control: control as unknown as RunControl,
		coordinator,
		createInspector,
		dispose,
		inspector,
		restart,
	};
}

describe("RunHandlerCoordinator", () => {
	test("fails explicitly when a JavaScript factory omits required workflow controls", async () => {
		const coordinator = new RunHandlerCoordinator(
			defineRunHandler(async () => {}, {
				inspectorKind: "workflow",
				createInspector: (() => ({
					inspector: { workflow: { getHistory: () => null } },
				})) as never,
			}),
		);

		await expect(
			coordinator.run(
				"actor-1",
				() => {},
				async () => {},
			),
		).rejects.toThrow(
			"createInspector returned an invalid workflow adapter for actor actor-1",
		);
	});

	test("initializes and disposes Inspector state even when it is never queried", async () => {
		const dispose = vi.fn();
		const createInspector = vi.fn(() => ({
			inspector: {
				workflow: {
					getHistory: () => null,
					getState: async () => null,
					onHistoryUpdated: () => () => {},
					replayFromStep: async () => null,
				},
			},
			dispose,
		}));
		const coordinator = new RunHandlerCoordinator(
			defineRunHandler(async () => {}, {
				inspectorKind: "workflow",
				createInspector,
			}),
		);

		await coordinator.run(
			"actor-1",
			() => {},
			async () => {},
		);
		expect(createInspector).toHaveBeenCalledOnce();
		coordinator.destroy("actor-1");
		expect(dispose).toHaveBeenCalledOnce();
	});

	test("creates one inspector per live actor and disposes it exactly once", () => {
		const subject = createCoordinator();
		expect(
			subject.coordinator.getInspector("actor-1", subject.restart),
		).toBe(subject.inspector);
		expect(subject.createInspector).toHaveBeenCalledOnce();

		subject.coordinator.destroy("actor-1");
		subject.coordinator.destroy("actor-1");
		expect(subject.dispose).toHaveBeenCalledOnce();

		const recreated = subject.coordinator.getInspector(
			"actor-1",
			subject.restart,
		);
		expect(recreated).not.toBe(subject.inspector);
		expect(subject.createInspector).toHaveBeenCalledTimes(2);
	});

	test("rejects replay while a run is active", async () => {
		const subject = createCoordinator();
		const active = new Deferred();
		const started = new Deferred();
		const running = subject.coordinator.run(
			"actor-1",
			subject.restart,
			async () => {
				started.resolve();
				await active.promise;
			},
		);
		await started.promise;

		await expect(
			subject.control.run.withInactive(
				{ restartOnSuccess: true },
				async () => {},
			),
		).rejects.toMatchObject({
			group: "actor",
			code: "run_handler_unavailable",
		});

		active.resolve();
		await running;
	});

	test("uses a start queued during replay as the single restart", async () => {
		const subject = createCoordinator();
		const replay = new Deferred();
		const replayStarted = new Deferred();
		const events: string[] = [];
		subject.restart.mockImplementation(() => {
			events.push("restart");
		});

		const exclusive = subject.control.run.withInactive(
			{ restartOnSuccess: true },
			async () => {
				events.push("replay");
				replayStarted.resolve();
				await replay.promise;
			},
		);
		await replayStarted.promise;

		const queued = subject.coordinator.run(
			"actor-1",
			subject.restart,
			async () => {
				events.push("run");
			},
		);
		await Promise.resolve();
		expect(events).toEqual(["replay"]);

		replay.resolve();
		await exclusive;
		await queued;
		expect(events).toEqual(["replay", "run"]);
		expect(subject.restart).not.toHaveBeenCalled();
	});

	test("drops starts queued behind a failed replay and leaves the run inactive", async () => {
		const subject = createCoordinator();
		const replay = new Deferred();
		const replayStarted = new Deferred();
		const run = vi.fn();
		const expected = new Error("rewrite failed");

		const exclusive = subject.control.run.withInactive(
			{ restartOnSuccess: true },
			async () => {
				replayStarted.resolve();
				await replay.promise;
				throw expected;
			},
		);
		await replayStarted.promise;
		const queued = subject.coordinator.run("actor-1", subject.restart, run);

		replay.resolve();
		await expect(exclusive).rejects.toBe(expected);
		await expect(queued).resolves.toBe("suppressed");
		expect(run).not.toHaveBeenCalled();
		expect(subject.restart).not.toHaveBeenCalled();
	});

	test("restores a durable wake suppressed by a failed replay", async () => {
		const subject = createCoordinator();
		const replay = new Deferred();
		const replayStarted = new Deferred();
		const run = vi.fn();

		const exclusive = subject.control.run.withInactive({}, async () => {
			replayStarted.resolve();
			await replay.promise;
			throw new Error("rewrite failed");
		});
		await replayStarted.promise;
		const queued = subject.coordinator.run("actor-1", subject.restart, run);

		replay.resolve();
		await expect(exclusive).rejects.toThrow("rewrite failed");
		await expect(queued).resolves.toBe("suppressed");
		expect(run).not.toHaveBeenCalled();
	});

	test("allows only one concurrent replay and closes old controls on destroy", async () => {
		const subject = createCoordinator();
		const replay = new Deferred();
		const first = subject.control.run.withInactive({}, async () => {
			await replay.promise;
		});

		await expect(
			subject.control.run.withInactive({}, async () => {}),
		).rejects.toMatchObject({ code: "run_handler_unavailable" });
		subject.coordinator.destroy("actor-1");
		replay.resolve();
		await expect(first).rejects.toMatchObject({
			code: "run_handler_unavailable",
		});
		await expect(
			subject.control.run.withInactive({}, async () => {}),
		).rejects.toMatchObject({ code: "run_handler_unavailable" });
	});
});
