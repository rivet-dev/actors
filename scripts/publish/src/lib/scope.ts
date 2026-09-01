/**
 * Selective-publish scope. Single source of truth mapping a preview's selected
 * target groups to the native build targets, the wasm/docker jobs, and the npm
 * package families that are in scope for the run.
 *
 * Preview publishes (`trigger === "branch"`) may narrow scope to speed up the
 * common single-target case. Release publishes always cover every group so a
 * cut is never partial.
 */

/** User-facing target groups selected via the `targets` workflow_dispatch input. */
export type TargetGroup = "rivetkit" | "container-runner" | "engine" | "cli";

/** Native artifacts produced by the `build` matrix (matrix `build_target`). */
export type BuildTarget = "rivetkit-napi" | "engine" | "container-runner" | "cli";

/**
 * npm package families. `container-runner` has no npm package (it ships as an
 * R2 binary only), so it is not a family.
 */
export type PackageFamily = "rivetkit" | "engine" | "cli";

export const ALL_GROUPS: readonly TargetGroup[] = [
	"rivetkit",
	"container-runner",
	"engine",
	"cli",
];

/** Classify a publishable package into its family by name. */
export function packageFamily(name: string): PackageFamily {
	if (name === "@rivetkit/engine-cli" || name.startsWith("@rivetkit/engine-cli-")) {
		return "engine";
	}
	if (name === "@rivetkit/cli" || name.startsWith("@rivetkit/cli-")) {
		return "cli";
	}
	// Everything else (rivetkit, @rivetkit/rivetkit-napi[-*], wasm, react,
	// engine SDK + shared TS packages) is part of the rivetkit family.
	return "rivetkit";
}

/** Parse the raw `targets` input into a validated, de-duplicated group list. */
export function parseTargetGroups(raw: string | undefined): TargetGroup[] {
	if (!raw || raw.trim() === "" || raw.trim() === "all") {
		return [...ALL_GROUPS];
	}
	const parts = raw
		.split(",")
		.map((p) => p.trim())
		.filter((p) => p.length > 0);
	const out: TargetGroup[] = [];
	for (const p of parts) {
		if (p === "all") return [...ALL_GROUPS];
		if (!(ALL_GROUPS as readonly string[]).includes(p)) {
			throw new Error(
				`unknown target "${p}" (expected one of: ${["all", ...ALL_GROUPS].join(", ")})`,
			);
		}
		if (!out.includes(p as TargetGroup)) out.push(p as TargetGroup);
	}
	if (out.length === 0) return [...ALL_GROUPS];
	return out;
}

export function isAllGroups(groups: readonly TargetGroup[]): boolean {
	return ALL_GROUPS.every((g) => groups.includes(g));
}

/** npm families that should be built + published for the selected groups. */
export function scopedFamilies(
	groups: readonly TargetGroup[],
): Set<PackageFamily> {
	const families = new Set<PackageFamily>();
	if (groups.includes("rivetkit")) families.add("rivetkit");
	if (groups.includes("engine")) families.add("engine");
	if (groups.includes("cli")) families.add("cli");
	return families;
}

export interface BuildScope {
	/** Native `build` matrix targets in scope. */
	buildTargets: BuildTarget[];
	/** Whether the `build-wasm` job runs. */
	buildWasm: boolean;
	/** Whether the `docker-images` job runs. */
	buildDocker: boolean;
}

/**
 * Resolve which native builds a set of groups requires.
 *
 * `cli` implies an `engine` build because the CLI platform packages bundle the
 * rivet-engine binary via a file copy at publish time. That does not put the
 * `engine` npm family in publish scope; it only ensures the binary artifact
 * exists to copy.
 */
export function buildScope(groups: readonly TargetGroup[]): BuildScope {
	const targets = new Set<BuildTarget>();
	let buildWasm = false;
	let buildDocker = false;
	if (groups.includes("rivetkit")) {
		targets.add("rivetkit-napi");
		buildWasm = true;
	}
	if (groups.includes("engine")) {
		targets.add("engine");
		buildDocker = true;
	}
	if (groups.includes("cli")) {
		targets.add("cli");
		targets.add("engine");
	}
	if (groups.includes("container-runner")) {
		targets.add("container-runner");
	}
	return { buildTargets: [...targets], buildWasm, buildDocker };
}
