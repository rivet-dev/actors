import { RivetError } from "@rivetkit/engine-api-full";
import type { ReadRangeOptions, ReadRangeWire } from "@rivetkit/traces";
import { decodeReadRangeWire } from "@rivetkit/traces/encoding";
import {
	mutationOptions,
	type QueryClient,
	queryOptions,
	useQuery,
	useQueryClient,
} from "@tanstack/react-query";
import * as cbor from "cbor-x";
import { createContext, useContext, useMemo, useRef } from "react";
import type ReconnectingWebSocket from "reconnectingwebsocket";
import {
	type Connection,
	decodeWorkflowHistoryTransport,
	CURRENT_VERSION as INSPECTOR_PROTOCOL_CURRENT_VERSION,
	type QueueStatus,
	type Schedule,
	type ScheduleFire,
	type ToServer,
	TO_CLIENT_VERSIONED as toClient,
	TO_SERVER_VERSIONED as toServer,
} from "rivetkit/inspector/client";
import { toast } from "sonner";
import { match } from "ts-pattern";
import z from "zod";
import { type ConnectionStatus, useWebSocket } from "../hooks/use-websocket";
import type { ActorId } from "./queries";
import { transformWorkflowHistory } from "./workflow/transform-workflow-history";
import type { WorkflowHistory } from "./workflow/workflow-types";

export const actorInspectorQueriesKeys = {
	actorState: (actorId: ActorId) => ["actor", actorId, "state"] as const,
	actorIsStateEnabled: (actorId: ActorId) =>
		["actor", actorId, "is-state-enabled"] as const,
	actorConnections: (actorId: ActorId) =>
		["actor", actorId, "connections"] as const,
	actorDatabase: (actorId: ActorId) =>
		["actor", actorId, "database"] as const,
	actorRpcs: (actorId: ActorId) => ["actor", actorId, "rpcs"] as const,
	actorTraces: (actorId: ActorId) => ["actor", actorId, "traces"] as const,
	actorQueueStatus: (actorId: ActorId, limit: number) =>
		["actor", actorId, "queue", limit] as const,
	actorQueueSize: (actorId: ActorId) =>
		["actor", actorId, "queue", "size"] as const,
	actorWakeUp: (actorId: ActorId) => ["actor", actorId, "wake-up"] as const,
	actorWorkflowHistory: (actorId: ActorId) =>
		["actor", actorId, "workflow-history"] as const,
	actorIsWorkflowEnabled: (actorId: ActorId) =>
		["actor", actorId, "is-workflow-enabled"] as const,
	actorTabConfig: (actorId: ActorId) =>
		["actor", actorId, "tab-config"] as const,
	actorInspectorInitialized: (actorId: ActorId) =>
		["actor", actorId, "inspector-initialized"] as const,
	actorSchedules: (actorId: ActorId) =>
		["actor", actorId, "schedules"] as const,
	actorScheduleHistory: (actorId: ActorId, scheduleId: string) =>
		["actor", actorId, "schedules", scheduleId, "history"] as const,
};

type QueueStatusSummary = {
	size: number;
	maxSize: number;
	truncated: boolean;
	messages: Array<{
		id: string;
		name: string;
		createdAtMs: number;
	}>;
};

export type DatabaseColumn = {
	cid: number;
	name: string;
	type: string;
	notnull: boolean;
	dflt_value: string | null;
	pk: boolean | null;
};

export type DatabaseForeignKey = {
	id: number;
	table: string;
	from: string;
	to: string;
};

export type DatabaseTableInfo = {
	table: { schema: string; name: string; type: string };
	columns: DatabaseColumn[];
	foreignKeys: DatabaseForeignKey[];
	records: number;
};

export type DatabaseSchema = {
	tables: DatabaseTableInfo[];
};

export type DatabaseExecuteResult = {
	rows: unknown[];
};

export type DatabaseExecuteRequest = {
	sql: string;
	args?: unknown[];
	properties?: Record<string, unknown>;
};

export type InspectorTabConfigEntry = {
	id: string;
	label?: string;
	icon?: string | null;
	hidden?: boolean;
};

export type InspectorSchedule = {
	id: string;
	name?: string;
	kind: "at" | "cron" | "every";
	action: string;
	args: unknown[];
	nextRunAt: number;
	lastRunAt?: number;
	expression?: string;
	timezone?: string;
	intervalMs?: number;
	maxHistory?: number;
};

export type InspectorScheduleFire = {
	action: string;
	scheduledAt: number;
	firedAt: number;
	finishedAt?: number;
	result: "running" | "ok" | "error" | "skipped";
	error?: {
		group: string;
		code: string;
		message: string;
		metadata?: unknown;
	};
};

interface ActorInspectorApi {
	ping: () => Promise<void>;
	executeAction: (name: string, args: unknown[]) => Promise<unknown>;
	patchState: (state: unknown) => Promise<void>;
	getConnections: () => Promise<Connection[]>;
	getState: () => Promise<{ isEnabled: boolean; state: unknown }>;
	getRpcs: () => Promise<string[]>;
	getTraces: (options: ReadRangeOptions) => Promise<ReadRangeWire>;
	getQueueStatus: (limit: number) => Promise<QueueStatusSummary>;
	getWorkflowHistory: () => Promise<{
		history: WorkflowHistory | null;
		isEnabled: boolean;
		error: string | null;
	}>;
	replayWorkflowFromStep: (entryId?: string) => Promise<{
		isEnabled: boolean;
	}>;
	getDatabaseSchema: () => Promise<DatabaseSchema>;
	getDatabaseTableRows: (
		table: string,
		limit: number,
		offset: number,
	) => Promise<unknown[]>;
	executeDatabaseSql: (
		request: DatabaseExecuteRequest,
	) => Promise<DatabaseExecuteResult>;
	getMetadata: () => Promise<{ version: string }>;
	getSchedules: () => Promise<InspectorSchedule[]>;
	getScheduleHistory: (
		scheduleId: string,
		limit: number,
	) => Promise<InspectorScheduleFire[]>;
	deleteSchedule: (
		scheduleId: string,
		kind: InspectorSchedule["kind"],
	) => Promise<boolean>;
}

