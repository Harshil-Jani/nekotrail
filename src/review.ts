import type { Agent, ReviewPlan, ReviewRequest, ReviewVerdict, Workspace } from "./api";
import { AGENTS } from "./agents";

/** The agent a review starts with: the one you last picked for a new workspace, else Claude. */
export function reviewAgent(saved: unknown): Agent {
  const a = (saved as { agent?: unknown } | null)?.agent;
  return typeof a === "string" && (AGENTS as string[]).includes(a) ? (a as Agent) : "claude";
}

/** The open review workspace for a request, if you already started one. */
export function reviewWorkspaceFor(req: Pick<ReviewRequest, "repo_id" | "number">, workspaces: Workspace[]): Workspace | undefined {
  return workspaces.find((w) => w.state === "active" && w.repo_id === req.repo_id && w.review_pr === req.number);
}

export const VERDICTS: { value: ReviewVerdict; label: string; hint: string }[] = [
  { value: "comment", label: "Comment", hint: "Feedback without a verdict." },
  { value: "approve", label: "Approve", hint: "Good to merge." },
  { value: "request_changes", label: "Request changes", hint: "Must change before it merges." },
];

/**
 * Why Submit is off, or null when it can go. Mirrors `review::check` in the backend: GitHub needs a
 * summary to request changes, and a comment review needs something in it.
 */
export function submitBlocker(verdict: ReviewVerdict, summary: string, plan: Pick<ReviewPlan, "inline" | "folded">): string | null {
  const hasSummary = summary.trim().length > 0 || plan.folded.length > 0;
  if (verdict === "request_changes" && !hasSummary) return "Say what needs to change. GitHub needs a summary to request changes.";
  if (verdict === "comment" && !hasSummary && plan.inline === 0) return "Add a comment or a summary first.";
  return null;
}

const n = (k: number, one: string, many: string) => `${k} ${k === 1 ? one : many}`;

/** One plain line on where the drafts go, for the submit sheet. */
export function planLine(plan: Pick<ReviewPlan, "inline" | "folded" | "suggestions">): string {
  const parts: string[] = [];
  if (plan.inline) parts.push(`${n(plan.inline, "comment goes", "comments go")} on lines${plan.suggestions ? `, ${n(plan.suggestions, "with a suggested change", "with suggested changes")}` : ""}.`);
  if (plan.folded.length) parts.push(`${n(plan.folded.length, "comment is", "comments are")} on lines outside this PR's changes, so ${plan.folded.length === 1 ? "it goes" : "they go"} in the summary.`);
  if (!parts.length) parts.push("No line comments. Only the summary goes up.");
  return parts.join(" ");
}

/** "#12 · @sam · app" under a review request's title. */
export function requestMeta(req: ReviewRequest, repoName?: string): string {
  return [`#${req.number}`, `@${req.author.login}`, repoName, req.isDraft ? "draft" : ""].filter(Boolean).join(" · ");
}

/**
 * A comment split into prose and ```suggestion blocks, so the card shows the suggested code as code
 * instead of the raw fence. The text that goes to GitHub is unchanged.
 */
export function suggestionParts(text: string): { kind: "text" | "suggestion"; text: string }[] {
  const out: { kind: "text" | "suggestion"; text: string }[] = [];
  const re = /^[ \t]*(`{3,})suggestion[ \t]*\n([\s\S]*?)\n?^[ \t]*\1[ \t]*$/gm;
  let at = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    const before = text.slice(at, m.index).trim();
    if (before) out.push({ kind: "text", text: before });
    out.push({ kind: "suggestion", text: m[2] });
    at = m.index + m[0].length;
  }
  const rest = text.slice(at).trim();
  if (rest || !out.length) out.push({ kind: "text", text: rest || text });
  return out;
}

/** Has a ```suggestion block (GitHub applies it in one click). */
export const hasSuggestion = (text: string) => /^\s*`{3,}suggestion\s*$/m.test(text);
