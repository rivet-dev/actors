import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";
import tsconfigPaths from "vite-tsconfig-paths";

const rivetkitVersion = JSON.parse(
	readFileSync(
		path.resolve(
			__dirname,
			"../../../rivetkit-typescript/packages/rivetkit/package.json",
		),
		"utf8",
	),
).version as string;
const require = createRequire(path.resolve(__dirname, "package.json"));
const WORKER_IMPORT = 'import ActorWorker from "./actor-repl.worker?worker";';
let sawConsoleWorker = false;

export default defineConfig({
	root: path.resolve(__dirname),
	base: "./",
	plugins: [
		// @rivet-gg/icons re-exports Font Awesome Pro packages that are not
		// installed in every checkout. Without a fallback the single-file
		// build fails to resolve them.
		{
			name: "fallback-unavailable-mcp-icons",
			enforce: "pre",
			transform(code, id) {
				if (!id.endsWith("/packages/icons/src/index.gen.js")) return;
				let usedFallback = false;
				const transformed = code.replace(
					/export \{([^}]+)\} from "([^"]+)";/g,
					(statement, names: string, specifier: string) => {
						try {
							require.resolve(specifier);
							return statement;
						} catch {
							usedFallback = true;
							const aliases = names.split(",").map((entry) => {
								const parts = entry.trim().split(/\s+as\s+/);
								return `__mcpFallbackIcon as ${parts.at(-1)}`;
							});
							return `export { ${aliases.join(", ")} };`;
						}
					},
				);
				if (!usedFallback) return transformed;
				return `${transformed}\nconst __mcpFallbackIcon = { prefix: "fas", iconName: "circle", icon: [16, 16, [], "", "M8 1a7 7 0 1 0 0 14A7 7 0 0 0 8 1Z"] };`;
			},
		},
		react(),
		{
			name: "disable-unsupported-mcp-console-worker",
			enforce: "pre",
			transform(code, id) {
				if (!id.endsWith("/actor-worker-container.ts")) return;
				// viteSingleFile cannot inline a `?worker` chunk, so a silently
				// unmatched import ships a bundle whose console throws at load.
				if (!code.includes(WORKER_IMPORT)) {
					throw new Error(
						`${id} no longer contains ${WORKER_IMPORT}; update the MCP console worker stub`,
					);
				}
				sawConsoleWorker = true;
				return code.replace(
					WORKER_IMPORT,
					`class ActorWorker extends EventTarget {
	constructor() {
		super();
		throw new Error("The actor console is unavailable in the embedded Inspector.");
	}
	postMessage() {}
	terminate() {}
}`,
				);
			},
			buildEnd() {
				if (!sawConsoleWorker) {
					throw new Error(
						"actor-worker-container.ts was never transformed; the MCP console worker stub did not apply",
					);
				}
			},
		},
		tsconfigPaths({ projects: [path.resolve(__dirname, "tsconfig.json")] }),
		viteSingleFile(),
	],
	resolve: {
		alias: {
			"@rivet-gg/icons": path.resolve(
				__dirname,
				"../../packages/icons/src/index.gen.js",
			),
			"@": path.resolve(__dirname, "../../src"),
		},
	},
	define: {
		__MCP_APP__: JSON.stringify(true),
		__APP_TYPE__: JSON.stringify("inspector"),
		__APP_BUILD_ID__: JSON.stringify("mcp-actor-inspector"),
		__RIVETKIT_VERSION__: JSON.stringify(rivetkitVersion),
	},
	optimizeDeps: {
		include: ["@fortawesome/*", "@rivet-gg/icons", "@rivet-gg/cloud"],
	},
	worker: { format: "es" },
	build: {
		outDir: "../../dist/mcp-inspector-ui",
		emptyOutDir: true,
		sourcemap: false,
		cssCodeSplit: false,
		rollupOptions: {
			input: path.resolve(__dirname, "mcp-index.html"),
			output: { inlineDynamicImports: true },
		},
		commonjsOptions: { include: [/@rivet-gg\/components/, /node_modules/] },
	},
});
