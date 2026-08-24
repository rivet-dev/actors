import type { Hono, Context as HonoContext } from "hono";
import type { ActorKey, Encoding, UniversalWebSocket } from "@/actor/mod";
import type { ActorQuery, CrashPolicy } from "@/client/query";
import type { RegistryConfig } from "@/registry/config";
import type { GetUpgradeWebSocket } from "@/utils";

export type GatewayTarget = { directId: string } | ActorQuery;

export interface GatewayRequestOptions {
	skipReadyWait?: boolean;
}

export function shouldSkipReadyWait(
	options: GatewayRequestOptions = {},
): boolean {
	return options.skipReadyWait === true;
}

export interface EngineControlClient {
	getForId(input: GetForIdInput): Promise<ActorOutput | undefined>;
	getWithKey(input: GetWithKeyInput): Promise<ActorOutput | undefined>;
	getOrCreateWithKey(input: GetOrCreateWithKeyInput): Promise<ActorOutput>;
	createActor(input: CreateInput): Promise<ActorOutput>;
	listActors(input: ListActorsInput): Promise<ActorOutput[]>;

	sendRequest(
		target: GatewayTarget,
		actorRequest: Request,
		options?: GatewayRequestOptions,
	): Promise<Response>;
	openWebSocket(
		path: string,
		target: GatewayTarget,
		encoding: Encoding,
		params: unknown,
		options?: GatewayRequestOptions,
	): Promise<UniversalWebSocket>;
	proxyRequest(
		c: HonoContext,
		actorRequest: Request,
		actorId: string,
	): Promise<Response>;
	proxyWebSocket(
		c: HonoContext,
		path: string,
		actorId: string,
		encoding: Encoding,
		params: unknown,
	): Promise<Response>;
	buildGatewayUrl(
		target: GatewayTarget,
		options?: GatewayRequestOptions,
	): Promise<string>;
	displayInformation(): RuntimeDisplayInformation;
	extraStartupLog?: () => Record<string, unknown>;
	modifyRuntimeRouter?: (config: RegistryConfig, router: Hono) => void;
	setGetUpgradeWebSocket(getUpgradeWebSocket: GetUpgradeWebSocket): void;
	shutdown?(): void;

	/**
	 * Test-only helper that simulates an abrupt actor crash.
	 */
	hardCrashActor?(actorId: string): Promise<void>;
}

export interface RuntimeDisplayInformation {
	properties: Record<string, string>;
}

export interface GetForIdInput {
	c?: HonoContext | undefined;
	name: string;
	actorId: string;
}

export interface GetWithKeyInput {
	c?: HonoContext | undefined;
	name: string;
	key: ActorKey;
}

export interface GetOrCreateWithKeyInput {
	c?: HonoContext | undefined;
	name: string;
	key: ActorKey;
	input?: unknown;
	region?: string;
	crashPolicy?: CrashPolicy;
	/** Overrides the client's configured pool name for this actor. */
	poolName?: string;
}

export interface CreateInput {
	c?: HonoContext | undefined;
	name: string;
	key: ActorKey;
	input?: unknown;
	region?: string;
	crashPolicy?: CrashPolicy;
	/** Overrides the client's configured pool name for this actor. */
	poolName?: string;
}

export interface ListActorsInput {
	c?: HonoContext | undefined;
	name: string;
	key?: string;
	includeDestroyed?: boolean;
}

export interface ActorOutput {
	actorId: string;
	name: string;
	key: ActorKey;
	createTs?: number;
	startTs?: number | null;
	connectableTs?: number | null;
	sleepTs?: number | null;
	destroyTs?: number | null;
	error?: unknown;
}
