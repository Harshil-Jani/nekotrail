// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { clampZoom, fontStack, resolveDark, snapshotScale, stepZoom, TERM_STACK, ZOOM_LEVELS, ZOOM_MAX, ZOOM_MIN } from "../src/appearance";

describe("zoom", () => {
  test("stays between 80% and 150%", () => {
    expect(clampZoom(0.5)).toBe(ZOOM_MIN);
    expect(clampZoom(3)).toBe(ZOOM_MAX);
    expect(clampZoom(Number.NaN)).toBe(1);
    expect(clampZoom(1.25)).toBe(1.25);
  });

  test("⌘+ and ⌘- walk the same steps the pane offers, without float drift", () => {
    const seen = [ZOOM_MIN];
    let z = ZOOM_MIN;
    while (z < ZOOM_MAX) seen.push((z = stepZoom(z, 1)));
    expect(seen).toEqual(ZOOM_LEVELS);
    expect(stepZoom(ZOOM_MAX, 1)).toBe(ZOOM_MAX);
    expect(stepZoom(ZOOM_MIN, -1)).toBe(ZOOM_MIN);
    expect(stepZoom(1.25, 1)).toBe(1.4); // an odd level from an old build lands back on a step
  });
});

describe("annotate crop scale", () => {
  test("uses the measured root, so zoom is counted once", () => {
    // Retina window 1100 points wide: 2200 snapshot pixels.
    expect(snapshotScale(2200, 1100)).toBe(2); // 100%
    expect(snapshotScale(2200, 1100 / 1.25)).toBe(2.5); // web view zoomed to 125%: fewer, bigger CSS pixels
    expect(snapshotScale(2200, 0)).toBe(1);
  });
});

describe("fonts", () => {
  test("a chosen font goes first and falls back to the default stack", () => {
    expect(fontStack("", TERM_STACK)).toBe(TERM_STACK);
    expect(fontStack("JetBrains Mono", TERM_STACK)).toBe(`"JetBrains Mono", ${TERM_STACK}`);
    expect(fontStack('Bad"Name\\', TERM_STACK)).toBe(`"BadName", ${TERM_STACK}`);
  });
});

describe("theme", () => {
  test("follow system uses the system, the others override it", () => {
    expect(resolveDark("system", true)).toBe(true);
    expect(resolveDark("system", false)).toBe(false);
    expect(resolveDark("light", true)).toBe(false);
    expect(resolveDark("dark", false)).toBe(true);
  });
});
