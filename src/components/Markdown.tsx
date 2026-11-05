import { memo, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

let mermaidReady: Promise<typeof import("mermaid").default> | null = null;
/** Mermaid is big, so it loads the first time a chat contains a diagram. */
function loadMermaid() {
  mermaidReady ??= import("mermaid").then((m) => {
    const dark = matchMedia("(prefers-color-scheme: dark)").matches;
    // Plain SVG text labels: measured reliably everywhere (HTML labels can collapse to nothing).
    m.default.initialize({
      startOnLoad: false, securityLevel: "strict", theme: dark ? "dark" : "neutral", htmlLabels: false,
      flowchart: { htmlLabels: false, useMaxWidth: true }, fontFamily: "-apple-system, BlinkMacSystemFont, Helvetica, sans-serif", fontSize: 13,
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
      }}
    >
      {text}
    </ReactMarkdown>
  );
});
