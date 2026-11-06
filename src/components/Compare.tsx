import { useEffect, useState } from "react";
import { Crown, X } from "lucide-react";
import { api, type Workspace } from "../api";
import { agentName } from "../agents";
import { ask } from "./Confirm";

type Stats = Awaited<ReturnType<typeof api.workspaceStats>>;

/** Bake-off results: the same task done by different agents, side by side. Keep one, archive the rest. */
export function Compare({
  workspaces, names, task, lines, prStates, onOpen, onKeep, onClose,
}: {
  workspaces: Workspace[];
  /** Each column's star: the workspaces share one task, so their titles match. */
  names: (w: Workspace) => string;
  /** The task they share, or "" before it has a title. */
  task: string;
  lines: Record<string, [number, number]>;
  prStates: Record<string, string>;
  onOpen: (id: string) => void;
  onKeep: (id: string) => void;
  onClose: () => void;
}) {
  const [stats, setStats] = useState<Record<string, Stats>>({});
  const [files, setFiles] = useState<Record<string, number>>({});
  useEffect(() => {
    for (const w of workspaces) {
      api.workspaceStats(w.id).then((s) => setStats((x) => ({ ...x, [w.id]: s }))).catch(() => {});
      api.changes(w.id).then((c) => setFiles((x) => ({ ...x, [w.id]: c.length }))).catch(() => {});
    }
  }, [workspaces.map((w) => w.id).join()]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // Cheapest and fastest get a quiet marker; the human decides.
  const done = workspaces.filter((w) => stats[w.id]?.turns);
  const best = (f: (s: Stats) => number) => done.length > 1 ? done.reduce((a, b) => (f(stats[a.id]) <= f(stats[b.id]) ? a : b)).id : null;
  const cheapest = best((s) => s.cost_usd || Infinity);
  const fastest = best((s) => s.duration_ms || Infinity);
  // Recommended only when one result is both the cheapest and the fastest; otherwise no button shouts.
  const recommended = cheapest && cheapest === fastest ? cheapest : null;
  const keep = async (w: Workspace) => {
    const others = workspaces.filter((x) => x.id !== w.id).map(names).join(" and ");
    const go = await ask({
      title: `Keep ${names(w)}?`,
      body: `${others} is archived. Its branch is kept, but uncommitted changes in its folder are lost.`,
      ok: `Keep ${names(w)}`,
    });
    if (go) onKeep(w.id);
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal compare" role="dialog" aria-modal="true" aria-label="Compare agents" onClick={(e) => e.stopPropagation()}>
        <div className="row">
          {/* The shared task is the sheet's subtitle, right under its title. */}
          <div className="compare-head">
            <h2>Compare agents</h2>
            {task && <p className="compare-task" title={task}>{task}</p>}
          </div>
          <span className="spacer" />
          <button className="icon" aria-label="Close" onClick={onClose}><X size={16} /></button>
        </div>
        <p className="muted">Same prompt, different agents. Open each to read the diff, then keep the one you like. The other workspace is archived, and its branch is kept.</p>
        <div className="compare-grid" style={{ gridTemplateColumns: `repeat(${workspaces.length}, 1fr)` }}>
          {workspaces.map((w) => {
            const s = stats[w.id];
            const l = lines[w.id];
            return (
              <section key={w.id} className="compare-col">
                <span className="agent-tag">{s?.agent ? agentName(s.agent) : "Agent"}</span>
                <h3>{names(w)}</h3>
                <code className="muted">{w.branch}</code>
                <dl>
                  <div><dt>Changes</dt><dd className="num">{files[w.id] ?? "…"} files {l && <><span className="add">+{l[0]}</span> <span className="del">-{l[1]}</span></>}</dd></div>
                  <div><dt>Turns</dt><dd className="num">{s ? `${s.turns}${s.errors ? `, ${s.errors} with errors` : ""}` : "…"}</dd></div>
                  <div><dt>Time</dt><dd className="num">{s ? `${Math.round(s.duration_ms / 1000)}s` : "…"}{fastest === w.id && <Crown size={12} className="win" aria-label="fastest" />}</dd></div>
                  <div><dt>Cost</dt><dd className="num">{s ? `$${s.cost_usd.toFixed(2)}` : "…"}{cheapest === w.id && <Crown size={12} className="win" aria-label="cheapest" />}</dd></div>
                  <div><dt>PR</dt><dd>{prStates[w.id] ? prStates[w.id].toLowerCase() : "none yet"}</dd></div>
                </dl>
                <div className="row">
                  <button onClick={() => onOpen(w.id)}>Open changes</button>
                  <button className={`keep ${recommended === w.id ? "primary" : ""}`} onClick={() => keep(w)} title={recommended === w.id ? "Cheapest and fastest of the two" : undefined}>Keep this one</button>
                </div>
              </section>
            );
          })}
        </div>
      </div>
    </div>
  );
}
