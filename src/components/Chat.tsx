import { createContext, memo, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ShieldAlert, ShieldX, Undo2, ShieldQuestion, Split, MessageSquarePlus, AlertCircle, Brain, Check, ChevronDown, ChevronRight, ChevronUp, Circle, CircleCheck, CircleDot, ClipboardList, Copy, GitFork, History, Pencil, Paperclip, Play, Plus, FilePen, FileText, Globe, ListTodo, Lock, Search, Square, SquareTerminal, Users, Wrench, X, BookmarkPlus, Ellipsis, CircleX, ListChecks } from "lucide-react";
import { api, errText, type Agent, type ChatEvent, type Effort, type GuardHit, type GuardLevel, type JumpTo, type PermissionMode, type Session } from "../api";
import { occurrences } from "../search";
import { resumeCommand } from "../terminalImport";
import { turnLine } from "../checks";
import { Mascot } from "./Mascot";
import { Markdown } from "./Markdown";
import { ModelPicker } from "./ModelPicker";
import { McpSheet } from "./McpSheet";
import { ask } from "./Confirm";
import { getSettings } from "../settings";
import { keyHint, mod, modKey } from "../platform";
import { AGENT_INFO, agentName, otherAgent, splitAgents } from "../agents";
import { useInstalledAgents } from "../useInstalledAgents";
import { AgentMenuItems } from "./AgentMenuItems";
import { okFullAuto } from "../fullAuto";
import { fuzzy, getChangesSelection, loadPrompts, type Builtins, type PromptEntry } from "../prompts";
import { FillBar, SavePromptSheet, SlashList, usePromptFill, type SlashItem } from "./PromptMenu";

/** Rank files for an @-mention: name matches beat path matches, shorter paths win ties. */
export function matchFiles(files: string[], q: string, max = 8): string[] {
  const query = q.toLowerCase();
  const scored: [number, string][] = [];
  for (const f of files) {
    const lower = f.toLowerCase();
    const base = lower.slice(lower.lastIndexOf("/") + 1);
    let score: number;
    if (!query) score = 3;
    else if (base.startsWith(query)) score = 0;
    else if (base.includes(query)) score = 1;
    else if (lower.includes(query)) score = 2;
    else {
      // Subsequence match ("srvhttp" finds server/http.go).
      let i = 0;
      for (const ch of lower) if (ch === query[i]) i++;
      if (i < query.length) continue;
      score = 3;
    }
    scored.push([score * 1000 + f.length, f]);
  }
  return scored.sort((a, b) => a[0] - b[0]).slice(0, max).map(([, f]) => f);
}

const TOOL_ICON: Record<string, typeof Wrench> = {
  Bash: SquareTerminal, Read: FileText, Write: FilePen, Edit: FilePen, MultiEdit: FilePen, NotebookEdit: FilePen,
  Grep: Search, Glob: Search, WebFetch: Globe, WebSearch: Globe, Task: Users, Agent: Users, TodoWrite: ListTodo,
};

const MODES: { value: PermissionMode; label: string }[] = [
  { value: "plan", label: "Plan (read only)" },
  { value: "ask", label: "Ask before changes" },
  { value: "acceptEdits", label: "Auto-accept edits" },
  { value: "bypassPermissions", label: "Full auto" },
];


/** Ring that fills as the conversation uses up the model's context window. */
function ContextMeter({ tokens, window }: { tokens: number; window: number }) {
  const pct = Math.min(1, tokens / window);
  const r = 7, c = 2 * Math.PI * r;
  const tone = pct > 0.85 ? "var(--bad)" : pct > 0.7 ? "var(--wait)" : "var(--muted)";
  const k = (n: number) => (n >= 1000 ? `${Math.round(n / 1000)}k` : String(n));
  return (
    <span className="ctx" title={`Context: ${k(tokens)} of ${k(window)} tokens (${Math.round(pct * 100)}%). When it fills up, run /compact or start a new chat.`}>
      <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden>
        <circle cx="9" cy="9" r={r} fill="none" stroke="var(--line)" strokeWidth="2.5" />
        <circle cx="9" cy="9" r={r} fill="none" stroke={tone} strokeWidth="2.5" strokeDasharray={`${pct * c} ${c}`} transform="rotate(-90 9 9)" />
      </svg>
      {pct >= 0.5 && <span className="num" style={{ color: tone }}>{Math.round(pct * 100)}%</span>}
    </span>
  );
}


type Row =
  | { type: "user"; text: string; checkpoint?: string | null }
  | { type: "rewind"; text: string; undo: string }
  | { type: "text" | "thinking" | "error" | "note"; text: string; fix?: "install" | "login" | "limit" | null; agent?: Agent }
  | { type: "tool"; name: string; input: string; output?: string; is_error?: boolean; pending: boolean }
  | { type: "done"; ev: Extract<ChatEvent, { kind: "done" }> }
  | { type: "approval"; request_id: string; tool: string; input: string; description?: string; can_always: boolean; answer?: "allow" | "always" | "deny" }
  | { type: "blocked"; text: string; files: string[] }
  | { type: "guard"; files: GuardHit[]; checkpoint: string | null; ignored: string | null; level: GuardLevel; restored?: string[]; workspace_id?: string; repo?: string }
  | { type: "checks"; results: Extract<ChatEvent, { kind: "checks" }>["results"] };

/**
 * Pair each tool call with its result so one row shows both. With `rowOf`, also note for each event
 * the row it starts (-1 when it only updates an earlier row), so search can find a message's row.
 */
function toRows(events: ChatEvent[], rowOf?: number[]): Row[] {
  const rows: Row[] = [];
  const toolIdx = new Map<string, number>();
  const approvalIdx = new Map<string, number>();
  const guardIdx = new Map<string, number>();
  for (const ev of events) {
    const before = rows.length;
    rowOf?.push(before);
    switch (ev.kind) {
      case "tool":
        toolIdx.set(ev.id, rows.length);
        rows.push({ type: "tool", name: ev.name, input: ev.input, pending: true });
        break;
      case "tool_result": {
        const i = toolIdx.get(ev.id);
        if (i !== undefined) {
          const r = rows[i] as Extract<Row, { type: "tool" }>;
          rows[i] = { ...r, output: ev.output, is_error: ev.is_error, pending: false };
        }
        break;
      }
      case "done":
        rows.push({ type: "done", ev });
        break;
      case "approval":
        approvalIdx.set(ev.request_id, rows.length);
        rows.push({ type: "approval", request_id: ev.request_id, tool: ev.tool, input: ev.input, description: ev.description, can_always: ev.can_always });
        break;
      case "approval_answer": {
        const i = approvalIdx.get(ev.request_id);
        if (i !== undefined) rows[i] = { ...(rows[i] as Extract<Row, { type: "approval" }>), answer: ev.allow ? (ev.always ? "always" : "allow") : "deny" };
        break;
      }
      case "user":
        rows.push({ type: "user", text: ev.text, checkpoint: ev.checkpoint });
        break;
      case "rewind":
        rows.push({ type: "rewind", text: ev.text, undo: ev.undo });
        break;
      case "blocked":
        rows.push({ type: "blocked", text: ev.text, files: ev.files });
        break;
      case "guard":
        guardIdx.set(ev.checkpoint ?? "", rows.length);
        rows.push({ type: "guard", files: ev.files, checkpoint: ev.checkpoint, ignored: ev.ignored, level: ev.level, workspace_id: ev.workspace_id, repo: ev.repo });
        break;
      case "checks":
        rows.push({ type: "checks", results: ev.results });
        break;
      case "guard_restored": {
        const i = guardIdx.get(ev.checkpoint);
        if (i !== undefined) {
          const r = rows[i] as Extract<Row, { type: "guard" }>;
          rows[i] = { ...r, restored: [...(r.restored ?? []), ...ev.files] };
        }
        break;
      }
      default:
        rows.push("fix" in ev && ev.fix ? { type: ev.kind, text: ev.text, fix: ev.fix, agent: ev.agent } : { type: ev.kind, text: ev.text });
    }
    if (rowOf && rows.length === before) rowOf[rowOf.length - 1] = -1;
  }
  return rows;
}

/** The row showing event `index`, or the next row after it. */
export function rowForEvent(rowOf: number[], index: number, rowCount: number): number {
  for (let k = Math.max(0, index); k < rowOf.length; k++) if (rowOf[k] >= 0) return rowOf[k];
  return rowCount - 1;
}

/** What ⌘F matches in a row: its text, or a tool call's command or path. */
const findText = (r: Row): string | null => ("text" in r ? r.text : r.type === "tool" ? r.input : null);

