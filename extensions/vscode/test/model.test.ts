import { describe, expect, test } from "bun:test";
import { codeBlock, draftText, fence, isInside, rowForFile, statusBar, taskPrompt, toItems, where, type Selection } from "../src/model";
import { MAX_DRAFT, MAX_EDITOR_PROMPT, type Row } from "../src/protocol";

const row = (over: Partial<Row>): Row => ({
  id: "w1", name: "mochi", title: "", display: "mochi", branch: "me/mochi", path: "/w/mochi", target_branch: "main",
  repo_id: "r", repo: "app", state: "idle", agent: "claude", pr: null, pr_url: null, needs: [], ...over,
});

describe("the workspace list", () => {
  const rows = [
    row({ id: "a", display: "Zeta idle", name: "zeta" }),
    row({ id: "b", display: "Busy", state: "working", agent: "codex", path: "/w/busy" }),
    row({ id: "c", display: "Asks", state: "needs OK", needs: ["waiting for your OK"] }),
    row({ id: "d", display: "Unread", state: "done, unread", needs: ["agent finished, not read yet"] }),
    row({ id: "e", display: "Alpha idle", pr: "#4 open, checks failing", pr_url: "https://github.com/me/app/pull/4", needs: ["PR checks are failing"] }),
    row({ id: "f", display: "Read", state: "done, unread", needs: [] }),
  ];

  test("puts what needs you first, then working, then unread, then the rest by name", () => {
    expect(toItems(rows, null).map((i) => i.id)).toEqual(["e", "c", "d", "b", "f", "a"]);
  });

  test("says state, agent and PR, and marks needs-you", () => {
    const items = toItems(rows, "/w/busy");
    const get = (id: string) => items.find((i) => i.id === id)!;
    expect(get("b").description).toBe("working · Codex · this window");
    expect(get("b").icon).toBe("sync~spin");
    expect(get("b").contextValue).toBe("workspace.here");
    expect(get("e").description).toBe("needs you · Claude · #4 open, checks failing");
    expect(get("e").icon).toBe("bell-dot");
    expect(get("e").color).toBe("charts.orange");
    expect(get("e").contextValue).toBe("workspace.pr");
    expect(get("e").tooltip).toContain("Needs you: PR checks are failing");
    expect(get("f").icon).toBe("circle-filled");
    expect(get("a").icon).toBe("circle-outline");
    expect(get("a").tooltip).toContain("Branch me/mochi, compared with main");
  });

  test("works with rows from an app that predates v2 (no path)", () => {
    const old = toItems([row({ path: undefined, target_branch: undefined })], "/w/mochi");
    expect(old[0].contextValue).toBe("workspace");
    expect(old[0].tooltip).toContain("Branch me/mochi\n");
  });

  test("a label falls back to the name; an unknown agent shows its id", () => {
    expect(toItems([row({ display: "", agent: "aider" })], null)[0]).toMatchObject({ label: "mochi", description: "idle · aider" });
  });
});

describe("the status bar", () => {
  test("counts what needs you", () => {
    expect(statusBar("ok", 0).text).toBe("$(check) NekoTrail");
    expect(statusBar("ok", 1)).toEqual({ text: "$(bell-dot) 1 needs you", tooltip: "1 workspace needs you. Click to show NekoTrail." });
    expect(statusBar("ok", 3).text).toBe("$(bell-dot) 3 need you");
    expect(statusBar("offline", 3).tooltip).toContain("Click to start it");
    expect(statusBar("outdated", 0).tooltip).toContain("newer NekoTrail");
  });
});

describe("selections", () => {
  const sel = (code: string, over: Partial<Selection> = {}): Selection => ({ relPath: "src/a.ts", startLine: 12, endLine: 14, languageId: "typescript", code, ...over });

  test("carry their file and lines", () => {
    expect(where("src/a.ts", 3, 3)).toBe("src/a.ts:3");
    expect(where("src/a.ts", 3, 9)).toBe("src/a.ts:3-9");
    expect(codeBlock(sel("let a = 1;\n"), 1000)).toBe("From src/a.ts:12-14:\n\n```typescript\nlet a = 1;\n```");
    expect(codeBlock(sel("", { endLine: 12 }), 1000)).toBe("From src/a.ts:12.");
    expect(codeBlock(sel("x", { languageId: "plaintext" }), 1000)).toBe("From src/a.ts:12-14:\n\n```\nx\n```");
  });

  test("code with backticks can't close the block early", () => {
    expect(fence("no ticks")).toBe("```");
    expect(fence("````md\n```")).toBe("`````");
    expect(codeBlock(sel("a ``` b"), 1000)).toContain("````typescript\na ``` b\n````");
  });

  test("a task is the instruction, then the code", () => {
    expect(taskPrompt("  make it retry  ", sel("call()"))).toBe("make it retry\n\nFrom src/a.ts:12-14:\n\n```typescript\ncall()\n```");
  });

  test("long selections are cut at a line and still fit the app's limits", () => {
    const code = Array.from({ length: 3000 }, (_, i) => `line ${i} ${"x".repeat(20)}`).join("\n");
    const p = taskPrompt("explain", sel(code));
    expect(p.length).toBeLessThanOrEqual(MAX_EDITOR_PROMPT);
    expect(p).toMatch(/\(\d+ more lines left out to fit\.\)$/);
    expect(p).toContain("```typescript\nline 0 ");
    const kept = p.split("\n").filter((l) => l.startsWith("line ")).length;
    expect(p).toContain(`(${3000 - kept} more lines left out to fit.)`);
    expect(draftText(sel(code)).length).toBeLessThanOrEqual(MAX_DRAFT);
    expect(draftText(sel(code))).not.toContain("left out");
    // One enormous line can't be cut at a line: it says so instead.
    expect(codeBlock(sel("y".repeat(50_000)), 1000)).toBe("From src/a.ts:12-14. (The selection was too long to include.)");
  });
});

describe("which workspace a file is in", () => {
  test("inside a worktree, not a sibling with the same prefix", () => {
    expect(isInside("/w/mochi/src/a.ts", "/w/mochi")).toBe(true);
    expect(isInside("/w/mochi", "/w/mochi/")).toBe(true);
    expect(isInside("/w/mochi-2/a.ts", "/w/mochi")).toBe(false);
    expect(isInside("/w", "/w/mochi")).toBe(false);
    const rows = [row({ id: "a", path: "/w/mochi" }), row({ id: "b", path: "/w/mochi-2" }), row({ id: "c", path: undefined })];
    expect(rowForFile(rows, "/w/mochi-2/src/x.ts")?.id).toBe("b");
    expect(rowForFile(rows, "/elsewhere/x.ts")).toBeUndefined();
  });
});
