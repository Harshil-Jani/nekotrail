import { memo, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";

const SAFE_LINK = /^(https?:\/\/|mailto:)/i;
/** Hosts allowed by img-src in tauri.conf.json. */
const SAFE_IMAGE = /^https:\/\/(github\.com|avatars\.githubusercontent\.com|user-images\.githubusercontent\.com|private-user-images\.githubusercontent\.com)\//i;

let mermaidReady: Promise<typeof import("mermaid").default> | null = null;
/** Mermaid is big, so it loads the first time a chat contains a diagram. */
function loadMermaid() {
  mermaidReady ??= import("mermaid").then((m) => {
    const dark = matchMedia("(prefers-color-scheme: dark)").matches;
    // Plain SVG text labels: measured reliably everywhere (HTML labels can collapse to nothing).
    m.default.initialize({
      startOnLoad: false, securityLevel: "strict", theme: dark ? "dark" : "neutral", htmlLabels: false,
      flowchart: { htmlLabels: false, useMaxWidth: true, curve: "basis" }, fontFamily: "-apple-system, BlinkMacSystemFont, Helvetica, sans-serif", fontSize: 12,
      themeVariables: dark
        ? { primaryColor: "#2c2c2e", primaryBorderColor: "#4a4a4c", primaryTextColor: "#e8e8ea", lineColor: "#6b6b70", fontSize: "12px" }
        : { primaryColor: "#ffffff", primaryBorderColor: "#c7c7cc", primaryTextColor: "#1d1d1f", lineColor: "#8e8e93", fontSize: "12px" },
    });
    return m.default;
  });
  return mermaidReady;
}

function Mermaid({ code }: { code: string }) {
  const id = useId().replace(/[^a-zA-Z0-9]/g, "");
  const [svg, setSvg] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    loadMermaid()
      .then((m) => m.render(`mmd${id}`, code))
      .then(({ svg }) => { if (live) setSvg(svg); })
      .catch(() => { if (live) setFailed(true); });
    return () => { live = false; };
  }, [code, id]);
  // Fit the frame to what was actually drawn, so nothing gets cropped.
  const box = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = box.current?.querySelector("svg");
    const g = el?.querySelector("g");
    if (!el || !g) return;
    const b = (g as SVGGraphicsElement).getBBox();
    if (!b.width || !b.height) return;
    const pad = 12;
    el.setAttribute("viewBox", `${b.x - pad} ${b.y - pad} ${b.width + pad * 2} ${b.height + pad * 2}`);
    el.removeAttribute("height");
    el.style.maxWidth = `${Math.min(b.width + pad * 2, 900)}px`;
    el.style.width = "100%";
  }, [svg]);
  if (failed) return <pre><code>{code}</code></pre>;
  if (!svg) return <div className="mermaid-loading muted"><span className="spinner" /> Drawing the diagram…</div>;
  // Mermaid renders with securityLevel "strict": no scripts or links in the SVG.
  return <div className="mermaid-diagram" ref={box} dangerouslySetInnerHTML={{ __html: svg }} />;
}

/** Markdown as the agents write it: GitHub flavour, and ```mermaid blocks drawn as diagrams. */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        code({ className, children, ...rest }) {
          if (/language-mermaid/.test(className ?? "")) return <Mermaid code={String(children).trim()} />;
          return <code className={className} {...rest}>{children}</code>;
        },
        // Agents write what a repo or an issue told them to, so a link must never take over the
        // app window: web links open in your browser, anything else is plain text.
        a({ href, children }) {
          if (!href || !SAFE_LINK.test(href)) return <span>{children}</span>;
          return <a href={href} title={href} onClick={(e) => { e.preventDefault(); openUrl(href).catch(() => {}); }}>{children}</a>;
        },
        // Loading an image is a request the agent chose, which can carry data out in its URL.
        // Only GitHub-hosted images load (the CSP enforces the same); others show as a link.
        img({ src, alt }) {
          const url = typeof src === "string" ? src : "";
          if (SAFE_IMAGE.test(url)) return <img src={url} alt={alt ?? ""} />;
          return SAFE_LINK.test(url) ? <a href={url} onClick={(e) => { e.preventDefault(); openUrl(url).catch(() => {}); }}>{alt || "image"}</a> : <span>{alt}</span>;
        },
      }}
    >
      {text}
    </ReactMarkdown>
  );
});
