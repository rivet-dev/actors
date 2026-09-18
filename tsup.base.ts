import type { Options } from "tsup";
import { createBoundaryEnforcementPlugin } from "./tsup.plugin.boundaries.ts";

const FAST_BUILD = process.env.FAST_BUILD === "1";

export default {
	esbuildPlugins: [createBoundaryEnforcementPlugin()],
	target: "node16",
	platform: "node",
	format: ["cjs", "esm"],
	sourcemap: true,
	clean: true,
	// DTS takes the longest time, so we skip it if doing a fast build
	dts: FAST_BUILD
		? false
		: {
				compilerOptions: {
					skipLibCheck: true,
					resolveJsonModule: true,
				},
			},
	minify: false,
	// IMPORTANT: Splitting is required to fix a bug with ESM (https://github.com/egoist/tsup/issues/992#issuecomment-1763540165)
	splitting: true,
	skipNodeModulesBundle: true,
	publicDir: true,
	external: [/^node:.*/],
	// Required to replace `import.meta.ur.` with CJS shims
	shims: true,
} satisfies Options;
