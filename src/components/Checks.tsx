import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, CheckCircle2, ChevronRight, Circle, Clock, FileCode2, Play, RotateCw, Settings2, ShieldAlert, Sparkles, Square, XCircle } from "lucide-react";
import { api, errText, type CheckDef, type CheckFailure, type CheckResult, type ChecksView, type Repo, type RepoPrefs } from "../api";
import { countLine, duration, fixAllPrompt, fixPrompt, headline, where } from "../checks";

/** Failures shown per check before "Show N more". */
const SHOWN = 20;

const PREFS: RepoPrefs = { general_prompt: "", review_prompt: "", pr_prompt: "", copy_files: [], env: [], merge_method: "", actions: [], protected: [] };

/**
 * The Checks tab: the repo's test, lint and typecheck commands, run in the background and read
 * into a list of failures. Each failure goes to the agent with one click.
 */
export function Checks({ workspaceId, repo, active, onSendAgent, onRepoSettings, onRepoChanged, onSay }: {
  workspaceId: string;
  repo: Repo | undefined;
  /** The tab is showing: reload when it comes back. */
  active: boolean;
  onSendAgent: (text: string) => void;
  onRepoSettings: () => void;
  onRepoChanged: () => void;
  onSay: (text: string, bad?: boolean) => void;
}) {
  const [view, setView] = useState<ChecksView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    api.checksList(workspaceId).then((v) => { setView(v); setError(null); }).catch((e) => setError(errText(e)));
  }, [workspaceId]);
  useEffect(() => { if (active) load(); }, [active, load, repo?.prefs]);
  useEffect(() => {
    const un = listen<{ workspace_id: string }>("checks", ({ payload }) => { if (payload.workspace_id === workspaceId) load(); });
    return () => { un.then((f) => f()); };
  }, [workspaceId, load]);

  const run = async (names: string[] | null = null) => {
    try { await api.checksRun(workspaceId, names); load(); } catch (e) { onSay(errText(e), true); }
  };
  const stop = () => api.checksStop(workspaceId).catch((e) => onSay(errText(e), true));
  const savePrefs = async (patch: Partial<RepoPrefs>) => {
    if (!repo) return;
    setBusy(true);
    try {
      await api.setRepoPrefs(repo.id, { ...PREFS, ...repo.prefs, ...patch });
      onRepoChanged();
    } catch (e) {
      onSay(errText(e), true);
    } finally {
      setBusy(false);
    }
  };

  if (error && !view) {
    return (
      <div className="checks"><div className="empty checks-empty">
        <AlertTriangle size={22} className="bad" aria-hidden />
        <h2>Couldn't load the checks</h2>
        <p>{error}</p>
        <button onClick={load}>Try again</button>
      </div></div>
    );
  }
  if (!view) return <div className="checks"><div className="empty checks-empty"><span className="spinner" aria-label="Loading" /></div></div>;

  const results = new Map(view.results.map((r) => [r.name, r]));
  const failing = view.results.filter((r) => r.status !== "passed" && r.failures.length > 0);
  const none = view.checks.length === 0;

  return (
    <div className="checks">
      {!none && <div className="checks-bar" role="toolbar" aria-label="Checks">
        <span className="checks-headline" role="status">
          {view.active && <span className="spinner" aria-hidden />}
          {headline(view)}
        </span>
        <span className="spacer" />
        <label className="checks-auto" title="After each agent turn that ends well, run these checks. If one that passed before fails, the workspace moves to Needs you.">
          <input type="checkbox" className="switch" checked={view.after_turn} disabled={busy || !repo} onChange={(e) => savePrefs({ checks_after_turn: e.target.checked })} />
          Run after every agent turn
        </label>
        {failing.length > 0 && !view.active && (
          <button className="primary" onClick={() => onSendAgent(fixAllPrompt(failing))} title="Send every failure to the agent in one message"><Sparkles size={14} />Fix all</button>
        )}
        {view.active
          ? <button className="danger" onClick={stop}><Square size={12} />Stop</button>
          : <button className={failing.length > 0 || view.trust ? "" : "primary"} onClick={() => run()}><Play size={13} />Run checks</button>}
      </div>}

      <div className="checks-body">
        {view.trust && <TrustCard checks={view.trust.checks} onAllow={async () => {
          try { await api.checksTrust(workspaceId, view.trust!.fingerprint); load(); } catch (e) { onSay(errText(e), true); load(); }
        }} />}

        {none && !view.trust && (
          <Suggest suggestions={view.suggestions} busy={busy} onRepoSettings={onRepoSettings}
            onAdd={async (picked) => {
              const have = new Set((repo?.prefs?.actions ?? []).map(([n]) => n));
              await savePrefs({ actions: [...(repo?.prefs?.actions ?? []), ...picked.filter(([n]) => !have.has(n))] });
              onSay(picked.length === 1 ? "Check added. Run it when you're ready." : `${picked.length} checks added. Run them when you're ready.`);
            }} />
        )}

        {view.checks.map((c) => (
          <CheckRow key={c.name} def={c} result={results.get(c.name)} workspaceId={workspaceId}
            state={view.current === c.name ? "running" : view.queued.includes(c.name) ? "queued" : null}
            runDisabled={view.active}
            onRun={() => run([c.name])}
            onFix={(f) => onSendAgent(fixPrompt(c, f))} />
        ))}

        {!none && (
          <p className="checks-foot muted">
            Checks are this repo's actions named test, lint, typecheck or similar, and the <code>checks</code> in its <code>nekotrail.json</code>.{" "}
            <button className="link" onClick={onRepoSettings}>Edit in repo settings</button>
          </p>
        )}
      </div>
    </div>
  );
}

