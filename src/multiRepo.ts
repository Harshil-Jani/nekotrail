import type { Agent, Pr, Workspace } from "./api";
import { AGENTS, AGENT_INFO } from "./agents";

/**
 * Multi-repo tasks: one task across 2 to 5 repos (backend: src-tauri/src/multirepo.rs). Every
 * workspace of the task holds the main workspace's id in `group_id`; the main one is where the
 * chat runs, and the sidebar shows only it.
 */
export const MIN_REPOS = 2;
export const MAX_REPOS = 5;

/** Agents that can work in more than one folder, so a multi-repo task can use them. */
export const MULTI_REPO_AGENTS: Agent[] = AGENTS.filter((a) => AGENT_INFO[a].multiRepo);

/** One plain line for pickers: which agents can take a multi-repo task. */
export function multiRepoNote(): string {
  const names = MULTI_REPO_AGENTS.map((a) => AGENT_INFO[a].name);
  const list = names.length > 1 ? `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}` : names[0];
  return `${list} can work across repos. The other agents only open one folder, so they aren't offered here.`;
}

/** Part of a multi-repo task, but not its main workspace: hidden from the sidebar. */
export const isLinked = (w: Workspace) => !!w.group_id && w.group_id !== w.id;

/** The main workspace of a task (or the workspace itself when it isn't in one). */
export const mainOf = (w: Workspace, all: Workspace[]) => (isLinked(w) ? all.find((x) => x.id === w.group_id) ?? w : w);

/** Every workspace of `w`'s task, the main one first, then in the order they were made. */
export function membersOf(w: Workspace, all: Workspace[]): Workspace[] {
  if (!w.group_id) return [w];
  const m = all.filter((x) => x.group_id === w.group_id && x.state === w.state);
  return m.sort((a, b) => (a.id === a.group_id ? -1 : b.id === b.group_id ? 1 : a.created_at.localeCompare(b.created_at)));
}

/** The picked repos, with `main` moved to the front: the first repo holds the chat. */
export function withMainFirst(picked: string[], main: string | null): string[] {
  if (!main || !picked.includes(main)) return picked;
  return [main, ...picked.filter((r) => r !== main)];
}

/** Tick or untick a repo, keeping the pick order and at most `MAX_REPOS`. */
export function togglePick(picked: string[], id: string): string[] {
  if (picked.includes(id)) return picked.filter((r) => r !== id);
  return picked.length >= MAX_REPOS ? picked : [...picked, id];
}

/** Move one entry of the merge order up (-1) or down (+1). */
export function move<T>(list: T[], i: number, by: -1 | 1): T[] {
  const j = i + by;
  if (j < 0 || j >= list.length) return list;
  const out = [...list];
  [out[i], out[j]] = [out[j], out[i]];
  return out;
}

/** Why a PR can't be merged yet, in a few words; null when it is green. Twin of `multirepo::blocker`. */
export function blocker(pr: Pr | undefined): string | null {
  if (!pr) return "no PR";
  if (pr.state === "MERGED") return "merged";
  if (pr.state === "CLOSED") return "closed";
  if (pr.isDraft) return "draft";
  if (pr.mergeable === "CONFLICTING") return "conflicts";
  const result = (c: NonNullable<Pr>["statusCheckRollup"][number]) => (c.conclusion || c.state || c.status || "").toUpperCase();
  const checks = pr.statusCheckRollup ?? [];
  const failing = checks.filter((c) => ["FAILURE", "ERROR", "TIMED_OUT", "ACTION_REQUIRED", "STARTUP_FAILURE", "CANCELLED"].includes(result(c))).length;
  if (failing) return `${failing} failing ${failing === 1 ? "check" : "checks"}`;
  const running = checks.filter((c) => ["", "PENDING", "QUEUED", "IN_PROGRESS", "EXPECTED", "WAITING"].includes(result(c))).length;
  if (running) return `${running} ${running === 1 ? "check" : "checks"} running`;
  if (pr.reviewDecision === "CHANGES_REQUESTED") return "changes requested";
  return null;
}

/**
 * Can Merge all run: every PR open and green, or already merged, and at least one left to merge.
 * `waiting` lists the repos that hold it up, for the button's title.
 */
export function mergeAllState(rows: { name: string; pr: Pr | undefined }[]): { ready: boolean; waiting: string[]; left: number } {
  const waiting: string[] = [];
  let left = 0;
  for (const r of rows) {
    const why = blocker(r.pr);
    if (why === "merged") continue;
    if (why) waiting.push(`${r.name}: ${why}`);
    else left++;
  }
  return { ready: waiting.length === 0 && left > 0, waiting, left };
}

/** A PR as GitHub names it across repos, from its link: "org/api#12". */
export function shortRef(url: string, number: number): string {
  const m = /^https?:\/\/[^/]+\/([^/]+\/[^/]+)\/pull\/\d+/.exec(url);
  return m ? `${m[1]}#${number}` : `#${number}`;
}
