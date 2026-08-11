import { getCollection } from "astro:content";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import path from "node:path";

import { getDocsPath, normalizeSlug, SITE_BASE_URL } from "./shared";
import skillBaseRivetkit from "./skill-base-rivetkit.md?raw";
import skillBaseRivetkitCookbook from "./skill-base-rivetkit-cookbook.md?raw";
import skillBaseClientJavascript from "./skill-base-rivetkit-client-javascript.md?raw";
import skillBaseClientReact from "./skill-base-rivetkit-client-react.md?raw";
import skillBaseClientRust from "./skill-base-rivetkit-client-rust.md?raw";
import skillBaseClientSwift from "./skill-base-rivetkit-client-swift.md?raw";
import skillBaseClientSwiftUI from "./skill-base-rivetkit-client-swiftui.md?raw";
import skillAppendCloudflareMigration from "./skill-append-cloudflare-migration.md?raw";

export type BaseSkillId =
	| "rivetkit"
	| "rivetkit-client-javascript"
	| "rivetkit-client-react"
	| "rivetkit-client-rust"
	| "rivetkit-client-swift"
	| "rivetkit-client-swiftui";

export type SkillId = string;

type SkillContentSource = {
	collection: "docs" | "cookbook";
	docId: string;
	fallbackDocIds?: string[];
	startMarker?: string;
	endMarker?: string;
};

type SkillConfig = {
	id: SkillId;
	name: string;
	directory: string;
	description: string;
	baseTemplate: string;
	content: SkillContentSource;
	includeReferences: boolean;
	includeOpenApi: boolean;
	baseSkillId?: SkillId;
};

const BASE_SKILL_CONFIGS = {
	rivetkit: {
		id: "rivetkit",
		name: "rivetkit",
		directory: "rivetkit",
		description:
			"RivetKit backend and Rivet Actor runtime guidance. Use for building, modifying, debugging, or testing Rivet Actors, registries, serverless/runner modes, deployment, or actor-based workflows.",
		baseTemplate: skillBaseRivetkit,
		content: {
			collection: "docs",
			docId: "actors/crash-course",
			fallbackDocIds: ["actors"],
			startMarker: "{/* SKILL_OVERVIEW_START */}",
			endMarker: "{/* SKILL_OVERVIEW_END */}",
		},
		includeReferences: true,
		includeOpenApi: true,
	},
	"rivetkit-client-javascript": {
		id: "rivetkit-client-javascript",
		name: "rivetkit-client-javascript",
		directory: "rivetkit-client-javascript",
		description:
			"RivetKit JavaScript client guidance. Use for browser, Node.js, or Bun clients that connect to Rivet Actors with rivetkit/client, create clients, call actions, or manage connections.",
		baseTemplate: skillBaseClientJavascript,
		content: {
			collection: "docs",
			docId: "clients/javascript",
		},
		includeReferences: false,
		includeOpenApi: false,
	},
	"rivetkit-client-react": {
		id: "rivetkit-client-react",
		name: "rivetkit-client-react",
		directory: "rivetkit-client-react",
		description:
			"RivetKit React client guidance. Use for React apps that connect to Rivet Actors with @rivetkit/react, create hooks with createRivetKit, or manage realtime state with useActor.",
		baseTemplate: skillBaseClientReact,
		content: {
			collection: "docs",
			docId: "clients/react",
		},
		includeReferences: false,
		includeOpenApi: false,
	},
	"rivetkit-client-rust": {
		id: "rivetkit-client-rust",
		name: "rivetkit-client-rust",
		directory: "rivetkit-client-rust",
		description:
			"RivetKit Rust client guidance. Use for Rust clients and backends that connect to Rivet Actors with rivetkit::client, create typed actor handles, call actions, or manage connections.",
		baseTemplate: skillBaseClientRust,
		content: {
			collection: "docs",
			docId: "clients/rust",
		},
		includeReferences: false,
		includeOpenApi: false,
	},
	"rivetkit-client-swift": {
		id: "rivetkit-client-swift",
		name: "rivetkit-client-swift",
		directory: "rivetkit-client-swift",
		description:
			"RivetKit Swift client guidance. Use for Swift clients that connect to Rivet Actors with RivetKitClient, create actor handles, call actions, or manage connections.",
		baseTemplate: skillBaseClientSwift,
		content: {
			collection: "docs",
			docId: "clients/swift",
		},
		includeReferences: false,
		includeOpenApi: false,
	},
	"rivetkit-client-swiftui": {
		id: "rivetkit-client-swiftui",
		name: "rivetkit-client-swiftui",
		directory: "rivetkit-client-swiftui",
		description:
			"RivetKit SwiftUI client guidance. Use for SwiftUI apps that connect to Rivet Actors with RivetKitSwiftUI, @Actor, rivetKit view modifiers, and SwiftUI bindings.",
		baseTemplate: skillBaseClientSwiftUI,
		content: {
			collection: "docs",
			docId: "clients/swiftui",
		},
		includeReferences: false,
		includeOpenApi: false,
	},
} as const satisfies Record<BaseSkillId, SkillConfig>;

