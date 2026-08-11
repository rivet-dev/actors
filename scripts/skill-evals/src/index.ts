import { spawn, ChildProcess } from "node:child_process";
import { readdir, readFile, mkdir, writeFile, access, rm } from "node:fs/promises";
import { createWriteStream } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { parseArgs } from "node:util";
import { z } from "zod";
import { SandboxAgent } from "sandbox-agent";

// --- Terminal styling ---

const { fmt, STDOUT_IS_TTY } = (() => {
	const STDOUT_IS_TTY = !!process.stdout.isTTY;
	const enabled =
		STDOUT_IS_TTY &&
		process.env.TERM !== "dumb" &&
		!("NO_COLOR" in process.env);

	const esc = (code: string) => (enabled ? `\u001b[${code}m` : "");
	const wrap = (text: string, code: string) => (enabled ? `${esc(code)}${text}${esc("0")}` : text);

	const fmt = {
		bold: (s: string) => wrap(s, "1"),
		dim: (s: string) => wrap(s, "2"),
		gray: (s: string) => wrap(s, "90"),
		cyan: (s: string) => wrap(s, "36"),
		yellow: (s: string) => wrap(s, "33;1"),
		red: (s: string) => wrap(s, "31;1"),
		green: (s: string) => wrap(s, "32;1"),
	};

	return { fmt, STDOUT_IS_TTY };
})();

// --- Config ---

const {
	ROOT,
	PACKAGE_ROOT,
	REPO_ROOT,
	EVALS_DIR,
	RESULTS_DIR,
	JUDGE_SYSTEM_PATH,
	SKILLS_DIR,
	EVAL_PROJECTS_DIR,
	FRICTION_LOG_FILENAME,
} = (() => {
	const ROOT = dirname(new URL(import.meta.url).pathname);
	const PACKAGE_ROOT = resolve(ROOT, "..");
	const REPO_ROOT = resolve(PACKAGE_ROOT, "../..");
	const EVALS_DIR = join(PACKAGE_ROOT, "evals");
	const RESULTS_DIR = join(PACKAGE_ROOT, "results");
	const JUDGE_SYSTEM_PATH = join(ROOT, "judge-system.md");
	const SKILLS_DIR = join(REPO_ROOT, "website/dist/metadata/skills");
	const EVAL_PROJECTS_DIR = join(REPO_ROOT, ".context", "skill-eval-projects");
	const FRICTION_LOG_FILENAME = "FRICTION.md";

	return {
		ROOT,
		PACKAGE_ROOT,
		REPO_ROOT,
		EVALS_DIR,
		RESULTS_DIR,
		JUDGE_SYSTEM_PATH,
		SKILLS_DIR,
		EVAL_PROJECTS_DIR,
		FRICTION_LOG_FILENAME,
	};
})();

// --- CLI args ---

const {
	AGENT,
	AGENT_SYSTEM,
	EVAL_NAME,
	MODEL,
	VARIANT,
	JUDGE,
	JUDGE_MODEL,
	JUDGE_VARIANT,
	KEEP_TMP,
} = (() => {
	const { values: args } = parseArgs({
		options: {
			agent: { type: "string", default: "claude" },
			"agent-system": { type: "string", default: "" },
			eval: { type: "string", default: "" },
			model: { type: "string", default: "" },
			variant: { type: "string", default: "" },
			judge: { type: "string", default: "claude" },
			"judge-model": { type: "string", default: "" },
			"judge-variant": { type: "string", default: "" },
			"keep-tmp": { type: "boolean", default: false },
			help: { type: "boolean", short: "h", default: false },
		},
	});

	if (args.help) {
		console.log(`Usage: npx tsx src/index.ts [options]

Options:
  --agent  claude|codex   Agent for project generation (default: claude)
  --agent-system <text>   Extra system prompt for the project agent (supports {{TMP_DIR}} and {{FRICTION_LOG_PATH}})
  --eval   <name>         Eval to run (required)
  --model  <name>         Model for agent under test (default: opus for claude, 5.3 for codex)
  --variant <name>        Model variant (default: high for codex, empty otherwise)
  --judge  claude|codex   Agent for judge (default: claude)
  --judge-model <name>    Model for judge (default: opus for claude, 5.3 for codex)
  --judge-variant <name>  Model variant for judge (default: high for codex, empty otherwise)
  --keep-tmp              Keep temp dirs even on success
  -h, --help              Show this help`);
		process.exit(0);
	}

	const AGENT = args.agent!;
	const AGENT_SYSTEM = args["agent-system"]!;
	const EVAL_NAME = args.eval!;
	let MODEL = args.model!.trim();
	let VARIANT = args.variant!.trim();
	const JUDGE = args.judge!;
	let JUDGE_MODEL = args["judge-model"]!.trim();
	let JUDGE_VARIANT = args["judge-variant"]!.trim();
	const KEEP_TMP = args["keep-tmp"]!;

	if (!EVAL_NAME) {
		console.error("Missing required flag: pass --eval <name>.");
		process.exit(1);
	}

	const applyDefaults = (
		agent: string,
		model: string,
		variant: string,
	): { model: string; variant: string } => {
		if (agent === "codex") {
			const nextModel = model || "5.3";
			const nextVariant = variant || "high";
			return { model: nextModel, variant: nextVariant };
		}
		if (agent === "claude") {
			const nextModel = model || "opus";
			return { model: nextModel, variant: variant || "" };
		}
		return { model: model || "", variant: variant || "" };
	};

	({ model: MODEL, variant: VARIANT } = applyDefaults(AGENT, MODEL, VARIANT));
	({ model: JUDGE_MODEL, variant: JUDGE_VARIANT } = applyDefaults(JUDGE, JUDGE_MODEL, JUDGE_VARIANT));

	return {
		AGENT,
		AGENT_SYSTEM,
		EVAL_NAME,
		MODEL,
		VARIANT,
		JUDGE,
		JUDGE_MODEL,
		JUDGE_VARIANT,
		KEEP_TMP,
	};
})();

