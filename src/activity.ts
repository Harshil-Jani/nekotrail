/**
 * What the agent is doing right now, in plain words, for the working row under your message
 * ("Reading src/auth.ts · 4s"). Built only from the chat events every agent already sends: Claude,
 * Codex, Gemini and the ACP agents all map their tools to the same names (Bash, Read, Edit...),
 * see src-tauri/src/agent.rs and acp.rs. Pure, so the mapping has tests (tests/activity.test.ts).
 */

/** The fields of a chat event this file reads (a `ChatEvent` fits). */
export type ActivityEvent = { kind: string; id?: string; name?: string; input?: string; request_id?: string };

/**
 * - idle: no turn is running after your last message (it ended: Done, Stopped, a stop card).
 * - thinking: nothing to name yet, or between steps.
 * - tool: a tool call is running; `label` names it.
 * - answer: the agent's text is on screen, so the row steps aside.
 * - waiting: a tool call waits for your answer (its card asks), so the row steps aside.
 */
export type Phase = "idle" | "thinking" | "tool" | "answer" | "waiting";

/** `turn` is the index of the message that started the turn (-1 when there is none). */
export type Activity = { phase: Phase; label: string; turn: number };

/** No event for this long: the row says "Still working" (calm, no alarm). */
export const STALL_MS = 60_000;
/** Text showed, the agent is still busy and nothing new came for this long: the row comes back. */
export const ANSWER_GRACE_MS = 3_000;
const MAX_LABEL = 120;

const EDIT = new Set(["Edit", "MultiEdit", "NotebookEdit", "Write"]);
const READ = new Set(["Read"]);
// Events that say nothing about what the agent is doing now.
const QUIET = new Set(["note", "usage", "session", "guard", "guard_restored", "checks", "blocked", "rewind", "handoff"]);

/** A path as people say it: workspace-relative when `root` is given, else its last two parts. */
export function shortPath(p: string, root?: string): string {
  let s = p.trim();
  if (root) {
    const r = root.replace(/\/+$/, "");
    if (s === r) return ".";
    if (s.startsWith(`${r}/`)) return s.slice(r.length + 1);
  }
  if (s.startsWith("/") || /^[A-Za-z]:[\\/]/.test(s) || s.startsWith("~/")) {
    const parts = s.split(/[\\/]+/).filter(Boolean);
    s = parts.slice(-2).join("/");
  }
  return s;
}

function clip(s: string, max = MAX_LABEL): string {
  const one = s.replace(/\s+/g, " ").trim();
  return one.length > max ? `${one.slice(0, max - 1)}…` : one;
}

