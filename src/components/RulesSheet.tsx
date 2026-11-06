import { useEffect, useMemo, useState } from "react";
import { ChevronLeft, CircleCheck, ExternalLink, RotateCw, ScrollText, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, errText, type RuleDiffLine, type RuleHow, type RuleOpened, type RulesOverview, type RulesPlan, type RulesRequest } from "../api";
import { agentName } from "../agents";
import { canApply, files, HOW_LABEL, keepConfirmed, stateLabel, statusLabel, toWrite } from "../rules";
import { Markdown } from "./Markdown";

type Pane = "files" | "edit" | "task";

/**
 * Rules: one set of project rules for every agent. Shows which instructions files this workspace
 * has, which agent reads which, and how each differs from the source of truth; makes the others
 * point at it (preview, backup, nothing committed); edits a rules file with a markdown preview; and
 * keeps this workspace's "Rules for this task".
 */
export function RulesSheet({ workspaceId, name, readOnly, initialPane = "files", onClose }: { workspaceId: string; name: string; readOnly: boolean; initialPane?: Pane; onClose: () => void }) {
  const [pane, setPane] = useState<Pane>(initialPane);
  const [source, setSource] = useState<string | undefined>(undefined);
  const [ov, setOv] = useState<RulesOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [planning, setPlanning] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const [editPath, setEditPath] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  // Leaving with unsaved text asks first, in the sheet (the web view has no reliable confirm()).
  const [ask, setAsk] = useState<(() => void) | null>(null);
  const guard = (fn: () => void) => { if (dirty) setAsk(() => fn); else fn(); };
  const load = () => {
    setError(null);
    api.rulesOverview(workspaceId, source).then(setOv).catch((e) => setError(errText(e)));
  };
  useEffect(load, [workspaceId, source]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (ask) setAsk(null);
      else if (planning) setPlanning(false);
      else guard(onClose);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, planning, dirty, ask]);
  const src = ov?.source ?? "AGENTS.md";
  const edit = (path: string) => { setEditPath(path); setPane("edit"); };
  const close = () => guard(onClose);

  return (
    <div className="modal-backdrop" onClick={close}>
      <div className="modal rules" role="dialog" aria-modal="true" aria-label="Rules" onClick={(e) => e.stopPropagation()}>
        <div className="row">
          {planning ? <button className="icon" onClick={() => setPlanning(false)} aria-label="Back" title="Back"><ChevronLeft size={15} /></button> : <ScrollText size={16} />}
          <h2>{planning ? `Use ${src} for every agent` : "Rules"}</h2>
          <span className="muted rules-where">{name}</span>
          <span className="spacer" />
          {!planning && pane === "files" && <button className="icon" onClick={load} aria-label="Check again" title="Check again"><RotateCw size={14} /></button>}
          <button className="icon" onClick={close} aria-label="Close"><X size={15} /></button>
        </div>
        {!planning && (
          <div className="seg" role="tablist" aria-label="Rules view">
            {([["files", "Files"], ["edit", "Edit rules"], ["task", "This task"]] as [Pane, string][]).map(([k, v]) => (
              <button key={k} role="tab" aria-selected={pane === k} className={pane === k ? "on" : ""} onClick={() => guard(() => { setDirty(false); setPane(k); })}>{v}</button>
            ))}
          </div>
        )}
        {ask && (
          <div className="mcp-note warn rules-ask" role="alert">
            <span>You have changes that aren't saved.</span>
            <span className="spacer" />
            <button onClick={() => setAsk(null)}>Keep editing</button>
            <button onClick={() => { const fn = ask; setAsk(null); setDirty(false); fn(); }}>Discard</button>
          </div>
        )}
        {planning && ov ? (
          <PlanPanel workspaceId={workspaceId} ov={ov} readOnly={readOnly} onBack={() => setPlanning(false)} onDone={(m) => { setPlanning(false); setDone(m); load(); }} />
        ) : pane === "files" ? (
          <div className="rules-body">
            {done && <div className="mcp-done"><CircleCheck size={14} /> <span>{done}</span></div>}
            {error && <div className="mcp-note bad">{error}</div>}
            {!ov && !error && <p className="muted"><span className="spinner" /> Reading the rules files…</p>}
            {ov && <FilesPane ov={ov} onSource={(s) => { setDone(null); setSource(s); }} onEdit={edit} />}
          </div>
        ) : pane === "edit" ? (
          <EditPane workspaceId={workspaceId} paths={ov ? editablePaths(ov) : [src]} path={editPath ?? src} readOnly={readOnly} onPath={(p) => guard(() => { setDirty(false); setEditPath(p); })} onDirty={setDirty} onSaved={load} />
        ) : (
          <TaskPane workspaceId={workspaceId} readOnly={readOnly} onDirty={setDirty} />
        )}
        {!planning && pane === "files" && ov && (
          <div className="row">
            <span className="muted rules-foot">{summary(ov)}</span>
            <span className="spacer" />
            <button className="primary" disabled={readOnly} title={readOnly ? "This workspace is read-only." : undefined} onClick={() => { setDone(null); setPlanning(true); }}>Use {src} for every agent…</button>
          </div>
        )}
      </div>
    </div>
  );
}