const SANDBOX_DEBUG = process.env.SKILL_EVAL_SANDBOX_DEBUG === "1";

function buildClaudeToolEnv(): Record<string, string> {
	const env: Record<string, string> = {
		PATH: process.env.PATH ?? "",
		HOME: process.env.HOME ?? "",
		SHELL: process.env.SHELL ?? "/bin/zsh",
	};

	if (process.env.TERM) {
		env.TERM = process.env.TERM;
	}

	return env;
}

function buildSessionInit(
	agent: string,
	cwd: string,
	purpose: "agent" | "judge",
): Record<string, unknown> {
	const sessionInit: Record<string, unknown> = {
		cwd,
		mcpServers: [],
	};

	if (agent === "claude") {
		sessionInit._meta = {
			claudeCode: {
				options: {
					env: buildClaudeToolEnv(),
					...(purpose === "judge"
						? {
							disallowedTools: ["Write", "Edit", "NotebookEdit", "TodoWrite"],
						}
						: {}),
				},
			},
		};
	}

	return sessionInit;
}

function buildSandboxSpawnOptions(): { log: "silent" | "inherit"; env?: Record<string, string> } {
	const env: Record<string, string> = {
		SANDBOX_AGENT_ACP_REQUEST_TIMEOUT_MS: "600000",
	};

	if (!SANDBOX_DEBUG) {
		return { log: "silent", env };
	}

	return {
		log: "inherit",
		env: {
			...env,
			RUST_LOG: "info,sandbox_agent=debug,acp_http_adapter=debug,agent_management=debug",
			SANDBOX_AGENT_LOG_HTTP: "1",
			SANDBOX_AGENT_LOG_HTTP_HEADERS: "1",
		},
	};
}

function createLogSink(
	stream: ReturnType<typeof createWriteStream>,
	mirrorStdout: boolean,
): {
	writeDelta: (delta: string) => void;
	writeLine: (line: string) => void;
} {
	let atLineStart = true;

	return {
		writeDelta: (delta: string) => {
			stream.write(delta);
			if (mirrorStdout) process.stdout.write(delta);
			atLineStart = delta.endsWith("\n");
		},
		writeLine: (line: string) => {
			if (!atLineStart) {
				stream.write("\n");
				if (mirrorStdout) process.stdout.write("\n");
			}
			stream.write(line + "\n");
			if (mirrorStdout) process.stdout.write(line + "\n");
			atLineStart = true;
		},
	};
}

// --- Prompt helpers ---

