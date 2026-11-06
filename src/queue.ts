import type { Agent, PermissionMode } from "./api";
import { agentName } from "./agents";

/**
 * The task queue's types and the words the Queue view shows. The rules (what starts when, usage
 * limits, the fallback agent) live in the backend, src-tauri/src/queue.rs.
 */

export type QueueState = "queued" | "waiting" | "starting" | "running" | "then" | "done" | "needs_you" | "failed";
export type QueueThen = { checks: boolean; draft_pr: boolean; autopilot: boolean };

export type QueueTask = {
  id: string;
  prompt: string;
  repo_id: string;
  base: string | null;
  agent: Agent;
  model: string | null;
  effort: string | null;
  mode: PermissionMode;
  fallback: Agent | null;
  then: QueueThen;
  state: QueueState;
  /** What happened, in plain words. */
  note: string;
  /** "setup", "checks", "pr" or "autopilot" while that part runs. */
  step: string;
  workspace_id: string | null;
  session_id: string | null;
  /** The agent that ran it: the fallback when the first was out of usage. */
  ran_with: Agent | null;
  resume: boolean;
  attempts: number;
  /** Epoch seconds. */
  added_at: number;
  started_at: number | null;
  ended_at: number | null;
};

export type QueueWait =
  | { kind: "paused" }
  | { kind: "limit"; agent: Agent; until: number; reported: boolean }
  | { kind: "probe"; agent: Agent }
  | { kind: "slots" }
  | { kind: "repo_slots" };

export type QueueLimit = { until: number; reported: boolean; text: string };

export type QueueView = {
  tasks: QueueTask[];
  paused: boolean;
  max_total: number;
  max_per_repo: number;
  /** Agents out of usage right now. */
  limits: Record<string, QueueLimit>;
  /** Why each queued task hasn't started. */
  waits: Record<string, QueueWait>;
  /** Tasks whose agent waits for your OK on a command. */
  asking: string[];
  now: number;
};

export type QueueSummary = { done: number; needs_you: number; failed: number; running: number; queued: number; text: string };

export type NewQueueTask = {
  prompt: string; repo_id: string; base: string | null; agent: Agent; model: string | null; effort: string | null;
  mode: PermissionMode; fallback: Agent | null; then: QueueThen;
};

export const MODE_LABEL: Record<string, string> = {
  plan: "Plan (read only)",
  ask: "Ask before changes",
  acceptEdits: "Auto-accept edits",
  bypassPermissions: "Full auto",
};

/** The first line of the prompt, short. */
export function taskTitle(prompt: string): string {
  const line = prompt.split("\n").map((l) => l.trim()).find(Boolean) ?? "";
  return line.length > 80 ? `${line.slice(0, 77)}...` : line;
}

/** "3:10 AM", with the day when it isn't within the next 24 hours ("Sat 3:10 AM"). */
export function clockTime(epoch: number, now: number, locale?: string): string {
  const d = new Date(epoch * 1000);
  const time = d.toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit" });
  return epoch - now > 24 * 3600 || epoch < now - 12 * 3600 ? `${d.toLocaleDateString(locale, { weekday: "short" })} ${time}` : time;
}

/** "12 min", "1 h 5 min". */
export function elapsed(from: number, to: number): string {
  const m = Math.max(0, Math.floor((to - from) / 60));
  if (m < 1) return "just now";
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return m % 60 ? `${h} h ${m % 60} min` : `${h} h`;
}

/** Why a queued task hasn't started, as one short line. */
export function waitText(w: QueueWait | undefined, view: Pick<QueueView, "max_total" | "max_per_repo" | "now">, locale?: string): string {
  if (!w) return "Starting";
  switch (w.kind) {
    case "paused": return "Paused";
    case "limit": return w.reported
      ? `${agentName(w.agent)} is out of usage until ${clockTime(w.until, view.now, locale)}`
      : `${agentName(w.agent)} is out of usage. Trying again at ${clockTime(w.until, view.now, locale)}`;
    case "probe": return `One task checks that ${agentName(w.agent)} has usage again first`;
    case "slots": return `Waits for a free slot. ${view.max_total} run at once`;
    case "repo_slots": return `Waits for a slot in this repo. ${view.max_per_repo} run at once per repo`;
  }
}

