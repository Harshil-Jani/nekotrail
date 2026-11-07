import { useEffect, useMemo, useRef, useState } from "react";
import { Copy, Eye, EyeOff, RotateCw, Settings2, Variable, X } from "lucide-react";
import { api, errText, type EnvEntry, type EnvSection, type EnvView } from "../api";
import { envMatches, envSourceLabel, MASK } from "../envView";
import { keepTabInside } from "../useDismiss";

/**
 * Environment: every variable this workspace's agents, terminals and scripts get, grouped by where
 * it comes from, with the value that loses marked "overridden". Read only. Values from .env files
 * and anything that looks like a secret stay masked until you click the eye; nothing is saved or
 * sent anywhere.
 */
export function EnvSheet({ workspaceId, name, onEditRepo, onSay, onClose }: { workspaceId: string; name: string; onEditRepo: (repoId: string) => void; onSay: (msg: string) => void; onClose: () => void }) {
  const [view, setView] = useState<EnvView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [q, setQ] = useState("");
  // Revealed rows, by section and position. Cleared when the sheet reloads.
  const [shown, setShown] = useState<Set<string>>(new Set());
  const load = () => {
    setError(null); setShown(new Set());
    api.workspaceEnvironment(workspaceId).then(setView).catch((e) => setError(errText(e)));
  };
  useEffect(load, [workspaceId]);
  // Focus goes back where you were on close; opened from the ⋯ menu or ⌘K (both gone by then), to the ⋯ button.
  useEffect(() => {
    const back = document.activeElement as HTMLElement | null;
    return () => {
      const to = back?.isConnected ? back : document.querySelector<HTMLElement>("button[aria-label='More actions']");
      to?.focus();
    };
  }, []);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (error) box.current?.querySelector<HTMLElement>(".env-error button")?.focus();
    else if (!box.current?.contains(document.activeElement)) box.current?.focus();
  }, [view, error]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const sections = useMemo(() => (view?.sections ?? []).map((s) => ({ ...s, rows: s.entries.map((e, i) => ({ e, i })).filter(({ e }) => envMatches(e, q)) })), [view, q]);
  const total = view?.sections.reduce((n, s) => n + s.entries.length, 0) ?? 0;
  const matching = sections.reduce((n, s) => n + s.rows.length, 0);
  const toggle = (id: string) => setShown((cur) => { const next = new Set(cur); if (next.has(id)) next.delete(id); else next.add(id); return next; });
  const copy = (e: EnvEntry) => { navigator.clipboard.writeText(e.value).then(() => onSay(`Copied the value of ${e.key}.`), () => onSay("Couldn't copy that value.")); };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div ref={box} tabIndex={-1} onKeyDown={keepTabInside} className="modal env-sheet" role="dialog" aria-modal="true" aria-label="Environment" onClick={(e) => e.stopPropagation()}>
        <div className="row">
          <Variable size={16} aria-hidden />
          <h2>Environment</h2>
          <span className="muted rules-where">{name}</span>
          <span className="spacer" />
          <button className="icon" onClick={load} aria-label="Read again" title="Read again"><RotateCw size={14} /></button>
          <button className="icon" onClick={onClose} aria-label="Close"><X size={15} /></button>
        </div>
        <p className="muted env-lede">What agents, terminals and scripts in this workspace get, and where each value comes from. When a name is set twice, the one that loses is struck through. Secrets and .env values stay hidden until you click the eye.</p>
        {view && total > 0 && (
          <input className="env-filter" type="search" value={q} placeholder="Filter by name, source or file" aria-label="Filter variables" onChange={(e) => setQ(e.target.value)} autoFocus />
        )}
        {view && q.trim() === "" && (
          <nav className="env-jump" aria-label="Sections">
            {view.sections.map((s) => (
              <button key={s.id} className="quiet" onClick={() => document.getElementById(`env-sec-${s.id}`)?.scrollIntoView({ block: "start", behavior: "smooth" })}>
                {JUMP[s.id] ?? s.title}<span className="muted">{s.entries.length}</span>
              </button>
            ))}
          </nav>
        )}
        <div className="env-body">
          {error && <div className="mcp-note bad env-error"><span>{error}</span><button onClick={load}>Try again</button></div>}
          {!view && !error && <p className="muted"><span className="spinner" /> Reading this workspace's environment…</p>}
          {view && q.trim() !== "" && matching === 0 && <p className="muted env-empty">No variable matches "{q.trim()}". <button className="link" onClick={() => setQ("")}>Clear the filter</button></p>}
          {view && sections.map((s) => (q.trim() !== "" && s.rows.length === 0 ? null : (
            <Section key={s.id} s={s} rows={s.rows} filtering={q.trim() !== ""} shown={shown} onToggle={toggle} onCopy={copy} onEditRepo={() => onEditRepo(view.repo_id)} />
          )))}
        </div>
        <div className="row env-foot">
          <span className="muted">Read only. Nothing here is saved or sent anywhere.</span>
          <span className="spacer" />
          <button onClick={() => view && onEditRepo(view.repo_id)} disabled={!view}><Settings2 size={13} />Edit in Repo settings</button>
        </div>
      </div>
    </div>
  );
}

