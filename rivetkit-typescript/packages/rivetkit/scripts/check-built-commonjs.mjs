import { readFileSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";

const outputDir = new URL("../dist/tsup/", import.meta.url);
for (const relativePath of readdirSync(outputDir, { recursive: true })) {
	if (!relativePath.endsWith(".cjs")) continue;
	const source = readFileSync(new URL(relativePath, outputDir), "utf8");
	if (
		source.includes("import.meta.url") ||
		/\bimport_meta\d*\.url\b/.test(source)
	) {
		throw new Error(`${relativePath} contains an invalid CommonJS import.meta URL`);
	}
}

const require = createRequire(import.meta.url);
const rivetkit = require("../dist/tsup/mod.cjs");
const workflowInspectorCjs = require("../dist/tsup/inspector/workflow.cjs");
const workflowInspectorEsm = await import(
	new URL("../dist/tsup/inspector/workflow.js", import.meta.url)
);

if (typeof rivetkit.actor !== "function") {
	throw new Error("CommonJS build does not export actor()");
}

for (const [format, workflowInspector] of [
	["CommonJS", workflowInspectorCjs],
	["ESM", workflowInspectorEsm],
]) {
	if (
		typeof workflowInspector.encodeWorkflowHistoryTransport !== "function" ||
		typeof workflowInspector.decodeWorkflowHistoryTransport !== "function"
	) {
		throw new Error(`${format} workflow Inspector build is missing its codec`);
	}
}
