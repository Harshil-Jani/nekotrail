// Run with `bun test tests/`. The brief itself is built and tested in Rust (src-tauri/src/handoff.rs).
import { describe, expect, test } from "bun:test";
import type { Agent, ChatEvent } from "../src/api";
import { briefSize, defaultTarget, handoffMode, handoffOffer, handoffTargets, markerText, modeNote, reasonLine } from "../src/handoff";
import { buildReport, DEFAULT_OPTIONS, toMarkdown } from "../src/sessionReport";

describe("who a chat can go to", () => {
  const installed: Agent[] = ["claude", "codex", "gemini", "opencode", "cursor"];

  test("installed agents other than the chat's own, in the usual order", () => {
    expect(handoffTargets(installed, "claude")).toEqual(["codex", "gemini", "opencode", "cursor"]);
    expect(handoffTargets(installed, "codex")).toEqual(["claude", "gemini", "opencode", "cursor"]);
    expect(handoffTargets(null, "claude")).toEqual([]);
    expect(handoffTargets(["claude"], "claude")).toEqual([]);
  });

  test("a multi-repo task only offers agents that open every repo", () => {
    expect(handoffTargets(installed, "claude", true)).toEqual(["codex", "gemini"]);
  });

  test("the first pick: what was asked for, else Claude and Codex swap, else the first", () => {
    const t = handoffTargets(installed, "claude");
    expect(defaultTarget(t, "claude")).toBe("codex");
    expect(defaultTarget(t, "claude", "gemini")).toBe("gemini");
    expect(defaultTarget(t, "claude", "kiro")).toBe("codex");
    expect(defaultTarget(handoffTargets(installed, "codex"), "codex")).toBe("claude");
    expect(defaultTarget(["gemini", "cursor"], "claude")).toBe("gemini");
    expect(defaultTarget([], "claude")).toBeNull();
  });
});

describe("the new chat's mode never loosens", () => {
  test("Ask before changes stays where the agent can ask, else becomes Plan", () => {
    expect(handoffMode("ask", "claude")).toBe("ask");
    expect(handoffMode("ask", "opencode")).toBe("ask");
    expect(handoffMode("ask", "codex")).toBe("plan");
    expect(handoffMode("ask", "gemini")).toBe("plan");
    for (const m of ["plan", "acceptEdits", "bypassPermissions", "default"] as const) expect(handoffMode(m, "codex")).toBe(m);
  });

  test("the sheet says so", () => {
    expect(modeNote("ask", "codex")).toBe("Codex can't ask before each change, so it starts in Plan (read only). Switch its mode in the chat when you're ready.");
    expect(modeNote("acceptEdits", "codex")).toBe("Codex starts in Auto-accept edits, like this chat.");
  });
});

describe("when the chat offers a hand-off", () => {
  const user: ChatEvent = { kind: "user", text: "Fix the 502s." };
  const limit: ChatEvent = { kind: "text", text: "You've hit your limit · resets 3pm", fix: "limit", agent: "claude" };

  test("out of usage, going in circles, or a spend cap, after your last message", () => {
    expect(handoffOffer([user, limit, { kind: "done", is_error: true }])).toBe("limit");
    expect(handoffOffer([user, { kind: "error", text: "429", fix: "limit" }])).toBe("limit");
    expect(handoffOffer([user, { kind: "stop", reason: "loop", text: "Same command failed 4 times" }])).toBe("loop");
    expect(handoffOffer([user, { kind: "stop", reason: "budget", text: "Cap reached" }])).toBe("budget");
  });

  test("not once you wrote again, already handed off, or for other problems", () => {
    expect(handoffOffer([])).toBeNull();
    expect(handoffOffer([user, limit, { kind: "user", text: "try again" }])).toBeNull();
    expect(handoffOffer([user, limit, { kind: "handoff", dir: "to", agent: "codex", session_id: "s2" }])).toBeNull();
    expect(handoffOffer([user, { kind: "error", text: "not signed in", fix: "login" }])).toBeNull();
    expect(handoffOffer([user, { kind: "text", text: "Done." }, { kind: "done", is_error: false }])).toBeNull();
  });

  test("its line names the agent", () => {
    expect(reasonLine("limit", "claude")).toBe("Claude ran out of usage. Another agent can carry on in the same workspace with what Claude did.");
    expect(reasonLine("loop", "codex")).toContain("NekoTrail stopped Codex because it was going in circles");
    expect(reasonLine("budget", "claude")).toContain("The cap still holds");
    expect(reasonLine("asked", "claude")).toBeNull();
  });
});

describe("marker lines", () => {
  test("each chat points at the other", () => {
    expect(markerText({ kind: "handoff", dir: "to", agent: "codex", session_id: "s2" })).toBe("Handed off to Codex");
    expect(markerText({ kind: "handoff", dir: "from", agent: "claude", session_id: "s1", task: "Fix it", reason: "limit" })).toBe("Continued from Claude");
  });

  test("an export says where the chat handed off", () => {
    const report = buildReport({
      title: "Fix 502s", repo: "api", branch: "b", target: "main", star: "Mochi", changes: [], diffs: {}, checks: [], pr: null, home: null, exportedAt: "2026-11-07T10:00:00Z",
      chats: [{ title: "Fix", agent: "claude", model: null, events: [{ kind: "user", text: "Fix it" }, { kind: "handoff", dir: "to", agent: "codex", session_id: "s2" }] }],
    }, DEFAULT_OPTIONS);
    expect(report.chats[0].items.at(-1)).toEqual({ kind: "note", text: "Handed off to Codex.", bad: false });
    expect(toMarkdown(report)).toContain("_Handed off to Codex._");
  });
});

describe("brief size", () => {
  test("counts characters, not UTF-16 units, against the soft limit", () => {
    expect(briefSize("abc", 6000)).toEqual({ chars: 3, over: false, label: "3 of about 6,000 characters" });
    expect(briefSize("😀".repeat(10), 6000).chars).toBe(10);
    const big = briefSize("x".repeat(6001), 6000);
    expect(big.over).toBe(true);
    expect(big.label).toBe("6,001 of about 6,000 characters");
  });
});
