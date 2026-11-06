/** Markdown to safe, static HTML for the exported report (agent answers and the plan). */
import { createElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { safeUrl } from "./sessionReport";

/**
 * react-markdown never passes raw HTML through (a `<script>` in a chat comes out as text). On top of
 * that, images become their alt text so the file makes no request when opened, and only web links stay links.
 */
export function mdToHtml(markdown: string): string {
  return renderToStaticMarkup(
    createElement(ReactMarkdown, {
      remarkPlugins: [remarkGfm],
      components: {
        img: ({ alt }: { alt?: string }) => createElement("span", null, alt ? `[image: ${alt}]` : "[image]"),
        a: ({ href, children }: { href?: string; children?: ReactNode }) => {
          const url = safeUrl(href);
          return url ? createElement("a", { href: url, rel: "noopener noreferrer" }, children) : createElement("span", null, children);
        },
      },
    }, markdown),
  );
}
