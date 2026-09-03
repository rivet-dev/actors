import { z } from "zod";

export const ChangelogItem = z.object({
	published: z.string(),
	images: z.array(
		z.object({ url: z.string(), width: z.number(), height: z.number() }),
	),
	title: z.string(),
	description: z.string(),
	slug: z.string(),
	authors: z.array(
		z.object({
			name: z.string(),
			role: z.string(),
			avatar: z.object({ url: z.string() }),
			socials: z
				.object({
					twitter: z.string().optional(),
					github: z.string().optional(),
					bluesky: z.string().optional(),
				})
				.optional(),
		}),
	),
});
export const Changelog = z.array(ChangelogItem);

export type Changelog = z.infer<typeof Changelog>;
export type ChangelogItem = z.infer<typeof ChangelogItem>;

export type RivetActorError =
	| string
	| null
	| object
	| { runner_id: string }
	| { serverless_http_error: unknown }
	| { serverless_connection_error: unknown }
	| { serverless_destination_blocked: unknown }
	| { serverless_invalid_sse_payload: unknown };
