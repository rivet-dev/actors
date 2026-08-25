import { defineConfig } from "nitro";

export default defineConfig({
	modules: ["workflow/nitro"],
	routes: {
		"/**": "./src/server.ts",
	},
});