function summary(ov: RulesOverview): string {
  const miss = ov.agents.filter((a) => !a.gets_source).length;
  if (!ov.source_exists) return `There's no ${ov.source} yet.`;
  return miss === 0 ? `Every agent reads ${ov.source}.` : `${miss} of ${ov.agents.length} agents don't read ${ov.source}.`;
}

/** Files the editor offers: the source first, then every rules file here (scoped folders' files too). */
function editablePaths(ov: RulesOverview): string[] {
  const out = [ov.source];
  for (const f of ov.files) {
    if (f.dir) out.push(...f.files.filter((x) => /\.(md|mdc|txt)$/i.test(x)));
    else if (f.exists && !out.includes(f.path)) out.push(f.path);
  }
  return out;
}

function FilesPane({ ov, onSource, onEdit }: { ov: RulesOverview; onSource: (s: string) => void; onEdit: (path: string) => void }) {
  const src = ov.source;
  return (
    <>
      <p className="mcp-lead">Each agent reads its own rules file. Pick one as the source of truth, and nekotrail points the others at it.</p>
      <label className="rules-source">
        <span>Source of truth</span>
        <select value={src} onChange={(e) => onSource(e.target.value)} aria-label="Source of truth">
          {ov.sources.map((s) => <option key={s} value={s}>{s}{s === "AGENTS.md" ? " (read by most agents)" : ""}</option>)}
        </select>
      </label>
      {ov.warnings.map((w) => <div key={w} className="mcp-note warn">{w}</div>)}
      <h3 className="mcp-h">Files</h3>
      <p className="muted rules-small">Under each file, the agents that read it. A crossed-out agent reads it only in some setups; point at it to see when.</p>
      <ul className="rules-files">
        {ov.files.map((f) => {
          const st = stateLabel(f, src);
          const label = f.dir ? `${f.path}/` : f.path;
          return (
            <li key={f.path + (f.dir ? "/" : "")}>
              <div className="rules-file-head">
                <code className={f.exists ? "" : "muted"}>{label}</code>
                <span className={`rules-chip ${st.tone}`}>{st.text}</span>
                {f.exists && !f.dir && f.lines > 0 && <span className="muted rules-small">{f.lines} {f.lines === 1 ? "line" : "lines"}</span>}
                <span className="spacer" />
                {!f.dir && f.exists && !f.link && <button className="quiet" onClick={() => onEdit(f.path)}>Edit</button>}
              </div>
              <div className="rules-readers">
                {f.readers.map((r) => (
                  <span key={r.agent} className={`rules-reader ${r.active ? "" : "off"}`} title={r.active ? `${agentName(r.agent)} reads it` : `${agentName(r.agent)} reads it ${r.when || "only in some setups"}`}>{agentName(r.agent)}</span>
                ))}
              </div>
              {f.note && <p className="muted rules-small">{f.note}</p>}
              {f.dir && f.files.length > 0 && <p className="muted rules-small">{files(f.files.length)}: {f.files.slice(0, 4).map((x) => x.split("/").pop()).join(", ")}{f.files.length > 4 ? ", …" : ""}</p>}
              {f.diff.length > 0 && (
                <details className="rules-diff-wrap">
                  <summary>What's different from {src}</summary>
                  <DiffLines lines={f.diff} from={src} to={f.path} />
                </details>
              )}
            </li>
          );
        })}
      </ul>
      <h3 className="mcp-h">Who reads what</h3>
      <ul className="rules-agents">
        {ov.agents.map((a) => (
          <li key={a.agent}>
            <span className={`rules-dot ${a.gets_source ? "good" : "wait"}`} aria-label={a.gets_source ? `Reads ${src}` : `Doesn't read ${src}`} />
            <b>{agentName(a.agent)}</b>
            <span className="rules-reads">{a.reads.length ? a.reads.map((r) => <code key={r}>{r}</code>) : <span className="muted">No rules file here</span>}</span>
            {a.doc && <button className="icon" aria-label={`${agentName(a.agent)} docs`} title="Where this comes from" onClick={() => openUrl(a.doc).catch(() => {})}><ExternalLink size={12} /></button>}
          </li>
        ))}
      </ul>
      <p className="muted rules-small">From each agent's docs. Rules files in subfolders and your own home-folder rules aren't shown.</p>
    </>
  );
}