/** A repo's nekotrail.json wants to run these: they wait for the user's OK, like its scripts. */
function TrustCard({ checks, onAllow }: { checks: [string, string][]; onAllow: () => void }) {
  return (
    <section className="checks-card trust" aria-label="Checks from nekotrail.json">
      <div className="checks-card-head"><ShieldAlert size={15} className="warn" aria-hidden /><b>This repo's nekotrail.json has checks</b></div>
      <p className="muted">They run on your Mac with your access, so nekotrail asks before running them. If the file changes, it asks again.</p>
      <ul className="checks-cmds">
        {checks.map(([n, c]) => <li key={n}><span>{n}</span><code>{c}</code></li>)}
      </ul>
      <div className="row end"><button className="primary" onClick={onAllow}>Allow these commands</button></div>
    </section>
  );
}

/** No checks yet: what fits this repo, ticked, ready to add as actions. */
function Suggest({ suggestions, busy, onAdd, onRepoSettings }: {
  suggestions: [string, string][]; busy: boolean; onAdd: (picked: [string, string][]) => void; onRepoSettings: () => void;
}) {
  const [off, setOff] = useState<Set<string>>(new Set());
  if (!suggestions.length) {
    return (
      <div className="empty checks-empty">
        <FileCode2 size={22} className="muted" aria-hidden />
        <h2>No checks yet</h2>
        <p>Add a test, lint or typecheck command as an action in the repo's settings, or list them under <code>checks</code> in <code>nekotrail.json</code>. They run here in the background and each failure goes to the agent in one click.</p>
        <button onClick={onRepoSettings}><Settings2 size={14} />Open repo settings</button>
      </div>
    );
  }
  const picked = suggestions.filter(([n]) => !off.has(n));
  return (
    <section className="checks-card" aria-label="Suggested checks">
      <div className="checks-card-head"><b>These fit this repo</b></div>
      <ul className="checks-suggest">
        {suggestions.map(([n, c]) => (
          <li key={n}>
            <label>
              <input type="checkbox" checked={!off.has(n)} onChange={() => setOff((s) => { const x = new Set(s); if (x.has(n)) x.delete(n); else x.add(n); return x; })} />
              <span className="checks-name">{n}</span>
              <code>{c}</code>
            </label>
          </li>
        ))}
      </ul>
      <p className="muted">They're saved as actions in the repo's settings, where you can change them. Nothing runs until you press Run checks.</p>
      <div className="row end">
        <button className="quiet" onClick={onRepoSettings}>Edit in repo settings</button>
        <button className="primary" onClick={() => onAdd(picked)} disabled={busy || !picked.length}>{picked.length === 1 ? "Add 1 check" : `Add ${picked.length} checks`}</button>
      </div>
    </section>
  );
}