for (const config of Object.values(BASE_SKILL_CONFIGS)) {
	if (config.description.length > 500) {
		throw new Error(
			`SKILL_DESCRIPTION must be <= 500 chars for ${config.id}, got ${config.description.length}`,
		);
	}
}

export type SkillReference = {
	slug: string;
	fileId: string;
	title: string;
	description: string;
	docPath: string;
	canonicalUrl: string;
	sourcePath: string | null;
	tags: string[];
	markdown: string;
};

const cachedReferences = new Map<SkillId, SkillReference[]>();
const cachedSkillFiles = new Map<SkillId, string>();
let cachedDocs: Awaited<ReturnType<typeof getCollection>> | null = null;
let cachedCookbook: Awaited<ReturnType<typeof getCollection>> | null = null;
let cachedCookbookSkillConfigs: Map<string, SkillConfig> | null = null;

async function getDocs() {
	if (!cachedDocs) {
		cachedDocs = await getCollection("docs");
	}
	return cachedDocs;
}

async function getCookbook() {
	if (!cachedCookbook) {
		cachedCookbook = await getCollection("cookbook");
	}
	return cachedCookbook;
}

function cookbookSkillIdFromEntryId(entryId: string) {
	const slug = normalizeSlug(entryId);
	const flattened = slug.replaceAll("/", "-");
	if (!flattened) {
		throw new Error(`cookbook entry id resolved to empty slug: ${entryId}`);
	}
	return flattened;
}

async function getCookbookSkillConfigs(): Promise<Map<string, SkillConfig>> {
	if (cachedCookbookSkillConfigs) return cachedCookbookSkillConfigs;

	const entries = await getCookbook();
	const map = new Map<string, SkillConfig>();

	for (const entry of entries) {
		const id = cookbookSkillIdFromEntryId(entry.id);
		const description = entry.data.description;
		if (description.length > 500) {
			throw new Error(
				`SKILL_DESCRIPTION must be <= 500 chars for ${id} (from cookbook/${entry.id}), got ${description.length}`,
			);
		}

		const isCfMigration = id.startsWith("migrate-cloudflare-") || id.startsWith("migrate-partykit");
		const template = isCfMigration
			? skillBaseRivetkitCookbook.replace(
				"<!-- CONTENT -->",
				skillAppendCloudflareMigration + "\n\n<!-- CONTENT -->",
			)
			: skillBaseRivetkitCookbook;

		map.set(id, {
			id,
			name: id,
			directory: id,
			description,
			baseTemplate: template,
			content: {
				collection: "cookbook",
				docId: entry.id,
			},
			includeReferences: true,
			includeOpenApi: true,
			baseSkillId: "rivetkit",
		});
	}

	cachedCookbookSkillConfigs = map;
	return map;
}

export async function listSkillIds(): Promise<SkillId[]> {
	const base = Object.keys(BASE_SKILL_CONFIGS) as BaseSkillId[];
	const cookbook = [...(await getCookbookSkillConfigs()).keys()];
	return [...base, ...cookbook];
}

export async function getSkillConfig(skillId: string): Promise<SkillConfig> {
	const base = BASE_SKILL_CONFIGS[skillId as BaseSkillId];
	if (base) return base;

	const cookbook = await getCookbookSkillConfigs();
	const config = cookbook.get(skillId);
	if (config) return config;

	throw new Error(`Unknown skill id: ${skillId}`);
}

async function getRivetkitVersion(): Promise<string> {
	const versionFilePath = path.join(process.cwd(), "src/generated/skill-version.json");

	if (!existsSync(versionFilePath)) {
		throw new Error(
			`skill-version.json not found at ${versionFilePath}. ` +
			`Ensure the skillVersion integration runs before skill generation.`
		);
	}

	const content = await readFile(versionFilePath, "utf-8");
	const data = JSON.parse(content);
	return data.rivetkit;
}