const { buildProjectAgentSystemPrompt } = (() => {
	const DEFAULT_BASE = "Create a project in {{TMP_DIR}}. Install dependencies. Do not ask questions; proceed autonomously.";
	const CLAUDE_SHELL_BOOTSTRAP = [
		"Claude shell note: the default PATH may be incomplete.",
		'When using shell commands, prefer absolute paths on this host: `/bin/mkdir`, `/bin/ls`, `/usr/bin/find`, `/usr/bin/env`, `/usr/bin/curl`, `/opt/homebrew/opt/node@22/bin/node`, `/opt/homebrew/opt/node@22/bin/npm`, and `/opt/homebrew/opt/node@22/bin/npx`.',
		'If you still need PATH, prefix each shell command with `export PATH="/opt/homebrew/opt/node@22/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:$PATH" &&`.',
	].join(" ");

	function interpolate(template: string, data: Record<string, string>): string {
		let out = template;
		for (const [key, value] of Object.entries(data)) {
			out = out.replaceAll(`{{${key}}}`, value);
		}
		return out;
	}

	function buildProjectAgentSystemPrompt(tmpDir: string): string {
		const frictionLogPath = `${tmpDir}/${FRICTION_LOG_FILENAME}`;

		const base = (AGENT_SYSTEM || "").trim() ? AGENT_SYSTEM : DEFAULT_BASE;
		const baseInterpolated = interpolate(base, {
			TMP_DIR: tmpDir,
			FRICTION_LOG_PATH: frictionLogPath,
		}).trim();

		const friction = `Keep a friction log as you work: append bullet points to ${frictionLogPath} whenever you hit errors, confusing docs/APIs, or need workarounds.`;
		const handoff = "Do not start the dev server yourself, do not run browser-style verification, and do not spend time on exhaustive validation loops. Finish once the project files and any required dependencies are ready. The harness and judge will validate the result after you stop.";
		const scope = "Work only inside the current working directory unless the task explicitly tells you to fetch a reference into a named subdirectory. Create and modify the migrated project only under `{{TMP_DIR}}`. Do not inspect `~/.claude`, `~/.config`, `/Users/nathan`, other repositories, sibling eval projects, or search for more skills. Any skill documentation you need is already included below.";
		const packageManager = "Package manager: this repository uses a pnpm workspace. Use `pnpm`, not `npm`, for installs and scripts so local workspace package resolution works correctly.";
		const dependencyVersions = "Dependency versions: prefer versions already used by nearby local examples or workspace packages. Do not invent older package versions when a current local example exists.";

		const rivetkitLinking = [
			"RivetKit linking: If you create a package.json that depends on RivetKit packages, set versions to '*' (do not pin).",
			"This repository uses package manager resolutions to redirect these to the local workspace builds.",
			"Examples:",
			'  "rivetkit": "*",',
			'  "@rivetkit/react": "*",',
		].join("\n");

		const shellBootstrap = AGENT === "claude" ? CLAUDE_SHELL_BOOTSTRAP : "";

		return ["# System", baseInterpolated, friction, handoff, interpolate(scope, { TMP_DIR: tmpDir }), packageManager, dependencyVersions, rivetkitLinking, shellBootstrap].filter(Boolean).join("\n\n");
	}

	return { buildProjectAgentSystemPrompt };
})();

// --- Sandbox agent helpers ---

const { collectTurnText, getSessionMode } = (() => {
	type AgentLogSink = {
		writeDelta: (delta: string) => void;
		writeLine: (line: string) => void;
	};

	function logItemParts(
		item: {
			content?: Array<{
				type?: string;
				label?: string;
				detail?: string | null;
				action?: string;
				path?: string;
				name?: string;
				call_id?: string;
			}>;
		},
		sink: AgentLogSink,
	) {
		if (!Array.isArray(item.content)) return;
		for (const part of item.content) {
			if (!part?.type) continue;

			if (part.type === "status") {
				const label = part.label ?? "status";
				const detail = part.detail ? `: ${part.detail}` : "";
				sink.writeLine(`[status] ${label}${detail}`);
			} else if (part.type === "file_ref") {
				const action = part.action ?? "file";
				const path = part.path ?? "";
				sink.writeLine(`[file] ${action} ${path}`);
			} else if (part.type === "tool_call") {
				const name = part.name ?? "tool";
				sink.writeLine(`[tool] ${name}`);
			} else if (part.type === "tool_result") {
				const callId = part.call_id ?? "tool_result";
				sink.writeLine(`[tool_result] ${callId}`);
			}
		}
	}

	function formatToolUpdateText(update: {
		content?: Array<{ type?: string; content?: { type?: string; text?: string } }>;
		rawOutput?: string;
		status?: string;
		title?: string;
	}): string[] {
		const lines: string[] = [];
		if (Array.isArray(update.content)) {
			for (const item of update.content) {
				const text = item?.content?.text?.trim();
				if (text) lines.push(text);
			}
		}
		if (update.status === "failed" && update.rawOutput?.trim()) {
			lines.push(update.rawOutput.trim());
		}
		return lines;
	}

	function getSessionMode(agent: string): string | undefined {
		if (agent === "claude") return "bypassPermissions";
		if (agent === "codex") return "full-access";
		return undefined;
	}

	async function collectTurnText(
		session: {
			prompt: (prompt: Array<{ type: "text"; text: string }>) => Promise<unknown>;
			onEvent: (listener: (event: any) => void) => (() => void) | void;
		},
		message: string,
		sink?: AgentLogSink,
		timeoutMs = 600_000,
	): Promise<string> {
		let deltaText = "";
		let unsubscribe: (() => void) | void;

		unsubscribe = session.onEvent((event: any) => {
			if (event?.sender !== "agent") return;

			const payload = event.payload;
			if (payload?.method !== "session/update") return;

			const update = payload.params?.update;
			if (!update) return;

			if (update.sessionUpdate === "agent_message_chunk") {
				const chunk = update.content?.text ?? "";
				if (chunk) {
					deltaText += chunk;
					sink?.writeDelta(chunk);
				}
				return;
			}

			if (update.sessionUpdate === "tool_call") {
				const title = update.title ?? update._meta?.claudeCode?.toolName ?? "tool";
				sink?.writeLine(`[tool] ${title}`);
				return;
			}

			if (update.sessionUpdate === "tool_call_update") {
				for (const line of formatToolUpdateText(update)) {
					sink?.writeLine(`[tool] ${line}`);
				}
			}
		});

		try {
			await Promise.race([
				session.prompt([{ type: "text", text: message }]),
				new Promise((_, reject) =>
					setTimeout(() => reject(new Error(`Prompt timed out after ${timeoutMs}ms`)), timeoutMs),
				),
			]);
		} finally {
			try {
				unsubscribe?.();
			} catch {
				// Best effort
			}
		}

		return deltaText;
	}

	return { collectTurnText, getSessionMode };
})();

