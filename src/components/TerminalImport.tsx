import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronRight, Download, SquareTerminal, X } from "lucide-react";
import { api, errText, type TerminalPreview, type TerminalReport, type TerminalSession } from "../api";
import { agentName } from "../agents";
import { bytes, fresh, keyOf, newKeys, picks, plural, range, totals } from "../terminalImport";

function Tick({ checked, mixed, disabled, label, onChange }: { checked: boolean; mixed?: boolean; disabled?: boolean; label: string; onChange: (on: boolean) => void }) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => { if (ref.current) ref.current.indeterminate = !!mixed; }, [mixed]);
  return <input ref={ref} type="checkbox" checked={checked} disabled={disabled} aria-label={label} onChange={(e) => onChange(e.target.checked)} onClick={(e) => e.stopPropagation()} />;
}

const summary = (list: TerminalSession[]) => {
  const t = totals(list);
  return [plural(t.sessions, "session"), plural(t.messages, "message"), range(t.first, t.last), bytes(t.bytes)].filter(Boolean).join(" · ");
};

/**
 * "Import terminal sessions": the Claude Code, Codex and Gemini sessions you ran in a terminal,
 * grouped by repo and folder, with counts before anything comes in. Read-only on the CLIs' folders.
 */
export function TerminalImport({ onClose, onImported }: { onClose: () => void; onImported: (r: TerminalReport) => void }) {
  const [preview, setPreview] = useState<TerminalPreview | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.terminalScan().then((p) => { setPreview(p); setSelected(newKeys(p)); }).catch((e) => setError(errText(e)));
  }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape" && !busy) onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, busy]);

  const chosen = useMemo(() => (preview ? picks(preview, selected) : []), [preview, selected]);
  const all = preview?.repos.flatMap((r) => r.folders.flatMap((f) => f.sessions)) ?? [];
  const waiting = fresh(all).length;
  const skippedSessions = preview?.skipped.reduce((n, s) => n + s.sessions, 0) ?? 0;

  const setMany = (list: TerminalSession[], on: boolean) => setSelected((cur) => {
    const next = new Set(cur);
    for (const s of fresh(list)) on ? next.add(keyOf(s)) : next.delete(keyOf(s));
    return next;
  });
  const state = (list: TerminalSession[]) => {
    const f = fresh(list);
    const n = f.filter((s) => selected.has(keyOf(s))).length;
    return { checked: f.length > 0 && n === f.length, mixed: n > 0 && n < f.length, disabled: f.length === 0 };
  };

  const go = async () => {
    setBusy(true);
    setError(null);
    try {
      onImported(await api.terminalImport(chosen));
    } catch (e) {
      setError(errText(e));
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={() => !busy && onClose()}>
      <div className="modal terminal-import" role="dialog" aria-modal="true" aria-label="Import terminal sessions" onClick={(e) => e.stopPropagation()}>
        <div className="row"><SquareTerminal size={16} /><h2>Import terminal sessions</h2><span className="spacer" /><button className="icon" aria-label="Close" disabled={busy} onClick={onClose}><X size={15} /></button></div>
        <p className="muted">
          Chats you had with Claude Code, Codex or Gemini in a terminal, by the repo they ran in. NekoTrail only reads them: nothing in those folders changes, and no worktree is made.
          A worktree's chats go in a workspace for that folder. Your main checkout's go in a read-only History workspace.
        </p>
        {error && <div className="banner error">{error}</div>}
        {!preview && !error && <p className="muted row"><span className="spinner" />Reading your sessions…</p>}
        {preview && (
          <p className="ti-found">
            {all.length === 0 && skippedSessions === 0
              ? "No terminal sessions on this Mac yet."
              : <>{plural(all.length, "session")} in {plural(preview.repos.length, "repo")}{waiting < all.length ? `, ${all.length - waiting} already here` : ""}.</>}
            <span className="muted"> Read in {(preview.took_ms / 1000).toFixed(1)} s.</span>
          </p>
        )}
        {preview && preview.repos.length > 0 && (
          <div className="ti-list">
            {preview.repos.map((r) => {
              const list = r.folders.flatMap((f) => f.sessions);
              const st = state(list);
              return (
                <details key={r.root} className="ti-repo">
                  <summary>
                    <ChevronRight size={14} className="ti-chevron" aria-hidden />
                    <Tick {...st} label={`Import ${r.name}`} onChange={(on) => setMany(list, on)} />
                    <span className="ti-name"><b>{r.name}</b>{!r.repo_id && <span className="chip">New repo</span>}</span>
                    <span className="muted ti-sum">{summary(list)}</span>
                  </summary>
                  {!r.repo_id && <p className="muted ti-note">Not in NekoTrail yet. Importing adds {r.root}.</p>}
                  {r.folders.map((f) => {
                    const fs = state(f.sessions);
                    const label = f.kind === "history" ? "History (main checkout, read-only)" : f.kind === "workspace" ? `Workspace on ${f.branch}` : `Worktree on ${f.branch}`;
                    return (
                      <div key={f.path} className="ti-folder">
                        <div className="row ti-folder-head">
                          <Tick {...fs} label={`Import ${label}`} onChange={(on) => setMany(f.sessions, on)} />
                          <span>{label}</span>
                          <span className="muted ti-path" title={f.path}>{f.path}</span>
                        </div>
                        {f.sessions.map((s) => (
                          <label key={keyOf(s)} className={`row ti-session ${s.imported ? "done" : ""}`}>
                            <Tick checked={s.imported || selected.has(keyOf(s))} disabled={s.imported} label={s.title || s.id} onChange={(on) => setMany([s], on)} />
                            <span className="chip">{agentName(s.agent)}</span>
                            <span className="ti-title" title={s.title}>{s.title || "Untitled session"}</span>
                            <span className="muted">{s.imported ? "Already here" : `${plural(s.messages, "message")} · ${range(s.last, s.last)}`}</span>
                            {!s.imported && !s.resumable && f.kind !== "history" && <span className="muted" title="It started in a subfolder, and the CLI only resumes it from there. Its history still comes in.">Read only</span>}
                          </label>
                        ))}
                      </div>
                    );
                  })}
                </details>
              );
            })}
          </div>
        )}
        {preview && preview.skipped.length > 0 && (
          <details className="ti-skipped">
            <summary className="muted">{plural(skippedSessions, "session")} can't come in (the folder is gone or isn't a git repo)</summary>
            <ul>
              {preview.skipped.map((s) => <li key={s.folder}><span className="ti-path">{s.folder}</span> <span className="muted">{s.reason} {plural(s.sessions, "session")}.</span></li>)}
            </ul>
          </details>
        )}
        {preview && (
          <div className="row">
            <span className="muted ti-sources" title={preview.sources.join("\n")}>Read from {preview.sources.length ? preview.sources.join(", ") : "nowhere: no CLI folders found"}</span>
            <span className="spacer" />
            <button onClick={onClose} disabled={busy}>Cancel</button>
            <button className="primary" disabled={busy || chosen.length === 0} onClick={go}>
              {busy ? <span className="spinner" /> : <Download size={15} />}{chosen.length ? `Import ${plural(chosen.length, "session")}` : "Import"}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
