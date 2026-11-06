/**
 * Sample data for rendering the real UI in a plain browser (no Tauri backend).
 * Open /preview.html?ws=w1&tab=chat (tabs: chat, changes, terminal, preview, checks, notes; ?view=sky).
 * Checks: ?ws=w1&tab=checks (a failing run), ?ws=w2 (running), ?ws=w4 (suggestions), ?checks=trust, ?checks=none.
 * Queue: ?view=queue (a night's worth of tasks), ?queue=empty, ?queue=paused, ?queueAdd (the Add form open), ?summary (the morning summary).
 * First run (a fresh install): ?fresh (nothing found), &found=conductor or &found=terminal (what "Where's your code?"
 * leads with), &noagents (no agent CLI yet), &gh=0 (GitHub not signed in), &step=code or &step=try (open on that
 * step; try comes with one repo). ?tour opens the coach-mark tour on any workspace.
 * All names, numbers and PRs here are made up for screenshots.
 */
import React from "react";
import ReactDOM from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import App from "./App";
import catalog from "../src-tauri/src/models.json";
import { highlights } from "./updateNotes";
import type { UpdatePhase } from "./useUpdater";
import { getSettings } from "./settings";
import { startAppearance } from "./appearance";
import { AGENTS } from "./agents";
import type { Agent } from "./api";

// A fresh install: no first-run marks, so the welcome flow and the clock start as they would.
const fresh = new URLSearchParams(location.search).has("fresh");
const freshFound = new URLSearchParams(location.search).get("found");
const freshStep = new URLSearchParams(location.search).get("step");
const freshRepos: any[] = [];
const freshWorkspaces: any[] = [];
// Cleared once per tab, so a reload keeps them (that is how "never twice" can be checked here).
if (!sessionStorage.getItem("nekotrail.previewStarted")) {
  sessionStorage.setItem("nekotrail.previewStarted", "1");
  for (const k of ["nekotrail.firstRun", "nekotrail.tour", "nekotrail.onboarding"]) localStorage.removeItem(k);
  // A later step means this install already had its first look (and started its clock).
  if (fresh && freshStep) { localStorage.setItem("nekotrail.onboarding", freshStep); localStorage.setItem("nekotrail.firstRun", JSON.stringify({ launch: Date.now() })); }
}
if (fresh && freshStep === "try") freshRepos.push({ id: "r2", name: "pawprint-web", root_path: "/code/pawprint-web", default_branch: "main", remote_url: null, setup_script: null, run_script: null, archive_script: null });
if (new URLSearchParams(location.search).get("filter")) localStorage.setItem("nekotrail.settings", JSON.stringify({ repoFilter: ["r1"], collapsed: [] }));
localStorage.setItem("nekotrail.sidebarHidden", new URLSearchParams(location.search).has("nosidebar") ? "1" : "0");
if (new URLSearchParams(location.search).get("split")) localStorage.setItem("nekotrail.split", "1");
if (new URLSearchParams(location.search).get("group") === "status") localStorage.setItem("nekotrail.settings", JSON.stringify({ groupBy: "status" }));
// Appearance for screenshots: ?theme=light|dark, ?zoom=1.2, ?codeSize=14 (CSS zoom here; the app zooms the web view).
{
  const q = new URLSearchParams(location.search);
  const s = getSettings();
  const theme = q.get("theme");
  if (theme === "light" || theme === "dark" || theme === "system") s.theme = theme;
  if (q.get("codeSize")) s.codeSize = Number(q.get("codeSize"));
  localStorage.setItem("nekotrail.settings", JSON.stringify(s));
  localStorage.setItem("nekotrail.zoom", q.get("zoom") ?? "1");
  startAppearance(getSettings);
}

// The update card: ?update=available | downloading | indeterminate | busy | ready | waiting | restarting | latest | error.
// The notes are sample text run through the real parser (markdown and HTML come out as plain text).
{
  const notes = [
    "**Updates** (sample notes for the preview)",
    "- The app tells you when a new version is out, and updates itself in the background.",
    "- Restarting waits for your agents: pick **Restart when idle** and it happens once they're done.",
    "- `⌘K` → Check for updates, also in Settings next to the version.",
    "- Faster diff view on <b>large</b> files.",
    "- Fixes the Run button losing its URL after a reload.",
    "",
    "### Install",
    "- Download the .dmg below",
  ].join("\n");
  const { items, more } = highlights(notes);
  const v = "0.11.0";
  const phases: Record<string, UpdatePhase> = {
    available: { kind: "available", version: v, current: "0.10.1", items, more },
    downloading: { kind: "downloading", version: v, got: 7.6 * 1_048_576, total: 18.2 * 1_048_576 },
    indeterminate: { kind: "downloading", version: v, got: 3.1 * 1_048_576, total: null },
    busy: { kind: "busy", version: v, agents: 2, terminals: 1 },
    ready: { kind: "busy", version: v, agents: 0, terminals: 0 },
    waiting: { kind: "waiting", version: v, agents: 1, terminals: 0 },
    restarting: { kind: "restarting", version: v },
    latest: { kind: "latest", version: "0.10.1" },
    error: { kind: "error", title: "The update didn't download", message: "Check your connection and try again, or download it from GitHub.", detail: "error sending request for url (https://github.com/Harshil-Jani/nekotrail/releases/download/v0.11.0/nekotrail.app.tar.gz)", version: v, retry: true },
  };
  const want = new URLSearchParams(location.search).get("update");
  if (want && phases[want]) window.__nekotrailUpdatePreview = phases[want];
}

// Sample review comments so the Changes tab shows them: a two-line range, one line, and one whose
// lines the agent has since changed (outdated). ?noreview starts with none. Kept in memory like the app's database.
const reviews: Record<string, any[]> = new URLSearchParams(location.search).has("noreview") ? {} : { w1: [
  { id: "c1", file: "internal/server/http.go", side: "new", line: 20, end_line: 21, code: "+\t\t// Must outlive the ALB idle timeout (60s), or the ALB reuses\n+\t\t// connections we already closed and clients see 502s.", text: "Link the AWS doc for the 60s default here." },
  { id: "c2", file: "internal/server/http.go", side: "new", line: 22, end_line: 22, code: "+\t\tIdleTimeout: 75 * time.Second,", text: "Read this from config instead, so ops can tune it per environment." },
  { id: "c3", file: "internal/server/http.go", side: "new", line: 24, end_line: 24, code: "+\t\tReadHeaderTimeout: 5 * time.Second,", text: "Is 5s enough for slow mobile clients?" },
] };
// ?review: two PRs wait for your review (?folded puts one draft outside the diff), and Biscuit (w6) is a read-only review of #731 with the
// agent's findings as drafts, one with a suggested change. ?ws=w6&tab=changes opens its drafts.
const reviewing = new URLSearchParams(location.search).has("review");
const reviewRequests = reviewing ? [
  { repo_id: "r1", number: 731, url: "https://github.com", title: "Retry webhook deliveries with backoff", isDraft: false, headRefName: "sam/webhook-retries", headRefOid: "b7c1d2e", baseRefName: "main", isCrossRepository: false, additions: 84, deletions: 12, changedFiles: 4, updatedAt: "2026-11-06T09:00:00Z", author: { login: "octocat", avatarUrl: "" } },
  { repo_id: "r2", number: 88, url: "https://github.com", title: "Annual plan toggle on the pricing page", isDraft: true, headRefName: "lee/annual", headRefOid: "9a8b7c6", baseRefName: "main", isCrossRepository: true, additions: 40, deletions: 3, changedFiles: 2, updatedAt: "2026-11-06T08:00:00Z", author: { login: "hubot", avatarUrl: "" } },
] : [];
if (reviewing) reviews.w6 = [
  { id: "f1", file: "internal/server/http.go", side: "new", line: 22, end_line: 22, code: "", text: "75s only covers the ALB. The gRPC port sits behind an NLB with a 350s idle timeout, so it still drops connections.\n\n```suggestion\n\t\tIdleTimeout: cfg.IdleTimeout,\n```" },
  { id: "f2", file: "internal/server/http.go", side: "new", line: 20, end_line: 21, code: "", text: "Link the AWS doc for the 60s default here, so the next person doesn't lower it." },
];

const repos = [
  { id: "r1", name: "pawprint-api", root_path: "/code/pawprint-api", default_branch: "main", remote_url: null, setup_script: "go mod download", run_script: "make dev", archive_script: null,
    prefs: new URLSearchParams(location.search).has("guard") ? { protected: [".env*", "migrations/**", "infra/**", "!migrations/README.md"] } : undefined },
  { id: "r2", name: "pawprint-web", root_path: "/code/pawprint-web", default_branch: "main", remote_url: null, setup_script: null, run_script: null, archive_script: null },
];
const pairOf = (id: string) => (id === "w1" || id === "w2" ? "p1" : "");
const ws = (id: string, repo_id: string, name: string, branch: string, extra = {}) => ({
  id, repo_id, name, branch, path: `/code/ws/${name}`, target_branch: "main", state: "active", pinned: false, unread: false, notes: "", imported_from: null, created_at: "2026-11-05T10:00:00Z", title: "", title_source: "", status: "", pair_id: pairOf(id), section_id: id === "w3" || id === "w4" ? "sec1" : "", parent_id: "", review_pr: 0, group_id: "", ...extra,
});
const planEvents = [
  { kind: "user", checkpoint: "c1", text: "Tickets should breach SLA after 4 business hours, not 4 hours. Plan it first." },
  { kind: "tool", id: "t1", name: "Read", input: "internal/tickets/sla.go" },
  { kind: "tool_result", id: "t1", output: "…", is_error: false },
  { kind: "tool", id: "t2", name: "TodoWrite", input: JSON.stringify([
    { content: "Add a business-hours calendar", status: "completed" },
    { content: "Use it in the SLA timer", status: "in_progress", activeForm: "Wiring the calendar into the SLA timer" },
    { content: "Tests for weekends and holidays", status: "pending" },
  ]) },
  { kind: "text", text: "How the SLA timer flows today:\n\n```mermaid\nflowchart LR\n  A[Ticket opened] --> B{Business hours?}\n  B -- yes --> C[Start 4h timer]\n  B -- no --> D[Wait for 9:00]\n  D --> C\n  C --> E[Breach alert]\n```" },
  { kind: "approval", request_id: "a1", tool: "Bash", input: "go test ./internal/tickets/... -run TestBusinessHours", description: "Run the SLA tests", can_always: true },
  { kind: "tool", id: "t3", name: "ExitPlanMode", input: "## Plan\n1. Add `BusinessHours` (9:00 to 18:00, Mon to Fri, holiday list from config).\n2. `slaDeadline` adds 4 business hours instead of 4 wall-clock hours.\n3. Table tests: Friday 17:00 rolls to Monday 12:00; holidays skip.\n\nNo schema changes." },
];