// --- Assert skills are built ---

const { assertSkillsBuilt, assertRivetkitBuilt, cleanupOldEvalProjects, loadSkillContent, discoverSkillIds } = (() => {
	async function assertSkillsBuilt(): Promise<void> {
		try {
			await access(SKILLS_DIR);
		} catch {
			console.error(`Skills not found at ${SKILLS_DIR}`);
			console.error(`Run: pnpm build -F rivet-website`);
			process.exit(1);
		}
	}

	async function assertRivetkitBuilt(): Promise<void> {
		const modJs = join(REPO_ROOT, "rivetkit-typescript/packages/rivetkit/dist/tsup/mod.js");
		try {
			await access(modJs);
		} catch {
			console.error(`RivetKit build output not found: ${modJs}`);
			console.error("Build it before running evals:");
			console.error("  pnpm -C rivetkit-typescript/packages/rivetkit build");
			console.error("  pnpm -F rivetkit build");
			process.exit(1);
		}
	}

	async function cleanupOldEvalProjects(): Promise<void> {
		await mkdir(EVAL_PROJECTS_DIR, { recursive: true });
		const entries = await readdir(EVAL_PROJECTS_DIR, { withFileTypes: true });
		for (const entry of entries) {
			if (!entry.isDirectory()) continue;
			if (!entry.name.startsWith("skill-eval-")) continue;
			await rm(join(EVAL_PROJECTS_DIR, entry.name), { recursive: true, force: true });
		}
	}

	async function loadSkillContent(skillId: string): Promise<string> {
		const skillDir = join(SKILLS_DIR, skillId);
		const baseSkillMdPath = join(skillDir, "BASE_SKILL.md");
		const skillMdPath = join(skillDir, "SKILL.md");

		let baseSkillMd = "";
		try {
			baseSkillMd = await readFile(baseSkillMdPath, "utf-8");
		} catch {
			// Some skills do not have a base document.
		}

		let skillMd: string;
		try {
			skillMd = await readFile(skillMdPath, "utf-8");
		} catch {
			throw new Error(`Skill file not found: ${skillMdPath}`);
		}

		const sections = [`# Reference Bundle: ${skillId}`];
		if (baseSkillMd.trim()) {
			sections.push("## Preloaded BASE_SKILL.md", baseSkillMd.trim());
		}
		sections.push("## Preloaded SKILL.md", skillMd.trim());

		return `${sections.join("\n\n")}\n`;
	}

	async function discoverSkillIds(): Promise<string[]> {
		const entries = await readdir(SKILLS_DIR);
		const skillIds: string[] = [];
		for (const entry of entries.sort()) {
			try {
				await access(join(SKILLS_DIR, entry, "SKILL.md"));
				skillIds.push(entry);
			} catch {
				// Not a skill directory
			}
		}
		return skillIds;
	}

	return { assertSkillsBuilt, assertRivetkitBuilt, cleanupOldEvalProjects, loadSkillContent, discoverSkillIds };
})();

function getEvalSkillIds(evalName: string, availableSkillIds: string[]): string[] {
	const byEval: Array<[string, string[]]> = [
		["cf-do-", ["migrate-cloudflare-durable-objects", "rivetkit"]],
		["cf-agents-", ["migrate-cloudflare-agents", "rivetkit"]],
		["cf-workflows-", ["migrate-cloudflare-workflows", "rivetkit"]],
		["cf-d1-", ["migrate-cloudflare-d1", "rivetkit"]],
		["cf-queues-", ["migrate-cloudflare-queues", "rivetkit"]],
		["cf-partykit-", ["migrate-partykit", "rivetkit"]],
	];

	for (const [prefix, skills] of byEval) {
		if (evalName.startsWith(prefix)) {
			return skills.filter((skillId) => availableSkillIds.includes(skillId));
		}
	}

	return [];
}

// --- Resolve eval files ---

