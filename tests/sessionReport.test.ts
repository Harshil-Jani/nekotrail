// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import type { ChatEvent } from "../src/api";
import { buildReport, DEFAULT_OPTIONS, esc, exportFileName, findSecrets, isEnvPath, keyHunks, mdCode, mdFence, pickHunkFiles, prSummary, redactReport, toHtml, toMarkdown, type ReportSource } from "../src/sessionReport";
import { mdToHtml } from "../src/reportHtml";

const GH = "ghp_" + "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8";

const events: ChatEvent[] = [
  { kind: "user", text: "The load balancer returns 502 after a deploy. Fix it.", checkpoint: "c1" },
  { kind: "thinking", text: "hmm" },
  { kind: "tool", id: "t1", name: "Bash", input: "cd /Users/sam/code/api && kubectl logs deploy/api | grep 502" },
  { kind: "tool_result", id: "t1", output: "proxy error: connection reset", is_error: false },
  { kind: "tool", id: "t2", name: "Read", input: "/Users/sam/code/api/.env" },
  { kind: "tool_result", id: "t2", output: `PORT=8080\nGITHUB_TOKEN=${GH}\nSENTRY_ORG=pawprint-prod-8842`, is_error: false },
  { kind: "tool", id: "t3", name: "Bash", input: "aws elbv2 describe-load-balancers" },
  { kind: "tool_result", id: "t3", output: "AccessDenied", is_error: true },
  { kind: "tool", id: "t4", name: "TodoWrite", input: JSON.stringify([{ content: "Find the timeout", status: "completed" }, { content: "Add a test", status: "pending" }]) },
  { kind: "text", text: "Found it. The server closes idle connections after **30s**.\n\nRaised `IdleTimeout` to 75s." },
  { kind: "tool", id: "t5", name: "Edit", input: "internal/server/http.go" },
  { kind: "tool_result", id: "t5", output: "edited", is_error: false },
  { kind: "done", is_error: false, duration_ms: 61_000, cost_usd: 0.42 },
  { kind: "checks", results: [{ name: "test", status: "failed", passed: 41, failed: 2, regressed: true }] },
];

const src = (over: Partial<ReportSource> = {}): ReportSource => ({
  title: "Fix 502s after deploy",
  repo: "pawprint-api",
  branch: "sam/fix-502",
  target: "main",
  star: "Mochi",
  chats: [{ title: "Fix 502", agent: "claude", model: "opus", events }],
  changes: [
    { path: "internal/server/http.go", status: "M", additions: 3, deletions: 1 },
    { path: "bun.lock", status: "M", additions: 400, deletions: 10 },
  ],
  diffs: { "internal/server/http.go": "diff --git a/x b/x\n@@ -18,3 +18,4 @@\n srv := &http.Server{\n-  IdleTimeout: 30,\n+  IdleTimeout: 75,\n }" },
  checks: [{ name: "test", command: "go test ./...", status: "failed", passed: 41, failed: 2, regressed: true }],
  pr: { number: 482, url: "https://github.com/pawprint/api/pull/482", state: "OPEN", title: "Fix 502s", isDraft: false },
  home: "/Users/sam",
  exportedAt: "2026-11-07T10:00:00Z",
  ...over,
});