export async function listSkillReferences(skillId: SkillId): Promise<SkillReference[]> {
	if (cachedReferences.has(skillId)) {
		return cachedReferences.get(skillId)!;
	}

	const config = await getSkillConfig(skillId);
	if (!config.includeReferences) {
		cachedReferences.set(skillId, []);
		return [];
	}

	const docs = await getDocs();
	const skillDocs = docs.filter((entry) => entry.data.skill);
	const references: SkillReference[] = skillDocs.map((entry) => buildReference(entry));

	const cookbookEntries = await getCookbook();
	for (const entry of cookbookEntries) {
		references.push(buildCookbookReference(entry));
	}

	references.sort((a, b) => a.title.localeCompare(b.title));
	cachedReferences.set(skillId, references);
	return references;
}

export async function getReferenceByFileId(skillId: SkillId, fileId: string) {
	const references = await listSkillReferences(skillId);
	return references.find((ref) => ref.fileId === fileId);
}

export async function renderSkillFile(skillId: SkillId): Promise<string> {
	if (cachedSkillFiles.has(skillId)) {
		return cachedSkillFiles.get(skillId)!;
	}

	const config = await getSkillConfig(skillId);
	const base = config.baseTemplate;
	const content = await buildSkillContent(config);

	const frontmatter = ["---", `name: "${config.name}"`, `description: "${config.description}"`, "---", ""].join(
		"\n",
	);

	if (!base.includes("<!-- CONTENT -->")) {
		throw new Error(`skill base for ${config.id} does not contain <!-- CONTENT --> marker`);
	}

	const rivetkitVersion = await getRivetkitVersion();

	let fileBody = base.replace("<!-- CONTENT -->", content);

	if (base.includes("<!-- TITLE -->")) {
		const title = await resolveContentTitle(config);
		fileBody = fileBody.replace("<!-- TITLE -->", title);
	}

	if (base.includes("<!-- REFERENCE_INDEX -->")) {
		if (!config.includeReferences) {
			throw new Error(`skill base for ${config.id} includes a reference index but references are disabled`);
		}
		const references = await listSkillReferences(skillId);
		const referenceList = buildReferenceSection(references);
		fileBody = fileBody.replace("<!-- REFERENCE_INDEX -->", referenceList);
	} else if (config.includeReferences) {
		throw new Error(`skill base for ${config.id} must include <!-- REFERENCE_INDEX --> marker`);
	}

	fileBody = fileBody.replace(/\{\{RIVETKIT_VERSION\}\}/g, rivetkitVersion);

	const finalFile = `${frontmatter}\n${fileBody}\n`;
	cachedSkillFiles.set(skillId, finalFile);
	return finalFile;
}

export async function listReferenceSummaries(skillId: SkillId) {
	const references = await listSkillReferences(skillId);
	const config = await getSkillConfig(skillId);
	return references.map((ref) => ({
		name: ref.fileId,
		title: ref.title,
		description: ref.description,
		canonical_url: ref.canonicalUrl,
		reference_url: `/metadata/skills/${config.directory}/reference/${ref.fileId}.md`,
	}));
}

export async function skillSupportsOpenApi(skillId: SkillId) {
	const config = await getSkillConfig(skillId);
	return config.includeOpenApi;
}

function buildReference(entry: Awaited<ReturnType<typeof getCollection>>[number]): SkillReference {
	const slug = normalizeSlug(entry.id);
	const docPath = getDocsPath(slug);
	const canonicalUrl = `${SITE_BASE_URL}${docPath}`;
	const fileId = createFileId(slug);
	const body = entry.body ?? "";

	return {
		slug,
		fileId,
		title: entry.data.title,
		description: entry.data.description,
		docPath,
		canonicalUrl,
		sourcePath: entry.filePath ?? null,
		tags: slug.split("/").filter(Boolean),
		markdown: convertDocToReference(body),
	};
}

function buildCookbookReference(entry: Awaited<ReturnType<typeof getCollection>>[number]): SkillReference {
	const rawSlug = normalizeSlug(entry.id);
	const slug = `cookbook/${rawSlug}`;
	const fileId = slug;
	const canonicalUrl = `https://rivet.dev/cookbook/${rawSlug}`;
	const body = entry.body ?? "";

	return {
		slug,
		fileId,
		title: entry.data.title,
		description: entry.data.description,
		docPath: `/cookbook/${rawSlug}`,
		canonicalUrl,
		sourcePath: entry.filePath ?? null,
		tags: slug.split("/").filter(Boolean),
		markdown: convertDocToReference(body),
	};
}

