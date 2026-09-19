import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
	bumpPackageJsons,
	parseWorkspaceCatalogs,
	resolveCatalogDependency,
} from "./version.js";

test("resolves default and named pnpm catalog dependencies", () => {
	const catalogs = parseWorkspaceCatalogs(`
catalog:
  drizzle-orm: "0.45.2"
catalogs:
  react19:
    react: "^19.0.0"
`);

	assert.equal(
		resolveCatalogDependency("drizzle-orm", "catalog:", catalogs),
		"0.45.2",
	);
	assert.equal(
		resolveCatalogDependency("drizzle-orm", "catalog:default", catalogs),
		"0.45.2",
	);
	assert.equal(
		resolveCatalogDependency("react", "catalog:react19", catalogs),
		"^19.0.0",
	);
	assert.equal(resolveCatalogDependency("zod", "^4.0.0", catalogs), undefined);
});

test("supports a named default catalog and rejects duplicate defaults", () => {
	const catalogs = parseWorkspaceCatalogs(`
catalogs:
  default:
    react: "^19.0.0"
`);
	for (const spec of ["catalog:", "catalog:default"]) {
		assert.equal(resolveCatalogDependency("react", spec, catalogs), "^19.0.0");
	}
	assert.throws(
		() => parseWorkspaceCatalogs("catalog: {}\ncatalogs:\n  default: {}\n"),
		/default catalog is defined twice/,
	);
});

test("rejects unresolved catalog references before publication", () => {
	const catalogs = parseWorkspaceCatalogs("catalog:\n  drizzle-orm: 0.45.2\n");

	assert.throws(
		() => resolveCatalogDependency("missing", "catalog:", catalogs),
		/missing from pnpm catalog default/,
	);
	assert.throws(
		() => resolveCatalogDependency("react", "catalog:react19", catalogs),
		/missing pnpm catalog react19/,
	);
});

test("rewrites catalog specs in the manifest passed to npm", async () => {
	const root = await mkdtemp(join(tmpdir(), "rivet-publish-catalog-"));
	const packageDirectory = join(root, "packages", "rivetkit");
	try {
		await mkdir(packageDirectory, { recursive: true });
		await writeFile(
			join(root, "pnpm-workspace.yaml"),
			'catalog:\n  drizzle-orm: "0.45.2"\npackages:\n  - packages/*\n',
		);
		await writeFile(
			join(packageDirectory, "package.json"),
			JSON.stringify({
				dependencies: { "drizzle-orm": "catalog:" },
				name: "rivetkit",
				version: "1.0.0",
			}),
		);

		await bumpPackageJsons(root, "0.0.0-preview.abcdef0", {
			repository: "rivet-dev/rivet",
		});
		const manifest: unknown = JSON.parse(
			await readFile(join(packageDirectory, "package.json"), "utf8"),
		);
		if (typeof manifest !== "object" || manifest === null) {
			throw new Error("rewritten package manifest is not an object");
		}
		const dependencies: unknown = Reflect.get(manifest, "dependencies");
		if (typeof dependencies !== "object" || dependencies === null) {
			throw new Error("rewritten package dependencies are not an object");
		}
		assert.equal(
			Reflect.get(dependencies, "drizzle-orm"),
			"0.45.2",
		);
	} finally {
		await rm(root, { force: true, recursive: true });
	}
});
