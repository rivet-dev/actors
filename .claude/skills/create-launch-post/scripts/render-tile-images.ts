import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

// Logo-tile launch hero. The alternative to the painting hero in
// `render-launch-images.ts`: the launch title over two to four product logos,
// each in a rounded "app tile" with its own rotation and the middle tile
// dominant. The tile treatment comes from the agentOS launch graphics, which in
// turn copies the landing page's floating-agent tiles.
const BLOG_WIDTH = 2048;
const BLOG_HEIGHT = 1024;
const SOCIAL_WIDTH = 2048;
const SOCIAL_HEIGHT = 1238;
const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const SKILL_DIR = path.resolve(SCRIPT_DIR, "..");
const REPO_ROOT = path.resolve(SKILL_DIR, "../../..");
const WEBSITE_DIR = path.join(REPO_ROOT, "website");

// Site tokens (website/tailwind.config.mjs).
const INK = "#1B1916";
const PAPER = "#EFEFEF";

interface Options {
	title: string;
	logos: string[];
	outputDir: string;
}

function parseArgs(argv: string[]): Options {
	const args = argv[0] === "--" ? argv.slice(1) : argv;
	const values = new Map<string, string>();
	for (let index = 0; index < args.length; index += 2) {
		const key = args[index];
		const value = args[index + 1];
		if (!key?.startsWith("--") || value === undefined) {
			throw new Error(`Invalid argument near ${key ?? "end of command"}`);
		}
		values.set(key.slice(2), value);
	}

	const title = values.get("title");
	const logos = values.get("logos");
	const outputDir = values.get("output-dir");
	if (!title || !logos || !outputDir) {
		throw new Error(
			"Usage: pnpm --dir website render-tile-images -- --title <text> --logos <svg,svg,svg> --output-dir <path>",
		);
	}

	const logoPaths = logos
		.split(",")
		.map((entry) => entry.trim())
		.filter(Boolean)
		.map((entry) => path.resolve(entry));
	if (logoPaths.length < 2 || logoPaths.length > 4) {
		throw new Error("--logos must list two to four SVG paths");
	}

	return { title, logos: logoPaths, outputDir: path.resolve(outputDir) };
}

function escapeHtml(value: string): string {
	return value
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;");
}

function dataUrl(mime: string, bytes: Buffer): string {
	return `data:${mime};base64,${bytes.toString("base64")}`;
}

function stripDoctype(svg: string): string {
	return svg.replace(/<\?xml[^>]*\?>/i, "").replace(/<!DOCTYPE[^>]*>/i, "");
}

// Prefix ids and their references so inlining several SVGs cannot collide on a
// shared id such as `_clip1`.
function namespaceIds(svg: string, prefix: string): string {
	return svg
		.replace(/id="([^"]+)"/g, `id="${prefix}$1"`)
		.replace(/url\(#([^)]+)\)/g, `url(#${prefix}$1)`)
		.replace(/(xlink:href|href)="#([^"]+)"/g, `$1="#${prefix}$2"`);
}

interface TilePlacement {
	left: number;
	top: number;
	size: number;
	rot: number;
	inner: number;
}

// Middle-dominant arrangements. Percentages are relative to the scatter box.
const TILE_LAYOUTS: Record<number, TilePlacement[]> = {
	2: [
		{ left: 34, top: 48, size: 300, rot: -6, inner: 0.6 },
		{ left: 66, top: 52, size: 300, rot: 5, inner: 0.6 },
	],
	3: [
		{ left: 23, top: 52, size: 268, rot: -6, inner: 0.58 },
		{ left: 50, top: 38, size: 330, rot: 5, inner: 0.6 },
		{ left: 77, top: 53, size: 268, rot: -5, inner: 0.62 },
	],
	4: [
		{ left: 18, top: 54, size: 248, rot: -6, inner: 0.58 },
		{ left: 39, top: 40, size: 292, rot: 4, inner: 0.6 },
		{ left: 61, top: 40, size: 292, rot: -4, inner: 0.6 },
		{ left: 82, top: 54, size: 248, rot: 6, inner: 0.58 },
	],
};