/** What a task is doing, for its row. */
export function stateText(t: QueueTask, asking: boolean): string {
  switch (t.state) {
    case "queued": return "Up next";
    case "waiting": return "Waits for you to start it";
    case "starting": return "Making its workspace";
    case "running": return t.step === "setup" ? "Running setup" : asking ? "Waiting for your OK" : "Working";
    case "then": return t.step === "checks" ? "Running checks" : t.step === "pr" ? "Opening a draft PR" : "Autopilot";
    case "done": return "Done";
    case "needs_you": return "Needs you";
    case "failed": return "Failed";
  }
}

/** "then checks, draft PR". Empty when nothing follows. */
export function thenText(then: QueueThen): string {
  const steps = [then.checks && "checks", (then.draft_pr || then.autopilot) && "draft PR", then.autopilot && "autopilot"].filter(Boolean);
  return steps.length ? `then ${steps.join(", ")}` : "";
}

/** "8 done, 2 need you, 1 failed". Twin of `Summary::text` in queue.rs. */
export function summaryText(s: Pick<QueueSummary, "done" | "needs_you" | "failed">): string {
  return [
    s.done && `${s.done} done`,
    s.needs_you && `${s.needs_you} ${s.needs_you === 1 ? "needs" : "need"} you`,
    s.failed && `${s.failed} failed`,
  ].filter(Boolean).join(", ");
}

/** The line under the summary: what is still going. Empty when nothing is. */
export function stillGoing(s: Pick<QueueSummary, "running" | "queued">): string {
  const parts = [s.running && `${s.running} still running`, s.queued && `${s.queued} in line`].filter(Boolean);
  return parts.length ? `${parts.join(", ")}.` : "";
}

/** The three lists of the Queue view: running (incl. its steps), up next in order, finished newest first. */
export function groups(tasks: QueueTask[]) {
  return {
    running: tasks.filter((t) => t.state === "starting" || t.state === "running" || t.state === "then"),
    next: tasks.filter((t) => t.state === "queued" || t.state === "waiting"),
    finished: tasks.filter((t) => t.state === "done" || t.state === "needs_you" || t.state === "failed")
      .sort((a, b) => (b.ended_at ?? 0) - (a.ended_at ?? 0)),
  };
}

/** Counts for the sidebar row. */
export function counts(tasks: QueueTask[]) {
  const g = groups(tasks);
  return { running: g.running.length, next: g.next.length, needs: g.finished.filter((t) => t.state !== "done").length };
}

/**
 * Drag and drop in Up next: where `dragged` lands when dropped on `target`'s row, as the
 * backend's "before" id. Dropping on the lower half of a row puts it after that row.
 */
export function dropBefore(next: string[], dragged: string, target: string, lowerHalf: boolean): string | null | undefined {
  if (dragged === target) return undefined;
  const rest = next.filter((id) => id !== dragged);
  const i = rest.indexOf(target);
  if (i < 0) return undefined;
  const at = lowerHalf ? i + 1 : i;
  return at < rest.length ? rest[at] : null;
}

/** The order Up next shows after moving `id` before `before` (null: last). Same rule as `move_before`. */
export function moved(next: string[], id: string, before: string | null): string[] {
  const rest = next.filter((x) => x !== id);
  const at = before === null ? rest.length : rest.indexOf(before);
  return at < 0 ? next : [...rest.slice(0, at), id, ...rest.slice(at)];
}

/** Agents that ask before commands outside Full auto (the ACP ones). Claude, Codex and Gemini don't. */
export const asksBeforeCommands = (a: Agent) => !["claude", "codex", "gemini"].includes(a);

/** Can this agent run in this mode with nobody there? Twin of `queue::unattended`. */
export function unattended(agent: Agent, mode: PermissionMode): boolean {
  if (mode === "ask") return false;
  if (mode === "bypassPermissions") return true;
  return !asksBeforeCommands(agent);
}
