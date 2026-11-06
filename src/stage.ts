// The commit composer's pure parts: subject length, which diff lines can be picked, and editing a
// split plan. The components in components/Commit.tsx draw them.
import type { SplitGroup, StageFile } from "./api";

type DiffLine = { kind: "hunk" | "add" | "del" | "ctx" };

export const SUBJECT_SOFT = 50;
export const SUBJECT_MAX = 72;

/** How the first line of a commit message is doing: short, getting long, or too long. */
export function subjectHint(message: string): { len: number; tone: "empty" | "ok" | "long" | "over"; text: string } {
  const subject = message.split("\n")[0].trim();
  const len = [...subject].length;
  if (!len) return { len, tone: "empty", text: "Write a subject line first" };
  if (len <= SUBJECT_SOFT) return { len, tone: "ok", text: `${len} of ${SUBJECT_MAX}` };
  if (len <= SUBJECT_MAX) return { len, tone: "long", text: `${len} of ${SUBJECT_MAX}. Under ${SUBJECT_SOFT} reads best` };
  return { len, tone: "over", text: `${len} of ${SUBJECT_MAX}. Shorten the first line` };
}

/**
 * Each diff line's number in a line selection: + and - lines count from 0 in order, the way the
 * backend numbers them. Context and hunk headers get null.
 */
export function changeNumbers(lines: DiffLine[]): (number | null)[] {
  let n = 0;
  return lines.map((l) => (l.kind === "add" || l.kind === "del" ? n++ : null));
}

/** The change numbers of the hunk whose header is at `at`. */
export function hunkChanges(lines: DiffLine[], at: number): number[] {
  const nums = changeNumbers(lines);
  const out: number[] = [];
  for (let i = at + 1; i < lines.length && lines[i].kind !== "hunk"; i++) if (nums[i] !== null) out.push(nums[i]!);
  return out;
}

/** Every change number from `a` to `b` (either order) that is a real change line. */
export function changeRange(lines: DiffLine[], a: number, b: number): number[] {
  const nums = changeNumbers(lines);
  const [lo, hi] = a < b ? [a, b] : [b, a];
  return nums.slice(lo, hi + 1).filter((n): n is number => n !== null);
}

/** Toggle one line in a selection, or with shift add the whole range from the last line picked. */
export function pickLine(sel: Set<number>, lines: DiffLine[], i: number, anchor: number | null, shift: boolean): Set<number> {
  const n = changeNumbers(lines)[i];
  if (n === null || n === undefined) return sel;
  const next = new Set(sel);
  if (shift && anchor !== null) {
    for (const x of changeRange(lines, anchor, i)) next.add(x);
  } else if (next.has(n)) {
    next.delete(n);
  } else {
    next.add(n);
  }
  return next;
}

/** "3 lines" / "1 line". */
export const linesWord = (n: number) => `${n} ${n === 1 ? "line" : "lines"}`;

/** Files that only move whole: binary, conflicted, or with no lines (a mode change). */
export const wholeOnly = (f: StageFile) => f.binary || f.status === "U";

/** The protected files in a list, for the warning before staging or committing them. */
export const protectedIn = (files: StageFile[]) => files.filter((f) => f.protected).map((f) => f.path);

// ---------- split plans ----------

/** Move hunk `h` into group `to` (a new group at the end when `to` equals the group count). */
export function moveHunk(groups: SplitGroup[], h: number, to: number): SplitGroup[] {
  const out = groups.map((g) => ({ ...g, hunks: g.hunks.filter((x) => x !== h) }));
  if (to >= out.length) out.push({ message: "", hunks: [h] });
  else out[to] = { ...out[to], hunks: [...out[to].hunks, h].sort((a, b) => a - b) };
  return out.filter((g) => g.hunks.length > 0);
}

/** Move group `i` one place up or down. */
export function moveGroup(groups: SplitGroup[], i: number, dir: -1 | 1): SplitGroup[] {
  const j = i + dir;
  if (j < 0 || j >= groups.length) return groups;
  const out = [...groups];
  [out[i], out[j]] = [out[j], out[i]];
  return out;
}

/** Why a plan can't be committed yet, in words. Null: it can. */
export function planBlocker(groups: SplitGroup[]): string | null {
  if (!groups.length) return "There's nothing to commit.";
  const unnamed = groups.findIndex((g) => !g.message.trim());
  if (unnamed >= 0) return `Commit ${unnamed + 1} needs a message.`;
  const long = groups.findIndex((g) => subjectHint(g.message).tone === "over");
  if (long >= 0) return `Commit ${long + 1}'s first line is over ${SUBJECT_MAX} characters.`;
  return null;
}