async function buildHtml(
	options: Options,
	height: number,
): Promise<string> {
	const [titleFont, rivetLogoRaw, ...logoSources] = await Promise.all([
		readFile(
			path.join(
				WEBSITE_DIR,
				"public/fonts/perfectly-nineties/PerfectlyNineties-Semibold.otf",
			),
		),
		readFile(
			path.join(WEBSITE_DIR, "src/images/rivet-logos/icon-text-black.svg"),
			"utf8",
		),
		...options.logos.map((logoPath) => readFile(logoPath, "utf8")),
	]);

	const rivetLogo = namespaceIds(stripDoctype(rivetLogoRaw), "rv-");
	// Marks authored white for dark UIs are recolored to the site ink so they
	// read on the light tile.
	const art = logoSources.map((source, index) =>
		namespaceIds(
			stripDoctype(source)
				.replace(/white/g, INK)
				.replace(/#fff(fff)?\b/gi, INK),
			`lg${index}-`,
		),
	);

	const placements = TILE_LAYOUTS[options.logos.length];
	const tiles = placements
		.map((tile, index) => {
			const inner = Math.round(tile.size * tile.inner);
			const radius = Math.round(tile.size * 0.2);
			return `<div class="tile" style="left:${tile.left}%;top:${tile.top}%;width:${tile.size}px;height:${tile.size}px;
			border-radius:${radius}px;transform:translate(-50%,-50%) rotate(${tile.rot}deg)">
			<div class="tlogo" style="height:${inner}px">${art[index]}</div></div>`;
		})
		.join("");

	// The composition is authored against the 2048x1024 blog crop and shifted
	// down by half the surplus so the taller social canvas stays centered.
	const offset = Math.round((height - BLOG_HEIGHT) / 2);

	return `<!doctype html>
<html lang="en">
	<head>
		<meta charset="utf-8" />
		<style>
			@font-face {
				font-family: "Perfectly Nineties";
				src: url("${dataUrl("font/otf", titleFont)}") format("opentype");
				font-style: normal;
				font-weight: 600;
			}

			* { box-sizing: border-box; }
			html, body { margin: 0; background: ${PAPER}; }
			.card {
				position: relative;
				width: ${SOCIAL_WIDTH}px;
				height: ${height}px;
				background: ${PAPER};
				overflow: hidden;
			}
			.logo {
				position: absolute;
				top: ${150 + offset}px;
				left: 50%;
				transform: translateX(-50%);
				height: 56px;
				display: flex;
				align-items: center;
			}
			.logo svg { display: block; height: 100%; width: auto; }
			h1 {
				position: absolute;
				top: ${236 + offset}px;
				left: 50%;
				transform: translateX(-50%);
				width: 1720px;
				margin: 0;
				text-align: center;
				color: ${INK};
				font-family: "Perfectly Nineties", serif;
				font-style: normal;
				font-weight: 600;
				font-size: 112px;
				line-height: 1.1;
				letter-spacing: -0.01em;
			}
			.scatter {
				position: absolute;
				top: ${512 + offset}px;
				left: 50%;
				transform: translateX(-50%);
				width: 1340px;
				height: 400px;
			}
			.tile {
				position: absolute;
				display: flex;
				align-items: center;
				justify-content: center;
				background: linear-gradient(to bottom, #ffffff, #f1f1f3);
				border: 1px solid rgba(27, 25, 22, 0.10);
				transform-origin: center;
				box-shadow: 0 4px 12px -2px rgba(20, 20, 22, 0.10),
					0 34px 70px -26px rgba(20, 20, 22, 0.26);
			}
			.tlogo { display: flex; align-items: center; justify-content: center; }
			.tlogo svg { display: block; height: 100%; width: auto; }
		</style>
	</head>
	<body>
		<main class="card">
			<div class="logo" aria-label="Rivet">${rivetLogo}</div>
			<h1>${escapeHtml(options.title)}</h1>
			<div class="scatter">${tiles}</div>
		</main>
	</body>
</html>`;
}

async function main(): Promise<void> {
	const options = parseArgs(process.argv.slice(2));
	await mkdir(options.outputDir, { recursive: true });

	const browser = await chromium.launch();
	try {
		const targets = [
			{ name: "image", width: BLOG_WIDTH, height: BLOG_HEIGHT },
			{ name: "social", width: SOCIAL_WIDTH, height: SOCIAL_HEIGHT },
		];
		for (const target of targets) {
			const html = await buildHtml(options, target.height);
			await writeFile(path.join(options.outputDir, `${target.name}.html`), html);
			const page = await browser.newPage({
				viewport: { width: target.width, height: target.height },
				deviceScaleFactor: 1,
			});
			await page.setContent(html, { waitUntil: "load" });
			await page.evaluate(() => document.fonts.ready);
			await page.screenshot({
				path: path.join(options.outputDir, `${target.name}.png`),
				fullPage: false,
			});
			await page.close();
			console.log(`wrote ${path.join(options.outputDir, `${target.name}.png`)}`);
		}
	} finally {
		await browser.close();
	}
}

main().catch((error) => {
	console.error(error);
	process.exitCode = 1;
});
