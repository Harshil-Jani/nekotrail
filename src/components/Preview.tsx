import { useEffect, useRef, useState } from "react";
import { ExternalLink, MoveUpRight, PenLine, Play, RectangleHorizontal, RotateCw, Settings2, TriangleAlert, Undo2 } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, errText, type PortProblem } from "../api";
import { snapshotScale } from "../appearance";
import { keyHint, mod } from "../platform";

/**
 * Turn what someone typed into a local address, or null. Only localhost and 127.0.0.1 are allowed:
 * the preview must never load the open web inside the app. "3000" and ":3000" mean localhost:3000.
 * The frame-src CSP in index.html enforces the same rule for links clicked inside the page.
 */
export function toLocalUrl(input: string): string | null {
  let s = input.trim();
  if (!s) return null;
  if (/^:?\d{2,5}(\/.*)?$/.test(s)) s = `localhost:${s.replace(/^:/, "")}`;
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(s)) s = `http://${s}`;
  let u: URL;
  try { u = new URL(s); } catch { return null; }
  if (u.protocol !== "http:" && u.protocol !== "https:") return null;
  if (u.username || u.password) return null;
  if (u.hostname === "0.0.0.0" || u.hostname === "[::1]") u.hostname = "localhost";
  if (u.hostname !== "localhost" && u.hostname !== "127.0.0.1") return null;
  return u.href;
}

