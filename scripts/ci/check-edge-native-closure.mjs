// Guard: keep the Supabase edge adapter free of the native packages the wasm
// runtime never uses (`@rivetkit/rivetkit-napi`, `@rivetkit/engine-cli`,
// `@rivet-dev/services`, `@rivet-dev/agent-os-core`). A Supabase Edge
// Function deploy (Deno eszip)
// snapshots the whole declared npm closure AND statically resolves literal
// dynamic imports, so either can silently re-bloat the deploy by hundreds of MB
// and 413.
//
// Two independent regressions, two checks:
//   1. Dependency closure: a forbidden package re-enters the adapter's prod
//      dependency tree (directly or transitively).
//   2. Literal import in the bundle: rivetkit-core loads these via a computed
//      specifier `import(["@rivetkit","rivetkit-napi"].join("/"))` that esbuild
//      and Deno cannot statically resolve. If someone "simplifies" that to a
//      literal `import("@rivetkit/rivetkit-napi")`, the adapter's pre-bundled
//      dist would contain a statically-resolvable specifier that Deno's eszip
//      snapshots. size-limit and the closure check both miss this (the literal
//      import is external, so it neither inflates the bundle nor changes any
//      `dependencies` field), so we grep the built bundle directly.
//
// Exits 1 if either check fails.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ADAPTER = "@rivetkit/supabase";
const FORBIDDEN = [
	"@rivetkit/rivetkit-napi",
	"@rivetkit/engine-cli",
	"@rivet-dev/services",
	"@rivet-dev/agent-os-core",
];

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
let failed = false;

// --- Check 1: production dependency closure ---
{
	const raw = execFileSync(
		"pnpm",
		["--filter", ADAPTER, "list", "--prod", "--depth", "Infinity", "--json"],
		{ encoding: "utf8" },
	);
	const found = new Set();
	const walk = (deps) => {
		if (!deps) return;
		for (const [name, info] of Object.entries(deps)) {
			if (FORBIDDEN.includes(name)) found.add(name);
			walk(info.dependencies);
		}
	};
	for (const project of JSON.parse(raw)) {
		walk(project.dependencies);
		walk(project.devDependencies);
		walk(project.optionalDependencies);
	}
	if (found.size > 0) {
		failed = true;
		for (const name of found) {
			console.error(
				`::error::forbidden native package '${name}' is in ${ADAPTER}'s production closure; ` +
					"a Supabase edge deploy would embed it and 413. Keep it out of the adapter's deps.",
			);
		}
	} else {
		console.log(`ok: ${ADAPTER} edge closure is free of native packages`);
	}
}

// --- Check 2: no literal native import specifiers in the built bundle ---
{
	const bundles = [
		resolve(repoRoot, "rivetkit-typescript/packages/supabase/dist/mod.mjs"),
		resolve(repoRoot, "rivetkit-typescript/packages/supabase/dist/mod.js"),
	].filter(existsSync);

	if (bundles.length === 0) {
		console.error(
			`::error::${ADAPTER} dist not built; run \`pnpm --filter ${ADAPTER} build\` before this check`,
		);
		failed = true;
	}

	for (const file of bundles) {
		const src = readFileSync(file, "utf8");
		for (const pkg of FORBIDDEN) {
			const p = pkg.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
			// A literal module specifier for a forbidden package inside
			// import(...) / require(...) / `from "..."`. The load-bearing computed
			// form `["@rivetkit","rivetkit-napi"].join("/")` does not match (the
			// full specifier never appears as one quoted string), and error-message
			// strings that merely mention the name are not specifiers.
			const re = new RegExp(
				`(?:\\bimport\\s*\\(|\\brequire\\s*\\(|\\bfrom)\\s*['"\`]${p}['"\`]`,
			);
			if (re.test(src)) {
				failed = true;
				console.error(
					`::error::${file.split("/").pop()} contains a literal import of '${pkg}'. ` +
						"rivetkit-core must load native packages via the computed " +
						'`import([...].join("/"))` form so Deno cannot statically snapshot them. ' +
						"Do not simplify it to a literal specifier.",
				);
			}
		}
	}
	if (bundles.length > 0 && !failed) {
		console.log(`ok: ${ADAPTER} bundle has no literal native import specifiers`);
	}
}

process.exit(failed ? 1 : 0);
