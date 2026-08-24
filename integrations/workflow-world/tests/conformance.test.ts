import { createTestSuite } from "@workflow/world-testing";
import { describe } from "vitest";

if (process.env.RUN_WORKFLOW_CONFORMANCE === "1") {
	createTestSuite("./dist/index.js");
} else {
	describe.skip("@workflow/world-testing conformance", () => {});
}