const short = (url: string) => url.replace(/^https?:\/\//, "").replace(/\/$/, "");

type Probe = { up: boolean; blocked: string | null } | "unknown";
type Shape = { kind: "box" | "arrow"; x1: number; y1: number; x2: number; y2: number };
type Shot = { canvas: HTMLCanvasElement; src: string; w: number; h: number; scale: number };

const STROKE = "#ffd27a"; // lantern gold, the colour for "look here"; the dark halo keeps it readable on white pages
const HALO = "rgba(10, 10, 28, 0.55)";

/** Arrowhead points for a line ending at (x2, y2). */
function head(s: Shape, len: number): [number, number, number, number, number, number] {
  const a = Math.atan2(s.y2 - s.y1, s.x2 - s.x1);
  const l = (d: number) => [s.x2 - len * Math.cos(a + d), s.y2 - len * Math.sin(a + d)];
  const [ax, ay] = l(0.5);
  const [bx, by] = l(-0.5);
  return [ax, ay, s.x2, s.y2, bx, by];
}

function box(s: Shape) {
  return { x: Math.min(s.x1, s.x2), y: Math.min(s.y1, s.y2), w: Math.abs(s.x2 - s.x1), h: Math.abs(s.y2 - s.y1) };
}

function ShapeSvg({ s }: { s: Shape }) {
  const parts = (stroke: string, width: number) =>
    s.kind === "box" ? (
      <rect {...(({ x, y, w, h }) => ({ x, y, width: w, height: h }))(box(s))} rx={4} fill="none" stroke={stroke} strokeWidth={width} />
    ) : (
      <>
        <line x1={s.x1} y1={s.y1} x2={s.x2} y2={s.y2} stroke={stroke} strokeWidth={width} strokeLinecap="round" />
        <polyline points={head(s, 14).join(",")} fill="none" stroke={stroke} strokeWidth={width} strokeLinecap="round" strokeLinejoin="round" />
      </>
    );
  return <g>{parts(HALO, 6)}{parts(STROKE, 3)}</g>;
}

/** Burn the marks into a copy of the screenshot at full resolution. */
function compose(shot: Shot, shapes: Shape[]): Promise<Blob | null> {
  const c = document.createElement("canvas");
  c.width = shot.canvas.width;
  c.height = shot.canvas.height;
  const g = c.getContext("2d")!;
  g.drawImage(shot.canvas, 0, 0);
  g.scale(shot.scale, shot.scale);
  g.lineCap = "round";
  g.lineJoin = "round";
  for (const [stroke, width] of [[HALO, 6], [STROKE, 3]] as const) {
    g.strokeStyle = stroke;
    g.lineWidth = width;
    for (const s of shapes) {
      g.beginPath();
      if (s.kind === "box") {
        const { x, y, w, h } = box(s);
        g.roundRect(x, y, w, h, 4);
      } else {
        const [ax, ay, hx, hy, bx, by] = head(s, 14);
        g.moveTo(s.x1, s.y1);
        g.lineTo(s.x2, s.y2);
        g.moveTo(ax, ay);
        g.lineTo(hx, hy);
        g.lineTo(bx, by);
      }
      g.stroke();
    }
  }
  return new Promise((r) => c.toBlob(r, "image/png"));
}

export function Preview({
  workspaceId,
  detected,
  running,
  active,
  onRun,
  onSetup,
  onAttach,
  onSay,
  show,
}: {
  workspaceId: string;
  /** Local URL spotted in the Run script's output, if any. */
  detected?: string;
  /** The Run script is alive. */
  running: boolean;
  /** The Preview tab is the one showing (polls only then). */
  active: boolean;
  onRun: () => void;
  /** Open the repo's settings, where the Run script is set. */
  onSetup: () => void;
  /** Text for the chat composer, with the saved screenshot's @path. */
  onAttach: (text: string) => void;
  onSay: (text: string, error?: boolean) => void;
  /** Show this address now (from the Ports popover). */
  show?: { url: string; nonce: number };
}) {
  // Until the Run script prints an address, the Preview opens the workspace's own port.
  const [own, setOwn] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    api.workspacePorts(workspaceId).then((p) => { if (live && p) setOwn(`http://localhost:${p.main}/`); }).catch(() => {});
    return () => { live = false; };
  }, [workspaceId]);
  const detectedUrl = detected ? toLocalUrl(detected) : null;
  const auto = detectedUrl ?? own;
  const [manual, setManual] = useState<string | null>(null);
  const src = manual ?? auto;
  /** Showing the workspace's own port only because nothing else is known yet. */
  const onOwn = !manual && !detectedUrl && !!own;
  const [field, setField] = useState(src ?? "");
  const [fieldError, setFieldError] = useState(false);
  const [probe, setProbe] = useState<Probe | null>(null);
  const [reload, setReload] = useState(0);
  const [loading, setLoading] = useState(false);
  const frame = useRef<HTMLIFrameElement>(null);
  const fieldRef = useRef<HTMLInputElement>(null);

  useEffect(() => { setField(src ?? ""); setFieldError(false); }, [src]);
  useEffect(() => {
    if (!show) return;
    const u = toLocalUrl(show.url);
    if (u) { setManual(u === auto ? null : u); setReload((n) => n + 1); }
  }, [show?.nonce]);

  // Without a Run script there is nothing to start: say how to set one instead of offering Start.
  const [hasRun, setHasRun] = useState<boolean | null>(null);
  useEffect(() => {
    if (!active) return;
    let live = true;
    api.workspaceScripts(workspaceId).then((s) => { if (live) setHasRun(!!s.run); }).catch(() => { if (live) setHasRun(null); });
    return () => { live = false; };
  }, [workspaceId, active]);

  // Is anything answering? While it isn't, check again every 2 seconds (dev servers take a moment).
  useEffect(() => {
    if (!src) { setProbe(null); return; }
    let stop = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const check = async () => {
      let p: Probe;
      try {
        const r = await api.previewProbe(src);
        p = r ? { up: r.up, blocked: r.blocked } : "unknown"; // no backend (browser preview): just show it
      } catch {
        p = "unknown";
      }
      if (stop) return;
      setProbe(p);
      if (p !== "unknown" && !p.up && active) timer = setTimeout(check, 2000);
    };
    check();
    return () => { stop = true; clearTimeout(timer); };
  }, [src, reload, active]);

  const showing = !!src && probe !== null && (probe === "unknown" || (probe.up && !probe.blocked));
  // Nothing answers on the workspace's port: nothing started yet, the same as having no address.
  const waiting = !src || (onOwn && probe !== null && probe !== "unknown" && !probe.up);

  // While the app runs but its port stays quiet, ask whether it went somewhere else (it ignored
  // PORT, or its default was taken by another workspace), and say so with the fix.
  const [problem, setProblem] = useState<PortProblem | null>(null);
  useEffect(() => {
    if (!active || !running || !waiting) { setProblem(null); return; }
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const look = () =>
      api.portsStatus(workspaceId)
        .then((l) => { if (live) { const ps = l[0]?.problems ?? []; setProblem(ps[ps.length - 1] ?? null); } })
        .catch(() => {})
        .finally(() => { if (live) timer = setTimeout(look, 4000); });
    timer = setTimeout(look, 1500);
    return () => { live = false; clearTimeout(timer); };
  }, [active, running, waiting, workspaceId]);
  useEffect(() => { if (showing) setLoading(true); }, [showing, src, reload]);

  const go = () => {
    const u = toLocalUrl(field);
    if (!u) { setFieldError(true); return; }
    setManual(u === auto ? null : u);
    setReload((n) => n + 1);
  };
  const openOutside = () => { if (src) openUrl(src).catch((e) => onSay(errText(e), true)); };

  // ---------- Annotate: freeze, draw, note, attach ----------
  const [shot, setShot] = useState<Shot | null>(null);
  const [shapes, setShapes] = useState<Shape[]>([]);
  const [draft, setDraft] = useState<Shape | null>(null);
  const [tool, setTool] = useState<"box" | "arrow">("box");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const svg = useRef<SVGSVGElement>(null);

  const freeze = async () => {
    const el = frame.current;
    if (!el || busy) return;
    setBusy(true);
    try {
      const buf = await api.previewSnapshot();
      if (!buf) throw new Error("Annotate works in the NekoTrail app, not in a browser.");
      const bmp = await createImageBitmap(new Blob([buf], { type: "image/png" }));
      // The snapshot is the whole window at screen pixels; keep just the preview. Measure how many
      // snapshot pixels one layout pixel is, so the crop stays right at any app zoom (80% to 150%).
      const r = el.getBoundingClientRect();
      const scale = snapshotScale(bmp.width, document.documentElement.getBoundingClientRect().width);
      const canvas = document.createElement("canvas");
      canvas.width = Math.round(r.width * scale);
      canvas.height = Math.round(r.height * scale);
      canvas.getContext("2d")!.drawImage(bmp, r.left * scale, r.top * scale, r.width * scale, r.height * scale, 0, 0, canvas.width, canvas.height);
      setShot({ canvas, src: canvas.toDataURL("image/png"), w: r.width, h: r.height, scale });
      setShapes([]);
      setNote("");
    } catch (e) {
      onSay(`Couldn't take the screenshot. ${errText(e)}`, true);
    } finally {
      setBusy(false);
    }
  };
  const cancel = () => { setShot(null); setShapes([]); setDraft(null); setNote(""); };
  const attach = async () => {
    if (!shot || busy) return;
    setBusy(true);
    try {
      const blob = await compose(shot, shapes);
      if (!blob) throw new Error("The image came out empty.");
      const where = src ? short(src) : "the preview";
      const name = `preview-${where.replace(/[^a-z0-9]+/gi, "-").replace(/-+$/, "")}.png`;
      const rel = await api.saveAttachment(workspaceId, new File([blob], name, { type: "image/png" }));
      const text = note.trim()
        ? `@${rel} In the preview (${where}): ${note.trim()}`
        : `@${rel} In the preview (${where}), see what I marked.`;
      onAttach(text);
      cancel();
    } catch (e) {
      onSay(`Couldn't attach the screenshot. ${errText(e)}`, true);
    } finally {
      setBusy(false);
    }
  };

  // Esc leaves annotate, ⌘Z takes back the last mark.
  useEffect(() => {
    if (!shot || !active) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); cancel(); }
      else if (mod(e) && !e.shiftKey && e.key.toLowerCase() === "z" && !(e.target as HTMLElement).closest("input, textarea")) {
        e.preventDefault();
        setShapes((s) => s.slice(0, -1));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [shot, active]);

  const point = (e: React.PointerEvent) => {
    const r = svg.current!.getBoundingClientRect();
    return {
      x: Math.max(0, Math.min(shot!.w, ((e.clientX - r.left) * shot!.w) / r.width)),
      y: Math.max(0, Math.min(shot!.h, ((e.clientY - r.top) * shot!.h) / r.height)),
    };
  };
  const down = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    (e.target as Element).setPointerCapture(e.pointerId);
    const p = point(e);
    setDraft({ kind: tool, x1: p.x, y1: p.y, x2: p.x, y2: p.y });
  };
  const move = (e: React.PointerEvent) => {
    if (!draft) return;
    const p = point(e);
    setDraft({ ...draft, x2: p.x, y2: p.y });
  };
  const up = () => {
    if (draft && Math.hypot(draft.x2 - draft.x1, draft.y2 - draft.y1) > 6) setShapes((s) => [...s, draft]);
    setDraft(null);
  };

  return (
    <div className="preview">
      {shot ? (
        <div className="preview-bar annotating" role="toolbar" aria-label="Annotate">
          <div className="seg" role="radiogroup" aria-label="Mark with">
            <button role="radio" aria-checked={tool === "box"} className={tool === "box" ? "on" : ""} onClick={() => setTool("box")}><RectangleHorizontal size={14} />Box</button>
            <button role="radio" aria-checked={tool === "arrow"} className={tool === "arrow" ? "on" : ""} onClick={() => setTool("arrow")}><MoveUpRight size={14} />Arrow</button>
          </div>
          <button className="icon" aria-label="Undo the last mark" title={keyHint("Undo (⌘Z)")} disabled={!shapes.length} onClick={() => setShapes((s) => s.slice(0, -1))}><Undo2 size={15} /></button>
          <input
            className="preview-note"
            autoFocus
            aria-label="Note for the agent"
            placeholder="What should change here?"
            value={note}
            onChange={(e) => setNote(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing) { e.preventDefault(); attach(); } }}
          />
          <button onClick={cancel}>Cancel</button>
          <button className="primary" onClick={attach} disabled={busy}>{busy ? <span className="spinner" /> : null}Attach to chat</button>
        </div>
      ) : (
        <form className="preview-bar" role="toolbar" aria-label="Preview" onSubmit={(e) => { e.preventDefault(); go(); }}>
          <button type="button" className="icon" aria-label="Reload" title="Reload" disabled={!src} onClick={() => setReload((n) => n + 1)}>
            {loading && showing ? <span className="spinner" /> : <RotateCw size={15} />}
          </button>
          <input
            ref={fieldRef}
            className={`preview-url ${fieldError ? "invalid" : ""}`}
            aria-label="Local address"
            aria-invalid={fieldError}
            aria-describedby={fieldError ? "preview-url-error" : undefined}
            placeholder="localhost:3000"
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            value={field}
            onChange={(e) => { setField(e.target.value); setFieldError(false); }}
            onKeyDown={(e) => { if (e.key === "Escape") { setField(src ?? ""); setFieldError(false); e.currentTarget.blur(); } }}
          />
          {fieldError && <span id="preview-url-error" className="preview-url-error" role="alert">Only localhost and 127.0.0.1 open here.</span>}
          <button type="button" className="icon" aria-label="Open in browser" title={keyHint("Open in browser (⌘⇧B)")} disabled={!src} onClick={openOutside}><ExternalLink size={15} /></button>
          <button type="button" onClick={freeze} disabled={!showing || busy} title="Freeze the page, mark it up and send it to the agent">
            {busy ? <span className="spinner" /> : <PenLine size={14} />}Annotate
          </button>
        </form>
      )}

      <div className="preview-stage">
        {showing && (
          <iframe
            key={`${src}#${reload}`}
            ref={frame}
            className="preview-frame"
            title={`Preview of ${short(src!)}`}
            src={src!}
            // No top navigation and no popups: the page can't take over the app window or open new ones.
            sandbox="allow-scripts allow-same-origin allow-forms allow-modals allow-downloads"
            referrerPolicy="no-referrer"
            onLoad={() => setLoading(false)}
          />
        )}

        {shot && (
          <div className="preview-shot">
            <img src={shot.src} alt="Frozen screenshot of the preview" draggable={false} />
            <svg
              ref={svg}
              viewBox={`0 0 ${shot.w} ${shot.h}`}
              preserveAspectRatio="none"
              role="img"
              aria-label={`Screenshot with ${shapes.length} mark${shapes.length === 1 ? "" : "s"}. Drag to draw a ${tool}.`}
              onPointerDown={down}
              onPointerMove={move}
              onPointerUp={up}
              onPointerCancel={() => setDraft(null)}
            >
              {shapes.map((s, i) => <ShapeSvg key={i} s={s} />)}
              {draft && <ShapeSvg s={draft} />}
            </svg>
          </div>
        )}

        {!showing && (
          <div className="empty preview-empty">
            {problem ? (
              <>
                <h2>Your app isn't on this workspace's port</h2>
                <div className="preview-problem" role="note">
                  <TriangleAlert size={16} aria-hidden />
                  <div>
                    <p>{problem.text}</p>
                    <p className="muted">{problem.fix.split("`").map((part, i) => (i % 2 ? <code key={i}>{part}</code> : part))}</p>
                  </div>
                </div>
                {problem.port !== null && (
                  <button className="link" onClick={() => { setManual(`http://localhost:${problem.port}/`); setReload((n) => n + 1); }}>Show localhost:{problem.port} anyway</button>
                )}
              </>
            ) : waiting ? (
              running ? (
                <>
                  <span className="spinner" aria-hidden />
                  <h2>Waiting for your app</h2>
                  <p>{own
                    ? <>It shows up here as soon as it answers on {short(own)}, this workspace's port, or the Run script prints a localhost address.</>
                    : "It shows up here as soon as the Run script prints a localhost address. You can also type one above."}</p>
                </>
              ) : hasRun === false ? (
                <>
                  <h2>No Run script for this repo yet</h2>
                  <p>Add the command that starts your dev server, like <code>npm run dev</code>, in the repo's settings. Your app shows up here as soon as it prints a localhost address.</p>
                  <button onClick={onSetup}><Settings2 size={14} />Set a Run script</button>
                  <button className="link" onClick={() => fieldRef.current?.focus()}>Or type a localhost address</button>
                </>
              ) : (
                <>
                  <h2>Nothing running yet</h2>
                  <p>Start the Run script and your app shows up here{own ? <> on {short(own)}, this workspace's port</> : null}, ready to click through and mark up for the agent.</p>
                  <button onClick={onRun}><Play size={14} />Start Run script</button>
                  <button className="link" onClick={() => fieldRef.current?.focus()}>Or type a localhost address</button>
                </>
              )
            ) : probe === null ? (
              <span className="spinner" aria-label="Checking" />
            ) : probe !== "unknown" && probe.blocked ? (
              <>
                <h2>{short(src)} won't show inside another app</h2>
                <p>The page asks browsers not to frame it ({probe.blocked === "x-frame-options" ? "X-Frame-Options" : "CSP frame-ancestors"}). Open it in your browser, or allow framing in development.</p>
                <button onClick={openOutside}><ExternalLink size={14} />Open in browser</button>
              </>
            ) : running ? (
              <>
                <span className="spinner" aria-hidden />
                <h2>Starting {short(src)}</h2>
                <p>Nothing answers yet. This page updates on its own when it does.</p>
              </>
            ) : (
              <>
                <h2>Nothing answers at {short(src)}</h2>
                <p>{hasRun === false ? "This repo has no Run script yet. Set one, or check the address." : "Start the Run script, or check the address."}</p>
                {hasRun === false
                  ? <button onClick={onSetup}><Settings2 size={14} />Set a Run script</button>
                  : <button onClick={onRun}><Play size={14} />Start Run script</button>}
                <button className="link" onClick={() => setReload((n) => n + 1)}>Try again</button>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
