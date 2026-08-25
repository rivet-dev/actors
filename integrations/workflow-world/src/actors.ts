import { setup } from "rivetkit";
import { coordinator } from "./actors/coordinator.js";
import { hookToken } from "./actors/hook-token.js";
import { workflowRun } from "./actors/workflow-run.js";

export { coordinator } from "./actors/coordinator.js";
export { hookToken } from "./actors/hook-token.js";
export { workflowRun } from "./actors/workflow-run.js";

export const workflowWorldActors = {
	workflowRun,
	coordinator,
	hookToken,
};

export const registry = setup({
	use: workflowWorldActors,
});