async function resolveContentTitle(config: SkillConfig): Promise<string> {
	const collection = config.content.collection;
	const entries = collection === "docs" ? await getDocs() : await getCookbook();
	const docIds = [config.content.docId, ...(config.content.fallbackDocIds ?? [])];
	const doc = entries.find((entry) =>
		docIds.some((docId) => entry.id === docId || (docId.includes("/") && entry.id.startsWith(`${docId}/`))),
	);
	if (!doc) {
		throw new Error(`Doc ${config.content.docId} not found when resolving title.`);
	}
	return doc.data.title;
}

async function buildSkillContent(config: SkillConfig) {
	const collection = config.content.collection;
	const entries = collection === "docs" ? await getDocs() : await getCookbook();
	const docIds = [config.content.docId, ...(config.content.fallbackDocIds ?? [])];
	const doc = entries.find((entry) =>
		docIds.some((docId) => {
			if (entry.id === docId) return true;
			if (docId.includes("/") && entry.id.startsWith(`${docId}/`)) {
				return true;
			}
			return false;
		}),
	);

	if (!doc) {
		throw new Error(`Doc ${config.content.docId} not found in docs collection.`);
	}
	if (!doc.body) {
		throw new Error(`${doc.id} has no body content`);
	}

	let prefix = "";
	if (collection === "cookbook") {
		const templates = (doc.data as { templates?: string[] } | undefined)?.templates;
		if (Array.isArray(templates) && templates.length > 0) {
			const lines = [
				"## Working Examples",
				"",
				"If you need a reference implementation, read the raw working example code in these templates:",
				"",
				...templates.map((name) => `- [${name}](https://github.com/rivet-dev/rivet/tree/main/examples/${name})`),
				"",
			];
			prefix = lines.join("\n");
		}
	}

	let rawBody = doc.body;
	if (config.content.startMarker && config.content.endMarker) {
		const startIdx = rawBody.indexOf(config.content.startMarker);
		const endIdx = rawBody.indexOf(config.content.endMarker);

		if (startIdx === -1) {
			throw new Error(`${config.content.startMarker} marker not found in ${doc.id}.mdx`);
		}
		if (endIdx === -1) {
			throw new Error(`${config.content.endMarker} marker not found in ${doc.id}.mdx`);
		}

		rawBody = rawBody.slice(startIdx + config.content.startMarker.length, endIdx).trim();
		if (!rawBody) {
			throw new Error(`Content between markers is empty for ${doc.id}.mdx`);
		}
	}

	const rendered = convertDocToReference(rawBody);
	const sep = prefix ? "\n\n" : "";
	return `${prefix}${sep}${rendered}`.trim();
}

function createFileId(slug: string) {
	// Preserve path structure instead of flattening to dashes
	if (!slug) return "index";
	return slug;
}

function convertDocToReference(body: string) {
	const { replaced, restore } = extractCodeBlocks(body ?? "");
	let text = replaced;

	text = text.replace(/^[ \t]*import\s+[^;]+;?\s*$/gm, "");
	text = text.replace(/^[ \t]*export\s+[^;]+;?\s*$/gm, "");
	text = text.replace(/\{\/\*[\s\S]*?\*\/\}/g, "");

	text = stripWrapperTags(text, "Steps");
	text = stripWrapperTags(text, "Tabs");
	text = stripWrapperTags(text, "CardGroup");
	text = stripWrapperTags(text, "CodeGroup");

	text = formatHeadingBlocks(text, "Step", "Step");
	text = formatHeadingBlocks(text, "Tab", "Tab");
	text = formatCards(text);

	text = applyCallouts(text, "Tip");
	text = applyCallouts(text, "Note");
	text = applyCallouts(text, "Warning");
	text = applyCallouts(text, "Info");
	text = applyCallouts(text, "Callout");

	text = text.replace(/<Card[^>]*>/gi, "").replace(/<\/Card>/gi, "");
	text = text.replace(/<Steps[^>]*>/gi, "").replace(/<\/Steps>/gi, "");
	text = text.replace(/<Tabs[^>]*>/gi, "").replace(/<\/Tabs>/gi, "");
	text = text.replace(/<Step[^>]*>/gi, "").replace(/<\/Step>/gi, "");
	text = text.replace(/<Tab[^>]*>/gi, "").replace(/<\/Tab>/gi, "");

	text = text.replace(/<[A-Z][A-Za-z0-9]*[^>]*>/g, "").replace(/<\/[A-Z][A-Za-z0-9]*>/g, "");
	text = text.replace(/\n{3,}/g, "\n\n");

	return restore(text).trim();
}

