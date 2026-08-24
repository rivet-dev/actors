/**
 * Parallel npm publish with bounded concurrency, exponential backoff retries,
 * and idempotent "already published" handling.
 *
 * Works for both preview and release flows — the only per-flow input is the
 * dist-tag and the `releaseMode` flag (which toggles strict preflight).
 */
import { spawn } from "node:child_process";
import { scoped } from "./logger.js";
import {
	assertDiscoverySanity,
	discoverPackages,
	META_PACKAGES,
	type Package,
} from "./packages.js";

const log = scoped("npm");

export interface PublishAllOptions {
	/** npm dist-tag (e.g. pr-123, main, latest, rc, next). */
	tag: string;
	/** Version being published. Used to repair preview latest tags. */
	version?: string;
	/** Max simultaneous publishes. */
	parallel?: number;
	/** Max retries per package. */
	retries?: number;
	/** Initial backoff in ms (doubled per retry). */
	initialBackoffMs?: number;
	/**
	 * When true, fail hard if every package is already published. Preview
	 * mode treats this as an idempotent no-op; release mode treats it as a
	 * "you forgot to bump the version" error.
	 */
	releaseMode?: boolean;
	/** Include release-only packages like Windows engine-cli artifacts. */
	includeReleaseOnlyPackages?: boolean;
}

export type PublishStatus =
	| "success"
	| "retried-success"
	| "already-exists"
	| "failed";

export interface PublishResult {
	pkg: Package;
	status: PublishStatus;
	attempts: number;
	lastError?: string;
}

export interface PublishSummary {
	results: PublishResult[];
	counts: {
		success: number;
		retried: number;
		alreadyExists: number;
		failed: number;
	};
	elapsedSeconds: number;
}

interface PublishBatchResult {
	results: PublishResult[];
	elapsedMs: number;
}

const ALREADY_PUBLISHED_PATTERNS = [
	"cannot publish over the previously published versions",
	"cannot publish over previously published version",
	"You cannot publish over",
];

function isAlreadyPublished(output: string): boolean {
	return ALREADY_PUBLISHED_PATTERNS.some((p) => output.includes(p));
}

function isRetryable(output: string): boolean {
	if (isAlreadyPublished(output)) return false;
	return (
		output.includes("ECONNRESET") ||
		output.includes("ETIMEDOUT") ||
		output.includes("ENOTFOUND") ||
		output.includes("EAI_AGAIN") ||
		output.includes("socket hang up") ||
		output.includes("npm error 503") ||
		output.includes("npm error 502") ||
		output.includes("npm error 504") ||
		output.includes("npm error 429") ||
		output.includes("ERR_STREAM_PREMATURE_CLOSE") ||
		// Some npm errors don't tag the status clearly; if we don't see a
		// definitive "already published" we can retry once.
		!/npm error (code|E[A-Z]+)/.test(output)
	);
}

function extractError(output: string, maxLines = 3): string {
	const lines = output
		.split("\n")
		.filter((l) => /npm error/i.test(l) && !l.includes("A complete log"))
		.slice(0, maxLines);
	if (lines.length === 0) {
		return output.trim().split("\n").slice(-maxLines).join(" | ");
	}
	return lines.join(" | ");
}

function runNpmPublish(
	pkg: Package,
	tag: string,
): Promise<{ code: number; output: string }> {
	return new Promise((resolvePromise) => {
		const args = ["publish", "--access", "public", "--tag", tag];
		const child = spawn("npm", args, {
			cwd: pkg.dir,
			stdio: ["ignore", "pipe", "pipe"],
			env: process.env,
		});
		const chunks: Buffer[] = [];
		child.stdout.on("data", (c) => chunks.push(c));
		child.stderr.on("data", (c) => chunks.push(c));
		child.on("close", (code) => {
			resolvePromise({
				code: code ?? 1,
				output: Buffer.concat(chunks).toString("utf8"),
			});
		});
	});
}

function runNpmDistTagAdd(
	pkg: Package,
	version: string,
	tag: string,
): Promise<{ code: number; output: string }> {
	return new Promise((resolvePromise) => {
		const child = spawn("npm", ["dist-tag", "add", `${pkg.name}@${version}`, tag], {
			cwd: pkg.dir,
			stdio: ["ignore", "pipe", "pipe"],
			env: process.env,
		});
		const chunks: Buffer[] = [];
		child.stdout.on("data", (c) => chunks.push(c));
		child.stderr.on("data", (c) => chunks.push(c));
		child.on("close", (code) => {
			resolvePromise({
				code: code ?? 1,
				output: Buffer.concat(chunks).toString("utf8"),
			});
		});
	});
}

