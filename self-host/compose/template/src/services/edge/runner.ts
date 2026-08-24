import type { TemplateContext } from "../../context";

export function generateRunner(context: TemplateContext) {
	// The runner service runs the kitchen-sink example in serverful mode,
	// connecting to the engine as a long-lived runner. The docker-compose
	// template builds examples/kitchen-sink/Dockerfile directly.
}
