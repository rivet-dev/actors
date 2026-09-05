import { actor, setup } from "rivetkit";

const job = actor({
	state: {},
	actions: {},
});

const registry = setup({
	use: { job },
	// Each worker thread can host at most four live actors.
	actorsPerThread: 4,
});

// Keep this startup call unguarded so worker threads can attach their copy of
// the registry instead of starting another runner connection.
registry.start();
