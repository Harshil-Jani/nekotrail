import { useEffect, useRef, useState } from "react";
import { AppWindow, CircleStop, ExternalLink, TriangleAlert } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, errText, type PortRow, type WsPorts } from "../api";
import { ask } from "./Confirm";
import { blockRange, holderText, orderWorkspaces, portLabel, quietPorts } from "../ports";

/** Fix text with `code` spans, as the backend writes it. */
function Fix({ text }: { text: string }) {
  return <>{text.split("`").map((part, i) => (i % 2 ? <code key={i}>{part}</code> : part))}</>;
}

/**
 * Every active workspace's ports: what listens on each, open it in the browser or the Preview tab,
 * and stop a server this workspace started. Refreshes every 3 seconds while open.
 */
export function PortsPopover({
  selected,
  task = [],
  label,
  repoOf,
  onPreview,
  onSay,
  onClose,
}: {
  selected: string;
  /** The other workspaces of the open multi-repo task, listed right after it. */
  task?: string[];
  /** A workspace's name as the sidebar shows it. */
  label: (id: string) => string;
  /** A workspace's repo name, so each repo of the open task is headed "This task, <repo>". */
  repoOf?: (id: string) => string;
  /** Show this local address in that workspace's Preview tab. */
  onPreview: (wsId: string, url: string) => void;
  onSay: (text: string, error?: boolean) => void;
  onClose: () => void;
}) {
  const [list, setList] = useState<WsPorts[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [freeing, setFreeing] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const load = () =>
      api.portsStatus()
        .then((l) => { if (live) { setList(l); setError(null); } })
        .catch((e) => { if (live) setError(errText(e)); })
        .finally(() => { if (live) timer = setTimeout(load, 3000); });
    load();
    return () => { live = false; clearTimeout(timer); };
  }, [tick]);

  // Click outside or Esc closes it. A confirm sheet on top is not "outside".
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (box.current?.contains(t) || t.closest(".modal-backdrop, .modal, .tb-ports")) return;
      onClose();
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape" && !document.querySelector(".modal")) onClose(); };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => { window.removeEventListener("mousedown", onDown); window.removeEventListener("keydown", onKey); };
  }, [onClose]);

  const url = (port: number) => `http://localhost:${port}`;
  const open = (port: number) => openUrl(url(port)).catch((e) => onSay(errText(e), true));

  const free = async (ws: WsPorts, row: PortRow) => {
    const h = row.holder;
    if (!h) return;
    const ok = await ask({
      title: `Stop ${h.command} on ${row.port}?`,
      body: `It was started in ${ws.id === selected ? "this workspace" : label(ws.id)} (pid ${h.pid}). Anything it hasn't saved is lost.`,
      ok: "Free this port",
      danger: true,
    });
    if (!ok) return;
    const key = `${ws.id}:${row.port}`;
    setFreeing(key);
    try {
      await api.freePort(ws.id, row.port);
      onSay(`Port ${row.port} is free.`);
    } catch (e) {
      onSay(errText(e), true);
    } finally {
      setFreeing(null);
      setTick((n) => n + 1);
    }
  };

  return (
    <div className="ports-pop" ref={box} role="dialog" aria-label="Ports">
      <div className="ports-head">
        <strong>Ports</strong>
        <span className="muted">Each workspace has 10 of its own, in <code>NEKOTRAIL_PORT</code> and <code>PORT</code>.</span>
      </div>
      <div className="ports-body">
        {error && <p className="ports-note bad-text">{error}</p>}
        {!list && !error && <span className="spinner" aria-label="Loading" />}
        {list && list.length === 0 && <p className="ports-note muted">No active workspaces.</p>}
        {list && orderWorkspaces(list, selected, task).map((ws) => {
          const quiet = quietPorts(ws);
          return (
            <section key={ws.id} className="ports-ws" aria-label={`Ports of ${label(ws.id)}`}>
              <div className="ports-ws-head">
                <span className="ports-ws-name">{task.length > 1 && task.includes(ws.id) && repoOf ? `This task, ${repoOf(ws.id)}` : ws.id === selected ? "This workspace" : label(ws.id)}</span>
                <span className="mono faint">{blockRange(ws)}</span>
              </div>
              {ws.problems.map((p, i) => (
                <div key={i} className="ports-problem" role="note">
                  <TriangleAlert size={14} aria-hidden />
                  <div>
                    <p>{p.text}</p>
                    <p className="muted"><Fix text={p.fix} /></p>
                  </div>
                </div>
              ))}
              {ws.rows.map((r) => {
                const busy = freeing === `${ws.id}:${r.port}`;
                return (
                  <div key={r.port} className={`ports-row${r.outside ? " outside" : ""}`}>
                    <span className="ports-num mono">{r.port}</span>
                    <span className="ports-what">
                      <span className="ports-label">{portLabel(r)}</span>
                      <span className={r.holder ? "ports-holder" : "ports-holder faint"}>{holderText(r, ws.id, label)}</span>
                    </span>
                    {/* Nothing to open on a free port, so no dead buttons there. */}
                    {r.holder && (
                      <span className="ports-actions">
                        <button className="icon" aria-label={`Open ${r.port} in your browser`} title="Open in browser" onClick={() => open(r.port)}><ExternalLink size={14} /></button>
                        <button className="icon" aria-label={`Show ${r.port} in Preview`} title="Open in Preview" onClick={() => { onPreview(ws.id, url(r.port)); onClose(); }}><AppWindow size={14} /></button>
                        {r.holder.can_free ? (
                          <button className="icon" aria-label={`Free port ${r.port}`} title={`Stop ${r.holder.command} to free this port`} disabled={busy} onClick={() => free(ws, r)}>
                            {busy ? <span className="spinner" /> : <CircleStop size={14} />}
                          </button>
                        ) : <span className="ports-noop" aria-hidden />}
                      </span>
                    )}
                  </div>
                );
              })}
              {quiet > 0 && <p className="ports-quiet faint">{quiet} more free, as <code>NEKOTRAIL_PORT_1</code> to <code>_9</code>.</p>}
            </section>
          );
        })}
      </div>
      <p className="ports-foot muted">Name them in <code>nekotrail.json</code>: <code>{`"ports": {"web": "PORT", "api": "API_PORT"}`}</code>. A <code>PORT</code> you export yourself is never replaced.</p>
    </div>
  );
}
