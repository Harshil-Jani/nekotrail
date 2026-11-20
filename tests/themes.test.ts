// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { contrast, mix, parseTheme, themeFromRemote, PRESET_ORDER, PRESETS, readable, resolvePalette, terminalColors, themeJson, themeVars, xtermTheme, type AppTheme } from "../src/themes";

const solid = (v: string) => v.startsWith("#");

describe("every preset reads well", () => {
  for (const id of Object.keys(PRESETS)) {
    const t = PRESETS[id];
    const v = themeVars(t);
    test(`${t.name}: text, secondary text and status text pass WCAG AA on the background`, () => {
      expect(contrast(v["--text"], v["--bg"])).toBeGreaterThanOrEqual(7);
      expect(contrast(v["--text2"], v["--bg"])).toBeGreaterThanOrEqual(7);
      for (const k of ["--muted", "--accent-text", "--accent-chip-text", "--lantern-text", "--good-text", "--bad-text"]) {
        expect(contrast(v[k], v["--bg"])).toBeGreaterThanOrEqual(4.5);
      }
    });
    test(`${t.name}: button text reads on the accent, Merge and Delete fills`, () => {
      expect(contrast(v["--accent-ink"], v["--accent"])).toBeGreaterThanOrEqual(4.5);
      expect(contrast("#ffffff", v["--good-fill"])).toBeGreaterThanOrEqual(4.5);
      expect(contrast("#ffffff", v["--bad-fill"])).toBeGreaterThanOrEqual(4.5);
    });
    test(`${t.name}: a full terminal palette`, () => {
      const x = terminalColors(t);
      expect(x.ansi).toHaveLength(16);
      expect(x.ansi.every(solid)).toBe(true);
      expect(contrast(x.foreground, x.background)).toBeGreaterThanOrEqual(4.5);
    });
  }

  test("presets cover what Conductor ships, and more", () => {
    for (const id of ["dracula", "gruvbox-dark", "tokyo-night", "nord", "catppuccin-mocha"]) expect(PRESETS[id]).toBeDefined();
    expect(PRESET_ORDER.length).toBeGreaterThanOrEqual(12);
    expect(PRESET_ORDER.some((id) => PRESETS[id].mode === "light")).toBe(true);
  });

  test("dark presets come first in the picker", () => {
    const modes = PRESET_ORDER.map((id) => PRESETS[id].mode);
    expect(modes.indexOf("light")).toBeGreaterThan(modes.lastIndexOf("dark"));
  });
});

describe("colour helpers", () => {
  test("mix goes from a to b", () => {
    expect(mix("#000000", "#ffffff", 0)).toBe("#000000");
    expect(mix("#000000", "#ffffff", 1)).toBe("#ffffff");
    expect(mix("#000", "#fff", 0.5)).toBe("#808080");
  });
  test("readable pushes a weak colour until it passes, and leaves a good one alone", () => {
    expect(readable("#ffffff", "#000000")).toBe("#ffffff");
    const c = readable("#777777", "#808080", 4.5);
    expect(contrast(c, "#808080")).toBeGreaterThanOrEqual(4.5);
  });
});

