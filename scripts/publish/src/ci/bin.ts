#!/usr/bin/env tsx
/**
 * CI entrypoint. Every workflow step calls exactly one subcommand.
 *
 * Each subcommand is a pure function of its flags — nothing orchestrates
 * other subcommands. The GitHub Actions workflow is the orchestrator.
 *
 * Subcommands accept inputs via flags AND will fall back to re-resolving
 * the `PublishContext` from env vars when flags aren't passed. The workflow
 * uses the `context-output` subcommand to resolve once and pin values as
 * job outputs, then passes those outputs to each subsequent step as flags.
 */
import { readFileSync } from "node:fs";
import { dirname, join, resolve as resolvePath } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { Command } from "commander";
import { $ } from "execa";
import {
	resolveContext,
	writeContextToGithubOutput,
	type PublishContext,
	type Trigger,
} from "../lib/context.js";
import {
	createMultiArchManifests,
	retagManifestsToVersion,
} from "../lib/docker.js";
import {
	createGhRelease,
	tagAndPush,
} from "../lib/git.js";
import { scoped } from "../lib/logger.js";
import { publishAll, repairBranchPreviewLatestTags } from "../lib/npm.js";
import {
	copyPrefix,
	uploadDir,
	uploadFile,
	uploadInstallScripts,
} from "../lib/r2.js";
import { bumpCargoVersions, bumpPackageJsons } from "../lib/version.js";

const log = scoped("ci");

const RUST_CRATES = [
	"rivet-error-macros",
	"rivet-error",
	"rivet-metrics",
	"rivet-util-serde",
	"rivet-actor-runtime-socket-protocol",
	// rivet-envoy-protocol must precede rivet-depot-client, which pins it as an
	// exact dependency. Publishing depot-client first makes cargo fail to
	// resolve rivet-envoy-protocol because it is not yet on crates.io.
	"rivet-envoy-protocol",
	"rivet-depot-client-types",
	"rivet-depot-client",
	"rivetkit-shared-types",
	"rivet-envoy-client",
	"rivetkit-actor-persist",
	"rivetkit-client-protocol",
	"rivetkit-inspector-protocol",
	"rivetkit-client",
	// rivetkit-core has an optional dependency on rivetkit-engine-process, which
	// cargo still requires to be resolvable on crates.io at publish time.
	"rivetkit-engine-process",
	"rivetkit-core",
	"rivetkit",
] as const;

function findRepoRoot(): string {
	if (process.env.GITHUB_WORKSPACE) return process.env.GITHUB_WORKSPACE;
	let dir = dirname(fileURLToPath(import.meta.url));
	for (let i = 0; i < 10; i++) {
		try {
			const ws = join(dir, "pnpm-workspace.yaml");
			readFileSync(ws, "utf-8");
			return dir;
		} catch {
			dir = dirname(dir);
		}
	}
	throw new Error("could not locate repo root");
}

async function crateVersionExists(name: string, version: string): Promise<boolean> {
	const response = await fetch(
		`https://crates.io/api/v1/crates/${encodeURIComponent(name)}/${encodeURIComponent(version)}`,
		{
			headers: {
				"User-Agent": "rivet-release-publisher (https://github.com/rivet-dev/rivet)",
			},
		},
	);
	if (response.status === 200) return true;
	if (response.status === 404) return false;
	throw new Error(
		`crates.io lookup failed for ${name}@${version}: ${response.status} ${response.statusText}`,
	);
}

async function waitForCrateVersion(
	name: string,
	version: string,
	timeoutSeconds: number,
): Promise<void> {
	const deadline = Date.now() + timeoutSeconds * 1000;
	while (Date.now() < deadline) {
		if (await crateVersionExists(name, version)) return;
		await sleep(10_000);
	}
	throw new Error(
		`timed out waiting for crates.io to index ${name}@${version}`,
	);
}

