import type {
	AgentOsOptions,
	JsonRpcNotification,
	PermissionRequest,
} from "@rivet-dev/agent-os-core";
import { z } from "zod/v4";
import type {
	ActorContext,
	ActorOptionsInput,
	BeforeConnectContext,
} from "@/actor/config";
import type { AgentOsActorState, AgentOsActorVars } from "./types";

const zFunction = <
	T extends (...args: any[]) => any = (...args: unknown[]) => unknown,
>() => z.custom<T>((val) => typeof val === "function");

const AgentOsOptionsSchema = z.custom<AgentOsOptions>(
	(val) => typeof val === "object" && val !== null,
);

export const agentOsActorConfigSchema = z
	.object({
		options: AgentOsOptionsSchema.optional(),
		actorOptions: z
			.record(z.string(), z.unknown())
			.optional(),
		preview: z
			.object({
				defaultExpiresInSeconds: z.number().positive().default(3600),
				maxExpiresInSeconds: z.number().positive().default(86400),
			})
			.strict()
			.prefault(() => ({})),
		onBeforeConnect: zFunction().optional(),
		onSessionEvent: zFunction().optional(),
		onPermissionRequest: zFunction().optional(),
	})
	.strict();

// --- Typed config types (generic callbacks overlaid on the Zod schema) ---

type AgentOsActorContext<TConnParams> = ActorContext<
	AgentOsActorState,
	TConnParams,
	undefined,
	AgentOsActorVars,
	undefined,
	any
>;

interface AgentOsActorConfigCallbacks<TConnParams> {
	onBeforeConnect?: (
		c: BeforeConnectContext<
			AgentOsActorState,
			AgentOsActorVars,
			undefined,
			any
		>,
		params: TConnParams,
	) => void | Promise<void>;
	onSessionEvent?: (
		c: AgentOsActorContext<TConnParams>,
		sessionId: string,
		event: JsonRpcNotification,
	) => void | Promise<void>;
	onPermissionRequest?: (
		c: AgentOsActorContext<TConnParams>,
		sessionId: string,
		request: PermissionRequest,
	) => void | Promise<void>;
}

// Parsed config (after Zod defaults/transforms applied).
export type AgentOsActorConfig<TConnParams = undefined> = Omit<
	z.infer<typeof agentOsActorConfigSchema>,
	"onBeforeConnect" | "onSessionEvent" | "onPermissionRequest" | "actorOptions"
> & {
	actorOptions?: ActorOptionsInput;
} & AgentOsActorConfigCallbacks<TConnParams>;

// Input config (what users pass in before Zod transforms).
export type AgentOsActorConfigInput<TConnParams = undefined> = Omit<
	z.input<typeof agentOsActorConfigSchema>,
	"onBeforeConnect" | "onSessionEvent" | "onPermissionRequest" | "actorOptions"
> & {
	actorOptions?: ActorOptionsInput;
} & AgentOsActorConfigCallbacks<TConnParams>;
