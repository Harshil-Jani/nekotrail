// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { byRepo, noticeShown, outcomeLine, primaryLabel, sizeLabel, summary, tilde, totals, verdict, whatMoves } from "../src/conductorRescue";
import type { ImportReport, RescueItem, RescueOutcome } from "../src/api";

const item = (label: string, repo: string, kb: number, uncommitted: number, extra: Partial<RescueItem> = {}): RescueItem => ({
  id: "", repo, label, branch: `me/${label}`, from: `/Users/sam/conductor/workspaces/${repo}/${label}`, to: `/Users/sam/nekotrail/workspaces/${repo}/${label}`, kb, uncommitted, imported: false, ...extra,
});
const out = (id: string, extra: Partial<RescueOutcome> = {}): RescueOutcome => ({
  id, label: id, from: `/a/${id}`, to: `/b/${id}`, moved: true, error: null, problems: [], uncommitted: 0, ...extra,
});
const report = (repos: number, workspaces: number): ImportReport => ({ repos, workspaces, sessions: 4, skipped: 0, chats_with_history: 2 });

describe("what moves", () => {
  const items = [item("lome", "api", 2048, 3), item("tunis", "web", 900, 0), item("oslo", "api", 1024 * 1024 * 2, 1)];

  test("sizes read like du", () => {
    expect(sizeLabel(900)).toBe("900 KB");
    expect(sizeLabel(2048)).toBe("2.0 MB");
    expect(sizeLabel(1024 * 300)).toBe("300 MB");
    expect(sizeLabel(1024 * 1024 * 3)).toBe("3.0 GB");
  });

  test("groups by repo in order, with sums", () => {
    const g = byRepo(items);
    expect(g.map((x) => [x.repo, x.items.length, x.uncommitted])).toEqual([["api", 2, 4], ["web", 1, 0]]);
    expect(g[0].kb).toBe(2048 + 1024 * 1024 * 2);
  });

  test("totals and the one-line summary", () => {
    expect(totals(items)).toEqual({ repos: 2, worktrees: 3, kb: 2048 + 900 + 1024 * 1024 * 2, uncommitted: 4 });
    expect(whatMoves(items)).toBe("2 repos, 3 worktrees, 2.0 GB, 4 uncommitted files");
    expect(whatMoves([item("a", "x", 10, 0)])).toBe("1 repo, 1 worktree, 10 KB, no uncommitted files");
    expect(whatMoves([])).toBe("Nothing to move.");
  });

  test("home folder shows as ~", () => {
    expect(tilde("/Users/sam/conductor/workspaces/api/lome")).toBe("~/conductor/workspaces/api/lome");
    expect(tilde("/home/sam")).toBe("~");
    expect(tilde("/Volumes/code/x")).toBe("/Volumes/code/x");
  });
});

describe("the main button", () => {
  const items = [item("lome", "api", 1, 0), item("tunis", "api", 1, 0)];
  test("moving is named with the count", () => {
    expect(primaryLabel("move", report(1, 2), items)).toBe("Import and move 2 worktrees");
    expect(primaryLabel("move", report(0, 0), items)).toBe("Move 2 worktrees");
    expect(primaryLabel("move", null, items.slice(0, 1))).toBe("Move 1 worktree");
  });
  test("leaving them is import only, or not now", () => {
    expect(primaryLabel("leave", report(1, 2), items)).toBe("Import only");
    expect(primaryLabel("leave", null, items)).toBe("Not now");
  });
  test("repos with no worktrees just import", () => {
    expect(primaryLabel("move", report(2, 0), [])).toBe("Import");
  });
});

describe("after the move", () => {
  test("each outcome says what happened", () => {
    expect(verdict(out("a"))).toBe("ok");
    expect(outcomeLine(out("a", { uncommitted: 3 }))).toBe("Checked: same branch head, and all 3 uncommitted files are intact.");
    expect(outcomeLine(out("a", { uncommitted: 1 }))).toBe("Checked: same branch head, and the 1 uncommitted file is intact.");
    expect(outcomeLine(out("a"))).toBe("Checked: same branch head, no uncommitted files.");
    const failed = out("b", { moved: false, error: "Git has this worktree locked." });
    expect(verdict(failed)).toBe("failed");
    expect(outcomeLine(failed)).toBe("Not moved: Git has this worktree locked. It is still in Conductor's folder, untouched.");
    const differs = out("c", { problems: ["Uncommitted files differ: 2 before the move, 1 after."] });
    expect(verdict(differs)).toBe("differs");
    expect(outcomeLine(differs)).toBe("Moved. The check after it found a difference. Uncommitted files differ: 2 before the move, 1 after.");
  });

  test("the summary counts each kind, and names the ones to retry", () => {
    expect(summary([out("a"), out("b")])).toEqual({ text: "Moved and checked 2 worktrees.", bad: false, failed: [] });
    const s = summary([out("a"), out("b", { moved: false, error: "x" }), out("c", { problems: ["y"] })]);
    expect(s.text).toBe("Moved and checked 1 of 3. 1 moved with a difference to look at. 1 wasn't moved and is still in Conductor's folder, untouched.");
    expect(s.bad).toBe(true);
    expect(s.failed).toEqual(["b"]);
    expect(summary([]).text).toBe("Nothing needed moving.");
  });

  test("the notice shows until dismissed, and only with something to move", () => {
    expect(noticeShown(2, null)).toBe(true);
    expect(noticeShown(2, "dismissed")).toBe(false);
    expect(noticeShown(0, null)).toBe(false);
  });
});