async function cargoPublishWithRateLimitRetry(
	repoRoot: string,
	args: string[],
): Promise<void> {
	for (;;) {
		const result = await $({
			all: true,
			cwd: repoRoot,
			reject: false,
		})`cargo ${args}`;

		if (result.all) process.stdout.write(result.all);
		if (result.exitCode === 0) return;

		const retryAt = parseCratesIoRateLimitRetry(result.all ?? "");
		if (retryAt === undefined) {
			throw new Error(`cargo ${args.join(" ")} failed with exit code ${result.exitCode}`);
		}

		const waitMs = Math.max(retryAt.getTime() - Date.now() + 5_000, 10_000);
		log.info(
			`crates.io rate limited publish; retrying at ${retryAt.toISOString()}`,
		);
		await sleep(waitMs);
	}
}

function parseCratesIoRateLimitRetry(output: string): Date | undefined {
	const match = output.match(/try again after (.+? GMT)/);
	if (!match) return undefined;
	const retryAt = new Date(match[1]);
	if (Number.isNaN(retryAt.getTime())) return undefined;
	return retryAt;
}

const program = new Command();
program.name("ci").description("CI subcommands for the publish flow");

// ---------------------------------------------------------------------------
// context-output — resolve once, write to $GITHUB_OUTPUT for downstream steps
// ---------------------------------------------------------------------------
program
	.command("context-output")
	.description("Resolve publish context and write to $GITHUB_OUTPUT")
	.option("--trigger <trigger>", "Override trigger (branch|release)")
	.option("--version <version>", "Override version")
	.option("--latest <bool>", "Override latest")
	.option("--branch <name>", "Override branch name")
	.action(async (opts) => {
		const overrides: Parameters<typeof resolveContext>[0] = {};
		if (opts.trigger) overrides.trigger = opts.trigger as Trigger;
		if (opts.version) overrides.version = opts.version;
		if (opts.latest !== undefined) {
			overrides.latest = opts.latest === "true";
		}
		if (opts.branch) overrides.branch = opts.branch;
		const ctx = await resolveContext(overrides);
		log.info(
			`resolved: trigger=${ctx.trigger} version=${ctx.version} npm_tag=${ctx.npmTag} sha=${ctx.sha} latest=${ctx.latest}${ctx.branch !== undefined ? ` branch=${ctx.branch}` : ""}`,
		);
		writeContextToGithubOutput(ctx);
	});

// ---------------------------------------------------------------------------
// bump-versions — rewrite package.jsons to a version (+ inject optionalDeps)
// ---------------------------------------------------------------------------
program
	.command("bump-versions")
	.description("Rewrite every publishable package.json to the given version")
	.option("--version <version>", "Version to write (defaults to resolved context)")
	.option(
		"--version-only",
		"Only rewrite package.json version fields without publish-time dependency injection",
	)
	.option(
		"--repository <owner/repo>",
		"GitHub repository recorded in publish-time package metadata (defaults to GITHUB_REPOSITORY)",
	)
	.option("--dry-run", "Do not write, only report")
	.action(async (opts) => {
		const repoRoot = findRepoRoot();
		const ctx = await resolveContext();
		const version = opts.version ?? ctx.version;
		await bumpPackageJsons(repoRoot, version, {
			dryRun: !!opts.dryRun,
			includeReleaseOnlyPackages: ctx.trigger === "release",
			versionOnly: !!opts.versionOnly,
			repository: opts.repository ?? process.env.GITHUB_REPOSITORY,
		});
		await bumpCargoVersions(repoRoot, version, {
			dryRun: !!opts.dryRun,
		});
	});

// ---------------------------------------------------------------------------
// publish-npm — parallel npm publish with retries
// ---------------------------------------------------------------------------
program
	.command("publish-npm")
	.description("Publish all discovered packages to npm")
	.option("--tag <tag>", "npm dist-tag (defaults to resolved context)")
	.option("--parallel <n>", "Max simultaneous publishes", "16")
	.option("--retries <n>", "Retries per package", "3")
	.option("--release-mode", "Fail if every package is already published")
	.action(async (opts) => {
		const repoRoot = findRepoRoot();
		const ctx = await resolveContext();
		let tag: string = opts.tag;
		let releaseMode: boolean | undefined = opts.releaseMode;
		if (!tag || releaseMode === undefined) {
			tag = tag ?? ctx.npmTag;
			if (opts.releaseMode === undefined) {
				releaseMode = ctx.trigger === "release";
			}
		}
		await publishAll(repoRoot, {
			tag,
			version: ctx.version,
			includeReleaseOnlyPackages: releaseMode,
			parallel: Number(opts.parallel),
			retries: Number(opts.retries),
			releaseMode,
		});
		if (!releaseMode) {
			await repairBranchPreviewLatestTags(repoRoot, {
				tag,
				version: ctx.version,
				includeReleaseOnlyPackages: releaseMode,
			});
		}
	});

