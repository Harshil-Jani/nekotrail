import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, CheckCircle2, Clock, Gauge, GripVertical, Hand, ListOrdered, Pause, Play, Plus, RotateCw, Trash2, XCircle } from "lucide-react";
import { api, errText, type Agent, type PermissionMode, type Repo } from "../api";
import { AGENT_INFO, AGENTS, agentName } from "../agents";
import { useInstalledAgents } from "../useInstalledAgents";
import { useCatalog } from "./ModelPicker";
import { okFullAuto } from "../fullAuto";
import { ask } from "./Confirm";
import { keyHint, mod } from "../platform";
import { clockTime, dropBefore, elapsed, groups, MODE_LABEL, moved, QUEUE_EMPTY, queueScreen, stateText, taskTitle, thenText, unattended, waitText, type QueueTask, type QueueThen, type QueueView } from "../queue";

const MODES: PermissionMode[] = ["plan", "ask", "acceptEdits", "bypassPermissions"];
/** The choices for "Run up to N at once". The backend allows up to 16. */
const LIMITS = [1, 2, 3, 4, 5, 6, 8];

/**
 * The task queue: line up tasks and nekotrail runs a few at a time while you're away, each in its
 * own new workspace. Running tasks on top, then the line (drag to reorder), then what finished.
 */
