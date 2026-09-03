// This file consolidates all environment variables that affect RivetKit's behavior.
//
// IMPORTANT: When adding or modifying environment variables here, also update the
// documentation at: website/src/content/docs/general/environment-variables.mdx

import { getEnvUniversal } from "@/utils";

// Rivet configuration
export const getRivetEngine = (): string | undefined =>
	getEnvUniversal("RIVET_ENGINE");
export const getRivetEndpoint = (): string | undefined =>
	getEnvUniversal("RIVET_ENDPOINT");
export const getRivetToken = (): string | undefined =>
	getEnvUniversal("RIVET_TOKEN");
export const getRivetNamespace = (): string | undefined =>
	getEnvUniversal("RIVET_NAMESPACE");
export const getRivetPool = (): string | undefined =>
	getEnvUniversal("RIVET_POOL");
export const getRivetTotalSlots = (): number | undefined => {
	const value = getEnvUniversal("RIVET_TOTAL_SLOTS");
	return value !== undefined ? parseInt(value, 10) : undefined;
};
export const getRivetRunEngine = (): boolean =>
	getEnvUniversal("RIVET_RUN_ENGINE") === "1";
export const getRivetRunEngineHost = (): string | undefined =>
	getEnvUniversal("RIVET_RUN_ENGINE_HOST");
export const getRivetRunEnginePort = (): number | undefined => {
	const value = getEnvUniversal("RIVET_RUN_ENGINE_PORT");
	return value !== undefined ? parseInt(value, 10) : undefined;
};
export const getRivetRunEngineVersion = (): string | undefined =>
	getEnvUniversal("RIVET_RUN_ENGINE_VERSION");
export const getRivetRunServices = (): boolean | undefined => {
	const value = getEnvUniversal("RIVET_RUN_SERVICES");
	return value === undefined ? undefined : value === "1";
};
export const getRivetEnvoyKind = (): string | undefined =>
	getEnvUniversal("RIVET_ENVOY_KIND");
export const getRivetEnvoyVersion = (): number | undefined => {
	const value = getEnvUniversal("RIVET_ENVOY_VERSION");
	return value !== undefined ? parseInt(value, 10) : undefined;
};
export const getRivetPublicEndpoint = (): string | undefined =>
	getEnvUniversal("RIVET_PUBLIC_ENDPOINT");
export const getRivetPublicToken = (): string | undefined =>
	getEnvUniversal("RIVET_PUBLIC_TOKEN");
// There is no RIVET_PUBLIC_NAMESPACE because the frontend and backend cannot
// use different namespaces

// RivetKit configuration
export const getRivetkitInspectorDisable = (): boolean =>
	getEnvUniversal("RIVET_INSPECTOR_DISABLE") === "1";
export const getRivetkitStoragePath = (): string | undefined =>
	getEnvUniversal("RIVETKIT_STORAGE_PATH");
export const getRivetkitRuntime = (): string | undefined =>
	getEnvUniversal("RIVETKIT_RUNTIME");
export type RuntimeMode = "envoy" | "serverless";

export const getRivetkitRuntimeMode = (): RuntimeMode => {
	const value = getEnvUniversal("RIVETKIT_RUNTIME_MODE");
	if (value === undefined) return "envoy";
	if (value === "envoy" || value === "serverless") return value;
	throw new Error(
		`RIVETKIT_RUNTIME_MODE env var must be "envoy" or "serverless"; got "${value}"`,
	);
};

export const getRivetkitPublicDir = (): string | undefined => {
	const value = getEnvUniversal("RIVETKIT_PUBLIC_DIR");
	return value === undefined || value === "" ? undefined : value;
};

export function parsePortEnv(raw: string | undefined): number | undefined {
	if (raw === undefined || raw === "") return undefined;
	const parsed = Number.parseInt(raw, 10);
	if (
		!Number.isFinite(parsed) ||
		parsed < 1 ||
		parsed > 65535 ||
		String(parsed) !== raw.trim()
	) {
		throw new Error(
			`RIVET_PORT env var must be an integer between 1 and 65535; got "${raw}"`,
		);
	}
	return parsed;
}

// Logging configuration
// DEPRECATED: LOG_LEVEL will be removed in a future version
export const getLogLevel = (): string | undefined =>
	getEnvUniversal("RIVET_LOG_LEVEL") ?? getEnvUniversal("LOG_LEVEL");
export const getLogTarget = (): boolean =>
	getEnvUniversal("RIVET_LOG_TARGET") === "1";
export const getLogTimestamp = (): boolean =>
	getEnvUniversal("RIVET_LOG_TIMESTAMP") === "1";
export const getLogMessage = (): boolean =>
	getEnvUniversal("RIVET_LOG_MESSAGE") === "1";
export const getLogErrorStack = (): boolean =>
	getEnvUniversal("RIVET_LOG_ERROR_STACK") === "1";
export const getLogHeaders = (): boolean =>
	getEnvUniversal("RIVET_LOG_HEADERS") === "1";

// Environment configuration
export const getNodeEnv = (): string | undefined => getEnvUniversal("NODE_ENV");
export const getNextPhase = (): string | undefined =>
	getEnvUniversal("NEXT_PHASE");
export const isDev = (): boolean => getNodeEnv() !== "production";

// Experimental
/**
 * Enables experimental OTel tracing for Rivet Actors.
 *
 * When disabled, actors use an in-memory no-op traces implementation.
 */
export const getRivetExperimentalOtel = (): boolean =>
	getEnvUniversal("RIVET_EXPERIMENTAL_OTEL") === "1";