function DiffLines({ lines, from, to }: { lines: RuleDiffLine[]; from: string; to: string }) {
  return (
    <div className="rules-diff" role="table" aria-label={`Changes from ${from} to ${to}`}>
      {lines.map((l, i) => (
        <div key={i} className={`dl ${l.op}`} role="row">
          <span className="rules-mark" aria-hidden>{l.op === "add" ? "+" : l.op === "del" ? "-" : l.op === "gap" ? "" : " "}</span>
          <span>{l.op === "gap" ? `… ${l.text}` : l.text || " "}</span>
        </div>
      ))}
    </div>
  );
}

/** The plan: per file, what it becomes, the diff, and a confirm for text someone wrote. */
function PlanPanel({ workspaceId, ov, readOnly, onBack, onDone }: { workspaceId: string; ov: RulesOverview; readOnly: boolean; onBack: () => void; onDone: (msg: string) => void }) {
  const seeds = ov.files.filter((f) => f.role === "shared" && f.exists && !f.dir && f.path !== ov.source && f.via === null && f.state !== "empty").map((f) => f.path);
  const [req, setReq] = useState<RulesRequest>({ source: ov.source, seed: ov.source_exists ? null : seeds[0] ?? null, choices: {} });
  const [plan, setPlan] = useState<RulesPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmed, setConfirmed] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [rev, setRev] = useState(0);
  useEffect(() => {
    let live = true;
    setError(null);
    api.rulesPlan(workspaceId, req).then((p) => { if (live) { setPlan(p); setConfirmed((c) => keepConfirmed(p, c)); } }).catch((e) => { if (live) { setPlan(null); setError(errText(e)); } });
    return () => { live = false; };
  }, [workspaceId, req, rev]);
  const writes = toWrite(plan);
  const apply = async () => {
    if (!plan) return;
    setBusy(true);
    try {
      const r = await api.rulesApply(workspaceId, req, plan.token, confirmed);
      onDone(`Wrote ${r.written.join(", ")}. Nothing is committed: the changes are in Changes.${r.backup ? ` The old files are backed up in ${r.backup}.` : ""}`);
    } catch (e) {
      setError(errText(e));
      setRev((n) => n + 1);
    } finally {
      setBusy(false);
    }
  };
  const choose = (path: string, how: RuleHow) => setReq({ ...req, choices: { ...req.choices, [path]: how } });
  return (
    <>
      <div className="rules-body">
        {!ov.source_exists && (
          <label className="rules-source">
            <span>Start {ov.source} from</span>
            <select value={req.seed ?? ""} onChange={(e) => setReq({ ...req, seed: e.target.value || null })} aria-label={`Start ${ov.source} from`}>
              {seeds.length === 0 && <option value="">No rules file to start from</option>}
              {seeds.map((s) => <option key={s} value={s}>{s}</option>)}
            </select>
          </label>
        )}
        {error && <div className="mcp-note bad">{error}</div>}
        {!plan && !error && <p className="muted"><span className="spinner" /> Working out the changes…</p>}
        {plan?.notes.map((n) => <div key={n} className="mcp-note">{n}</div>)}
        {plan && (
          <ul className="rules-files">
            {plan.items.map((i) => (
              <li key={i.path} className={i.needs_confirm && !confirmed.includes(i.path) ? "rules-needs" : ""}>
                <div className="rules-file-head">
                  <code>{i.path}</code>
                  <span className={`rules-chip ${i.needs_confirm ? "bad" : i.status === "same" || i.status === "keep" ? "muted" : "good"}`}>{statusLabel(i)}</span>
                  <span className="spacer" />
                  {i.options.length > 1 && (
                    <select value={i.how} aria-label={`What ${i.path} becomes`} onChange={(e) => choose(i.path, e.target.value as RuleHow)}>
                      {i.options.map((o) => <option key={o} value={o}>{HOW_LABEL[o]}</option>)}
                    </select>
                  )}
                </div>
                {i.why && <p className="muted rules-small">{i.why}</p>}
                {i.how === "symlink" && i.status !== "same" && <p className="rules-small"><code>{i.path}</code> becomes a link to <code>{i.new}</code>.</p>}
                {i.diff.length > 0 && (
                  <details className="rules-diff-wrap" open={i.needs_confirm}>
                    <summary>{i.exists ? "The change" : "What goes in"}</summary>
                    <DiffLines lines={i.diff} from={i.exists ? i.path : "nothing"} to={i.path} />
                  </details>
                )}
                {i.needs_confirm && (
                  <label className="rules-confirm">
                    <input type="checkbox" checked={confirmed.includes(i.path)} onChange={(e) => setConfirmed(e.target.checked ? [...confirmed, i.path] : confirmed.filter((c) => c !== i.path))} />
                    <span>Replace the text in {i.path}. It's backed up first.</span>
                  </label>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
      <div className="row">
        <span className="muted rules-foot">{writes.length === 0 ? "Nothing to change." : `Writes ${files(writes.length)}, backs up the old ones, commits nothing.`}</span>
        <span className="spacer" />
        <button onClick={onBack}>Cancel</button>
        <button className="primary" disabled={readOnly || busy || !canApply(plan, confirmed)} onClick={apply}>{busy ? "Writing…" : writes.length ? `Write ${files(writes.length)}` : "Write"}</button>
      </div>
    </>
  );
}

/** "Edit rules": one file, with a live markdown preview beside it. */
function EditPane({ workspaceId, paths, path, readOnly, onPath, onDirty, onSaved }: { workspaceId: string; paths: string[]; path: string; readOnly: boolean; onPath: (p: string) => void; onDirty: (d: boolean) => void; onSaved: () => void }) {
  const [opened, setOpened] = useState<RuleOpened | null>(null);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const open = (p: string) => {
    setError(null); setSaved(null); setOpened(null);
    api.rulesOpen(workspaceId, p).then((o) => { setOpened(o); setText(o.text); onDirty(false); }).catch((e) => setError(errText(e)));
  };
  useEffect(() => open(path), [workspaceId, path]);
  const dirty = opened !== null && text !== opened.text;
  useEffect(() => onDirty(dirty), [dirty]);
  const all = useMemo(() => (paths.includes(path) ? paths : [path, ...paths]), [paths, path]);
  const save = async () => {
    if (!opened) return;
    setBusy(true); setError(null);
    try {
      const r = await api.rulesSave(workspaceId, opened.path, text, opened.hash);
      setSaved(`Saved ${opened.path}. It shows in Changes.${r.backup ? ` The old text is backed up in ${r.backup}.` : ""}`);
      const o = await api.rulesOpen(workspaceId, opened.path);
      setOpened(o); setText(o.text); onSaved();
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  };
  const locked = readOnly || !!opened?.link;
  return (
    <>
      <div className="row rules-edit-bar">
        <select value={path} aria-label="File" onChange={(e) => onPath(e.target.value)}>
          {all.map((p) => <option key={p} value={p}>{p}</option>)}
        </select>
        {opened && !opened.exists && <span className="muted rules-small">New file</span>}
        <span className="spacer" />
        {dirty && <button onClick={() => opened && setText(opened.text)}>Revert</button>}
        <button className="primary" disabled={locked || busy || !dirty} onClick={save}>{busy ? "Saving…" : "Save"}</button>
      </div>
      {saved && <div className="mcp-done"><CircleCheck size={14} /> <span>{saved}</span></div>}
      {error && <div className="mcp-note bad">{error}</div>}
      {opened?.link && <div className="mcp-note warn"><code>{opened.path}</code> is a link to <code>{opened.link}</code>. <button className="link" onClick={() => onPath(opened.link!)}>Edit {opened.link}</button></div>}
      {opened?.generated_from && <div className="mcp-note warn">nekotrail made this file from {opened.generated_from}, so the next sync replaces edits made here. <button className="link" onClick={() => onPath(opened.generated_from!)}>Edit {opened.generated_from}</button></div>}
      <div className="rules-editor">
        <textarea value={text} onChange={(e) => setText(e.target.value)} readOnly={locked} spellCheck={false} aria-label={`Text of ${path}`} placeholder={`# Project rules\n\nHow to build, test and style code in this repo.`} />
        <div className="rules-preview" aria-label="Preview">{text.trim() ? <Markdown text={text} /> : <p className="muted">The preview shows here.</p>}</div>
      </div>
    </>
  );
}

/** "Rules for this task": this workspace only, sent in front of the first message of each new chat. */
function TaskPane({ workspaceId, readOnly, onDirty }: { workspaceId: string; readOnly: boolean; onDirty: (d: boolean) => void }) {
  const [saved, setSaved] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { api.taskRules(workspaceId).then((t) => { setSaved(t); setText(t); }).catch((e) => setError(errText(e))); }, [workspaceId]);
  const dirty = saved !== null && text !== saved;
  useEffect(() => onDirty(dirty), [dirty]);
  const save = () => api.setTaskRules(workspaceId, text).then(() => { setSaved(text); setMsg(text.trim() ? "Saved. The next new chat here starts with these rules." : "Cleared."); }).catch((e) => setError(errText(e)));
  return (
    <>
      <p className="mcp-lead">Rules for this workspace only, for example "Don't touch the public API" or "Keep the change under 200 lines". They go in front of the first message of every new chat here, whichever agent it is. They stay on your Mac and never go into the repo.</p>
      {error && <div className="mcp-note bad">{error}</div>}
      {msg && <div className="mcp-done"><CircleCheck size={14} /> <span>{msg}</span></div>}
      <textarea className="rules-task" value={text} readOnly={readOnly} onChange={(e) => { setMsg(null); setText(e.target.value); }} aria-label="Rules for this task" placeholder="Rules for this task" />
      <div className="row">
        <span className="muted rules-foot">Chats that already started keep what they were sent.</span>
        <span className="spacer" />
        <button className="primary" disabled={readOnly || !dirty} onClick={save}>Save</button>
      </div>
    </>
  );
}
