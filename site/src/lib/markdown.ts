// Markdown from the repo (CHANGELOG.md, docs/*.md) rendered at build time, with links that point at
// this site's pages where one exists and at GitHub otherwise.
import { Marked } from "marked";
import { SITE, href } from "./site";

export function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/<[^>]+>/g, "")
    .replace(/&#39;|&apos;|&quot;|&amp;/g, "")
    .replace(/[`*_]/g, "")
    .replace(/[^a-z0-9\s-]/g, "")
    .trim()
    .replace(/\s+/g, "-");
}

/** Where a link inside a repo markdown file should go on the site. `from` is the file's folder in the repo. */
export function rewriteLink(link: string, from = ""): string {
  if (/^(https?:|mailto:)/.test(link) || link.startsWith("#")) return link;
  const [rawPath, hash = ""] = link.split("#");
  const parts = (from ? from.split("/") : []).filter(Boolean);
  for (const seg of rawPath.split("/")) {
    if (seg === "..") parts.pop();
    else if (seg && seg !== ".") parts.push(seg);
  }
  const p = parts.join("/");
  if (p === "docs/MOVING-FROM-CONDUCTOR.md") return href("moving-from-conductor/") + (hash ? `#${hash}` : "");
  if (p === "CHANGELOG.md") return href("changelog/") + (hash ? `#${hash}` : "");
  if (p === "README.md" && hash === "install") return href("download/");
  return `${SITE.repo}/blob/main/${p}${hash ? `#${hash}` : ""}`;
}

export function renderMarkdown(md: string, from = ""): string {
  const marked = new Marked({ gfm: true });
  marked.use({
    renderer: {
      heading({ tokens, depth }) {
        const text = this.parser.parseInline(tokens);
        return `<h${depth} id="${slugify(text)}">${text}</h${depth}>\n`;
      },
      link({ href: target, title, tokens }) {
        const text = this.parser.parseInline(tokens);
        const url = rewriteLink(target, from);
        const external = /^https?:/.test(url) && !url.startsWith(SITE.origin);
        return `<a href="${url}"${title ? ` title="${title}"` : ""}${external ? ' rel="noopener"' : ""}>${text}</a>`;
      },
      table(token) {
        // Wide tables scroll inside their own box on small screens.
        const head = token.header.map((c) => `<th>${this.parser.parseInline(c.tokens)}</th>`).join("");
        const body = token.rows.map((r) => `<tr>${r.map((c) => `<td>${this.parser.parseInline(c.tokens)}</td>`).join("")}</tr>`).join("\n");
        return `<div class="table-wrap"><table><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table></div>\n`;
      },
    },
  });
  return marked.parse(md, { async: false }) as string;
}

export type Release = {
  id: string;
  version: string;
  date: string | null;
  titles: string[];
  html: string;
};

/** CHANGELOG.md split into releases, newest first. Bold lines become the release's section headings. */
export function parseChangelog(md: string): Release[] {
  const chunks = md.split(/^## /m).slice(1);
  return chunks.map((chunk) => {
    const nl = chunk.indexOf("\n");
    const head = chunk.slice(0, nl).trim();
    const body = chunk.slice(nl + 1);
    const m = head.match(/^([\w.-]+)\s*(?:\((\d{4}-\d{2}-\d{2})\))?/);
    const version = m?.[1] ?? head;
    const date = m?.[2] ?? null;
    const titles: string[] = [];
    const withHeadings = body.replace(/^\*\*(.+?)\*\*\s*$/gm, (_all, t: string) => {
      titles.push(t);
      return `### ${t}`;
    });
    const id = version.toLowerCase() === "unreleased" ? "unreleased" : `v${version.replace(/\./g, "-")}`;
    return { id, version, date, titles, html: renderMarkdown(withHeadings) };
  });
}
