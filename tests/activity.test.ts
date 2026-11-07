// Run with `bun test tests/`.
// Event shapes are the ones the backend parsers send (src-tauri/src/agent.rs for Claude, Codex and
// Gemini; src-tauri/src/acp.rs for OpenCode and the other ACP agents).
import { describe, expect, test } from "bun:test";
import { ANSWER_GRACE_MS, currentActivity, elapsed, shortPath, STALL_MS, toolLabel, TurnClock, workingLine, type ActivityEvent } from "../src/activity";

const user = (text = "Fix the login bug"): ActivityEvent => ({ kind: "user", text } as ActivityEvent);
const tool = (id: string, name: string, input: string): ActivityEvent => ({ kind: "tool", id, name, input });
const result = (id: string): ActivityEvent => ({ kind: "tool_result", id, output: "", is_error: false } as ActivityEvent);
const text = (t: string): ActivityEvent => ({ kind: "text", text: t } as ActivityEvent);
const done: ActivityEvent = { kind: "done" };
const label = (evs: ActivityEvent[], root?: string) => currentActivity(evs, root).label;
const phase = (evs: ActivityEvent[]) => currentActivity(evs).phase;

describe("the turn", () => {
  test("right after you send, before any event: Thinking", () => {
    expect(currentActivity([user()])).toEqual({ phase: "thinking", label: "Thinking", turn: 0 });
  });
  test("an earlier turn's Done doesn't end the new one", () => {
    const evs = [user("a"), text("ok"), done, user("b")];
    expect(currentActivity(evs)).toEqual({ phase: "thinking", label: "Thinking", turn: 3 });
  });
  test("Done, Stopped and a stop card end it", () => {
    expect(phase([user(), tool("1", "Bash", "ls"), done])).toBe("idle");
    expect(phase([user(), { kind: "done", stopped: true } as ActivityEvent])).toBe("idle");
    expect(phase([user(), { kind: "stop", reason: "loop" } as ActivityEvent])).toBe("idle");
  });
  test("no message at all: nothing to show once it's done", () => {
    expect(currentActivity([])).toEqual({ phase: "thinking", label: "Thinking", turn: -1 });
  });
  test("text on screen: the row steps aside; an error too", () => {
    expect(phase([user(), text("Found it.")])).toBe("answer");
    expect(phase([user(), { kind: "error", text: "rate limited" } as ActivityEvent])).toBe("answer");
  });
  test("text, then a new tool call: back to naming it", () => {
    expect(label([user(), text("Let me look."), tool("1", "Read", "src/auth.ts")])).toBe("Reading src/auth.ts");
  });
  test("a thinking block: Thinking", () => {
    expect(label([user(), tool("1", "Read", "a.ts"), result("1"), { kind: "thinking", text: "hmm" } as ActivityEvent])).toBe("Thinking");
  });
  test("a finished tool and nothing since: Thinking, not the old tool", () => {
    expect(label([user(), tool("1", "Bash", "bun test"), result("1")])).toBe("Thinking");
  });
  test("notes, usage and guard lines don't change what it's doing", () => {
    const evs = [user(), tool("1", "Bash", "bun test"), { kind: "usage" }, { kind: "note", text: "x" } as ActivityEvent];
    expect(label(evs)).toBe("Running bun test");
  });
  test("a question waiting for you: the card asks, the row steps aside", () => {
    const ask = { kind: "approval", request_id: "r1", tool: "Bash", input: "rm -rf dist" } as ActivityEvent;
    expect(phase([user(), tool("1", "Bash", "rm -rf dist"), ask])).toBe("waiting");
    const answered = { kind: "approval_answer", request_id: "r1", allow: true } as ActivityEvent;
    expect(label([user(), tool("1", "Bash", "rm -rf dist"), ask, answered])).toBe("Running rm -rf dist");
  });
});

