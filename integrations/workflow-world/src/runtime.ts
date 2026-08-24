// Harness entry: runs the World registry as a standalone worker against an
// externally managed Engine. Not part of the package exports.
import { registry } from "./actors.js";

const endpoint = process.env.RIVET_ENDPOINT;
const namespace = process.env.RIVET_NAMESPACE;
const poolName = process.env.RIVET_POOL;

if (!endpoint || !namespace || !poolName) {
	throw new Error(
		"RIVET_ENDPOINT, RIVET_NAMESPACE, and RIVET_POOL are required",
	);
}

registry.config.test = {
	...registry.config.test,
	enabled: true,
	sqliteBackend: "local",
};
registry.config.runtime = "native";
registry.config.startEngine = false;
registry.config.endpoint = endpoint;
registry.config.token = process.env.RIVET_TOKEN;
registry.config.namespace = namespace;
registry.config.envoy = {
	...registry.config.envoy,
	poolName,
};
registry.config.noWelcome = true;

await registry.startAndWait();
