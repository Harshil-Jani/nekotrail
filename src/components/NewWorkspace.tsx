import { useEffect, useRef, useState } from "react";
import { api as backend } from "../api";
import { ModelPicker } from "./ModelPicker";
import { ChevronDown, CornerDownLeft } from "lucide-react";
import type { Agent, PermissionMode, Repo } from "../api";
import { AGENTS, AGENT_INFO } from "../agents";

const MODES: { value: PermissionMode; label: string }[] = [
  { value: "plan", label: "Plan (read only)" },
  { value: "ask", label: "Ask before changes" },
  { value: "acceptEdits", label: "Auto-accept edits" },
  { value: "bypassPermissions", label: "Full auto" },
];

export type NewWorkspaceRequest = {
  /** "both" runs a bake-off: the same task in two workspaces, one per agent. */
  repoId: string; prompt: string; agent: Agent | "both"; model: string | null; effort?: string | null; mode: PermissionMode;
  /** "pr:123", "branch:name" or "workspace:<id>" (stack on it) to start from existing work; empty for a fresh branch. */
  from?: string;
};

export type Source = "task" | "pr" | "branch" | "issue" | "stack";
/** An active workspace a new one can be stacked on. */
export type StackTarget = { id: string; repo_id: string; name: string; branch: string };
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
}) {
  // Remember the last choices so the next workspace starts the same way.
  const saved = JSON.parse(localStorage.getItem("nekotrail.new") || "{}");
  const [repoId, setRepoId] = useState(initialRepo);
  const [prompt, setPrompt] = useState("");
  const [agent, setAgent] = useState<Agent | "both">(saved.agent ?? "claude");
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
  const [stackPick, setStackPick] = useState<string | null>(initialStack ?? null);
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
        : source === "stack" && stackOn ? `workspace:${stackOn.id}` : undefined;
      await onCreate({ repoId, prompt: prompt.trim(), agent, model: model.trim() || null, effort, mode, from });
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
          <label className="nw-repo">
            <select value={repoId} onChange={(e) => setRepoId(e.target.value)} aria-label="Repo">
              {repos.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
            </select>
            <ChevronDown size={16} />
          </label>
          <span className="spacer" />
          <div className="nw-agent" role="radiogroup" aria-label="Agent">
            {([...AGENTS, "both"] as (Agent | "both")[]).map((a) => (
              <button key={a} role="radio" aria-checked={agent === a} className={agent === a ? "on" : ""} onClick={() => {
                // A model id belongs to one CLI, so a different agent starts on its own default.
                const cli = (x: Agent | "both") => (x === "both" ? "claude" : x);
                if (cli(a) !== cli(agent)) { setModel(""); setEffort(null); }
                setAgent(a);
              }}
                title={a === "both" ? "Run Claude and Codex side by side, then keep the better one" : undefined}>
                {a === "both" ? "Claude + Codex" : AGENT_INFO[a].name}
              </button>
            ))}
          </div>
        </div>
        <nav className="seg nw-source" role="tablist" aria-label="Start from">
          {([["task", "New branch"], ["issue", "Issue"], ["pr", "PR"], ["branch", "Existing branch"], ["stack", "On a workspace"]] as [Source, string][]).map(([id, label]) => (
            <button key={id} role="tab" aria-selected={source === id} className={source === id ? "on" : ""} onClick={() => setSource(id)}>{label}</button>
          ))}
        </nav>
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
        {source === "stack" && (
          <>
            <div className="nw-list" role="listbox" aria-label="Workspaces to stack on">
              {stackable.length === 0 && <p className="muted">No other active workspace in this repo yet.</p>}
              {stackable.map((t) => (
                <button key={t.id} role="option" aria-selected={stackPick === t.id} className={stackPick === t.id ? "on" : ""} onClick={() => setStackPick(t.id)}>
                  <span className="nw-pr-title">{t.name}</span>
                  <code className="muted nw-stack-branch">{t.branch}</code>
                </button>
              ))}
            </div>
            <p className="muted nw-note">
              {stackOn
                ? `Starts from ${stackOn.name}'s last commit, and its PR goes into ${stackOn.name}'s branch. When ${stackOn.name}'s PR merges, one click moves this one over.`
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
        <textarea
          className={`nw-prompt ${source === "pr" || source === "branch" || source === "stack" ? "short" : ""}`}
          autoFocus
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              submit();
            }
          }}
          placeholder={source === "task" || source === "issue" ? `What should the agent do in ${repo?.name ?? "this repo"}? Leave empty for an empty workspace.` : "Optional first task, e.g. address the review comments"}
          aria-label="What the agent should do"
        />
        <div className="nw-foot">
          <ModelPicker
            agent={agent === "both" ? "claude" : agent}
            model={model || null}
            effort={effort}
            onChange={(n) => { setModel(n.model ?? ""); setEffort(n.effort); }}
            compact
          />
          <select value={mode} onChange={(e) => setMode(e.target.value as PermissionMode)} aria-label="Permissions">
            {MODES.filter((m) => m.value !== "ask" || (agent !== "both" && AGENT_INFO[agent].ask)).map((m) => <option key={m.value} value={m.value}>{m.label}</option>)}
          </select>
          <span className="spacer" />
          <span className="muted nw-hint">⇧↵ new line · Esc to close</span>
          <button className="primary" onClick={submit} disabled={busy || !repo || (source === "pr" && !prPick) || (source === "branch" && !branch.trim()) || (source === "issue" && !issuePick) || (source === "stack" && !stackOn)}>
            {busy ? <span className="spinner" /> : null}
            {source === "issue" ? (issuePick ? `Create from #${issuePick}` : "Pick an issue") : source === "pr" ? (prPick ? `Create from #${prPick}` : "Pick a PR") : source === "branch" ? "Create from branch" : source === "stack" ? (stackOn ? `Stack on ${stackOn.name}` : "Pick a workspace") : prompt.trim() ? "Create workspace" : "Create empty workspace"}
            <CornerDownLeft size={14} />
          </button>
        </div>
      </div>
    </div>
  );
}