export function Queue({ repos, initialRepo, addNonce, onOpen, onSay, onUsage }: {
  repos: Repo[];
  /** The repo the Add form starts on: the one you were looking at. */
  initialRepo?: string;
  /** Bumped to open the Add form (⌘K, "Add task to queue"). */
  addNonce: number;
  onOpen: (workspaceId: string) => void;
  onSay: (text: string, bad?: boolean) => void;
  /** Open the Usage view, where the spend caps are. */
  onUsage: () => void;
}) {
  const [view, setView] = useState<QueueView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [drag, setDrag] = useState<{ id: string; over: string | null; lower: boolean } | null>(null);
  /** The order shown while a move saves, so the row doesn't jump back. */
  const [order, setOrder] = useState<string[] | null>(null);

  const load = useCallback(() => {
    api.queueView().then((v) => { setView(v); setError(null); }).catch((e) => setError(errText(e)));
  }, []);
  useEffect(() => {
    load();
    const uns = ["task-queue", "running", "needs"].map((ev) => listen(ev, load));
    // Elapsed times and limit countdowns.
    const t = setInterval(load, 30_000);
    return () => { clearInterval(t); uns.forEach((u) => u.then((f) => f())); };
  }, [load]);
  useEffect(() => { if (addNonce) setAdding(true); }, [addNonce]);

  const act = (p: Promise<unknown>) => p.then(load).catch((e) => onSay(errText(e), true));
  const repoName = (id: string) => repos.find((r) => r.id === id)?.name ?? "Removed repo";

  const screen = queueScreen(view, error);
  if (!view) {
    return (
      <div className="tq">
        <QueueHead view={null} onAdd={() => setAdding(true)} act={act} />
        {screen === "loading" && <div className="empty tq-empty" role="status"><span className="spinner" aria-hidden /><p>Loading the queue…</p></div>}
        {screen === "error" && <div className="empty"><AlertCircle size={22} aria-hidden /><h2>Couldn't load the queue</h2><p>{error}</p><button onClick={load}>Try again</button></div>}
      </div>
    );
  }

  const g = groups(view.tasks);
  const byId = new Map(view.tasks.map((t) => [t.id, t]));
  const next = (order ?? g.next.map((t) => t.id)).map((id) => byId.get(id)).filter((t): t is QueueTask => !!t && (t.state === "queued" || t.state === "waiting"));
  const nextIds = next.map((t) => t.id);
  const move = (id: string, before: string | null) => {
    setOrder(moved(nextIds, id, before));
    api.queueMove(id, before).catch((e) => onSay(errText(e), true)).finally(() => { setOrder(null); load(); });
  };
  const limited = Object.entries(view.limits);
  const retry = async (t: QueueTask) => act(api.queueRetry(t.id));
  const remove = async (t: QueueTask) => {
    if (t.state === "queued" || t.state === "waiting") {
      const go = await ask({ title: "Remove this task from the queue?", quote: t.prompt, ok: "Remove", danger: true });
      if (!go) return;
    }
    act(api.queueRemove(t.id));
  };

  const row = (t: QueueTask, kind: "running" | "next" | "finished", n?: number) => {
    const asking = view.asking.includes(t.id);
    const agent = t.ran_with ?? t.agent;
    const who = t.ran_with && t.ran_with !== t.agent ? `${agentName(agent)}, for ${agentName(t.agent)}` : agentName(agent);
    const meta = [repoName(t.repo_id), t.base ? `from ${t.base}` : "", who, MODE_LABEL[t.mode], thenText(t.then)].filter(Boolean).join(" · ");
    const status = kind === "running"
      ? `${stateText(t, asking)}${t.started_at ? ` · ${elapsed(t.started_at, view.now)}` : ""}${t.step === "autopilot" || !t.note ? "" : ` · ${t.note}`}`
      : kind === "next"
        ? t.state === "waiting" ? stateText(t, false) : `${waitText(view.waits[t.id], view)}${t.resume ? " · continues in its workspace" : ""}`
        : t.note || stateText(t, false);
    const capped = kind === "next" && view.waits[t.id]?.kind === "budget";
    const tone = t.state === "failed" ? "bad" : t.state === "needs_you" || asking ? "attn" : t.state === "waiting" || capped ? "wait" : t.state === "done" ? "good" : "";
    const dropping = drag && drag.over === t.id && drag.id !== t.id;
    return (
      <div
        key={t.id}
        className={`tq-row ${kind}${drag?.id === t.id ? " dragging" : ""}${dropping ? (drag.lower ? " drop-below" : " drop-above") : ""}`}
        role="listitem"
        aria-label={taskTitle(t.prompt)}
        tabIndex={kind === "next" ? 0 : -1}
        draggable={kind === "next"}
        onDragStart={(e) => { e.dataTransfer.effectAllowed = "move"; e.dataTransfer.setData("text/plain", t.id); setDrag({ id: t.id, over: null, lower: false }); }}
        onDragEnd={() => setDrag(null)}
        onDragOver={(e) => {
          if (!drag || kind !== "next") return;
          e.preventDefault();
          const r = e.currentTarget.getBoundingClientRect();
          const lower = e.clientY > r.top + r.height / 2;
          if (drag.over !== t.id || drag.lower !== lower) setDrag({ ...drag, over: t.id, lower });
        }}
        onDrop={(e) => {
          e.preventDefault();
          if (!drag) return;
          const before = dropBefore(nextIds, drag.id, t.id, drag.lower);
          setDrag(null);
          if (before !== undefined) move(drag.id, before);
        }}
        onKeyDown={(e) => {
          // ⌥↑ / ⌥↓ move a task in line without a mouse.
          if (kind !== "next" || !e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
          e.preventDefault();
          const i = nextIds.indexOf(t.id);
          if (e.key === "ArrowUp" && i > 0) move(t.id, nextIds[i - 1]);
          if (e.key === "ArrowDown" && i < nextIds.length - 1) move(t.id, nextIds[i + 2] ?? null);
        }}
        title={t.prompt}
      >
        <span className="tq-mark" aria-hidden>
          {kind === "running" ? (asking ? <Hand size={15} className="attn" /> : <span className="spinner" />)
            : kind === "next" ? <><GripVertical size={14} className="grip" /><span className="tq-n num">{n}</span></>
            : t.state === "done" ? <CheckCircle2 size={15} className="good" />
            : t.state === "needs_you" ? <AlertCircle size={15} className="attn" />
            : <XCircle size={15} className="bad" />}
        </span>
        <span className="tq-text">
          <span className="tq-title">{taskTitle(t.prompt)}</span>
          <span className="tq-meta">{meta}</span>
          <span className={`tq-status ${tone}`}>{status}</span>
          {kind === "finished" && t.evidence && t.evidence.length > 0 && (
            <ul className="tq-evidence" aria-label="What NekoTrail saw">
              {t.evidence.map((line, i) => <li key={i}>{line}</li>)}
            </ul>
          )}
        </span>
        <span className="tq-actions">
          {kind === "next" && t.state === "waiting" && <button onClick={() => act(api.queueStartNow(t.id))} title="Start it now. It needs you there to answer it."><Hand size={13} className="wait" />Start</button>}
          {kind === "next" && t.state === "queued" && <button className="quiet hover-only" onClick={() => act(api.queueStartNow(t.id))} title="Start it now, ahead of the line and the limits">Start now</button>}
          {(kind === "running" || kind === "finished") && t.workspace_id && <button onClick={() => onOpen(t.workspace_id!)}>Open</button>}
          {kind === "finished" && t.state === "failed" && <button onClick={() => retry(t)} title={t.workspace_id ? "Run it again in its workspace" : "Run it again in a new workspace"}><RotateCw size={13} />Retry</button>}
          {kind !== "running" && <button className="icon" onClick={() => remove(t)} aria-label="Remove from the queue" title="Remove from the queue"><Trash2 size={14} /></button>}
        </span>
      </div>
    );
  };

  const empty = screen === "empty";
  return (
    <div className="tq">
      {/* Empty, the page's own Add task is the one button; paused, the banner's Resume is. */}
      <QueueHead view={view} onAdd={empty && !adding ? null : () => setAdding(true)} act={act} />
      <div className="tq-body">
        {view.paused && (
          <div className="tq-banner">
            <Pause size={14} aria-hidden /><span>The queue is paused. Running tasks finish; nothing new starts.</span>
            <span className="spacer" /><button onClick={() => act(api.queueSetPaused(false))}><Play size={13} />Resume</button>
          </div>
        )}
        {view.cap && (
          <div className="tq-banner cap-hit" role="status">
            <Gauge size={14} aria-hidden /><span>{view.cap} Running turns finish.</span>
            <span className="spacer" /><button onClick={onUsage}>Change caps</button>
          </div>
        )}
        {limited.map(([agent, l]) => (
          <div key={agent} className="tq-banner limit" title={l.text}>
            <Clock size={14} aria-hidden /><span>
              {l.reported
                ? `${agentName(agent)} is out of usage until ${clockTime(l.until, view.now)}. Its tasks wait, or run on their fallback agent.`
                : `${agentName(agent)} is out of usage and didn't say until when. Its tasks try again at ${clockTime(l.until, view.now)}.`}
            </span>
          </div>
        ))}
        {adding && (
          <QueueForm
            repos={repos}
            initialRepo={initialRepo}
            onCancel={() => setAdding(false)}
            onAdded={(text) => { setAdding(false); onSay(text); load(); }}
            onError={(text) => onSay(text, true)}
          />
        )}
        {empty && !adding && (
          <div className="empty tq-empty">
            <ListOrdered size={24} aria-hidden />
            <p>{QUEUE_EMPTY}</p>
            <button className="primary" onClick={() => setAdding(true)}><Plus size={14} />Add task</button>
          </div>
        )}
        {g.running.length > 0 && (
          <section className="tq-section" aria-label="Running">
            <h2>Running <span className="num">{g.running.length}</span></h2>
            <div className="tq-list" role="list">{g.running.map((t) => row(t, "running"))}</div>
          </section>
        )}
        {next.length > 0 && (
          <section className="tq-section" aria-label="Up next">
            <h2>Up next <span className="num">{next.length}</span><span className="spacer" /><span className="tq-hint">Drag to reorder</span></h2>
            <div className="tq-list" role="list" onDragLeave={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node)) setDrag((d) => (d ? { ...d, over: null } : d)); }}>
              {next.map((t, i) => row(t, "next", i + 1))}
            </div>
          </section>
        )}
        {g.finished.length > 0 && (
          <section className="tq-section" aria-label="Finished">
            <h2>Finished <span className="num">{g.finished.length}</span><span className="spacer" />
              <button className="quiet" onClick={() => act(api.queueClearFinished())}>Clear finished</button>
            </h2>
            <div className="tq-list" role="list">{g.finished.map((t) => row(t, "finished"))}</div>
          </section>
        )}
      </div>
    </div>
  );
}