type FeatureSupport = {
	supported: boolean;
	minVersion: string;
	currentVersion?: string;
	message: string;
};

type WorkflowHistoryHttpResponse = {
	// `history` is the decoded workflow JSON documented for the inspector HTTP
	// routes. It is deliberately not read here: the tab refreshes history over
	// the websocket after a replay, and the BARE transform expects transport
	// bytes rather than this shape.
	history: unknown;
	isWorkflowEnabled: boolean;
};

const MIN_RIVETKIT_VERSION_TRACES = "2.0.40";
const MIN_RIVETKIT_VERSION_QUEUE = "2.0.40";
const MIN_RIVETKIT_VERSION_DATABASE = "2.0.42";
const MIN_RIVETKIT_VERSION_WORKFLOW_REPLAY = "2.1.6";
// Inspector protocol v5 delivers the tab config inside the WS `Init` message
// (there is no `/inspector/tab-config` HTTP route anymore). Only negotiate v5
// with runtimes new enough to send it.
const MIN_RIVETKIT_VERSION_TABCONFIG_INIT = "2.3.3";
const MIN_RIVETKIT_VERSION_SCHEDULES = "2.3.4";
// Before 2.3.4, inspector WebSockets did not require a protocol version on the
// URL and instead embedded one in every message. Only use connection-level
// negotiation with runtimes that implement it; older runtimes need the legacy
// URL and embedded framing.
const MIN_RIVETKIT_VERSION_INSPECTOR_NEGOTIATION = "2.3.4";
const INSPECTOR_ERROR_EVENTS_DROPPED = "inspector.events_dropped";

function parseSemver(version?: string) {
	if (!version) {
		return null;
	}
	const match = version.match(/^(\d+)\.(\d+)\.(\d+)/);
	if (!match) {
		return null;
	}
	return {
		major: Number(match[1]),
		minor: Number(match[2]),
		patch: Number(match[3]),
	};
}

function compareSemver(
	a: { major: number; minor: number; patch: number },
	b: { major: number; minor: number; patch: number },
) {
	if (a.major !== b.major) {
		return a.major - b.major;
	}
	if (a.minor !== b.minor) {
		return a.minor - b.minor;
	}
	return a.patch - b.patch;
}

export function isVersionAtLeast(
	version: string | undefined,
	minVersion: string,
) {
	const parsed = parseSemver(version);
	const minParsed = parseSemver(minVersion);
	if (!parsed || !minParsed) {
		return false;
	}
	// `0.0.0` is the dev/preview placeholder (e.g. `0.0.0-<branch>.<sha>` from
	// pkg.pr.new or `0.0.0-main.<sha>` snapshots). These are built from the
	// latest source, so treat them as newer than any release gate. Released
	// prereleases like `2.3.0-rc.1` keep comparing by major.minor.patch
	// (`parseSemver` already drops the prerelease suffix), so they pass too.
	if (parsed.major === 0 && parsed.minor === 0 && parsed.patch === 0) {
		return true;
	}
	return compareSemver(parsed, minParsed) >= 0;
}

function buildFeatureSupport(
	currentVersion: string | undefined,
	minVersion: string,
	label: string,
): FeatureSupport {
	const supported = isVersionAtLeast(currentVersion, minVersion);
	if (!currentVersion) {
		return {
			supported: false,
			minVersion,
			currentVersion,
			message: `${label} requires RivetKit ${minVersion}+. Please upgrade.`,
		};
	}
	return {
		supported,
		minVersion,
		currentVersion,
		message: supported
			? ""
			: `${label} requires RivetKit ${minVersion}+ (current ${currentVersion}). Please upgrade.`,
	};
}

export function getInspectorProtocolVersion(version: string | undefined) {
	const parsed = parseSemver(version);
	if (!parsed) {
		return 2;
	}
	if (isVersionAtLeast(version, MIN_RIVETKIT_VERSION_DATABASE)) {
		if (isVersionAtLeast(version, MIN_RIVETKIT_VERSION_SCHEDULES)) {
			return 6;
		}
		if (isVersionAtLeast(version, MIN_RIVETKIT_VERSION_TABCONFIG_INIT)) {
			return 5;
		}
		if (isVersionAtLeast(version, MIN_RIVETKIT_VERSION_WORKFLOW_REPLAY)) {
			return 4;
		}
		return 3;
	}
	if (parsed.major >= 2) {
		return 2;
	}
	return 1;
}

export function usesNegotiatedInspectorProtocol(
	version: string | undefined,
): boolean {
	return isVersionAtLeast(
		version,
		MIN_RIVETKIT_VERSION_INSPECTOR_NEGOTIATION,
	);
}

export function buildInspectorWebSocketUrl(
	baseUrl: string,
	version: number,
	negotiated: boolean,
): string {
	return `${baseUrl}/inspector/connect${
		negotiated ? `?protocol_version=${version}` : ""
	}`;
}

