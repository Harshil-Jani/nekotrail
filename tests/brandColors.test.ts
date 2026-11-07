import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { CAT } from "../src/components/Mascot";

// brand/palette.json is the source of truth. The app draws the cat and sets its theme by hand, so these
// tests keep the hand-written copies in step with it.
const ROOT = join(import.meta.dir, "..");
const palette = JSON.parse(readFileSync(join(ROOT, "brand/palette.json"), "utf8")) as {
  colors: { token: string; hex: string }[];
  themes: { dark: Record<string, string>; light: Record<string, string> };
};
const hex = (token: string) => palette.colors.find((c) => c.token === token)?.hex.toLowerCase();

describe("brand colours", () => {
  test("the app's cat uses the palette's mascot colours", () => {
    expect(CAT.fur).toBe(hex("cat-fur")!);
    expect(CAT.stripe).toBe(hex("cat-stripe")!);
    expect(CAT.ink).toBe(hex("cat-ink")!);
    expect(CAT.glint).toBe(hex("cat-glint")!);
    expect(CAT.blush).toBe(hex("blush")!);
    expect(CAT.nose).toBe(hex("cat-nose")!);
    expect(CAT.paw).toBe(hex("cat-paw")!);
    expect(CAT.tailStar).toBe(hex("cat-tail-star")!);
  });

  test("App.css carries the theme's text, primary and attention colours", () => {
    const css = readFileSync(join(ROOT, "src/App.css"), "utf8").toLowerCase();
    const { dark, light } = palette.themes;
    for (const k of ["text", "muted", "primary", "primary-ink", "accent", "accent-text", "success", "warning", "danger"]) {
      expect(css).toContain(dark[k].toLowerCase());
      expect(css).toContain(light[k].toLowerCase());
    }
  });

  test("no old cat orange is left in the app", () => {
    const css = readFileSync(join(ROOT, "src/App.css"), "utf8").toLowerCase();
    for (const old of ["#ff9a4d", "#e5762a", "#ffad6e", "#b35400", "#ffa863", "#f28b3c", "255, 154, 77"]) {
      expect(css).not.toContain(old);
    }
  });
});