// ---------------------------------------------------------------------------
// publish-crates — ordered, idempotent crates.io publish
// ---------------------------------------------------------------------------
program
	.command("publish-crates")
	.description("Publish Rust SDK crates to crates.io in dependency order")
	.option("--version <version>", "Version to publish (defaults to resolved context)")
	.option("--wait-seconds <n>", "Max wait for crates.io indexing per crate", "600")
	.option("--dry-run", "Run cargo publish --dry-run for the first crate only")
	.option("--allow-dirty", "Pass --allow-dirty to cargo publish")
	.action(async (opts) => {
		const repoRoot = findRepoRoot();
		const version = opts.version ?? (await resolveContext()).version;
		const waitSeconds = Number(opts.waitSeconds);
		if (!Number.isFinite(waitSeconds) || waitSeconds <= 0) {
			throw new Error("--wait-seconds must be a positive number");
		}

		if (opts.dryRun) {
			const crate = RUST_CRATES[0];
			log.info(
				`dry-running ${crate}; later crates require earlier versions to exist in the crates.io index`,
			);
			const args = ["publish", "-p", crate, "--dry-run"];
			if (opts.allowDirty) args.push("--allow-dirty");
			await $({ stdio: "inherit", cwd: repoRoot })`cargo ${args}`;
			return;
		}

		if (!process.env.CARGO_REGISTRY_TOKEN) {
			throw new Error("CARGO_REGISTRY_TOKEN must be set to publish crates");
		}

		for (const crate of RUST_CRATES) {
			if (await crateVersionExists(crate, version)) {
				log.info(`skipping ${crate}@${version}; already published`);
				continue;
			}

			log.info(`publishing ${crate}@${version}`);
			const args = ["publish", "-p", crate];
			if (opts.allowDirty) args.push("--allow-dirty");
			await cargoPublishWithRateLimitRetry(repoRoot, args);
			log.info(`waiting for crates.io to index ${crate}@${version}`);
			await waitForCrateVersion(crate, version, waitSeconds);
		}
	});

// ---------------------------------------------------------------------------
// upload-r2 — upload a directory to rivet/{sha}/engine/ (or a custom dest)
// ---------------------------------------------------------------------------
program
	.command("upload-r2")
	.description("Upload an artifact directory to R2")
	.requiredOption("--source <dir>", "Local directory to upload")
	.option("--sha <sha>", "Short sha (defaults to resolved context)")
	.option("--name <name>", "R2 sub-path name", "engine")
	.action(async (opts) => {
		const sha = opts.sha ?? (await resolveContext()).sha;
		const prefix = `rivet/${sha}/${opts.name}/`;
		await uploadDir(opts.source, prefix);
	});

// ---------------------------------------------------------------------------
// copy-r2 — copy rivet/{sha}/engine/ to rivet/{version}/engine/ (+latest)
// ---------------------------------------------------------------------------
program
	.command("copy-r2")
	.description("Copy R2 artifacts from {sha} to {version} (+latest)")
	.option("--sha <sha>", "Source sha (defaults to resolved context)")
	.option("--version <version>", "Target version (defaults to resolved context)")
	.option("--latest <bool>", "Also copy to /latest/ (defaults to resolved context)")
	.option("--name <name>", "R2 sub-path name", "engine")
	.action(async (opts) => {
		const ctx = await resolveContext();
		const sha: string = opts.sha ?? ctx.sha;
		const version: string = opts.version ?? ctx.version;
		const latest =
			opts.latest !== undefined ? opts.latest === "true" : ctx.latest;
		const source = `rivet/${sha}/${opts.name}/`;
		await copyPrefix(source, `rivet/${version}/${opts.name}/`);
		if (latest) {
			await copyPrefix(source, `rivet/latest/${opts.name}/`);
		}
	});

