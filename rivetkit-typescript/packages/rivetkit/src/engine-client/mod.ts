import type { Context as HonoContext } from "hono";
import invariant from "invariant";
import { deserializeActorKey, serializeActorKey } from "@/actor/keys";
import type { ClientConfig } from "@/client/client";
import {
	PATH_CONNECT,
	PATH_WEBSOCKET_BASE,
	PATH_WEBSOCKET_PREFIX,
} from "@/common/actor-router-consts";
import type { JsonCompatValue } from "@/common/encoding";
import { noopNext } from "@/common/utils";
import type { Actor as ApiActor } from "@/engine-api/actors";
import type {
	ActorOutput,
	CreateInput,
	EngineControlClient,
	GatewayRequestOptions,
	GatewayTarget,
	GetForIdInput,
	GetOrCreateWithKeyInput,
	GetWithKeyInput,
	ListActorsInput,
	RuntimeDisplayInformation,
} from "@/engine-client/driver";
import { shouldSkipReadyWait } from "@/engine-client/driver";
import type { Encoding, UniversalWebSocket } from "@/mod";
import { encodeCborCompat, uint8ArrayToBase64 } from "@/serde";
import { combineUrlPath, type GetUpgradeWebSocket } from "@/utils";
import { getNextPhase } from "@/utils/env-vars";
import { sendHttpRequestToGateway } from "./actor-http-client";
import {
	buildActorGatewayUrl,
	buildActorQueryGatewayUrl,
	buildWebSocketProtocols,
	openWebSocketToGateway,
} from "./actor-websocket-client";
import {
	createActor,
	destroyActor,
	getActor,
	getActorByKey,
	getOrCreateActor,
	listActorsByName,
} from "./api-endpoints";
import { EngineApiError, getEndpoint } from "./api-utils";
import { logger } from "./log";
import { lookupMetadataCached } from "./metadata";
import { createWebSocketProxy } from "./ws-proxy";

export class RemoteEngineControlClient implements EngineControlClient {
	#config: ClientConfig;
	#metadataPromise: Promise<void> | undefined;

