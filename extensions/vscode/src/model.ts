// What the view, the status bar and the selection commands show. Pure: no `vscode` import, so
// every rule here is tested on its own (test/model.test.ts).

import * as path from "node:path";
import { MAX_DRAFT, MAX_EDITOR_PROMPT, type Row } from "./protocol";

/** Why the view has no rows. Each one has its own welcome text (package.json, viewsWelcome). */
export type ViewState = "offline" | "outdated" | "noFolder" | "notAdded" | "empty" | "ready";

export type Item = {
  id: string;
  label: string;
  description: string;
  tooltip: string;
  /** A codicon id. */
  icon: string;
  /** A theme colour id for the icon, or null for the default. */
  color: string | null;
  /** `workspace`, plus `.pr` when it has a PR link and `.here` when it is this window's folder. */
  contextValue: string;
  row: Row;
};

const AGENTS: Record<string, string> = { claude: "Claude", codex: "Codex", gemini: "Gemini", opencode: "OpenCode", copilot: "Copilot", cursor: "Cursor" };
export const agentName = (id: string | null | undefined) => (id ? AGENTS[id] ?? id : "");

/** Needs you first, then working, then finished and unread, then the rest; by name within each. */
function rank(r: Row): number {
  if (r.needs.length > 0) return 0;
  if (r.state === "working") return 1;
  if (r.state === "done, unread") return 2;
  return 3;
}

const same = (a: string, b: string) => path.resolve(a) === path.resolve(b);
/** Is `folder` this workspace's worktree or inside it? */
export function isInside(folder: string, root: string): boolean {
  const rel = path.relative(path.resolve(root), path.resolve(folder));
  return rel === "" || (!!rel && !rel.startsWith("..") && !path.isAbsolute(rel));
}

/** The rows as tree items, for the repo of `folder`. */
export function toItems(rows: Row[], folder: string | null): Item[] {
  return [...rows]
    .sort((a, b) => rank(a) - rank(b) || a.display.localeCompare(b.display))
    .map((r) => {
      const here = !!(folder && r.path && same(folder, r.path));
      const needs = r.needs.length > 0;
      const bits = [needs ? "needs you" : r.state, agentName(r.agent), r.pr ?? ""].filter(Boolean);
      if (here) bits.push("this window");
      const icon = needs ? "bell-dot" : r.state === "working" ? "sync~spin" : r.state === "done, unread" ? "circle-filled" : "circle-outline";
      const color = needs ? "charts.orange" : r.state === "done, unread" ? "charts.blue" : null;
      const tip = [
        r.title && r.title !== r.name ? `${r.title} (${r.name})` : r.name,
        `Branch ${r.branch}${r.target_branch ? `, compared with ${r.target_branch}` : ""}`,
        r.agent ? `Agent: ${agentName(r.agent)}` : "",
        r.pr ? `PR ${r.pr}` : "",
        needs ? `Needs you: ${r.needs.join(", ")}` : `State: ${r.state}`,
      ].filter(Boolean);
      return {
        id: r.id,
        label: r.display || r.name,
        description: bits.join(" · "),
        tooltip: tip.join("\n"),
        icon,
        color,
        contextValue: `workspace${r.pr_url ? ".pr" : ""}${here ? ".here" : ""}`,
        row: r,
      };
    });
}

/** The status bar: how many workspaces need you, across every repo. */
export function statusBar(state: "offline" | "outdated" | "ok", needCount: number): { text: string; tooltip: string } {
  if (state === "offline") return { text: "$(circle-slash) NekoTrail", tooltip: "NekoTrail isn't running. Click to start it." };
  if (state === "outdated") return { text: "$(warning) NekoTrail", tooltip: "This extension needs a newer NekoTrail. Update NekoTrail, then reopen it." };
  if (needCount === 0) return { text: "$(check) NekoTrail", tooltip: "Nothing needs you. Click to show NekoTrail." };
  const n = needCount === 1 ? "1 needs you" : `${needCount} need you`;
  return { text: `$(bell-dot) ${n}`, tooltip: `${needCount === 1 ? "1 workspace needs" : `${needCount} workspaces need`} you. Click to show NekoTrail.` };
}

/** Where the selection came from, as `src/a.ts:12` or `src/a.ts:12-20`. */
export function where(relPath: string, startLine: number, endLine: number): string {
  return startLine === endLine ? `${relPath}:${startLine}` : `${relPath}:${startLine}-${endLine}`;
}

/** A fence longer than any run of backticks in the code, so the code can't close it early. */
export function fence(code: string): string {
  const longest = Math.max(0, ...[...code.matchAll(/`+/g)].map((m) => m[0].length));
  return "`".repeat(Math.max(3, longest + 1));
}

export type Selection = { relPath: string; startLine: number; endLine: number; languageId: string; code: string };

/**
 * The selected code as a block with its file and lines. Cut to `room` characters at a line
 * boundary, saying how many lines were left out, so the result always fits.
 */
export function codeBlock(sel: Selection, room: number): string {
  const head = `From ${where(sel.relPath, sel.startLine, sel.endLine)}:`;
  if (!sel.code.trim()) return head.replace(/:$/, ".");
  const lang = /^[\w+-]+$/.test(sel.languageId) && sel.languageId !== "plaintext" ? sel.languageId : "";
  const f = fence(sel.code);
  const wrap = (code: string, note: string) => `${head}\n\n${f}${lang}\n${code}\n${f}${note}`;
  const code = sel.code.replace(/\s+$/, "");
  const whole = wrap(code, "");
  if (whole.length <= room) return whole;
  const lines = code.split("\n");
  let keep = lines.length;
  let out = whole;
  for (;;) {
    keep -= Math.max(1, Math.ceil((keep * (out.length - room)) / out.length));
    if (keep <= 0) break;
    const left = lines.length - keep;
    out = wrap(lines.slice(0, keep).join("\n"), `\n\n(${left} more ${left === 1 ? "line" : "lines"} left out to fit.)`);
    if (out.length <= room) return out;
  }
  return `From ${where(sel.relPath, sel.startLine, sel.endLine)}. (The selection was too long to include.)`;
}

/** The task for "New task from selection": what to do, then the code it's about. */
export function taskPrompt(instruction: string, sel: Selection): string {
  const ask = instruction.trim();
  return `${ask}\n\n${codeBlock(sel, MAX_EDITOR_PROMPT - ask.length - 2)}`;
}

/** The message for "Send selection to workspace chat". */
export function draftText(sel: Selection): string {
  return codeBlock(sel, MAX_DRAFT);
}

/** The workspace a file belongs to, when it's inside one of the worktrees. */
export function rowForFile(rows: Row[], file: string): Row | undefined {
  return rows.find((r) => r.path && isInside(file, r.path));
}

/** The left side of a file in the diff: nothing for files the workspace added. */
export const addedStatus = (status: string) => status === "A" || status === "?";
/** The right side: nothing for files the workspace deleted. */
export const deletedStatus = (status: string) => status === "D";