/** Rewind handler shared through context, so memoized rows don't re-render when it changes. */
type ChatCtxValue = {
  /** Rewind files to `checkpoint`, after the confirm sheet. `prompt` is the message it was taken before, or null to undo a rewind. */
  rewind: (checkpoint: string, prompt: string | null) => Promise<void>;
  /** Checkpoints that still exist (null while loading: show them all). */
  live: Set<string> | null;
  disabled: boolean;
  /** Approve a plan: switch the chat to auto-accept edits and tell the agent to go. */
  approvePlan: () => void;
  /** Ctrl+O: show every tool call's output instead of collapsing them. */
  expandAll: boolean;
  /** Row index of the current search match, and the query to highlight. */
  query: string;
  /** Start a fresh chat with the other agent (when one is missing or out of quota). */
  switchAgent: (to: Agent) => void | Promise<void>;
  /** Start one workspace per plan step, in parallel. */
  fanOut: (plan: string, steps: string[]) => void;
  /** Put `file:line` findings from a reply into the Changes tab as review comments. */
  addFindings: (f: Finding[]) => void;
  /** Put back the protected files a turn changed, after the confirm sheet. */
  putBack: (checkpoint: string, ignored: string | null, files: GuardHit[], workspace?: string) => Promise<void>;
  /** Open the save sheet with a past message's text. */
  savePrompt: (text: string) => void;
};

export type Finding = { file: string; line: number; text: string };
/** Lines like "- src/a.ts:42: problem" (or with backticks) in an agent's reply. */
export function parseFindings(text: string): Finding[] {
  const out: Finding[] = [];
  for (const l of text.split("\n")) {
    const m = /^\s*(?:[-*]|\d+[.)])\s+\**`?([\w@./-]+\.[\w]+):(\d+)(?:-\d+)?`?\**\s*[:\-]\s*(.+)$/.exec(l);
    if (m) out.push({ file: m[1].replace(/^\.\//, ""), line: Number(m[2]), text: m[3].trim() });
  }
  return out;
}
const RewindCtx = createContext<ChatCtxValue>({ rewind: async () => {}, live: null, disabled: true, approvePlan: () => {}, expandAll: false, query: "", switchAgent: () => {}, addFindings: () => {}, fanOut: () => {}, putBack: async () => {}, savePrompt: () => {} });

const CHANGE: Record<GuardHit["change"], string> = { added: "new", changed: "changed", deleted: "deleted" };

/** A turn changed protected files: say which, and offer to put just those back. */
function GuardCard({ r }: { r: Extract<Row, { type: "guard" }> }) {
  const { putBack, disabled } = useContext(RewindCtx);
  const [busy, setBusy] = useState(false);
  const left = r.files.filter((f) => !r.restored?.includes(f.path));
  const names = r.files.map((f) => f.path);
  // The list below names the files, so the title only counts them.
  // In a multi-repo task the file may be in another repo, which has its own protected paths.
  const title = `${names.length === 1 ? "Changed a protected file" : `Changed ${names.length} protected files`}${r.repo ? ` in ${r.repo}` : ""}`;
  return (
    <div className={`guard-card ${left.length ? "" : "settled"}`}>
      <div className="fix-head"><ShieldAlert size={15} /><b>{title}</b></div>
      <ul className="guard-files">
        {r.files.map((f) => (
          <li key={f.path}>
            <code>{f.path}</code>
            {/* A file the agent created isn't put back: it goes to the Trash. */}
            <span className="muted">{r.restored?.includes(f.path) ? (f.change === "added" ? "removed" : "put back") : CHANGE[f.change]}</span>
          </li>
        ))}
      </ul>
      {left.length > 0 ? (
        <div className="row">
          <span className="muted">{r.level === "blocked" ? "It got past the block, likely through a script." : r.level === "asks" ? "It changed this without asking first, so nekotrail caught it after the turn." : "This agent can't be blocked up front, so nekotrail checks after each turn."}</span>
          <span className="spacer" />
          {r.checkpoint && (
            <button className="accent" disabled={disabled || busy} title={disabled ? "Stop the agent first" : "Restore only these files from before the turn"}
              onClick={async () => { setBusy(true); try { await putBack(r.checkpoint!, r.ignored, left, r.workspace_id); } finally { setBusy(false); } }}>
              <Undo2 size={14} />Put {left.length === 1 ? "it" : "them"} back
            </button>
          )}
        </div>
      ) : (
        <p className="muted">{r.files.length === 1 ? "The agent's version is in the Trash." : "The agent's versions are in the Trash."}</p>
      )}
    </div>
  );
}

function FindingsButton({ text }: { text: string }) {
  const { addFindings } = useContext(RewindCtx);
  const found = useMemo(() => parseFindings(text), [text]);
  const [added, setAdded] = useState(false);
  if (found.length === 0) return null;
  return (
    <button className="findings-btn" disabled={added} onClick={() => { addFindings(found); setAdded(true); }}>
      <MessageSquarePlus size={13} />{added ? `Added to Changes` : `Add ${found.length} ${found.length === 1 ? "finding" : "findings"} to review`}
    </button>
  );
}


/** An agent problem with the way out: install it, sign in, or switch agents. */
function FixCard({ fix, agent, text }: { fix: "install" | "login" | "limit"; agent: Agent; text: string }) {
  const { switchAgent } = useContext(RewindCtx);
  const name = AGENT_INFO[agent]?.full ?? "Claude Code";
  const other = otherAgent(agent);
  const install = AGENT_INFO[agent]?.install ?? AGENT_INFO.claude.install;
  const [copied, setCopied] = useState(false);
  const runInTerminal = (cmd: string) => window.dispatchEvent(new CustomEvent("nekotrail-terminal", { detail: cmd }));
  return (
    <div className="fix-card">
      <div className="fix-head"><AlertCircle size={15} />
        <b>{fix === "install" ? `${name} isn't installed` : fix === "login" ? `${name} needs you to sign in` : `${name} hit its usage limit`}</b>
      </div>
      <p className="muted">{text.split("\n")[0].slice(0, 200)}</p>
      <div className="row">
        {fix === "install" && (
          <>
            <code className="cmd-inline">{install}</code>
            <button onClick={() => { navigator.clipboard.writeText(install); setCopied(true); }}>{copied ? <Check size={13} /> : <Copy size={13} />}{copied ? "Copied" : "Copy"}</button>
            <button onClick={() => runInTerminal(install)}><SquareTerminal size={13} />Run in terminal</button>
          </>
        )}
        {fix === "login" && <button className="accent" onClick={() => runInTerminal((AGENT_INFO[agent] ?? AGENT_INFO.claude).login)}><SquareTerminal size={13} />Sign in from terminal</button>}
        {fix === "limit" && <span className="muted">The limit resets on its own. Your chat is kept.</span>}
        <span className="spacer" />
        <button className={fix === "limit" ? "accent" : "quiet"} onClick={() => switchAgent(other)}>Continue in {agentName(other)}</button>
      </div>
    </div>
  );
}

type Todo = { content: string; status: "pending" | "in_progress" | "completed"; activeForm?: string };
const parseTodos = (input: string): Todo[] | null => {
  try {
    const t = JSON.parse(input);
    return Array.isArray(t) ? t : null;
  } catch {
    return null;
  }
};

/** The plan the agent proposes in plan mode, with a button to approve it. */
/** Top-level steps of a plan: "1. Do X" lines (continuation lines stay with their step). */
export function planSteps(plan: string): string[] {
  const steps: string[] = [];
  for (const line of plan.split("\n")) {
    const m = /^(\d+)[.)]\s+(.*)$/.exec(line);
    if (m) steps.push(m[2].trim());
    else if (steps.length && /^\s{2,}\S/.test(line)) steps[steps.length - 1] += " " + line.trim();
  }
  return steps;
}

function PlanCard({ plan, latest }: { plan: string; latest: boolean }) {
  const { approvePlan, disabled, fanOut } = useContext(RewindCtx);
  const steps = planSteps(plan);
  return (
    <div className="plan-card">
      <div className="plan-head"><ClipboardList size={15} /><b>Plan</b></div>
      <div className="msg assistant"><Markdown text={plan} /></div>
      {latest && (
        <div className="row">
          <span className="muted">Approving switches this chat to auto-accept edits.</span>
          <span className="spacer" />
          {steps.length >= 2 && (
            <button disabled={disabled} onClick={() => fanOut(plan, steps)} title="One workspace and agent per step, working in parallel">
              <Split size={14} />Split into {steps.length} workspaces
            </button>
          )}
          <button className="accent" disabled={disabled} onClick={approvePlan} title={keyHint("Approve and build (⌘⇧↵)")}><Play size={14} />Approve and build</button>
        </div>
      )}
    </div>
  );
}

/** A tool call waiting for the user's OK ("Ask before changes" mode). ↵ allows, ⌫ denies. */
function ApprovalCard({ r }: { r: Extract<Row, { type: "approval" }> }) {
  // "disabled" in the chat context means an agent is running, which is when a request can be answered.
  const live = useContext(RewindCtx).disabled;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const answer = async (allow: boolean, always = false) => {
    setBusy(true);
    try { await api.answerApproval(r.request_id, allow, always); } catch (e) { setError(errText(e)); } finally { setBusy(false); }
  };
  if (r.answer) {
    return (
      <div className={`msg done approval-done ${r.answer === "deny" ? "denied" : ""}`}>
        {r.answer === "deny" ? <X size={14} /> : <Check size={14} />}
        {r.answer === "deny" ? "Denied" : r.answer === "always" ? "Allowed for this chat" : "Allowed"}: {r.tool} <code>{r.input}</code>
      </div>
    );
  }
  return (
    <div className={`approval-card ${live ? "" : "stale"}`} data-approval={live ? r.request_id : undefined}>
      <div className="plan-head"><ShieldQuestion size={15} /><b>{r.tool} wants your OK</b></div>
      <pre className="approval-input">{r.input}</pre>
      {error && <p className="muted">{error}</p>}
      {live && (
        <div className="row">
          <button className="danger" disabled={busy} onClick={() => answer(false)} title={keyHint("Deny (⌫)")}>Deny</button>
          <span className="spacer" />
          {r.can_always && <button disabled={busy} onClick={() => answer(true, true)} title="Allow this and similar changes for the rest of this chat">Allow for this chat</button>}
          <button className="accent" disabled={busy} onClick={() => answer(true)} title={keyHint("Allow (↵)")}><Check size={14} />Allow</button>
        </div>
      )}
      {!live && <p className="muted">The turn ended before this was answered.</p>}
    </div>
  );
}

/** Quiet "Rewind to here" under a message (or "Undo" on a rewind marker). The confirm sheet says
 *  what changes before anything does. Hidden once the checkpoint was pruned. */
function RewindButton({ checkpoint, prompt, label = "Rewind to here" }: { checkpoint: string; prompt: string | null; label?: string }) {
  const { rewind, disabled, live } = useContext(RewindCtx);
  const [busy, setBusy] = useState(false);
  if (live && !live.has(checkpoint)) return null;
  return (
    <button
      className="rewind"
      disabled={disabled || busy}
      title={disabled ? "Stop the agent to rewind" : prompt === null ? "Put the files back as they were right before this rewind" : "Put the files back as they were before this message"}
      onClick={async () => {
        setBusy(true);
        try { await rewind(checkpoint, prompt); } finally { setBusy(false); }
      }}
    >
      <History size={13} />
      {label}
    </button>
  );
}

/** "⋯" under your own message: save it as a prompt, or copy it. */
function MessageMenu({ text }: { text: string }) {
  const { savePrompt } = useContext(RewindCtx);
  const [open, setOpen] = useState(false);
  return (
    <span className="msg-menu">
      <button className="rewind" aria-label="More for this message" title="More" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((o) => !o)} onBlur={() => setOpen(false)}><Ellipsis size={13} /></button>
      {open && (
        <span className="menu msg-menu-list" role="menu">
          <button role="menuitem" onMouseDown={(e) => { e.preventDefault(); setOpen(false); savePrompt(text); }}><BookmarkPlus size={13} />Save as prompt…</button>
          <button role="menuitem" onMouseDown={(e) => { e.preventDefault(); setOpen(false); navigator.clipboard.writeText(text).catch(() => {}); }}><Copy size={13} />Copy text</button>
        </span>
      )}
    </span>
  );
}