function StatusIcon({ result, state }: { result?: CheckResult; state: "running" | "queued" | null }) {
  if (state === "running") return <span className="spinner" role="img" aria-label="Running" />;
  if (state === "queued") return <Clock size={15} className="muted" role="img" aria-label="Waiting to run" />;
  if (!result) return <Circle size={15} className="faint" role="img" aria-label="Not run yet" />;
  if (result.status === "passed") return <CheckCircle2 size={15} className="ok" role="img" aria-label="Passed" />;
  if (result.status === "error") return <AlertTriangle size={15} className="warn" role="img" aria-label="Couldn't run" />;
  return <XCircle size={15} className="bad" role="img" aria-label="Failed" />;
}

/** One check: its state, counts, failures and raw output. */
function CheckRow({ def, result, state, workspaceId, runDisabled, onRun, onFix }: {
  def: CheckDef; result?: CheckResult; state: "running" | "queued" | null; workspaceId: string; runDisabled: boolean;
  onRun: () => void; onFix: (f: CheckFailure) => void;
}) {
  const [output, setOutput] = useState<string | null>(null);
  const [showOutput, setShowOutput] = useState(false);
  const [all, setAll] = useState(false);
  const finished = result?.finished_at;
  // New result: fetch the output again next time it's opened.
  useEffect(() => { setOutput(null); }, [finished]);
  useEffect(() => {
    if (showOutput && output === null) api.checksOutput(workspaceId, def.name).then(setOutput).catch((e) => setOutput(errText(e)));
  }, [showOutput, output, workspaceId, def.name]);

  const failures = result && result.status !== "passed" ? result.failures : [];
  const shown = all ? failures : failures.slice(0, SHOWN);
  return (
    <section className={`checks-card one ${result?.status ?? "none"}`} aria-label={`${def.name} check`}>
      <div className="check-head">
        <StatusIcon result={result} state={state} />
        <b className="checks-name">{def.name}</b>
        <code className="check-cmd" title={def.command}>{def.command}</code>
        {result?.regressed && <span className="check-tag bad" title="It passed the time before this run">Passed before</span>}
        <span className="spacer" />
        {state && <span className="check-meta muted">{state === "running" ? "Running…" : "Waiting to run"}</span>}
        {result && !state && (
          <span className="check-meta num muted" title={`Finished ${new Date(result.finished_at).toLocaleString()}${result.formats.length ? `\nRead as ${result.formats.join(", ")}` : ""}`}>
            <span className={result.status === "passed" ? "" : "bad-text"}>{countLine(result)}</span> · {duration(result.duration_ms)}
          </span>
        )}
        {!result && !state && <span className="check-meta muted">Not run yet</span>}
        {result && (
          <button className="quiet check-output-btn" aria-expanded={showOutput} onClick={() => setShowOutput((s) => !s)}>
            <ChevronRight size={13} className={showOutput ? "open" : ""} aria-hidden />Output
          </button>
        )}
        <button className="icon" aria-label={`Run ${def.name}`} title={`Run ${def.name}`} disabled={runDisabled} onClick={onRun}><RotateCw size={14} /></button>
      </div>

      {shown.length > 0 && (
        <ul className="failures">
          {shown.map((f, i) => <FailureRow key={`${f.file}:${f.line}:${f.title}:${i}`} f={f} onFix={() => onFix(f)} />)}
        </ul>
      )}
      {failures.length > shown.length && (
        <button className="link check-more" onClick={() => setAll(true)}>Show {failures.length - shown.length} more</button>
      )}

      {showOutput && (
        <pre className="check-output" tabIndex={0} aria-label={`Output of ${def.name}`}>
          {output === null ? "Loading…" : output.trim() || "It printed nothing."}
        </pre>
      )}
    </section>
  );
}

function FailureRow({ f, onFix }: { f: CheckFailure; onFix: () => void }) {
  const at = where(f);
  const msg = f.message.trim() !== f.title.trim() ? f.message.trim() : "";
  return (
    <li className="failure">
      <div className="failure-head">
        <span className="failure-title">{f.title}</span>
        <span className="spacer" />
        <button className="quiet failure-fix" onClick={onFix} title="Send this failure and the code around it to the agent"><Sparkles size={13} />Fix with agent</button>
      </div>
      {at && <code className="failure-at">{at}</code>}
      {msg && <pre className="failure-msg">{msg}</pre>}
      {f.snippet && (
        <details className="failure-code">
          <summary>Code</summary>
          <pre>{f.snippet}</pre>
        </details>
      )}
    </li>
  );
}
