/**
 * Sample data for rendering the real UI in a plain browser (no Tauri backend).
 * Open /preview.html?ws=w1&tab=chat (tabs: chat, changes, terminal, notes; ?view=sky).
 * All names, numbers and PRs here are made up for screenshots.
 */
import React from "react";
import ReactDOM from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import App from "./App";
import catalog from "../src-tauri/src/models.json";

if (new URLSearchParams(location.search).get("filter")) localStorage.setItem("nekotrail.settings", JSON.stringify({ repoFilter: ["r1"], collapsed: [] }));
if (new URLSearchParams(location.search).get("split")) localStorage.setItem("nekotrail.split", "1");
if (new URLSearchParams(location.search).get("group") === "status") localStorage.setItem("nekotrail.settings", JSON.stringify({ groupBy: "status" }));

// Sample review comments so the Changes tab shows them.
localStorage.setItem("nekotrail.review.w1", JSON.stringify([
  { id: "c1", file: "internal/server/http.go", line: 21, side: "new", code: "IdleTimeout: 75 * time.Second,", text: "Read this from config instead, so ops can tune it per environment." },
]));

const repos = [
  { id: "r1", name: "sage-backend", root_path: "/code/sage-backend", default_branch: "main", remote_url: null, setup_script: "go mod download", run_script: "make dev", archive_script: null },
  { id: "r2", name: "website", root_path: "/code/website", default_branch: "main", remote_url: null, setup_script: null, run_script: null, archive_script: null },
];
const pairOf = (id: string) => (id === "w1" || id === "w2" ? "p1" : "");
const ws = (id: string, repo_id: string, name: string, branch: string, extra = {}) => ({
  id, repo_id, name, branch, path: `/code/ws/${name}`, target_branch: "main", state: "active", pinned: false, unread: false, notes: "", imported_from: null, created_at: "2026-11-05T10:00:00Z", title: "", status: "", pair_id: pairOf(id), ...extra,
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
  { kind: "tool", id: "t3", name: "ExitPlanMode", input: "## Plan\n1. Add `BusinessHours` (9:00 to 18:00, Mon to Fri, holiday list from config).\n2. `slaDeadline` adds 4 business hours instead of 4 wall-clock hours.\n3. Table tests: Friday 17:00 rolls to Monday 12:00; holidays skip.\n\nNo schema changes." },
];

const workspaces = [
  ws("w1", "r1", "mochi", "harshil/fix-502-gateway", { pinned: true }),
  ws("w2", "r1", "comet", "harshil/ticket-sla", { unread: true }),
  ws("w3", "r1", "dash", "harshil/rate-limit"),
  ws("w4", "r2", "ziggy", "harshil/pricing-page"),
  ws("w5", "r2", "pip", "harshil/blog-rss"),
];
const events = [
  { kind: "user", checkpoint: "abc123", text: "The load balancer returns 502 on /v1/tickets right after a deploy. Find out why and fix it." },
  { kind: "tool", id: "t1", name: "Bash", input: "aws logs tail /ecs/sage-backend --since 30m | grep -i 502" },
  { kind: "tool_result", id: "t1", output: "2026-11-05T09:12:44Z http: proxy error: read tcp 10.0.3.14:43120->10.0.7.2:8080: connection reset by peer", is_error: false },
  { kind: "tool", id: "t2", name: "Read", input: "internal/server/http.go" },
  { kind: "tool_result", id: "t2", output: "srv := &http.Server{\n  Addr: \":8080\",\n  IdleTimeout: 30 * time.Second,\n}", is_error: false },
  { kind: "text", text: "Found it. The server closes idle keep-alive connections after **30s**, but the load balancer keeps them for **60s**. After a quiet minute the balancer reuses a connection the server already closed, and the client gets a 502.\n\nI raised `IdleTimeout` to 75s so the server always outlives the balancer, and added a test that fails on the old value." },
  { kind: "tool", id: "t3", name: "Edit", input: "internal/server/http.go" },
  { kind: "tool_result", id: "t3", output: "", is_error: false },
  { kind: "tool", id: "t4", name: "Bash", input: "go test ./internal/server/..." },
  { kind: "tool_result", id: "t4", output: "ok  \tsage/internal/server\t0.412s", is_error: false },
  { kind: "text", text: "Review of the branch:\n- internal/server/http.go:22: 75s is right for the ALB, but the gRPC port sits behind an NLB with a 350s timeout. Fix: make it configurable.\n- internal/server/http_test.go:14: the test sleeps 61s. Fix: inject a clock.\n\nOtherwise ready to merge." },
  { kind: "done", is_error: false, duration_ms: 41200, cost_usd: 0.18, context_tokens: 128000, context_window: 200000 },
];
const pr = {
  number: 482, url: "https://github.com", state: "OPEN", title: "Fix 502s after deploy: outlive the ALB idle timeout", isDraft: false,
  reviewDecision: "REVIEW_REQUIRED", mergeable: "MERGEABLE", additions: 38, deletions: 4, comments: [{}, {}],
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
        return { repos, workspaces, running: [], running_workspaces: ["w3"], lit: ["mochi", "biscuit", "tofu", "dumpling", "pudding", "muffin", "ziggy", "comet", "dash", "pip"], shining: ["mochi", "biscuit", "tofu", "dumpling", "pudding", "muffin", "comet", "dash"], costs: { w1: 4.82, w2: 1.1 } };
      case "github_profile": return { login: "Harshil-Jani", name: "Harshil Jani", avatar_url: "https://github.com/Harshil-Jani.png?size=96", html_url: "", public_repos: 80, followers: 120 };
      case "repo_github": return { nameWithOwner: "nactore/" + args.id, description: "", stargazerCount: args.id === "r1" ? 1240 : 86, url: "", owner_avatar: "https://github.com/github.png?size=64", isPrivate: false };
      case "sky": return SKY;
      case "conductor_available": return true;
      case "sessions": return args.workspaceId === "w1"
        ? [{ id: "s2", workspace_id: "w1", agent: "codex", agent_session_id: null, title: "Second opinion", model: null, permission_mode: "plan" }, { id: "s1", workspace_id: "w1", agent: "claude", agent_session_id: "x", title: "Fix 502 after deploy", model: "opus", permission_mode: "acceptEdits", effort: "high" }]
        : [{ id: "s9", workspace_id: args.workspaceId, agent: "claude", agent_session_id: null, title: "New chat", model: null, permission_mode: "acceptEdits" }];
      case "session_events": return args.id === "s1" ? events : args.id === "s9" && new URLSearchParams(location.search).get("ws") === "w2" ? planEvents : [];
      case "queued": return args.id === "s9" && new URLSearchParams(location.search).get("ws") === "w2" ? ["Also add a metric for SLA breaches", "Then open the PR"] : [];
      case "pr_status":
        if (args.id === "w1") return pr;
        if (args.id === "w2") return { ...pr, number: 479, statusCheckRollup: [{ name: "go test", conclusion: "FAILURE" }, { name: "lint", conclusion: "SUCCESS" }], latestReviews: [], reviewRequests: [] };
        if (args.id === "w4") return { ...pr, number: 471, state: "MERGED", latestReviews: [{ author: { login: "octocat" }, state: "APPROVED" }], reviewRequests: [] };
        return null;
      case "changes": return args.id === "w1" ? [
        { path: "internal/server/http.go", status: "M", additions: 3, deletions: 1 },
        { path: "internal/server/http_test.go", status: "A", additions: 35, deletions: 0 },
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
      case "line_counts": return { w1: [38, 4], w2: [120, 31], w3: [12, 0], w4: [6, 2] };
      case "pr_states": return { w1: "OPEN", w2: "OPEN", w4: "MERGED" };
      case "open_in_targets": return [{ id: "cursor", name: "Cursor" }, { id: "vscode", name: "Visual Studio Code" }, { id: "zed", name: "Zed" }, { id: "ghostty", name: "Ghostty" }, { id: "finder", name: "Finder" }];
      case "doctor": return { git: { ok: true, version: "git version 2.50.1" }, gh: { installed: true, ok: !location.search.includes("fresh"), login: location.search.includes("fresh") ? null : "Harshil-Jani" }, claude: { ok: true, version: "2.4.1 (Claude Code)" }, codex: { ok: false, version: "" } };
      case "import_preview": return { repos: 3, workspaces: 12, sessions: 31, skipped: 2, chats_with_history: 0 };
      case "usage_summary": return { today: 3.4, week: 41.27, by_agent: { claude: 33.1, codex: 8.17 } };
      case "workspace_stats": return args.id === "w1" ? { agent: "claude", turns: 4, errors: 0, duration_ms: 182000, cost_usd: 1.92 } : { agent: "codex", turns: 3, errors: 1, duration_ms: 141000, cost_usd: 0.88 };
      case "model_catalog": return catalog;
      case "mcp_list": return "Checking MCP server health…\n\ngithub: https://api.githubcopilot.com/mcp - ✔ Connected\nlinear: https://mcp.linear.app/sse - ✔ Connected\npostgres: npx -y @modelcontextprotocol/server-postgres - ✗ Failed to connect";
      case "slash_commands": return [["ship", "Run tests, bump the version and open the release PR"], ["triage", "Label and prioritise new issues"]];
      case "workspace_files": return ["internal/server/http.go", "internal/server/http_test.go", "cmd/api/main.go", "go.mod", "README.md", "internal/tickets/handler.go"];
      case "workspace_scripts": return { setup: null, run: "make dev", archive: null };
      default: return null;
    }
  },
  { shouldMockEvents: true },
);

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
