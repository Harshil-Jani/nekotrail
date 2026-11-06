// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { canApply, keepConfirmed, stateLabel, statusLabel, toWrite } from "../src/rules";
import type { RulePlanItem, RulesPlan } from "../src/api";

const item = (path: string, over: Partial<RulePlanItem> = {}): RulePlanItem => ({
  path, how: "import", options: ["import", "copy", "symlink", "keep"], status: "create", needs_confirm: false, why: "", diff: [], new: "", exists: false, ...over,
});
const plan = (items: RulePlanItem[]): RulesPlan => ({ source: "AGENTS.md", items, notes: [], token: "t" });

describe("rules plan", () => {
  test("apply waits for every hand-written file to be confirmed", () => {
    const p = plan([item("CLAUDE.md"), item("GEMINI.md", { status: "replace", needs_confirm: true, exists: true })]);
    expect(canApply(p, [])).toBe(false);
    expect(canApply(p, ["GEMINI.md"])).toBe(true);
  });

  test("nothing to write means nothing to apply", () => {
    const p = plan([item("CLAUDE.md", { status: "same" }), item("GEMINI.md", { how: "keep", status: "keep" })]);
    expect(toWrite(p)).toEqual([]);
    expect(canApply(p, [])).toBe(false);
    expect(canApply(null, [])).toBe(false);
  });

  test("confirmations that no longer apply are dropped", () => {
    const p = plan([item("GEMINI.md", { status: "replace", needs_confirm: true, exists: true }), item("CLAUDE.md", { how: "keep", status: "keep", exists: true })]);
    expect(keepConfirmed(p, ["GEMINI.md", "CLAUDE.md", ".goosehints"])).toEqual(["GEMINI.md"]);
  });

  test("labels say what happens in plain words", () => {
    expect(statusLabel(item("CLAUDE.md", { how: "keep", status: "keep", exists: false }))).toBe("Not created");
    expect(statusLabel(item("CLAUDE.md", { status: "replace", needs_confirm: true }))).toBe("Replaces your text");
    expect(statusLabel(item("CLAUDE.md", { status: "same" }))).toBe("Already done");
    expect(stateLabel({ state: "points", points_to: "AGENTS.md", via: "link" }, "AGENTS.md").text).toBe("Links to AGENTS.md");
    expect(stateLabel({ state: "edited", points_to: null, via: null }, "AGENTS.md").tone).toBe("bad");
  });
});
