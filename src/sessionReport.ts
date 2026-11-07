/**
 * "Export session": turns a workspace's (or one chat's) history into a report someone else can read.
 * Pure functions, no I/O: ExportSheet gathers the data, these build the report, hide what the user
 * asked to hide, and write it as Markdown, a single self-contained HTML file, or a PR summary.
 */
import type { ChatEvent, CheckResult, FileChange } from "./api";
import { agentName } from "./agents";
import { maskText, scanSecrets, shortenHome, type ScanContext } from "./redact";

export type ReportChat = { title: string; agent: string; model: string | null; events: ChatEvent[] };

/** Everything the report is built from, as the app already has it. */
export type ReportSource = {
  title: string;
  repo: string;
  branch: string;
  target: string;
  /** The workspace's star ("Mochi"). */
  star: string;
  chats: ReportChat[];
  /** Files changed against the target branch, and the diff text of the ones worth showing. */
  changes: FileChange[];
  diffs: Record<string, string>;
  checks: Pick<CheckResult, "name" | "command" | "status" | "passed" | "failed" | "regressed">[];
  pr: { number: number; url: string; state: string; title: string; isDraft: boolean } | null;
  /** The user's home folder, shortened to ~ in every path. */
  home: string | null;
  /** When the export was made, as an ISO string. */
  exportedAt: string;
};

export type ReportOptions = {
  /** What commands and tools printed. Off by default: outputs are where secrets and noise live. */
  toolOutputs: boolean;
  /** Code: files the agent read or wrote, and the diff hunks. */
  fileContents: boolean;
  /** The "Made with NekoTrail" line (Settings, General). */
  footer: boolean;
};

export const DEFAULT_OPTIONS: ReportOptions = { toolOutputs: false, fileContents: true, footer: true };

export type TimelineItem =
  | { kind: "user"; text: string }
  | { kind: "agent"; text: string }
  /** One tool call on one line: `target` is the command, file, pattern or URL. */
  | { kind: "tool"; name: string; target: string; failed: boolean; pending: boolean; output: string | null; env: boolean }
  | { kind: "note"; text: string; bad: boolean };

export type ReportHunk = { path: string; text: string; more: number; env: boolean };

export type Report = {
  title: string;
  repo: string;
  branch: string;
  target: string;
  star: string;
  agents: string[];
  task: string;
  plan: string | null;
  chats: { title: string; agent: string; model: string | null; items: TimelineItem[] }[];
  files: FileChange[];
  additions: number;
  deletions: number;
  hunks: ReportHunk[];
  /** Files with changes the hunks leave out because they're too big or generated. */
  hunksSkipped: number;
  checks: ReportSource["checks"];
  stats: { turns: number; errors: number; durationMs: number; costUsd: number; costKnown: boolean; toolCalls: number; failedCommands: number };
  pr: ReportSource["pr"];
  exportedAt: string;
  footer: boolean;
};

const FILE_TOOLS = new Set(["Read", "Write", "Edit", "MultiEdit", "NotebookEdit"]);
const LOCKFILE = /(^|\/)(bun\.lockb?|package-lock\.json|yarn\.lock|pnpm-lock\.yaml|Cargo\.lock|go\.sum|poetry\.lock|uv\.lock|Gemfile\.lock|composer\.lock)$|\.min\.(js|css)$|\.(png|jpe?g|gif|webp|ico|pdf|zip|woff2?)$/i;
/** A .env file (not .env.example, whose values are meant to be shared). */
export const isEnvPath = (p: string) => /(^|[\/\s])\.env(\.[A-Za-z0-9_-]+)?$/.test(p.trim()) && !/\.(example|sample|template|dist)$/.test(p.trim());

const OUTPUT_LINES = 40;
const HUNK_LINES = 30;
const MAX_HUNK_FILES = 4;
const HUNKS_PER_FILE = 2;

/** At most `n` lines, with a note of how many were left out. */
export function clipLines(text: string, n: number): string {
  const lines = text.replace(/\s+$/, "").split("\n");
  return lines.length <= n ? lines.join("\n") : `${lines.slice(0, n).join("\n")}\n… ${lines.length - n} more lines`;
}

