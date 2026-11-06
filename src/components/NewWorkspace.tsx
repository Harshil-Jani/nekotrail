import { useEffect, useRef, useState } from "react";
import { api as backend } from "../api";
import { ModelPicker } from "./ModelPicker";
import { ChevronDown, CornerDownLeft } from "lucide-react";
import type { Agent, PermissionMode, Repo } from "../api";
import { keyHint } from "../platform";
import { AGENTS, AGENT_INFO, splitAgents } from "../agents";
import { useInstalledAgents } from "../useInstalledAgents";
import { AgentMenuItems } from "./AgentMenuItems";
import { okFullAuto } from "../fullAuto";
import { forNewWorkspace, fuzzy, loadPrompts, type PromptEntry } from "../prompts";
import { FillBar, SlashList, usePromptFill, type SlashItem } from "./PromptMenu";
import { MAX_REPOS, MIN_REPOS, MULTI_REPO_AGENTS, multiRepoNote, togglePick } from "../multiRepo";
import { LinearPicker } from "./LinearPicker";
import { createLabel, linearFrom, type LinearIssue } from "../linear";

const MODES: { value: PermissionMode; label: string }[] = [
  { value: "plan", label: "Plan (read only)" },
  { value: "ask", label: "Ask before changes" },
  { value: "acceptEdits", label: "Auto-accept edits" },
  { value: "bypassPermissions", label: "Full auto" },
];

export type NewWorkspaceRequest = {
  /** "both" runs a bake-off: the same task in two workspaces, one per agent. */
  repoId: string; prompt: string; agent: Agent | "both"; model: string | null; effort?: string | null; mode: PermissionMode;
  /** "pr:123", "branch:name", "workspace:<id>" (stack on it) or "linear:ENG-123" (a Linear issue's branch) to start from existing work; empty for a fresh branch. */
  from?: string;
  /** A multi-repo task: 2 to 5 repos, the main one (where the chat runs) first. `repoId` is the main one. */
  repoIds?: string[];
};

export type Source = "task" | "multi" | "pr" | "branch" | "issue" | "linear" | "stack";
/** An active workspace a new one can be stacked on. */
/** `name` for the list, `text` inside a sentence (a task title comes quoted), `star` under it. */
export type StackTarget = { id: string; repo_id: string; name: string; text: string; star: string; branch: string };
type IssueItem = { number: number; title: string; body: string; url: string; labels: { name: string }[] };
type PrItem = { number: number; title: string; headRefName: string; author: { login: string }; isDraft: boolean };

/**
 * Prompt-first workspace creation: write the task, press Enter, and the agent starts in a fresh
 * worktree. The panel floats over a blurred app and can be dragged by its top bar.
 */
