import { defineConfig, mergeConfig } from "vitest/config";
import baseConfig from "../../vitest.base.ts";

export default mergeConfig(
	baseConfig,
	defineConfig({
		test: {
			setupFiles: ["./tests/setup.ts"],
			// vitest.base enables sequence.concurrent for the engine's own
			// concurrency-safe suites. These actor/SQL suites share per-test
			// `beforeEach` fixtures and assert deterministic row ordering, so they
			// must run sequentially within each file.
			sequence: { concurrent: false },
			// Actor / chaos tests stand up native runtimes and poll; give them room.
			testTimeout: 30_000,
			hookTimeout: 30_000,
			// The RivetKit test driver occasionally drops an actor mid-
			// provisioning (`actor_wake_retries_exceeded`); that's harness flakiness,
			// not product behavior (real regressions fail deterministically across
			// retries — see the negative-control checks in each suite). Retry to keep
			// the gate reliable without masking genuine failures.
			retry: 2,
			// RivetKit actors keep module-level singletons; isolate files.
			pool: "forks",
			// Each setupTest file spins up its own native engine; running files in
			// parallel makes those engines contend (sporadic `fetch failed` across whole
			// files). Serialize file execution — the deterministic suites are fast, so
			// the wall-clock cost is small and the gate becomes reliable.
			fileParallelism: false,
		},
	}),
);
