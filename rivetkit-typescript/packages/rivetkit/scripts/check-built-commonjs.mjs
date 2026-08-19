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

if (typeof rivetkit.actor !== "function") {
	throw new Error("CommonJS build does not export actor()");
}