function extractCodeBlocks(input: string) {
	const blocks: string[] = [];
	const replaced = input.replace(/```[\s\S]*?```/g, (match) => {
		const token = `@@CODE_BLOCK_${blocks.length}@@`;
		blocks.push(match);
		return token;
	});

	return {
		replaced,
		restore: (value: string) => value.replace(/@@CODE_BLOCK_(\d+)@@/g, (_, index) => blocks[Number(index)] ?? ""),
	};
}

function stripWrapperTags(input: string, tag: string) {
	const open = new RegExp(`<${tag}[^>]*>`, "gi");
	const close = new RegExp(`</${tag}>`, "gi");
	return input.replace(open, "\n").replace(close, "\n");
}

function formatHeadingBlocks(input: string, tag: string, fallback: string) {
	const withTitles = input.replace(
		new RegExp(`<${tag}[^>]*title=(?:"([^"]+)"|'([^']+)')[^>]*>`, "gi"),
		(_, doubleQuoted, singleQuoted) => `\n### ${(doubleQuoted ?? singleQuoted ?? fallback).trim()}\n\n`,
	);
	const withFallback = withTitles.replace(new RegExp(`<${tag}[^>]*>`, "gi"), `\n### ${fallback}\n\n`);
	return withFallback.replace(new RegExp(`</${tag}>`, "gi"), "\n");
}

function formatCards(input: string) {
	return input.replace(/<Card([^>]*)>([\s\S]*?)<\/Card>/gi, (_, attrs, content) => {
		const title = getAttributeValue(attrs, "title") ?? "Resource";
		const href = getAttributeValue(attrs, "href");
		const summary = collapseWhitespace(stripHtml(content));
		const link = href ? `[${title}](${href})` : title;
		const suffix = summary ? ` — ${summary}` : "";
		return `\n- ${link}${suffix}\n\n`;
	});
}

function applyCallouts(input: string, tag: string) {
	const regex = new RegExp(`<${tag}[^>]*>([\s\S]*?)</${tag}>`, "gi");
	return input.replace(regex, (_, content) => {
		const label = tag.toUpperCase();
		const text = collapseWhitespace(stripHtml(content));
		return `\n> **${label}:** ${text}\n\n`;
	});
}

function getAttributeValue(attrs: string, name: string) {
	const regex = new RegExp(`${name}=(?:"([^"]+)"|'([^']+)')`, "i");
	const match = attrs.match(regex);
	if (!match) return undefined;
	return (match[1] ?? match[2] ?? "").trim();
}

function stripHtml(value: string) {
	return value.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim();
}

function collapseWhitespace(value: string) {
	return value.replace(/\s+/g, " ").trim();
}

function buildReferenceSection(references: SkillReference[]) {
	// Group by top-level segment only (actors, clients, general)
	const groups = new Map<string, SkillReference[]>();

	for (const ref of references) {
		const top = resolveGroup(ref).top;
		if (!groups.has(top)) {
			groups.set(top, []);
		}
		groups.get(top)!.push(ref);
	}

	const lines: string[] = [];
	const sortedTop = [...groups.keys()].sort((a, b) => a.localeCompare(b));

	for (const topKey of sortedTop) {
		const topTitle = formatSegment(topKey);
		lines.push(`### ${topTitle}`, "");

		const entries = groups.get(topKey)!;
		entries.sort((a, b) => a.title.localeCompare(b.title));
		for (const entry of entries) {
			lines.push(`- [${entry.title}](reference/${entry.fileId}.md)`);
		}
		lines.push("");
	}

	return lines.join("\n").trim();
}

function resolveGroup(ref: SkillReference) {
	const segments = (ref.slug ?? "").split("/").filter(Boolean);
	const top = segments[0] ?? "general";
	const sub = segments[1] ?? "";
	return { top, sub };
}

function formatSegment(value: string) {
	if (!value) return "";
	const normalized = value === "index" ? "overview" : value;
	return normalized
		.split("-")
		.map((part) => part.charAt(0).toUpperCase() + part.slice(1))
		.join(" ");
}
