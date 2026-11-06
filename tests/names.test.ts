// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { displayName, nameInText, starName, subName, type Sky, type Workspace } from "../src/api";

const sky: Sky = {
  stars: [{ slug: "mochi", name: "Mochi", constellation: "felis", x: 0, y: 0 }],
  constellations: [{ slug: "felis", name: "Felis", story: "", lines: [] }],
};
const ws = (name: string, title = "", title_source: Workspace["title_source"] = title ? "auto" : ""): Workspace => ({
  id: "w", repo_id: "r", name, branch: "b", path: "/p", target_branch: "main", state: "active", pinned: false, unread: false,
  notes: "", imported_from: null, created_at: "", title, title_source, status: "", pair_id: "", section_id: "", parent_id: "",
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
});
