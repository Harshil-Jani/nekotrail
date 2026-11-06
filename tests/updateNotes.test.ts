// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { busyText, busyVerb, clip, highlights, isSnoozed, plainText, snooze, SNOOZE_MS } from "../src/updateNotes";

const memory = () => {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
};

describe("highlights", () => {
  test("takes top-level bullets as plain text and stops at the Install heading", () => {
    const notes = [
      "**Design pass** (from an outside design review)",
      "- Chat, Changes and **Terminal** moved into the `toolbar`.",
      "  - a nested detail that should not count",
      "- A calmer [PR bar](https://example.com): plain text.",
      "- Third",
      "- Fourth",
      "- Fifth",
      "",
      "### Install",
      "- Download the .dmg",
    ].join("\n");
    const h = highlights(notes);
    expect(h.items).toEqual(["Chat, Changes and Terminal moved into the toolbar.", "A calmer PR bar: plain text.", "Third", "Fourth"]);
    expect(h.more).toBe(1);
  });

  test("HTML in notes becomes plain text, never markup", () => {
    const h = highlights('- Fix <img src=x onerror="alert(1)"> the <b>menu</b> &amp; more');
    expect(h.items[0]).toBe("Fix the menu & more");
    expect(h.items[0]).not.toContain("<");
  });

  test("notes without bullets give the first paragraph", () => {
    expect(highlights("Small fixes.\n\nMore text later.").items).toEqual(["Small fixes."]);
    expect(highlights(undefined).items).toEqual([]);
  });

  test("underscores in names survive", () => {
    expect(plainText("Run db_migrate and NEKOTRAIL_PORT")).toBe("Run db_migrate and NEKOTRAIL_PORT");
  });

  test("long bullets are clipped at a word", () => {
    const c = clip("word ".repeat(60), 40);
    expect(c.length).toBeLessThanOrEqual(41);
    expect(c.endsWith("…")).toBe(true);
  });
});

describe("snooze", () => {
  test("snoozes that version for 24 hours only", () => {
    const s = memory();
    snooze("0.11.0", 1000, s);
    expect(isSnoozed("0.11.0", 1000 + SNOOZE_MS - 1, s)).toBe(true);
    expect(isSnoozed("0.11.0", 1000 + SNOOZE_MS, s)).toBe(false);
  });

  test("a newer version prompts again", () => {
    const s = memory();
    snooze("0.11.0", 1000, s);
    expect(isSnoozed("0.12.0", 2000, s)).toBe(false);
  });

  test("garbage in storage is not a snooze", () => {
    const s = memory();
    s.setItem("nekotrail.update.snooze", "{not json");
    expect(isSnoozed("0.11.0", 0, s)).toBe(false);
  });
});

test("busy text", () => {
  expect(busyText(2, 1)).toBe("2 agents and 1 terminal");
  expect(busyText(1, 0)).toBe("1 agent");
  expect(busyText(0, 3)).toBe("3 terminals");
  expect(busyText(0, 0)).toBe("");
  expect(busyVerb(1, 0)).toBe("is");
  expect(busyVerb(1, 1)).toBe("are");
});