function npmViewLatestTag(pkg: Package): Promise<string | undefined> {
	return new Promise((resolvePromise, reject) => {
		const child = spawn("npm", ["view", pkg.name, "dist-tags.latest", "--json"], {
			cwd: pkg.dir,
			stdio: ["ignore", "pipe", "pipe"],
			env: process.env,
		});
		const chunks: Buffer[] = [];
		child.stdout.on("data", (c) => chunks.push(c));
		child.stderr.on("data", (c) => chunks.push(c));
		child.on("close", (code) => {
			const output = Buffer.concat(chunks).toString("utf8").trim();
			if (code !== 0) {
				reject(new Error(`npm view ${pkg.name} latest failed: ${output}`));
				return;
			}
			if (!output || output === "null") {
				resolvePromise(undefined);
				return;
			}
			try {
				const parsed = JSON.parse(output);
				resolvePromise(typeof parsed === "string" ? parsed : undefined);
			} catch {
				resolvePromise(output);
			}
		});
	});
}

async function publishOne(
	pkg: Package,
	opts: Required<
		Pick<PublishAllOptions, "tag" | "retries" | "initialBackoffMs">
	>,
): Promise<PublishResult> {
	for (let attempt = 1; attempt <= opts.retries + 1; attempt++) {
		const { code, output } = await runNpmPublish(pkg, opts.tag);
		if (code === 0) {
			return {
				pkg,
				status: attempt === 1 ? "success" : "retried-success",
				attempts: attempt,
			};
		}
		if (isAlreadyPublished(output)) {
			return { pkg, status: "already-exists", attempts: attempt };
		}
		if (!isRetryable(output) || attempt > opts.retries) {
			return {
				pkg,
				status: "failed",
				attempts: attempt,
				lastError: extractError(output),
			};
		}
		const delay = opts.initialBackoffMs * 2 ** (attempt - 1);
		log.info(`  [retry ${attempt}/${opts.retries}] ${pkg.name} — waiting ${delay}ms`);
		await new Promise((r) => setTimeout(r, delay));
	}
	return { pkg, status: "failed", attempts: opts.retries + 1 };
}

export async function repairBranchPreviewLatestTags(
	repoRoot: string,
	opts: Required<Pick<PublishAllOptions, "tag" | "version">> &
		Pick<PublishAllOptions, "includeReleaseOnlyPackages">,
): Promise<void> {
	const previewPrefix = `0.0.0-${opts.tag}.`;
	const packages = discoverPackages(repoRoot, {
		includeReleaseOnly: opts.includeReleaseOnlyPackages,
	});

	for (const pkg of packages) {
		const latest = await npmViewLatestTag(pkg);
		if (
			latest === undefined ||
			latest === opts.version ||
			!latest.startsWith(previewPrefix)
		) {
			continue;
		}

		log.info(
			`repairing ${pkg.name} latest tag: ${latest} -> ${opts.version}`,
		);
		const result = await runNpmDistTagAdd(pkg, opts.version, "latest");
		if (result.code !== 0) {
			throw new Error(
				`npm dist-tag add ${pkg.name}@${opts.version} latest failed: ${extractError(result.output)}`,
			);
		}
	}
}

function printResult(r: PublishResult): void {
	const name = r.pkg.name.padEnd(48);
	const symbol =
		r.status === "success" || r.status === "retried-success"
			? "✓"
			: r.status === "already-exists"
				? "="
				: "✗";
	const suffix =
		r.status === "retried-success"
			? ` (after ${r.attempts} attempts)`
			: r.status === "failed"
				? ` — ${r.lastError ?? "unknown error"}`
				: "";
	log.info(`  ${symbol} ${name}${suffix}`);
}

