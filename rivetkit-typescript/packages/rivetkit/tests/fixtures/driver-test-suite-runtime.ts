import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type { Registry } from "../../src/registry";
import { buildConfiguredRegistry } from "../../src/registry/native";

const registryPath = process.env.RIVETKIT_DRIVER_REGISTRY_PATH;
const endpoint = process.env.RIVETKIT_TEST_ENDPOINT;
const token = process.env.RIVET_TOKEN ?? "dev";
const namespace = process.env.RIVET_NAMESPACE ?? "default";
const poolName = process.env.RIVETKIT_TEST_POOL_NAME ?? "default";
const sqliteBackend = process.env.RIVETKIT_TEST_SQLITE_BACKEND ?? "local";

if (!registryPath) {
	throw new Error("RIVETKIT_DRIVER_REGISTRY_PATH is required");
}

if (!endpoint) {
	throw new Error("RIVETKIT_TEST_ENDPOINT is required");
}

const { registry } = (await import(
	pathToFileURL(resolve(registryPath)).href
)) as {
	registry: Registry<any>;
};

if (sqliteBackend !== "local" && sqliteBackend !== "remote") {
	throw new Error(
		`unsupported RIVETKIT_TEST_SQLITE_BACKEND: ${sqliteBackend}`,
	);
}

registry.config.test = {
	...registry.config.test,
	enabled: true,
	sqliteBackend,
};
registry.config.runtime = "native";
registry.config.startEngine = false;
registry.config.endpoint = endpoint;
registry.config.token = token;
registry.config.namespace = namespace;
registry.config.envoy = {
	...registry.config.envoy,
	poolName,
};

if (process.env.RIVETKIT_TEST_ACTORS_PER_THREAD) {
	// Exercise the public bootstrap path in both the host and actor workers.
	registry.config.actorsPerThread = Number(
		process.env.RIVETKIT_TEST_ACTORS_PER_THREAD,
	);
	await registry.startAndWait();
} else {
	const { registry: nativeRegistry, serveConfig } =
		await buildConfiguredRegistry(registry.parseConfig());
	await nativeRegistry.serve(serveConfig);
}
