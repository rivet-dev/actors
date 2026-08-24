import { workflow } from "@workflow/vitest";
import { defineConfig } from "vitest/config";

export default defineConfig({
	plugins: [workflow()],
	test: {
		include: ["workflows/**/*.integration.test.ts"],
		setupFiles: ["./vitest.setup.ts"],
		testTimeout: 60_000,
	},
});
