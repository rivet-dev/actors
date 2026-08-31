import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import type { Registry } from "../../src/registry";
import { buildConfiguredRegistry } from "../../src/registry/native";

const registryPath = process.env.RIVETKIT_DRIVER_REGISTRY_PATH;
const endpoint = process.env.RIVETKIT_TEST_ENDPOINT;
const token = process.env.RIVET_TOKEN ?? "dev";
const namespace = process.env.RIVET_NAMESPACE ?? "default";
const poolName = process.env.RIVETKIT_TEST_POOL_NAME ?? "default";
const sqliteBackend = process.env.RIVETKIT_TEST_SQLITE_BACKEND ?? "remote";
const wasmPath =
	process.env.RIVETKIT_WASM_PATH ??
	resolve(
		dirname(fileURLToPath(import.meta.url)),
		"../../../rivetkit-wasm/pkg/rivetkit_wasm_bg.wasm",
	);

if (!registryPath) {
	throw new Error("RIVETKIT_DRIVER_REGISTRY_PATH is required");
}

if (!endpoint) {
	throw new Error("RIVETKIT_TEST_ENDPOINT is required");
}

if (sqliteBackend !== "remote") {
	throw new Error(
		`unsupported RIVETKIT_TEST_SQLITE_BACKEND for wasm runtime: ${sqliteBackend}`,
	);
}

const { registry } = (await import(
	pathToFileURL(resolve(registryPath)).href
)) as {
	registry: Registry<any>;
};

registry.config.test = {
	...registry.config.test,
	enabled: true,
	sqliteBackend,
};
registry.config.runtime = "wasm";
registry.config.wasm = {
	...registry.config.wasm,
	initInput: readFileSync(wasmPath),
};
registry.config.startEngine = false;
registry.config.endpoint = endpoint;
registry.config.token = token;
registry.config.namespace = namespace;
registry.config.envoy = {
	...registry.config.envoy,
	poolName,
};

const {
	runtime,
	registry: wasmRegistry,
	serveConfig,
} = await buildConfiguredRegistry(registry.parseConfig());

// buildConfiguredRegistry best-effort resolves the optional native Engine
// package. This fixture connects Wasm to the Engine owned by the parent test,
// so do not pass that native-only path across the Wasm runtime boundary.
delete serveConfig.engineBinaryPath;

await runtime.serveRegistry(wasmRegistry, serveConfig);