// ?opencode makes Comet's chat an OpenCode chat in "Ask before changes", mid-turn, waiting on an edit.
const opencode = new URLSearchParams(location.search).has("opencode");
const agentsParam = new URLSearchParams(location.search).get("agents");
const installed: Agent[] = agentsParam === "all" ? AGENTS : ["claude", ...((agentsParam ?? "copilot").split(",").filter(Boolean) as Agent[])];
if (opencode) planEvents.splice(planEvents.findIndex((e) => e.kind === "approval"), 1, { kind: "approval", request_id: "a2", tool: "Edit", input: "internal/tickets/sla.go", description: "Edit internal/tickets/sla.go", can_always: true });

const workspaces = [
  // Titles come from the PR (Mochi), the first prompt (Comet, Ziggy) or the user (Dash); Pip has none yet.
  ws("w1", "r1", "mochi", "harshil/fix-502-gateway", { pinned: true, title: "Fix 502s after deploy", title_source: "pr" }),
  ws("w2", "r1", "comet", "harshil/ticket-sla", { unread: true, title: "Tickets should breach SLA after 4 business hours…", title_source: "auto" }),
  // Dash is stacked on Mochi: it started from Mochi's branch and its PR goes there.
  ws("w3", "r1", "dash", "harshil/rate-limit", { parent_id: "w1", target_branch: "harshil/fix-502-gateway", title: "Rate limit the tickets API", title_source: "user" }),
  ws("w4", "r2", "ziggy", "harshil/pricing-page", { title: "Pricing page with an annual toggle", title_source: "auto" }),
  ws("w5", "r2", "pip", "harshil/blog-rss"),
];
if (new URLSearchParams(location.search).has("review")) workspaces.push(ws("w6", "r1", "biscuit", "review/pr-731", { title: "Retry webhook deliveries with backoff", title_source: "pr", review_pr: 731 }));
// ?history&ws=w7: a repo's History workspace (terminal chats from the main checkout, read-only).
// ?multi&ws=g1: a task across three repos (api, web, types) on one branch, the chat in the api one.
// ?multi=green: every PR green, so Merge all can run; ?multi=nopr: no PRs yet; default: one check still running.
const multi = new URLSearchParams(location.search).get("multi");
const multiMerged = new Set<string>();
if (multi !== null) {
  repos.push({ id: "r3", name: "pawprint-types", root_path: "/code/pawprint-types", default_branch: "main", remote_url: null, setup_script: null, run_script: null, archive_script: null });
  const t = { group_id: "g1", created_at: "2026-11-06T10:00:00Z" };
  workspaces.push(
    ws("g1", "r1", "vega", "harshil/order-export", { ...t, title: "Export orders as CSV", title_source: "auto", path: "/code/ws/pawprint-api/vega" }),
    ws("g2", "r2", "vega", "harshil/order-export", { ...t, path: "/code/ws/pawprint-web/vega", created_at: "2026-11-06T10:00:01Z" }),
    ws("g3", "r3", "vega", "harshil/order-export", { ...t, path: "/code/ws/pawprint-types/vega", created_at: "2026-11-06T10:00:02Z" }),
  );
}
if (new URLSearchParams(location.search).has("history")) workspaces.push(ws("w7", "r1", "history", "main", { path: "/code/pawprint-api", title: "History", title_source: "user", imported_from: "terminal-history" }));
// ?many adds 20 more workspaces (most titled, a few not yet) to judge the sidebar's density.
if (new URLSearchParams(location.search).has("many")) {
  const tasks = ["Retry webhooks with backoff", "Move the SLA timer to the worker", "", "Dark mode for the ticket view", "Paginate /v1/tickets with cursors", "Fix flaky login test on CI", "", "Add audit log for admin actions", "Upgrade to Go 1.24", "CSV export for reports", "Stop double-sending reminder emails", "Rate limit password resets", "", "Search tickets by customer email", "Bump Next to 16 and fix the build", "Explain the billing retry logic", "Cache the pricing API for 5 minutes", "Add Sentry to the worker", "Translate onboarding into Hindi", "Remove the old v0 endpoints"];
  const stars = ["biscuit", "tofu", "dumpling", "pudding", "muffin", "pepper", "rocket", "flick", "button", "bean", "peanut", "pebble", "tiny", "biscuit", "tofu", "dumpling", "pudding", "muffin", "pepper", "rocket"];
  tasks.forEach((t, i) => workspaces.push(ws(`m${i}`, i % 3 ? "r1" : "r2", stars[i], `harshil/task-${i}`, t ? { title: t, title_source: i % 4 ? "auto" : "pr" } : {})));
}
const events = [
  { kind: "user", checkpoint: "abc123", text: "The load balancer returns 502 on /v1/tickets right after a deploy. Find out why and fix it." },
  { kind: "tool", id: "t1", name: "Bash", input: "kubectl logs deploy/pawprint-api --since=30m | grep 502" },
  { kind: "tool_result", id: "t1", output: "2026-11-05T09:12:44Z http: proxy error: read tcp 10.0.3.14:43120->10.0.7.2:8080: connection reset by peer", is_error: false },
  { kind: "tool", id: "t2", name: "Read", input: "internal/server/http.go" },
  { kind: "tool_result", id: "t2", output: "srv := &http.Server{\n  Addr: \":8080\",\n  IdleTimeout: 30 * time.Second,\n}", is_error: false },
  { kind: "text", text: "Found it. The server closes idle keep-alive connections after **30s**, but the load balancer keeps them for **60s**. After a quiet minute the balancer reuses a connection the server already closed, and the client gets a 502.\n\nI raised `IdleTimeout` to 75s so the server always outlives the balancer, and added a test that fails on the old value." },
  { kind: "tool", id: "t3", name: "Edit", input: "internal/server/http.go" },
  { kind: "tool_result", id: "t3", output: "", is_error: false },
  { kind: "tool", id: "t4", name: "Bash", input: "go test ./internal/server/..." },
  { kind: "tool_result", id: "t4", output: "ok  \tpawprint/internal/server\t0.412s", is_error: false },
  { kind: "text", text: "Review of the branch:\n- internal/server/http.go:22: 75s is right for the ALB, but the gRPC port sits behind an NLB with a 350s timeout. Fix: make it configurable.\n- internal/server/http_test.go:14: the test sleeps 61s. Fix: inject a clock.\n\nOtherwise ready to merge." },
  { kind: "done", is_error: false, duration_ms: 41200, cost_usd: 0.18, context_tokens: 128000, context_window: 200000 },
  { kind: "checks", results: [{ name: "test", status: "failed", passed: 41, failed: 2, regressed: true }, { name: "lint", status: "passed", passed: null, failed: 0 , regressed: false }, { name: "typecheck", status: "passed", passed: null, failed: 0, regressed: false }] },
];
// ?rewound shows the marker a rewind leaves, with its Undo.
if (location.search.includes("rewound")) events.push({ kind: "rewind", text: "Rewound to before: The load balancer returns 502 on /v1/tickets right after a deploy. Find out why and fix it.", undo: "def456", files: 2 } as any);
// ?guard shows the protected-files cards: a refused edit, a turn caught after (one file, Put it back),
// one that got past the block (three files), and one already put back. Mochi's repo protects .env*, migrations/** and infra/**.
const guard = new URLSearchParams(location.search).has("guard");
if (guard) {
  events.splice(events.length - 1, 0,
    { kind: "blocked", text: "OpenCode tried to change a protected file, so nekotrail said no.", files: [".env.production"] } as any,
    { kind: "guard", files: [{ path: "migrations/0042_ticket_sla.sql", change: "changed" }], checkpoint: "g1", ignored: null, level: "asks" } as any,
    { kind: "guard", files: [{ path: ".env", change: "changed" }, { path: "infra/terraform/alb.tf", change: "changed" }, { path: "migrations/0043_backfill_sla_deadlines_for_open_tickets.sql", change: "added" }], checkpoint: "g2", ignored: "i2", level: "blocked" } as any,
    { kind: "guard", files: [{ path: "infra/terraform/alb.tf", change: "changed" }], checkpoint: "g3", ignored: null, level: "after" } as any,
    { kind: "guard_restored", files: ["infra/terraform/alb.tf"], checkpoint: "g3" } as any,
  );
}
// Shell commands for the "Commands the agent ran" sheet (?commands=w1): a long one, a failed one, one cut off mid-turn.
const codexEvents = [
  { kind: "user", checkpoint: "x1", text: "Second opinion on the idle timeout fix." },
  { kind: "tool", id: "c1", name: "Bash", input: "rg -n \"IdleTimeout|ReadHeaderTimeout|WriteTimeout\" --type go internal/ cmd/" },
  { kind: "tool_result", id: "c1", output: "internal/server/http.go:22:\t\tIdleTimeout: 75 * time.Second,\ninternal/server/http.go:24:\t\tReadHeaderTimeout: 5 * time.Second,", is_error: false },
  { kind: "tool", id: "c2", name: "Bash", input: "aws elbv2 describe-load-balancer-attributes --load-balancer-arn arn:aws:elasticloadbalancing:ap-south-1:123456789012:loadbalancer/app/pawprint-api-prod/50dc6c495c0c9188 --query 'Attributes[?Key==`idle_timeout.timeout_seconds`].Value' --output text" },
  { kind: "tool_result", id: "c2", output: "An error occurred (AccessDenied) when calling the DescribeLoadBalancerAttributes operation: User is not authorized to perform: elasticloadbalancing:DescribeLoadBalancerAttributes", is_error: true },
  { kind: "tool", id: "c3", name: "Bash", input: "go test ./... -run TestIdle -count=1" },
  { kind: "tool_result", id: "c3", output: "", is_error: false },
  { kind: "tool", id: "c4", name: "Bash", input: "go test -race ./internal/server/..." },
];
// ⌘⇧F: an archived workspace (Bean) with a chat of its own, and a search over every sample chat.
// ?search opens the sheet; ?q=retry types a query; ?indexing shows the one-time indexing of older chats.
if (new URLSearchParams(location.search).get("q")) sessionStorage.setItem("nekotrail.search", new URLSearchParams(location.search).get("q")!);
const archivedWs = ws("w9", "r1", "bean", "harshil/webhook-retry", { state: "archived", title: "Retry webhooks with backoff", title_source: "pr" });
const archivedEvents = [
  { kind: "user", text: "Webhooks to Stripe fail now and then with a 503. Retry them with backoff, at most 5 times." },
  { kind: "tool", id: "a1", name: "Read", input: "internal/webhooks/sender.go" },
  { kind: "tool_result", id: "a1", output: "…", is_error: false },
  { kind: "text", text: "Added exponential backoff with jitter (1s, 2s, 4s, 8s, 16s). A 4xx is never retried, since sending it again won't help." },
  { kind: "tool", id: "a2", name: "Bash", input: "go test ./internal/webhooks/... -run TestRetry" },
  { kind: "tool_result", id: "a2", output: "ok", is_error: false },
  { kind: "done", is_error: false, duration_ms: 52000, cost_usd: 0.31 },
];
const corpus = (() => {
  const day = (d: number) => new Date(Date.now() - d * 86_400_000).toISOString();
  const out: { id: number; ws: string; session: string; index: number; total: number; ev: any; at: string; agent: string; chat: string }[] = [];
  const add = (w: string, session: string, evs: any[], agent: string, chat: string, at: string) =>
    evs.forEach((ev, index) => out.push({ id: out.length + 1, ws: w, session, index, total: evs.length, ev, at, agent, chat }));
  add("w1", "s1", events, "claude", "Fix 502 after deploy", day(0.1));
  add("w2", "s9", planEvents, "claude", "New chat", day(1.2));
  add("w9", "s7", archivedEvents, "codex", "Webhook retries", day(12));
  return out;
})();
const searchKind = (ev: any) => ev.kind === "user" ? "user" : ev.kind === "text" ? "agent" : ev.kind === "tool" ? (ev.name === "Bash" ? "command" : ["Read", "Edit", "Write"].includes(ev.name) ? "file" : null) : null;
function previewSearch(q: string, f: any) {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  const groups: any[] = [];
  for (const c of corpus) {
    const kind = searchKind(c.ev);
    const text: string = c.ev.kind === "tool" ? c.ev.input : c.ev.text;
    if (!kind || !words.length || !words.every((w) => text.toLowerCase().includes(w))) continue;
    if (f.kind && f.kind !== kind) continue;
    if (f.agent && f.agent !== c.agent) continue;
    if (f.since && c.at < f.since) continue;
    const w = [...workspaces, archivedWs].find((x) => x.id === c.ws)!;
    if (f.repo && f.repo !== w.repo_id) continue;
    if (f.workspace && f.workspace !== w.id) continue;
    const first = text.toLowerCase().indexOf(words[0]);
    let snip = (first > 50 ? "…" : "") + text.slice(Math.max(0, first - 50), first + 110) + (first + 110 < text.length ? "…" : "");
    for (const wd of words) snip = snip.replace(new RegExp(wd.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi"), (m) => `\u0002${m}\u0003`);
    let g = groups.find((x) => x.workspace_id === w.id);
    if (!g) groups.push(g = { workspace_id: w.id, repo_id: w.repo_id, repo: repos.find((r) => r.id === w.repo_id)!.name, name: w.name, title: w.title, branch: w.branch, state: w.state, hits: [], more: 0 });
    g.hits.push({ message_id: c.id, session_id: c.session, kind, snippet: snip, created_at: c.at, agent: c.agent, chat: c.chat });
  }
  return { groups, indexing: new URLSearchParams(location.search).has("indexing") ? { done: 7120, total: 19844, finished: false } : null };
}
const pr = {
  number: 482, url: "https://github.com", state: "OPEN", title: "Fix 502s after deploy: outlive the ALB idle timeout", isDraft: false,
  reviewDecision: "REVIEW_REQUIRED", mergeable: "MERGEABLE", additions: 38, deletions: 1, commentCount: 2,
  author: { login: "Harshil-Jani" },
  latestReviews: [{ author: { login: "octocat" }, state: "APPROVED" }],
  reviewRequests: [{ login: "hubot" }],
  statusCheckRollup: Array.from({ length: 12 }, (_, i) => ({ name: `check-${i}`, conclusion: "SUCCESS" })),
};
const diff = `diff --git a/internal/server/http.go b/internal/server/http.go
@@ -18,7 +18,9 @@ func New(cfg Config) *http.Server {
 	srv := &http.Server{
 		Addr:    ":8080",
-		IdleTimeout: 30 * time.Second,
+		// Must outlive the ALB idle timeout (60s), or the ALB reuses
+		// connections we already closed and clients see 502s.
+		IdleTimeout: 75 * time.Second,
 	}`;

// Ports (?ports opens the popover). Each workspace has a block of 10 from 4100. ?ports=empty, =error and
// =loading show those states; ?portclash raises the toast a Run script gives when its port is taken;
// ?portProblem makes Preview say the app landed outside its block (use with ?tab=preview&previewDown).
const portsMode = new URLSearchParams(location.search).get("ports");
const blockOf = (id: string) => 4100 + Math.max(0, ["w1", "w2", "w3", "w4", "w5", "w6"].indexOf(id)) * 10;
const holder = (command: string, pid: number, workspace_id: string | null, can_free = true) => ({ pid, command, workspace_id, how: workspace_id ? "tree" : null, can_free });
const portsList = () => [
  { id: "w1", base: 4100, main: 4100, problems: [], rows: [
    { port: 4100, vars: ["NEKOTRAIL_PORT", "PORT", "WEB_PORT"], name: "web", outside: false, holder: holder("node", 41230, "w1") },
    { port: 4101, vars: ["NEKOTRAIL_PORT_1", "API_PORT"], name: "api", outside: false, holder: holder("api", 41388, "w1") },
    { port: 4102, vars: ["NEKOTRAIL_PORT_2", "ADMIN_PORT"], name: "admin", outside: false, holder: null },
  ] },
  { id: "w2", base: 4110, main: 4110, rows: [
    { port: 4110, vars: ["NEKOTRAIL_PORT", "PORT"], name: null, outside: false, holder: null },
    { port: 3000, vars: [], name: null, outside: true, holder: holder("next-server", 52011, "w2") },
  ], problems: [{ port: 3000, text: "Comet's app is on 3000, not its own port 4110. It ignored PORT.", fix: "Start it with `next dev -p $PORT`, or set `\"dev\": \"next dev -p $PORT\"` in package.json." }] },
  { id: "w4", base: 4130, main: 4130, problems: [], rows: [
    { port: 4130, vars: ["NEKOTRAIL_PORT", "PORT"], name: null, outside: false, holder: holder("vite", 50112, null, false) },
  ] },
  // ?multi: each repo of the task has its own block, named "<task>, <repo>".
  ...(new URLSearchParams(location.search).get("multi") !== null ? [
    { id: "g1", base: 4170, main: 4170, problems: [], rows: [{ port: 4170, vars: ["NEKOTRAIL_PORT", "PORT"], name: null, outside: false, holder: holder("api", 61200, "g1") }] },
    { id: "g2", base: 4180, main: 4180, problems: [], rows: [{ port: 4180, vars: ["NEKOTRAIL_PORT", "PORT"], name: null, outside: false, holder: holder("vite", 61201, "g2") }] },
  ] : []),
];
if (new URLSearchParams(location.search).has("portclash")) setTimeout(() => emit("port-clash", { workspace_id: "w2", text: "Comet's Run script stopped: port 3000 is taken by Mochi (node, pid 41230).", fix: "Use `$PORT` in the Run script so each workspace gets its own." }), 1200);
// ?link: a nekotrail://new link waiting for "Start this task?". ?link=long has a long prompt and adds the repo.
const linkMode = new URLSearchParams(location.search).get("link");
let linkTaken = false;
// The Checks tab. Mochi (w1): test fails after passing before, lint and vet pass. Comet (w2): a run going.
// pawprint-web (w4, w5): no checks yet, so suggestions. ?checks=trust: nekotrail.json checks wait for an OK.
const checksMode = new URLSearchParams(location.search).get("checks");
const goChecks = [
  { name: "test", command: "go test ./...", source: "app" },
  { name: "lint", command: "golangci-lint run", source: "app" },
  { name: "vet", command: "go vet ./...", source: "app" },
];
const checkResults = [
  {
    name: "test", command: "go test ./...", status: "failed", passed: 41, failed: 2, formats: ["go test"], exit_code: 1, duration_ms: 8412, finished_at: "2026-11-06T14:02:11Z", regressed: true,
    failures: [
      { title: "TestIdleTimeout/behind_nlb", file: "internal/server/http_test.go", line: 31, message: "IdleTimeout = 75s, want at least 350s for the NLB listener",
        snippet: "  28 | \tt.Run(\"behind_nlb\", func(t *testing.T) {\n  29 | \t\tsrv := New(Config{Listener: NLB})\n  30 | \t\tif srv.IdleTimeout < 350*time.Second {\n> 31 | \t\t\tt.Errorf(\"IdleTimeout = %s, want at least 350s for the NLB listener\", srv.IdleTimeout)\n  32 | \t\t}\n  33 | \t})" },
      { title: "TestSLA/friday_evening", file: "internal/tickets/sla_test.go", line: 42, message: "deadline = 2026-11-06 21:00, want 2026-11-09 12:00\nbusiness hours skipped the weekend" },
    ],
  },
  { name: "lint", command: "golangci-lint run", status: "passed", passed: null, failed: 0, failures: [], formats: [], exit_code: 0, duration_ms: 3120, finished_at: "2026-11-06T14:02:14Z", regressed: false },
  { name: "vet", command: "go vet ./...", status: "passed", passed: null, failed: 0, failures: [], formats: [], exit_code: 0, duration_ms: 1840, finished_at: "2026-11-06T14:02:16Z", regressed: false },
];
const checksFor = (id: string) => {
  const base = { trust: null, suggestions: [], after_turn: true, results: [], active: false, current: null, queued: [] };
  if (checksMode === "trust") return { ...base, checks: [], after_turn: false, trust: { fingerprint: "f", checks: [["lint", "golangci-lint run ./..."], ["test", "go test -race ./..."]] } };
  if (checksMode === "none") return { ...base, checks: [], after_turn: false };
  if (id === "w1") return { ...base, checks: goChecks, results: checkResults };
  if (id === "w2") return { ...base, checks: goChecks, results: checkResults.slice(1), active: true, current: "test", queued: ["vet"] };
  if (id === "w3") return { ...base, checks: goChecks, after_turn: false };
  return { ...base, checks: [], after_turn: false, suggestions: [["test", "pnpm test"], ["lint", "pnpm lint"], ["typecheck", "pnpm exec tsc --noEmit"]] };
};
const checkOutput = "--- FAIL: TestIdleTimeout (0.00s)\n    --- FAIL: TestIdleTimeout/behind_nlb (0.00s)\n        http_test.go:31: IdleTimeout = 75s, want at least 350s for the NLB listener\nFAIL\nFAIL\tpawprint/internal/server\t0.412s\n--- FAIL: TestSLA (0.00s)\n    --- FAIL: TestSLA/friday_evening (0.00s)\n        sla_test.go:42: deadline = 2026-11-06 21:00, want 2026-11-09 12:00\n            business hours skipped the weekend\nFAIL\nFAIL\tpawprint/internal/tickets\t0.012s\nok  \tpawprint/internal/billing\t0.220s\nFAIL";

// The prompt library. A small stand-in for the backend's parser (src-tauri/src/prompts.rs), enough
// for screenshots: blanks, defaults, built-ins and \{{ escapes. ?noprompts starts with none of yours.
const BUILTIN = ["branch", "base", "files_changed", "selection"];
function segsOf(text: string) {
  const out: any[] = [];
  let buf = "";
  for (let i = 0; i < text.length;) {
    if (text.startsWith("\\{{", i)) { buf += "{{"; i += 3; continue; }
    if (text.startsWith("{{", i)) {
      const end = text.indexOf("}}", i + 2);
      const m = end > 0 ? /^\s*([\w-]+)\s*(?::(.*))?$/.exec(text.slice(i + 2, end)) : null;
      if (m && !text.slice(i + 2, end).includes("\n")) {
        if (buf) { out.push({ t: "text", text: buf }); buf = ""; }
        out.push({ t: "field", name: m[1], default: m[2]?.trim() || null, builtin: BUILTIN.includes(m[1]) });
        i = end + 2;
        continue;
      }
    }
    buf += text[i++];
  }
  if (buf) out.push({ t: "text", text: buf });
  return out;
}
let personalPrompts: { name: string; description: string; text: string }[] | null = new URLSearchParams(location.search).has("noprompts") ? null : [
  { name: "tests", description: "Tests for the last change", text: "Write tests for what you just changed. Cover the edge cases, run them, and fix anything that fails." },
  { name: "fix-ticket", description: "Fix a ticket on this branch", text: "Fix {{ticket}} on {{branch}}. Keep the change small, add a test that fails before the fix, and target {{env:staging}} first." },
  { name: "explain", description: "Explain the selected code", text: "Explain this code from the diff, and what could break if we change it:\n\n{{selection}}" },
  { name: "release", description: "My release checklist", text: "Bump the version and write the changelog." },
];
const repoPrompts = [
  { name: "release", description: "Cut a release the team's way", text: "Bump the version in {{file:package.json}}, update CHANGELOG.md for {{version}}, and open the release PR against {{base}}.", source: ".nekotrail/prompts/release.md" },
  { name: "migration", description: "Write a safe database migration", text: "Write a migration for {{change}}. It must be reversible and safe to run while the old code is still serving traffic.", source: "nekotrail.json" },
];
const claudeCommands = [
  { name: "ship", description: "Run tests, bump the version and open the release PR", text: "Run the tests, bump the version and open the release PR for $ARGUMENTS", source: "~/.claude/commands/ship.md" },
  { name: "triage", description: "Label and prioritise new issues", text: "Label and prioritise the new issues.", source: ".claude/commands/triage.md" },
  { name: "tests", description: "Claude's own tests command", text: "Write tests.", source: "~/.claude/commands/tests.md" },
];
function promptEntries() {
  const all: any[] = [];
  const tiers: [string, any[]][] = [["repo", repoPrompts], ["personal", (personalPrompts ?? []).map((x) => ({ ...x, source: "" }))], ["claude", claudeCommands]];
  for (const [scope, list] of tiers) for (const x of list) {
    const top = all.find((e) => e.name === x.name && !e.hidden_by);
    all.push({ ...x, segs: scope === "claude" ? [{ t: "text", text: x.text }] : segsOf(x.text), scope, read_only: scope !== "personal", hidden_by: top ? top.scope : null });
  }
  return all.sort((a, b) => a.name.localeCompare(b.name) || Number(!!a.hidden_by) - Number(!!b.hidden_by));
}

// The Rules sheet (?rules=w1, &rulesPane=edit|task, &rulesPlan to open the plan): AGENTS.md as the
// source, a CLAUDE.md that drifted, Copilot's file saying the same, a Cursor rules folder.
const rulesAgents = "claude codex gemini opencode cursor copilot goose qwen kimi auggie grok kiro droid vibe cline kilo".split(" ");
const agentsMdText = "# acme-api\n\n## Build and test\n- `make dev` runs the API on $PORT.\n- `make test` before every commit.\n\n## Style\n- Go 1.23, gofmt, no new dependencies without asking.\n- Errors wrap with `fmt.Errorf(\"...: %w\", err)`.\n\n## Don't\n- Touch `migrations/` by hand.\n";
const claudeMdText = "# acme-api\n\n## Build and test\n- `make dev` runs the API on port 3000.\n- `make test` before every commit.\n\n## Style\n- Go 1.22, gofmt.\n- Errors wrap with `fmt.Errorf(\"...: %w\", err)`.\n\n## Claude\n- Use plan mode for anything under `billing/`.\n";
let rulesText: Record<string, string> = { "AGENTS.md": agentsMdText, "CLAUDE.md": claudeMdText, ".github/copilot-instructions.md": agentsMdText };
let taskRulesText = "Keep the change under 200 lines. Don't change the public API.";
const rulesReaders = (path: string) => {
  const r = (agent: string, active = true, when = "") => ({ agent, active, when });
  if (path === "AGENTS.md") return rulesAgents.map((a) => a === "claude" ? r(a, false, "only when there's no CLAUDE.md or CLAUDE.local.md") : a === "gemini" ? r(a, false, "only when context.fileName in .gemini/settings.json lists it") : r(a));
  if (path === "CLAUDE.md") return [r("claude"), r("opencode", false, "only when there's no AGENTS.md"), r("cursor"), r("copilot"), r("auggie", false, "only when there's no AGENTS.md"), r("droid")];
  if (path === "GEMINI.md") return [r("gemini", false), r("copilot", false)];
  if (path === ".github/copilot-instructions.md") return [r("copilot")];
  return [r("cursor")];
};
const rulesDiff = [
  { op: "gap", text: "3 same lines" },
  { op: "del", text: "- `make dev` runs the API on $PORT." },
  { op: "add", text: "- `make dev` runs the API on port 3000." },
  { op: "same", text: "- `make test` before every commit." },
  { op: "same", text: "" },
  { op: "same", text: "## Style" },
  { op: "del", text: "- Go 1.23, gofmt, no new dependencies without asking." },
  { op: "add", text: "- Go 1.22, gofmt." },
  { op: "same", text: "- Errors wrap with `fmt.Errorf(\"...: %w\", err)`." },
  { op: "same", text: "" },
  { op: "del", text: "## Don't" },
  { op: "del", text: "- Touch `migrations/` by hand." },
  { op: "add", text: "## Claude" },
  { op: "add", text: "- Use plan mode for anything under `billing/`." },
];
const rulesOverview = () => ({
  source: "AGENTS.md", source_exists: true, sources: ["AGENTS.md", "CLAUDE.md"],
  warnings: [],
  files: [
    { path: "AGENTS.md", role: "shared", dir: false, exists: true, readers: rulesReaders("AGENTS.md"), note: "", state: "source", link: null, points_to: null, via: null, diff: [], lines: 13, files: [] },
    { path: "CLAUDE.md", role: "shared", dir: false, exists: true, readers: rulesReaders("CLAUDE.md"), note: "", state: "differs", link: null, points_to: null, via: null, diff: rulesDiff, lines: 12, files: [] },
    { path: "GEMINI.md", role: "shared", dir: false, exists: false, readers: rulesReaders("GEMINI.md"), note: "", state: "missing", link: null, points_to: null, via: null, diff: [], lines: 0, files: [] },
    { path: ".github/copilot-instructions.md", role: "shared", dir: false, exists: true, readers: rulesReaders(".github/copilot-instructions.md"), note: "", state: "same", link: null, points_to: null, via: null, diff: [], lines: 13, files: [] },
    { path: ".cursor/rules", role: "scoped", dir: true, exists: true, readers: rulesReaders(".cursor/rules"), note: "", state: "folder", link: null, points_to: null, via: null, diff: [], lines: 0, files: [".cursor/rules/go.mdc", ".cursor/rules/sql.mdc"] },
  ],
  agents: rulesAgents.map((agent) => {
    const reads = agent === "claude" ? ["CLAUDE.md"] : agent === "gemini" ? [] : agent === "copilot" ? ["AGENTS.md", "CLAUDE.md", ".github/copilot-instructions.md"] : agent === "cursor" ? ["AGENTS.md", "CLAUDE.md", ".cursor/rules/"] : ["AGENTS.md"];
    return { agent, reads, gets_source: agent !== "claude" && agent !== "gemini", doc: "https://example.com" };
  }),
});
const rulesPlan = (req: any) => {
  const how = (p: string, d: string) => req.choices?.[p] ?? d;
  const gem = how("GEMINI.md", "keep");
  return {
    source: "AGENTS.md", token: "t", notes: [],
    items: [
      { path: "CLAUDE.md", how: how("CLAUDE.md", "import"), options: ["import", "copy", "symlink", "keep"], status: how("CLAUDE.md", "import") === "keep" ? "keep" : "replace", needs_confirm: how("CLAUDE.md", "import") !== "keep", exists: true,
        why: "Claude doesn't read AGENTS.md, so this file imports it.", new: "",
        diff: [...claudeMdText.trimEnd().split("\n").map((t) => ({ op: "del", text: t })), { op: "add", text: "<!-- Claude reads this file, so it imports AGENTS.md. Edit AGENTS.md, not this file. -->" }, { op: "add", text: "@AGENTS.md" }] },
      { path: "GEMINI.md", how: gem, options: ["import", "copy", "symlink", "keep"], status: gem === "keep" ? "keep" : "create", needs_confirm: false, exists: false,
        why: gem === "keep" ? "Gemini doesn't read AGENTS.md. Pick Import it to make this file." : "Gemini doesn't read AGENTS.md, so this file imports it.", new: "",
        diff: gem === "keep" ? [] : [{ op: "add", text: "<!-- Gemini reads this file, so it imports AGENTS.md. Edit AGENTS.md, not this file. -->" }, { op: "add", text: "@./AGENTS.md" }] },
      { path: ".github/copilot-instructions.md", how: how(".github/copilot-instructions.md", "note"), options: ["note", "keep"], status: "replace", needs_confirm: false, exists: true,
        why: "Copilot reads AGENTS.md too, so this only needs to say where the rules are.", new: "",
        diff: [{ op: "gap", text: "13 lines, the same as AGENTS.md" }, { op: "add", text: "<!-- nekotrail:points-to AGENTS.md -->" }, { op: "add", text: "This project's rules live in AGENTS.md, which Copilot also reads. Edit AGENTS.md, not this file." }] },
    ],
  };
};

// The task queue: Claude ran out of usage at night, so its tasks wait until 2 h 40 min from now.
const queueMode = new URLSearchParams(location.search).get("queue");
const qNow = Math.floor(Date.now() / 1000);
const qTask = (id: string, prompt: string, extra: Record<string, unknown>) => ({
  id, prompt, repo_id: "r1", base: null, agent: "claude", model: null, effort: null, mode: "acceptEdits", fallback: null,
  then: { checks: false, draft_pr: false, autopilot: false }, state: "queued", note: "", step: "", workspace_id: null, session_id: null,
  ran_with: null, resume: false, attempts: 0, added_at: qNow - 9 * 3600, started_at: null, ended_at: null, ...extra,
});
let queueTasks: any[] = queueMode === "empty" || fresh ? [] : [
  qTask("q1", "Fix the flaky login test on CI. It times out about one run in five.", { state: "running", ran_with: "claude", started_at: qNow - 14 * 60, workspace_id: "w1", session_id: "s1", then: { checks: true, draft_pr: true, autopilot: false } }),
  qTask("q2", "Add rate limiting to the tickets API, 100 requests a minute per token", { state: "running", ran_with: "claude", mode: "ask", started_at: qNow - 41 * 60, workspace_id: "w2", session_id: "s2" }),
  qTask("q3", "Bump Go to 1.25 and fix the deprecation warnings", { state: "then", step: "autopilot", agent: "codex", ran_with: "codex", started_at: qNow - 2 * 3600, workspace_id: "w3", note: "Autopilot is getting the PR green.", then: { checks: false, draft_pr: true, autopilot: true } }),
  qTask("q4", "Move the settings page to the new form components", { repo_id: "r2", resume: true, workspace_id: "w4", note: "Claude ran out of usage. It continues when the limit resets.", then: { checks: true, draft_pr: true, autopilot: false } }),
  qTask("q5", "Write tests for the webhook retry backoff", { agent: "codex", fallback: "claude", base: "release/2.4" }),
  qTask("q6", "Rotate the staging database password and update the runbook", { mode: "ask", state: "waiting" }),
  qTask("q7", "Remove the old v1 export endpoint and its docs", { fallback: "codex", mode: "bypassPermissions" }),
  qTask("q8", "Add a dark mode toggle to the settings page", { repo_id: "r2", state: "done", ran_with: "claude", workspace_id: "w5", ended_at: qNow - 3 * 3600, note: "Finished. Opened a draft PR.", then: { checks: false, draft_pr: true, autopilot: false } }),
  qTask("q9", "Plan the move from REST to tRPC for the dashboard", { repo_id: "r2", mode: "plan", state: "needs_you", ran_with: "claude", workspace_id: "w4", ended_at: qNow - 4 * 3600, note: "The plan waits for your OK." }),
  qTask("q10", "Upgrade React Native to 0.79", { repo_id: "r2", agent: "gemini", state: "failed", ran_with: "gemini", ended_at: qNow - 5 * 3600, note: "Gemini isn't installed." }),
  qTask("q11", "Fix the 502s after deploy", { state: "done", ran_with: "claude", workspace_id: "w1", ended_at: qNow - 6 * 3600, note: "Autopilot got the PR green. It's ready for you to merge.", then: { checks: true, draft_pr: true, autopilot: true } }),
];
let queuePaused = queueMode === "paused";
let queueMax = [3, 3];
const claudeBack = qNow + 2 * 3600 + 40 * 60;
const queueView = () => {
  const waits: Record<string, any> = {};
  for (const t of queueTasks.filter((x) => x.state === "queued")) {
    waits[t.id] = queuePaused ? { kind: "paused" }
      : t.agent === "claude" && t.fallback !== "codex" ? { kind: "limit", agent: "claude", until: claudeBack, reported: true }
      : { kind: "slots" };
  }
  return {
    tasks: queueTasks, paused: queuePaused, max_total: queueMax[0], max_per_repo: queueMax[1], now: Math.floor(Date.now() / 1000),
    limits: queueMode === "empty" ? {} : { claude: { until: claudeBack, reported: true, text: "Claude AI usage limit reached" } }, waits, asking: ["q2"],
  };
};
const queueChanged = () => setTimeout(() => emit("task-queue", {}), 0);

mockIPC(
  (cmd, args: any) => {
    switch (cmd) {
      case "queue_view": return queueView();
      case "queue_add": {
        const t = qTask(`q${Date.now()}`, args.task.prompt, { ...args.task, state: args.task.mode === "ask" ? "waiting" : "queued", added_at: Math.floor(Date.now() / 1000) });
        queueTasks = [...queueTasks, t]; queueChanged(); return t;
      }
      case "queue_remove": queueTasks = queueTasks.filter((t) => t.id !== args.id); queueChanged(); return null;
      case "queue_move": {
        const t = queueTasks.find((x) => x.id === args.id);
        const rest = queueTasks.filter((x) => x.id !== args.id);
        const at = args.before ? rest.findIndex((x) => x.id === args.before) : rest.length;
        queueTasks = [...rest.slice(0, at), t, ...rest.slice(at)]; queueChanged(); return null;
      }
      case "queue_retry": queueTasks = queueTasks.map((t) => (t.id === args.id ? { ...t, state: "queued", note: "", ended_at: null, attempts: t.attempts + 1 } : t)); queueChanged(); return null;
      case "queue_start_now": queueTasks = queueTasks.map((t) => (t.id === args.id ? { ...t, state: "starting", ran_with: t.agent, started_at: Math.floor(Date.now() / 1000) } : t)); queueChanged(); return null;
      case "queue_set_paused": queuePaused = args.paused; queueChanged(); return null;
      case "queue_set_limits": queueMax = [args.maxTotal, args.maxPerRepo]; queueChanged(); return null;
      case "queue_clear_finished": { const n = queueTasks.length; queueTasks = queueTasks.filter((t) => !["done", "needs_you", "failed"].includes(t.state)); queueChanged(); return n - queueTasks.length; }
      case "queue_summary": return location.search.includes("summary") ? { done: 8, needs_you: 2, failed: 1, running: 1, queued: 3, text: "8 done, 2 need you, 1 failed" } : { done: 0, needs_you: 0, failed: 0, running: 0, queued: 0, text: "" };
      case "queue_summary_seen": return null;
      case "checks_list": return checksFor(args.id);
      case "checks_statuses": return { w1: "failed", w2: "running", w3: "passed" };
      case "checks_output": return args.name === "test" ? checkOutput : "";
      case "checks_run": case "checks_stop": case "checks_trust": return null;
      case "snapshot": if (fresh) return { repos: [...freshRepos], workspaces: [...freshWorkspaces], running: [], running_workspaces: [], lit: [], shining: [], costs: {} };
        return { repos, workspaces: args.includeArchived ? [...workspaces, archivedWs] : workspaces, running: opencode ? ["s9"] : [], running_workspaces: opencode ? ["w3", "w2"] : ["w3"], lit: ["mochi", "biscuit", "tofu", "dumpling", "pudding", "muffin", "ziggy", "comet", "dash", "pip"], shining: ["mochi", "biscuit", "tofu", "dumpling", "pudding", "muffin", "comet", "dash"], costs: { w1: 4.82, w2: 1.1 } };
      case "github_profile": return { login: "Harshil-Jani", name: "Harshil Jani", avatar_url: "https://github.com/Harshil-Jani.png?size=96", html_url: "", public_repos: 80, followers: 120 };
      case "repo_github": return { nameWithOwner: "pawprint/" + args.id, description: "", stargazerCount: args.id === "r1" ? 1240 : 86, url: "", owner_avatar: "", isPrivate: false };
      case "sky": return SKY;
      case "conductor_available": return !fresh || freshFound === "conductor";
      case "repo_brief": return {
        readme: "# Pawprint\n\nA help desk for vet clinics: tickets, SLAs and on-call.\n",
        package_json: JSON.stringify({ name: "pawprint-web", scripts: { dev: "vite", test: "vitest" } }),
        files: ["bun.lock", "docs/", "package.json", "public/", "README.md", "src/", "tests/"],
      };
      case "clone_repo": case "add_repo": {
        const r = { ...repos[1], id: "r9", name: String(args.spec ?? args.path ?? "pawprint-web").split("/").pop() };
        if (fresh) { freshRepos.push(r); setTimeout(() => emit("workspaces-changed", {}), 50); }
        return r;
      }
      case "search_chats": return previewSearch(args.query, args.filter);
      case "search_open": { const c = corpus.find((x) => x.id === args.messageId)!; return { session: { id: c.session, workspace_id: c.ws, agent: c.agent, agent_session_id: null, title: c.chat, model: null, permission_mode: "acceptEdits", effort: null }, workspace_id: c.ws, index: c.index, total: c.total }; }
      case "sessions": if (args.workspaceId === "w7") return [{ id: "s7", workspace_id: "w7", agent: "claude", agent_session_id: "4b1f0c2e-9d7a-4e55-a1c3-2f6d8e9b7a10", title: "Fix the flaky checkout test", model: null, permission_mode: "acceptEdits" }];
        if (args.workspaceId === "w9") return [{ id: "s7", workspace_id: "w9", agent: "codex", agent_session_id: null, title: "Webhook retries", model: null, permission_mode: "acceptEdits" }];
        return args.workspaceId === "w1"
        ? [{ id: "s2", workspace_id: "w1", agent: "codex", agent_session_id: null, title: "Second opinion", model: null, permission_mode: "plan" }, { id: "s1", workspace_id: "w1", agent: "claude", agent_session_id: "x", title: "Fix 502 after deploy", model: "opus", permission_mode: "acceptEdits", effort: "high" }]
        : [{ id: "s9", workspace_id: args.workspaceId, agent: opencode && args.workspaceId === "w2" ? "opencode" : "claude", agent_session_id: null, title: opencode && args.workspaceId === "w2" ? "Business-hours SLA" : "New chat", model: null, permission_mode: opencode && args.workspaceId === "w2" ? "ask" : "acceptEdits" }];
      case "session_events_tail": { const all = args.id === "s7" ? archivedEvents : args.id === "s1" ? events : args.id === "s9" && new URLSearchParams(location.search).get("ws") === "w2" ? planEvents : []; return { events: all, total: all.length }; }
      case "session_events": return args.id === "s7" ? archivedEvents : args.id === "s1" ? events : args.id === "s2" ? codexEvents : args.id === "s9" && new URLSearchParams(location.search).get("ws") === "w2" ? planEvents : [];
      case "queued": return args.id === "s9" && new URLSearchParams(location.search).get("ws") === "w2" ? ["Also add a metric for SLA breaches", "Then open the PR"] : [];
      case "pr_cache": {
        // What the PR poller would have pushed by now: one view per workspace.
        const prFor = (id: string) => {
          if (id === "w1") return pr;
          // Comet's PR is the draft the sidebar shows, with the same +120 -31 as its files.
          if (id === "w2") return { ...pr, number: 479, isDraft: true, additions: 120, deletions: 31, statusCheckRollup: [{ name: "go test", conclusion: "FAILURE" }, { name: "lint", conclusion: "SUCCESS" }], latestReviews: [], reviewRequests: [] };
          if (id === "w4") return { ...pr, number: 471, state: "MERGED", additions: 6, deletions: 2, latestReviews: [{ author: { login: "octocat" }, state: "APPROVED" }], reviewRequests: [] };
          if (id.startsWith("g") && multi !== "nopr") {
            const n = { g1: 512, g2: 96, g3: 14 }[id]!;
            const repo = { g1: "pawprint-api", g2: "pawprint-web", g3: "pawprint-types" }[id]!;
            const running = multi !== "green" && id === "g2";
            return { ...pr, number: n, url: `https://github.com/pawprint/${repo}/pull/${n}`, state: multiMerged.has(id) ? "MERGED" : "OPEN", title: "Export orders as CSV",
              statusCheckRollup: running ? [{ name: "build", conclusion: "SUCCESS" }, { name: "e2e", status: "IN_PROGRESS", conclusion: null }] : [{ name: "build", conclusion: "SUCCESS" }] };
          }
          return null;
        };
        return Object.fromEntries(["w1", "w2", "w3", "w4", "w5", ...(multi !== null ? ["g1", "g2", "g3"] : [])].map((id) => [id, { workspace_id: id, pr: prFor(id) }]));
      }
      // Files per workspace add up to line_counts below. Dash (w3) and Pip (w5) have no changes yet.
      case "changes": return args.id === "w1" || args.id === "w6" ? [
        { path: "internal/server/http.go", status: "M", additions: 3, deletions: 1 },
        { path: "internal/server/http_test.go", status: "A", additions: 35, deletions: 0 },
      ] : args.id === "w2" ? [
        // ?manyfiles adds enough files for the filter box to appear (it shows from 8 files).
        ...(location.search.includes("manyfiles") ? ["calendar.go", "calendar_test.go", "holidays.go", "metrics.go", "notify.go"].map((f) => ({ path: `internal/tickets/${f}`, status: "M", additions: 4, deletions: 1 })) : []),
        { path: "internal/tickets/sla.go", status: "M", additions: 61, deletions: 26 },
        { path: "internal/tickets/business_hours.go", status: "A", additions: 41, deletions: 0 },
        { path: "internal/tickets/sla_test.go", status: "M", additions: 15, deletions: 4 },
        { path: "internal/server/http.go", status: "M", additions: 3, deletions: 1 },
      ] : args.id === "w4" ? [
        { path: "src/pages/pricing.tsx", status: "M", additions: 6, deletions: 2 },
      ] : args.id === "g1" ? [
        { path: "internal/orders/export.go", status: "A", additions: 64, deletions: 0 },
        { path: "internal/server/routes.go", status: "M", additions: 2, deletions: 0 },
      ] : args.id === "g2" ? [
        { path: "src/pages/orders.tsx", status: "M", additions: 18, deletions: 3 },
      ] : args.id === "g3" ? [
        { path: "src/orders.ts", status: "M", additions: 9, deletions: 0 },
      ] : [];
      case "file_diff": return diff;
      // Changes, Commit. ?pushed makes the last commit pushed (Amend and Undo off); ?nostaged empties
      // the staged list; ?busyagent makes Draft message fall back to the file list.
      case "stage_status": return {
        staged: location.search.includes("nostaged") ? [] : [{ path: "internal/server/http.go", old_path: null, status: "M", additions: 3, deletions: 1, binary: false, protected: false }],
        unstaged: [
          { path: "internal/server/http_test.go", old_path: null, status: "?", additions: 35, deletions: 0, binary: false, protected: false },
          { path: "deploy/.env.production", old_path: null, status: "M", additions: 1, deletions: 1, binary: false, protected: true },
          { path: "docs/alb-timeouts.png", old_path: null, status: "M", additions: 0, deletions: 0, binary: true, protected: false },
        ],
      };
      case "stage_diff": return { text: args.path.endsWith(".png") ? "Binary files a/docs/alb-timeouts.png and b/docs/alb-timeouts.png differ" : diff, sig: "preview", binary: args.path.endsWith(".png") };
      case "head_commit": return { sha: "a1b2c3d4", short: "a1b2c3d", subject: "test(server): cover the idle timeout", message: "test(server): cover the idle timeout", pushed: location.search.includes("pushed"), block: location.search.includes("pushed") ? "The last commit is already pushed. Changing it would rewrite what's on the remote." : null };
      case "draft_commit_message": return location.search.includes("busyagent")
        ? { message: "chore(internal): update http.go", source: "local", note: "The agent is busy, so this draft comes from the file list." }
        : { message: "fix(server): keep idle connections open past the ALB timeout\n\nThe ALB keeps connections for 60s, so a 30s IdleTimeout made it reuse\nconnections we had closed, and clients saw 502s. 75s outlives it.", source: "agent", note: null };
      case "split_plan": return {
        hunks: [
          { path: "internal/server/http.go", key: "k1", header: "@@ -18,7 +18,9 @@ func New(cfg Config) *http.Server {", preview: diff.split("\n").slice(2).join("\n"), additions: 3, deletions: 1, whole: false, protected: false },
          { path: "internal/server/http_test.go", key: "k2", header: "@@ -0,0 +1,35 @@", preview: "+package server\n+\n+func TestIdleTimeout(t *testing.T) {", additions: 35, deletions: 0, whole: false, protected: false },
          { path: "deploy/.env.production", key: "k3", header: "@@ -3,1 +3,1 @@", preview: "-IDLE_TIMEOUT=30s\n+IDLE_TIMEOUT=75s", additions: 1, deletions: 1, whole: false, protected: true },
          { path: "docs/alb-timeouts.png", key: "file:k4", header: "Binary file", preview: "", additions: 0, deletions: 0, whole: true, protected: false },
        ],
        groups: [
          { message: "fix(server): keep idle connections open past the ALB timeout", hunks: [0, 2] },
          { message: "test(server): cover the idle timeout", hunks: [1] },
          { message: "", hunks: [3] },
        ],
        source: "agent", note: null,
      };
      case "stage_apply": case "stage_everything": return null;
      case "commit_staged": case "split_commit": return "e4f5a6b";
      case "undo_commit": return "test(server): cover the idle timeout";
      case "repo_prs": return [
        { number: 512, title: "Retry webhooks with backoff", headRefName: "ana/webhook-retry", author: { login: "ana" }, isDraft: false, updatedAt: "" },
        { number: 509, title: "Move ticket SLA timer to the worker", headRefName: "raj/sla-worker", author: { login: "raj" }, isDraft: true, updatedAt: "" },
      ];
      case "app_settings": return { caffeinate: "on", auto_branch: "on", home: "~/nekotrail" };
      case "pr_review_comments": return args.id === "w1" ? [
        { id: 1, path: "internal/server/http.go", line: 22, side: "RIGHT", body: "75s works for the ALB. What about the NLB path, which keeps connections for 350s?", user: "octocat", avatar: "", url: "https://github.com", reply_to: null, outdated: false },
        { id: 2, path: "internal/server/http.go", line: 22, side: "RIGHT", body: "Good catch, NLB is only used by the gRPC port.", user: "Harshil-Jani", avatar: "", url: "https://github.com", reply_to: 1, outdated: false },
      ] : [];
      case "read_file": return "package server\n\nfunc New(cfg Config) *http.Server {\n  return nil\n}\n";
      case "pty_open": return "\x1b[32m~/code/ws/mochi\x1b[0m on \x1b[35mharshil/fix-502-gateway\x1b[0m\r\n$ ";
      case "pty_attach": return String(args.id).endsWith(":run") ? "$ make dev\r\ngo run ./cmd/api\r\n2026/11/06 01:40:12 listening on http://localhost:41230\r\n" : null;
      case "line_counts": return { w1: [38, 1], w2: [120, 31], w4: [6, 2], g1: [66, 0], g2: [18, 3], g3: [9, 0] };
      case "create_multi_task": return (args.repoIds as string[]).map((r, i) => ({ ...workspaces[0], id: `new${i}`, repo_id: r, name: "vega", branch: "harshil/vega", group_id: "new0", pinned: false, title: "" }));
      case "group_prs": return [{ workspace_id: "g1", repo: "pawprint/pawprint-api", number: 512, url: "" }, { workspace_id: "g2", repo: "pawprint/pawprint-web", number: 96, url: "" }, { workspace_id: "g3", repo: "pawprint/pawprint-types", number: 14, url: "" }];
      case "merge_all": {
        // ?multi=green&mergefail: the second repo in the order fails, so the third waits.
        const order = args.order as string[];
        const fail = location.search.includes("mergefail") ? order[1] : null;
        const steps = order.map((id, i) => ({ workspace_id: id, name: { g1: "pawprint-api", g2: "pawprint-web", g3: "pawprint-types" }[id as "g1"], detail: "",
          outcome: fail && i > order.indexOf(fail) ? "skipped" : id === fail ? "failed" : "merged" }));
        for (const s of steps) if (s.outcome === "merged") multiMerged.add(s.workspace_id);
        if (fail) steps[order.indexOf(fail)].detail = "Pull request is not mergeable: the base branch policy prohibits the merge.";
        return steps;
      }
      case "open_in_targets": return [{ id: "cursor", name: "Cursor" }, { id: "vscode", name: "Visual Studio Code" }, { id: "zed", name: "Zed" }, { id: "ghostty", name: "Ghostty" }, { id: "finder", name: "Finder" }];
      case "doctor": {
        const noAgents = location.search.includes("noagents");
        const gh = new URLSearchParams(location.search).get("gh") !== "0";
        return { git: { ok: true, version: "git version 2.50.1" }, gh: { installed: true, ok: gh, login: gh ? "Harshil-Jani" : null }, claude: noAgents ? { ok: false, version: "" } : { ok: true, version: "2.4.1 (Claude Code)" },
          ...Object.fromEntries(AGENTS.filter((a) => a !== "claude").map((a) => [a, !noAgents && installed.includes(a) ? { ok: true, version: `${a} 1.0.0` } : { ok: false, version: "" }])) };
      }
      // ?agents=copilot,grok: the CLIs on PATH besides Claude (default: Copilot). ?agents=all: every one.
      case "installed_agents": return installed;
      case "github_repos": return [
        { nameWithOwner: "pawprint/pawprint-web", description: "The help desk's web app", pushedAt: "2026-11-06T10:00:00Z", isPrivate: false },
        { nameWithOwner: "pawprint/pawprint-api", description: "Tickets, SLAs and on-call", pushedAt: "2026-11-05T10:00:00Z", isPrivate: true },
      ];
      case "import_preview": return { repos: 3, workspaces: 12, sessions: 31, skipped: 2, chats_with_history: 0 };
      case "terminal_scan": {
        if (fresh && freshFound !== "terminal") return { repos: [], skipped: [], found: {}, sources: [], took_ms: 120 };
        const s = (agent: string, id: string, title: string, messages: number, last: string, extra = {}) => ({ agent, id, title, messages, first: last, last, bytes: messages * 9000, imported: false, resumable: true, ...extra });
        return {
          repos: [
            { root: "/code/storefront", name: "storefront", repo_id: "r1", folders: [
              { path: "/code/storefront", kind: "history", branch: "main", workspace_id: null, sessions: [s("claude", "c1", "Fix the flaky checkout test", 42, "2026-11-04T10:00:00Z"), s("codex", "x1", "Add retries to the webhook sender", 18, "2026-10-28T09:00:00Z"), s("claude", "c2", "Explain the cart reducer", 6, "2026-10-12T09:00:00Z", { imported: true })] },
              { path: "/code/storefront-search", kind: "worktree", branch: "me/search", workspace_id: null, sessions: [s("claude", "c3", "Search by SKU", 77, "2026-11-05T15:00:00Z"), s("gemini", "g1", "Rename the config loader", 9, "2026-11-01T11:00:00Z")] },
            ] },
            { root: "/code/infra", name: "infra", repo_id: null, folders: [
              { path: "/code/infra", kind: "history", branch: "main", workspace_id: null, sessions: [s("claude", "c4", "Bump the Postgres version", 23, "2026-10-30T08:00:00Z")] },
            ] },
          ],
          skipped: [{ folder: "/Users/me/conductor/workspaces/storefront/tunis", reason: "The folder is gone.", sessions: 4, messages: 120 }, { folder: "/Users/me", reason: "Not in a git repo.", sessions: 2, messages: 9 }],
          found: { claude: 9, codex: 1, gemini: 1 }, sources: ["/Users/me/.claude/projects", "/Users/me/.codex/sessions", "/Users/me/.gemini/tmp"], took_ms: 861,
        };
      }
      case "terminal_import": return { repos: 1, workspaces: 1, chats: (args as { picks: unknown[] }).picks.length, messages: 400, skipped: 0, problems: [] };
      case "usage_summary": if (fresh) return { today: 0, week: 0, by_agent: {} }; return { today: 3.4, week: 41.27, by_agent: { claude: 33.1, codex: 8.17 } };
      case "workspace_stats": return args.id === "w1" ? { agent: "claude", turns: 4, errors: 0, duration_ms: 182000, cost_usd: 1.92 } : { agent: "codex", turns: 3, errors: 1, duration_ms: 141000, cost_usd: 0.88 };
      case "model_catalog": return catalog;
      case "mcp_list": return "Checking MCP server health…\n\ngithub: https://api.githubcopilot.com/mcp - ✔ Connected\nlinear: https://mcp.linear.app/sse - ✔ Connected\npostgres: npx -y @modelcontextprotocol/server-postgres - ✗ Failed to connect";
      // ?mcp opens the sheet (made-up servers and skills). ?mcp=empty, ?mcp=error and ?mcp=loading show
      // those states; ?mcp=stale makes "Add" fail as if the file changed after the preview.
      case "mcp_share_overview": {
        const st = new URLSearchParams(location.search).get("mcp");
        if (st === "loading") return new Promise(() => {});
        if (st === "error") throw "Couldn't read ~/.codex/config.toml: expected `=` at line 14, column 9.";
        const files = { claude: "~/.claude.json", codex: "~/.codex/config.toml", gemini: "~/.gemini/settings.json", opencode: "~/.config/opencode/opencode.json" };
        const dirs = ["~/.config/opencode/skills", "~/.claude/skills", "~/.agents/skills"];
        if (st === "empty") return { servers: [], skills: [], files, opencode_skill_dirs: dirs, problems: [] };
        const u = (agent: string, file: string) => ({ agent, scope: "user", file });
        const ok = (agent: string) => ({ agent, ok: true, why: null });
        const sse = "Codex only connects to command servers and streamable HTTP servers. This one uses SSE, so Codex can't use it.";
        return {
          servers: [
            { name: "github", transport: "http", summary: "https://api.githubcopilot.com/mcp", env: [], has: [u("claude", files.claude), u("codex", files.codex)], from: u("claude", files.claude), secrets: [], targets: [ok("gemini"), ok("opencode")], problem: null },
            { name: "linear", transport: "sse", summary: "https://mcp.linear.app/sse", env: [], has: [u("claude", files.claude)], from: u("claude", files.claude), secrets: [], targets: [{ agent: "codex", ok: false, why: sse }, ok("gemini"), ok("opencode")], problem: null },
            { name: "postgres", transport: "stdio", summary: "npx -y @modelcontextprotocol/server-postgres", env: ["PGHOST", "PGPASSWORD"], has: [{ agent: "claude", scope: "project", file: ".mcp.json" }], from: { agent: "claude", scope: "project", file: ".mcp.json" }, secrets: [{ id: "env:PGPASSWORD", kind: "env", key: "PGPASSWORD", can_ref: true }], targets: [ok("codex"), ok("gemini"), ok("opencode")], problem: null },
            { name: "playwright", transport: "stdio", summary: "npx @playwright/mcp@latest --headless --browser chromium --viewport-size 1440,900", env: [], has: [u("claude", files.claude), u("codex", files.codex), u("gemini", files.gemini), u("opencode", files.opencode)], from: u("claude", files.claude), secrets: [], targets: [], problem: null },
            { name: "sentry", transport: "http", summary: "https://mcp.sentry.dev/mcp", env: [], has: [u("opencode", files.opencode)], from: u("opencode", files.opencode), secrets: [{ id: "header:Authorization", kind: "header", key: "Authorization", can_ref: false }], targets: [ok("claude"), ok("codex"), ok("gemini")], problem: null },
          ],
          skills: [
            { name: "release-notes", description: "Write release notes from merged PRs", has: ["claude", "codex", "gemini", "opencode"], share: [] },
            { name: "triage", description: "Label and prioritise new issues, then post a summary of what changed in the last day to the team channel", has: ["claude", "opencode"], share: [{ target: "agents", label: "Add to Codex and Gemini", link: "~/.agents/skills/triage", to: "~/.claude/skills/triage", ok: true, why: null }] },
            { name: "db-migrate", description: "Plan and check a database migration", has: ["codex", "gemini", "opencode"], share: [{ target: "claude", label: "Add to Claude", link: "~/.claude/skills/db-migrate", to: "~/.agents/skills/db-migrate", ok: false, why: "~/.claude/skills/db-migrate already exists and isn't a link. nekotrail won't replace it." }] },
          ],
          files, opencode_skill_dirs: dirs, problems: [],
        };
      }
      case "mcp_share_preview": {
        if (args.name === "linear" && args.target === "codex") return {
          file: "~/.codex/config.toml", creates: false, backup: null, snippet: `[mcp_servers.linear]\nurl = "https://mcp.linear.app/sse"`, notes: [], asks: [], secrets: [], token: "t",
          blockers: ["Codex only connects to command servers and streamable HTTP servers. This one uses SSE, so Codex can't use it."],
        };
        if (args.target === "opencode") return {
          // No OpenCode config yet: the preview is the whole new file.
          file: "~/.config/opencode/opencode.json", creates: true, backup: null, notes: [], blockers: [], asks: [], secrets: [], token: "t",
          snippet: `{\n  "$schema": "https://opencode.ai/config.json",\n  "mcp": {\n    "${args.name}": {\n      "type": "remote",\n      "url": "https://api.githubcopilot.com/mcp",\n      "enabled": true\n    }\n  }\n}`,
        };
        const ref = args.choices["env:PGPASSWORD"] === "ref";
        const answered = !!args.choices["env:PGPASSWORD"];
        return {
          file: "~/.codex/config.toml", creates: false, backup: "~/.codex/config.toml.bak-nekotrail-20261106-142233",
          snippet: `[mcp_servers.postgres]\ncommand = "npx"\nargs = ["-y", "@modelcontextprotocol/server-postgres"]${ref ? "\nenv_vars = [\"PGPASSWORD\"]" : ""}\n\n[mcp_servers.postgres.env]\nPGHOST = "localhost"${!ref && answered ? "\nPGPASSWORD = \"<hidden: your value, 18 characters>\"" : ""}`,
          notes: ref ? ["Codex reads PGPASSWORD from the environment it starts in, so set it in your shell profile."] : [],
          blockers: [], asks: answered ? [] : ["PGPASSWORD looks like a secret. Choose whether to bring it across as a variable or copy the value."],
          secrets: [{ id: "env:PGPASSWORD", kind: "env", key: "PGPASSWORD", can_ref: true }], token: "t",
        };
      }
      case "mcp_share_apply":
        if (new URLSearchParams(location.search).get("mcp") === "stale") throw "The file changed since the preview. Look at it again before adding.";
        return { file: args.target === "opencode" ? "~/.config/opencode/opencode.json" : "~/.codex/config.toml", backup: args.target === "opencode" ? null : "~/.codex/config.toml.bak-nekotrail-20261106-142233" };
      // ?dirty: archiving asks first, as if the workspace had uncommitted changes.
      case "archive_workspace": if (location.search.includes("dirty") && !args.force) throw "DIRTY"; return null;
      case "rules_overview": return rulesOverview();
      case "rules_plan": return rulesPlan(args.req);
      case "rules_apply": return { written: ["CLAUDE.md", ".github/copilot-instructions.md"], backup: "~/Library/Application Support/dev.nekotrail.app/rules-backups/mochi-20261106-142200" };
      case "rules_open": { const t = rulesText[args.path] ?? ""; return { path: args.path, text: t, exists: args.path in rulesText, hash: "h", link: null, generated_from: null }; }
      case "rules_save": rulesText[args.path] = args.text; return { written: [args.path], backup: null };
      case "task_rules": return taskRulesText;
      case "set_task_rules": taskRulesText = args.text; return null;
      case "skill_share": return "~/.agents/skills/triage";
      case "prompts_list": return { entries: promptEntries(), saved: personalPrompts !== null };
      case "prompts_save": personalPrompts = args.list; return null;
      case "prompts_duplicate": { const name = `${args.name}-copy`; const e = promptEntries().find((x) => x.name === args.name && x.scope === args.scope); personalPrompts = [...(personalPrompts ?? []), { name, description: e.description, text: e.text.replace("$ARGUMENTS", "{{arguments}}") }]; return name; }
      case "prompts_export": return (personalPrompts ?? []).length;
      case "prompts_import": return { added: [], renamed: [], skipped: [] };
      case "prompt_context": return { branch: "harshil/fix-502-gateway", base: "main", files_changed: "internal/server/http.go\ninternal/server/http_test.go" };
      case "sections": if (fresh) return []; return [{ id: "sec1", name: "This week", sort: 1 }, { id: "sec2", name: "Waiting on review", sort: 2 }];
      case "resource_usage": return { total_mb: 1597, top: [{ name: "claude", mb: 812 }, { name: "node", mb: 431 }, { name: "nekotrail", mb: 208 }, { name: "zsh", mb: 12 }] };
      case "imported_outside": return location.search.includes("moveimport") ? ["w1", "w2", "w3"] : [];
      case "autopilot_status": return { w2: 1 };
      // Mochi (w1, PR open, 39 lines) and Comet (w2, draft, 151 lines) both change http.go.
      case "collisions": return [{ repo_id: "r1", order: ["w1", "w2"], pairs: [{ a: "w1", b: "w2", files: ["internal/server/http.go"] }] }];
      // Dash sits on Mochi's PR #482; add ?stackmerged to see it after #482 merged.
      case "stack_parent": return args.id === "w3" ? { id: "w1", branch: "harshil/fix-502-gateway", target: "main", state: "active",
        pr: { number: 482, url: "https://github.com/pawprint/pawprint-api/pull/482", state: location.search.includes("stackmerged") ? "MERGED" : "OPEN", title: "Fix 502s after deploy", isDraft: false, baseRefName: "main" } } : null;
      case "agent_scorecard": return [
        { agent: "claude", turns: 212, errors: 9, duration_ms: 212 * 96000, merged: 31, cost_usd: 148.2 },
        { agent: "codex", turns: 87, errors: 6, duration_ms: 87 * 71000, merged: 11, cost_usd: 39.6 },
      ];
      case "free_disk_mb": return 1430;
      case "workspace_storage": if (fresh) return []; return [
        { workspace_id: "w1", total_kb: 2_400_000, cleanable_kb: 2_150_000, dirs: [{ path: ".venv", kb: 1_900_000 }, { path: "node_modules", kb: 250_000 }] },
        { workspace_id: "w2", total_kb: 940_000, cleanable_kb: 431_000, dirs: [{ path: "web/.next", kb: 431_000 }] },
        { workspace_id: "w3", total_kb: 120_000, cleanable_kb: 0, dirs: [] },
      ];
      case "workspace_files": return ["internal/server/http.go", "internal/server/http_test.go", "cmd/api/main.go", "go.mod", "README.md", "internal/tickets/handler.go"];
      // pawprint-web (Ziggy, Pip) has no Run script, for the Preview tab's "set one up" state.
      case "workspace_scripts": return { setup: null, run: args.id === "w4" || args.id === "w5" ? null : "make dev", archive: null, trust: location.search.includes("trust") ? { fingerprint: "f", setup: "go mod download && ./scripts/bootstrap.sh", run: "make dev", archive: null } : null };
      case "agent_config_trust": return location.search.includes("agenttrust") ? { repo: "acme-api", fingerprint: "a", items: [
        { agent: "claude", file: ".claude/settings.json", what: "hook SessionStart", command: "./scripts/warm-cache.sh" },
        { agent: "claude", file: ".mcp.json", what: "MCP server db", command: "npx -y @acme/db-mcp --readonly" },
        { agent: "codex", file: ".codex/config.toml", what: "shell env", command: "ZDOTDIR=./tools/shell" },
      ] } : null;
      // Preview tab. ?previewUrl=http://localhost:5173 pretends the Run script printed that address;
      // ?previewDown and ?previewBlocked show those states. The snapshot is a drawn stand-in,
      // since a browser page can't photograph a cross-origin frame (the app does it natively).
      case "preview_probe": return location.search.includes("previewDown") ? { up: false, status: null, blocked: null }
        : location.search.includes("previewBlocked") ? { up: true, status: 200, blocked: "x-frame-options" } : null;
      case "preview_snapshot": return fakeSnapshot();
      case "repo_guard": return { app: guard ? [".env*", "migrations/**", "infra/**"] : [], file: guard ? ["*.tfstate", "deploy/prod/**"] : [] };
      case "put_back_protected": setTimeout(() => emit("agent", { session_id: "s1", event: { kind: "guard_restored", files: args.paths, checkpoint: args.checkpoint } }), 300); return args.paths;
      case "live_checkpoints": return args.checkpoints;
      case "checkpoint_preview": return { files: 2, commits: 1 };
      case "review_comments": return reviews[args.id] ?? [];
      case "review_requests": return reviewRequests;
      case "review_plan": return { number: 731, url: "https://github.com", title: "Retry webhook deliveries with backoff", author: "octocat", head_sha: "b7c1d2e", inline: (reviews[args.id] ?? []).length, folded: location.search.includes("folded") ? ["internal/server/retry.go:88"] : [], suggestions: 1 };
      case "update_review_comment": reviews[args.id] = (reviews[args.id] ?? []).map((c) => (c.id === args.comment ? { ...c, text: args.text } : c)); return null;
      // The preview never posts anything: it only says what would happen.
      case "submit_review": { const n = (reviews[args.id] ?? []).length; reviews[args.id] = []; return { url: "https://github.com", inline: n, folded: 0, archived: true }; }
      case "add_review_comments": { const cur = reviews[args.id] ?? []; reviews[args.id] = [...cur, ...args.comments.filter((c: any) => !cur.some((x) => x.id === c.id))]; return null; }
      case "remove_review_comments": reviews[args.id] = (reviews[args.id] ?? []).filter((c) => !args.ids.includes(c.id)); return null;
      case "send":
        (window as any).__sent = [...((window as any).__sent ?? []), args.text];
        // A fresh install's first task: the agent works for a few seconds, then answers.
        if (fresh) {
          const w = freshWorkspaces[0]?.id;
          emit("running", { running: true, workspace_id: w });
          setTimeout(() => emit("running", { running: false, workspace_id: w, ok: true, title: "Explain the layout" }), 3000);
        }
        return null;
      case "create_workspace": if (!fresh) return null; {
        const w = { ...workspaces[4], id: "f1", repo_id: args.repoId, name: "mochi", branch: "harshil/mochi", title: "", created_at: new Date().toISOString() };
        freshWorkspaces.push(w);
        return w;
      }
      case "new_session": return fresh ? { id: "s9", workspace_id: args.workspaceId, agent: args.agent, agent_session_id: null, title: "New chat", model: null, permission_mode: "plan" } : null;
      case "save_attachment": (window as any).__lastAttachment = args.bytes; return `.nekotrail/attachments/140212-${args.name}`;
      case "remote_take": if (linkMode === null || linkTaken) return [];
        linkTaken = true;
        return [{ kind: "confirm", token: "t1", repo: linkMode === "long" ? "pawprint-mobile" : "pawprint-api", repo_path: linkMode === "long" ? "~/code/pawprint-mobile" : "/code/pawprint-api", adds_repo: linkMode === "long", agent: linkMode === "long" ? "codex" : "claude", base: linkMode === "long" ? "release/2.4" : null, plan: linkMode === "long",
          prompt: linkMode === "long" ? "The iOS app crashes on launch for users upgrading from 2.3 when the keychain still holds the old token format.\n\n1. Reproduce it with the 2.3 fixture in test/fixtures/keychain-2.3.json.\n2. Migrate the token on first launch instead of reading it as the new format.\n3. Add a test that upgrades from 2.3 and launches twice.\n4. Don't touch the Android code.\n\nOpen a draft PR when the tests pass, and list anything you weren't sure about in its description so a reviewer can check it." : "Fix the flaky login test on CI" }];
      case "link_confirm": return null;
      case "cli_status": return { path: new URLSearchParams(location.search).get("cli") === "installed" ? "/usr/local/bin/nekotrail" : null };
      case "cli_install": if (new URLSearchParams(location.search).get("cli") === "fail") throw "Couldn't write /usr/local/bin/nekotrail: permission denied. Run it again and allow the admin prompt."; return { path: "/usr/local/bin/nekotrail", hint: null };
      case "workspace_ports": return { base: blockOf(args.id), main: blockOf(args.id) };
      case "ports_status": {
        if (portsMode === "loading") return new Promise(() => {});
        if (portsMode === "error") throw "Couldn't list ports: lsof exited with status 1.";
        if (portsMode === "empty") return [];
        const all = portsList();
        if (args.id) { const one = all.find((w) => w.id === args.id) ?? { id: args.id, base: blockOf(args.id), main: blockOf(args.id), rows: [], problems: [] };
          return new URLSearchParams(location.search).has("portProblem") ? [{ ...one, problems: all[1].problems }] : [one]; }
        return all;
      }
      case "free_port": return args.port;
      default: return null;
    }
  },
  { shouldMockEvents: true },
);

/** A window-sized PNG with a plain made-up page where the preview frame is. */
async function fakeSnapshot(): Promise<ArrayBuffer> {
  const s = 2;
  const c = document.createElement("canvas");
  c.width = window.innerWidth * s;
  c.height = window.innerHeight * s;
  const g = c.getContext("2d")!;
  g.scale(s, s);
  const r = document.querySelector(".preview-frame")!.getBoundingClientRect();
  g.fillStyle = "#ffffff";
  g.fillRect(r.left, r.top, r.width, r.height);
  g.fillStyle = "#1d1d1f";
  g.fillRect(r.left, r.top, r.width, 56);
  g.fillStyle = "#ffffff";
  g.font = "600 16px -apple-system, sans-serif";
  g.fillText("Pawprint", r.left + 24, r.top + 34);
  g.fillStyle = "#1d1d1f";
  g.font = "700 32px -apple-system, sans-serif";
  g.fillText("Tickets", r.left + 24, r.top + 120);
  for (let i = 0; i < 5; i++) {
    g.fillStyle = i % 2 ? "#f5f5f7" : "#ffffff";
    g.fillRect(r.left + 24, r.top + 150 + i * 44, r.width - 48, 44);
    g.fillStyle = "#3a3a3c";
    g.font = "14px -apple-system, sans-serif";
    g.fillText(`#${1040 + i}  Gateway returns 502 after deploy`, r.left + 36, r.top + 177 + i * 44);
  }
  const blob: Blob = await new Promise((res) => c.toBlob((b) => res(b!), "image/png"));
  return blob.arrayBuffer();
}

const previewUrl = new URLSearchParams(location.search).get("previewUrl");
if (previewUrl) setTimeout(() => emit("pty", { id: "w1:run", data: `listening on ${previewUrl}\r\n` }), 400);

// A small slice of the sky from src-tauri/src/names.rs, enough for screenshots.
const SKY = {
  stars: [
    ["mochi", "Mochi", "loaf", 90, 120], ["biscuit", "Biscuit", "loaf", 130, 70], ["tofu", "Tofu", "loaf", 175, 115],
    ["dumpling", "Dumpling", "loaf", 270, 120], ["pudding", "Pudding", "loaf", 270, 210], ["muffin", "Muffin", "loaf", 90, 210],
    ["ziggy", "Ziggy", "pounce", 380, 200], ["comet", "Comet", "pounce", 430, 140], ["dash", "Dash", "pounce", 500, 110],
    ["pepper", "Pepper", "pounce", 575, 80], ["rocket", "Rocket", "pounce", 540, 170], ["flick", "Flick", "pounce", 340, 120],
    ["pip", "Pip", "kitten", 430, 350], ["button", "Button", "kitten", 470, 320], ["bean", "Bean", "kitten", 510, 350],
    ["peanut", "Peanut", "kitten", 510, 410], ["pebble", "Pebble", "kitten", 430, 410], ["tiny", "Tiny", "kitten", 470, 450],
  ].map(([slug, name, constellation, x, y]) => ({ slug, name, constellation, x, y })),
  constellations: [
    { slug: "loaf", name: "The Loaf", story: "A cat tucked into a perfect rectangle, paws hidden.", lines: [["biscuit", "mochi"], ["biscuit", "tofu"], ["tofu", "dumpling"], ["dumpling", "pudding"], ["pudding", "muffin"], ["muffin", "mochi"]] },
    { slug: "pounce", name: "The Pounce", story: "Mid-leap, front paws out, eyes on a bug in the code.", lines: [["flick", "ziggy"], ["ziggy", "comet"], ["comet", "dash"], ["dash", "pepper"], ["dash", "rocket"]] },
    { slug: "kitten", name: "The Kitten", story: "Small, round and mostly head.", lines: [["pip", "button"], ["button", "bean"], ["bean", "peanut"], ["peanut", "tiny"], ["tiny", "pebble"], ["pebble", "pip"]] },
  ],
};

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