const { resolveEvalFiles } = (() => {
	async function resolveEvalFiles(): Promise<{ promptPath: string; judgePath: string }> {
		const caseDir = join(EVALS_DIR, EVAL_NAME);
		const promptPath = join(caseDir, "prompt.md");
		const judgePath = join(caseDir, "judge.md");

		try {
			await access(promptPath);
			await access(judgePath);
		} catch {
			console.error(`Eval not found or missing files: ${caseDir}`);
			console.error("Expected files:");
			console.error(`  ${promptPath}`);
			console.error(`  ${judgePath}`);
			process.exit(1);
		}

		return { promptPath, judgePath };
	}

	return { resolveEvalFiles };
})();

// --- Start dev server and wait for it ---

const { startDevServer, waitForServer, killServer } = (() => {
	type Tail = { stdout: string[]; stderr: string[] };

	function pushTail(tail: string[], chunk: string, maxLines: number): void {
		const lines = chunk.replace(/\r\n/g, "\n").split("\n");
		for (const line of lines) {
			if (!line) continue;
			tail.push(line);
			if (tail.length > maxLines) tail.shift();
		}
	}

	function startDevServer(cwd: string, resultDir: string): {
		proc: ChildProcess;
		stdoutPath: string;
		stderrPath: string;
		tail: Tail;
		closeLogs: () => void;
	} {
		const proc = spawn("pnpm", ["run", "dev"], {
			cwd,
			stdio: ["ignore", "pipe", "pipe"],
			detached: true,
		});

		const stdoutPath = join(resultDir, "dev-server.stdout.log");
		const stderrPath = join(resultDir, "dev-server.stderr.log");
		const stdoutStream = createWriteStream(stdoutPath, { flags: "w" });
		const stderrStream = createWriteStream(stderrPath, { flags: "w" });
		const tail: Tail = { stdout: [], stderr: [] };

		proc.stdout?.on("data", (buf) => {
			const chunk = String(buf);
			stdoutStream.write(chunk);
			pushTail(tail.stdout, chunk, 200);
		});
		proc.stderr?.on("data", (buf) => {
			const chunk = String(buf);
			stderrStream.write(chunk);
			pushTail(tail.stderr, chunk, 200);
		});

		const closeLogs = () => {
			try { stdoutStream.end(); } catch {}
			try { stderrStream.end(); } catch {}
		};

		return { proc, stdoutPath, stderrPath, tail, closeLogs };
	}

	function extractServerUrl(tail: Tail): string | null {
		const lines = [...tail.stdout, ...tail.stderr];
		for (const line of lines) {
			const match = line.match(/https?:\/\/(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]):(\d+)/);
			if (!match) continue;
			return `http://127.0.0.1:${match[1]}`;
		}
		return null;
	}

	async function waitForServer(url: string, tail: Tail, timeoutMs = 60_000): Promise<string> {
		const start = Date.now();
		while (Date.now() - start < timeoutMs) {
			const candidates = [url, extractServerUrl(tail)].filter(
				(value, index, arr): value is string => !!value && arr.indexOf(value) === index,
			);
			for (const candidate of candidates) {
				try {
					const resp = await fetch(candidate);
					if (resp.ok || resp.status < 500) return candidate;
				} catch {
					// Not ready yet
				}
			}
			await new Promise((r) => setTimeout(r, 1000));
		}
		throw new Error(`Server at ${url} did not become ready within ${timeoutMs / 1000}s`);
	}

	function killServer(proc: ChildProcess): void {
		if (proc.pid) {
			try {
				// Kill the process group since we used detached
				process.kill(-proc.pid, "SIGTERM");
			} catch {
				try {
					proc.kill("SIGTERM");
				} catch {
					// Already dead
				}
			}
		}
	}

	return { startDevServer, waitForServer, killServer };
})();

async function pathExists(path: string): Promise<boolean> {
	try {
		await access(path);
		return true;
	} catch {
		return false;
	}
}

async function waitForProjectReady(tmpDir: string, timeoutMs = 180_000): Promise<boolean> {
	const start = Date.now();
	while (Date.now() - start < timeoutMs) {
		const hasPackageJson = await pathExists(join(tmpDir, "package.json"));
		const hasNodeModules = await pathExists(join(tmpDir, "node_modules"));
		const hasSrc = await pathExists(join(tmpDir, "src"));
		const hasIndexHtml = await pathExists(join(tmpDir, "index.html"));

		if (hasPackageJson && (hasNodeModules || hasSrc || hasIndexHtml)) {
			return true;
		}

		await new Promise((resolve) => setTimeout(resolve, 1000));
	}

	return false;
}