function normalizeSchedules(
	schedules: readonly Schedule[],
): InspectorSchedule[] {
	return schedules.map((schedule) => ({
		id: schedule.id,
		name: schedule.name ?? undefined,
		kind: schedule.kind as InspectorSchedule["kind"],
		action: schedule.action,
		args: cbor.decode(new Uint8Array(schedule.args)) as unknown[],
		nextRunAt: Number(schedule.nextRunAt),
		lastRunAt:
			schedule.lastRunAt == null ? undefined : Number(schedule.lastRunAt),
		expression: schedule.expression ?? undefined,
		timezone: schedule.timezone ?? undefined,
		intervalMs:
			schedule.intervalMs == null
				? undefined
				: Number(schedule.intervalMs),
		maxHistory:
			schedule.maxHistory == null
				? undefined
				: Number(schedule.maxHistory),
	}));
}

function normalizeScheduleHistory(
	history: readonly ScheduleFire[],
): InspectorScheduleFire[] {
	return history.map((fire) => ({
		action: fire.action,
		scheduledAt: Number(fire.scheduledAt),
		firedAt: Number(fire.firedAt),
		finishedAt:
			fire.finishedAt == null ? undefined : Number(fire.finishedAt),
		result: fire.result as InspectorScheduleFire["result"],
		error: fire.error
			? {
					group: fire.error.group,
					code: fire.error.code,
					message: fire.error.message,
					metadata:
						fire.error.metadata == null
							? undefined
							: cbor.decode(new Uint8Array(fire.error.metadata)),
				}
			: undefined,
	}));
}

function normalizeQueueStatus(status: QueueStatus): QueueStatusSummary {
	return {
		size: Number(status.size),
		maxSize: Number(status.maxSize),
		truncated: status.truncated,
		messages: status.messages.map((message) => ({
			id: message.id.toString(),
			name: message.name,
			createdAtMs: Number(message.createdAtMs),
		})),
	};
}

export const createDefaultActorInspectorContext = ({
	api,
}: {
	api: ActorInspectorApi;
}) => ({
	api,
	actorStateQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: Infinity,
			queryKey: actorInspectorQueriesKeys.actorState(actorId),
			queryFn: () => {
				return api.getState();
			},
		});
	},

	actorIsStateEnabledQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: Infinity,
			queryKey: actorInspectorQueriesKeys.actorIsStateEnabled(actorId),
			queryFn: () => {
				return false;
			},
		});
	},

	actorConnectionsQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: Infinity,
			queryKey: actorInspectorQueriesKeys.actorConnections(actorId),
			queryFn: () => {
				return api.getConnections();
			},
		});
	},

	actorDatabaseQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: 0,
			queryKey: actorInspectorQueriesKeys.actorDatabase(actorId),
			queryFn: () => {
				return api.getDatabaseSchema();
			},
		});
	},

	actorDatabaseEnabledQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: Infinity,
			queryKey: [
				...actorInspectorQueriesKeys.actorDatabase(actorId),
				"enabled",
			],
			queryFn: () => new Promise<boolean>(() => {}),
		});
	},

	actorDatabaseTablesQueryOptions(actorId: ActorId) {
		return queryOptions({
			...this.actorDatabaseQueryOptions(actorId),
			select: (data) =>
				data.tables?.map((table) => ({
					name: table.table.name,
					type: table.table.type,
					records: table.records,
				})) || [],
			notifyOnChangeProps: ["data", "isError", "isLoading"],
		});
	},

	actorDatabaseRowsQueryOptions(
		actorId: ActorId,
		table: string,
		page: number,
		pageSize = 100,
	) {
		return queryOptions({
			staleTime: 0,
			gcTime: 5000,
			queryKey: [
				...actorInspectorQueriesKeys.actorDatabase(actorId),
				table,
				page,
				pageSize,
			],
			queryFn: () => {
				return api.getDatabaseTableRows(
					table,
					pageSize,
					page * pageSize,
				);
			},
		});
	},

	actorDatabaseExecuteMutation(actorId: ActorId) {
		return mutationOptions({
			mutationKey: [
				...actorInspectorQueriesKeys.actorDatabase(actorId),
				"execute",
			],
			mutationFn: async ({
				sql,
				args,
				properties,
			}: {
				sql: string;
				args?: unknown[];
				properties?: Record<string, unknown>;
			}) => {
				return api.executeDatabaseSql({ sql, args, properties });
			},
		});
	},

	actorRpcsQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: Infinity,
			queryKey: actorInspectorQueriesKeys.actorRpcs(actorId),
			queryFn: () => {
				return api.getRpcs();
			},
		});
	},

	actorTracesQueryOptions(actorId: ActorId, options: ReadRangeOptions) {
		return queryOptions({
			staleTime: 0,
			queryKey: [
				...actorInspectorQueriesKeys.actorTraces(actorId),
				options.startMs,
				options.endMs,
				options.limit,
			],
			queryFn: () => {
				return api.getTraces(options);
			},
		});
	},
	actorQueueStatusQueryOptions(actorId: ActorId, limit: number) {
		return queryOptions({
			staleTime: 0,
			queryKey: actorInspectorQueriesKeys.actorQueueStatus(
				actorId,
				limit,
			),
			queryFn: () => {
				return api.getQueueStatus(limit);
			},
		});
	},
	actorQueueSizeQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: Infinity,
			queryKey: actorInspectorQueriesKeys.actorQueueSize(actorId),
			queryFn: () => 0,
		});
	},

	actorWorkflowHistoryQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: Infinity,
			queryKey: actorInspectorQueriesKeys.actorWorkflowHistory(actorId),
			queryFn: () => {
				return api.getWorkflowHistory();
			},
		});
	},

	actorIsWorkflowEnabledQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: Infinity,
			queryKey: actorInspectorQueriesKeys.actorIsWorkflowEnabled(actorId),
			queryFn: () => false,
		});
	},

	// Tab config is delivered by the WS `Init` message and written into this
	// query's cache by the message handler (see `createMessageHandler`). There
	// is no HTTP fetch. The `queryFn` only supplies the empty default for the
	// window before `Init` lands. Because `Init` also flips
	// `actorInspectorInitialized` in the same tick, the tab list and the
	// capability flags become known together, so tabs never flash.
	actorTabConfigQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: Infinity,
			queryKey: actorInspectorQueriesKeys.actorTabConfig(actorId),
			queryFn: (): { tabs: InspectorTabConfigEntry[] } => ({ tabs: [] }),
		});
	},

	// Flipped to `true` by the WS `Init` handler once the real capability
	// flags (workflow/state/database enabled) have landed. Tab computation
	// waits on this instead of on metadata success so it never emits a
	// premature list that omits capability-gated tabs and then re-adds them.
	actorInspectorInitializedQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: Infinity,
			queryKey:
				actorInspectorQueriesKeys.actorInspectorInitialized(actorId),
			queryFn: () => false,
		});
	},

	actorSchedulesQueryOptions(actorId: ActorId) {
		return queryOptions({
			staleTime: Infinity,
			queryKey: actorInspectorQueriesKeys.actorSchedules(actorId),
			queryFn: () => api.getSchedules(),
		});
	},

	actorScheduleHistoryQueryOptions(
		actorId: ActorId,
		scheduleId: string,
		limit = 25,
	) {
		return queryOptions({
			staleTime: Infinity,
			queryKey: actorInspectorQueriesKeys.actorScheduleHistory(
				actorId,
				scheduleId,
			),
			queryFn: () => api.getScheduleHistory(scheduleId, limit),
		});
	},

	actorScheduleDeleteMutation(actorId: ActorId) {
		return mutationOptions({
			mutationKey: ["actor", actorId, "schedules", "delete"],
			mutationFn: ({
				scheduleId,
				kind,
			}: {
				scheduleId: string;
				kind: InspectorSchedule["kind"];
			}) => api.deleteSchedule(scheduleId, kind),
		});
	},

	actorWorkflowReplayMutation(actorId: ActorId) {
		return mutationOptions({
			mutationKey: ["actor", actorId, "workflow", "replay"],
			mutationFn: async (entryId?: string) => {
				return api.replayWorkflowFromStep(entryId);
			},
		});
	},

	actorPingQueryOptions(actorId: ActorId) {
		return queryOptions({
			queryKey: ["actor", actorId, "ping"],
			queryFn: async () => {
				try {
					await api.ping();
					return true;
				} catch {
					return false;
				}
			},
			retry: false,
		});
	},

	actorStatePatchMutation(actorId: ActorId) {
		return mutationOptions({
			mutationKey: ["actor", actorId, "state", "patch"],
			mutationFn: async (state: unknown) => {
				return api.patchState(state);
			},
		});
	},
});

