import type { AgentOsOptions, MountConfig } from "@rivet-dev/agent-os-core";
import { AgentOs, createInMemoryFileSystem } from "@rivet-dev/agent-os-core";
import { type ActorDefinition, actor, event } from "@/actor/mod";
import type { DatabaseProvider, RawAccess } from "@/common/database/config";
import { db } from "@/common/database/mod";
import {
	type AgentOsActorConfig,
	type AgentOsActorConfigInput,
	agentOsActorConfigSchema,
} from "../config";
import type {
	AgentOsActionContext,
	AgentOsActorState,
	AgentOsActorVars,
	CronEventPayload,
	PermissionRequestPayload,
	ProcessExitPayload,
	ProcessOutputPayload,
	SessionEventPayload,
	ShellDataPayload,
	VmBootedPayload,
	VmShutdownPayload,
} from "../types";
import { buildCronActions } from "./cron";
import { migrateAgentOsTables } from "./db";
import { buildFilesystemActions } from "./filesystem";
import { buildNetworkActions } from "./network";
import { buildOnRequestHandler, buildPreviewActions } from "./preview";
import { buildProcessActions } from "./process";
import {
	buildConfigActions,
	buildPromptActions,
	buildSessionActions,
	buildSessionPersistenceActions,
} from "./session";
import { buildShellActions } from "./shell";

// --- VM lifecycle helpers ---

async function ensureVm<TConnParams>(
	c: AgentOsActionContext<TConnParams>,
	config: AgentOsActorConfig<TConnParams>,
): Promise<AgentOs> {
	if (c.vars.agentOs) {
		return c.vars.agentOs;
	}

	const start = Date.now();

	// Build options with in-memory VFS as default working directory mount.
	const options = buildVmOptions(config.options);

	const agentOs = await AgentOs.create(options);
	c.vars.agentOs = agentOs;

	// Wire cron events to actor events.
	agentOs.onCronEvent((cronEvent) => {
		c.broadcast("cronEvent", { event: cronEvent });
	});

	c.broadcast("vmBooted", {});
	c.log.info({
		msg: "agent-os vm booted",
		bootDurationMs: Date.now() - start,
	});

	return agentOs;
}

function buildVmOptions(userOptions?: AgentOsOptions): AgentOsOptions {
	const userMounts = userOptions?.mounts ?? [];

	// Check if the user already provided a mount at /home/user. If so, respect
	// their override and skip the default in-memory VFS mount.
	const hasWorkdirMount = userMounts.some(
		(m: MountConfig) => m.path === "/home/user",
	);

	if (hasWorkdirMount) {
		return userOptions ?? {};
	}

	// TODO: Reimplement with persistent backend (SQLite-backed metadata and
	// actor storage-backed blocks) so VM filesystem state survives sleep/wake.
	const memMount: MountConfig = {
		path: "/home/user",
		driver: createInMemoryFileSystem(),
	};

	return {
		...userOptions,
		mounts: [memMount, ...userMounts],
	};
}

// --- Prevent-sleep coordination ---

function syncPreventSleep<TConnParams>(
	c: AgentOsActionContext<TConnParams>,
): void {
	const shouldPrevent =
		c.vars.activeSessionIds.size > 0 ||
		c.vars.activeProcesses.size > 0 ||
		c.vars.activeHooks.size > 0 ||
		c.vars.activeShells.size > 0;

	c.setPreventSleep(shouldPrevent);

	c.log.info({
		msg: "agent-os prevent sleep sync",
		preventSleep: shouldPrevent,
		activeSessions: c.vars.activeSessionIds.size,
		activeProcesses: c.vars.activeProcesses.size,
		activeHooks: c.vars.activeHooks.size,
		activeShells: c.vars.activeShells.size,
	});
}

// --- Hook tracking ---

function runHook<TConnParams>(
	c: AgentOsActionContext<TConnParams>,
	name: string,
	callback: () => void | Promise<void>,
): void {
	const promise = Promise.resolve(callback())
		.catch((error) =>
			c.log.error({ msg: "agent-os hook failed", hookName: name, error }),
		)
		.finally(() => {
			c.vars.activeHooks.delete(promise);
			syncPreventSleep(c);
		});
	c.vars.activeHooks.add(promise);
	syncPreventSleep(c);
	c.waitUntil(promise);
}

// --- Public API ---

