import * as crypto from "node:crypto";
import path from "node:path";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import tsconfigPaths from "vite-tsconfig-paths";

// https://vitejs.dev/config/
export default defineConfig({
	root: path.resolve(__dirname),
	base: "/",
	publicDir: path.resolve(__dirname, "../../public"),
	envDir: path.resolve(__dirname, "../.."),
	plugins: [
		tanstackRouter({
			target: "react",
			autoCodeSplitting: true,
			routesDirectory: path.resolve(__dirname, "src/routes"),
			generatedRouteTree: path.resolve(__dirname, "src/routeTree.gen.ts"),
		}),
		react(),
		tsconfigPaths(),
	],
	define: {
		__APP_BUILD_ID__: JSON.stringify(
			`${new Date().toISOString()}@${crypto.randomUUID()}`,
		),
		__MCP_APP__: JSON.stringify(false),
		"import.meta.env.VITE_FEATURE_FLAGS": JSON.stringify(""),
	},
	optimizeDeps: {
		include: ["@fortawesome/*", "@rivet-gg/icons", "@rivet-gg/cloud"],
	},
	worker: {
		format: "es",
	},
	server: {
		port: 43709,
		proxy: {},
	},
	preview: {
		port: 43709,
	},
	build: {
		outDir: "../../dist/inspector",
		sourcemap: true,
		commonjsOptions: {
			include: [/@rivet-gg\/components/, /node_modules/],
		},
	},
});
