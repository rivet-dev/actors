import { builtinModules } from "node:module";
import { defineConfig } from "tsup";

// Pre-bundle the rivetkit wasm-path runtime into this adapter's own dist. A
// Supabase Edge Function deploy (Deno eszip) snapshots the entire declared npm
// dependency closure, so a package that declares `rivetkit` as a runtime
// dependency drags in rivetkit's native packages (engine-cli, Services,
// rivetkit-napi, agent-os secure-exec) that the wasm runtime never executes. By bundling
// rivetkit here and not declaring it as a runtime dependency, and by having the
// Supabase function's import map point `rivetkit` at this adapter, the deploy
// ships only the code the wasm path actually uses. Only @rivetkit/rivetkit-wasm
// and a few node-oriented CJS libs stay external (see below).
export default defineConfig({
	entry: { mod: "src/mod.ts" },
	outDir: "dist",
	target: "esnext",
	platform: "node",
	format: ["esm", "cjs"],
	sourcemap: false,
	clean: true,
	dts: {
		compilerOptions: {
			skipLibCheck: true,
			resolveJsonModule: true,
		},
	},
	splitting: false,
	skipNodeModulesBundle: false,
	shims: false,
	external: [
		"@rivetkit/rivetkit-wasm",
		// Native packages the wasm path never executes.
		"@rivetkit/rivetkit-napi",
		"@rivetkit/engine-cli",
		"@rivet-dev/services",
		"@rivet-dev/agent-os-core",
		// Node CommonJS libs with dynamic require() that esbuild cannot bundle
		// into ESM for Deno; Deno's node compat loads them at runtime. Declared
		// as runtime dependencies of this package.
		"pino",
		"cbor-x",
	],
	esbuildPlugins: [
		{
			// Deno requires the `node:` prefix on built-in modules. esbuild, when
			// bundling node-targeted CJS deps, can emit bare `import "module"` /
			// `import "os"` which Deno rejects. Rewrite every bare Node built-in
			// import to its `node:`-prefixed external form.
			name: "node-builtin-prefix",
			setup(build) {
				build.onResolve({ filter: /^[^.]/ }, (args) => {
					const bare = args.path.replace(/^node:/, "");
					if (builtinModules.includes(bare)) {
						return { path: `node:${bare}`, external: true };
					}
					return undefined;
				});
			},
		},
	],
});