async function waitForProjectScaffold(tmpDir: string, timeoutMs = 120_000): Promise<boolean> {
	const start = Date.now();
	while (Date.now() - start < timeoutMs) {
		const hasPackageJson = await pathExists(join(tmpDir, "package.json"));
		const hasSrc = await pathExists(join(tmpDir, "src"));
		const hasIndexHtml = await pathExists(join(tmpDir, "index.html"));
		if (hasPackageJson || hasSrc || hasIndexHtml) {
			return true;
		}
		await new Promise((resolve) => setTimeout(resolve, 1000));
	}
	return false;
}

async function readTextIfExists(path: string): Promise<string> {
	try {
		return await readFile(path, "utf-8");
	} catch {
		return "";
	}
}

async function destroySessionBestEffort(
	client: SandboxAgent,
	sessionId: string,
	timeoutMs = 5000,
): Promise<void> {
	try {
		await Promise.race([
			client.destroySession(sessionId),
			new Promise((resolve) => setTimeout(resolve, timeoutMs)),
		]);
	} catch {
		// Best effort.
	}
}

function tailText(text: string, maxChars = 4000): string {
	if (text.length <= maxChars) return text;
	return text.slice(text.length - maxChars);
}

// --- Judge verdict schema ---

const { VerdictSchema, parseVerdict } = (() => {
	const VerdictSchema = z.object({
		criteria: z.array(z.object({
			name: z.string(),
			pass: z.boolean(),
			reason: z.string(),
		})),
		observations: z.array(z.object({
			summary: z.string(),
			severity: z.enum(["low", "medium", "high"]),
		})),
		friction: z.array(z.object({
			summary: z.string(),
			fix: z.string(),
		})),
		pass: z.boolean(),
		summary: z.string(),
	});
	const SimpleVerdictSchema = z.object({
		verdict: z.enum(["pass", "fail"]),
		reason: z.string(),
	});

	function extractJson(text: string): unknown | null {
		const fenced = text.match(/```(?:json)?\s*\n([\s\S]*?)\n```/);
		const jsonStr = fenced ? fenced[1] : text;

		try {
			return JSON.parse(jsonStr.trim());
		} catch {
			const match = jsonStr.match(/\{[\s\S]*\}/);
			if (match) {
				try {
					return JSON.parse(match[0]);
				} catch {
					return null;
				}
			}
			return null;
		}
	}

	function parseVerdict(raw: string): {
		verdict: z.infer<typeof VerdictSchema> | null;
		errors: string[];
	} {
		const json = extractJson(raw);
		if (json === null) {
			return { verdict: null, errors: ["Could not extract JSON from judge response"] };
		}

		const result = VerdictSchema.safeParse(json);
		if (!result.success) {
			const simple = SimpleVerdictSchema.safeParse(json);
			if (simple.success) {
				const pass = simple.data.verdict === "pass";
				return {
					verdict: {
						criteria: [
							{
								name: "overall",
								pass,
								reason: simple.data.reason,
							},
						],
						observations: [],
						friction: [],
						pass,
						summary: simple.data.reason,
					},
					errors: [],
				};
			}

			const errors = result.error.issues.map(
				(issue) => `${issue.path.join(".")}: ${issue.message}`,
			);
			return { verdict: null, errors };
		}

		return { verdict: result.data, errors: [] };
	}

	return { VerdictSchema, parseVerdict };
})();

type Verdict = z.infer<typeof VerdictSchema>;

// --- Main ---

type Result = "PASS" | "FAIL" | "ERROR";