/** The command people would recognise: first line, without a leading `cd somewhere &&`. */
function command(input: string): string {
  const first = input.split("\n").find((l) => l.trim()) ?? "";
  return first.replace(/^\s*cd\s+("[^"]*"|'[^']*'|\S+)\s*(&&|;)\s*/, "").trim();
}

function host(url: string): string {
  try { return new URL(url).host || url; } catch { return url; }
}

/** The files one edit call touches (Codex sends one Edit for several files, joined by ", "). */
function targets(name: string, input: string): string[] {
  if (!input) return [];
  return name === "Edit" && input.includes(", ") ? input.split(", ").map((x) => x.trim()).filter(Boolean) : [input.trim()];
}

/** One tool call as a short phrase: "Reading src/auth.ts", "Running bun test". */
export function toolLabel(name: string, input: string, root?: string): string {
  const path = () => shortPath(input, root);
  const t = input.trim();
  switch (name) {
    case "Bash": { const c = command(input); return c ? `Running ${clip(c)}` : "Running a command"; }
    case "Read": return t ? `Reading ${path()}` : "Reading a file";
    case "Write": return t ? `Writing ${path()}` : "Writing a file";
    case "Edit": case "MultiEdit": case "NotebookEdit": {
      const files = targets(name, input);
      return files.length > 1 ? `Editing ${files.length} files` : t ? `Editing ${path()}` : "Editing a file";
    }
    case "Grep": return t ? `Searching for ${clip(t, 60)}` : "Searching the code";
    case "Glob": return t ? `Finding ${clip(t, 60)}` : "Finding files";
    case "List": case "list_directory": return t ? `Looking in ${path()}` : "Looking at the files";
    case "WebFetch": return t ? `Reading ${host(t)}` : "Reading a web page";
    case "WebSearch": return t ? `Searching the web for ${clip(t, 60)}` : "Searching the web";
    case "Task": case "Agent": return t ? `Handing a task to a helper: ${clip(t, 80)}` : "Handing a task to a helper";
    case "TodoWrite": return "Updating the to-do list";
    case "ExitPlanMode": return "Writing the plan";
    case "Delete": return t ? `Deleting ${path()}` : "Deleting a file";
    case "Move": return t ? `Moving ${path()}` : "Moving a file";
  }
  // MCP tools: mcp__server__tool. Anything else under its own name, made readable.
  const mcp = /^mcp__([^_]+(?:_[^_]+)*?)__(.+)$/.exec(name);
  if (mcp) return `Using ${mcp[1]}: ${mcp[2].replace(/_/g, " ")}`;
  const readable = name.replace(/[_-]+/g, " ").trim();
  return readable && readable !== "tool" ? `Using ${clip(readable, 60)}` : "Using a tool";
}

/**
 * What the newest turn is doing. The turn starts at your last message; Done or a stop card ends it.
 * When several calls of one kind run back to back (Claude reads three files at once, edits two),
 * it says how many: "Reading 3 files", "Editing 2 files".
 */
export function currentActivity(events: ActivityEvent[], root?: string): Activity {
  let turn = -1;
  for (let i = events.length - 1; i >= 0; i--) if (events[i].kind === "user") { turn = i; break; }
  const thinking = (): Activity => ({ phase: "thinking", label: "Thinking", turn });
  const tail = events.slice(turn + 1);
  if (tail.some((e) => e.kind === "done" || e.kind === "stop")) return { phase: "idle", label: "", turn };

  const finished = new Set<string>();
  const answered = new Set<string>();
  for (const e of tail) {
    if (e.kind === "tool_result" && e.id) finished.add(e.id);
    if (e.kind === "approval_answer" && e.request_id) answered.add(e.request_id);
  }
  const loud = tail.filter((e) => !QUIET.has(e.kind));
  const last = loud[loud.length - 1];
  if (!last) return thinking();
  if (last.kind === "text" || last.kind === "error") return { phase: "answer", label: "", turn };
  if (last.kind === "thinking") return thinking();
  if (loud.some((e) => e.kind === "approval" && !answered.has(e.request_id ?? ""))) return { phase: "waiting", label: "", turn };

  // The newest tool call still running, after the agent last spoke.
  let at = -1;
  for (let i = loud.length - 1; i >= 0; i--) {
    const e = loud[i];
    if (e.kind === "text" || e.kind === "thinking") break;
    if (e.kind === "tool" && !finished.has(e.id ?? "")) { at = i; break; }
  }
  if (at < 0) return thinking();
  const t = loud[at];
  const name = t.name ?? "tool";
  const kind = EDIT.has(name) ? "edit" : READ.has(name) ? "read" : null;
  if (kind) {
    // Edits: the run of edits it belongs to, done or not (results in between don't break the run).
    // Reads: only those still running (several at once), so a finished read isn't counted again.
    const files = new Set<string>();
    for (let i = loud.length - 1; i >= 0; i--) {
      const e = loud[i];
      if (e.kind !== "tool") { if (e.kind === "text" || e.kind === "thinking") break; continue; }
      const n = e.name ?? "";
      const same = (kind === "edit" ? EDIT : READ).has(n);
      if (same && (kind === "edit" || !finished.has(e.id ?? ""))) targets(n, e.input ?? "").forEach((f) => files.add(f));
      else if (!same && i < at) break;
    }
    if (files.size > 1) return { phase: "tool", label: `${kind === "edit" ? "Editing" : "Reading"} ${files.size} files`, turn };
  }
  return { phase: "tool", label: toolLabel(name, t.input ?? "", root), turn };
}

/** Elapsed time, short: "4s", "1m 12s", "1h 3m". */
export function elapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** What the row shows at `now`: visible or not, its words, and the time beside them. */
export type WorkingLine = { show: boolean; label: string; time: string; stalled: boolean };

/**
 * The working row at `now`, for a turn that started at `start` whose last event came at `last`.
 * Text on screen and a question waiting hide it; text followed by a quiet spell brings it back
 * (the agent went on working after it spoke). A minute with no event says "Still working".
 */
export function workingLine(a: Activity, now: number, start: number, last: number): WorkingLine {
  const time = elapsed(now - start);
  const quiet = now - last;
  if (a.phase === "idle" || a.phase === "waiting") return { show: false, label: "", time, stalled: false };
  if (quiet >= STALL_MS) return { show: true, label: "Still working", time, stalled: true };
  if (a.phase === "answer") return quiet >= ANSWER_GRACE_MS ? { show: true, label: "Thinking", time, stalled: false } : { show: false, label: "", time, stalled: false };
  return { show: true, label: a.label || "Thinking", time, stalled: false };
}

/**
 * When each turn started and when its chat last heard an event, kept for the whole window so
 * switching tabs doesn't restart the clock. `count` is the chat's event count: a new count is a
 * new event. (Events carry no times, so a turn first seen mid-way counts from when it was seen.)
 */
export class TurnClock {
  private turns = new Map<string, { start: number; last: number; count: number }>();
  /** The clock for turn `key` (chat id and turn), noting an event when `count` moved. */
  see(key: string, count: number, now: number): { start: number; last: number } {
    const t = this.turns.get(key);
    if (!t) {
      this.turns.set(key, { start: now, last: now, count });
      if (this.turns.size > 200) this.turns.delete(this.turns.keys().next().value!);
      return { start: now, last: now };
    }
    if (t.count !== count) { t.count = count; t.last = now; }
    return { start: t.start, last: t.last };
  }
  /** Set a turn's times by hand (the preview shows a long, quiet turn this way). */
  seed(key: string, start: number, last: number, count: number): void {
    this.turns.set(key, { start, last, count });
  }
}

export const turnClock = new TurnClock();