/** One chat row. Memoized: a new message renders only the new row, not the whole history. */
const RowView = memo(function RowView({ r, pendingSpin, latestPlan }: { r: Row; pendingSpin: boolean; latestPlan: boolean }) {
  const { expandAll, query } = useContext(RewindCtx);
  const hit = !!query && !!findText(r)?.toLowerCase().includes(query);
  return <div className={hit ? "row-hit" : undefined}>{rowBody(r, pendingSpin, latestPlan, expandAll)}</div>;
}, (a, b) => {
  if (a.pendingSpin !== b.pendingSpin || a.latestPlan !== b.latestPlan) return false;
  const x = a.r as Record<string, unknown>, y = b.r as Record<string, unknown>;
  const keys = Object.keys(x);
  return keys.length === Object.keys(y).length && keys.every((k) => x[k] === y[k]);
});

function rowBody(r: Row, pendingSpin: boolean, latestPlan: boolean, expandAll: boolean) {
  switch (r.type) {
    case "approval":
      return <ApprovalCard r={r} />;
    case "user":
      return (
        <div className="msg-user-wrap">
          <div className="msg user">{r.text}</div>
          <span className="msg-user-actions">
            {r.checkpoint && <RewindButton checkpoint={r.checkpoint} prompt={r.text} />}
            <MessageMenu text={r.text} />
          </span>
        </div>
      );
    case "note":
      return <div className="msg note"><History size={14} />{r.text}</div>;
    case "blocked":
      return (
        <div className="msg note blocked">
          <ShieldX size={14} />
          <span>{r.text}</span>
          <span className="guard-file">{r.files.map((f) => <code key={f}>{f}</code>)}</span>
        </div>
      );
    case "guard":
      return <GuardCard r={r} />;
    case "checks": {
      // The turn summary for "Run checks after every agent turn".
      const bad = r.results.some((x) => x.status !== "passed");
      const back = r.results.filter((x) => x.regressed).map((x) => x.name);
      return (
        <div className="msg done checks-note">
          {bad ? <CircleX size={14} className="bad" /> : <ListChecks size={14} className="ok" />}
          <span>Checks: {turnLine(r.results)}</span>
          {back.length > 0 && <span className="bad-text">{back.join(" and ").replace(/^./, (c) => c.toUpperCase())} passed before and {back.length === 1 ? "fails" : "fail"} now.</span>}
          {bad && <button className="link" onClick={() => window.dispatchEvent(new CustomEvent("nekotrail-action", { detail: "checks" }))}>Open Checks</button>}
        </div>
      );
    }
    case "rewind":
      return (
        <div className="msg note rewound">
          <History size={14} />
          <span className="rewound-text">{r.text}</span>
          <RewindButton checkpoint={r.undo} prompt={null} label="Undo" />
        </div>
      );
    case "text":
      if (r.fix) return <FixCard fix={r.fix} agent={r.agent ?? "claude"} text={r.text} />;
      return (
        <div className="msg assistant">
          <Markdown text={r.text} />
          <FindingsButton text={r.text} />
        </div>
      );
    case "thinking":
      return (
        <details className="step">
          <summary><Brain size={16} /><b>Thinking</b></summary>
          <div className="body">{r.text}</div>
        </details>
      );
    case "tool": {
      // The card already says "Plan", so drop a leading "# Plan" heading from the agent's text.
      if (r.name === "ExitPlanMode" && r.input) return <PlanCard plan={r.input.replace(/^\s*#{1,3}\s*(the\s+)?plan:?\s*\n/i, "")} latest={latestPlan} />;
      if (r.name === "TodoWrite") {
        const todos = parseTodos(r.input);
        const done = todos?.filter((t) => t.status === "completed").length ?? 0;
        return <div className="msg done"><ListTodo size={14} />Todo list: {done} of {todos?.length ?? 0} done</div>;
      }
      const Icon = r.is_error ? AlertCircle : TOOL_ICON[r.name] ?? Wrench;
      return (
        <details className={`step ${r.is_error ? "failed" : ""}`} open={expandAll || undefined}>
          <summary>
            {r.pending && pendingSpin ? <span className="spinner" /> : <Icon size={16} />}
            <b>{r.name}</b> <code>{r.input}</code>
          </summary>
          {r.output ? <pre>{r.output}</pre> : null}
        </details>
      );
    }
    case "error":
      if (r.fix) return <FixCard fix={r.fix} agent={r.agent ?? "claude"} text={r.text} />;
      return <div className="msg error">{r.text}</div>;
    case "done":
      return (
        <div className="msg done num">
          {r.ev.stopped ? <Square size={14} /> : r.ev.is_error ? <X size={14} /> : <Check size={14} />}
          {r.ev.stopped ? "Stopped" : r.ev.is_error ? "Ended with an error" : "Done"}
          {r.ev.duration_ms ? ` · ${(r.ev.duration_ms / 1000).toFixed(1)}s` : ""}
          {r.ev.cost_usd ? ` · $${r.ev.cost_usd.toFixed(2)}` : ""}
        </div>
      );
  }
}

/** The rendered rows. Memoized so typing in the composer or an app refresh doesn't walk a long chat. */
const Thread = memo(function Thread({ visible, hidden, total, current, flash, busy, latestPlanIdx }: {
  visible: Row[]; hidden: number; total: number; current: number | undefined; flash: number | null; busy: boolean; latestPlanIdx: number;
}) {
  return visible.map((r, k) => (
    <div key={hidden + k} data-row={hidden + k} className={[current === hidden + k ? "row-current" : "", flash === hidden + k ? "row-flash" : ""].join(" ").trim() || undefined}>
      <RowView r={r} pendingSpin={busy && hidden + k === total - 1} latestPlan={hidden + k === latestPlanIdx} />
    </div>
  ));
});

const PAGE = 150; // rows rendered at a time; older rows load on demand
const TAIL = 600; // events fetched when a chat opens

export function Chat({
  workspaceId,
  running,
  onRunningChange,
  inject,
  append,
  jump,
  readOnly = false,
  reviewPr = 0,
  history,
}: {
  workspaceId: string;
  running: Set<string>;
  onRunningChange: () => void;
  /**
   * Text to drop into the composer (from PR buttons), for the user to review and send. With `send`
   * (Send review in Changes) it goes straight to the agent, unless the agent is busy: then it waits
   * in the composer, where Queue sends it after the current turn.
   */
  inject?: { text: string; nonce: number; send?: boolean };
  /** Text to add to the end of the draft without replacing it (Annotate in the Preview tab). */
  append?: { text: string; nonce: number };
  /** Open a chat at one message (from ⌘⇧F): load enough history, scroll there, flash it. */
  jump?: JumpTo;
  /** Archived workspaces: read the chats, but nothing can be sent, started or changed. */
  readOnly?: boolean;
  /** A review workspace for this PR: the agent only reads (the backend sends every turn in plan mode). */
  reviewPr?: number;
  /** A History workspace (read-only): the main checkout these terminal chats ran in. */
  history?: string;
}) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [events, setEvents] = useState<ChatEvent[]>([]);
  const [older, setOlder] = useState(0); // events not loaded yet
  const loadOlder = async () => {
    if (!active) return;
    anchor.current = scroller.current ? scroller.current.scrollHeight - scroller.current.scrollTop : null;
    const all = await api.sessionEvents(active);
    setEvents(all);
    setOlder(0);
    setLimit(Number.MAX_SAFE_INTEGER);
  };
  const [queue, setQueue] = useState<string[]>([]);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState<string | null>(null); // text for the "Save as prompt" sheet
  const [menu, setMenu] = useState(false);
  const [moreAgents, setMoreAgents] = useState(false);
  const installedAgents = useInstalledAgents();
  const [error, setError] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const session = sessions.find((s) => s.id === active) ?? null;
  const busy = !!active && running.has(active);
  // The + menu: Claude, Codex and installed agents (and this chat's), the rest under "More agents".
  const chatAgents = splitAgents(installedAgents, session?.agent, 6);
  const anyBusy = sessions.some((s) => running.has(s.id));

  // Which checkpoints in this chat can still be rewound to (workspaces keep their last 50).
  const checkpointKey = useMemo(() => events.flatMap((ev) => ev.kind === "user" && ev.checkpoint ? [ev.checkpoint] : ev.kind === "rewind" ? [ev.undo] : []).join(","), [events]);
  const [liveCheckpoints, setLiveCheckpoints] = useState<Set<string> | null>(null);
  useEffect(() => {
    if (!checkpointKey) return setLiveCheckpoints(null);
    let gone = false;
    api.liveCheckpoints(workspaceId, checkpointKey.split(","))
      .then((ids) => { if (!gone) setLiveCheckpoints(new Set(ids)); })
      .catch(() => { if (!gone) setLiveCheckpoints(null); });
    return () => { gone = true; };
  }, [workspaceId, checkpointKey, anyBusy]);

  // @-mentions: the file list loads the first time "@" is typed in this workspace.
  const files = useRef<string[] | null>(null);
  useEffect(() => { files.current = null; }, [workspaceId]);
  const [mention, setMention] = useState<{ q: string; start: number; items: string[]; sel: number } | null>(null);
  const updateMention = async (text: string, caret: number) => {
    const m = /(^|\s)@([^\s@]*)$/.exec(text.slice(0, caret));
    if (!m) return setMention(null);
    if (!files.current) files.current = await api.workspaceFiles(workspaceId).catch(() => []);
    const items = matchFiles(files.current, m[2]);
    setMention(items.length ? { q: m[2], start: caret - m[2].length - 1, items, sel: 0 } : null);
  };
  const pickMention = (path: string) => {
    if (!mention) return;
    const el = input.current;
    const end = mention.start + 1 + mention.q.length;
    const next = `${draft.slice(0, mention.start)}@${path} ${draft.slice(end)}`;
    setDraft(next);
    setMention(null);
    const caret = mention.start + path.length + 2;
    requestAnimationFrame(() => { el?.focus(); el?.setSelectionRange(caret, caret); });
  };

  const insertAtCaret = (text: string) => {
    const el = input.current;
    const at = el?.selectionStart ?? draft.length;
    const pad = at > 0 && !/\s$/.test(draft.slice(0, at)) ? " " : "";
    setDraft(draft.slice(0, at) + pad + text + " " + draft.slice(at));
  };
  const attach = async (list: FileList | File[]) => {
    for (const f of Array.from(list)) {
      try {
        const rel = await api.saveAttachment(workspaceId, f);
        insertAtCaret(`@${rel}`);
      } catch (e) {
        setError(errText(e));
      }
    }
  };

  const [expandAll, setExpandAll] = useState(false);
  const [search, setSearch] = useState<string | null>(null);
  const [hitIdx, setHitIdx] = useState(0);
  const approvePlan = async () => {
    if (!session || !active) return;
    await update({ permission_mode: "acceptEdits" });
    await api.send(active, "Approved. Go ahead and implement the plan.");
    onRunningChange();
  };
  // The handlers below read the latest chat through this ref, so the context value (and with it
  // every memoized row) only changes when busy, expand-all or search change, not on each new event.
  const live = useRef({ session, events, active, workspaceId, approvePlan, newChat: async (_: Agent) => {} });
  const rewindCtx = useMemo(() => ({
    // Hand the task to the other agent with the conversation so far, ready to send.
    switchAgent: async (to: Agent) => {
      const { session, events, newChat } = live.current;
      const from = agentName(session?.agent);
      const lines: string[] = [];
      for (const ev of events) {
        if (ev.kind === "user") lines.push(`User: ${ev.text}`);
        else if (ev.kind === "text" && !("fix" in ev && ev.fix)) lines.push(`${from}: ${ev.text}`);
        else if (ev.kind === "tool" && ["Edit", "Write", "MultiEdit"].includes(ev.name)) lines.push(`(${from} edited ${ev.input})`);
      }
      const transcript = lines.join("\n\n").slice(-8000);
      await newChat(to);
      setDraft(`You're taking over this task from ${from}, which had to stop. The conversation so far (latest part):\n\n${transcript}\n\nContinue from where it stopped. Check \`git status\` and \`git diff\` first to see what is already done.`);
    },
    fanOut: (plan: string, steps: string[]) => {
      const { session, workspaceId } = live.current;
      window.dispatchEvent(new CustomEvent("nekotrail-fanout", { detail: { workspaceId, agent: session?.agent ?? "claude", model: session?.model ?? null, effort: session?.effort ?? null, plan, steps } }));
    },
    addFindings: (f: Finding[]) => {
      const { session, workspaceId } = live.current;
      const by = agentName(session?.agent);
      const add = f.map((x) => ({ id: crypto.randomUUID(), file: x.file, side: "new" as const, line: x.line, end_line: x.line, code: "", text: `${by}: ${x.text}` }));
      api.addReviewComments(workspaceId, add)
        .then(() => window.dispatchEvent(new CustomEvent("nekotrail-review", { detail: workspaceId })))
        .catch((e) => setError(errText(e)));
    },
    approvePlan: () => live.current.approvePlan(),
    savePrompt: (text: string) => setSaving(text),
    putBack: async (checkpoint: string, ignored: string | null, files: GuardHit[], workspace?: string) => {
      const { active } = live.current;
      if (!active) return;
      setError(null);
      // Say what happens to each file, by name, once: restored, or (new files) moved to the Trash.
      const gone = files.filter((f) => f.change === "added").map((f) => f.path);
      const back = files.filter((f) => f.change !== "added").map((f) => f.path);
      const one = files.length === 1;
      const ok = await ask({
        title: one ? (gone.length ? "Remove the protected file the agent created?" : "Put the protected file back?") : `Put ${files.length} protected files back?`,
        body: [
          back.length ? `${back.length === 1 ? "Goes" : "Go"} back to how ${back.length === 1 ? "it was" : "they were"} before this turn: ${back.join(", ")}.` : "",
          gone.length ? `New, so ${gone.length === 1 ? "it goes" : "they go"} to the Trash: ${gone.join(", ")}.` : "",
          `Nothing else in the workspace changes. The agent's ${one ? "version goes" : "versions go"} to the Trash, so you can get ${one ? "it" : "them"} back.`,
        ].filter(Boolean).join("\n"),
        ok: one && gone.length ? "Remove" : "Put back",
      });
      if (!ok) return;
      try { await api.putBackProtected(active, checkpoint, ignored, files.map((f) => f.path), workspace); } catch (e) { setError(errText(e)); }
    },
    expandAll,
    query: (search ?? "").trim().toLowerCase(),
    disabled: anyBusy || readOnly,
    live: liveCheckpoints,
    rewind: async (checkpoint: string, prompt: string | null) => {
      const { active } = live.current;
      if (!active) return;
      setError(null);
      try {
        const { files, commits } = await api.checkpointPreview(active, checkpoint);
        if (files === 0 && commits === 0) {
          await ask({ title: "Nothing to rewind", body: prompt === null ? "Your files already match how they were before the rewind." : "Your files already match how they were before this message.", ok: "OK", notice: true });
          return;
        }
        const first = (prompt ?? "").split("\n").find((l) => l.trim())?.trim() ?? "";
        const short = first.length > 60 ? `${first.slice(0, 59)}…` : first;
        const what = `${files} ${files === 1 ? "file goes" : "files go"} back to how ${files === 1 ? "it was" : "they were"}`;
        const off = commits ? `\n${commits} ${commits === 1 ? "commit comes" : "commits come"} off the branch.` : "";
        const ok = await ask({
          title: prompt === null ? "Undo this rewind?" : "Rewind files to before this message?",
          body: `${what} ${prompt === null ? "right before the rewind" : `before "${short}"`}.${off}\nYour current files are saved first, so you can undo this. Ignored files like .env and node_modules are not touched.`,
          ok: prompt === null ? "Undo rewind" : "Rewind",
        });
        if (ok) await api.restoreCheckpoint(active, checkpoint);
      } catch (e) { setError(errText(e)); }
    },
  }), [anyBusy, expandAll, search, liveCheckpoints, readOnly]);

  // A search hit to open once its chat is showing (see `openTail`).
  const pendingJump = useRef<JumpTo | null>(null);
  const [flash, setFlash] = useState<{ row: number; nonce: number } | null>(null);

  const loadSessions = async (pick?: string) => {
    let list = await api.sessions(workspaceId);
    // A hit in a chat you closed: show that chat as a tab again (its history was kept).
    const j = pendingJump.current?.session.workspace_id === workspaceId ? pendingJump.current : null;
    if (j && !list.some((s) => s.id === j.sessionId)) list = [...list, j.session];
    if (list.length === 0) {
      if (readOnly) { setSessions([]); setActive(null); return; }
      list = [await api.newSession(workspaceId, "claude")];
    }
    setSessions(list);
    const want = pick ?? j?.sessionId;
    setActive((cur) => want ?? (cur && list.some((s) => s.id === cur) ? cur : list[list.length - 1].id));
  };

  // Declared before the workspace effect below, so a jump into another workspace is seen by its loadSessions.
  useEffect(() => {
    if (!jump || jump.session.workspace_id !== workspaceId) return;
    pendingJump.current = jump;
    setSessions((list) => (list.some((s) => s.id === jump.sessionId) ? list : [...list, jump.session]));
    if (active === jump.sessionId) openTail(jump.sessionId).catch((e) => setError(errText(e)));
    else setActive(jump.sessionId);
  }, [jump?.nonce]);

  useEffect(() => {
    setActive(null);
    loadSessions().catch((e) => setError(errText(e)));
  }, [workspaceId]);

  /**
   * Load the chat's latest events. Big chats open with the last TAIL; older ones load on request.
   * For a search hit, enough is loaded to include it, and the view scrolls to it and flashes it.
   */
  const openTail = async (sid: string) => {
    const j = pendingJump.current?.sessionId === sid ? pendingJump.current : null;
    pendingJump.current = null;
    const n = j ? Math.max(TAIL, j.total - j.index + 40) : TAIL;
    const { events, total } = await api.sessionEventsTail(sid, n);
    setEvents(events);
    setOlder(total - events.length);
    if (!j) return;
    const rowOf: number[] = [];
    const rs = toRows(events, rowOf);
    const row = rowForEvent(rowOf, j.index - (total - events.length), rs.length);
    stick.current = false;
    setLimit(Math.max(PAGE, rs.length - row + 20));
    setFlash({ row, nonce: Date.now() });
  };

  useEffect(() => {
    if (!active) return;
    setEvents([]);
    setOlder(0);
    openTail(active).catch((e) => setError(errText(e)));
    api.queued(active).then(setQueue);
    const un1 = listen<{ session_id: string; event: ChatEvent }>("agent", ({ payload }) => {
      if (payload.session_id === active) setEvents((e) => [...e, payload.event]);
    });
    const un2 = listen<{ session_id: string; queued: string[] }>("queue", ({ payload }) => {
      if (payload.session_id === active) setQueue(payload.queued);
    });
    // App already reloads its snapshot on every "running" event; only the chat tabs are ours to refresh.
    const un3 = listen("running", () => {
      api.sessions(workspaceId).then(setSessions);
    });
    const un4 = listen<{ session_id: string; context_tokens: number }>("usage", ({ payload }) => {
      if (payload.session_id === active) setLiveTokens(payload.context_tokens);
    });
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
      un3.then((f) => f());
      un4.then((f) => f());
    };
  }, [active]);

  // A git action (Create PR, Review...) messaged one of this workspace's chats: show it.
  useEffect(() => {
    const un = listen<{ workspace_id: string; session_id: string }>("focus-session", ({ payload }) => {
      if (payload.workspace_id === workspaceId) loadSessions(payload.session_id);
    });
    return () => { un.then((f) => f()); };
  }, [workspaceId]);

  const stick = useRef(true); // follow new output only while the reader is at the bottom
  useEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [events.length]);
  useEffect(() => { stick.current = true; }, [active]);

  useEffect(() => {
    if (!inject?.text) return;
    const text = inject.text;
    const toComposer = () => setDraft((d) => (d.trim() ? `${d.trimEnd()}\n\n${text}` : text));
    if (!inject.send || !active) { if (inject.send) toComposer(); else setDraft(text); return; }
    if (busy) {
      toComposer();
      setNote("The agent is busy, so your review is waiting in the message box. Press Queue to send it after this turn.");
      return;
    }
    setError(null);
    api.send(active, text).then(onRunningChange).catch((e) => { toComposer(); setError(errText(e)); });
  }, [inject?.nonce]);
  useEffect(() => {
    if (!append?.text) return;
    setDraft((d) => `${d}${d && !/\s$/.test(d) ? "\n" : ""}${append.text} `);
    requestAnimationFrame(() => { const el = input.current; if (el) { el.focus(); el.setSelectionRange(el.value.length, el.value.length); } });
  }, [append?.nonce]);

  const rows = useMemo(() => toRows(events), [events]);
  const [liveTokens, setLiveTokens] = useState<number | null>(null);
  useEffect(() => setLiveTokens(null), [active]);
  // Latest context size: live while a turn runs, else from the last finished turn.
  const context = useMemo(() => {
    for (let i = events.length - 1; i >= 0; i--) {
      const ev = events[i];
      if (ev.kind === "done" && ev.context_window) return { tokens: ev.context_tokens ?? 0, window: ev.context_window };
    }
    return null;
  }, [events]);
  const latestPlanIdx = useMemo(() => {
    for (let i = rows.length - 1; i >= 0; i--) {
      const r = rows[i];
      if (r.type === "user") return -1; // a newer message was sent after the plan
      if (r.type === "tool" && r.name === "ExitPlanMode") return i;
    }
    return -1;
  }, [rows]);
  const todos = useMemo(() => {
    for (let i = rows.length - 1; i >= 0; i--) {
      const r = rows[i];
      if (r.type === "tool" && r.name === "TodoWrite") return parseTodos(r.input);
    }
    return null;
  }, [rows]);
  // A plan or a tool call is waiting for your answer: fold the todo list so the dock never covers its buttons.
  const answerPending = latestPlanIdx >= 0 || (anyBusy && rows.some((r) => r.type === "approval" && !r.answer));
  const [todosChoice, setTodosChoice] = useState<boolean | null>(null);
  useEffect(() => { if (answerPending) setTodosChoice(null); }, [answerPending]);
  const todosOpen = todosChoice ?? !answerPending;
  const setTodosOpen = (f: (o: boolean) => boolean) => setTodosChoice(f(todosOpen));
  const hits = useMemo(() => {
    const q = (search ?? "").trim().toLowerCase();
    if (!q) return [] as number[];
    return rows.flatMap((r, i) => (findText(r)?.toLowerCase().includes(q) ? [i] : []));
  }, [rows, search]);
  // ⌘F covers the whole chat: the first search in a big chat loads the events not loaded yet
  // (only the rows near a match are drawn).
  const [findingOlder, setFindingOlder] = useState(false);
  useEffect(() => {
    if (!active || !(search ?? "").trim() || older === 0 || findingOlder) return;
    setFindingOlder(true);
    const sid = active;
    api.sessionEvents(sid)
      .then((all) => { if (live.current.active === sid) { setEvents(all); setOlder(0); } })
      .catch((e) => setError(errText(e)))
      .finally(() => setFindingOlder(false));
  }, [search, older, active]);
  useEffect(() => setHitIdx(Math.max(0, hits.length - 1)), [hits.length]);
  // Bring the current match into view (loading older rows if it is hidden).
  useEffect(() => {
    const i = hits[hitIdx];
    if (i === undefined) return;
    if (i < rows.length - limit) setLimit(rows.length - i + 20);
    requestAnimationFrame(() => scroller.current?.querySelector(`[data-row="${i}"]`)?.scrollIntoView({ block: "center" }));
  }, [hitIdx, hits]);
  const ctxWindow = context?.window ?? (session?.agent === "claude" ? 200_000 : 0);
  const ctxTokens = liveTokens ?? context?.tokens ?? 0;
  const [limit, setLimit] = useState(PAGE);
  useEffect(() => setLimit(PAGE), [active]);
  const hidden = Math.max(0, rows.length - limit);
  const visible = useMemo(() => rows.slice(hidden), [rows, hidden]);
  // A search hit just opened: bring it to the middle and flash it for a moment.
  useEffect(() => {
    if (!flash) return;
    const show = () => scroller.current?.querySelector(`[data-row="${flash.row}"]`)?.scrollIntoView({ block: "center" });
    requestAnimationFrame(show);
    const again = setTimeout(show, 250); // code blocks and diagrams can change heights after the first paint
    const t = setTimeout(() => setFlash(null), 2000);
    return () => { clearTimeout(again); clearTimeout(t); };
  }, [flash?.nonce]);
  // ⌘F: mark every occurrence in the rows on screen, the current one stronger (CSS Custom Highlights,
  // so the rendered markdown is never touched).
  useEffect(() => {
    const reg = typeof CSS !== "undefined" ? CSS.highlights : undefined;
    const q = (search ?? "").trim();
    if (!reg || typeof Highlight === "undefined") return;
    const clear = () => { reg.delete("chat-find"); reg.delete("chat-find-current"); };
    clear();
    if (!q || !scroller.current) return;
    const all: Range[] = [];
    const cur: Range[] = [];
    const current = hits[hitIdx];
    for (const el of scroller.current.querySelectorAll<HTMLElement>("[data-row] > .row-hit")) {
      const mine = Number((el.parentElement as HTMLElement).dataset.row) === current ? cur : all;
      const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        for (const [a, b] of occurrences(n.textContent ?? "", q)) {
          const r = new Range();
          r.setStart(n, a);
          r.setEnd(n, b);
          mine.push(r);
        }
      }
    }
    reg.set("chat-find", new Highlight(...all));
    reg.set("chat-find-current", new Highlight(...cur));
    return clear;
  }, [search, hits, hitIdx, visible]);
  // Keep the reader's place when older rows are added above.
  const anchor = useRef<number | null>(null);
  const showEarlier = () => {
    anchor.current = scroller.current ? scroller.current.scrollHeight - scroller.current.scrollTop : null;
    setLimit((l) => l + PAGE);
  };
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && anchor.current !== null) {
      el.scrollTop = el.scrollHeight - anchor.current;
      anchor.current = null;
    }
  }, [limit]);

  const runSlash = async (text: string): Promise<boolean> => {
    const [cmd, ...rest] = text.split(/\s+/);
    const arg = rest.join(" ");
    // Saved prompts expand into the chat box, blanks first, so you can edit before sending.
    const sn = promptFor(cmd);
    if (sn) { await insertPrompt(sn, arg); return true; }
    switch (cmd) {
      case "/clear": await newChat(session?.agent ?? "claude"); return true;
      case "/plan": await update({ permission_mode: "plan" }); return true;
      case "/auto": await update({ permission_mode: "acceptEdits" }); return true;
      case "/fork": await fork(); return true;
      case "/model":
        if (arg) await update({ model: arg });
        else window.dispatchEvent(new KeyboardEvent("keydown", { key: "/", ...modKey() }));
        return true;
      case "/effort": case "/intelligence": await update({ effort: (arg || null) as Effort | null }); return true;
      case "/mcp": setMcpOpen(true); return true;
      case "/run":
        window.dispatchEvent(new CustomEvent("nekotrail-action", { detail: arg ? `run:${arg}` : "run" }));
        return true;
      case "/review": case "/second-opinion": case "/pr": case "/merge": case "/pull": case "/fix-ci": case "/diff": case "/terminal": case "/preview": case "/checks":
        window.dispatchEvent(new CustomEvent("nekotrail-action", { detail: cmd.slice(1) }));
        return true;
      case "/add-dir": {
        if (!active || !arg) { setError("Add a folder path, e.g. /add-dir ~/code/other-repo"); return true; }
        try {
          const dirs = await api.addDir(active, arg);
          setNote(`Added. The agent can now use: ${dirs.join(", ")}`);
        } catch (e) { setError(errText(e)); }
        return true;
      }
      default: return false; // anything else (e.g. /compact, custom commands) goes to the agent
    }
  };

  const submit = async () => {
    const text = draft.trim();
    if (!text || !active) return;
    const blank = pf.missing();
    if (blank) { setNote(`Fill in "${blank}" first, or press Esc to send the text as it is.`); return; }
    pf.stop();
    setDraft("");
    setError(null);
    setSlash(null);
    if (text.startsWith("/") && (await runSlash(text))) return;
    try {
      await api.send(active, text);
      onRunningChange();
    } catch (e) {
      setError(errText(e));
    }
  };

  const update = async (patch: Partial<Pick<Session, "model" | "permission_mode" | "title" | "effort">>) => {
    if (!session) return;
    const next = { ...session, ...patch };
    if ("effort" in patch) await api.setSessionEffort(next.id, next.effort || null);
    if ("model" in patch || "permission_mode" in patch || "title" in patch || !("effort" in patch)) await api.updateSession(next.id, next.title, next.model, next.permission_mode);
    setSessions((ss) => ss.map((s) => (s.id === next.id ? next : s)));
  };

  const closeChat = async (id: string) => {
    try {
      await api.hideSession(id);
      await loadSessions(active === id ? undefined : active ?? undefined);
    } catch (e) {
      setError(errText(e));
    }
  };

  const newChat = async (agent: Agent) => {
    const s = await api.newSession(workspaceId, agent);
    await loadSessions(s.id);
  };
  live.current = { session, events, active, workspaceId, approvePlan, newChat };

  const fork = async () => {
    if (!active) return;
    try {
      const s = await api.forkSession(active);
      await loadSessions(s.id);
    } catch (e) {
      setError(errText(e));
    }
  };

  const copyTranscript = async () => {
    const md = events.flatMap((ev) =>
      ev.kind === "user" ? [`**You:** ${ev.text}`] : ev.kind === "text" ? [ev.text] : ev.kind === "tool" ? [`> ${ev.name} ${ev.input.split("\n")[0].slice(0, 120)}`] : []);
    await navigator.clipboard.writeText(md.join("\n\n"));
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  const [copied, setCopied] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => { if (note) { const t = setTimeout(() => setNote(null), 5000); return () => clearTimeout(t); } }, [note]);

  // Slash commands: typed at the start of the composer.
  // Slash commands: nekotrail's own actions first, then the repo's and your own .claude/commands.
  const BASE_SLASH: [string, string][] = [
    ["/mcp", "MCP servers and skills, and share them across agents"],
    ["/model", keyHint("Pick the model and intelligence (⌘/)")],
    ["/intelligence", "Set how hard it thinks: /intelligence high"],
    ["/plan", "Switch to plan mode, no edits"], ["/auto", "Switch to auto-accept edits"],
    ["/review", keyHint("Review the branch with this agent (⌘⇧R)")], ["/second-opinion", "Get a read-only review from the other agent"],
    ["/pr", keyHint("Commit, push and create the PR (⌘⇧P)")], ["/merge", keyHint("Merge the PR (⌘⇧M)")], ["/pull", keyHint("Merge the latest target branch (⌘⇧L)")],
    ["/fix-ci", keyHint("Fix failing checks from their logs (⌘⇧X)")],
    ["/run", keyHint("Run or stop the app (⌘R), or /run test for a named action")], ["/diff", keyHint("Show the changes (⌘⇧D)")], ["/terminal", keyHint("Open the terminal (⌘J)")],
    ["/preview", keyHint("Show the running app (⌘4)")],
    ["/checks", keyHint("Show tests, lint and typecheck results (⌘5)")],
    ["/fork", "Fork this chat into a new tab"], ["/clear", "Start a new chat"], ["/compact", "Summarise the chat to free up context"],
    ["/add-dir", "Give the agent another folder: /add-dir ~/code/api"],
  ];
  // Saved prompts: yours and the repo's, plus Claude Code's own commands in a Claude chat (Claude
  // runs those itself, so they go through as typed and are never expanded here).
  const [prompts, setPrompts] = useState<PromptEntry[]>([]);
  const reloadPrompts = () => { if (!readOnly) loadPrompts({ id: workspaceId }).then(setPrompts).catch(() => {}); };
  useEffect(reloadPrompts, [workspaceId, readOnly]);
  useEffect(() => {
    window.addEventListener("nekotrail-prompts", reloadPrompts);
    return () => window.removeEventListener("nekotrail-prompts", reloadPrompts);
  }, [workspaceId, readOnly]);
  const isBase = (c: string) => BASE_SLASH.some(([b]) => b === c);
  const usable = prompts.filter((p) => !isBase(`/${p.name}`) && (p.scope !== "claude" || (session?.agent ?? "claude") === "claude"));
  // Your and the repo's prompts first (what you saved is what you reach for), then Claude's commands, then the app's.
  const SLASH: SlashItem[] = [
    ...[...usable].sort((a, b) => Number(a.scope === "claude") - Number(b.scope === "claude"))
      .map((p) => ({ cmd: `/${p.name}`, desc: p.description || (p.scope === "claude" ? "Claude Code command" : p.text.split("\n")[0].slice(0, 80)), prompt: p })),
    ...BASE_SLASH.map(([cmd, desc]) => ({ cmd, desc })),
  ];
  const promptFor = (cmd: string) => usable.find((p) => p.scope !== "claude" && `/${p.name}` === cmd);
  const pf = usePromptFill(input, draft, setDraft);
  const insertPrompt = async (p: PromptEntry, after = "") => {
    if (p.scope === "claude") { setDraft(`/${p.name} ${after}`.trimEnd() + " "); input.current?.focus(); return; }
    const known: Builtins = {};
    if (p.segs.some((x) => x.t === "field" && x.builtin && x.name !== "selection")) {
      Object.assign(known, await api.promptContext(workspaceId).catch(() => ({})));
    }
    const sel = getChangesSelection();
    if (sel) known.selection = sel;
    pf.start(p.segs, known, after ? `\n\n${after}` : "");
  };
  const pickSlash = (it: SlashItem) => {
    setSlash(null);
    if (it.prompt) insertPrompt(it.prompt);
    else { setDraft(it.cmd + " "); input.current?.focus(); }
  };
  // ⌘K "Insert /name" lands here.
  const insertRef = useRef<(name: string) => void>(() => {});
  insertRef.current = (name: string) => {
    const p = usable.find((x) => x.name === name);
    if (p) insertPrompt(p);
  };
  useEffect(() => {
    const on = (e: Event) => insertRef.current((e as CustomEvent<string>).detail);
    window.addEventListener("nekotrail-insert-prompt", on);
    return () => window.removeEventListener("nekotrail-insert-prompt", on);
  }, []);
  const [mcpOpen, setMcpOpen] = useState(() => new URLSearchParams(location.search).has("mcp"));

  const [slash, setSlash] = useState<{ items: SlashItem[]; sel: number } | null>(null);
  const updateSlash = (text: string) => {
    const m = /^\/(\S*)$/.exec(text);
    if (!m) return setSlash(null);
    if (text === "/") reloadPrompts(); // repo prompts may have changed on disk
    const q = m[1];
    const items = SLASH.map((it, i) => ({ it, s: fuzzy(it.cmd, q, q.length >= 3 ? it.desc : ""), i }))
      .filter((x) => x.s >= 0).sort((a, b) => a.s - b.s || a.i - b.i).map((x) => x.it);
    setSlash(items.length ? { items, sel: 0 } : null);
  };

  // ⌘T new chat, ⌘F search, ⌘G next match, Ctrl+O expand all, ⌘⇧↵ approve plan.
  const keys = useRef({ newChat, approvePlan, latestPlanIdx, agent: session?.agent, readOnly });
  keys.current = { newChat, approvePlan, latestPlanIdx, agent: session?.agent, readOnly };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (scroller.current?.closest(".pane")?.hasAttribute("hidden")) return; // Chat tab not showing
      if (e.ctrlKey && !e.metaKey && e.key === "o") { e.preventDefault(); setExpandAll((x) => !x); return; }
      // ↵ allows and ⌫ denies the waiting tool call, unless you're typing.
      if ((e.key === "Enter" || e.key === "Backspace") && !mod(e) && !e.shiftKey && !keys.current.readOnly && !(e.target as HTMLElement).closest("input, textarea, select")) {
        const cards = scroller.current?.querySelectorAll<HTMLElement>("[data-approval]");
        const card = cards?.[cards.length - 1];
        if (card) { e.preventDefault(); api.answerApproval(card.dataset.approval!, e.key === "Enter"); return; }
      }
      if (!mod(e)) return;
      const k = e.key.toLowerCase();
      if (k === "t" && !e.shiftKey) { if (!keys.current.readOnly) { e.preventDefault(); keys.current.newChat(keys.current.agent ?? "claude"); } }
      else if (k === "f" && !e.shiftKey) { e.preventDefault(); setSearch((q) => q ?? ""); requestAnimationFrame(() => document.getElementById("chat-search")?.focus()); }
      else if (k === "g") { e.preventDefault(); setHitIdx((i) => (e.shiftKey ? i - 1 : i + 1)); }
      else if (e.key === "Enter" && e.shiftKey && keys.current.latestPlanIdx >= 0 && !keys.current.readOnly) { e.preventDefault(); keys.current.approvePlan(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  useEffect(() => {
    if (hits.length) setHitIdx((i) => ((i % hits.length) + hits.length) % hits.length);
  }, [hitIdx, hits.length]);

  return (
    <div className={`chat ${readOnly ? "read-only" : ""}`}>
      <div className="chat-tabs">
        {/* Only the tabs scroll sideways: the + menu sits outside the scroller, so it isn't clipped. */}
        <div className="chat-tab-strip" role="tablist" aria-label="Chats">
        {sessions.map((s) => (
          <span key={s.id} className={`chat-tab ${s.id === active ? "active" : ""}`}>
            <button role="tab" aria-selected={s.id === active} onClick={() => setActive(s.id)} title={s.title}>
              {running.has(s.id) && <span className="spinner" />}
              {s.agent !== "claude" ? `${agentName(s.agent)} · ` : ""}{s.title}
            </button>
            {sessions.length > 1 && !running.has(s.id) && !readOnly && (
              <button className="tab-close" aria-label={`Close ${s.title}`} title="Close chat. Its history is kept." onClick={() => closeChat(s.id)}><X size={12} /></button>
            )}
          </span>
        ))}
        </div>
        {!readOnly && <span className="newchat">
          <button className="icon" title={keyHint("Start a chat (⌘T)")} aria-label="Start a chat" aria-expanded={menu} onClick={() => setMenu((m) => !m)}><Plus size={15} /></button>
          {menu && (
            <span className="newchat-menu" role="menu" onMouseLeave={() => setMenu(false)}>
              {chatAgents.shown.map((a) => (
                <button key={a} role="menuitem" onClick={() => { setMenu(false); newChat(a); }}><span className="menu-icon-space" aria-hidden />Start a {AGENT_INFO[a].name} chat</button>
              ))}
              {(chatAgents.installed.length > 0 || chatAgents.missing.length > 0) && (
                <button role="menuitem" aria-expanded={moreAgents} onClick={() => setMoreAgents((m) => !m)}>
                  <ChevronRight size={13} style={{ transform: moreAgents ? "rotate(90deg)" : undefined }} />More agents
                </button>
              )}
              {moreAgents && (
                <span className="newchat-more" role="group" aria-label="More agents">
                  <AgentMenuItems split={chatAgents} known={installedAgents !== null} pick={(a) => { setMenu(false); setMoreAgents(false); newChat(a); }} label={(a) => `Start a ${AGENT_INFO[a].name} chat`} />
                </span>
              )}
              <div className="menu-sep" />
              <button role="menuitem" onClick={() => { setMenu(false); fork(); }} disabled={!events.length}><GitFork size={13} />Fork this chat</button>
              <button role="menuitem" onClick={() => { setMenu(false); copyTranscript(); }} disabled={!events.length}><Copy size={13} />Copy transcript</button>
            </span>
          )}
        </span>}
        <span className="spacer" />
        {copied && <span className="muted chat-flag">Copied</span>}
        {expandAll && <button className="quiet chat-flag" onClick={() => setExpandAll(false)} title={keyHint("Collapse tool output (⌃O)")}>Hide tool output</button>}
      </div>

      {search !== null && (
        <div className="chat-search">
          <Search size={14} />
          <input
            id="chat-search"
            value={search}
            placeholder="Find in this chat"
            aria-label="Find in this chat"
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setSearch(null);
              if (e.key === "Enter") { e.preventDefault(); setHitIdx((i) => (e.shiftKey ? i - 1 : i + 1)); }
            }}
          />
          <span className="muted num" role="status">{search.trim() ? (findingOlder ? "Searching older messages" : hits.length ? `${hitIdx + 1} of ${hits.length}` : "No matches") : ""}</span>
          <button className="icon" aria-label="Previous match" onClick={() => setHitIdx((i) => i - 1)} disabled={!hits.length}><ChevronUp size={15} /></button>
          <button className="icon" aria-label="Next match" onClick={() => setHitIdx((i) => i + 1)} disabled={!hits.length}><ChevronDown size={15} /></button>
          <button className="icon" aria-label="Close search" onClick={() => setSearch(null)}><X size={14} /></button>
        </div>
      )}

      <div
        className="messages"
        ref={scroller}
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
      >
        {rows.length === 0 && readOnly && (
          <div className="empty">
            <Mascot size={64} mood="sleep" />
            <h2>{sessions.length ? "Nothing in this chat" : "No chats here"}</h2>
            <p>{history ? "Your terminal chats from this repo's main checkout land here." : "This workspace is archived. Its branch is kept."}</p>
          </div>
        )}
        {rows.length === 0 && !readOnly && (
          <div className="empty">
            <Mascot size={64} mood="sleep" />
            <h2>{reviewPr ? `Ask about #${reviewPr}` : session?.agent_session_id ? "Pick up where you left off" : "What should we build?"}</h2>
            <p>
              {reviewPr
                ? `${agentName(session?.agent)} reads this PR's code and can't change it. Ask it to explain a change or look for a bug. Findings go to Changes as drafts, and nothing reaches GitHub until you submit.`
                : session?.agent_session_id
                ? "This chat came over from Conductor. New messages continue the same agent session."
                : `Describe a change, a bug or a plan. ${agentName(session?.agent)} works in this workspace's own branch, so nothing touches your main checkout.`}
            </p>
          </div>
        )}
        <RewindCtx.Provider value={rewindCtx}>
        <div className="thread">
          {older > 0 && hidden === 0 && (
            <button className="quiet earlier" onClick={loadOlder}>Load {older.toLocaleString()} older events</button>
          )}
          {hidden > 0 && (
            <button className="quiet earlier" onClick={showEarlier}>
              Show {Math.min(PAGE, hidden)} earlier messages ({hidden} hidden)
            </button>
          )}
          <Thread visible={visible} hidden={hidden} total={rows.length} current={hits[hitIdx]} flash={flash?.row ?? null} busy={busy} latestPlanIdx={latestPlanIdx} />
        </div>
        </RewindCtx.Provider>
      </div>

      {((todos && todos.length > 0) || queue.length > 0) && <div className="chat-dock">
      {todos && todos.length > 0 && (
        <div className="todos">
          <button className="todos-head" onClick={() => setTodosOpen((o) => !o)} aria-expanded={todosOpen}>
            <ListTodo size={14} />
            <span>Todo · {todos.filter((t) => t.status === "completed").length} of {todos.length} done</span>
            <span className="spacer" />
            {todosOpen ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
          </button>
          {todosOpen && (
            <ul>
              {todos.map((t, i) => (
                <li key={i} className={`todo ${t.status}`}>
                  {t.status === "completed" ? <CircleCheck size={14} /> : t.status === "in_progress" ? <CircleDot size={14} /> : <Circle size={14} />}
                  <span>{t.status === "in_progress" && t.activeForm ? t.activeForm : t.content}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
      {queue.length > 0 && (
        <div className="queue">
          <div className="queue-head">
            <span className="spinner" />
            <span>{queue.length} queued. They run one by one when the agent finishes.</span>
            <span className="spacer" />
            <button className="quiet" onClick={() => active && api.clearQueue(active)}>Clear queue</button>
          </div>
          {queue.map((q, i) => (
            <div className="queue-item" key={i}>
              <span className="num muted">{i + 1}</span>
              <span className="queue-text">{q}</span>
              <button className="icon" title="Move back to the composer to edit" aria-label="Edit queued message" onClick={() => { if (active) { api.removeQueued(active, i); setDraft(q); input.current?.focus(); } }}><Pencil size={13} /></button>
              <button className="icon" title="Remove from queue" aria-label="Remove queued message" onClick={() => active && api.removeQueued(active, i)}><X size={13} /></button>
            </div>
          ))}
        </div>
      )}
      </div>}
      {error && <div className="banner error">{error}</div>}
      {note && <div className="banner">{note}</div>}
      {saving !== null && <SavePromptSheet text={saving} where={{ id: workspaceId }} onClose={() => setSaving(null)} onSaved={(n) => setNote(`Saved. Type /${n} to use it.`)} />}
      {mcpOpen && session && <McpSheet workspaceId={workspaceId} agent={session.agent} onClose={() => setMcpOpen(false)} />}

      {readOnly ? (
        history ? (
          <div className="chat-readonly muted row" role="note">
            <span>This chat ran in your main checkout, so it is read-only here and nekotrail never changes that folder.</span>
            {session?.agent_session_id && (
              <button className="quiet" title="Paste it in a terminal to continue this session there." onClick={() => { navigator.clipboard.writeText(resumeCommand(session.agent, session.agent_session_id!, history)); setNote("Copied. Paste it in a terminal to continue this session."); }}>
                <Copy size={13} />Copy resume command
              </button>
            )}
          </div>
        ) : <div className="chat-readonly muted" role="note">Archived, so this chat is read-only. Restore the workspace to keep working in it.</div>
      ) : (
      <div className="composer" data-tour="chat">
        <div className="composer-box">
          {slash && <SlashList items={slash.items} sel={slash.sel} onPick={pickSlash} onHover={(i) => setSlash({ ...slash, sel: i })} />}
          {pf.fill && !slash && !mention && <FillBar fill={pf.fill} onGo={pf.go} />}
          {mention && (
            <div className="mention" role="listbox" aria-label="Files">
              {mention.items.map((f, i) => {
                const cut = f.lastIndexOf("/") + 1;
                return (
                  <button
                    key={f}
                    role="option"
                    aria-selected={i === mention.sel}
                    className={i === mention.sel ? "on" : ""}
                    onMouseDown={(e) => { e.preventDefault(); pickMention(f); }}
                  >
                    <FileText size={13} />
                    <b>{f.slice(cut)}</b>
                    <span className="muted">{f.slice(0, cut)}</span>
                  </button>
                );
              })}
            </div>
          )}
          <textarea
            ref={input}
            value={draft}
            aria-label="Message"
            placeholder={busy ? "Agent is working. Messages you send now run next." : reviewPr ? "Ask about this PR. Type @ to mention a file." : "Ask for a change, fix or plan. Type @ to mention a file, or paste a screenshot."}
            onChange={(e) => { const v = pf.change(e.target.value); setDraft(v); updateMention(v, e.target.selectionStart); if (!pf.fill) updateSlash(v); }}
            onBlur={() => { setMention(null); setSlash(null); }}
            onPaste={(e) => {
              if (e.clipboardData.files.length) {
                e.preventDefault();
                attach(e.clipboardData.files);
              }
            }}
            onKeyDown={(e) => {
              if (slash) {
                if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                  e.preventDefault();
                  const d = e.key === "ArrowDown" ? 1 : -1;
                  setSlash({ ...slash, sel: (slash.sel + d + slash.items.length) % slash.items.length });
                  return;
                }
                if (e.key === "Tab" || (e.key === "Enter" && draft.trim() !== slash.items[slash.sel].cmd)) {
                  e.preventDefault();
                  pickSlash(slash.items[slash.sel]);
                  return;
                }
                if (e.key === "Escape") { e.preventDefault(); setSlash(null); return; }
              }
              if (mention) {
                if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                  e.preventDefault();
                  const d = e.key === "ArrowDown" ? 1 : -1;
                  setMention({ ...mention, sel: (mention.sel + d + mention.items.length) % mention.items.length });
                  return;
                }
                if (e.key === "Enter" || e.key === "Tab") {
                  e.preventDefault();
                  pickMention(mention.items[mention.sel]);
                  return;
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  setMention(null);
                  return;
                }
              }
              if (pf.key(e)) return;
              const enterSends = getSettings().sendKey === "enter";
              if (e.key === "Enter" && !e.nativeEvent.isComposing && (enterSends ? !e.shiftKey && !mod(e) : e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                submit();
              }
            }}
            rows={3}
          />
          <div className="composer-bar">
            {session && (
              <>
                <span className="agent-badge">{agentName(session.agent)}</span>
                <ModelPicker
                  agent={session.agent}
                  model={session.model}
                  effort={session.effort}
                  onChange={(next) => update(next)}
                />
                {reviewPr ? (
                  <span className="chip read-only composer-ro" title="A review workspace only reads: the agent can't edit files or push."><Lock size={11} />Read only</span>
                ) : <select
                  value={session.permission_mode}
                  onChange={async (e) => { const m = e.target.value as PermissionMode; if (await okFullAuto(m)) await update({ permission_mode: m }); }}
                  aria-label="Permissions"
                  title="What the agent may do without asking"
                >
                  {MODES.filter((m) => m.value !== "ask" || AGENT_INFO[session.agent]?.ask).map((m) => <option key={m.value} value={m.value}>{m.label}</option>)}
                </select>}
                {ctxWindow > 0 && ctxTokens > 0 && <ContextMeter tokens={ctxTokens} window={ctxWindow} />}
              </>
            )}
            <button className="icon attach" title="Save as prompt" aria-label="Save the message as a prompt" disabled={!draft.trim()} onClick={() => setSaving(draft)}><BookmarkPlus size={15} /></button>
            <label className="icon attach" title="Attach files or paste a screenshot">
              <Paperclip size={15} />
              <input type="file" multiple hidden onChange={(e) => { if (e.target.files) attach(e.target.files); e.target.value = ""; }} />
            </label>
            <span className="spacer" />
            {/* One group, kept at the right edge even when the controls wrap. */}
            <span className="composer-send">
            <span className="muted" style={{ fontSize: 13 }}>{getSettings().sendKey === "enter" ? keyHint("↵") : keyHint("⌘↵")}</span>
            {busy && <button className="danger" onClick={() => active && api.stop(active)}><Square size={14} />Stop</button>}
            <button className="accent" onClick={submit} disabled={!draft.trim()}>{busy ? "Queue" : "Send"}</button>
            </span>
          </div>
        </div>
      </div>
      )}
    </div>
  );
}
