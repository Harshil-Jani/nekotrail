import rss from "@astrojs/rss";
import { getCollection } from "astro:content";
import type { APIContext } from "astro";
import { absolute } from "../lib/site";

export async function GET(context: APIContext) {
  const posts = (await getCollection("blog", ({ data }) => !data.draft)).sort((a, b) => +b.data.date - +a.data.date);
  return rss({
    title: "NekoTrail blog",
    description: "Notes on building NekoTrail and on running coding agents in parallel.",
    site: context.site ?? absolute(),
    items: posts.map((p) => ({
      title: p.data.title,
      description: p.data.description,
      pubDate: p.data.date,
      author: p.data.author,
      link: absolute(`blog/${p.id}/`),
    })),
    customData: "<language>en</language>",
  });
}