/** One line for a tool call: its first line, at most 200 characters. */
function oneLine(s: string): string {
  const lines = s.trim().split("\n");
  const first = lines[0].length > 200 ? `${lines[0].slice(0, 199)}…` : lines[0];
  return lines.length > 1 && first === lines[0] ? `${first} …` : first;
}

/** The first hunks of a diff, each cut to a readable length. */
export function keyHunks(diff: string): { text: string; more: number } {
  const hunks: string[][] = [];
  for (const line of diff.split("\n")) {
    if (line.startsWith("@@")) hunks.push([line]);
    else if (hunks.length && !line.startsWith("\\ No newline")) hunks[hunks.length - 1].push(line);
  }
  const shown = hunks.slice(0, HUNKS_PER_FILE).map((h) => clipLines(h.join("\n"), HUNK_LINES));
  return { text: shown.join("\n"), more: Math.max(0, hunks.length - HUNKS_PER_FILE) };
}

/** Files whose hunks the report shows: the biggest real changes, not lockfiles or binaries. */
export function pickHunkFiles(changes: FileChange[]): string[] {
  return changes
    .filter((c) => !LOCKFILE.test(c.path) && c.additions + c.deletions > 0)
    .sort((a, b) => b.additions + b.deletions - (a.additions + a.deletions))
    .slice(0, MAX_HUNK_FILES)
    .map((c) => c.path);
}

