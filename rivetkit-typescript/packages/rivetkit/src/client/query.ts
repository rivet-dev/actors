import { z } from "zod/v4";
import {
	HEADER_ACTOR_ID,
	HEADER_ACTOR_QUERY,
	HEADER_CONN_PARAMS,
	HEADER_ENCODING,
} from "@/common/actor-router-consts";
import { EncodingSchema } from "@/common/encoding";

// Maximum size of a key component in bytes
// Set to 128 bytes to allow for separators and escape characters in the full key
// Cloudflare's maximum key size is 512 bytes, so we need to be significantly smaller
export const MAX_ACTOR_KEY_SIZE = 128;

export const ActorKeySchema = z.array(z.string().max(MAX_ACTOR_KEY_SIZE));

export type ActorKey = z.infer<typeof ActorKeySchema>;

/**
 * Crash policy for actor lifecycle management.
 *
 * This schema is only used by the engine driver for actor creation. The manager
 * driver ignores crash policy and passes it through to the engine unchanged.
 */
export const CrashPolicySchema = z.enum(["restart", "sleep", "destroy"]);

export type CrashPolicy = z.infer<typeof CrashPolicySchema>;

export const CreateRequestSchema = z.object({
	name: z.string(),
	key: ActorKeySchema,
	input: z.unknown().optional(),
	region: z.string().optional(),
	poolName: z.string().optional(),
});

export const GetForKeyRequestSchema = z.object({
	name: z.string(),
	key: ActorKeySchema,
});

export const GetOrCreateRequestSchema = z.object({
	name: z.string(),
	key: ActorKeySchema,
	input: z.unknown().optional(),
	region: z.string().optional(),
	poolName: z.string().optional(),
});

export const ActorQuerySchema = z.union([
	z.object({
		getForId: z.object({
			name: z.string(),
			actorId: z.string(),
		}),
	}),
	z.object({
		getForKey: GetForKeyRequestSchema,
	}),
	z.object({
		getOrCreateForKey: GetOrCreateRequestSchema,
	}),
	z.object({
		create: CreateRequestSchema,
	}),
]);

export const ConnectRequestSchema = z.object({
	query: ActorQuerySchema.describe(HEADER_ACTOR_QUERY),
	encoding: EncodingSchema.describe(HEADER_ENCODING),
	connParams: z.string().optional().describe(HEADER_CONN_PARAMS),
});

export const ConnectWebSocketRequestSchema = z.object({
	query: ActorQuerySchema.describe("query"),
	encoding: EncodingSchema.describe("encoding"),
	connParams: z.unknown().optional().describe("conn_params"),
});

export const ConnMessageRequestSchema = z.object({
	actorId: z.string().describe(HEADER_ACTOR_ID),
	encoding: EncodingSchema.describe(HEADER_ENCODING),
});

export const ResolveRequestSchema = z.object({
	query: ActorQuerySchema.describe(HEADER_ACTOR_QUERY),
	connParams: z.string().optional().describe(HEADER_CONN_PARAMS),
});

export type ActorQuery = z.infer<typeof ActorQuerySchema>;
export type ActorGatewayQuery = Extract<
	ActorQuery,
	{ getForKey: unknown } | { getOrCreateForKey: unknown }
>;
export type GetForKeyRequest = z.infer<typeof GetForKeyRequestSchema>;
export type GetOrCreateRequest = z.infer<typeof GetOrCreateRequestSchema>;
export type ConnectQuery = z.infer<typeof ConnectRequestSchema>;
/**
 * Interface representing a request to create a actor.
 */
export type CreateRequest = z.infer<typeof CreateRequestSchema>;
