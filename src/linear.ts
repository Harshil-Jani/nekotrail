/**
 * Linear issues as a place to start a workspace from. The key, the queries and the first
 * message are all in `src-tauri/src/linear.rs`; this file only has the shapes and the words.
 */

export type LinearAccount = { name: string; org: string };
/** What Settings and New workspace need to know. Never contains the key. */
export type LinearStatus = { connected: boolean; account: LinearAccount | null; move_started: boolean; last_error: string | null };
export type LinearTeam = { id: string; key: string; name: string };
export type LinearIssue = {
  id: string;
  identifier: string;
  title: string;
  description: string;
  url: string;
  /** Linear's branch name for it; the workspace uses it. */
  branch: string;
  state: string;
  /** triage, backlog, unstarted, started, completed or canceled. */
  state_type: string;
  team: LinearTeam;
  assignee: string | null;
  labels: string[];
  /** 0 none, 1 urgent, 2 high, 3 medium, 4 low. */
  priority: number;
  updated_at: string;
  /** The first message a workspace from this issue starts with. */
  seed: string;
};

export type LinearStateFilter = "open" | "backlog" | "unstarted" | "started" | "all";
export type LinearFilter = { anyone: boolean; team: string | null; state: LinearStateFilter; query: string };

export const DEFAULT_FILTER: LinearFilter = { anyone: false, team: null, state: "open", query: "" };

export const STATE_FILTERS: [LinearStateFilter, string][] = [
  ["open", "Open"],
  ["backlog", "Backlog"],
  ["unstarted", "Todo"],
  ["started", "In progress"],
  ["all", "Any state"],
];

const PRIORITY = ["", "Urgent", "High", "Medium", "Low"];

/** "Ada Lovelace, Acme", or whichever half there is. */
export function accountLine(a: LinearAccount | null | undefined): string {
  if (!a) return "";
  return [a.name, a.org].map((x) => x.trim()).filter(Boolean).join(", ");
}

/** The muted line under an issue: its state, an urgent or high priority, and up to two labels. */
export function issueMeta(i: Pick<LinearIssue, "state" | "priority" | "labels">): string {
  const p = i.priority === 1 || i.priority === 2 ? PRIORITY[i.priority] : "";
  return [i.state, p, i.labels.slice(0, 2).join(", ")].filter(Boolean).join(" · ");
}

/** Worth sending to Linear: the shape of a personal API key. Linear has the final say. */
export function looksLikeKey(raw: string): boolean {
  const k = raw.trim().replace(/^bearer\s+/i, "");
  return /^lin_api_[A-Za-z0-9_]{12,}$/.test(k);
}

/** "ENG-123" from what was typed or pasted (any case, or a linear.app issue link), else null. */
export function issueRef(raw: string): string | null {
  let t = raw.trim();
  const link = /linear\.app\/[^/]+\/issue\/([^/?#]+)/i.exec(t);
  if (link) t = link[1];
  const m = /^([A-Za-z][A-Za-z0-9]{0,9})-([1-9][0-9]{0,6})$/.exec(t);
  return m ? `${m[1].toUpperCase()}-${m[2]}` : null;
}

/** What the list says when there is nothing in it, in the filter's own words. */
export function emptyText(f: LinearFilter, teamName?: string): string {
  const ref = issueRef(f.query);
  if (ref) return `No issue ${ref}, or your key can't see it.`;
  const state = f.state === "open" ? "open " : f.state === "all" ? "" : `${STATE_FILTERS.find(([id]) => id === f.state)?.[1].toLowerCase()} `;
  const who = f.anyone ? "" : " assigned to you";
  const where = teamName ? ` in ${teamName}` : "";
  const q = f.query.trim() ? ` matching "${f.query.trim()}"` : "";
  return `No ${state}issues${who}${where}${q}.`;
}

/** The create button. */
export function createLabel(picked: string | null): string {
  return picked ? `Create from ${picked}` : "Pick an issue";
}

/** The New workspace request's `from` for an issue. The backend looks the issue up again. */
export function linearFrom(identifier: string): string {
  return `linear:${identifier}`;
}
