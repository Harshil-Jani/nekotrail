/**
 * Words and prompts for the Checks tab. Pure, so `bun test` covers them.
 */
import type { CheckDef, CheckFailure, CheckResult, ChecksView } from "./api";

/** "src/a.ts:12", "src/a.ts", or "" when the tool gave no place. */
export const where = (f: Pick<CheckFailure, "file" | "line">) => (f.file ? `${f.file}${f.line ? `:${f.line}` : ""}` : "");

/** A fence the text can't close early. */
const fence = (text: string) => (text.includes("```") ? "~~~~" : "```");
const block = (text: string, lang = "") => {
  const f = fence(text);
  return `${f}${lang}\n${text}\n${f}`;
};
const firstLines = (text: string, n: number) => {
  const lines = text.split("\n");
  return lines.length > n ? `${lines.slice(0, n).join("\n")}\n…` : text;
};

/** "Fix with agent" for one failure: the check, the failure, and the code around it. */
export function fixPrompt(check: Pick<CheckDef, "name" | "command">, f: CheckFailure): string {
  const at = where(f);
  const parts = [
    `The \`${check.name}\` check fails. Fix this failure:`,
    "",
    `**${f.title}**${at ? ` at \`${at}\`` : ""}`,
  ];
  if (f.message.trim() && f.message.trim() !== f.title.trim()) parts.push(block(firstLines(f.message.trim(), 40)));
  if (f.snippet) parts.push("", "The code there:", block(f.snippet));
  parts.push("", `Then run \`${check.command}\` and make sure it passes.`);
  return parts.join("\n");
}

/** At most this many failures go into one "Fix all" message. */
export const FIX_ALL_LIMIT = 25;

/** "Fix all": every failing check with its failures, in one message. */
export function fixAllPrompt(results: CheckResult[]): string {
  const failing = results.filter((r) => r.status !== "passed" && r.failures.length > 0);
  const total = failing.reduce((n, r) => n + r.failures.length, 0);
  const parts = [failing.length === 1 ? `The \`${failing[0].name}\` check fails. Fix every failure below.` : `${failing.length} checks fail. Fix every failure below.`];
  let left = FIX_ALL_LIMIT;
  for (const r of failing) {
    if (left <= 0) break;
    parts.push("", `## ${r.name}: \`${r.command}\``);
    for (const f of r.failures.slice(0, left)) {
      const at = where(f);
      parts.push("", `- **${f.title}**${at ? ` at \`${at}\`` : ""}`);
      if (f.message.trim() && f.message.trim() !== f.title.trim()) parts.push(block(firstLines(f.message.trim(), 12)));
    }
    left -= r.failures.length;
  }
  if (total > FIX_ALL_LIMIT) parts.push("", `There are ${total - FIX_ALL_LIMIT} more failures like these. Run the checks to see them.`);
  parts.push("", `When you're done, run ${failing.map((r) => `\`${r.command}\``).join(", ")} and make sure ${failing.length === 1 ? "it passes" : "they all pass"}.`);
  return parts.join("\n");
}

/** "2 failed, 40 passed", "40 passed", "Passed", "Couldn't run". */
export function countLine(r: Pick<CheckResult, "status" | "passed" | "failed">): string {
  if (r.status === "error") return "Couldn't run";
  const ok = r.passed != null && r.passed > 0 ? `${r.passed} passed` : "";
  if (r.status === "passed") return ok || "Passed";
  const bad = `${r.failed} failed`;
  return ok ? `${bad}, ${ok}` : bad;
}

/** "0.4 s", "12 s", "2 min 5 s". */
export function duration(ms: number): string {
  if (ms < 10_000) return `${(ms / 1000).toFixed(1)} s`;
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s} s` : `${Math.floor(s / 60)} min${s % 60 ? ` ${s % 60} s` : ""}`;
}

/** The line at the top of the tab. */
export function headline(v: Pick<ChecksView, "checks" | "results" | "active" | "current">): string {
  if (v.active) return v.current ? `Running ${v.current}…` : "Starting…";
  const ran = v.results.filter((r) => v.checks.some((c) => c.name === r.name));
  if (!ran.length) return v.checks.length === 1 ? "1 check, not run yet" : `${v.checks.length} checks, not run yet`;
  const bad = ran.filter((r) => r.status !== "passed").length;
  if (bad === 0) return ran.length === v.checks.length ? (ran.length === 1 ? "Passing" : `All ${ran.length} passing`) : `${ran.length} of ${v.checks.length} passing`;
  return `${bad} of ${v.checks.length} failing`;
}

/** The note a turn's checks leave in the chat: "test passed · lint 2 failed". */
export function turnLine(results: { name: string; status: CheckResult["status"]; passed: number | null; failed: number }[]): string {
  return results.map((r) => `${r.name} ${r.status === "passed" ? "passed" : r.status === "error" ? "couldn't run" : `${r.failed} failed`}`).join(" · ");
}
