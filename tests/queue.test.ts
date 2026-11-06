// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { counts, dropBefore, elapsed, groups, moved, stateText, stillGoing, summaryText, taskTitle, thenText, unattended, waitText, type QueueTask } from "../src/queue";

const task = (id: string, extra: Partial<QueueTask> = {}): QueueTask => ({
  id, prompt: `task ${id}`, repo_id: "r1", base: null, agent: "claude", model: null, effort: null, mode: "acceptEdits", fallback: null,
  then: { checks: false, draft_pr: false, autopilot: false }, state: "queued", note: "", step: "", workspace_id: null, session_id: null,
  ran_with: null, resume: false, attempts: 0, added_at: 0, started_at: null, ended_at: null, ...extra,
});

describe("queue words", () => {
  test("the morning summary reads like the backend's", () => {
    expect(summaryText({ done: 8, needs_you: 2, failed: 1 })).toBe("8 done, 2 need you, 1 failed");
    expect(summaryText({ done: 0, needs_you: 1, failed: 0 })).toBe("1 needs you");
    expect(summaryText({ done: 0, needs_you: 0, failed: 0 })).toBe("");
    expect(stillGoing({ running: 1, queued: 3 })).toBe("1 still running, 3 in line.");
    expect(stillGoing({ running: 0, queued: 0 })).toBe("");
  });

  test("why a task waits", () => {
    const v = { max_total: 3, max_per_repo: 2, now: 1_800_000_000 };
    expect(waitText({ kind: "paused" }, v)).toBe("Paused");
    expect(waitText({ kind: "slots" }, v)).toBe("Waits for a free slot. 3 run at once");
    expect(waitText({ kind: "repo_slots" }, v)).toBe("Waits for a slot in this repo. 2 run at once per repo");
    expect(waitText({ kind: "probe", agent: "codex" }, v)).toBe("One task checks that Codex has usage again first");
    expect(waitText({ kind: "limit", agent: "claude", until: v.now + 3600, reported: true }, v, "en-US")).toMatch(/^Claude is out of usage until \d{1,2}:\d{2}\s?[AP]M$/);
    expect(waitText({ kind: "limit", agent: "claude", until: v.now + 3600, reported: false }, v, "en-US")).toMatch(/^Claude is out of usage\. Trying again at /);
    // More than a day away: the day too.
    expect(waitText({ kind: "limit", agent: "claude", until: v.now + 3 * 86400, reported: true }, v, "en-US")).toMatch(/until [A-Z][a-z]{2} \d/);
  });

  test("what a task is doing, and what follows it", () => {
    expect(stateText(task("a", { state: "running" }), false)).toBe("Working");
    expect(stateText(task("a", { state: "running" }), true)).toBe("Waiting for your OK");
    expect(stateText(task("a", { state: "running", step: "setup" }), false)).toBe("Running setup");
    expect(stateText(task("a", { state: "then", step: "pr" }), false)).toBe("Opening a draft PR");
    expect(stateText(task("a", { state: "waiting" }), false)).toBe("Waits for you to start it");
    expect(thenText({ checks: true, draft_pr: false, autopilot: true })).toBe("then checks, draft PR, autopilot");
    expect(thenText({ checks: false, draft_pr: false, autopilot: false })).toBe("");
    expect(elapsed(0, 30)).toBe("just now");
    expect(elapsed(0, 14 * 60)).toBe("14 min");
    expect(elapsed(0, 65 * 60)).toBe("1 h 5 min");
    expect(taskTitle("\n  Fix the login bug\nwith details")).toBe("Fix the login bug");
    expect(taskTitle("x".repeat(100)).length).toBe(80);
  });

  test("only modes that never stop for you run unattended", () => {
    expect(unattended("claude", "acceptEdits")).toBe(true);
    expect(unattended("claude", "plan")).toBe(true);
    expect(unattended("claude", "ask")).toBe(false);
    expect(unattended("opencode", "acceptEdits")).toBe(false);
    expect(unattended("opencode", "bypassPermissions")).toBe(true);
    expect(unattended("gemini", "acceptEdits")).toBe(true);
  });
});

describe("queue lists", () => {
  test("running, up next in order, finished newest first", () => {
    const g = groups([
      task("a", { state: "done", ended_at: 10 }), task("b"), task("c", { state: "running" }), task("d", { state: "waiting" }),
      task("e", { state: "failed", ended_at: 30 }), task("f", { state: "then" }), task("g", { state: "needs_you", ended_at: 20 }),
    ]);
    expect(g.running.map((t) => t.id)).toEqual(["c", "f"]);
    expect(g.next.map((t) => t.id)).toEqual(["b", "d"]);
    expect(g.finished.map((t) => t.id)).toEqual(["e", "g", "a"]);
    expect(counts([task("a", { state: "done" }), task("b", { state: "needs_you" }), task("c", { state: "failed" }), task("d", { state: "starting" }), task("e")]))
      .toEqual({ running: 1, next: 1, needs: 2 });
  });

  test("drag and drop gives the backend the task to go before", () => {
    const next = ["a", "b", "c", "d"];
    expect(dropBefore(next, "d", "b", false)).toBe("b");
    expect(dropBefore(next, "d", "b", true)).toBe("c");
    expect(dropBefore(next, "a", "d", true)).toBe(null);
    expect(dropBefore(next, "a", "a", false)).toBe(undefined);
    expect(dropBefore(next, "a", "zz", false)).toBe(undefined);
    expect(moved(next, "d", "b")).toEqual(["a", "d", "b", "c"]);
    expect(moved(next, "a", null)).toEqual(["b", "c", "d", "a"]);
    // The same answer the drop gives, applied.
    expect(moved(next, "b", dropBefore(next, "b", "c", true)!)).toEqual(["a", "c", "b", "d"]);
  });
});
