/**
 * "Hand off to another agent": the parts of the hand-off sheet and the chat's marker lines that
 * are plain logic, kept here so they are tested. The brief itself is built by the backend
 * (src-tauri/src/handoff.rs), so the app and `nekotrail handoff` send the same one.
 */
import type { Agent, ChatEvent, PermissionMode } from "./api";
import { AGENTS, AGENT_INFO, agentName, otherAgent } from "./agents";

export type HandoffReason = "limit" | "loop" | "budget" | "asked";

/**
 * The agents a chat can go to: installed ones other than its own, in the usual order. A multi-repo
 * task only offers agents that can open its other repos.
 */
export function handoffTargets(installed: Agent[] | null, from: Agent, multiRepo = false): Agent[] {
  const have = new Set(installed ?? []);
  return AGENTS.filter((a) => a !== from && have.has(a) && (!multiRepo || !!AGENT_INFO[a].multiRepo));
}

/** The agent picked first: the one asked for, else Claude and Codex swap, else the first offered. */
export function defaultTarget(targets: Agent[], from: Agent, wanted?: Agent | null): Agent | null {
  if (wanted && targets.includes(wanted)) return wanted;
  const other = otherAgent(from);
  return targets.includes(other) ? other : targets[0] ?? null;
}

/**
 * The new chat's mode, as the backend sets it (`handoff::mode_for`): the same as the chat it
 * continues, except "Ask before changes" on an agent that can't ask, which becomes Plan. A
 * hand-off never gives an agent more room than you gave the last one.
 */
export function handoffMode(mode: PermissionMode, to: Agent): PermissionMode {
  if (mode === "ask") return AGENT_INFO[to]?.ask ? "ask" : "plan";
  return mode;
}

const MODE_LABEL: Record<PermissionMode, string> = {
  plan: "Plan (read only)", default: "Default", ask: "Ask before changes", acceptEdits: "Auto-accept edits", bypassPermissions: "Full auto",
};
export const modeLabel = (m: PermissionMode) => MODE_LABEL[m] ?? m;

/** The sheet's line about the new chat's mode. */
export function modeNote(mode: PermissionMode, to: Agent): string {
  const next = handoffMode(mode, to);
  if (next !== mode) return `${agentName(to)} can't ask before each change, so it starts in ${modeLabel(next)}. Switch its mode in the chat when you're ready.`;
  return `${agentName(to)} starts in ${modeLabel(next)}, like this chat.`;
}

/** Why the sheet is open, in one line, or null when you simply chose to hand off. */
export function reasonLine(reason: HandoffReason, from: Agent): string | null {
  const name = agentName(from);
  switch (reason) {
    case "limit": return `${name} ran out of usage. Another agent can carry on in the same workspace with what ${name} did.`;
    case "loop": return `NekoTrail stopped ${name} because it was going in circles. A fresh agent can take another look.`;
    case "budget": return `${name} stopped at a spend cap. The cap still holds for the agent you hand off to.`;
    default: return null;
  }
}

/** What the chat says where it handed off, or where it picked up. */
export function markerText(ev: Extract<ChatEvent, { kind: "handoff" }>): string {
  return ev.dir === "to" ? `Handed off to ${agentName(ev.agent)}` : `Continued from ${agentName(ev.agent)}`;
}

/** The brief's length against the soft limit: "4,812 of about 6,000 characters". */
export function briefSize(text: string, max: number): { chars: number; over: boolean; label: string } {
  const chars = [...text].length;
  return { chars, over: chars > max, label: `${chars.toLocaleString("en-US")} of about ${max.toLocaleString("en-US")} characters` };
}

/**
 * Should the chat offer a hand-off where it ends? Only when its agent stopped for a reason a
 * hand-off fixes (out of usage, going in circles, a spend cap) and you haven't written since.
 */
export function handoffOffer(events: ChatEvent[]): HandoffReason | null {
  for (let i = events.length - 1; i >= 0; i--) {
    const ev = events[i];
    if (ev.kind === "user" || ev.kind === "handoff") return null;
    if ((ev.kind === "text" || ev.kind === "error") && ev.fix === "limit") return "limit";
    if (ev.kind === "stop") return ev.reason === "budget" ? "budget" : "loop";
  }
  return null;
}