const computeActorUrl = ({ url, actorId }: { url: string; actorId: ActorId }) =>
	new URL(`/gateway/${actorId}`, url).href;

const replayWorkflowFromStepHttp = async ({
	actorId,
	credentials,
	entryId,
}: {
	actorId: ActorId;
	credentials: { url: string; inspectorToken: string; token: string };
	entryId?: string;
}) => {
	const headers: Record<string, string> = {
		Authorization: `Bearer ${credentials.inspectorToken}`,
		"Content-Type": "application/json",
		"X-Rivet-Target": "actor",
		"X-Rivet-Actor": actorId,
	};
	if (credentials.token) {
		headers["x-rivet-token"] = credentials.token;
	}

	const response = await fetch(
		new URL(
			`${computeActorUrl({ url: credentials.url, actorId })}/inspector/workflow/replay`,
		).href,
		{
			method: "POST",
			headers,
			signal: AbortSignal.timeout(10_000),
			body: JSON.stringify(entryId ? { entryId } : {}),
		},
	);

	if (!response.ok) {
		throw new Error(`Failed to replay workflow: ${response.statusText}`);
	}

	const data = z
		.object({ isWorkflowEnabled: z.boolean() })
		.parse((await response.json()) satisfies WorkflowHistoryHttpResponse);

	return { isEnabled: data.isWorkflowEnabled };
};

export const actorWakeUpMutationOptions = () =>
	mutationOptions({
		mutationKey: ["actor", "wake-up"],
		mutationFn: async ({
			actorId,
			credentials,
		}: {
			actorId: ActorId;
			credentials: { url: string; token: string };
		}) => {
			const response = await fetch(
				new URL(
					`${computeActorUrl({ ...credentials, actorId })}/health`,
				).href,
				{
					signal: AbortSignal.timeout(10_000),
					headers: {
						"X-Rivet-Target": "actor",
						"X-Rivet-Actor": actorId,
						"x-rivet-token": credentials.token,
					},
				},
			);

			return await response.text();
		},
	});

const getActorMetadata = async ({
	actorId,
	credentials,
}: {
	actorId: ActorId;
	credentials: { url: string; token: string };
}) => {
	const response = await fetch(
		new URL(`${computeActorUrl({ ...credentials, actorId })}/metadata`)
			.href,
		{
			headers: {
				"X-Rivet-Target": "actor",
				"X-Rivet-Actor": actorId,
				"x-rivet-token": credentials.token,
			},
			signal: AbortSignal.timeout(10_000),
		},
	);

	if (!response.ok) {
		const body: unknown = await response.json().catch(() => undefined);
		const parsed = z.object({ message: z.string() }).safeParse(body);
		throw new RivetError({
			message:
				parsed.data?.message ??
				`Failed to fetch actor metadata: ${response.statusText}`,
			statusCode: response.status,
			body,
		});
	}
	return z
		.object({
			version: z.string(),
			type: z.enum(["local", "deployed"]).optional(),
		})
		.parse(await response.json());
};