async function main() {
	await assertSkillsBuilt();
	await assertRivetkitBuilt();

	const judgeSystem = await readFile(JUDGE_SYSTEM_PATH, "utf-8");
	const { promptPath, judgePath } = await resolveEvalFiles();

	const availableSkillIds = await discoverSkillIds();
	const skillIds = getEvalSkillIds(EVAL_NAME, availableSkillIds);

	const allSkillContent: string[] = [];
	for (const skillId of skillIds) {
		allSkillContent.push(await loadSkillContent(skillId));
	}
	const skillsBlock = allSkillContent.join("\n\n---\n\n");

	// Start sandbox-agent daemon
	const fmtModel = (model: string, variant: string) =>
		variant ? `${model} (${variant})` : model;

	console.log(`${fmt.bold("eval:")} ${EVAL_NAME}`);
	console.log(`${fmt.bold("agent:")} ${AGENT} (${fmtModel(MODEL, VARIANT)})`);
	console.log(`${fmt.bold("judge:")} ${JUDGE} (${fmtModel(JUDGE_MODEL, JUDGE_VARIANT)})`);
	console.log("");
	console.log("Starting sandbox-agent daemon...");
	const client = await SandboxAgent.start({ spawn: buildSandboxSpawnOptions() });
	console.log("Daemon ready.\n");

	try {
		const prompt = await readFile(promptPath, "utf-8");
		const judgeTemplate = await readFile(judgePath, "utf-8");

		const resultDir = join(RESULTS_DIR, EVAL_NAME);
		await mkdir(resultDir, { recursive: true });
		await cleanupOldEvalProjects();

		const start = Date.now();

		// Create temp project directory inside a gitignored pnpm workspace path.
		await mkdir(EVAL_PROJECTS_DIR, { recursive: true });
		const tmpDir = join(EVAL_PROJECTS_DIR, `skill-eval-${EVAL_NAME}-${Date.now()}`);
		await mkdir(tmpDir, { recursive: true });
		console.log(`${fmt.bold("tmp:")} ${tmpDir}`);

		// 1) Build agent message from skills + prompt + system instructions
		const systemPrompt = buildProjectAgentSystemPrompt(tmpDir);

		const agentMessage = [
			systemPrompt,
			"",
			"# Preloaded Migration References\n",
			"These reference files are already loaded inline below. Use them directly. Do not search the filesystem for `SKILL.md`, `BASE_SKILL.md`, or any other skill files.",
			"",
			skillsBlock,
			"\n---\n",
			"# Task\n",
			prompt,
		].join("\n");

		// 2) Run agent to generate the project
		const agentSessionId = `eval-agent-${EVAL_NAME}-${Date.now()}`;
		let response: string;
		let agentStoppedEarly = false;
		let agentSession:
			| Awaited<ReturnType<SandboxAgent["createSession"]>>
			| null = null;
		console.log(`${fmt.bold("== agent ==")}`);
		try {
			agentSession = await client.createSession({
				id: agentSessionId,
				agent: AGENT,
				model: MODEL,
				...(getSessionMode(AGENT) ? { mode: getSessionMode(AGENT) } : {}),
				sessionInit: buildSessionInit(AGENT, tmpDir, "agent") as any,
			});

			const agentLogPath = join(resultDir, "agent.log");
				const agentLogStream = createWriteStream(agentLogPath, { flags: "w" });
				const sink = createLogSink(agentLogStream, true);

				const promptPromise = collectTurnText(agentSession, agentMessage, sink);
				const readyPromise = waitForProjectReady(tmpDir);
				const scaffoldPromise = waitForProjectScaffold(tmpDir);
				const winner = await Promise.race([
					promptPromise.then((value) => ({ kind: "prompt" as const, value })),
					readyPromise.then((ready) => ({ kind: "ready" as const, ready })),
					scaffoldPromise.then((hasScaffold) => ({ kind: "scaffold" as const, hasScaffold })),
				]);

				if (winner.kind === "prompt") {
					response = winner.value;
				} else if (winner.kind === "ready" && winner.ready) {
					response = "Project appears ready; stopping agent and proceeding to judge.";
					agentStoppedEarly = true;
					await destroySessionBestEffort(client, agentSessionId);
					await new Promise((resolve) => setTimeout(resolve, 500));
				} else if (winner.kind === "scaffold" && !winner.hasScaffold) {
					throw new Error("Agent made no project scaffold progress after 120s");
				} else {
					response = await promptPromise;
				}
				try { agentLogStream.end(); } catch {}
			} catch (err) {
			const duration = Math.round((Date.now() - start) / 1000);
			console.log(`\n${fmt.red("ERROR")} (agent, ${duration}s)  ${(err as Error).message.slice(0, 200)}`);
				await destroySessionBestEffort(client, agentSessionId);
				console.log(`${fmt.bold("results:")} ${RESULTS_DIR}`);
				process.exit(1);
			}

			if (!agentStoppedEarly) {
				await destroySessionBestEffort(client, agentSessionId);
			}
		await writeFile(join(resultDir, "response.md"), response);

		// Read friction log if it exists
		let frictionLog = "";
		try {
			frictionLog = await readFile(join(tmpDir, FRICTION_LOG_FILENAME), "utf-8");
			await writeFile(join(resultDir, "friction.md"), frictionLog);
		} catch {
			// No friction log created, that's fine
		}

		const expectedDevUrl = EVAL_NAME.startsWith("cf-")
			? "http://127.0.0.1:3000"
			: "http://127.0.0.1:5173";
		const devUrl = expectedDevUrl;

		// 3) Run the judge, retrying on invalid JSON
		const baseJudgeInput = judgeTemplate.replace(/\{\{URL\}\}/g, devUrl);
		const judgeInput = [
			`Expected local URL: ${expectedDevUrl}`,
			"The app is not pre-started by the harness. Start it yourself from the current working directory if needed, then verify it or diagnose the startup failure.",
			"",
			"Agent final response:",
			"```text",
			tailText(response, 3000),
			"```",
			"",
			"Agent log tail:",
			"```text",
			tailText(await readTextIfExists(join(resultDir, "agent.log"))),
			"```",
			"",
			"Friction log:",
			"```text",
			tailText(frictionLog, 2000),
			"```",
			"",
			"Original eval instructions:",
			baseJudgeInput,
		].join("\n");
		const MAX_JUDGE_ATTEMPTS = 3;
		const judgeShellBootstrap = JUDGE === "claude"
			? 'Claude shell note: prefix every shell command with `export PATH="/opt/homebrew/opt/node@22/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:$PATH" &&` because the default PATH may be incomplete.'
			: "";

		let verdict: Verdict | null = null;
		let verdictRaw = "";
		let judgeError: Error | null = null;
		let lastValidationErrors: string[] | null = null;

		console.log(`\n${fmt.bold("== judge ==")}`);
		for (let attempt = 1; attempt <= MAX_JUDGE_ATTEMPTS; attempt++) {
			const retryInput = attempt === 1
				? [judgeSystem, judgeShellBootstrap, judgeInput].filter(Boolean).join("\n\n")
				: [
					judgeSystem,
					judgeShellBootstrap,
					judgeInput,
					`IMPORTANT: Your previous response had invalid JSON. Validation errors:\n${lastValidationErrors!.map((e) => `- ${e}`).join("\n")}\n\nYou MUST respond with valid JSON matching the schema exactly. Every field is required.`,
				].filter(Boolean).join("\n\n");

			const judgeSessionId = `eval-judge-${EVAL_NAME}-${attempt}-${Date.now()}`;
			try {
				const judgeSession = await client.createSession({
					id: judgeSessionId,
					agent: JUDGE,
					model: JUDGE_MODEL,
					...(getSessionMode(JUDGE) ? { mode: getSessionMode(JUDGE) } : {}),
					sessionInit: buildSessionInit(JUDGE, tmpDir, "judge") as any,
				});

				const judgeLogPath = join(resultDir, "judge.log");
				const judgeLogStream = createWriteStream(judgeLogPath, {
					flags: attempt === 1 ? "w" : "a",
				});
				const judgeSink = createLogSink(judgeLogStream, false);
				judgeSink.writeLine(`[attempt] ${attempt}`);
				verdictRaw = await collectTurnText(judgeSession, retryInput, judgeSink);
				try { judgeLogStream.end(); } catch {}
				} catch (err) {
					judgeError = err as Error;
					await destroySessionBestEffort(client, judgeSessionId);
					break;
				}

				await destroySessionBestEffort(client, judgeSessionId);

			const parsed = parseVerdict(verdictRaw);
			if (parsed.verdict) {
				verdict = parsed.verdict;
				break;
			}

			lastValidationErrors = parsed.errors;
			if (attempt < MAX_JUDGE_ATTEMPTS) {
				console.log(`judge attempt ${attempt} invalid JSON, retrying`);
			}
		}

		if (judgeError) {
			const duration = Math.round((Date.now() - start) / 1000);
			console.log(`${fmt.red("ERROR")} (judge, ${duration}s)  ${judgeError.message.slice(0, 200)}`);
			console.log(`${fmt.bold("results:")} ${RESULTS_DIR}`);
			process.exit(1);
		}

		await writeFile(join(resultDir, "verdict.json"), verdict ? JSON.stringify(verdict, null, 2) : verdictRaw);

		// 6) Record result
		const duration = Math.round((Date.now() - start) / 1000);

		const meta = {
			agent: AGENT,
			model: MODEL,
			variant: VARIANT || undefined,
			judge: JUDGE,
			judge_model: JUDGE_MODEL,
			judge_variant: JUDGE_VARIANT || undefined,
			eval: EVAL_NAME,
			skills: skillIds,
			duration_s: duration,
			tmp_dir: tmpDir,
			dev_url: devUrl,
			has_friction_log: !!frictionLog,
			timestamp: new Date().toISOString(),
		};
		await writeFile(join(resultDir, "meta.json"), JSON.stringify(meta, null, 2));

		let result: Result = "ERROR";
		let summary = "";
		if (verdict && verdict.pass === true) {
			result = "PASS";
		} else if (verdict) {
			result = "FAIL";
			summary = verdict.summary;
		} else {
			result = "ERROR";
			summary = "Invalid judge verdict JSON";
		}

		if (result === "PASS") {
			console.log(`\n${fmt.green("PASS")} (${duration}s)`);
			if (!KEEP_TMP) {
				try {
					await rm(tmpDir, { recursive: true, force: true });
				} catch {
					// Non-fatal
				}
			}
			console.log(`${fmt.bold("results:")} ${RESULTS_DIR}`);
			process.exit(0);
		}

		console.log(`\n${result === "FAIL" ? fmt.red("FAIL") : fmt.red("ERROR")} (${duration}s)${summary ? `  ${summary}` : ""}`);
		console.log(`Project kept at: ${tmpDir}`);
		console.log(`${fmt.bold("results:")} ${RESULTS_DIR}`);
		process.exit(1);
	} finally {
		await client.dispose();
	}
}

(async () => {
	await main();
})().catch((err) => {
	console.error(err);
	process.exit(1);
});