describe("Claude", () => {
  test("absolute paths read as the last two parts, or workspace-relative with a root", () => {
    expect(label([user(), tool("t1", "Read", "/Users/me/code/pawprint/src/auth.ts")])).toBe("Reading src/auth.ts");
    expect(label([user(), tool("t1", "Read", "/w/mochi/src/components/Chat.tsx")], "/w/mochi")).toBe("Reading src/components/Chat.tsx");
  });
  test("parallel reads still running: Reading 3 files", () => {
    const evs = [user(), tool("a", "Read", "/w/a.ts"), tool("b", "Read", "/w/b.ts"), tool("c", "Read", "/w/c.ts")];
    expect(label(evs)).toBe("Reading 3 files");
  });
  test("a finished read isn't counted again", () => {
    expect(label([user(), tool("a", "Read", "/w/a.ts"), result("a"), tool("b", "Read", "/w/b.ts")])).toBe("Reading w/b.ts");
  });
  test("a run of edits: Editing 2 files", () => {
    const evs = [user(), tool("a", "Edit", "/w/src/a.ts"), result("a"), tool("b", "MultiEdit", "/w/src/b.ts")];
    expect(label(evs)).toBe("Editing 2 files");
  });
  test("the same file twice is one file", () => {
    expect(label([user(), tool("a", "Edit", "/w/src/a.ts"), result("a"), tool("b", "Edit", "/w/src/a.ts")])).toBe("Editing src/a.ts");
  });
  test("an edit after a command starts a new run", () => {
    const evs = [user(), tool("a", "Edit", "/w/src/a.ts"), result("a"), tool("b", "Bash", "bun test"), result("b"), tool("c", "Edit", "/w/src/c.ts")];
    expect(label(evs)).toBe("Editing src/c.ts");
  });
  test("commands: first line, without the cd", () => {
    expect(label([user(), tool("1", "Bash", "cd /w/mochi && bun test")])).toBe("Running bun test");
    expect(label([user(), tool("1", "Bash", "git status\ngit diff")])).toBe("Running git status");
  });
  test("every built-in tool in plain words", () => {
    expect(toolLabel("Write", "/w/src/new.ts")).toBe("Writing src/new.ts");
    expect(toolLabel("Grep", "useEffect")).toBe("Searching for useEffect");
    expect(toolLabel("Glob", "**/*.test.ts")).toBe("Finding **/*.test.ts");
    expect(toolLabel("WebFetch", "https://docs.rs/tokio/latest")).toBe("Reading docs.rs");
    expect(toolLabel("WebSearch", "tauri 2 window state")).toBe("Searching the web for tauri 2 window state");
    expect(toolLabel("Task", "Find the flaky test")).toBe("Handing a task to a helper: Find the flaky test");
    expect(toolLabel("TodoWrite", "[]")).toBe("Updating the to-do list");
    expect(toolLabel("ExitPlanMode", "# Plan")).toBe("Writing the plan");
  });
  test("MCP tools name the server and the tool", () => {
    expect(toolLabel("mcp__github__create_issue", "{}")).toBe("Using github: create issue");
    expect(toolLabel("mcp__claude_ai_Linear__list_issues", "{}")).toBe("Using claude_ai_Linear: list issues");
  });
  test("long commands are cut to one line", () => {
    const l = toolLabel("Bash", `echo ${"x".repeat(300)}`);
    expect(l.length).toBeLessThanOrEqual("Running ".length + 120);
    expect(l.endsWith("…")).toBe(true);
  });
});

describe("Codex", () => {
  test("a running command", () => {
    expect(label([user(), tool("item_1", "Bash", "bash -lc 'cargo test'")])).toBe("Running bash -lc 'cargo test'");
  });
  test("one Edit for several files reads as Editing N files", () => {
    expect(toolLabel("Edit", "src/a.rs, src/b.rs, src/c.rs")).toBe("Editing 3 files");
  });
  test("a file change comes with its result at once: back to Thinking", () => {
    expect(label([user(), tool("item_2", "Edit", "src/a.rs"), result("item_2")])).toBe("Thinking");
  });
  test("reasoning, then the answer", () => {
    expect(label([user(), { kind: "thinking", text: "plan" } as ActivityEvent])).toBe("Thinking");
    expect(phase([user(), { kind: "thinking", text: "plan" } as ActivityEvent, text("Done.")])).toBe("answer");
  });
});

describe("Gemini", () => {
  test("mapped tools read like Claude's", () => {
    expect(label([user(), tool("g1", "Read", "src/index.ts")])).toBe("Reading src/index.ts");
    expect(label([user(), tool("g2", "Bash", "npm test")])).toBe("Running npm test");
    expect(label([user(), tool("g3", "Grep", "TODO")])).toBe("Searching for TODO");
  });
  test("list_directory and unmapped tools", () => {
    expect(toolLabel("list_directory", "src")).toBe("Looking in src");
    expect(toolLabel("read_many_files", "{\"paths\":[]}")).toBe("Using read many files");
    expect(toolLabel("save_memory", "{}")).toBe("Using save memory");
  });
});