export const actorMetadataQueryOptions = ({
	actorId,
	credentials,
}: {
	actorId: ActorId;
	credentials: { url: string; token: string };
}) =>
	queryOptions({
		queryKey: ["actor", actorId, "metadata"],
		retry: 0,
		retryDelay: 5_000,
		refetchInterval: 1_000,
		queryFn: async () => {
			return getActorMetadata({ actorId, credentials });
		},
	});

export type ActorInspectorContext = ReturnType<
	typeof createDefaultActorInspectorContext
> & {
	connectionStatus: ConnectionStatus;
	isInspectorAvailable: boolean;
	rivetkitVersion?: string;
	inspectorProtocolVersion: number;
	features: {
		traces: FeatureSupport;
		queue: FeatureSupport;
		schedules: FeatureSupport;
	};
};

export const ActorInspectorContext = createContext({} as ActorInspectorContext);

export const useActorInspector = () => useContext(ActorInspectorContext);

export const ActorInspectorProvider = ({
	children,
	actorId,
	credentials,
	initialVersion,
}: {
	children: React.ReactNode;
	actorId: ActorId;
	credentials: { url: string; inspectorToken: string; token: string };
	/**
	 * RivetKit version the host already resolved via its own `/metadata`
	 * fetch. When provided it seeds the metadata query so `isInspectorAvailable`
	 * is true on first render and the WebSocket opens without waiting on a
	 * duplicate `/metadata` round trip. Liveness polling still runs.
	 */
	initialVersion?: string;
}) => {
	const protocols = useMemo(
		() =>
			[
				"rivet",
				`rivet_target.actor`,
				`rivet_actor.${actorId}`,
				`rivet_encoding.bare`,
				credentials.token ? `rivet_token.${credentials.token}` : "",
				credentials.inspectorToken
					? `rivet_inspector_token.${credentials.inspectorToken}`
					: "",
			].filter(Boolean),
		[actorId, credentials.token, credentials.inspectorToken],
	);

	const queryClient = useQueryClient();

	const actionsManager = useRef(new ActionsManager());

	const { data: actorMetadata, isSuccess: isActorMetadataSuccess } = useQuery(
		{
			...actorMetadataQueryOptions({ actorId, credentials }),
			...(initialVersion
				? {
						initialData: { version: initialVersion },
						// Mark the seed as fresh so it doesn't trigger an
						// immediate refetch; `refetchInterval` still polls for
						// liveness a beat later.
						initialDataUpdatedAt: () => Date.now(),
					}
				: {}),
		},
	);

	// The provider is "available" as soon as the actor's metadata fetch
	// resolves — that proves the credentials reach the actor and the
	// inspector protocol version is known. No separate data-provider gate is
	// needed; callers that need to fetch the inspector token before mounting
	// this provider do so themselves.
	const isInspectorAvailable = isActorMetadataSuccess;
	const rivetkitVersion = actorMetadata?.version;
	const inspectorProtocolVersion = useMemo(
		() =>
			Math.min(
				getInspectorProtocolVersion(rivetkitVersion),
				INSPECTOR_PROTOCOL_CURRENT_VERSION,
			),
		[rivetkitVersion],
	);
	const negotiatedInspectorProtocol =
		usesNegotiatedInspectorProtocol(rivetkitVersion);
	const features = useMemo(
		() => ({
			traces: buildFeatureSupport(
				rivetkitVersion,
				MIN_RIVETKIT_VERSION_TRACES,
				"Traces",
			),
			queue: buildFeatureSupport(
				rivetkitVersion,
				MIN_RIVETKIT_VERSION_QUEUE,
				"Queue",
			),
			schedules: buildFeatureSupport(
				rivetkitVersion,
				MIN_RIVETKIT_VERSION_SCHEDULES,
				"Schedules",
			),
		}),
		[rivetkitVersion],
	);

	const onMessage = useMemo(() => {
		return createMessageHandler({
			queryClient,
			actorId,
			actionsManager,
			version: inspectorProtocolVersion,
			negotiated: negotiatedInspectorProtocol,
		});
	}, [
		queryClient,
		actorId,
		inspectorProtocolVersion,
		negotiatedInspectorProtocol,
	]);

	const inspectorUrl = buildInspectorWebSocketUrl(
		computeActorUrl({ ...credentials, actorId }),
		inspectorProtocolVersion,
		negotiatedInspectorProtocol,
	);

	const { sendMessage, reconnect, status } = useWebSocket(
		inspectorUrl,
		protocols,
		{ onMessage, enabled: isInspectorAvailable },
	);

	const getActorMetadataProxy = useRef(async () => {
		return getActorMetadata({ actorId, credentials });
	});

	const api = useMemo(() => {
		return {
			ping: async () => {
				return reconnect();
			},
			executeAction: async (name, args) => {
				const { id, promise } =
					actionsManager.current.createResolver<unknown>({
						name: "executeAction",
					});

				sendMessage(
					serverMessage(
						{
							body: {
								tag: "ActionRequest",
								val: {
									id: BigInt(id),
									name,
									args: new Uint8Array(cbor.encode(args))
										.buffer,
								},
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);

				return promise;
			},

			patchState: async (state) => {
				sendMessage(
					serverMessage(
						{
							body: {
								tag: "PatchStateRequest",
								val: {
									state: new Uint8Array(cbor.encode(state))
										.buffer,
								},
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);
			},

			getConnections: async () => {
				const { id, promise } = actionsManager.current.createResolver<
					Connection[]
				>({ name: "getConnections" });

				sendMessage(
					serverMessage(
						{
							body: {
								tag: "ConnectionsRequest",
								val: { id: BigInt(id) },
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);

				return promise;
			},

			getState: async () => {
				const { id, promise } = actionsManager.current.createResolver<{
					isEnabled: boolean;
					state: unknown;
				}>({ name: "getState" });

				sendMessage(
					serverMessage(
						{
							body: {
								tag: "StateRequest",
								val: { id: BigInt(id) },
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);

				return promise;
			},

			getRpcs() {
				const { id, promise } = actionsManager.current.createResolver<
					string[]
				>({ name: "getRpcs" });

				sendMessage(
					serverMessage(
						{
							body: {
								tag: "RpcsListRequest",
								val: { id: BigInt(id) },
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);

				return promise;
			},
			getTraces: async ({ startMs, endMs, limit }) => {
				const { id, promise } =
					actionsManager.current.createResolver<ReadRangeWire>({
						name: "getTraces",
						timeoutMs: 10_000,
					});

				sendMessage(
					serverMessage(
						{
							body: {
								tag: "TraceQueryRequest",
								val: {
									id: BigInt(id),
									startMs: BigInt(Math.floor(startMs)),
									endMs: BigInt(Math.floor(endMs)),
									limit: BigInt(limit),
								},
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);

				return promise;
			},
			getQueueStatus: async (limit) => {
				const safeLimit = Math.max(0, Math.floor(limit));
				const { id, promise } =
					actionsManager.current.createResolver<QueueStatusSummary>({
						name: "getQueueStatus",
						timeoutMs: 10_000,
					});

				sendMessage(
					serverMessage(
						{
							body: {
								tag: "QueueRequest",
								val: {
									id: BigInt(id),
									limit: BigInt(safeLimit),
								},
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);

				return promise;
			},

			getWorkflowHistory: async () => {
				const { id, promise } = actionsManager.current.createResolver<{
					history: WorkflowHistory | null;
					isEnabled: boolean;
					error: string | null;
				}>({
					name: "getWorkflowHistory",
					timeoutMs: 10_000,
				});

				sendMessage(
					serverMessage(
						{
							body: {
								tag: "WorkflowHistoryRequest",
								val: { id: BigInt(id) },
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);

				return promise;
			},

			replayWorkflowFromStep: async (entryId) => {
				return replayWorkflowFromStepHttp({
					actorId,
					credentials: {
						url: credentials.url,
						inspectorToken: credentials.inspectorToken,
						token: credentials.token,
					},
					entryId,
				});
			},

			getDatabaseSchema: async () => {
				const { id, promise } =
					actionsManager.current.createResolver<DatabaseSchema>({
						name: "getDatabaseSchema",
						timeoutMs: 10_000,
					});

				sendMessage(
					serverMessage(
						{
							body: {
								tag: "DatabaseSchemaRequest",
								val: { id: BigInt(id) },
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);

				return promise;
			},

			getDatabaseTableRows: async (table, limit, offset) => {
				const { id, promise } = actionsManager.current.createResolver<
					unknown[]
				>({
					name: "getDatabaseTableRows",
					timeoutMs: 10_000,
				});

				sendMessage(
					serverMessage(
						{
							body: {
								tag: "DatabaseTableRowsRequest",
								val: {
									id: BigInt(id),
									table,
									limit: BigInt(limit),
									offset: BigInt(offset),
								},
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);

				return promise;
			},

			executeDatabaseSql: async ({ sql, args, properties }) => {
				const headers: Record<string, string> = {
					"Content-Type": "application/json",
					"X-Rivet-Target": "actor",
					"X-Rivet-Actor": actorId,
				};
				if (credentials.inspectorToken) {
					headers.Authorization = `Bearer ${credentials.inspectorToken}`;
				}
				if (credentials.token) {
					headers["x-rivet-token"] = credentials.token;
				}

				const response = await fetch(
					`${computeActorUrl({ ...credentials, actorId })}/inspector/database/execute`,
					{
						method: "POST",
						headers,
						body: JSON.stringify({
							sql,
							args,
							properties,
						}),
						signal: AbortSignal.timeout(10_000),
					},
				);

				const payload = await response
					.json()
					.catch(() => ({ error: response.statusText }));

				if (!response.ok) {
					const errorMessage =
						typeof payload?.error === "string"
							? payload.error
							: "Failed to execute SQL";
					throw new Error(errorMessage);
				}

				return z
					.object({
						rows: z.array(z.unknown()),
					})
					.parse(payload);
			},

			getSchedules: async () => {
				const { id, promise } = actionsManager.current.createResolver<
					InspectorSchedule[]
				>({ name: "getSchedules", timeoutMs: 10_000 });
				sendMessage(
					serverMessage(
						{
							body: {
								tag: "SchedulesRequest",
								val: { id: BigInt(id) },
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);
				return promise;
			},

			getScheduleHistory: async (scheduleId, limit) => {
				const { id, promise } = actionsManager.current.createResolver<
					InspectorScheduleFire[]
				>({ name: "getScheduleHistory", timeoutMs: 10_000 });
				sendMessage(
					serverMessage(
						{
							body: {
								tag: "ScheduleHistoryRequest",
								val: {
									id: BigInt(id),
									scheduleId,
									limit: BigInt(
										Math.max(1, Math.floor(limit)),
									),
								},
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);
				return promise;
			},

			deleteSchedule: async (scheduleId, kind) => {
				const { id, promise } =
					actionsManager.current.createResolver<boolean>({
						name: "deleteSchedule",
						timeoutMs: 10_000,
					});
				sendMessage(
					serverMessage(
						{
							body: {
								tag: "ScheduleDeleteRequest",
								val: { id: BigInt(id), scheduleId, kind },
							},
						},
						inspectorProtocolVersion,
						negotiatedInspectorProtocol,
					),
				);
				return promise;
			},

			getMetadata() {
				return getActorMetadataProxy.current();
			},
		} satisfies ActorInspectorApi;
	}, [
		sendMessage,
		reconnect,
		inspectorProtocolVersion,
		negotiatedInspectorProtocol,
		actorId,
		credentials,
	]);

	const value = useMemo(() => {
		return {
			connectionStatus: status,
			isInspectorAvailable,
			rivetkitVersion,
			inspectorProtocolVersion,
			features,
			...createDefaultActorInspectorContext({
				api,
			}),
		};
	}, [
		api,
		status,
		isInspectorAvailable,
		rivetkitVersion,
		inspectorProtocolVersion,
		features,
	]);

	return (
		<ActorInspectorContext.Provider value={value}>
			{children}
		</ActorInspectorContext.Provider>
	);
};

const createMessageHandler =
	({
		queryClient,
		actorId,
		actionsManager,
		version,
		negotiated,
	}: {
		queryClient: QueryClient;
		actorId: ActorId;
		actionsManager: React.RefObject<ActionsManager>;
		version: number;
		negotiated: boolean;
	}) =>
	async (e: ReconnectingWebSocket.MessageEvent) => {
		let message: ReturnType<typeof toClient.deserialize>;
		try {
			const bytes = new Uint8Array(await e.data.arrayBuffer());
			message = negotiated
				? toClient.deserialize(bytes, version)
				: toClient.deserializeWithEmbeddedVersion(bytes);
		} catch (error) {
			console.warn("Failed to decode inspector message", error);
			return;
		}

		match(message.body)
			.with({ tag: "Init" }, (body) => {
				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorSchedules(actorId),
					normalizeSchedules(body.val.schedules ?? []),
				);
				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorState(actorId),
					!body.val.isStateEnabled || body.val.state == null
						? { state: null, isEnabled: body.val.isStateEnabled }
						: {
								state: transformState(body.val.state),
								isEnabled: body.val.isStateEnabled,
							},
				);

				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorConnections(actorId),
					transformConnections(body.val.connections),
				);

				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorIsStateEnabled(actorId),
					body.val.isStateEnabled,
				);

				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorIsWorkflowEnabled(actorId),
					body.val.isWorkflowEnabled,
				);

				queryClient.setQueryData(
					[
						...actorInspectorQueriesKeys.actorDatabase(actorId),
						"enabled",
					],
					body.val.isDatabaseEnabled,
				);

				if (body.val.workflowHistory) {
					queryClient.setQueryData(
						actorInspectorQueriesKeys.actorWorkflowHistory(actorId),
						transformWorkflowHistoryFromInspector(
							body.val.workflowHistory,
						),
					);
				}

				// Tab config rides on `Init` (protocol v5+) instead of a
				// separate HTTP request, so the custom-tab list and the
				// capability flags land in the same tick. Older runtimes send
				// an empty list (the v4 -> v5 converter fills it in), which the
				// consumer reads as "built-in tabs only".
				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorTabConfig(actorId),
					{
						tabs: (body.val.tabConfig ?? []).map((tab) => ({
							id: tab.id,
							label: tab.label ?? undefined,
							icon: tab.icon,
							hidden: tab.hidden,
						})),
					} satisfies { tabs: InspectorTabConfigEntry[] },
				);

				// Mark capabilities as known only now — the flags above are
				// the source of truth for which tabs exist. Tab computation
				// gates on this to avoid a skeleton → partial → full flash.
				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorInspectorInitialized(
						actorId,
					),
					true,
				);
			})
			.with({ tag: "ConnectionsResponse" }, (body) => {
				const { rid } = body.val;
				actionsManager.current.resolve(
					Number(rid),
					transformConnections(body.val.connections),
				);
			})
			.with({ tag: "StateResponse" }, (body) => {
				const { rid } = body.val;
				actionsManager.current.resolve(
					Number(rid),
					body.val.state
						? {
								state: cbor.decode(
									new Uint8Array(body.val.state),
								),
								isEnabled: body.val.isStateEnabled,
							}
						: { state: null, isEnabled: body.val.isStateEnabled },
				);
			})
			.with({ tag: "ActionResponse" }, (body) => {
				const { rid } = body.val;
				actionsManager.current.resolve(
					Number(rid),
					cbor.decode(new Uint8Array(body.val.output)),
				);
			})
			.with({ tag: "ConnectionsUpdated" }, (body) => {
				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorConnections(actorId),
					transformConnections(body.val.connections),
				);
			})
			.with({ tag: "StateUpdated" }, (body) => {
				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorState(actorId),
					{ isEnabled: true, state: transformState(body.val.state) },
				);
			})
			.with({ tag: "RpcsListResponse" }, (body) => {
				const { rid } = body.val;
				actionsManager.current.resolve(Number(rid), body.val.rpcs);
			})
			.with({ tag: "TraceQueryResponse" }, (body) => {
				const { rid } = body.val;
				actionsManager.current.resolve(
					Number(rid),
					decodeReadRangeWire(new Uint8Array(body.val.payload)),
				);
			})
			.with({ tag: "QueueResponse" }, (body) => {
				const { rid, status } = body.val;
				actionsManager.current.resolve(
					Number(rid),
					normalizeQueueStatus(status),
				);
			})
			.with({ tag: "QueueUpdated" }, (body) => {
				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorQueueSize(actorId),
					Number(body.val.queueSize),
				);
				queryClient.invalidateQueries({
					queryKey: ["actor", actorId, "queue"],
				});
			})
			.with({ tag: "WorkflowHistoryUpdated" }, (body) => {
				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorWorkflowHistory(actorId),
					transformWorkflowHistoryFromInspector(body.val.history),
				);
			})
			.with({ tag: "WorkflowHistoryResponse" }, (body) => {
				const { rid } = body.val;
				const transformed = body.val.history
					? transformWorkflowHistoryFromInspector(body.val.history)
					: null;
				actionsManager.current.resolve(Number(rid), {
					history: transformed?.history ?? null,
					isEnabled: body.val.isWorkflowEnabled,
					error: transformed?.error ?? null,
				});
			})
			.with({ tag: "WorkflowReplayResponse" }, (body) => {
				const { rid } = body.val;
				const transformed = body.val.history
					? transformWorkflowHistoryFromInspector(body.val.history)
					: null;
				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorWorkflowHistory(actorId),
					transformed,
				);
				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorIsWorkflowEnabled(actorId),
					body.val.isWorkflowEnabled,
				);
				actionsManager.current.resolve(Number(rid), {
					history: transformed?.history ?? null,
					isEnabled: body.val.isWorkflowEnabled,
					error: transformed?.error ?? null,
				});
			})
			.with({ tag: "DatabaseSchemaResponse" }, (body) => {
				const { rid } = body.val;
				actionsManager.current.resolve(
					Number(rid),
					cbor.decode(new Uint8Array(body.val.schema)),
				);
			})
			.with({ tag: "DatabaseTableRowsResponse" }, (body) => {
				const { rid } = body.val;
				actionsManager.current.resolve(
					Number(rid),
					cbor.decode(new Uint8Array(body.val.result)),
				);
			})
			.with({ tag: "SchedulesResponse" }, (body) => {
				actionsManager.current.resolve(
					Number(body.val.rid),
					normalizeSchedules(body.val.schedules),
				);
			})
			.with({ tag: "SchedulesUpdated" }, (body) => {
				queryClient.setQueryData(
					actorInspectorQueriesKeys.actorSchedules(actorId),
					normalizeSchedules(body.val.schedules),
				);
				queryClient.invalidateQueries({
					queryKey: actorInspectorQueriesKeys.actorSchedules(actorId),
					predicate: (query) => query.queryKey.includes("history"),
				});
			})
			.with({ tag: "ScheduleHistoryResponse" }, (body) => {
				actionsManager.current.resolve(
					Number(body.val.rid),
					normalizeScheduleHistory(body.val.history),
				);
			})
			.with({ tag: "ScheduleDeleteResponse" }, (body) => {
				actionsManager.current.resolve(
					Number(body.val.rid),
					body.val.deleted,
				);
			})
			.with({ tag: "Error" }, (body) => {
				if (body.val.message === INSPECTOR_ERROR_EVENTS_DROPPED) {
					return;
				}
				toast.error(`Inspector error: ${body.val.message}`);
			})
			.exhaustive();
	};

function transformConnections(connections: readonly Connection[]) {
	return connections.map((connection) => ({
		...connection,
		details: cbor.decode(new Uint8Array(connection.details)),
	}));
}

function transformState(state: ArrayBuffer) {
	return cbor.decode(new Uint8Array(state));
}

function transformWorkflowHistoryFromInspector(raw: ArrayBuffer): {
	history: WorkflowHistory | null;
	isEnabled: boolean;
	error: string | null;
} {
	try {
		const decoded = decodeWorkflowHistoryTransport(raw);
		return {
			history: transformWorkflowHistory(decoded),
			isEnabled: true,
			error: null,
		};
	} catch (error) {
		// A decode failure is a protocol mismatch, not an empty workflow. It is
		// reported so the tab cannot render it as "nothing recorded yet".
		console.error("Failed to decode workflow history", error);
		return {
			history: null,
			isEnabled: true,
			error:
				error instanceof Error
					? error.message
					: "Workflow history could not be decoded.",
		};
	}
}

export function serverMessage(
	data: ToServer,
	version: number,
	negotiated: boolean,
) {
	return negotiated
		? toServer.serialize(data, version)
		: toServer.serializeWithEmbeddedVersion(data, version);
}

class ActionsManager {
	private suspensions = new Map<number, PromiseWithResolvers<any>>();

	private nextId = 1;

	createResolver<T = void>(options?: {
		timeoutMs?: number;
		name?: string;
	}): { id: number; promise: Promise<T> } {
		const id = this.nextId++;
		const { promise, resolve, reject } = Promise.withResolvers<T>();
		this.suspensions.set(id, { promise, resolve, reject });
		const timeoutMs = options?.timeoutMs ?? 2_000;

		// set a timeout to reject the promise if not resolved in time
		setTimeout(() => {
			if (this.suspensions.has(id)) {
				reject(
					new Error(
						`Action timed out: ${options?.name ?? "unknown"}`,
					),
				);
				this.suspensions.delete(id);
			}
		}, timeoutMs);

		return { id, promise };
	}

	resolve(id: number, value?: unknown) {
		const suspension = this.suspensions.get(id);
		if (suspension) {
			suspension.resolve(value);
			this.suspensions.delete(id);
		}
	}
}