describe("report builder", () => {
  test("task, plan, timeline, stats and PR", () => {
    const r = buildReport(src());
    expect(r.task).toBe("The load balancer returns 502 after a deploy. Fix it.");
    // No ExitPlanMode, so the todo list stands in for the plan.
    expect(r.plan).toBe("- [x] Find the timeout\n- [ ] Add a test");
    const items = r.chats[0].items;
    expect(items.map((i) => i.kind)).toEqual(["user", "tool", "tool", "tool", "agent", "tool", "note"]);
    const tools = items.filter((i) => i.kind === "tool") as Extract<(typeof items)[number], { kind: "tool" }>[];
    expect(tools.map((t) => [t.name, t.target, t.failed])).toEqual([
      ["Bash", "cd ~/code/api && kubectl logs deploy/api | grep 502", false],
      ["Read", "~/code/api/.env", false],
      ["Bash", "aws elbv2 describe-load-balancers", true],
      ["Edit", "internal/server/http.go", false],
    ]);
    expect(r.stats).toEqual({ turns: 1, errors: 0, durationMs: 61_000, costUsd: 0.42, costKnown: true, toolCalls: 4, failedCommands: 1 });
    expect(r.agents).toEqual(["Claude"]);
    expect(r.additions).toBe(403);
  });

  test("tool outputs are off by default, and file contents need both toggles", () => {
    const off = buildReport(src());
    expect(off.chats[0].items.some((i) => i.kind === "tool" && i.output)).toBe(false);
    const outputs = buildReport(src(), { ...DEFAULT_OPTIONS, toolOutputs: true, fileContents: false });
    const out = (r: typeof off, name: string) => r.chats[0].items.filter((i) => i.kind === "tool" && i.name === name).map((i) => (i as { output: string | null }).output);
    expect(out(outputs, "Bash")).toEqual(["proxy error: connection reset", "AccessDenied"]);
    expect(out(outputs, "Read")).toEqual([null]);
    expect(outputs.hunks).toEqual([]);
    const all = buildReport(src(), { ...DEFAULT_OPTIONS, toolOutputs: true });
    expect(out(all, "Read")[0]).toContain("PORT=8080");
  });

  test("the task comes from the main chat, not a short second-opinion chat listed first", () => {
    const second = { title: "Second opinion", agent: "codex", model: null, events: [{ kind: "user", text: "Second opinion please." }, { kind: "text", text: "ok" }] as ChatEvent[] };
    const r = buildReport(src({ chats: [second, { title: "Fix 502", agent: "claude", model: "opus", events }] }));
    expect(r.task).toBe("The load balancer returns 502 after a deploy. Fix it.");
  });

  test("an ExitPlanMode plan wins over the todo list, without its heading", () => {
    const r = buildReport(src({ chats: [{ title: "c", agent: "codex", model: null, events: [...events, { kind: "tool", id: "p", name: "ExitPlanMode", input: "## Plan\n1. Do it" }] }] }));
    expect(r.plan).toBe("1. Do it");
  });

  test("key hunks skip lockfiles and cut long hunks", () => {
    expect(pickHunkFiles(src().changes)).toEqual(["internal/server/http.go"]);
    const long = `@@ -1,40 +1,40 @@\n${Array.from({ length: 40 }, (_, i) => `+line ${i}`).join("\n")}\n@@ -90 +90 @@\n+a\n@@ -99 +99 @@\n+b`;
    const k = keyHunks(long);
    expect(k.more).toBe(1);
    expect(k.text).toContain("… 11 more lines");
    expect(buildReport(src()).hunks.map((h) => h.path)).toEqual(["internal/server/http.go"]);
  });

  test("an empty workspace still makes a report", () => {
    const r = buildReport(src({ title: "", chats: [], changes: [], diffs: {}, checks: [], pr: null }));
    expect(r.title).toBe("Mochi");
    expect(toMarkdown(r)).toContain("_No messages yet._");
    expect(toMarkdown(r)).toContain("No PR yet.");
  });

  test(".env paths", () => {
    expect(isEnvPath("/x/.env")).toBe(true);
    expect(isEnvPath(".env.production")).toBe(true);
    expect(isEnvPath(".env.example")).toBe(false);
    expect(isEnvPath("src/env.ts")).toBe(false);
  });
});

describe("redaction across the report", () => {
  test("finds the token in .env output and hides it everywhere, with per-value un-mask", () => {
    const r = buildReport(src({ chats: [{ title: "c", agent: "claude", model: null, events: [...events, { kind: "user", text: `use ${GH} and pawprint-prod-8842` }] }] }), { ...DEFAULT_OPTIONS, toolOutputs: true });
    const found = findSecrets(r);
    expect(found.map((f) => [f.kind, f.value, f.count])).toEqual([
      ["GitHub token", GH, 2],
      [".env value", "pawprint-prod-8842", 1],
    ]);
    const hidden = toMarkdown(redactReport(r, found, new Set()));
    expect(hidden).not.toContain(GH);
    expect(hidden).not.toContain("pawprint-prod-8842");
    expect(hidden).toContain("[hidden: GitHub token]");
    // The user un-masks one value: only that one comes back.
    const shown = toMarkdown(redactReport(r, found, new Set(["pawprint-prod-8842"])));
    expect(shown).toContain("pawprint-prod-8842");
    expect(shown).not.toContain(GH);
  });
});

