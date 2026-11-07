import { memo, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useDark } from "../appearance";

const SAFE_LINK = /^(https?:\/\/|mailto:)/i;
/** Hosts allowed by img-src in tauri.conf.json. */
const SAFE_IMAGE = /^https:\/\/(github\.com|avatars\.githubusercontent\.com|user-images\.githubusercontent\.com|private-user-images\.githubusercontent\.com)\//i;

let mermaidReady: Promise<typeof import("mermaid").default> | null = null;
let mermaidDark: boolean | null = null;
/** Mermaid is big, so it loads the first time a chat contains a diagram. */
function loadMermaid(dark: boolean) {
  mermaidReady ??= import("mermaid").then((m) => m.default);
  return mermaidReady.then((m) => {
    if (mermaidDark === dark) return m;
    mermaidDark = dark;
    // Plain SVG text labels: measured reliably everywhere (HTML labels can collapse to nothing).
    m.initialize({
      // "classic" look: flat shapes with hairlines. The newer look adds drop shadows and gradient strokes.
      startOnLoad: false, securityLevel: "strict", theme: dark ? "dark" : "neutral", htmlLabels: false, look: "classic",
      flowchart: { htmlLabels: false, useMaxWidth: true, curve: "basis" }, fontFamily: "-apple-system, BlinkMacSystemFont, Helvetica, sans-serif", fontSize: 13,
      themeVariables: dark
        ? { primaryColor: "#1f1f45", primaryBorderColor: "#3c3c78", primaryTextColor: "#f0f0ff", lineColor: "#9c9ccc", fontSize: "13px" }
        : { primaryColor: "#ffffff", primaryBorderColor: "#bdbdde", primaryTextColor: "#14143a", lineColor: "#545485", fontSize: "13px" },
    });
    return m;
  });
}

/** One render at a time: Mermaid's theme is global, so a light and a dark render must not interleave. */
let queue: Promise<unknown> = Promise.resolve();
function renderMermaid(id: string, code: string, dark: boolean) {
  const job = queue.then(() => loadMermaid(dark)).then((m) => m.render(id, code));
  queue = job.catch(() => {});
  return job;
}

/** A ```mermaid block: the diagram, with a toggle to read or copy its source. Redraws when light or dark changes. */
function Mermaid({ code }: { code: string }) {
  const id = useId().replace(/[^a-zA-Z0-9]/g, "");
  const dark = useDark();
  const [svg, setSvg] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [source, setSource] = useState(false);
  useEffect(() => {
    let live = true;
    renderMermaid(`mmd${id}${dark ? "d" : "l"}`, code, dark)
      .then(({ svg }) => { if (live) setSvg(svg); })
      .catch(() => { if (live) setFailed(true); });
    return () => { live = false; };
  }, [code, id, dark]);
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
    // Shrink wide diagrams to fit, but not below 85%: past that the labels get too small to read,
    // so the frame scrolls sideways instead.
    el.style.minWidth = `${Math.min(b.width + pad * 2, 900) * 0.85}px`;
  }, [svg, source]);
  if (failed) return <pre><code>{code}</code></pre>;
  return (
    <div className="mermaid-block">
      <div className="seg mermaid-toggle" role="radiogroup" aria-label="Show the diagram or its source">
        <button role="radio" aria-checked={!source} className={source ? "" : "on"} onClick={() => setSource(false)}>Diagram</button>
        <button role="radio" aria-checked={source} className={source ? "on" : ""} onClick={() => setSource(true)}>Source</button>
      </div>
      {source ? (
        <pre><code>{code}</code></pre>
      ) : !svg ? (
        <div className="mermaid-loading muted"><span className="spinner" /> Drawing the diagram…</div>
      ) : (
        // Mermaid renders with securityLevel "strict": no scripts or links in the SVG.
        <div className="mermaid-diagram" ref={box} dangerouslySetInnerHTML={{ __html: svg }} />
      )}
    </div>
  );
}

/** Markdown as the agents write it: GitHub flavour, and ```mermaid blocks drawn as diagrams. */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        // A diagram is not a code block: skip the <pre> so it gets the app font and its own frame.
        pre({ node, children, ...rest }) {
          const first = node?.children?.[0];
          const cls = first?.type === "element" ? String(first.properties?.className ?? "") : "";
          if (/language-mermaid/.test(cls)) return <>{children}</>;
          return <pre {...rest}>{children}</pre>;
        },
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
