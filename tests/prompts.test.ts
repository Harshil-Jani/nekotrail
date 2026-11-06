// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { activeRange, applyEdit, fuzzy, render, startFill, step, stops, unfilled, type Seg } from "../src/prompts";

const t = (text: string): Seg => ({ t: "text", text });
const f = (name: string, def: string | null = null): Seg => ({ t: "field", name, default: def, builtin: ["branch", "base", "files_changed", "selection"].includes(name) });

describe("the / menu match", () => {
  test("exact, prefix, word prefix, substring, letters in order", () => {
    expect(fuzzy("/fix-ticket", "fix-ticket")).toBe(0);
    expect(fuzzy("/fix-ticket", "fix")).toBe(1);
    expect(fuzzy("/fix-ticket", "tick")).toBe(2);
    expect(fuzzy("/release", "lea")).toBe(3);
    expect(fuzzy("/fix-ticket", "fxt")).toBe(4);
    expect(fuzzy("/tests", "zz")).toBe(-1);
    expect(fuzzy("/tests", "")).toBe(1);
    // The description only counts as a loose match.
    expect(fuzzy("/ship", "release", "Cut a release")).toBe(4);
  });
});

describe("filling blanks", () => {
  // "Fix {{ticket}} on {{branch}} in {{env:staging}}, see {{ticket}}"
  const segs = [t("Fix "), f("ticket"), t(" on "), f("branch"), t(" in "), f("env", "staging"), t(", see "), f("ticket")];

  test("built-ins the app knows are filled and skipped; the rest are tab-stops", () => {
    const { fill, text } = startFill(segs, { branch: "feat/x" });
    expect(text).toBe("Fix ticket on feat/x in staging, see ticket");
    expect(fill).not.toBeNull();
    expect(stops(fill!).map((i) => (segs[i] as { name: string }).name)).toEqual(["ticket", "env"]);
    expect(activeRange(fill!)).toEqual([4, 10]);
    expect(unfilled(fill!)).toEqual(["ticket"]); // env has a starting value
  });

  test("an unknown built-in is a tab-stop like any blank, and stays one once typed", () => {
    const { fill } = startFill([t("Base: "), f("base"), t(" "), f("x")]);
    const after = applyEdit(fill!, "Base: base x", "Base: main x")!;
    expect(render(after).text).toBe("Base: main x");
    expect(stops(after)).toEqual([1, 3]);
    expect(step(after, 1)!.at).toBe(1);
  });

  test("typing in a blank updates every place it appears", () => {
    const { fill, text } = startFill(segs, { branch: "b" });
    // Select-all of "ticket" replaced with "NEK-7".
    const next = applyEdit(fill!, text, text.replace("ticket", "NEK-7"))!;
    expect(next).not.toBeNull();
    expect(render(next).text).toBe("Fix NEK-7 on b in staging, see NEK-7");
    expect(unfilled(next)).toEqual([]);
    // Emptying a blank is allowed.
    const empty = applyEdit(next, render(next).text, "Fix  on b in staging, see NEK-7")!;
    expect(render(empty).text).toBe("Fix  on b in staging, see ");
  });

  test("typing outside the active blank ends filling", () => {
    const { fill, text } = startFill(segs, { branch: "b" });
    expect(applyEdit(fill!, text, "X" + text)).toBeNull();
    expect(applyEdit(fill!, text, text + "!")).toBeNull();
  });

  test("Tab past the last blank finishes; ⇧Tab stops at the first", () => {
    const { fill } = startFill(segs, { branch: "b" });
    const second = step(fill!, 1)!;
    expect(second.at).toBe(1);
    expect(activeRange(second)).toEqual([render(second).text.indexOf("staging"), render(second).text.indexOf("staging") + 7]);
    expect(step(second, 1)).toBeNull();
    expect(step(fill!, -1)!.at).toBe(0);
  });

  test("no blanks left to fill: plain text, no fill", () => {
    expect(startFill([t("Tests for "), f("files_changed")], { files_changed: "a.ts\nb.ts" })).toEqual({ fill: null, text: "Tests for a.ts\nb.ts" });
    expect(startFill([t("Just text")]).fill).toBeNull();
  });
});