function QueueHead({ view, onAdd, act }: { view: QueueView | null; onAdd: (() => void) | null; act: (p: Promise<unknown>) => void }) {
  const g = view ? groups(view.tasks) : null;
  const line = !g ? "" : g.running.length + g.next.length === 0 ? "Nothing waiting." : [g.running.length && `${g.running.length} running`, g.next.length && `${g.next.length} up next`].filter(Boolean).join(", ") + ".";
  const opts = (cur: number) => [...new Set([...LIMITS, cur])].sort((a, b) => a - b);
  return (
    <div className="board-head tq-head" data-tauri-drag-region>
      <h1>Queue</h1>
      <span className="muted">{line}</span>
      <span className="spacer" />
      {view && (
        <span className="tq-limits">
          <label>Run up to <select value={view.max_total} aria-label="Tasks at once" onChange={(e) => act(api.queueSetLimits(Number(e.target.value), view.max_per_repo))}>
            {opts(view.max_total).map((n) => <option key={n} value={n}>{n}</option>)}
          </select> at once,</label>
          <label><select value={view.max_per_repo} aria-label="Tasks at once per repo" onChange={(e) => act(api.queueSetLimits(view.max_total, Number(e.target.value)))}>
            {opts(view.max_per_repo).map((n) => <option key={n} value={n}>{n}</option>)}
          </select> per repo</label>
        </span>
      )}
      {view && !view.paused && view.tasks.length > 0 && <button onClick={() => act(api.queueSetPaused(true))} title="Running tasks finish; nothing new starts"><Pause size={13} />Pause</button>}
      {onAdd && <button className="primary" onClick={onAdd}><Plus size={14} />Add task</button>}
    </div>
  );
}