// ---------------------------------------------------------------------------
// upload-install-scripts — template install.sh/ps1 with the release version
// ---------------------------------------------------------------------------
program
	.command("upload-install-scripts")
	.description("Upload install.sh/install.ps1 templated with the version")
	.requiredOption("--scripts-dir <dir>", "Directory containing install.sh and install.ps1")
	.option("--version <version>", "Version to template (defaults to context)")
	.option("--latest <bool>", "Also upload to /latest/")
	.action(async (opts) => {
		const ctx = await resolveContext();
		const version: string = opts.version ?? ctx.version;
		const latest =
			opts.latest !== undefined ? opts.latest === "true" : ctx.latest;
		const scripts = [
			resolvePath(opts.scriptsDir, "install.sh"),
			resolvePath(opts.scriptsDir, "install.ps1"),
		];
		await uploadInstallScripts(scripts, version);
		if (latest) await uploadInstallScripts(scripts, "latest");
	});

// ---------------------------------------------------------------------------
// upload-devtools — build & upload @rivetkit/devtools to R2 (release only)
// ---------------------------------------------------------------------------
program
	.command("upload-devtools")
	.description("Build @rivetkit/devtools and upload dist to R2")
	.option("--sha <sha>", "Short sha (defaults to resolved context)")
	.option("--version <version>", "Version (defaults to resolved context)")
	.option("--latest <bool>", "Also copy to /latest/ (defaults to resolved context)")
	.action(async (opts) => {
		const repoRoot = findRepoRoot();
		const ctx = await resolveContext();
		const sha = opts.sha ?? ctx.sha;
		const version: string = opts.version ?? ctx.version;
		const latest =
			opts.latest !== undefined ? opts.latest === "true" : ctx.latest;
		log.info("building @rivetkit/devtools");
		await $({ stdio: "inherit", cwd: repoRoot })`pnpm build -F @rivetkit/devtools`;
		const dist = resolvePath(
			repoRoot,
			"rivetkit-typescript/packages/devtools/dist",
		);
		// Upload to commit path first, then copy to version/latest (matches the
		// R2 engine path shape so both can be promoted the same way).
		await uploadDir(dist, `rivet/${sha}/devtools/`);
		await copyPrefix(
			`rivet/${sha}/devtools/`,
			`rivet/${version}/devtools/`,
		);
		if (latest) {
			await copyPrefix(
				`rivet/${sha}/devtools/`,
				"rivet/latest/devtools/",
			);
		}
	});

// ---------------------------------------------------------------------------
// docker-manifest — create multi-arch manifest for {sha}
// ---------------------------------------------------------------------------
program
	.command("docker-manifest")
	.description("Create multi-arch Docker manifests for the commit sha")
	.option("--sha <sha>", "Short sha (defaults to resolved context)")
	.action(async (opts) => {
		const sha = opts.sha ?? (await resolveContext()).sha;
		await createMultiArchManifests(sha);
	});

// ---------------------------------------------------------------------------
// docker-retag — retag {sha} manifests to {version} (+latest)
// ---------------------------------------------------------------------------
program
	.command("docker-retag")
	.description("Retag multi-arch manifests from {sha} to {version}")
	.option("--sha <sha>", "Source sha (defaults to resolved context)")
	.option("--version <version>", "Target version (defaults to resolved context)")
	.option("--latest <bool>", "Also tag as latest (defaults to resolved context)")
	.action(async (opts) => {
		const ctx = await resolveContext();
		const sha: string = opts.sha ?? ctx.sha;
		const version: string = opts.version ?? ctx.version;
		const latest =
			opts.latest !== undefined ? opts.latest === "true" : ctx.latest;
		await retagManifestsToVersion(sha, version, latest);
	});

