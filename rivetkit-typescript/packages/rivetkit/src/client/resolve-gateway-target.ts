import { actorNotFound, invalidRequest } from "@/actor/errors";
import type {
	EngineControlClient,
	GatewayTarget,
} from "@/engine-client/driver";

/**
 * Resolves a GatewayTarget to a concrete actor ID string.
 *
 * Shared across all EngineControlClient implementations to avoid duplicating the
 * same query-to-actorId dispatch logic.
 */
export async function resolveGatewayTarget(
	driver: EngineControlClient,
	target: GatewayTarget,
): Promise<string> {
	if ("directId" in target) {
		return target.directId;
	}

	if ("getForId" in target) {
		return target.getForId.actorId;
	}

	if ("getForKey" in target) {
		const output = await driver.getWithKey({
			name: target.getForKey.name,
			key: target.getForKey.key,
		});
		if (!output) {
			throw actorNotFound(
				`${target.getForKey.name}:${JSON.stringify(target.getForKey.key)}`,
			);
		}
		return output.actorId;
	}

	if ("getOrCreateForKey" in target) {
		const output = await driver.getOrCreateWithKey({
			name: target.getOrCreateForKey.name,
			key: target.getOrCreateForKey.key,
			input: target.getOrCreateForKey.input,
			region: target.getOrCreateForKey.region,
			poolName: target.getOrCreateForKey.poolName,
		});
		return output.actorId;
	}

	if ("create" in target) {
		const output = await driver.createActor({
			name: target.create.name,
			key: target.create.key,
			input: target.create.input,
			region: target.create.region,
			poolName: target.create.poolName,
		});
		return output.actorId;
	}

	throw invalidRequest("Invalid query format");
}
