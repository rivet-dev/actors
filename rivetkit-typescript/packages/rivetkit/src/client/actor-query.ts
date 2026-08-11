import * as errors from "@/actor/errors";
import type { ActorQuery } from "@/client/query";
import { stringifyError } from "@/common/utils";
import type {
	EngineControlClient,
	GatewayTarget,
} from "@/engine-client/driver";
import { type ActorSchedulingError, actorSchedulingError } from "./errors";
import { logger } from "./log";

/**
 * Extract the actor name from a query.
 */
export function getActorNameFromQuery(query: ActorQuery): string {
	if ("getForId" in query) return query.getForId.name;
	if ("getForKey" in query) return query.getForKey.name;
	if ("getOrCreateForKey" in query) return query.getOrCreateForKey.name;
	if ("create" in query) return query.create.name;
	throw errors.invalidRequest("Invalid query format");
}

export type ActorResolutionState = ActorQuery;

export function isDynamicActorQuery(
	actorQuery: ActorQuery,
): actorQuery is
	| Extract<ActorQuery, { getForKey: unknown }>
	| Extract<ActorQuery, { getOrCreateForKey: unknown }> {
	return "getForKey" in actorQuery || "getOrCreateForKey" in actorQuery;
}

export function getGatewayTarget(state: ActorResolutionState): GatewayTarget {
	if ("getForId" in state) {
		return { directId: state.getForId.actorId };
	}

	if ("create" in state) {
		throw errors.invalidRequest(
			"create queries cannot be used as gateway targets. Resolve to an actor ID first.",
		);
	}

	return state;
}

export function isStaleResolvedActorError(
	group: string,
	code: string,
): boolean {
	return (
		group === "actor" &&
		(code === "not_found" ||
			code === "starting" ||
			code === "stopping" ||
			code === "not_configured" ||
			code === "dropped_reply" ||
			code.startsWith("destroyed_"))
	);
}

/**
 * Fetch actor details and check for scheduling errors.
 */
export async function checkForSchedulingError(
	group: string,
	code: string,
	actorId: string,
	query: ActorQuery,
	driver: EngineControlClient,
	rayId?: string,
): Promise<ActorSchedulingError | null> {
	const name = getActorNameFromQuery(query);

	try {
		const actor = await driver.getForId({ name, actorId });

		if (actor?.error) {
			logger().info({
				msg: "found actor scheduling error",
				actorId,
				error: actor.error,
			});
			return actorSchedulingError(
				group,
				code,
				actorId,
				actor.error,
				rayId,
			);
		}
	} catch (err) {
		logger().warn({
			msg: "failed to fetch actor details for scheduling error check",
			actorId,
			error: stringifyError(err),
		});
	}

	return null;
}
