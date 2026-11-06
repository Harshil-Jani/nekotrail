// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { MAX_REPOS, MULTI_REPO_AGENTS, blocker, isLinked, mainOf, membersOf, mergeAllState, move, multiRepoNote, shortRef, togglePick, withMainFirst } from "../src/multiRepo";
import type { Pr, Workspace } from "../src/api";

const ws = (id: string, group_id: string, created_at = "2026-11-06T10:00:00Z", state: "active" | "archived" = "active") =>
  ({ id, group_id, created_at, state }) as Workspace;

const pr = (over: Partial<NonNullable<Pr>> = {}) =>
  ({ number: 1, url: "https://github.com/org/api/pull/1", state: "OPEN", isDraft: false, mergeable: "MERGEABLE", reviewDecision: "APPROVED",
    statusCheckRollup: [{ name: "ci", conclusion: "SUCCESS" }], ...over }) as unknown as Pr;

describe("multi-repo tasks", () => {
  test("only agents that can open more than one folder are offered, the same four as the backend", () => {
    expect(MULTI_REPO_AGENTS).toEqual(["claude", "codex", "gemini", "copilot"]);
    // The backend's own list (agent.rs, acp.rs) says the same.
    const rust = readFileSync(new URL("../src-tauri/src/multirepo.rs", import.meta.url), "utf8");
    expect(rust).toContain('assert_eq!(ids, ["claude", "codex", "copilot", "gemini"])');
    expect(multiRepoNote()).toBe("Claude, Codex, Gemini and Copilot can work across repos. The other agents only open one folder, so they aren't offered here.");
  });

  test("a task shows as its main workspace, the others in the order they were made", () => {
    const all = [ws("w9", ""), ws("t3", "t1", "2026-11-06T10:00:02Z"), ws("t2", "t1", "2026-11-06T10:00:01Z"), ws("t1", "t1", "2026-11-06T10:00:03Z"), ws("old", "t1", "2026-11-06T09:00:00Z", "archived")];
    expect(isLinked(all[1])).toBe(true);
    expect(isLinked(all[3])).toBe(false);
    expect(isLinked(all[0])).toBe(false);
    expect(mainOf(all[1], all).id).toBe("t1");
    expect(mainOf(all[0], all).id).toBe("w9");
    expect(membersOf(all[1], all).map((w) => w.id)).toEqual(["t1", "t2", "t3"]);
    expect(membersOf(all[0], all).map((w) => w.id)).toEqual(["w9"]);
  });

  test("picking repos keeps the order, caps at five, and the main one leads", () => {
    let p: string[] = [];
    for (const r of ["a", "b", "c", "d", "e", "f"]) p = togglePick(p, r);
    expect(p).toEqual(["a", "b", "c", "d", "e"]);
    expect(p.length).toBe(MAX_REPOS);
    expect(togglePick(p, "c")).toEqual(["a", "b", "d", "e"]);
    expect(withMainFirst(["a", "b", "c"], "c")).toEqual(["c", "a", "b"]);
    expect(withMainFirst(["a", "b"], "z")).toEqual(["a", "b"]);
  });

  test("merge order moves one step and stays in bounds", () => {
    expect(move(["a", "b", "c"], 2, -1)).toEqual(["a", "c", "b"]);
    expect(move(["a", "b", "c"], 0, -1)).toEqual(["a", "b", "c"]);
    expect(move(["a", "b", "c"], 2, 1)).toEqual(["a", "b", "c"]);
  });

  test("green means open, ready, passing and nothing requested, like the backend", () => {
    expect(blocker(pr())).toBeNull();
    expect(blocker(undefined)).toBe("no PR");
    expect(blocker(pr({ isDraft: true }))).toBe("draft");
    expect(blocker(pr({ mergeable: "CONFLICTING" }))).toBe("conflicts");
    expect(blocker(pr({ state: "MERGED" }))).toBe("merged");
    expect(blocker(pr({ reviewDecision: "CHANGES_REQUESTED" }))).toBe("changes requested");
    expect(blocker(pr({ statusCheckRollup: [{ name: "a", conclusion: "FAILURE" }, { name: "b", conclusion: "FAILURE" }] as never }))).toBe("2 failing checks");
    expect(blocker(pr({ statusCheckRollup: [{ name: "a", status: "IN_PROGRESS", conclusion: null }] as never }))).toBe("1 check running");
  });

  test("Merge all is ready only when every PR left is green", () => {
    const green = { name: "api", pr: pr() };
    expect(mergeAllState([green, { name: "web", pr: pr() }])).toEqual({ ready: true, waiting: [], left: 2 });
    expect(mergeAllState([green, { name: "web", pr: pr({ isDraft: true }) }, { name: "types", pr: undefined }])).toEqual({ ready: false, waiting: ["web: draft", "types: no PR"], left: 1 });
    // After a stop, the merged ones are skipped and the rest can go again.
    expect(mergeAllState([{ name: "types", pr: pr({ state: "MERGED" }) }, green])).toEqual({ ready: true, waiting: [], left: 1 });
    expect(mergeAllState([{ name: "types", pr: pr({ state: "MERGED" }) }]).ready).toBe(false);
  });

  test("PRs are named across repos the way GitHub links them", () => {
    expect(shortRef("https://github.com/org/api/pull/12", 12)).toBe("org/api#12");
    expect(shortRef("https://git.corp.example/team/web/pull/34", 34)).toBe("team/web#34");
    expect(shortRef("", 7)).toBe("#7");
  });
});
