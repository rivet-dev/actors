import { defineCollection, z } from 'astro:content';
import { glob } from 'astro/loaders';

const docs = defineCollection({
	loader: glob({ pattern: '**/*.mdx', base: './src/content/docs' }),
	schema: z.object({
		title: z.string(),
		description: z.string(),
		skill: z.boolean(),
	}),
});

const cookbook = defineCollection({
	loader: glob({ pattern: '**/*.mdx', base: './src/content/cookbook' }),
	schema: z.object({
		title: z.string(),
		description: z.string(),
		icon: z.string().optional(),
		templates: z.array(z.string()).optional(),
	}),
});

const guides = defineCollection({
	loader: glob({ pattern: '**/*.mdx', base: './src/content/guides' }),
	schema: z.object({
		title: z.string(),
		description: z.string(),
	}),
});

const learn = defineCollection({
	loader: glob({ pattern: '**/*.mdx', base: './src/content/learn' }),
	schema: z.object({
		title: z.string(),
		description: z.string(),
		act: z.string().optional(),
		subtitle: z.string().optional(),
	}),
});

const posts = defineCollection({
	loader: glob({ pattern: '**/page.mdx', base: './src/content/posts' }),
	schema: z.object({
		title: z.string(),
		description: z.string(),
		author: z.enum(['nathan-flurry', 'nicholas-kissel', 'forest-anderson', 'andrew-theberge']),
		published: z.coerce.date(),
		category: z.enum(['changelog', 'monthly-update', 'launch-week', 'technical', 'guide', 'frogs']),
		keywords: z.array(z.string()).optional(),
		// Hero image. The URL is derived from the post slug and the file is named
		// `image.{format}` in R2, so frontmatter only signals presence and format.
		// Use `image: true` for the default `image.png`, or `image: { format: "gif" }`
		// for a different extension. `file` overrides the filename for legacy assets
		// that are not named `image.*` (still resolved relative to the post slug).
		// Resolved via `getPostImage` in `@/lib/postImage`.
		image: z.union([
			z.boolean(),
			z.object({ format: z.string().optional(), file: z.string().optional() }),
		]).optional(),
		unpublished: z.boolean().optional(),
	}),
});

export const collections = {
	docs,
	cookbook,
	guides,
	learn,
	posts,
};