// ---------------------------------------------------------------------------
// git-tag — force-create and push v{version}
// ---------------------------------------------------------------------------
program
	.command("git-tag")
	.description("Create and force-push v{version} tag")
	.option("--version <version>", "Version (defaults to resolved context)")
	.action(async (opts) => {
		const version =
			opts.version ?? (await resolveContext()).version;
		await tagAndPush(version);
	});

// ---------------------------------------------------------------------------
// gh-release — create or update GitHub release for the version
// ---------------------------------------------------------------------------
program
	.command("gh-release")
	.description("Create or update GitHub release")
	.option("--version <version>", "Version (defaults to resolved context)")
	.action(async (opts) => {
		const version =
			opts.version ?? (await resolveContext()).version;
		await createGhRelease(version);
	});

// ---------------------------------------------------------------------------
// comment-pr — upsert the "Preview packages published to npm" PR comment
// ---------------------------------------------------------------------------
program
	.command("comment-pr")
	.description("Upsert the preview PR comment with install instructions")
	.requiredOption("--pr-number <number>", "PR number to comment on")
	.option("--version <version>", "Version (defaults to context)")
	.option("--tag <tag>", "npm dist-tag (defaults to context)")
	.action(async (opts) => {
		const ctx = await resolveContext();
		const prNumber = Number(opts.prNumber);
		const version: string = opts.version ?? ctx.version;
		const tag: string = opts.tag ?? ctx.npmTag;
		if (!Number.isFinite(prNumber)) {
			throw new Error("comment-pr requires a numeric --pr-number");
		}
		const repo = process.env.GITHUB_REPOSITORY;
		if (!repo) {
			throw new Error("GITHUB_REPOSITORY env var not set");
		}
		const marker = "Preview packages published to npm";
		const body = [
			`## ${marker}`,
			"",
			"Install with:",
			"```sh",
			`npm install rivetkit@${tag}`,
			"```",
			"",
			`All packages published as \`${version}\` with tag \`${tag}\`.`,
			"",
			"Engine binary is shipped via `@rivetkit/engine-cli` on linux-x64-musl, linux-arm64-musl, darwin-x64, and darwin-arm64. `@rivetkit/cli` ships the `rivet` binary plus bundled engine on the same platforms. Windows users should use release versions or set `RIVET_ENGINE_BINARY` / `RIVET_CLI_BINARY`.",
			"",
			"Docker images:",
			"```sh",
			`docker pull rivetdev/engine:slim-${ctx.sha}`,
			`docker pull rivetdev/engine:full-${ctx.sha}`,
			"```",
			"",
			"<details>",
			"<summary>Individual packages</summary>",
			"",
			"```sh",
			`npm install rivetkit@${tag}`,
			`npm install @rivetkit/react@${tag}`,
			`npm install @rivetkit/rivetkit-napi@${tag}`,
			`npm install @rivetkit/workflow-engine@${tag}`,
			"```",
			"",
			"</details>",
		].join("\n");

		// Find existing comment by marker, update if present, else create.
		// `gh api -f body=<value>` sends string fields directly (no @file needed).
		const { stdout: commentsJson } = await $({
			env: process.env as Record<string, string>,
		})`gh api repos/${repo}/issues/${String(prNumber)}/comments --paginate`;
		const comments = JSON.parse(commentsJson) as Array<{
			id: number;
			body: string;
		}>;
		const existing = comments.find((c) => c.body.includes(marker));
		if (existing) {
			log.info(`updating existing PR comment ${existing.id}`);
			await $({
				stdio: "inherit",
				env: process.env as Record<string, string>,
			})`gh api repos/${repo}/issues/comments/${String(existing.id)} -X PATCH -f ${`body=${body}`}`;
		} else {
			log.info("creating new PR comment");
			await $({
				stdio: "inherit",
				env: process.env as Record<string, string>,
			})`gh api repos/${repo}/issues/${String(prNumber)}/comments -f ${`body=${body}`}`;
		}
	});

program.parseAsync(process.argv).catch((err) => {
	log.error(String(err?.stack ?? err));
	process.exit(1);
});
