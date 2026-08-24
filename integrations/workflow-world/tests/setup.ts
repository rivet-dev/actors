// Runs before each test file is imported, so the dispatcher module (which reads
// these at evaluation time) picks up the test-tuned queue knobs. Keeps retry /
// dead-letter / stale-reclaim windows short and deterministic.
process.env.RIVET_WORLD_RIVET_MAX_DISPATCH_ATTEMPTS ??= "3";
process.env.RIVET_WORLD_RIVET_RETRY_DELAY_MS ??= "100";
process.env.RIVET_WORLD_RIVET_STALE_INFLIGHT_MS ??= "400";
process.env.RIVET_WORLD_RIVET_TESTING ??= "1";

const { registry } = await import("../src/actors.ts");
registry.config.runtime = "native";