describe("ACP agents (OpenCode and others)", () => {
  test("by kind: List, Delete, Move, relative paths", () => {
    expect(toolLabel("List", "src/components")).toBe("Looking in src/components");
    expect(toolLabel("Delete", "old.ts")).toBe("Deleting old.ts");
    expect(toolLabel("Move", "a.ts")).toBe("Moving a.ts");
  });
  test("a tool with no name the chat knows", () => {
    expect(toolLabel("tool", "")).toBe("Using a tool");
    expect(toolLabel("lsp-diagnostics", "")).toBe("Using lsp diagnostics");
  });
  test("tool, then permission ask, then its run", () => {
    const evs = [user(), tool("c1", "Edit", "src/app.ts"), { kind: "approval", request_id: "7", tool: "Edit", input: "src/app.ts" } as ActivityEvent];
    expect(phase(evs)).toBe("waiting");
    expect(label([...evs, { kind: "approval_answer", request_id: "7", allow: true } as ActivityEvent])).toBe("Editing src/app.ts");
  });
  test("a todo update", () => {
    expect(label([user(), tool("c2", "TodoWrite", "[]")])).toBe("Updating the to-do list");
  });
});

describe("paths and time", () => {
  test("shortPath", () => {
    expect(shortPath("src/a.ts")).toBe("src/a.ts");
    expect(shortPath("/w/ws/src/a.ts", "/w/ws/")).toBe("src/a.ts");
    expect(shortPath("/w/ws", "/w/ws")).toBe(".");
    expect(shortPath("C:\\code\\app\\main.rs")).toBe("app/main.rs");
  });
  test("elapsed", () => {
    expect(elapsed(0)).toBe("0s");
    expect(elapsed(4_900)).toBe("4s");
    expect(elapsed(72_000)).toBe("1m 12s");
    expect(elapsed(3_780_000)).toBe("1h 3m");
    expect(elapsed(-5)).toBe("0s");
  });
});

describe("the working row", () => {
  const start = 1_000_000;
  const thinking = currentActivity([user()]);
  const reading = currentActivity([user(), tool("1", "Read", "src/auth.ts")]);
  const answer = currentActivity([user(), text("Found it.")]);
  test("shows the activity and the time since the turn started", () => {
    expect(workingLine(thinking, start + 4_200, start, start)).toEqual({ show: true, label: "Thinking", time: "4s", stalled: false });
    expect(workingLine(reading, start + 9_000, start, start + 8_000).label).toBe("Reading src/auth.ts");
  });
  test("a minute with no event: Still working, with the whole turn's time", () => {
    expect(workingLine(reading, start + 72_000, start, start + 72_000 - STALL_MS)).toEqual({ show: true, label: "Still working", time: "1m 12s", stalled: true });
    expect(workingLine(reading, start + 72_000, start, start + 72_000 - STALL_MS + 1).stalled).toBe(false);
  });
  test("text on screen hides it; a quiet spell after it brings it back", () => {
    expect(workingLine(answer, start + 10_000, start, start + 10_000).show).toBe(false);
    expect(workingLine(answer, start + 10_000, start, start + 10_000 - ANSWER_GRACE_MS)).toMatchObject({ show: true, label: "Thinking" });
  });
  test("idle and waiting never show", () => {
    expect(workingLine(currentActivity([user(), done]), start + 90_000, start, start).show).toBe(false);
    const waiting = currentActivity([user(), { kind: "approval", request_id: "1" }]);
    expect(workingLine(waiting, start + 90_000, start, start).show).toBe(false);
  });
});

describe("TurnClock", () => {
  test("keeps the start, moves the last event only when the count moves", () => {
    const c = new TurnClock();
    expect(c.see("s1:4", 5, 100)).toEqual({ start: 100, last: 100 });
    expect(c.see("s1:4", 5, 900)).toEqual({ start: 100, last: 100 });
    expect(c.see("s1:4", 6, 1_500)).toEqual({ start: 100, last: 1_500 });
    expect(c.see("s1:9", 10, 2_000)).toEqual({ start: 2_000, last: 2_000 });
  });
  test("seed sets a turn's times", () => {
    const c = new TurnClock();
    c.seed("s1:3", 10, 20, 4);
    expect(c.see("s1:3", 4, 5_000)).toEqual({ start: 10, last: 20 });
  });
});
