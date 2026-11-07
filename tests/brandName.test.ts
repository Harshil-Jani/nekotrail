import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

// The product is written NekoTrail wherever a person reads it. Lowercase `nekotrail` is only for
// identifiers: the terminal command (`nekotrail new ...`), nekotrail.json, nekotrail:// links,
// storage keys and events (nekotrail.x, nekotrail-x), paths and the process name.
const ROOT = join(import.meta.dir, "..", "src");

/** A lowercase nekotrail that reads as the product name, not as an identifier. */
const PROSE = /(?<![\w./\-@$\\'])nekotrail(?![\w\-/_:@]|\.\w| (?:\.|new|--|queue|handoff|ls|open|status|<))/;

/** Lines that keep the lowercase name on purpose, with the reason. */
const KEPT: [file: string, snippet: string, why: string][] = [
  ["preview.tsx", `{ name: "nekotrail", mb: 208 }`, "a process name in mock data, as ps shows it"],
];

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? files(p) : /\.(ts|tsx)$/.test(n) ? [p] : [];
  });
}

describe("brand name", () => {
  test("user-facing copy in src/ says NekoTrail, not nekotrail", () => {
    const found: string[] = [];
    for (const f of files(ROOT)) {
      readFileSync(f, "utf8").split("\n").forEach((line, i) => {
        const t = line.trim();
        if (t.startsWith("//") || t.startsWith("*") || t.startsWith("/*")) return;
        if (!PROSE.test(line)) return;
        if (KEPT.some(([file, snippet]) => f.endsWith(file) && line.includes(snippet))) return;
        found.push(`${f.slice(ROOT.length + 1)}:${i + 1}: ${t.slice(0, 120)}`);
      });
    }
    expect(found).toEqual([]);
  });

  test("the rule tells the product name from identifiers", () => {
    for (const s of ["Welcome to nekotrail.", "nekotrail can't read it", "in nekotrail's folder", "Made with nekotrail", "`nekotrail ${v} is here`"]) expect(PROSE.test(s)).toBe(true);
    for (const s of ["nekotrail.json", "nekotrail://open", "\"nekotrail.zoom\"", "nekotrail-theme", "nekotrail new \"fix it\"", "nekotrail --help", "nekotrail queue add", "/usr/local/bin/nekotrail", "dev.nekotrail.app", "Made with NekoTrail"]) expect(PROSE.test(s)).toBe(false);
  });
});
