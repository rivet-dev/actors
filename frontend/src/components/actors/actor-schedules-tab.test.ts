import { describe, expect, test } from "vitest";
import type { InspectorSchedule } from "./actor-inspector-context";
import {
	describeCronExpression,
	formatDuration,
	formatSchedule,
} from "./actor-schedules-format";

const baseSchedule: InspectorSchedule = {
	id: "refresh-cache",
	name: "refresh-cache",
	kind: "every",
	action: "refresh",
	args: [],
	nextRunAt: 1_700_000_000_000,
};

describe("schedule inspector formatting", () => {
	test("formats one-time, interval, and cron schedules", () => {
		const oneTime = {
			...baseSchedule,
			kind: "at" as const,
			name: undefined,
		};
		expect(formatSchedule(oneTime)).toBe(
			new Intl.DateTimeFormat(undefined, {
				dateStyle: "medium",
				timeStyle: "short",
			}).format(oneTime.nextRunAt),
		);
		expect(
			formatSchedule({ ...baseSchedule, intervalMs: 5 * 60_000 }),
		).toBe("Every 5 minutes");
		expect(
			formatSchedule({
				...baseSchedule,
				kind: "cron",
				expression: "0 9 * * *",
				timezone: "America/Los_Angeles",
			}),
		).toBe("0 9 * * *");
		expect(describeCronExpression("0 9 * * *")).toBe(
			"At 9:00 AM, every day",
		);
		expect(describeCronExpression("not a cron")).toBe(
			"Unable to describe this cron expression",
		);
	});

	test("formats useful sub-second through multi-day durations", () => {
		expect(formatDuration(124)).toBe("124 ms");
		expect(formatDuration(5_000)).toBe("5 seconds");
		expect(formatDuration(90_000)).toBe("1.5 minutes");
		expect(formatDuration(7_200_000)).toBe("2 hours");
		expect(formatDuration(172_800_000)).toBe("2 days");
	});
});
