import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { api } from "../api";
import { agentName } from "../agents";

type Row = Awaited<ReturnType<typeof api.agentScorecard>>[number];

/** How each agent has done for you, from turns run in nekotrail (imported history excluded). */
export function Scorecard({ onClose }: { onClose: () => void }) {
  const [rows, setRows] = useState<Row[] | null>(null);
  useEffect(() => { api.agentScorecard().then(setRows).catch(() => setRows([])); }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const name = agentName;
  const fmt = (ms: number) => (ms >= 60_000 ? `${(ms / 60_000).toFixed(1)} min` : `${Math.round(ms / 1000)} s`);
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal scorecard" role="dialog" aria-modal="true" aria-label="Agent scorecard" onClick={(e) => e.stopPropagation()}>
        <div className="row"><h2>Agent scorecard</h2><span className="spacer" /><button className="icon" aria-label="Close" onClick={onClose}><X size={15} /></button></div>
        <p className="muted">From turns you ran in nekotrail. Costs are the API prices the CLIs report; on a subscription you aren't billed per turn.</p>
        {rows === null && <p className="muted"><span className="spinner" /> Counting…</p>}
        {rows?.length === 0 && <p className="muted">No finished turns yet. Run a few tasks, or a bake-off (Both), and come back.</p>}
        {rows && rows.length > 0 && (
          <div className="score-grid" style={{ gridTemplateColumns: `repeat(${rows.length}, 1fr)` }}>
            {rows.map((r) => {
              const ok = r.turns ? Math.round(((r.turns - r.errors) / r.turns) * 100) : 0;
              return (
                <section key={r.agent} className="compare-col">
                  <span className="agent-tag">{name(r.agent)}</span>
                  <p className="score-big num">{ok}%<small className="muted"> of turns finished cleanly</small></p>
                  <dl>
                    <div><dt>Turns</dt><dd className="num">{r.turns.toLocaleString()}</dd></div>
                    <div><dt>Ended in an error</dt><dd className="num">{r.errors}</dd></div>
                    <div><dt>Average turn</dt><dd className="num">{r.turns ? fmt(r.duration_ms / r.turns) : "none"}</dd></div>
                    <div><dt>Spend</dt><dd className="num">${r.cost_usd.toFixed(2)}{r.turns ? <span className="muted"> · ${(r.cost_usd / r.turns).toFixed(2)} a turn</span> : null}</dd></div>
                    <div><dt>Workspaces merged</dt><dd className="num">{r.merged}</dd></div>
                  </dl>
                </section>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