async function publishBatch(
	packages: Package[],
	opts: Required<
		Pick<PublishAllOptions, "tag" | "parallel" | "retries" | "initialBackoffMs">
	>,
): Promise<PublishBatchResult> {
	const queue = [...packages];
	const results: PublishResult[] = [];
	const startedAt = Date.now();

	async function worker(): Promise<void> {
		while (true) {
			const pkg = queue.shift();
			if (!pkg) return;
			const result = await publishOne(pkg, {
				tag: opts.tag,
				retries: opts.retries,
				initialBackoffMs: opts.initialBackoffMs,
			});
			printResult(result);
			results.push(result);
		}
	}

	const workers: Promise<void>[] = [];
	for (let i = 0; i < Math.min(opts.parallel, packages.length); i++) {
		workers.push(worker());
	}
	await Promise.all(workers);

	return {
		results,
		elapsedMs: Date.now() - startedAt,
	};
}

export async function publishAll(
	repoRoot: string,
	opts: PublishAllOptions,
): Promise<PublishSummary> {
	const parallel = opts.parallel ?? 16;
	const retries = opts.retries ?? 3;
	const initialBackoffMs = opts.initialBackoffMs ?? 2000;
	const tag = opts.tag;

	const packages = discoverPackages(repoRoot, {
		includeReleaseOnly: opts.includeReleaseOnlyPackages,
	});
	assertDiscoverySanity(packages);

	log.info(
		`publishing ${packages.length} packages | tag=${tag} | parallel=${parallel} | retries=${retries}`,
	);

	const metaNames = new Set(META_PACKAGES.map((p) => p.meta));
	const platformPackages = packages.filter((p) =>
		META_PACKAGES.some(({ platformPrefix }) =>
			p.name.startsWith(platformPrefix),
		),
	);
	const metaPackages = packages.filter((p) => metaNames.has(p.name));
	const otherPackages = packages.filter(
		(p) =>
			!metaNames.has(p.name) &&
			!META_PACKAGES.some(({ platformPrefix }) =>
				p.name.startsWith(platformPrefix),
			),
	);

	const batchOpts = { tag, parallel, retries, initialBackoffMs };
	let elapsedMs = 0;
	const results: PublishResult[] = [];
	for (const [label, batch] of [
		["platform packages", platformPackages],
		["meta packages", metaPackages],
		["regular packages", otherPackages],
	] as const) {
		if (batch.length === 0) continue;
		log.info(`publishing ${label} (${batch.length})`);
		const batchResult = await publishBatch(batch, batchOpts);
		elapsedMs += batchResult.elapsedMs;
		results.push(...batchResult.results);
		const failed = batchResult.results.filter((r) => r.status === "failed");
		if (failed.length > 0) {
			log.error(`${label} failed; not publishing later batches`);
			break;
		}
	}

	const elapsed = elapsedMs / 1000;
	const counts = {
		success: results.filter((r) => r.status === "success").length,
		retried: results.filter((r) => r.status === "retried-success").length,
		alreadyExists: results.filter((r) => r.status === "already-exists").length,
		failed: results.filter((r) => r.status === "failed").length,
	};

	log.info("");
	log.info(`summary (${elapsed.toFixed(1)}s)`);
	log.info(`  ${counts.success} succeeded`);
	if (counts.retried > 0) log.info(`  ${counts.retried} succeeded after retry`);
	if (counts.alreadyExists > 0)
		log.info(`  ${counts.alreadyExists} already published (no-op)`);
	if (counts.failed > 0) {
		log.error(`  ${counts.failed} FAILED`);
		for (const r of results.filter((x) => x.status === "failed")) {
			log.error(`    - ${r.pkg.name}: ${r.lastError}`);
		}
		throw new Error(`${counts.failed} npm package(s) failed to publish`);
	}

	// In release mode, if *every* package was already published, treat it as
	// an error — almost certainly a missed version bump. Reruns of successful
	// releases are OK because partial rerun (a few packages re-publish) still
	// has at least one success.
	if (
		opts.releaseMode &&
		counts.success === 0 &&
		counts.retried === 0 &&
		counts.failed === 0 &&
		counts.alreadyExists === packages.length
	) {
		// This usually means a forgotten version bump, but it also happens on a
		// legitimate re-run after npm fully published and a later pipeline step
		// (crates.io, git tag, GitHub release) failed. Warn instead of failing so
		// the re-run can reach those downstream steps; they are idempotent.
		log.warn(
			`release mode: all ${packages.length} packages already published at this version. Continuing (assuming a re-run); ensure the version was bumped.`,
		);
	}

	return { results, counts, elapsedSeconds: elapsed };
}
