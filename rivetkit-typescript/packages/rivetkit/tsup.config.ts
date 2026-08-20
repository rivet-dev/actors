/// <reference types="@types/node" />

import { defineConfig } from "tsup";
import defaultConfig from "../../../tsup.base.ts";

export default defineConfig({
	...defaultConfig,
	outDir: "dist/tsup/",
	// Override shims: false to prevent ESM shims (fileURLToPath, etc.) from being
	// injected into chunks. The shims import Node.js-only modules which break
	// browser builds when importing from rivetkit/client.
	// See: https://github.com/egoist/tsup/issues/958
	shims: false,
	esbuildOptions(options, context) {
		// Mark @rivetkit workspace packages as external to preserve their dependency chains
		options.external = [
			...(options.external || []),
			"@rivetkit/traces",
			"@rivetkit/traces/encoding",
			"@rivetkit/traces/otlp",
			"@rivetkit/workflow-engine",
			"@rivet-dev/agent-os-core",
		];
		// `shims: false` (above) keeps ESM shims out of the browser build, but it
		// also leaves `import.meta.url` (used by getRequireFn in src/utils/node.ts)
		// emitted verbatim into the CJS output — a SyntaxError at load time for any
		// CommonJS consumer (e.g. importing `rivetkit` under ts-node / a CJS host).
		// Define a CJS-safe replacement for the CJS format only; ESM keeps native
		// import.meta.
		if (context.format === "cjs") {
			options.define = {
				...(options.define || {}),
				"import.meta.url": "require('url').pathToFileURL(__filename).href",
			};
		}
	},
	define: {
		"globalThis.CUSTOM_RIVETKIT_DEVTOOLS_URL": process.env
			.CUSTOM_RIVETKIT_DEVTOOLS_URL
			? `"${process.env.CUSTOM_RIVETKIT_DEVTOOLS_URL}"`
			: "false",
	},
});
