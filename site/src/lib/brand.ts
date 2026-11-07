// The brand kit, read from the repo's brand/ folder at build time (scripts/brand.ts writes it).
// The site never keeps its own copy of the palette or the wordmark geometry.
import fs from "node:fs";
import path from "node:path";
import { repoRoot } from "./site";

export type Swatch = { token: string; name: string; hex: string; use: string; group: "brand" | "mascot" };
export type Wordmark = {
  stroke: number;
  viewBox: string;
  width: number;
  height: number;
  letters: { ch: string; d: string }[];
  star: { cx: number; cy: number; r: number };
  starPath: string;
};

const brandDir = () => path.join(repoRoot(), "brand");

export function palette(): Swatch[] {
  return JSON.parse(fs.readFileSync(path.join(brandDir(), "palette.json"), "utf8")).colors;
}

export function wordmark(): Wordmark {
  return JSON.parse(fs.readFileSync(path.join(brandDir(), "wordmark/geometry.json"), "utf8"));
}

/** Every file in brand/, as paths relative to it (e.g. "mark/nekotrail-mark.svg"). */
export function kitFiles(): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      if (e.name.startsWith(".")) continue;
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else out.push(path.relative(brandDir(), p).split(path.sep).join("/"));
    }
  };
  walk(brandDir());
  return out.sort();
}

export function kitFile(rel: string): Buffer {
  return fs.readFileSync(path.join(brandDir(), rel));
}

/** Width and height of a PNG, from its header. */
export function pngSize(rel: string): [number, number] {
  const b = kitFile(rel);
  return [b.readUInt32BE(16), b.readUInt32BE(20)];
}

/**
 * An SVG from the repo (the mark or a mascot), ready to inline: no fixed size, decorative unless
 * labelled, and its eyes tagged so the site's eye script can make them follow the pointer and blink.
 * `reach` is how far, in the drawing's own units, the eyes may travel (x, y).
 */
export function inlineCat(rel: string, opts: { cls: string; reach: [number, number]; label?: string }): string {
  let s = fs.readFileSync(path.join(repoRoot(), rel), "utf8").trim();
  s = s.replace(/<svg ([^>]*)>/, (_m, attrs: string) => {
    const keep = attrs.replace(/\s?(width|height|role|aria-label|class)="[^"]*"/g, "").trim();
    const a11y = opts.label ? `role="img" aria-label="${opts.label}"` : `aria-hidden="true" focusable="false"`;
    return `<svg ${keep} class="${opts.cls}" ${a11y} data-cat data-reach="${opts.reach.join(",")}">`;
  });
  // Eye balls are the dark ellipses; glints are the small white circles on them.
  const ink = palette().find((c) => c.token === "cat-ink")?.hex ?? "";
  s = s.replace(new RegExp(`<ellipse ([^>]*fill="${ink}"[^>]*)>`, "gi"), '<ellipse class="eye-ball" $1>');
  s = s.replace(/<circle ([^>]*r="(?:1\.8|2\.4)"[^>]*fill="#fff(?:fff)?"[^>]*)>/gi, '<circle class="eye-glint" $1>');
  return s;
}

export function fileSize(rel: string): string {
  const n = kitFile(rel).length;
  return n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${Math.round(n / 1024)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`;
}
