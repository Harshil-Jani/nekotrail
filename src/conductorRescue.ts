/**
 * Moving Conductor worktrees into nekotrail's own folder: the counts, labels and summaries the
 * import sheet shows. Pure, so it is tested in tests/conductorRescue.test.ts.
 */
import type { ImportReport, RescueItem, RescueOutcome } from "./api";

export const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** `du -sk` sizes (KiB) as people read them. */
export function sizeLabel(kb: number): string {
  if (kb < 1024) return `${kb} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}

/** A path with your home folder written as ~. */
export function tilde(path: string): string {
  return path.replace(/^\/(Users|home)\/[^/]+(?=\/|$)/, "~");
}

export type RepoGroup = { repo: string; items: RescueItem[]; kb: number; uncommitted: number };

/** Items by repo, in the order they first appear. */
export function byRepo(items: RescueItem[]): RepoGroup[] {
  const out: RepoGroup[] = [];
  for (const it of items) {
    let g = out.find((x) => x.repo === it.repo);
    if (!g) out.push((g = { repo: it.repo, items: [], kb: 0, uncommitted: 0 }));
    g.items.push(it);
    g.kb += it.kb;
    g.uncommitted += it.uncommitted;
  }
  return out;
}

export function totals(items: RescueItem[]) {
  return {
    repos: new Set(items.map((i) => i.repo)).size,
    worktrees: items.length,
    kb: items.reduce((n, i) => n + i.kb, 0),
    uncommitted: items.reduce((n, i) => n + i.uncommitted, 0),
  };
}

/** "2 repos, 5 worktrees, 1.4 GB, 9 uncommitted files". */
export function whatMoves(items: RescueItem[]): string {
  const t = totals(items);
  if (!t.worktrees) return "Nothing to move.";
  return [plural(t.repos, "repo"), plural(t.worktrees, "worktree"), sizeLabel(t.kb), t.uncommitted ? plural(t.uncommitted, "uncommitted file") : "no uncommitted files"].join(", ");
}

export type Choice = "move" | "leave";

/** The sheet's main button. Nothing to import means only the move is left to do. */
export function primaryLabel(choice: Choice, report: ImportReport | null, items: RescueItem[]): string {
  const importing = !!report && report.repos + report.workspaces > 0;
  const n = items.length;
  if (choice === "leave") return importing ? "Import only" : "Not now";
  if (!n) return importing ? "Import" : "Done";
  return importing ? `Import and move ${plural(n, "worktree")}` : `Move ${plural(n, "worktree")}`;
}

export type Verdict = "ok" | "differs" | "failed";

export const verdict = (o: RescueOutcome): Verdict => (!o.moved ? "failed" : o.problems.length ? "differs" : "ok");

/** One line under each workspace once the move ran. */
export function outcomeLine(o: RescueOutcome): string {
  const v = verdict(o);
  if (v === "failed") return `Not moved: ${o.error ?? "unknown error"} It is still in Conductor's folder, untouched.`;
  if (v === "differs") return `Moved. The check after it found a difference. ${o.problems.join(" ")}`;
  if (!o.uncommitted) return "Checked: same branch head, no uncommitted files.";
  return o.uncommitted === 1
    ? "Checked: same branch head, and the 1 uncommitted file is intact."
    : `Checked: same branch head, and all ${o.uncommitted} uncommitted files are intact.`;
}

export function summary(outcomes: RescueOutcome[]): { text: string; bad: boolean; failed: string[] } {
  const ok = outcomes.filter((o) => verdict(o) === "ok").length;
  const differs = outcomes.filter((o) => verdict(o) === "differs").length;
  const failed = outcomes.filter((o) => verdict(o) === "failed");
  const n = outcomes.length;
  if (!n) return { text: "Nothing needed moving.", bad: false, failed: [] };
  const parts = [ok === n ? `Moved and checked ${plural(n, "worktree")}.` : `Moved and checked ${ok} of ${n}.`];
  if (differs) parts.push(`${differs} moved with a difference to look at.`);
  if (failed.length) parts.push(`${failed.length} ${failed.length === 1 ? "wasn't" : "weren't"} moved and ${failed.length === 1 ? "is" : "are"} still in Conductor's folder, untouched.`);
  return { text: parts.join(" "), bad: differs + failed.length > 0, failed: failed.map((o) => o.id) };
}

/** The one-time notice about imported workspaces still in Conductor's folder. */
export const NOTICE_KEY = "nekotrail.conductorMoveNotice";
export const noticeShown = (outside: number, dismissed: string | null) => outside > 0 && dismissed !== "dismissed";
