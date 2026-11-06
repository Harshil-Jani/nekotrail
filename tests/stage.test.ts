// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { changeNumbers, changeRange, hunkChanges, moveGroup, moveHunk, pickLine, planBlocker, protectedIn, subjectHint, wholeOnly } from "../src/stage";
import type { StageFile } from "../src/api";

type K = "hunk" | "add" | "del" | "ctx";
const L = (...kinds: K[]) => kinds.map((kind) => ({ kind }));

describe("commit composer", () => {
  test("the subject hint counts the first line only", () => {
    expect(subjectHint("").tone).toBe("empty");
    expect(subjectHint("fix: x\n\na very long body ".repeat(20))).toMatchObject({ tone: "ok", len: 6 });
    expect(subjectHint("x".repeat(60)).tone).toBe("long");
    expect(subjectHint("x".repeat(73))).toMatchObject({ tone: "over", text: "73 of 72. Shorten the first line" });
    expect(subjectHint("é".repeat(50)).tone).toBe("ok");
  });

  test("change lines are numbered the way the backend numbers them", () => {
    const lines = L("hunk", "ctx", "del", "add", "ctx", "hunk", "add", "add");
    expect(changeNumbers(lines)).toEqual([null, null, 0, 1, null, null, 2, 3]);
    expect(hunkChanges(lines, 0)).toEqual([0, 1]);
    expect(hunkChanges(lines, 5)).toEqual([2, 3]);
    expect(changeRange(lines, 7, 2)).toEqual([0, 1, 2, 3]);
  });

  test("clicking toggles a line, shift-click adds the range", () => {
    const lines = L("hunk", "del", "ctx", "add", "add");
    let sel = pickLine(new Set(), lines, 1, null, false);
    expect([...sel]).toEqual([0]);
    sel = pickLine(sel, lines, 4, 1, true);
    expect([...sel].sort()).toEqual([0, 1, 2]);
    sel = pickLine(sel, lines, 3, null, false);
    expect([...sel].sort()).toEqual([0, 2]);
    expect(pickLine(sel, lines, 2, null, false)).toBe(sel); // context lines aren't pickable
  });

  test("whole-only files and protected paths", () => {
    const f = (p: Partial<StageFile>): StageFile => ({ path: "a", status: "M", additions: 1, deletions: 0, old_path: null, binary: false, protected: false, ...p });
    expect(wholeOnly(f({ binary: true }))).toBe(true);
    expect(wholeOnly(f({ status: "U" }))).toBe(true);
    expect(wholeOnly(f({}))).toBe(false);
    expect(protectedIn([f({ path: ".env", protected: true }), f({})])).toEqual([".env"]);
  });

  test("split plans: move hunks and groups, and say what blocks committing", () => {
    const groups = [{ message: "feat: a", hunks: [0, 2] }, { message: "fix: b", hunks: [1] }];
    expect(moveHunk(groups, 1, 0)).toEqual([{ message: "feat: a", hunks: [0, 1, 2] }]);
    expect(moveHunk(groups, 0, 2)).toEqual([{ message: "feat: a", hunks: [2] }, { message: "fix: b", hunks: [1] }, { message: "", hunks: [0] }]);
    expect(moveGroup(groups, 0, 1).map((g) => g.message)).toEqual(["fix: b", "feat: a"]);
    expect(moveGroup(groups, 0, -1)).toBe(groups);
    expect(planBlocker(groups)).toBeNull();
    expect(planBlocker([...groups, { message: " ", hunks: [3] }])).toBe("Commit 3 needs a message.");
    expect(planBlocker([{ message: "x".repeat(80), hunks: [0] }])).toBe("Commit 1's first line is over 72 characters.");
    expect(planBlocker([])).toBe("There's nothing to commit.");
  });
});