describe("pasting a theme", () => {
  test("round-trips what Copy theme gives", () => {
    const r = parseTheme(themeJson(PRESETS.nord));
    expect("theme" in r && r.theme).toEqual(PRESETS.nord);
  });

  test("takes a bare colour map with common names, and guesses light or dark", () => {
    const r = parseTheme('{"bg":"#FFFFFF","fg":"#111","primary":"#0969da"}');
    expect("theme" in r).toBe(true);
    const t = (r as { theme: AppTheme }).theme;
    expect(t.mode).toBe("light");
    expect(t.colors).toEqual({ background: "#ffffff", text: "#111", accent: "#0969da" });
    expect(t.name).toBe("Custom");
  });

  test("accepts JSON inside a code fence, the way agents often reply", () => {
    expect("theme" in parseTheme('```json\n{"background":"#101010","text":"#eeeeee","accent":"#ff8800"}\n```')).toBe(true);
  });

  test("says what's wrong in plain words", () => {
    expect(parseTheme("hello")).toEqual({ error: "That isn't a theme. Paste the JSON that Copy theme gives you." });
    expect(parseTheme("[1]")).toEqual({ error: "A theme is a JSON object with colors." });
    expect(parseTheme('{"background":"#000","text":"#fff"}')).toEqual({ error: "The theme needs a accent colour." });
    expect(parseTheme('{"background":"red","text":"#fff","accent":"#fff"}')).toEqual({ error: "background must be a hex colour like #1e1e2e." });
  });

  test("never lets CSS through, only hex colours", () => {
    const r = parseTheme('{"background":"#000","text":"#fff","accent":"#fff; background:url(x)"}');
    expect("error" in r).toBe(true);
  });

  test("keeps a terminal palette only when it is complete", () => {
    const base = { background: "#101010", text: "#eeeeee", accent: "#ff8800" };
    const ok = parseTheme({ colors: base, terminal: { background: "#000000", ansi: Array(16).fill("#abcdef") } });
    expect("theme" in ok && ok.theme.terminal?.ansi).toHaveLength(16);
    const short = parseTheme({ colors: base, terminal: { ansi: ["#abcdef"] } });
    expect("theme" in short && short.theme.terminal?.ansi).toBeUndefined();
  });
});

describe("custom themes with few colours", () => {
  const minimal: AppTheme = { name: "Min", mode: "dark", colors: { background: "#202020", text: "#555555", accent: "#3355ff" } };
  test("the rest is worked out and still readable", () => {
    const v = themeVars(minimal);
    expect(contrast(v["--text"], v["--bg"])).toBeGreaterThanOrEqual(7);
    expect(contrast(v["--muted"], v["--bg"])).toBeGreaterThanOrEqual(4.5);
    expect(contrast(v["--accent-text"], v["--bg"])).toBeGreaterThanOrEqual(4.5);
    expect(Object.values(v).every((x) => typeof x === "string" && x.length > 0)).toBe(true);
  });
  test("a light theme with no terminal colours keeps the dark Moonlight terminal", () => {
    const light: AppTheme = { name: "L", mode: "light", colors: { background: "#ffffff", text: "#111111", accent: "#0969da" } };
    expect(terminalColors(light).background).toBe("#0b0b1e");
    expect(xtermTheme(light).brightWhite).toBe("#f5f5ff");
  });
});

describe("which theme is on", () => {
  test("empty is Moonlight, custom is yours, an id is a preset", () => {
    const mine: AppTheme = { name: "Mine", mode: "dark", colors: { background: "#000000", text: "#ffffff", accent: "#ff0000" } };
    expect(resolvePalette("", mine)).toBeNull();
    expect(resolvePalette("custom", mine)).toBe(mine);
    expect(resolvePalette("custom", null)).toBeNull();
    expect(resolvePalette("dracula", null)).toBe(PRESETS.dracula);
    expect(resolvePalette("gone", null)).toBeNull();
  });
});

describe("nekotrail theme (the command agents use)", () => {
  test("the Rust side knows the same presets, in the same order", () => {
    const rs = require("fs").readFileSync(require("path").join(import.meta.dir, "../src-tauri/src/theme.rs"), "utf8") as string;
    const ids = [...rs.matchAll(/\("([a-z-]+)", "([^"]+)"\)/g)].map((m) => [m[1], m[2]]);
    expect(ids).toEqual(Object.entries(PRESETS).map(([id, t]) => [id, t.name]));
  });

  test("a preset, moonlight, or a whole theme becomes settings", () => {
    expect(themeFromRemote("Nord")).toEqual({ palette: "nord", name: "Nord" });
    expect(themeFromRemote("moonlight")).toEqual({ palette: "", name: "Moonlight" });
    expect(themeFromRemote("nope")).toEqual({ error: 'There\'s no theme called "nope".' });
    const r = themeFromRemote({ name: "Mint", colors: { background: "#0f1419", text: "#ffffff", accent: "#5ccfe6" } });
    expect("palette" in r && r.palette).toBe("custom");
    expect("custom" in r && r.custom?.name).toBe("Mint");
    expect("error" in themeFromRemote({ colors: { background: "#000" } })).toBe(true);
  });
});