/** Builds the report. Paths under the home folder become ~; secrets are hidden later by `redactReport`. */
export function buildReport(src: ReportSource, opts: ReportOptions = DEFAULT_OPTIONS): Report {
  const home = (s: string) => shortenHome(s, src.home);
  let turns = 0, errors = 0, durationMs = 0, costUsd = 0, costKnown = false, toolCalls = 0, failedCommands = 0;
  let plan: string | null = null;
  let todos: string | null = null;
  // The task is the first prompt of the main chat: the one with the most of your messages, then the most events (a short
  // "second opinion" chat beside it doesn't win).
  const userCount = (c: ReportChat) => c.events.filter((e) => e.kind === "user").length * 1e6 + c.events.length;
  const main = src.chats.reduce<ReportChat | null>((best, c) => (!best || userCount(c) > userCount(best) ? c : best), null);
  const firstPrompt = main?.events.find((e) => e.kind === "user") as { text: string } | undefined;
  let task = firstPrompt?.text ?? "";

  const chats = src.chats.map((chat) => {
    const items: TimelineItem[] = [];
    const tools = new Map<string, Extract<TimelineItem, { kind: "tool" }>>();
    for (const ev of chat.events) {
      switch (ev.kind) {
        case "user":
          items.push({ kind: "user", text: home(ev.text) });
          break;
        case "text":
          if (ev.fix) items.push({ kind: "note", text: home(ev.text), bad: true });
          else if (ev.text.trim()) items.push({ kind: "agent", text: home(ev.text) });
          break;
        case "tool": {
          if (ev.name === "ExitPlanMode") {
            plan = ev.input.replace(/^\s*#{1,3}\s*(the\s+)?plan:?\s*\n/i, "").trim();
            items.push({ kind: "note", text: "Proposed a plan (see Plan above).", bad: false });
            break;
          }
          if (ev.name === "TodoWrite") {
            todos = todoText(ev.input) ?? todos;
            break;
          }
          toolCalls++;
          const target = home(oneLine(ev.input));
          const item = { kind: "tool" as const, name: ev.name, target, failed: false, pending: true, output: null as string | null, env: isEnvPath(ev.input) };
          tools.set(ev.id, item);
          items.push(item);
          break;
        }
        case "tool_result": {
          const t = tools.get(ev.id);
          if (!t) break;
          t.pending = false;
          t.failed = ev.is_error;
          if (ev.is_error && t.name === "Bash") failedCommands++;
          const isFile = FILE_TOOLS.has(t.name);
          const keep = isFile ? opts.fileContents && opts.toolOutputs : opts.toolOutputs;
          if (keep && ev.output.trim()) t.output = home(clipLines(ev.output, OUTPUT_LINES));
          break;
        }
        case "done":
          turns++;
          if (ev.is_error) errors++;
          durationMs += ev.duration_ms ?? 0;
          if (typeof ev.cost_usd === "number") { costUsd += ev.cost_usd; costKnown = true; }
          if (ev.stopped) items.push({ kind: "note", text: "You stopped the turn.", bad: false });
          else if (ev.is_error) items.push({ kind: "note", text: "The turn ended with an error.", bad: true });
          break;
        case "error":
          items.push({ kind: "note", text: `Error: ${home(ev.text)}`, bad: true });
          break;
        case "checks":
          items.push({ kind: "note", text: `Checks: ${ev.results.map((r) => `${r.name} ${r.status}${r.failed ? ` (${r.failed} failing)` : ""}`).join(", ")}.`, bad: ev.results.some((r) => r.status !== "passed") });
          break;
        case "blocked":
          items.push({ kind: "note", text: `${home(ev.text)} ${ev.files.map(home).join(", ")}`.trim(), bad: true });
          break;
        case "guard":
          items.push({ kind: "note", text: `Changed protected ${ev.files.length === 1 ? "file" : "files"}: ${ev.files.map((f) => home(f.path)).join(", ")}.`, bad: true });
          break;
        case "guard_restored":
          items.push({ kind: "note", text: `Put back: ${ev.files.map(home).join(", ")}.`, bad: false });
          break;
        case "rewind":
          items.push({ kind: "note", text: home(ev.text), bad: false });
          break;
        case "handoff":
          items.push({ kind: "note", text: `${ev.dir === "to" ? "Handed off to" : "Continued from"} ${agentName(ev.agent)}.`, bad: false });
          break;
        // Thinking, approvals and their answers stay out: they repeat what the timeline shows.
        default:
          break;
      }
    }
    // Tool calls the turn never answered were cut off, not running: say nothing about them.
    return { title: chat.title, agent: chat.agent, model: chat.model, items };
  });

  const hunkFiles = opts.fileContents ? pickHunkFiles(src.changes) : [];
  const hunks: ReportHunk[] = [];
  for (const path of hunkFiles) {
    const d = src.diffs[path];
    if (!d) continue;
    const { text, more } = keyHunks(d);
    if (text) hunks.push({ path: home(path), text: home(text), more, env: isEnvPath(path) });
  }

  return {
    title: home(src.title.trim() || oneLine(task) || src.star),
    repo: src.repo,
    branch: src.branch,
    target: src.target,
    star: src.star,
    agents: [...new Set(src.chats.filter((c) => c.events.length).map((c) => agentName(c.agent)))],
    task: home(task.trim()),
    plan: plan ? home(plan) : todos ? home(todos) : null,
    chats,
    files: src.changes.map((c) => ({ ...c, path: home(c.path) })),
    additions: src.changes.reduce((n, c) => n + c.additions, 0),
    deletions: src.changes.reduce((n, c) => n + c.deletions, 0),
    hunks,
    hunksSkipped: opts.fileContents ? Math.max(0, src.changes.length - hunks.length) : src.changes.length,
    checks: src.checks.map((c) => ({ ...c, command: home(c.command) })),
    stats: { turns, errors, durationMs, costUsd, costKnown, toolCalls, failedCommands },
    pr: src.pr,
    exportedAt: src.exportedAt,
    footer: opts.footer,
  };
}

/** A todo list (TodoWrite) as a Markdown checklist, the plan when the agent made no formal one. */
function todoText(input: string): string | null {
  try {
    const list = JSON.parse(input) as { content?: string; status?: string }[];
    if (!Array.isArray(list) || !list.length) return null;
    return list.map((t) => `- [${t.status === "completed" ? "x" : " "}] ${t.content ?? ""}`).join("\n");
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------------------------
// Hiding secrets

/** One value that looks like a secret, with how many times it appears. Un-masking is per value. */
export type Finding = { value: string; kind: string; count: number };

/** Calls `fn` on every piece of text in the report, with whether it came from a .env file. */
function eachText(r: Report, fn: (text: string, ctx: ScanContext) => void) {
  for (const s of [r.title, r.task, r.plan ?? ""]) fn(s, {});
  for (const c of r.chats) {
    fn(c.title, {});
    for (const it of c.items) {
      if (it.kind === "tool") { fn(it.target, {}); if (it.output) fn(it.output, { env: it.env }); }
      else fn(it.text, {});
    }
  }
  for (const h of r.hunks) fn(h.text, { env: h.env });
  for (const c of r.checks) fn(c.command, {});
}

/** Everything in the report that looks like a secret, most frequent first. */
export function findSecrets(r: Report): Finding[] {
  const by = new Map<string, Finding>();
  eachText(r, (text, ctx) => {
    for (const s of scanSecrets(text, ctx)) {
      const f = by.get(s.value);
      if (f) f.count++;
      else by.set(s.value, { value: s.value, kind: s.kind, count: 1 });
    }
  });
  return [...by.values()].sort((a, b) => b.count - a.count || a.kind.localeCompare(b.kind));
}

/** The report with every finding hidden, except the values the user chose to show (`keep`). */
export function redactReport(r: Report, findings: Finding[], keep: Set<string>): Report {
  const m = (s: string, ctx: ScanContext = {}) => maskText(s, ctx, keep, findings);
  return {
    ...r,
    title: m(r.title),
    task: m(r.task),
    plan: r.plan === null ? null : m(r.plan),
    chats: r.chats.map((c) => ({
      ...c,
      title: m(c.title),
      items: c.items.map((it) => it.kind === "tool" ? { ...it, target: m(it.target), output: it.output === null ? null : m(it.output, { env: it.env }) } : { ...it, text: m(it.text) }),
    })),
    hunks: r.hunks.map((h) => ({ ...h, text: m(h.text, { env: h.env }) })),
    checks: r.checks.map((c) => ({ ...c, command: m(c.command) })),
  };
}

// ---------------------------------------------------------------------------------------------
// Formatting helpers

export function formatDuration(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}
/** The summary's Changes cell: "none" rather than "0 files, +0 −0". */
const changesCell = (r: Report) => (r.files.length ? `${plural(r.files.length, "file")}, +${r.additions} −${r.deletions}` : "none");
/** The summary's Agent time cell: "none" when no turn finished, rather than "0s". */
const timeCell = (r: Report) => (r.stats.turns ? formatDuration(r.stats.durationMs) : "none");
export const formatCost = (r: Report) => (r.stats.costKnown ? `$${r.stats.costUsd.toFixed(2)}` : "not reported");
const formatDate = (iso: string) => {
  const d = new Date(iso);
  return isNaN(d.getTime()) ? iso : d.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
};
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
const chatLabel = (c: Report["chats"][number]) => `${agentName(c.agent)}${c.model ? ` (${c.model})` : ""}`;
const prState = (pr: NonNullable<Report["pr"]>) => (pr.state === "OPEN" ? (pr.isDraft ? "draft" : "open") : pr.state.toLowerCase());
/** Only web links go into an export; anything else is shown as text. */
export const safeUrl = (u: string | null | undefined) => (u && /^https?:\/\/[^\s"'<>]+$/i.test(u) ? u : null);
const checkLine = (c: Report["checks"][number]) =>
  `${c.status === "passed" ? "passed" : c.status === "failed" ? "failed" : "could not run"}${c.passed !== null ? `, ${c.passed} passed` : ""}${c.failed ? `, ${c.failed} failing` : ""}${c.regressed ? ", passed before this change" : ""}`;
/** What the tool row says before its target: "Ran", "Read", "Edited". */
const toolVerb = (name: string) =>
  ({ Bash: "Ran", Read: "Read", Write: "Wrote", Edit: "Edited", MultiEdit: "Edited", NotebookEdit: "Edited", Grep: "Searched", Glob: "Listed", WebFetch: "Fetched", WebSearch: "Searched the web for", Task: "Asked a subagent to", Agent: "Asked a subagent to" } as Record<string, string>)[name] ?? name;

// ---------------------------------------------------------------------------------------------
// Markdown

/** A code span that survives backticks inside it. */
export function mdCode(s: string): string {
  const longest = Math.max(0, ...(s.match(/`+/g) ?? []).map((x) => x.length));
  const ticks = "`".repeat(longest + 1);
  const pad = s.startsWith("`") || s.endsWith("`") ? " " : "";
  return `${ticks}${pad}${s}${pad}${ticks}`;
}

/** A fenced block whose fence is longer than any run of backticks in the text. */
export function mdFence(s: string, lang = ""): string {
  const longest = Math.max(2, ...(s.match(/`+/g) ?? []).map((x) => x.length));
  const f = "`".repeat(longest + 1);
  return `${f}${lang}\n${s}\n${f}`;
}

const quote = (s: string) => s.split("\n").map((l) => (l ? `> ${l}` : ">")).join("\n");
/** Plain text in a Markdown line: no accidental headings, lists, HTML or links. */
const mdText = (s: string) => s.replace(/([\\`*_[\]<>#|])/g, "\\$1");

export function toMarkdown(r: Report): string {
  const out: string[] = [];
  out.push(`# ${mdText(r.title)}`);
  out.push([`${mdText(r.repo)}`, `${mdCode(r.branch)} into ${mdCode(r.target)}`, r.agents.join(", ") || null, `exported ${formatDate(r.exportedAt)}`].filter(Boolean).join(" · "));
  out.push([
    "| Agent time | Turns | Cost | Changes | Checks | PR |",
    "|---|---|---|---|---|---|",
    `| ${timeCell(r)} | ${r.stats.turns}${r.stats.errors ? ` (${r.stats.errors} with errors)` : ""} | ${formatCost(r)} | ${changesCell(r)} | ${checksSummary(r)} | ${r.pr ? `#${r.pr.number} (${prState(r.pr)})` : "none yet"} |`,
  ].join("\n"));

  if (r.task) out.push("## Task", quote(r.task));
  if (r.plan) out.push("## Plan", r.plan);

  out.push("## What the agent did");
  if (!r.chats.some((c) => c.items.length)) out.push("_No messages yet._");
  for (const c of r.chats) {
    if (!c.items.length) continue;
    if (r.chats.filter((x) => x.items.length).length > 1 || c.title) out.push(`### ${mdText(c.title || "Chat")} · ${chatLabel(c)}`);
    const who = agentName(c.agent);
    let tools: string[] = [];
    const flush = () => { if (tools.length) { out.push(tools.join("\n")); tools = []; } };
    for (const it of c.items) {
      if (it.kind === "tool") {
        const mark = it.failed ? "**Failed:** " : "";
        tools.push(`- ${mark}${toolVerb(it.name)} ${mdCode(it.target)}`);
        if (it.output) tools.push(`\n${mdFence(it.output).split("\n").map((l) => `  ${l}`).join("\n")}\n`);
        continue;
      }
      flush();
      if (it.kind === "user") out.push(`**You:**\n\n${quote(it.text)}`);
      else if (it.kind === "agent") out.push(`**${who}:**\n\n${it.text}`);
      else out.push(`_${mdText(it.text)}_`);
    }
    flush();
  }

  out.push("## Changes", r.files.length ? `${plural(r.files.length, "file")} changed, +${r.additions} −${r.deletions} against ${mdCode(r.target)}.` : "No changes against the target branch.");
  if (r.files.length) {
    out.push(["| File | Added | Removed |", "|---|---:|---:|", ...r.files.map((f) => `| ${mdCode(f.path)} | ${f.additions} | ${f.deletions} |`)].join("\n"));
  }
  for (const h of r.hunks) out.push(`### ${mdCode(h.path)}`, mdFence(h.text, "diff") + (h.more ? `\n\n_${plural(h.more, "more hunk")} not shown._` : ""));

  out.push("## Checks", r.checks.length ? r.checks.map((c) => `- **${mdText(c.name)}** ${checkLine(c)} (${mdCode(c.command)})`).join("\n") : "No checks ran in NekoTrail for this workspace.");

  out.push("## Pull request", r.pr ? `[#${r.pr.number} ${mdText(r.pr.title)}](${safeUrl(r.pr.url) ?? ""}) (${prState(r.pr)})` : "No PR yet.");
  if (r.footer) out.push("---", "_Made with NekoTrail_");
  return `${out.join("\n\n")}\n`;
}

function checksSummary(r: Report): string {
  if (!r.checks.length) return "none ran";
  const bad = r.checks.filter((c) => c.status !== "passed").length;
  return bad ? `${bad} of ${r.checks.length} failing` : `all ${r.checks.length} passed`;
}

// ---------------------------------------------------------------------------------------------
// PR description

/** A short summary for the PR body: what changed, how it was tested, which agent, and the cost. */
export function prSummary(r: Report): string {
  const lastAnswer = [...r.chats.flatMap((c) => c.items)].reverse().find((i) => i.kind === "agent") as { text: string } | undefined;
  const firstPara = (s: string) => {
    const p = s.trim().split(/\n\s*\n/)[0].trim();
    return p.length > 500 ? `${p.slice(0, 499)}…` : p;
  };
  const files = r.files.slice(0, 5).map((f) => mdCode(f.path)).join(", ") + (r.files.length > 5 ? ` and ${r.files.length - 5} more` : "");
  const lines = ["## Summary"];
  if (r.task) lines.push(`Task: ${mdText(firstPara(r.task).split("\n").join(" "))}`);
  if (lastAnswer) lines.push(firstPara(lastAnswer.text));
  if (r.files.length) lines.push(`${plural(r.files.length, "file")} changed (+${r.additions} −${r.deletions}): ${files}.`);
  lines.push("## How it was tested");
  lines.push(r.checks.length ? r.checks.map((c) => `- ${mdText(c.name)}: ${checkLine(c)}`).join("\n") : "No checks ran in NekoTrail. Say how you tested it here.");
  const agents = [...new Set(r.chats.filter((c) => c.items.length).map(chatLabel))].join(", ") || "none";
  lines.push(`Agent: ${agents}. ${plural(r.stats.turns, "turn")}, ${formatDuration(r.stats.durationMs)}, cost ${formatCost(r)}.`);
  if (r.footer) lines.push("<sub>Made with NekoTrail</sub>");
  return lines.join("\n\n");
}

// ---------------------------------------------------------------------------------------------
// HTML

const ESC: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
/** Text into HTML: every character that could start markup is escaped. */
export const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ESC[c]);

/** Markdown (agent answers, the plan) to safe HTML. Passed in so tests and the app share one renderer. */
export type MdToHtml = (markdown: string) => string;

const CSS = `
:root{color-scheme:light dark;--bg:#fdfdff;--surface:#f3f3fa;--text:#14143a;--muted:#545485;--line:#dcdcee;--accent:#3d3d85;--good:#095a32;--bad:#a01733;--bad-soft:rgba(181,30,59,.08);--add:rgba(11,106,59,.1);--del:rgba(181,30,59,.08)}
@media (prefers-color-scheme:dark){:root{--bg:#101026;--surface:#161634;--text:#f0f0ff;--muted:#9c9ccc;--line:#29295a;--accent:#ccccff;--good:#6ee8ac;--bad:#ff94a4;--bad-soft:rgba(255,122,142,.14);--add:rgba(94,224,160,.12);--del:rgba(255,122,142,.13)}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--text);font:14px/1.55 -apple-system,BlinkMacSystemFont,"SF Pro Text","Segoe UI",Helvetica,Arial,sans-serif;-webkit-font-smoothing:antialiased}
main{max-width:860px;margin:0 auto;padding:40px 24px 56px}
h1{font-size:24px;line-height:1.25;margin:0 0 6px}
h2{font-size:17px;margin:36px 0 12px;padding-bottom:6px;border-bottom:.5px solid var(--line)}
h3{font-size:14px;margin:24px 0 8px}
p{margin:8px 0}
a{color:var(--accent)}
code,pre{font-family:ui-monospace,"SF Mono",SFMono-Regular,Menlo,Consolas,monospace;font-size:12.5px}
:not(pre)>code{background:var(--surface);padding:1px 5px;border-radius:4px;overflow-wrap:anywhere}
pre{background:var(--surface);padding:10px 12px;border-radius:8px;overflow-x:auto;line-height:1.45;margin:8px 0}
.meta{color:var(--muted);margin:0}
.stats{display:grid;grid-template-columns:repeat(auto-fit,minmax(120px,1fr));gap:8px;margin:20px 0 0;padding:0}
.stats div{background:var(--surface);border-radius:10px;padding:10px 12px}
.stats dt{font-size:12px;color:var(--muted)}
.stats dd{margin:2px 0 0;font-weight:600}
blockquote{margin:8px 0;padding:8px 14px;border-radius:10px;background:var(--surface);white-space:pre-wrap}
.turn{margin:14px 0}
.who{font-size:12px;font-weight:600;color:var(--muted);margin-bottom:2px}
.answer>:first-child{margin-top:0}
.tools{list-style:none;margin:8px 0;padding:0;border-left:2px solid var(--line)}
.tools li{padding:2px 0 2px 12px;color:var(--muted);font-size:13px}
.tools li code{color:var(--text)}
.tools li.failed{color:var(--bad)}
.tools .tag{font-weight:600;font-size:11px;padding:0 6px;border-radius:4px;background:var(--bad-soft);color:var(--bad);margin-right:6px}
details summary{cursor:pointer;color:var(--muted);font-size:12px}
.note{font-size:13px;color:var(--muted);font-style:italic}
.note.bad{color:var(--bad)}
table{border-collapse:collapse;width:100%;font-size:13px;margin:8px 0}
th,td{text-align:left;padding:5px 8px;border-bottom:.5px solid var(--line)}
td.n,th.n{text-align:right;font-variant-numeric:tabular-nums}
.diff{padding:0}
.diff span{display:block;padding:0 12px;white-space:pre}
.diff .a{background:var(--add)}
.diff .d{background:var(--del)}
.diff .h{color:var(--muted)}
.ok{color:var(--good);font-weight:600}
.no{color:var(--bad);font-weight:600}
footer{margin-top:40px;color:var(--muted);font-size:12px}
img{display:none}
`;

/** A self-contained HTML page: inline CSS, light and dark, no scripts, and a policy that blocks any request. */
export function toHtml(r: Report, md: MdToHtml): string {
  const b: string[] = [];
  b.push(`<h1>${esc(r.title)}</h1>`);
  b.push(`<p class="meta">${[esc(r.repo), `<code>${esc(r.branch)}</code> into <code>${esc(r.target)}</code>`, esc(r.agents.join(", ")), `exported ${esc(formatDate(r.exportedAt))}`].filter(Boolean).join(" · ")}</p>`);
  const stat = (k: string, v: string) => `<div><dt>${esc(k)}</dt><dd>${esc(v)}</dd></div>`;
  b.push(`<dl class="stats">${[
    stat("Agent time", timeCell(r)),
    stat("Turns", `${r.stats.turns}${r.stats.errors ? ` (${r.stats.errors} with errors)` : ""}`),
    stat("Cost", formatCost(r)),
    stat("Changes", changesCell(r)),
    stat("Checks", checksSummary(r)),
    stat("PR", r.pr ? `#${r.pr.number} (${prState(r.pr)})` : "none yet"),
  ].join("")}</dl>`);

  if (r.task) b.push(`<h2>Task</h2><blockquote>${esc(r.task)}</blockquote>`);
  if (r.plan) b.push(`<h2>Plan</h2><div class="answer">${md(r.plan)}</div>`);

  b.push(`<h2>What the agent did</h2>`);
  const withItems = r.chats.filter((c) => c.items.length);
  if (!withItems.length) b.push(`<p class="note">No messages yet.</p>`);
  for (const c of withItems) {
    b.push(`<h3>${esc(c.title || "Chat")} · ${esc(chatLabel(c))}</h3>`);
    let tools: string[] = [];
    const flush = () => { if (tools.length) { b.push(`<ul class="tools">${tools.join("")}</ul>`); tools = []; } };
    for (const it of c.items) {
      if (it.kind === "tool") {
        const out = it.output ? `<details><summary>Output</summary><pre>${esc(it.output)}</pre></details>` : "";
        tools.push(`<li class="${it.failed ? "failed" : ""}">${it.failed ? `<span class="tag">Failed</span>` : ""}${esc(toolVerb(it.name))} <code>${esc(it.target)}</code>${out}</li>`);
        continue;
      }
      flush();
      if (it.kind === "user") b.push(`<div class="turn"><div class="who">You</div><blockquote>${esc(it.text)}</blockquote></div>`);
      else if (it.kind === "agent") b.push(`<div class="turn"><div class="who">${esc(agentName(c.agent))}</div><div class="answer">${md(it.text)}</div></div>`);
      else b.push(`<p class="note${it.bad ? " bad" : ""}">${esc(it.text)}</p>`);
    }
    flush();
  }

  b.push(`<h2>Changes</h2>`);
  b.push(r.files.length ? `<p>${plural(r.files.length, "file")} changed, +${r.additions} −${r.deletions} against <code>${esc(r.target)}</code>.</p>` : `<p>No changes against the target branch.</p>`);
  if (r.files.length) {
    b.push(`<table><thead><tr><th>File</th><th class="n">Added</th><th class="n">Removed</th></tr></thead><tbody>${r.files.map((f) => `<tr><td><code>${esc(f.path)}</code></td><td class="n">${f.additions}</td><td class="n">${f.deletions}</td></tr>`).join("")}</tbody></table>`);
  }
  for (const h of r.hunks) {
    const lines = h.text.split("\n").map((l) => `<span class="${l.startsWith("@@") ? "h" : l.startsWith("+") ? "a" : l.startsWith("-") ? "d" : ""}">${esc(l) || " "}</span>`).join("");
    b.push(`<h3><code>${esc(h.path)}</code></h3><pre class="diff">${lines}</pre>${h.more ? `<p class="note">${plural(h.more, "more hunk")} not shown.</p>` : ""}`);
  }

  b.push(`<h2>Checks</h2>`);
  b.push(r.checks.length
    ? `<ul>${r.checks.map((c) => `<li><b>${esc(c.name)}</b> <span class="${c.status === "passed" ? "ok" : "no"}">${esc(checkLine(c))}</span> <code>${esc(c.command)}</code></li>`).join("")}</ul>`
    : `<p class="note">No checks ran in NekoTrail for this workspace.</p>`);

  b.push(`<h2>Pull request</h2>`);
  if (r.pr) {
    const url = safeUrl(r.pr.url);
    const label = `#${r.pr.number} ${r.pr.title}`;
    b.push(`<p>${url ? `<a href="${esc(url)}" rel="noopener noreferrer">${esc(label)}</a>` : esc(label)} (${esc(prState(r.pr))})</p>`);
  } else b.push(`<p class="note">No PR yet.</p>`);
  if (r.footer) b.push(`<footer>Made with NekoTrail</footer>`);

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="referrer" content="no-referrer">
<meta name="color-scheme" content="light dark">
<title>${esc(r.title)}</title>
<style>${CSS}</style>
</head>
<body>
<main>
${b.join("\n")}
</main>
</body>
</html>
`;
}

/** A file name for the export: the title in lower-case words. */
export function exportFileName(r: Report, ext: "md" | "html"): string {
  const base = r.title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 60) || "session";
  return `${base}.${ext}`;
}
