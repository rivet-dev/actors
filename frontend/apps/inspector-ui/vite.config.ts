import * as crypto from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import tsconfigPaths from "vite-tsconfig-paths";

// The inspector-ui bundle is embedded into the rivetkit build that serves it,
// so rivetkit's package version IS the version of the actor runtime this bundle
// talks to. Baking it in lets the SPA seed its metadata query and open the
// inspector WebSocket without a `/metadata` round trip.
const rivetkitVersion = JSON.parse(
	readFileSync(
		path.resolve(
			__dirname,
			"../../../rivetkit-typescript/packages/rivetkit/package.json",
		),
		"utf8",
	),
).version as string;

// Single-entry SPA that renders the entire actor inspector right panel:
// tab strip, guard, WebSocket client, and every tab. Served by the engine at
// /inspector/ui/ for each actor and mounted in the dashboard as an iframe.
// `base: "./"` makes asset references relative to index.html so the bundle
// works at any mount point without a rebuild.
export default defineConfig({
	root: path.resolve(__dirname),
	base: "./",
	publicDir: path.resolve(__dirname, "../../public"),
	envDir: path.resolve(__dirname, "../.."),
	plugins: [react(), tsconfigPaths()],
	define: {
		__MCP_APP__: JSON.stringify(false),
		__APP_TYPE__: JSON.stringify("inspector"),
		__APP_BUILD_ID__: JSON.stringify(
			`${new Date().toISOString()}@${crypto.randomUUID()}`,
		),
		__RIVETKIT_VERSION__: JSON.stringify(rivetkitVersion),
	},
	optimizeDeps: {
		include: ["@fortawesome/*", "@rivet-gg/icons", "@rivet-gg/cloud"],
	},
	worker: {
		format: "es",
	},
	build: {
		outDir: "../../dist/inspector-ui",
		sourcemap: true,
		commonjsOptions: {
			include: [/@rivet-gg\/components/, /node_modules/],
		},
	},
});
