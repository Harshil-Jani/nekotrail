import { useEffect, useRef, useState } from "react";
import { AlertTriangle, CheckCircle2, Cloud, FolderInput, X, XCircle } from "lucide-react";
import { api, errText, type ImportReport, type RescueItem, type RescueOutcome, type RescuePlan } from "../api";
import { byRepo, plural, primaryLabel, outcomeLine, sizeLabel, summary, tilde, verdict, whatMoves, type Choice } from "../conductorRescue";

/** Backticked commands in a result line ("Run `git worktree unlock` on it") shown as code, not raw backticks. */
const withCode = (text: string) => text.split(/`([^`]+)`/).map((part, i) => (i % 2 ? <code key={i}>{part}</code> : part));

type Phase = "loading" | "choose" | "importing" | "moving" | "done" | "error";

/** Conductor Cloud's dates, as Conductor announced them. */
const CLOUD_NOTE = "nekotrail can't read Conductor Cloud, which goes read-only on 13 November and shuts down on 20 November 2026. Before then, right-click each Cloud workspace in Conductor and choose Transfer to local, then import it here.";

/**
 * Bringing work over from Conductor. Its worktrees live inside Conductor's folder, which Conductor
 * says goes when its app shuts down, so moving them into nekotrail's own folder is the default.
 * `mode` "import" imports first; "move" only moves workspaces imported earlier and left in place.
 */
export function ConductorSheet({ mode, autoStart, onChanged, onClose }: {
  mode: "import" | "move";
  /** Start right away with the default choice (the preview harness's in-progress and done states). */
  autoStart?: boolean;
  /** Something was imported or moved: refresh now. */
  onChanged: () => void;
  /** Closing, with a line for the status bar once anything happened. */
  onClose: (note?: { text: string; bad: boolean }) => void;
}) {
  const [phase, setPhase] = useState<Phase>("loading");
  const [plan, setPlan] = useState<RescuePlan | null>(null);
  const [choice, setChoice] = useState<Choice>("move");
  const [outcomes, setOutcomes] = useState<RescueOutcome[]>([]);
  const [imported, setImported] = useState<ImportReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<{ text: string; bad: boolean } | undefined>();
  const close = () => onClose(note);
  const started = useRef(false);
  const busy = phase === "importing" || phase === "moving";

  useEffect(() => {
    api.conductorRescuePlan().then((p) => {
      setPlan(mode === "move" ? { ...p, report: null, items: p.items.filter((i) => i.imported) } : p);
      setPhase("choose");
    }).catch((e) => { setError(errText(e)); setPhase("error"); });
  }, [mode]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape" && !busy) close(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const report = plan?.report ?? null;
  const items = plan?.items ?? [];
  const importing = !!report && report.repos + report.workspaces > 0;

  const go = async (retry?: string[]) => {
    if (!plan) return;
    setError(null);
    let rep = imported;
    try {
      if (!retry && importing) {
        setPhase("importing");
        rep = await api.importConductor();
        setImported(rep);
        onChanged();
      }
      if (choice === "leave" || (!retry && !items.length)) {
        onClose(rep ? { text: `Imported ${plural(rep.repos, "repo")}, ${plural(rep.workspaces, "workspace")} and ${plural(rep.sessions, "chat")}.${choice === "leave" && items.length ? " They stay in Conductor's folder; move them from ⌘K any time." : ""}`, bad: false } : undefined);
        return;
      }
      setPhase("moving");
      const res = await api.relocateImported(retry);
      const merged = retry ? outcomes.map((o) => res.find((r) => r.id === o.id) ?? o) : res;
      setOutcomes(merged);
      setPhase("done");
      onChanged();
      const s = summary(merged);
      setNote({ text: s.text, bad: s.bad });
    } catch (e) {
      setError(errText(e));
      setPhase(rep && !retry ? "done" : "error");
      if (rep) setNote({ text: `Imported ${plural(rep.workspaces, "workspace")}, but moving them failed: ${errText(e)}`, bad: true });
    }
  };
  useEffect(() => {
    if (autoStart && phase === "choose" && !started.current) { started.current = true; go(); }
  }, [autoStart, phase]);

  const s = summary(outcomes);
  const resultOf = (it: RescueItem) => outcomes.find((o) => o.from === it.from || (it.id && o.id === it.id));
  const title = phase === "done" && outcomes.length ? (s.bad ? "Moved, with some to look at" : "Your work is in nekotrail's folder") : mode === "move" ? "Move your workspaces out of Conductor's folder" : "Bring your work over from Conductor";

  return (
    <div className="modal-backdrop" onClick={() => !busy && close()}>
      <div className="modal conductor-sheet" role="dialog" aria-modal="true" aria-labelledby="cs-title" onClick={(e) => e.stopPropagation()}>
        <div className="row"><FolderInput size={16} aria-hidden /><h2 id="cs-title">{title}</h2><span className="spacer" />
          <button className="icon" aria-label="Close" disabled={busy} onClick={close}><X size={15} /></button>
        </div>

        {phase === "loading" && <p className="muted row"><span className="spinner" />Reading Conductor's data and measuring your worktrees…</p>}
        {phase === "error" && <div className="banner error" role="alert">{error}</div>}

        {plan && phase !== "done" && items.length > 0 && (
          <p className="cs-lead">
            Your worktrees live inside Conductor's folder, and Conductor says it will delete all user data when its app shuts down in early 2027. Moving them into <code>{tilde(plan.root)}</code> means your work no longer depends on Conductor.
          </p>
        )}

        {plan && phase === "choose" && items.length > 0 && (
          <div className="cs-choices" role="radiogroup" aria-label="Where your worktrees go">
            <label className={`cs-choice ${choice === "move" ? "on" : ""}`}>
              <input type="radio" name="cs-choice" checked={choice === "move"} onChange={() => setChoice("move")} />
              <span>
                <span className="cs-choice-title"><b>Move into nekotrail</b><span className="chip">Recommended</span></span>
                <small>Branches, commits and uncommitted changes come along, and each worktree is checked after it moves. Conductor won't find these folders any more.</small>
              </span>
            </label>
            <label className={`cs-choice ${choice === "leave" ? "on" : ""}`}>
              <input type="radio" name="cs-choice" checked={choice === "leave"} onChange={() => setChoice("leave")} />
              <span>
                <b>{importing ? "Import only, leave them in Conductor's folder" : "Leave them in Conductor's folder for now"}</b>
                <small>They keep depending on Conductor's folder. You can move them later from ⌘K.</small>
              </span>
            </label>
          </div>
        )}

        {plan && (phase === "done" && outcomes.length ? (
          <p className={`cs-summary ${s.bad ? "bad" : "good"}`} role="status">{s.text}</p>
        ) : phase === "done" && error ? (
          <div className="banner error" role="alert">{error}</div>
        ) : items.length > 0 ? (
          <p className="cs-what"><b>{choice === "move" || phase !== "choose" ? "What moves:" : "What stays:"}</b> {whatMoves(items)}</p>
        ) : phase === "choose" && !importing ? (
          <p className="muted">Every Conductor workspace you imported is already in nekotrail's folder. Nothing to move.</p>
        ) : null)}

        {plan && items.length > 0 && (
          <div className={`cs-list ${phase === "moving" || outcomes.length ? "has-state" : ""}`} aria-busy={busy}>
            {byRepo(items).map((g) => (
              <section key={g.repo} className="cs-repo" aria-label={g.repo}>
                <div className="cs-repo-head"><b>{g.repo}</b><span className="muted">{plural(g.items.length, "worktree")} · {sizeLabel(g.kb)}</span></div>
                {g.items.map((it) => {
                  const o = resultOf(it);
                  const v = o ? verdict(o) : null;
                  return (
                    <div key={it.from} className={`cs-item ${v ?? ""}`}>
                      {(phase === "moving" || outcomes.length > 0) && (
                        <span className="cs-state" aria-hidden>
                          {phase === "moving" && !o ? <span className="spinner" /> : v === "ok" ? <CheckCircle2 size={14} /> : v === "differs" ? <AlertTriangle size={14} /> : v === "failed" ? <XCircle size={14} /> : null}
                        </span>
                      )}
                      <span className="cs-main">
                        <span className="cs-name"><span className="cs-label">{it.label}</span><code className="cs-branch">{it.branch}</code></span>
                        {o ? (
                          <span className={`cs-result ${v}`}>{withCode(outcomeLine(o))}</span>
                        ) : (
                          <span className="cs-path muted" title={`${it.from} → ${it.to}`}>{tilde(it.from)}</span>
                        )}
                        {o?.moved && <span className="cs-path muted" title={o.to}>Now in {tilde(o.to)}</span>}
                      </span>
                      <span className="cs-meta muted">
                        {sizeLabel(it.kb)}
                        {it.uncommitted > 0 && <span className="chip cs-dirty" title="Staged, unstaged and untracked files. They come along.">{plural(it.uncommitted, "uncommitted file")}</span>}
                      </span>
                    </div>
                  );
                })}
              </section>
            ))}
          </div>
        )}

        {plan && phase === "choose" && importing && (
          <p className="muted cs-also">
            Also comes in: {plural(report!.repos, "repo")} with their scripts and prompts, and {plural(report!.sessions, "chat")} with their history. Chats continue the same agent session.
            {report!.skipped > 0 ? ` ${report!.skipped} skipped: already in nekotrail, or the folder is gone.` : ""}
          </p>
        )}

        {mode === "import" && phase === "choose" && (
          <div className="cs-cloud" role="note">
            <Cloud size={14} aria-hidden />
            <p><b>Using Conductor Cloud?</b> {CLOUD_NOTE}</p>
          </div>
        )}

        {phase === "importing" && <p className="row muted" role="status"><span className="spinner" />Importing repos, workspaces and chats…</p>}
        {phase === "moving" && <p className="row muted" role="status"><span className="spinner" />Moving {plural(items.length, "worktree")} and checking each one. Keep nekotrail open.</p>}

        <div className="row cs-actions">
          {phase === "choose" && <span className="muted cs-foot">Runs only <code>git worktree move</code>. Nothing is deleted.</span>}
          <span className="spacer" />
          {phase === "choose" && <button onClick={close}>Cancel</button>}
          {phase === "choose" && (
            <button className="primary" autoFocus onClick={() => go()}>
              {choice === "move" && items.length > 0 && <FolderInput size={15} />}{primaryLabel(choice, report, items)}
            </button>
          )}
          {phase === "done" && s.failed.length > 0 && <button onClick={() => go(s.failed)}>Try {plural(s.failed.length, "worktree")} again</button>}
          {(phase === "done" || phase === "error") && <button className="primary" onClick={close}>Done</button>}
          {busy && <button className="primary" disabled><span className="spinner" />{phase === "importing" ? "Importing…" : "Moving…"}</button>}
        </div>
      </div>
    </div>
  );
}

/** The one-time, dismissible nudge for workspaces imported earlier and still in Conductor's folder. */
export function MoveNotice({ count, onMove, onDismiss, compact }: { count: number; onMove: () => void; onDismiss: () => void; compact?: boolean }) {
  const where = `${count} imported ${count === 1 ? "workspace is" : "workspaces are"} still in Conductor's folder.`;
  const why = "Conductor says it will delete all user data when its app shuts down in early 2027.";
  if (compact) {
    return (
      <div className="away-banner move-notice" role="status">
        <FolderInput size={14} aria-hidden />
        <span className="mn-text"><b>{where}</b> Move them so your work no longer depends on Conductor.</span>
        <span className="mn-actions">
          <button onClick={onMove}>Move into nekotrail…</button>
          <button className="icon" aria-label="Dismiss" title="Don't show this again. ⌘K still has the move." onClick={onDismiss}><X size={13} /></button>
        </span>
      </div>
    );
  }
  return (
    <div className="import-card hero move-notice" role="status">
      <div className="row import-head">
        <h2>Move your imported workspaces into nekotrail</h2>
        <span className="spacer" />
        <button className="icon" aria-label="Dismiss" title="Don't show this again. ⌘K still has the move." onClick={onDismiss}><X size={14} /></button>
      </div>
      <p>{where} {why} Moving them takes seconds: branches, chats and uncommitted changes come along, and each one is checked after.</p>
      <button className="primary" onClick={onMove}><FolderInput size={15} />Move into nekotrail…</button>
    </div>
  );
}
