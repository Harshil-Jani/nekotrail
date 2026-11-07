import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { z } from "astro/zod";

const blog = defineCollection({
  loader: glob({ pattern: "**/*.md", base: "./src/content/blog" }),
  schema: z.object({
    title: z.string(),
    description: z.string(),
    date: z.coerce.date(),
    author: z.string().default("Harshil Jani"),
    draft: z.boolean().default(false),
    /** A clip from public/clips/ shown under the title. */
    clip: z.string().optional(),
    clipLabel: z.string().optional(),
    /** Share image in public/ (1200x630). Defaults to og/blog.png. */
    image: z.string().optional(),
    /** Canonical path under the base when another page is the main one for this topic. */
    canonical: z.string().optional(),
  }),
});

export const collections = { blog };