/** Short names for the jump bar. */
const JUMP: Record<string, string> = { app: "NekoTrail", repo: "Repo settings", files: ".env files", login: "Login shell" };

/** Headings for NekoTrail's own groups. */
const GROUP: Record<string, string> = { Workspace: "This workspace", "Port block": "Ports", "Review workspace": "Review workspace: git push switched off" };

const EMPTY: Record<string, string> = {
  repo: "No variables in Repo settings for this repo yet.",
  files: "No .env files in this workspace. Files you list under Repo settings, Files to copy, are copied in when a workspace is made.",
  login: "Your login shell hasn't passed anything on yet. It is read in the background when NekoTrail starts.",
  app: "Nothing set by NekoTrail here.",
};

function Section({ s, rows, filtering, shown, onToggle, onCopy, onEditRepo }: {
  s: EnvSection; rows: { e: EnvEntry; i: number }[]; filtering: boolean; shown: Set<string>;
  onToggle: (id: string) => void; onCopy: (e: EnvEntry) => void; onEditRepo: () => void;
}) {
  const head = `env-sec-${s.id}`;
  // .env files: one group per file, in the order they were found. NekoTrail's own: workspace, ports, review.
  const grouped = s.id === "files" || s.id === "app";
  const groups = grouped ? [...new Set(rows.map((r) => r.e.source))].map((f) => ({ file: f, rows: rows.filter((r) => r.e.source === f) })) : [{ file: "", rows }];
  const count = filtering && rows.length !== s.entries.length ? `${rows.length} of ${s.entries.length}` : `${s.entries.length}`;
  return (
    <section className="env-sec" aria-labelledby={head} data-section={s.id}>
      <div className="env-sec-head">
        <h3 id={head}>{s.title}</h3>
        <span className="rules-chip muted" aria-label={`${count} variables`}>{count}</span>
        {s.hidden > 0 && <span className="rules-chip muted" title="Passed on, never listed">+{s.hidden} hidden</span>}
      </div>
      <p className="muted env-note">{s.note}</p>
      {s.entries.length === 0 ? (
        <p className="env-empty muted">{EMPTY[s.id] ?? "Nothing here."}{s.id === "repo" && <> <button className="link" onClick={onEditRepo}>Add one in Repo settings</button></>}</p>
      ) : groups.map((g) => (
        <div key={g.file || "all"} className="env-group">
          {g.file && <div className="env-file">{s.id === "files" ? <code>{g.file}</code> : <b>{GROUP[g.file] ?? g.file}</b>}<span className="muted">{g.rows.length}</span></div>}
          <ul className="env-list">
            {g.rows.map(({ e, i }) => <Row key={i} e={e} id={`${s.id}:${i}`} open={shown.has(`${s.id}:${i}`)} onToggle={onToggle} onCopy={onCopy} />)}
          </ul>
        </div>
      ))}
    </section>
  );
}

function Row({ e, id, open, onToggle, onCopy }: { e: EnvEntry; id: string; open: boolean; onToggle: (id: string) => void; onCopy: (e: EnvEntry) => void }) {
  const hidden = e.masked && !open;
  return (
    <li className={e.overridden_by ? "env-row-item overridden" : "env-row-item"}>
      <div className="env-key">
        <code title={e.key}>{e.key}</code>
        {e.overridden_by && <span className="rules-chip wait" title={`${e.overridden_by} sets ${e.key} too, and that value is the one that applies.`}>overridden by {envSourceLabel(e.overridden_by)}</span>}
      </div>
      <code className={`env-value ${hidden ? "masked" : ""} ${e.masked && open ? "open" : ""} ${e.value === "" ? "empty" : ""}`} title={hidden || e.value === "" ? undefined : e.value} aria-label={hidden ? `${e.key} value hidden` : undefined}>
        {e.value === "" ? "(empty)" : hidden ? MASK : e.value}
      </code>
      <span className="env-actions">
        {e.masked && (
          <button className="icon" aria-pressed={open} aria-label={open ? `Hide the value of ${e.key}` : `Show the value of ${e.key}`} title={open ? "Hide" : "Show"} onClick={() => onToggle(id)}>
            {open ? <EyeOff size={14} /> : <Eye size={14} />}
          </button>
        )}
        <button className="icon" aria-label={`Copy the value of ${e.key}`} title="Copy the value" disabled={e.value === ""} onClick={() => onCopy(e)}><Copy size={13} /></button>
      </span>
    </li>
  );
}