type Saved = { repo?: string; agent?: Agent; mode?: PermissionMode; fallback?: Agent | ""; then?: QueueThen };

/** Add a task: the prompt, where it runs, which agent, and what happens after. */
function QueueForm({ repos, initialRepo, onCancel, onAdded, onError }: {
  repos: Repo[];
  initialRepo?: string;
  onCancel: () => void;
  onAdded: (text: string) => void;
  onError: (text: string) => void;
}) {
  // The last choices, so a night of tasks is quick to line up.
  const saved: Saved = (() => { try { return JSON.parse(localStorage.getItem("nekotrail.queueForm") || "{}"); } catch { return {}; } })();
  const [prompt, setPrompt] = useState("");
  const [repoId, setRepoId] = useState(initialRepo && repos.some((r) => r.id === initialRepo) ? initialRepo : saved.repo && repos.some((r) => r.id === saved.repo) ? saved.repo : repos[0]?.id ?? "");
  const [base, setBase] = useState("");
  const [agent, setAgent] = useState<Agent>(saved.agent && AGENTS.includes(saved.agent) ? saved.agent : "claude");
  const [model, setModel] = useState("");
  const [mode, setMode] = useState<PermissionMode>(saved.mode && MODES.includes(saved.mode) ? saved.mode : "acceptEdits");
  const [fallback, setFallback] = useState<Agent | "">(saved.fallback && AGENTS.includes(saved.fallback) ? saved.fallback : "");
  const [then, setThen] = useState<QueueThen>(saved.then ?? { checks: false, draft_pr: false, autopilot: false });
  const [branches, setBranches] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const installed = useInstalledAgents();
  const cat = useCatalog();
  const box = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { box.current?.focus(); }, []);
  useEffect(() => { if (repoId) api.repoBranches(repoId).then((b) => setBranches(b ?? [])).catch(() => setBranches([])); }, [repoId]);
  useEffect(() => { if (fallback === agent) setFallback(""); }, [agent, fallback]);
  const repo = repos.find((r) => r.id === repoId);
  const has = new Set(installed ?? []);
  const known = installed === null ? AGENTS : AGENTS.filter((a) => has.has(a) || a === agent || a === fallback);
  const missing = installed === null ? [] : AGENTS.filter((a) => !known.includes(a));
  const models = cat?.[agent]?.models ?? [];
  const alone = unattended(agent, mode);

  const submit = async () => {
    if (busy || !prompt.trim() || !repoId) return;
    if (!(await okFullAuto(mode))) return;
    setBusy(true);
    localStorage.setItem("nekotrail.queueForm", JSON.stringify({ repo: repoId, agent, mode, fallback, then } satisfies Saved));
    try {
      const t = await api.queueAdd({
        prompt: prompt.trim(), repo_id: repoId, base: base.trim() || null, agent, model: model || null, effort: null, mode,
        fallback: fallback || null, then: { ...then, draft_pr: then.draft_pr || then.autopilot },
      });
      onAdded(t.state === "waiting" ? "Added. It waits for you to start it." : "Added to the queue.");
    } catch (e) {
      onError(errText(e));
    } finally {
      setBusy(false);
    }
  };
  const agentOptions = (except?: Agent) => (
    <>
      {known.filter((a) => a !== except).map((a) => <option key={a} value={a}>{AGENT_INFO[a].name}</option>)}
      {missing.length > 0 && <optgroup label="Not installed">{missing.filter((a) => a !== except).map((a) => <option key={a} value={a}>{AGENT_INFO[a].name}</option>)}</optgroup>}
    </>
  );
  return (
    <form className="tq-form" onSubmit={(e) => { e.preventDefault(); submit(); }}
      onKeyDown={(e) => { if (e.key === "Enter" && mod(e)) { e.preventDefault(); submit(); } if (e.key === "Escape") onCancel(); }}>
      <textarea ref={box} rows={3} value={prompt} onChange={(e) => setPrompt(e.target.value)} placeholder="What should the agent do?" aria-label="Task" />
      <div className="tq-form-grid">
        <label><span>Repo</span>
          <select value={repoId} onChange={(e) => { setRepoId(e.target.value); setBase(""); }}>
            {repos.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
          </select>
        </label>
        <label><span>From branch</span>
          <input value={base} onChange={(e) => setBase(e.target.value)} placeholder={repo?.default_branch ?? "main"} list="queue-branches" spellCheck={false} />
          <datalist id="queue-branches">{branches.map((b) => <option key={b} value={b} />)}</datalist>
        </label>
        <label><span>Agent</span>
          <select value={agent} onChange={(e) => { setAgent(e.target.value as Agent); setModel(""); }}>{agentOptions()}</select>
        </label>
        <label><span>Model</span>
          <select value={model} onChange={(e) => setModel(e.target.value)}>
            <option value="">Your default</option>
            {models.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
          </select>
        </label>
        <label><span>Permissions</span>
          <select value={mode} onChange={(e) => setMode(e.target.value as PermissionMode)}>
            {MODES.filter((m) => m !== "ask" || AGENT_INFO[agent].ask).map((m) => <option key={m} value={m}>{MODE_LABEL[m]}</option>)}
          </select>
        </label>
        <label><span>If {AGENT_INFO[agent].name} runs out of usage</span>
          <select value={fallback} onChange={(e) => setFallback(e.target.value as Agent | "")}>
            <option value="">Wait for the reset</option>
            {known.filter((a) => a !== agent).map((a) => <option key={a} value={a}>Switch to {AGENT_INFO[a].name}</option>)}
          </select>
        </label>
      </div>
      <div className="tq-then" role="group" aria-label="When the agent is done">
        <span className="muted">When it's done</span>
        <label><input type="checkbox" checked={then.checks} onChange={(e) => setThen({ ...then, checks: e.target.checked })} />Run checks</label>
        <label><input type="checkbox" checked={then.draft_pr || then.autopilot} disabled={then.autopilot} onChange={(e) => setThen({ ...then, draft_pr: e.target.checked })} />Open a draft PR</label>
        <label title="Autopilot fixes failing CI, conflicts and requested changes, up to 3 agent turns"><input type="checkbox" checked={then.autopilot} onChange={(e) => setThen({ ...then, autopilot: e.target.checked })} />Autopilot to green</label>
      </div>
      {!alone && (
        <p className="tq-form-note"><Hand size={13} aria-hidden />
          {mode === "ask"
            ? "Ask before changes stops for you on every change, so this task waits until you start it."
            : `${AGENT_INFO[agent].name} asks before commands unless it's on Full auto, so this task waits until you start it.`}
        </p>
      )}
      <div className="row end">
        <button type="button" className="quiet" onClick={onCancel}>Cancel</button>
        <button type="submit" className="primary" disabled={busy || !prompt.trim() || !repoId} title={keyHint("Add to queue (⌘↵)")}>Add to queue</button>
      </div>
    </form>
  );
}
