/**
 * Single source of truth for the set of packages we publish.
 *
 * Discovery order matters: platform-specific packages are returned first so
 * they land on npm before their meta packages. `rivetkit` (the meta-meta
 * package users install) depends on `@rivetkit/rivetkit-napi` which in turn
 * has `optionalDependencies` on the platform packages — npm only resolves
 * those optionals at install time, so they must exist on the registry before
 * anyone installs the meta.
 *
 * NOTE: `@rivetkit/sqlite-native` and `@rivetkit/sqlite-wasm` were removed from
 * the workspace. Their replacements are `@rivetkit/rivetkit-napi` (which links
 * the sqlite-native Rust crate statically via `libsqlite3-sys`) and
 * `@rivetkit/rivetkit-wasm` (a single wasm bundle with no platform variants).
 * The deleted names remain in EXCLUDED so any stray package.json on disk is
 * still skipped by discovery.
 */
import { execSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

export interface Package {
	name: string;
	/** Directory containing the package.json (absolute). */
	dir: string;
	/** Directory relative to repo root. */
	relDir: string;
}

export interface DiscoverPackagesOptions {
	includeReleaseOnly?: boolean;
}

/**
 * Packages excluded from discovery (private, built separately, or otherwise
 * not publishable). Single source of truth — referenced by `bumpPackageJsons`,
 * `publishAll`, and any future consumer.
 */
export const EXCLUDED = new Set<string>([
	"@rivetkit/shared-data",
	"@rivetkit/engine-frontend",
	"@rivetkit/mcp-hub",
	"@rivetkit/sqlite-native",
	"@rivetkit/sqlite-wasm",
	"example-agent-os",
	"example-agent-os-e2e",
]);

/**
 * Meta packages that need `optionalDependencies` injected at publish time.
 * Each meta package's runtime loader requires the platform-specific package
 * for the current host — without the injected optionalDependencies, npm never
 * installs those and the require fails.
 *
 * The committed `package.json` files deliberately do NOT include these — they
 * would pollute non-CI installs with broken version pins — so `bumpPackageJsons`
 * injects them before publish.
 */
export interface MetaPackageSpec {
	/** Name of the meta package. */
	meta: string;
	/** Prefix of the platform-specific packages to inject. */
	platformPrefix: string;
}

export const META_PACKAGES: readonly MetaPackageSpec[] = [
	{
		meta: "@rivetkit/rivetkit-napi",
		platformPrefix: "@rivetkit/rivetkit-napi-",
	},
	{
		meta: "@rivetkit/engine-cli",
		platformPrefix: "@rivetkit/engine-cli-",
	},
	{
		meta: "@rivetkit/cli",
		platformPrefix: "@rivetkit/cli-",
	},
];

export const RELEASE_ONLY_PACKAGES = new Set<string>([
	"@rivetkit/rivetkit-napi-win32-x64-msvc",
	"@rivetkit/engine-cli-win32-x64",
	"@rivetkit/cli-win32-x64",
]);

function isPublishable(pkg: { name?: string; private?: boolean }): boolean {
	if (!pkg.name) return false;
	if (pkg.private) return false;
	if (EXCLUDED.has(pkg.name)) return false;
	return true;
}

function readPackageJson(
	dir: string,
): { name?: string; private?: boolean } | null {
	const pkgPath = join(dir, "package.json");
	if (!existsSync(pkgPath)) return null;
	try {
		return JSON.parse(readFileSync(pkgPath, "utf8"));
	} catch {
		return null;
	}
}

export function discoverPackages(
	repoRoot: string,
	opts: DiscoverPackagesOptions = {},
): Package[] {
	const includeReleaseOnly = opts.includeReleaseOnly ?? true;
	const packages: Package[] = [];
	const seen = new Set<string>();

	const add = (dir: string) => {
		const absDir = resolve(dir);
		const pkg = readPackageJson(absDir);
		if (!pkg) return;
		if (!pkg.name) return;
		if (!isPublishable(pkg)) return;
		if (!includeReleaseOnly && RELEASE_ONLY_PACKAGES.has(pkg.name)) return;
		if (seen.has(pkg.name)) return;
		seen.add(pkg.name);
		packages.push({
			name: pkg.name,
			dir: absDir,
			relDir: relative(repoRoot, absDir),
		});
	};

	// 1. Platform-specific packages first. These are `optionalDependencies` of
	//    their meta packages and must exist on npm before the meta package
	//    resolves at install time.
	//    - rivetkit-napi: the N-API addon (.node files)
	//    - engine-cli: the rivet-engine binary
	//    - cli: the rivet CLI binary plus bundled rivet-engine
	for (const metaRelDir of [
		"rivetkit-typescript/packages/rivetkit-napi/npm",
		"rivetkit-typescript/packages/engine-cli/npm",
		"rivetkit-typescript/packages/cli/npm",
	]) {
		const npmDir = join(repoRoot, metaRelDir);
		if (!existsSync(npmDir)) continue;
		for (const entry of readdirSync(npmDir).sort()) {
			const platDir = join(npmDir, entry);
			if (!statSync(platDir).isDirectory()) continue;
			add(platDir);
		}
	}

	// 2. pnpm workspace packages (rivetkit + @rivetkit/*).
	const pnpmList = execSync("pnpm -r list --json", {
		cwd: repoRoot,
		encoding: "utf8",
		maxBuffer: 16 * 1024 * 1024,
	});
	const workspacePkgs: Array<{
		name: string;
		path: string;
		private?: boolean;
	}> = JSON.parse(pnpmList);
	for (const p of workspacePkgs) {
		if (!p.name) continue;
		if (
			!p.name.startsWith("@rivetkit/") &&
			p.name !== "rivetkit" &&
			p.name !== "@rivet-dev/workflow-world"
		) continue;
		add(p.path);
	}

	// 3. Engine SDK TypeScript packages.
	const engineSdkDir = join(repoRoot, "engine/sdks/typescript");
	if (existsSync(engineSdkDir)) {
		for (const entry of readdirSync(engineSdkDir).sort()) {
			add(join(engineSdkDir, entry));
		}
	}

	// 4. Shared TypeScript packages.
	const sharedDir = join(repoRoot, "shared/typescript");
	if (existsSync(sharedDir)) {
		for (const entry of readdirSync(sharedDir).sort()) {
			add(join(sharedDir, entry));
		}
	}

	return packages;
}

/**
 * Returns a map of meta package name → list of platform package names that
 * should be injected as its `optionalDependencies`.
 */
export function buildMetaPlatformMap(
	packages: Package[],
): Map<string, string[]> {
	return new Map(
		META_PACKAGES.map(({ meta, platformPrefix }) => [
			meta,
			packages
				.filter((p) => p.name.startsWith(platformPrefix))
				.map((p) => p.name)
				.sort(),
		]),
	);
}

/**
 * Sanity check — asserts the expected root packages are present. Fail loud in
 * CI if discovery silently regressed. Called at the top of subcommands that
 * touch the full set.
 */
export function assertDiscoverySanity(packages: Package[]): void {
	const byName = new Set(packages.map((p) => p.name));
	const required = [
		"rivetkit",
		"@rivetkit/react",
		"@rivetkit/rivetkit-napi",
		"@rivetkit/engine-cli",
		"@rivetkit/cli",
	];
	const missing = required.filter((r) => !byName.has(r));
	if (missing.length > 0) {
		throw new Error(
			`package discovery missing required packages: ${missing.join(", ")}`,
		);
	}
	// Each meta must have at least one platform package.
	const metaMap = buildMetaPlatformMap(packages);
	for (const { meta } of META_PACKAGES) {
		const plats = metaMap.get(meta) ?? [];
		if (plats.length === 0) {
			throw new Error(
				`meta package ${meta} has zero platform packages discovered`,
			);
		}
	}
}
