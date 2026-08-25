import { waitForSleep } from "@workflow/vitest";
import { expect, test } from "vitest";
import { getRun, start } from "workflow/api";

import { processOrder } from "./order.ts";

test("runs the workflow end to end", async () => {
	const run = await start(processOrder, ["42"]);
	const sleepId = await waitForSleep(run);

	await getRun(run.runId).wakeUp({ correlationIds: [sleepId] });

	await expect(run.returnValue).resolves.toEqual({
		id: "42",
		reservationId: "reservation-42",
		status: "charged",
	});
	expect(await run.status).toBe("completed");
});