	constructor(runConfig: ClientConfig) {
		// Disable health check if in Next.js build phase since there is no `/metadata` endpoint
		//
		// See https://github.com/vercel/next.js/blob/5e6b008b561caf2710ab7be63320a3d549474a5b/packages/next/shared/lib/constants.ts#L19-L23
		if (getNextPhase() === "phase-production-build") {
			logger().info(
				"detected next.js build phase, disabling health check",
			);
			runConfig.disableMetadataLookup = true;
		}

		// Clone config so we can mutate the endpoint in #metadataPromise
		// NOTE: This is a shallow clone, so mutating nested properties will not do anything
		this.#config = { ...runConfig };

		// Perform metadata check if enabled
		if (!runConfig.disableMetadataLookup) {
			// This should never error, since it uses pRetry. If it does for
			// any reason, we'll surface the error anywhere #metadataPromise is
			// awaited.
			this.#metadataPromise = lookupMetadataCached(this.#config).then(
				(metadataData) => {
					// Override endpoint for all future requests
					if (metadataData.clientEndpoint) {
						this.#config.endpoint = metadataData.clientEndpoint;
						if (metadataData.clientNamespace) {
							this.#config.namespace =
								metadataData.clientNamespace;
						}
						if (metadataData.clientToken) {
							this.#config.token = metadataData.clientToken;
						}

						logger().info({
							msg: "overriding client endpoint",
							endpoint: metadataData.clientEndpoint,
							namespace: metadataData.clientNamespace,
							token: metadataData.clientToken,
						});
					}

					logger().info({
						msg: "connected to rivetkit runtime",
						runtime: metadataData.runtime,
						version: metadataData.version,
						envoy: metadataData.envoy,
					});
				},
			);
		}
	}

	async getForId({
		name,
		actorId,
	}: GetForIdInput): Promise<ActorOutput | undefined> {
		await this.#metadataPromise;

		// Fetch from API if not in cache
		const response = await getActor(this.#config, name, actorId);
		const actor = response.actors[0];
		if (!actor) return undefined;

		// Validate name matches
		if (actor.name !== name) {
			logger().debug({
				msg: "actor name mismatch from api",
				actorId,
				apiName: actor.name,
				requestedName: name,
			});
			return undefined;
		}

		return apiActorToOutput(actor);
	}

	async getWithKey({
		name,
		key,
	}: GetWithKeyInput): Promise<ActorOutput | undefined> {
		await this.#metadataPromise;

		logger().debug({ msg: "getWithKey: searching for actor", name, key });

		// If not in local cache, fetch by key from API
		try {
			const response = await getActorByKey(this.#config, name, key);
			const actor = response.actors[0];
			if (!actor) return undefined;

			logger().debug({
				msg: "getWithKey: found actor via api",
				actorId: actor.actor_id,
				name,
				key,
			});

			return apiActorToOutput(actor);
		} catch (error) {
			if (
				error instanceof EngineApiError &&
				(error as EngineApiError).group === "actor" &&
				(error as EngineApiError).code === "not_found"
			) {
				return undefined;
			}
			throw error;
		}
	}

	async getOrCreateWithKey(
		input: GetOrCreateWithKeyInput,
	): Promise<ActorOutput> {
		await this.#metadataPromise;

		const {
			name,
			key,
			input: actorInput,
			region,
			crashPolicy,
			poolName,
		} = input;

		logger().info({
			msg: "getOrCreateWithKey: getting or creating actor via engine api",
			name,
			key,
		});

		const { actor, created } = await getOrCreateActor(this.#config, {
			datacenter: region,
			name,
			key: serializeActorKey(key),
			runner_name_selector: poolName ?? this.#config.poolName,
			input: actorInput
				? uint8ArrayToBase64(
						encodeCborCompat(actorInput as JsonCompatValue),
					)
				: undefined,
			crash_policy: crashPolicy ?? "sleep",
		});

		logger().info({
			msg: "getOrCreateWithKey: actor ready",
			actorId: actor.actor_id,
			name,
			key,
			created,
		});

		return apiActorToOutput(actor);
	}

	async createActor({
		name,
		key,
		input,
		region,
		crashPolicy,
		poolName,
	}: CreateInput): Promise<ActorOutput> {
		await this.#metadataPromise;

		logger().info({ msg: "creating actor via engine api", name, key });

		// Create actor via engine API
		const result = await createActor(this.#config, {
			datacenter: region,
			name,
			runner_name_selector: poolName ?? this.#config.poolName,
			key: serializeActorKey(key),
			input: input
				? uint8ArrayToBase64(encodeCborCompat(input as JsonCompatValue))
				: undefined,
			crash_policy: crashPolicy ?? "sleep",
		});

		logger().info({
			msg: "actor created",
			actorId: result.actor.actor_id,
			name,
			key,
		});

		return apiActorToOutput(result.actor);
	}

	async listActors({ name }: ListActorsInput): Promise<ActorOutput[]> {
		await this.#metadataPromise;

		logger().debug({ msg: "listing actors via engine api", name });

		const response = await listActorsByName(this.#config, name);

		return response.actors.map(apiActorToOutput);
	}

	async destroyActor(actorId: string): Promise<void> {
		await this.#metadataPromise;

		logger().info({ msg: "destroying actor via engine api", actorId });

		await destroyActor(this.#config, actorId);

		logger().info({ msg: "actor destroyed", actorId });
	}

	async sendRequest(
		target: GatewayTarget,
		actorRequest: Request,
		options: GatewayRequestOptions = {},
	): Promise<Response> {
		await this.#metadataPromise;

		const path = requestPath(actorRequest);
		const gatewayUrl = this.#buildGatewayUrlForTarget(
			target,
			path,
			options,
		);
		const httpOptions = {
			...options,
			directActorId: shouldSkipReadyWait(options)
				? directActorIdFromTarget(target)
				: undefined,
		};

		return sendHttpRequestToGateway(
			this.#config,
			gatewayUrl,
			actorRequest,
			httpOptions,
		);
	}

	async openWebSocket(
		path: string,
		target: GatewayTarget,
		encoding: Encoding,
		params: unknown,
		options: GatewayRequestOptions = {},
	): Promise<UniversalWebSocket> {
		await this.#metadataPromise;

		const gatewayUrl = this.#buildGatewayUrlForTarget(
			target,
			path,
			options,
		);

		return openWebSocketToGateway(
			this.#config,
			gatewayUrl,
			encoding,
			params,
			{
				...options,
				directActorId: shouldSkipReadyWait(options)
					? directActorIdFromTarget(target)
					: undefined,
			},
		);
	}

	async buildGatewayUrl(
		target: GatewayTarget,
		options: GatewayRequestOptions = {},
	): Promise<string> {
		await this.#metadataPromise;
		return this.#buildGatewayUrlForTarget(target, "", options);
	}

	async proxyRequest(
		_c: HonoContext,
		actorRequest: Request,
		actorId: string,
	): Promise<Response> {
		await this.#metadataPromise;

		const gatewayUrl = this.#buildGatewayUrlForTarget(
			{ directId: actorId },
			requestPath(actorRequest),
		);

		return sendHttpRequestToGateway(this.#config, gatewayUrl, actorRequest);
	}

	async proxyWebSocket(
		c: HonoContext,
		path: string,
		actorId: string,
		encoding: Encoding,
		params: unknown,
	): Promise<Response> {
		await this.#metadataPromise;

		const upgradeWebSocket = this.#config.getUpgradeWebSocket?.();
		invariant(upgradeWebSocket, "missing getUpgradeWebSocket");

		const endpoint = getEndpoint(this.#config);
		const guardUrl = combineUrlPath(endpoint, path);
		const wsGuardUrl = guardUrl.replace("http://", "ws://");

		logger().debug({
			msg: "forwarding websocket to actor via guard",
			actorId,
			path,
			guardUrl,
		});

		// Build protocols
		const protocols = buildWebSocketProtocols(
			this.#config,
			encoding,
			params,
			undefined,
			{
				target: "actor",
				actorId,
			},
		);
		const args = await createWebSocketProxy(c, wsGuardUrl, protocols);

		return await upgradeWebSocket(() => args)(c, noopNext());
	}

	displayInformation(): RuntimeDisplayInformation {
		return { properties: {} };
	}

	setGetUpgradeWebSocket(getUpgradeWebSocket: GetUpgradeWebSocket): void {
		this.#config.getUpgradeWebSocket = getUpgradeWebSocket;
	}

	#buildGatewayUrlForTarget(
		target: GatewayTarget,
		path: string,
		options: GatewayRequestOptions = {},
	): string {
		const endpoint = getEndpoint(this.#config);

		if (
			shouldSkipReadyWait(options) &&
			directActorIdFromTarget(target) &&
			canUseDirectSkipReadyWaitPath(path)
		) {
			return combineUrlPath(endpoint, path);
		}

		if ("directId" in target) {
			return buildActorGatewayUrl(
				endpoint,
				target.directId,
				this.#config.token,
				path,
			);
		}

		if ("getForId" in target) {
			return buildActorGatewayUrl(
				endpoint,
				target.getForId.actorId,
				this.#config.token,
				path,
			);
		}

		if ("getForKey" in target || "getOrCreateForKey" in target) {
			return buildActorQueryGatewayUrl(
				endpoint,
				this.#config.namespace,
				target,
				this.#config.token,
				path,
				this.#config.maxInputSize,
				undefined,
				"getOrCreateForKey" in target
					? (target.getOrCreateForKey.poolName ??
						this.#config.poolName)
					: undefined,
				options,
			);
		}

		if ("create" in target) {
			throw new Error(
				"Gateway URLs only support direct actor IDs, get, and getOrCreate targets.",
			);
		}

		throw new Error("unreachable: unknown gateway target type");
	}
}

function canUseDirectSkipReadyWaitPath(path: string): boolean {
	return (
		isActorHttpRequestPath(path) ||
		isPathOrQuery(path, PATH_CONNECT) ||
		isPathOrQuery(path, PATH_WEBSOCKET_BASE) ||
		path.startsWith(PATH_WEBSOCKET_PREFIX)
	);
}

function isPathOrQuery(path: string, basePath: string): boolean {
	return path === basePath || path.startsWith(`${basePath}?`);
}

function isActorHttpRequestPath(path: string): boolean {
	const stripped = path.slice("/request".length);
	return (
		path.startsWith("/request") &&
		(stripped.length === 0 ||
			stripped.startsWith("/") ||
			stripped.startsWith("?"))
	);
}

function directActorIdFromTarget(target: GatewayTarget): string | undefined {
	if ("directId" in target) {
		return target.directId;
	}
	if ("getForId" in target) {
		return target.getForId.actorId;
	}
	return undefined;
}

function requestPath(req: Request): string {
	const url = new URL(req.url);
	return `${url.pathname}${url.search}`;
}

function apiActorToOutput(actor: ApiActor): ActorOutput {
	return {
		actorId: actor.actor_id,
		name: actor.name,
		key: deserializeActorKey(actor.key),
		createTs: actor.create_ts,
		startTs: actor.start_ts ?? null,
		connectableTs: actor.connectable_ts ?? null,
		sleepTs: actor.sleep_ts ?? null,
		destroyTs: actor.destroy_ts ?? null,
		error: actor.error ?? undefined,
	};
}