describe("markdown", () => {
  test("has every section, failed commands marked", () => {
    const md = toMarkdown(buildReport(src()));
    for (const h of ["# Fix 502s after deploy", "## Task", "## Plan", "## What the agent did", "## Changes", "## Checks", "## Pull request"]) expect(md).toContain(h);
    expect(md).toContain("- **Failed:** Ran `aws elbv2 describe-load-balancers`");
    expect(md).toContain("[#482 Fix 502s](https://github.com/pawprint/api/pull/482) (open)");
    expect(md).toContain("| 1m 1s | 1 | $0.42 |");
    expect(md.trimEnd().endsWith("_Made with nekotrail_")).toBe(true);
    expect(toMarkdown(buildReport(src(), { ...DEFAULT_OPTIONS, footer: false }))).not.toContain("Made with nekotrail");
  });

  test("code spans and fences survive backticks", () => {
    expect(mdCode("a`b")).toBe("``a`b``");
    expect(mdCode("`x`")).toBe("`` `x` ``");
    expect(mdFence("```js\nx\n```")).toBe("````\n```js\nx\n```\n````");
  });

  test("a non-web PR link is not written as a link", () => {
    const md = toMarkdown(buildReport(src({ pr: { number: 1, url: "javascript:alert(1)", state: "OPEN", title: "x", isDraft: true } })));
    expect(md).not.toContain("javascript:");
  });
});

describe("html", () => {
  const evil = '<script>alert("x")</script><img src=https://evil.example/p.png onerror=alert(1)>';
  const evilSrc = src({
    title: `Title ${evil}`,
    repo: `repo${evil}`,
    branch: `b"><script>1</script>`,
    chats: [{ title: `chat ${evil}`, agent: "claude", model: `m${evil}`, events: [
      { kind: "user", text: `prompt ${evil}` },
      { kind: "text", text: `answer ${evil}\n\n[click](javascript:alert(1)) ![pic](https://evil.example/t.png) <iframe src="https://evil.example"></iframe>` },
      { kind: "tool", id: "x", name: `Bash${evil}`, input: `echo ${evil}` },
      { kind: "tool_result", id: "x", output: `out ${evil}`, is_error: true },
      { kind: "error", text: `err ${evil}` },
    ] }],
    changes: [{ path: `a${evil}.ts`, status: "M", additions: 1, deletions: 0 }],
    diffs: { [`a${evil}.ts`]: `@@ -1 +1 @@\n+${evil}` },
    checks: [{ name: `t${evil}`, command: `c${evil}`, status: "passed", passed: 1, failed: 0, regressed: false }],
    pr: { number: 2, url: `https://github.com/x" onmouseover="alert(1)`, state: "OPEN", title: evil, isDraft: false },
  });

  test("nothing from the chat becomes markup", () => {
    const html = toHtml(buildReport(evilSrc, { ...DEFAULT_OPTIONS, toolOutputs: true }), mdToHtml);
    expect(html).not.toMatch(/<script/i);
    expect(html).not.toMatch(/<iframe/i);
    expect(html).not.toMatch(/<img/i);
    // No real tag carries an event handler or loads anything; escaped text may still read "onerror=".
    expect(html).not.toMatch(/<[^>]*\son[a-z]+=/i);
    expect(html).not.toMatch(/<[^>]*\ssrc=/i);
    expect(html).not.toContain("javascript:");
    // A web address in an answer may stay a link (opening it takes a click), never a broken-out attribute.
    expect(html).not.toContain('onmouseover="');
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("[image: pic]");
  });

  test("self-contained: inline style, light and dark, a policy that blocks requests, no scripts", () => {
    const html = toHtml(buildReport(src()), mdToHtml);
    expect(html).toContain("<style>");
    expect(html).toContain("prefers-color-scheme:dark");
    expect(html).toContain("default-src 'none'");
    expect(html).not.toMatch(/<script|<link|@import|url\(/i);
    expect(html).not.toMatch(/(src|href)="(?!https:\/\/github\.com\/pawprint)/);
    expect(html).toContain('<span class="tag">Failed</span>');
    expect(html).toContain("<strong>30s</strong>");
    expect(html).toContain("<footer>Made with nekotrail</footer>");
  });

  test("esc", () => {
    expect(esc(`<a href="x">'&'</a>`)).toBe("&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
  });
});

describe("PR summary", () => {
  test("what changed, how it was tested, which agent, cost", () => {
    const s = prSummary(buildReport(src()));
    expect(s).toContain("Task: The load balancer returns 502 after a deploy. Fix it.");
    expect(s).toContain("Found it. The server closes idle connections after **30s**.");
    expect(s).not.toContain("Raised");
    expect(s).toContain("2 files changed (+403 −11): `internal/server/http.go`, `bun.lock`.");
    expect(s).toContain("- test: failed, 41 passed, 2 failing, passed before this change");
    expect(s).toContain("Agent: Claude (opus). 1 turn, 1m 1s, cost $0.42.");
  });

  test("file names", () => {
    expect(exportFileName(buildReport(src()), "html")).toBe("fix-502s-after-deploy.html");
    expect(exportFileName(buildReport(src({ title: "!!!", chats: [] , star: "" })), "md")).toBe("session.md");
  });
});
