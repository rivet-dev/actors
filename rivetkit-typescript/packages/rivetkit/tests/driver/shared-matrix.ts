import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, describe } from "vitest";
import {
	type DriverRegistryVariant,
	getDriverRegistryVariants,
} from "../driver-registry-variants";
import {
	createNativeDriverTestConfig,
	createWasmDriverTestConfig,
	releaseSharedEngine,
} from "./shared-harness";
import type {
	DriverRuntime,
	DriverSqliteBackend,
	DriverTestConfig,
} from "./shared-types";
import type { SharedTestEngineOptions } from "../shared-engine";

const describeDriverSuite =
	process.env.RIVETKIT_DRIVER_TEST_PARALLEL === "1"
		? describe
		: describe.sequential;
const TEST_DIR = join(dirname(fileURLToPath(import.meta.url)), "..");

export interface DriverMatrixOptions {
	registryVariants?: DriverRegistryVariant["name"][];
	encodings?: Array<NonNullable<DriverTestConfig["encoding"]>>;
	runtimes?: DriverRuntime[];
	sqliteBackends?: DriverSqliteBackend[];
	config?: Pick<DriverTestConfig, "features" | "skip"> & {
		engine?: SharedTestEngineOptions;
	};
}

export const SQLITE_DRIVER_MATRIX_OPTIONS = {
	runtimes: ["native", "wasm"],
	sqliteBackends: ["local", "remote"],
} as const satisfies Pick<DriverMatrixOptions, "runtimes" | "sqliteBackends">;

export interface DriverMatrixCell {
	runtime: DriverRuntime;
	sqliteBackend: DriverSqliteBackend;
	encoding: NonNullable<DriverTestConfig["encoding"]>;
	skipReason?: string;
}

export function getDriverMatrixCells(
	options: DriverMatrixOptions = {},
): DriverMatrixCell[] {
	const encodings = applyDriverMatrixEnv(
		"RIVETKIT_DRIVER_TEST_ENCODING",
		options.encodings ?? ["bare", "cbor", "json"],
		["bare", "cbor", "json"],
	);
	const runtimes = applyDriverMatrixEnv(
		"RIVETKIT_DRIVER_TEST_RUNTIME",
		options.runtimes ?? ["native", "wasm"],
		["native", "wasm"],
	);
	const sqliteBackends = applyDriverMatrixEnv(
		"RIVETKIT_DRIVER_TEST_SQLITE",
		options.sqliteBackends ?? ["local", "remote"],
		["local", "remote"],
	);
	const cells: DriverMatrixCell[] = [];

	for (const runtime of runtimes) {
		for (const sqliteBackend of sqliteBackends) {
			if (runtime === "wasm" && sqliteBackend === "local") {
				continue;
			}

			for (const encoding of encodings) {
				cells.push({
					runtime,
					sqliteBackend,
					encoding,
				});
			}
		}
	}

	return cells;
}

function applyDriverMatrixEnv<const T extends string>(
	key: string,
	base: readonly T[],
	allowed: readonly T[],
): T[] {
	const value = process.env[key];
	if (!value) {
		return [...base];
	}

	const allowedSet = new Set<string>(allowed);
	const parsed = value
		.split(",")
		.map((part) => part.trim())
		.filter((part) => part.length > 0);
	for (const entry of parsed) {
		if (!allowedSet.has(entry)) {
			throw new Error(
				`invalid ${key} value ${JSON.stringify(entry)}. Expected one of: ${allowed.join(", ")}`,
			);
		}
	}

	const requested = new Set(parsed);
	return base.filter((entry) => requested.has(entry));
}

export function describeDriverMatrix(
	suiteName: string,
	defineTests: (driverTestConfig: DriverTestConfig) => void,
	options: DriverMatrixOptions = {},
) {
	const registryVariantNames = new Set(options.registryVariants);
	const variants = getDriverRegistryVariants(TEST_DIR).filter(
		(variant) =>
			registryVariantNames.size === 0 ||
			registryVariantNames.has(variant.name),
	);
	const cells = getDriverMatrixCells(options);

	describeDriverSuite(suiteName, () => {
		for (const variant of variants) {
			if (variant.skip) {
				describe.skip(`${variant.name} registry`, () => {});
				continue;
			}

			describeDriverSuite(`${variant.name} registry`, () => {
				afterAll(async () => {
					await releaseSharedEngine();
				});

				for (const cell of cells) {
					const suite = `runtime (${cell.runtime}) / sqlite (${cell.sqliteBackend}) / encoding (${cell.encoding})`;

					if (cell.skipReason) {
						describe.skip(`${suite}: ${cell.skipReason}`, () => {});
						continue;
					}

					describeDriverSuite(suite, () => {
						if (cell.runtime === "native") {
							defineTests(
								createNativeDriverTestConfig({
									variant,
									encoding: cell.encoding,
									sqliteBackend: cell.sqliteBackend,
									...options.config,
								}),
							);
						} else {
							defineTests(
								createWasmDriverTestConfig({
									variant,
									encoding: cell.encoding,
									...options.config,
								}),
							);
						}
					});
				}
			});
		}
	});
}
