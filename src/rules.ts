import type { RuleFile, RuleHow, RulePlanItem, RulesPlan, RuleState } from "./api";

/** One file's state, in plain words, and the tone of its chip. */
export function stateLabel(f: Pick<RuleFile, "state" | "points_to" | "via">, source: string): { text: string; tone: "good" | "wait" | "bad" | "muted" | "accent" } {
  const s: RuleState = f.state;
  switch (s) {
    case "source": return { text: "Source of truth", tone: "accent" };
    case "same": return { text: `Same as ${source}`, tone: "good" };
    case "points": return { text: f.via === "link" ? `Links to ${source}` : f.via === "note" ? `Points to ${source}` : `Imports ${source}`, tone: "good" };
    case "copy": return { text: "Copy, up to date", tone: "good" };
    case "stale": return { text: `Copy, behind ${source}`, tone: "wait" };
    case "edited": return { text: "Copy, edited by hand", tone: "bad" };
    case "differs": return { text: `Differs from ${source}`, tone: "wait" };
    case "empty": return { text: "Empty", tone: "muted" };
    case "missing": return { text: "Not in this repo", tone: "muted" };
    case "folder": return { text: "Scoped rules, left as they are", tone: "muted" };
  }
}

/** The choices in the plan's per-file menu. */
export const HOW_LABEL: Record<RuleHow, string> = {
  import: "Import it",
  symlink: "Link to it",
  note: "Say where it is",
  copy: "Generated copy",
  keep: "Leave as is",
  source: "New source of truth",
};

/** What a plan row is about to do. */
export function statusLabel(i: RulePlanItem): string {
  if (i.how === "keep") return i.exists ? "Left as is" : "Not created";
  switch (i.status) {
    case "create": return "New file";
    case "update": return "Updated";
    case "replace": return i.needs_confirm ? "Replaces your text" : "Replaced";
    case "same": return "Already done";
    default: return "";
  }
}

/** Files the plan would write. */
export const toWrite = (p: RulesPlan | null) => (p?.items ?? []).filter((i) => i.status === "create" || i.status === "update" || i.status === "replace");

/** Apply is ready: something to write, and every file with hand-written text confirmed. */
export function canApply(p: RulesPlan | null, confirmed: string[]): boolean {
  const w = toWrite(p);
  return w.length > 0 && w.every((i) => !i.needs_confirm || confirmed.includes(i.path));
}

/** Confirmations that still apply to this plan (a new plan can drop some). */
export const keepConfirmed = (p: RulesPlan | null, confirmed: string[]) => confirmed.filter((c) => toWrite(p).some((i) => i.path === c && i.needs_confirm));

/** "1 file" / "3 files". */
export const files = (n: number) => `${n} ${n === 1 ? "file" : "files"}`;