export function agentOs<TConnParams = undefined>(
	config: AgentOsActorConfigInput<TConnParams>,
): ActorDefinition<
	AgentOsActorState,
	TConnParams,
	undefined,
	AgentOsActorVars,
	undefined,
	DatabaseProvider<RawAccess>,
	{
		sessionEvent: typeof sessionEventToken;
		permissionRequest: typeof permissionRequestToken;
		vmBooted: typeof vmBootedToken;
		vmShutdown: typeof vmShutdownToken;
		processOutput: typeof processOutputToken;
		processExit: typeof processExitToken;
		shellData: typeof shellDataToken;
		cronEvent: typeof cronEventToken;
	},
	Record<never, never>,
	any
> {
	const parsedConfig = agentOsActorConfigSchema.parse(
		config,
	) as AgentOsActorConfig<TConnParams>;
	const actions = {
		...buildSessionActions(parsedConfig),
		...buildPromptActions(parsedConfig),
		...buildConfigActions(parsedConfig),
		...buildSessionPersistenceActions(parsedConfig),
		...buildProcessActions(parsedConfig),
		...buildFilesystemActions(parsedConfig),
		...buildPreviewActions(parsedConfig),
		...buildShellActions(parsedConfig),
		...buildCronActions(parsedConfig),
		...buildNetworkActions(parsedConfig),
	};

	return actor<
		AgentOsActorState,
		TConnParams,
		undefined,
		AgentOsActorVars,
		undefined,
		DatabaseProvider<RawAccess>,
		{
			sessionEvent: typeof sessionEventToken;
			permissionRequest: typeof permissionRequestToken;
			vmBooted: typeof vmBootedToken;
			vmShutdown: typeof vmShutdownToken;
			processOutput: typeof processOutputToken;
			processExit: typeof processExitToken;
			shellData: typeof shellDataToken;
			cronEvent: typeof cronEventToken;
		},
		Record<never, never>,
		typeof actions
	>({
		options: {
			sleepGracePeriod: 900_000,
			actionTimeout: 900_000,
			...parsedConfig.actorOptions,
		},
		createState: async () => ({}),
		createVars: () => ({
			agentOs: null,
			activeSessionIds: new Set<string>(),
			activeProcesses: new Set<number>(),
			activeHooks: new Set<Promise<void>>(),
			activeShells: new Set<string>(),
			sessions: new Set(),
		}),
		db: db({
			onMigrate: migrateAgentOsTables,
		}),
		events: {
			sessionEvent: sessionEventToken,
			permissionRequest: permissionRequestToken,
			vmBooted: vmBootedToken,
			vmShutdown: vmShutdownToken,
			processOutput: processOutputToken,
			processExit: processExitToken,
			shellData: shellDataToken,
			cronEvent: cronEventToken,
		},
		onBeforeConnect: parsedConfig.onBeforeConnect
			? async (ctx, params) => {
					// Skip user auth for preview URL requests. The signed token
					// in onRequest is the credential; browsers navigating preview
					// URLs cannot supply actor connection params.
					if (ctx.request) {
						const url = new URL(ctx.request.url);
						if (url.pathname.startsWith("/fetch/")) {
							return;
						}
					}
					await parsedConfig.onBeforeConnect?.(ctx, params);
				}
			: undefined,
		onRequest: buildOnRequestHandler(parsedConfig),
		onSleep: async (c) => {
			c.log.info({
				msg: "agent-os vm shutdown for sleep",
				activeSessions: c.vars.sessions.size,
				activeProcesses: c.vars.activeProcesses.size,
				activeShells: c.vars.activeShells.size,
			});

			if (c.vars.agentOs) {
				await c.vars.agentOs.dispose();
				c.vars.agentOs = null;
			}

			c.broadcast("vmShutdown", { reason: "sleep" as const });
		},
		onDestroy: async (c) => {
			c.log.info({
				msg: "agent-os vm shutdown for destroy",
				activeSessions: c.vars.sessions.size,
				activeProcesses: c.vars.activeProcesses.size,
				activeShells: c.vars.activeShells.size,
			});

			if (c.vars.agentOs) {
				await c.vars.agentOs.dispose();
				c.vars.agentOs = null;
			}

			c.broadcast("vmShutdown", { reason: "destroy" as const });
		},
		actions,
	});
}

// Event type tokens. Declared at module level so they can be referenced in
// the actor generic type parameters.
const sessionEventToken = event<SessionEventPayload>();
const permissionRequestToken = event<PermissionRequestPayload>();
const vmBootedToken = event<VmBootedPayload>();
const vmShutdownToken = event<VmShutdownPayload>();
const processOutputToken = event<ProcessOutputPayload>();
const processExitToken = event<ProcessExitPayload>();
const shellDataToken = event<ShellDataPayload>();
const cronEventToken = event<CronEventPayload>();

export { ensureVm, syncPreventSleep, runHook };
