// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { displayName, isNoiseTitle, nameInText, starName, subName, type Sky, type Workspace } from "../src/api";

const sky: Sky = {
  stars: [{ slug: "mochi", name: "Mochi", constellation: "felis", x: 0, y: 0 }],
  constellations: [{ slug: "felis", name: "Felis", story: "", lines: [] }],
};
const ws = (name: string, title = "", title_source: Workspace["title_source"] = title ? "auto" : ""): Workspace => ({
  id: "w", repo_id: "r", name, branch: "b", path: "/p", target_branch: "main", state: "active", pinned: false, unread: false,
  notes: "", imported_from: null, created_at: "", title, title_source, status: "", pair_id: "", section_id: "", parent_id: "", review_pr: 0,
});

describe("workspace names", () => {
  test("a titled workspace shows its task, with the star underneath", () => {
    const w = ws("mochi", "Fix 502s after deploy");
    expect(displayName(sky, w)).toBe("Fix 502s after deploy");
    expect(subName(sky, w)).toBe("Mochi");
    expect(nameInText(sky, w)).toBe("“Fix 502s after deploy”");
  });
  test("an untitled one is its star, with no second line and no quotes", () => {
    const w = ws("mochi");
    expect(displayName(sky, w)).toBe("Mochi");
    expect(subName(sky, w)).toBe("");
    expect(nameInText(sky, w)).toBe("Mochi");
  });
  test("a second workspace on the same star is numbered; imported ones keep their name", () => {
    expect(starName(sky, ws("mochi-2"))).toBe("Mochi 2");
    expect(subName(sky, ws("mochi-2", "Add dark mode"))).toBe("Mochi 2");
    expect(displayName(sky, ws("tokyo"))).toBe("tokyo");
  });
  test("a derived title that is only a screenshot's name or a timestamp gives way to the branch", () => {
    const w = { ...ws("mochi", "2026-11-05 at 5.42.16 PM.png"), branch: "harshil/sidebar-logo" };
    expect(displayName(sky, w)).toBe("sidebar-logo");
    expect(subName(sky, w)).toBe("");
    expect(nameInText(sky, w)).toBe("sidebar-logo");
    // A branch still named after the star: the star.
    expect(displayName(sky, { ...ws("mochi", "image.png"), branch: "harshil/mochi" })).toBe("Mochi");
    // Typed by hand: shown as typed.
    expect(displayName(sky, ws("mochi", "2026-11-05", "user"))).toBe("2026-11-05");
  });
  test("noise is only dates, times and screenshot words", () => {
    expect(isNoiseTitle("2026-11-05 at 5.42.16…")).toBe(true);
    expect(isNoiseTitle("image.png")).toBe(true);
    expect(isNoiseTitle("Screenshot 2026-11-05")).toBe(true);
    expect(isNoiseTitle("PR Issues")).toBe(false);
    expect(isNoiseTitle("Fix 502s after deploy")).toBe(false);
    expect(isNoiseTitle("ログインのバグを直して")).toBe(false);
  });
});
