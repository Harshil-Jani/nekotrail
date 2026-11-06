/**
 * Sample data for rendering the real UI in a plain browser (no Tauri backend).
 * Open /preview.html?ws=w1&tab=chat (tabs: chat, changes, terminal, notes; ?view=sky).
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

const repos = [
  { id: "r1", name: "pawprint-api", root_path: "/code/pawprint-api", default_branch: "main", remote_url: null, setup_script: "go mod download", run_script: "make dev", archive_script: null,
    prefs: new URLSearchParams(location.search).has("guard") ? { protected: [".env*", "migrations/**", "infra/**", "!migrations/README.md"] } : undefined },
  { id: "r2", name: "pawprint-web", root_path: "/code/pawprint-web", default_branch: "main", remote_url: null, setup_script: null, run_script: null, archive_script: null },
];
const pairOf = (id: string) => (id === "w1" || id === "w2" ? "p1" : "");
const ws = (id: string, repo_id: string, name: string, branch: string, extra = {}) => ({
  id, repo_id, name, branch, path: `/code/ws/${name}`, target_branch: "main", state: "active", pinned: false, unread: false, notes: "", imported_from: null, created_at: "2026-11-05T10:00:00Z", title: "", title_source: "", status: "", pair_id: pairOf(id), section_id: id === "w3" || id === "w4" ? "sec1" : "", parent_id: "", ...extra,
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

mockIPC(
  (cmd, args: any) => {
    switch (cmd) {
      case "snapshot": if (location.search.includes("fresh")) return { repos: [], workspaces: [], running: [], running_workspaces: [], lit: [], shining: [], costs: {} };
        return { repos, workspaces: args.includeArchived ? [...workspaces, archivedWs] : workspaces, running: opencode ? ["s9"] : [], running_workspaces: opencode ? ["w3", "w2"] : ["w3"], lit: ["mochi", "biscuit", "tofu", "dumpling", "pudding", "muffin", "ziggy", "comet", "dash", "pip"], shining: ["mochi", "biscuit", "tofu", "dumpling", "pudding", "muffin", "comet", "dash"], costs: { w1: 4.82, w2: 1.1 } };
      case "github_profile": return { login: "Harshil-Jani", name: "Harshil Jani", avatar_url: "https://github.com/Harshil-Jani.png?size=96", html_url: "", public_repos: 80, followers: 120 };
      case "repo_github": return { nameWithOwner: "pawprint/" + args.id, description: "", stargazerCount: args.id === "r1" ? 1240 : 86, url: "", owner_avatar: "", isPrivate: false };
      case "sky": return SKY;
      case "conductor_available": return true;
      case "search_chats": return previewSearch(args.query, args.filter);
      case "search_open": { const c = corpus.find((x) => x.id === args.messageId)!; return { session: { id: c.session, workspace_id: c.ws, agent: c.agent, agent_session_id: null, title: c.chat, model: null, permission_mode: "acceptEdits", effort: null }, workspace_id: c.ws, index: c.index, total: c.total }; }
      case "sessions": if (args.workspaceId === "w9") return [{ id: "s7", workspace_id: "w9", agent: "codex", agent_session_id: null, title: "Webhook retries", model: null, permission_mode: "acceptEdits" }];
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
          return null;
        };
        return Object.fromEntries(["w1", "w2", "w3", "w4", "w5"].map((id) => [id, { workspace_id: id, pr: prFor(id) }]));
      }
      // Files per workspace add up to line_counts below. Dash (w3) and Pip (w5) have no changes yet.
      case "changes": return args.id === "w1" ? [
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
      ] : [];
      case "file_diff": return diff;
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
      case "line_counts": return { w1: [38, 1], w2: [120, 31], w4: [6, 2] };
      case "open_in_targets": return [{ id: "cursor", name: "Cursor" }, { id: "vscode", name: "Visual Studio Code" }, { id: "zed", name: "Zed" }, { id: "ghostty", name: "Ghostty" }, { id: "finder", name: "Finder" }];
      case "doctor": return { git: { ok: true, version: "git version 2.50.1" }, gh: { installed: true, ok: !location.search.includes("fresh"), login: location.search.includes("fresh") ? null : "Harshil-Jani" }, claude: { ok: true, version: "2.4.1 (Claude Code)" },
        ...Object.fromEntries(AGENTS.filter((a) => a !== "claude").map((a) => [a, installed.includes(a) ? { ok: true, version: `${a} 1.0.0` } : { ok: false, version: "" }])) };
      // ?agents=copilot,grok: the CLIs on PATH besides Claude (default: Copilot). ?agents=all: every one.
      case "installed_agents": return installed;
      case "import_preview": return { repos: 3, workspaces: 12, sessions: 31, skipped: 2, chats_with_history: 0 };
      case "usage_summary": return { today: 3.4, week: 41.27, by_agent: { claude: 33.1, codex: 8.17 } };
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
      case "skill_share": return "~/.agents/skills/triage";
      case "slash_commands": return [["ship", "Run tests, bump the version and open the release PR"], ["triage", "Label and prioritise new issues"]];
      case "sections": return [{ id: "sec1", name: "This week", sort: 1 }, { id: "sec2", name: "Waiting on review", sort: 2 }];
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
      case "workspace_storage": return [
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
      case "add_review_comments": { const cur = reviews[args.id] ?? []; reviews[args.id] = [...cur, ...args.comments.filter((c: any) => !cur.some((x) => x.id === c.id))]; return null; }
      case "remove_review_comments": reviews[args.id] = (reviews[args.id] ?? []).filter((c) => !args.ids.includes(c.id)); return null;
      case "send": (window as any).__sent = [...((window as any).__sent ?? []), args.text]; return null;
      case "save_attachment": (window as any).__lastAttachment = args.bytes; return `.nekotrail/attachments/140212-${args.name}`;
      case "remote_take": return [];
      case "cli_status": return { path: null };
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