export function NewWorkspace({
  repos,
  initialRepo,
  initialSource = "task",
  stackTargets = [],
  initialStack,
  onCreate,
  onClose,
  onOpenSettings,
}: {
  repos: Repo[];
  initialRepo: string;
  initialSource?: Source;
  /** Active workspaces, for "Stack on a workspace". */
  stackTargets?: StackTarget[];
  /** Preselected workspace to stack on (from its ⋯ menu). */
  initialStack?: string;
  onCreate: (r: NewWorkspaceRequest) => Promise<void>;
  onClose: () => void;
  /** Settings, Integrations, from the Linear source when Linear isn't connected yet. */
  onOpenSettings?: () => void;
}) {
  // Remember the last choices so the next workspace starts the same way.
  const saved = JSON.parse(localStorage.getItem("nekotrail.new") || "{}");
  const [repoId, setRepoId] = useState(initialRepo);
  const [prompt, setPrompt] = useState("");
  const [agent, setAgent] = useState<Agent | "both">(saved.agent === "both" || AGENTS.includes(saved.agent) ? saved.agent : "claude");
  const [model, setModel] = useState<string>(saved.model ?? "");
  const [effort, setEffort] = useState<string | null>(saved.effort ?? null);
  const [mode, setMode] = useState<PermissionMode>(saved.mode ?? "acceptEdits");
  const [busy, setBusy] = useState(false);
  const [source, setSource] = useState<Source>(initialSource);
  const [prs, setPrs] = useState<PrItem[] | null>(null);
  const [prPick, setPrPick] = useState<number | null>(null);
  const [branches, setBranches] = useState<string[]>([]);
  const [branch, setBranch] = useState("");
  const [issues, setIssues] = useState<IssueItem[] | null>(null);
  const [issuePick, setIssuePick] = useState<number | null>(null);
  const [linearPick, setLinearPick] = useState<LinearIssue | null>(null);
  const [stackPick, setStackPick] = useState<string | null>(initialStack ?? null);
  // Several repos: the ones ticked, in order; the first is the main one, where the chat runs.
  const [picked, setPicked] = useState<string[]>([initialRepo]);
  const multi = source === "multi";
  useEffect(() => {
    // Only some agents can open more than one folder.
    if (multi && (agent === "both" || !MULTI_REPO_AGENTS.includes(agent))) { setAgent("claude"); setModel(""); setEffort(null); }
  }, [multi]);
  const stackable = stackTargets.filter((t) => t.repo_id === repoId);
  const stackOn = stackable.find((t) => t.id === stackPick);
  const [loadError, setLoadError] = useState<string | null>(null);
  useEffect(() => {
    setLoadError(null);
    if (source === "pr") { setPrs(null); backend.repoPrs(repoId).then(setPrs).catch((e) => { setPrs([]); setLoadError(String(e)); }); }
    if (source === "branch") backend.repoBranches(repoId).then(setBranches).catch(() => setBranches([]));
    if (source === "issue") { setIssues(null); backend.repoIssues(repoId).then(setIssues).catch((e) => { setIssues([]); setLoadError(String(e)); }); }
  }, [source, repoId]);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const drag = useRef<{ sx: number; sy: number; ox: number; oy: number } | null>(null);
  const panel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    localStorage.setItem("nekotrail.new", JSON.stringify({ agent, model, mode, effort }));
    try {
      const from = source === "pr" && prPick ? `pr:${prPick}` : source === "branch" && branch.trim() ? `branch:${branch.trim()}`
        : source === "stack" && stackOn ? `workspace:${stackOn.id}` : source === "linear" && linearPick ? linearFrom(linearPick.identifier) : undefined;
      if (multi) await onCreate({ repoId: picked[0], repoIds: picked, prompt: prompt.trim(), agent, model: model.trim() || null, effort, mode });
      else await onCreate({ repoId, prompt: prompt.trim(), agent, model: model.trim() || null, effort, mode, from });
    } finally {
      setBusy(false);
    }
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if ((e.target as HTMLElement).closest("select, button, input")) return;
    drag.current = { sx: e.clientX, sy: e.clientY, ox: pos.x, oy: pos.y };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    const r = panel.current?.getBoundingClientRect();
    if (!d || !r) return;
    // Keep at least 80px of the panel on screen in every direction.
    const maxX = window.innerWidth / 2 + r.width / 2 - 80;
    const maxY = window.innerHeight / 2 + r.height / 2 - 80;
    setPos({
      x: Math.max(-maxX, Math.min(maxX, d.ox + e.clientX - d.sx)),
      y: Math.max(-maxY, Math.min(maxY, d.oy + e.clientY - d.sy)),
    });
  };
  const onPointerUp = () => { drag.current = null; };

  const repo = repos.find((r) => r.id === repoId);
  // `/name` inserts a saved prompt (yours or this repo's), blanks first, like the chat box.
  const area = useRef<HTMLTextAreaElement>(null);
  const [prompts, setPrompts] = useState<PromptEntry[]>([]);
  useEffect(() => { loadPrompts({ repo: repoId }).then((l) => setPrompts(forNewWorkspace(l))).catch(() => setPrompts([])); }, [repoId]);
  const [slash, setSlash] = useState<{ items: SlashItem[]; sel: number } | null>(null);
  const [blankNote, setBlankNote] = useState<string | null>(null);
  const pf = usePromptFill(area, prompt, setPrompt);
  const updateSlash = (text: string) => {
    const m = /^\/(\S*)$/.exec(text);
    if (!m) return setSlash(null);
    const items = prompts.map((p, i) => ({ it: { cmd: `/${p.name}`, desc: p.description || p.text.split("\n")[0].slice(0, 80), prompt: p }, s: fuzzy(p.name, m[1], m[1].length >= 3 ? p.description : ""), i }))
      .filter((x) => x.s >= 0).sort((a, b) => a.s - b.s || a.i - b.i).map((x) => x.it);
    setSlash(items.length ? { items, sel: 0 } : null);
  };
  const pick = (it: SlashItem) => {
    setSlash(null);
    if (it.prompt) pf.start(it.prompt.segs, { base: repo?.default_branch });
  };
  const installed = useInstalledAgents();
  const split = splitAgents(installed, agent === "both" ? null : agent);
  const [moreOpen, setMoreOpen] = useState(false);
  const choose = (a: Agent | "both") => {
    // A model id belongs to one CLI, so a different agent starts on its own default.
    const cli = (x: Agent | "both") => (x === "both" ? "claude" : x);
    if (cli(a) !== cli(agent)) { setModel(""); setEffort(null); }
    setAgent(a);
  };

  return (
    <div className="nw-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div
        ref={panel}
        className="nw-panel"
        role="dialog"
        aria-modal="true"
        aria-label="New workspace"
        style={{ transform: `translate(${pos.x}px, ${pos.y}px)` }}
      >
        <div className="nw-head" onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp}>
          {multi ? (
            <span className="nw-repo nw-multi-title">{picked.length > 1 ? `${picked.length} repos` : "Several repos"}</span>
          ) : (
            <label className="nw-repo">
              <select value={repoId} onChange={(e) => setRepoId(e.target.value)} aria-label="Repo">
                {repos.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
              </select>
              <ChevronDown size={16} />
            </label>
          )}
          <span className="spacer" />
          <div className="nw-agent" role="radiogroup" aria-label="Agent">
            {(multi ? MULTI_REPO_AGENTS : [...split.shown, "both"] as (Agent | "both")[]).map((a) => (
              <button key={a} role="radio" aria-checked={agent === a} className={agent === a ? "on" : ""} onClick={() => choose(a)}
                title={a === "both" ? "Run Claude and Codex side by side, then keep the better one" : undefined}>
                {a === "both" ? "Claude + Codex" : AGENT_INFO[a].name}
              </button>
            ))}
          </div>
          {!multi && (split.installed.length > 0 || split.missing.length > 0) && (
            <span className="nw-more">
              <button className="quiet" aria-haspopup="menu" aria-expanded={moreOpen} onClick={() => setMoreOpen((o) => !o)} title="Other agents: Cursor, Copilot, Goose and more">
                More<ChevronDown size={12} />
              </button>
              {moreOpen && (
                <span className="menu nw-more-menu" role="menu" aria-label="More agents" onMouseLeave={() => setMoreOpen(false)}>
                  <AgentMenuItems split={split} known={installed !== null} pick={(a) => { setMoreOpen(false); choose(a); }} label={(a) => AGENT_INFO[a].full} />
                </span>
              )}
            </span>
          )}
        </div>
        <nav className="seg nw-source" role="tablist" aria-label="Start from">
          {([["task", "New branch"], ["multi", "Several repos"], ["issue", "Issue"], ["linear", "Linear"], ["pr", "PR"], ["branch", "Existing branch"], ["stack", "On a workspace"]] as [Source, string][]).map(([id, label]) => (
            <button key={id} role="tab" aria-selected={source === id} className={source === id ? "on" : ""} onClick={() => setSource(id)}>{label}</button>
          ))}
        </nav>
        {multi && (
          <>
            <div className="nw-list nw-repos" role="group" aria-label="Repos in this task">
              {repos.length < MIN_REPOS && <p className="muted">Add another repo first. A task across repos needs at least two.</p>}
              {repos.map((r) => {
                const at = picked.indexOf(r.id);
                const full = at < 0 && picked.length >= MAX_REPOS;
                return (
                  <div key={r.id} className={`nw-repo-row ${at >= 0 ? "on" : ""}`}>
                    <label title={full ? `A task can span up to ${MAX_REPOS} repos.` : undefined}>
                      <input type="checkbox" checked={at >= 0} disabled={full} onChange={() => setPicked((p) => togglePick(p, r.id))} />
                      <span className="nw-pr-title">{r.name}</span>
                    </label>
                    {at === 0 && <span className="chip nw-main" title="The agent's chat runs in this repo's workspace">Main</span>}
                    {at > 0 && <button className="quiet nw-make-main" onClick={() => setPicked((p) => [r.id, ...p.filter((x) => x !== r.id)])}>Make main</button>}
                  </div>
                );
              })}
            </div>
            <p className="muted nw-note">
              Each repo gets a workspace on the same branch. The agent works in the main one and can read and change the others. {multiRepoNote()}
            </p>
          </>
        )}
        {source === "pr" && (
          <div className="nw-list" role="listbox" aria-label="Open PRs">
            {prs === null && <p className="muted"><span className="spinner" /> Loading open PRs…</p>}
            {prs?.length === 0 && <p className="muted">{loadError ? `Couldn't load PRs. ${loadError}` : "No open PRs in this repo."}</p>}
            {prs?.map((p) => (
              <button key={p.number} role="option" aria-selected={prPick === p.number} className={prPick === p.number ? "on" : ""} onClick={() => setPrPick(p.number)}>
                <span className="num muted">#{p.number}</span>
                <span className="nw-pr-title">{p.title}</span>
                <span className="muted">@{p.author.login}{p.isDraft ? " · draft" : ""}</span>
              </button>
            ))}
          </div>
        )}
        {source === "issue" && (
          <div className="nw-list" role="listbox" aria-label="Open issues">
            {issues === null && <p className="muted"><span className="spinner" /> Loading open issues…</p>}
            {issues?.length === 0 && <p className="muted">{loadError ? `Couldn't load issues. ${loadError}` : "No open issues in this repo."}</p>}
            {issues?.map((i) => (
              <button key={i.number} role="option" aria-selected={issuePick === i.number} className={issuePick === i.number ? "on" : ""}
                onClick={() => {
                  setIssuePick(i.number);
                  setPrompt(`Fix issue #${i.number}: ${i.title}\n\n${(i.body || "").trim().slice(0, 4000)}\n\nWhen you open the PR, put "Fixes #${i.number}" in its description.`);
                }}>
                <span className="num muted">#{i.number}</span>
                <span className="nw-pr-title">{i.title}</span>
                <span className="muted">{i.labels.slice(0, 2).map((l) => l.name).join(", ")}</span>
              </button>
            ))}
          </div>
        )}
        {source === "linear" && (
          <LinearPicker picked={linearPick} onPick={(i) => { setLinearPick(i); setPrompt(i.seed); }} onOpenSettings={onOpenSettings} />
        )}
        {source === "stack" && (
          <>
            <div className="nw-list" role="listbox" aria-label="Workspaces to stack on">
              {stackable.length === 0 && <p className="muted">No other active workspace in this repo yet.</p>}
              {stackable.map((t) => (
                <button key={t.id} role="option" aria-selected={stackPick === t.id} className={stackPick === t.id ? "on" : ""} onClick={() => setStackPick(t.id)}>
                  <span className="nw-pr-title">{t.name}{t.star && <span className="muted nw-stack-star"> · {t.star}</span>}</span>
                  <code className="muted nw-stack-branch">{t.branch}</code>
                </button>
              ))}
            </div>
            <p className="muted nw-note">
              {stackOn
                ? `Starts from the last commit of ${stackOn.text}, and its PR goes into that branch. When the PR for ${stackOn.text} merges, one click moves this one over.`
                : "Build on work that isn't merged yet. The new PR goes into that workspace's branch, so reviewers see only the new changes."}
            </p>
          </>
        )}
        {source === "branch" && (
          <div className="nw-branch">
            <input list="nw-branches" value={branch} onChange={(e) => setBranch(e.target.value)} placeholder="Branch name, e.g. fix/login" aria-label="Branch" autoFocus />
            <datalist id="nw-branches">{(branches ?? []).map((b) => <option key={b} value={b} />)}</datalist>
          </div>
        )}
        <div className="nw-prompt-wrap">
        {slash && <SlashList items={slash.items} sel={slash.sel} onPick={pick} onHover={(i) => setSlash({ ...slash, sel: i })} label="Prompts" />}
        {pf.fill && !slash && <FillBar fill={pf.fill} onGo={pf.go} />}
        <textarea
          ref={area}
          className={`nw-prompt ${source === "pr" || source === "branch" || source === "stack" ? "short" : ""}`}
          autoFocus
          value={prompt}
          onChange={(e) => { const v = pf.change(e.target.value); setPrompt(v); setBlankNote(null); if (!pf.fill) updateSlash(v); }}
          onBlur={() => setSlash(null)}
          onKeyDown={(e) => {
            if (slash) {
              if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                e.preventDefault();
                const d = e.key === "ArrowDown" ? 1 : -1;
                setSlash({ ...slash, sel: (slash.sel + d + slash.items.length) % slash.items.length });
                return;
              }
              if (e.key === "Tab" || e.key === "Enter") { e.preventDefault(); pick(slash.items[slash.sel]); return; }
              if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); setSlash(null); return; }
            }
            if (e.key === "Escape" && pf.fill) { e.stopPropagation(); }
            if (pf.key(e)) return;
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              const blank = pf.missing();
              if (blank) { setBlankNote(`Fill in "${blank}" first, or press Esc to keep the text as it is.`); return; }
              pf.stop();
              submit();
            }
          }}
          placeholder={multi ? "What should the agent change across these repos? Type / for a saved prompt."
            : source === "task" || source === "issue" || source === "linear" ? `What should the agent do in ${repo?.name ?? "this repo"}? Type / for a saved prompt. Leave empty for an empty workspace.` : "Optional first task, e.g. address the review comments"}
          aria-label="What the agent should do"
        />
        {blankNote && <p className="muted nw-note" role="status">{blankNote}</p>}
        </div>
        <div className="nw-foot">
          <ModelPicker
            agent={agent === "both" ? "claude" : agent}
            model={model || null}
            effort={effort}
            onChange={(n) => { setModel(n.model ?? ""); setEffort(n.effort); }}
            compact
          />
          <select value={mode} onChange={async (e) => { const m = e.target.value as PermissionMode; if (await okFullAuto(m)) setMode(m); }} aria-label="Permissions">
            {MODES.filter((m) => m.value !== "ask" || (agent !== "both" && AGENT_INFO[agent].ask)).map((m) => <option key={m.value} value={m.value}>{m.label}</option>)}
          </select>
          <span className="spacer" />
          <span className="muted nw-hint">{keyHint("⇧↵ new line · Esc to close")}</span>
          <button className="primary" onClick={submit} disabled={busy || (multi ? picked.length < MIN_REPOS : !repo) || (source === "pr" && !prPick) || (source === "branch" && !branch.trim()) || (source === "issue" && !issuePick) || (source === "linear" && !linearPick) || (source === "stack" && !stackOn)}>
            {busy ? <span className="spinner" /> : null}
            {multi ? (picked.length < MIN_REPOS ? "Pick at least two repos" : `Create task in ${picked.length} repos`) : source === "issue" ? (issuePick ? `Create from #${issuePick}` : "Pick an issue") : source === "linear" ? createLabel(linearPick?.identifier ?? null) : source === "pr" ? (prPick ? `Create from #${prPick}` : "Pick a PR") : source === "branch" ? "Create from branch" : source === "stack" ? (stackOn ? "Create stacked workspace" : "Pick a workspace") : prompt.trim() ? "Create workspace" : "Create empty workspace"}
            <CornerDownLeft size={14} />
          </button>
        </div>
      </div>
    </div>
  );
}
