#!/usr/bin/env bun
/**
 * The NekoTrail brand kit, from one file.
 *
 * Every logo, icon and card is drawn from the geometry below and written to brand/, then copied to
 * every place that shows it: the app icons, the app's favicon, the site, the VS Code extension and the
 * GitHub social preview. Edit the geometry or the palette here, run the script, commit the output.
 *
 *   bun scripts/brand.ts
 *
 * Needs rsvg-convert (brew install librsvg). On macOS the Tauri icon set is rebuilt with `tauri icon`.
 */
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawnSync } from "node:child_process";

const ROOT = path.resolve(import.meta.dir, "..");
const BRAND = path.join(ROOT, "brand");

// ---------------------------------------------------------------------------------------------
// Palette. Names are what the brand kit page and brand/README.md show.
// ---------------------------------------------------------------------------------------------
type Swatch = { token: string; name: string; hex: string; use: string; group: "brand" | "mascot" };
export const PALETTE: Swatch[] = JSON.parse(fs.readFileSync(path.join(BRAND, "palette.json"), "utf8")).colors;
const byName = (n: string) => {
  const c = PALETTE.find((p) => p.name === n);
  if (!c) throw new Error(`brand/palette.json has no colour named ${n}`);
  return c.hex;
};
const C = new Proxy({} as Record<string, string>, { get: (_, k: string) => byName(k) });
const INK = C["Cat Ink"];
const NOSE = C.Nose;

// ---------------------------------------------------------------------------------------------
// The mark: the letter N drawn as a cat. Two stems are the cheeks, ears on top, the diagonal
// carries the nose. 120 x 120 grid, the drawing spans x 8..112 and y 10..100.
// ---------------------------------------------------------------------------------------------
const MARK = {
  stems: [
    "M22 40 L30 14 Q32 10 35 13 L46 32 L46 96 Q46 100 42 100 L26 100 Q22 100 22 96 Z",
    "M98 40 L90 14 Q88 10 85 13 L74 32 L74 96 Q74 100 78 100 L94 100 Q98 100 98 96 Z",
    "M38 30 L54 30 L82 100 L66 100 Z",
  ],
  ears: "M28 34 L32 22 L39 31 Z M92 34 L88 22 L81 31 Z",
  eyes: [[34, 54], [86, 54]] as [number, number][],
  nose: "M56 62 L64 62 L60 67 Z",
  mouth: "M60 67 q-2.5 4 -6 2.5 M60 67 q2.5 4 6 2.5",
  whiskers: ["M22 66 L8 63", "M22 72 L8 74", "M98 66 L112 63", "M98 72 L112 74"],
};

/** The full-colour mark as SVG children (no wrapper). `eyeClass` lets the site move the eyes. */
export function markColor(opts: { eyeClass?: string } = {}): string {
  const eye = ([x, y]: [number, number]) =>
    `<ellipse cx="${x}" cy="${y}" rx="5" ry="6.5" fill="${INK}"/><circle cx="${x + 2}" cy="${y - 2.5}" r="1.8" fill="#fff"/>`;
  const eyes = MARK.eyes.map(eye).join("");
  return [
    `<g fill="${C["Cat Orange"]}">${MARK.stems.map((d) => `<path d="${d}"/>`).join("")}</g>`,
    `<path d="${MARK.ears}" fill="${C.Blush}" opacity="0.6"/>`,
    opts.eyeClass ? `<g class="${opts.eyeClass}">${eyes}</g>` : eyes,
    `<path d="${MARK.nose}" fill="${NOSE}" stroke="${NOSE}" stroke-width="1.5" stroke-linejoin="round"/>`,
    `<path d="${MARK.mouth}" stroke="${INK}" stroke-width="1.8" stroke-linecap="round" fill="none"/>`,
    `<g stroke="${C.Marmalade}" stroke-width="2.2" stroke-linecap="round">${MARK.whiskers.map((d) => `<path d="${d}"/>`).join("")}</g>`,
  ].join("");
}

/** The one-colour mark: the face is cut out of the fur, so it reads on any background. */
function markMono(fill: string, id: string): string {
  const cut = [
    ...MARK.eyes.map(([x, y]) => `<ellipse cx="${x}" cy="${y}" rx="5" ry="6.5"/>`),
    `<path d="${MARK.ears}"/>`,
    `<path d="${MARK.nose}" fill="#000" stroke="#000" stroke-width="1.5" stroke-linejoin="round"/>`,
    `<path d="${MARK.mouth}" stroke="#000" stroke-width="1.8" stroke-linecap="round" fill="none"/>`,
  ].join("");
  return [
    `<defs><mask id="${id}" maskUnits="userSpaceOnUse" x="0" y="0" width="120" height="120"><rect width="120" height="120" fill="#fff"/><g fill="#000">${cut}</g></mask></defs>`,
    `<g fill="${fill}" mask="url(#${id})">${MARK.stems.map((d) => `<path d="${d}"/>`).join("")}</g>`,
    `<g stroke="${fill}" stroke-width="2.2" stroke-linecap="round">${MARK.whiskers.map((d) => `<path d="${d}"/>`).join("")}</g>`,
  ].join("");
}

// ---------------------------------------------------------------------------------------------
// The wordmark: NekoTrail in one monoline stroke with round ends, built only from straight lines
// and circles, so every letter is a constellation of points and lines. The i is dotted with a star.
// Cap height 100, x-height 70, stroke 15. Coordinates are stroke centrelines.
// ---------------------------------------------------------------------------------------------
export const WORDMARK = {
  stroke: 15,
  viewBox: "-8 -16 641 124",
  width: 641,
  height: 124,
  /** One entry per letter, in reading order. Circles are written as two arcs so they draw as paths. */
  letters: [
    { ch: "N", d: "M0 100V0L64 100V0" },
    { ch: "e", d: "M95 65H165A35 35 0 1 0 152.5 91.8" },
    { ch: "k", d: "M196 0V100M236 30L196 70M211 55L238 100" },
    { ch: "o", d: "M298 30A35 35 0 1 1 298 100A35 35 0 1 1 298 30" },
    { ch: "T", d: "M366 0H436M401 0V100" },
    { ch: "r", d: "M434 100V30M434 64C434 44 448 30 468 30" },
    { ch: "a", d: "M528 30A35 35 0 1 1 528 100A35 35 0 1 1 528 30M563 30V100" },
    { ch: "i", d: "M594 30V100" },
    { ch: "l", d: "M625 0V100" },
  ],
  star: { cx: 594, cy: 0, r: 15 },
};

function sparkle(cx: number, cy: number, r: number): string {
  const k = r * 0.16;
  return `M${cx} ${cy - r}Q${cx + k} ${cy - k} ${cx + r} ${cy}Q${cx + k} ${cy + k} ${cx} ${cy + r}Q${cx - k} ${cy + k} ${cx - r} ${cy}Q${cx - k} ${cy - k} ${cx} ${cy - r}Z`;
}
export const STAR_PATH = sparkle(WORDMARK.star.cx, WORDMARK.star.cy, WORDMARK.star.r);

function wordmarkBody(ink: string, star = C["Cat Orange"]): string {
  return [
    `<g fill="none" stroke="${ink}" stroke-width="${WORDMARK.stroke}" stroke-linecap="round" stroke-linejoin="round">`,
    WORDMARK.letters.map((l) => `<path d="${l.d}"/>`).join(""),
    `</g><path d="${STAR_PATH}" fill="${star}"/>`,
  ].join("");
}

// ---------------------------------------------------------------------------------------------
// The app icon: the mark on a night-sky squircle, on the macOS icon grid (824 body, 100 margin).
// ---------------------------------------------------------------------------------------------
function appIcon(): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${C.Dusk}"/><stop offset="0.6" stop-color="${C.Night}"/><stop offset="1" stop-color="${C.Midnight}"/></linearGradient>
    <radialGradient id="glow" cx="0.5" cy="0.46" r="0.5"><stop offset="0" stop-color="${C["Cat Orange"]}" stop-opacity="0.2"/><stop offset="1" stop-color="${C["Cat Orange"]}" stop-opacity="0"/></radialGradient>
    <filter id="sh" x="-10%" y="-10%" width="120%" height="125%"><feDropShadow dx="0" dy="10" stdDeviation="14" flood-color="#000" flood-opacity="0.35"/></filter>
  </defs>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#bg)" filter="url(#sh)"/>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#glow)"/>
  <rect x="101.5" y="101.5" width="821" height="821" rx="183.5" fill="none" stroke="#fff" stroke-opacity="0.09" stroke-width="3"/>
  <g fill="#fff"><circle cx="236" cy="252" r="5" opacity="0.55"/><circle cx="800" cy="226" r="4" opacity="0.4"/><circle cx="818" cy="772" r="5" opacity="0.45"/><circle cx="214" cy="736" r="3.5" opacity="0.3"/></g>
  <path d="${sparkle(770, 330, 22)}" fill="${C.Starlight}" opacity="0.85"/>
  <g transform="translate(212 206) scale(5)">${markColor()}</g>
</svg>`;
}

// ---------------------------------------------------------------------------------------------
// Social cards: GitHub social preview (1280 x 640) and Open Graph (1200 x 630). A night sky,
// The Loaf in the corner, the lockup and the one-line pitch.
// ---------------------------------------------------------------------------------------------
function seeded(seed: number) {
  let s = seed;
  return () => (s = (s * 16807) % 2147483647) / 2147483647;
}
const LOAF = { stars: [[90, 120], [130, 70], [175, 115], [270, 120], [270, 210], [90, 210]], lines: [[1, 0], [1, 2], [2, 3], [3, 4], [4, 5], [5, 0]] };

function card(w: number, h: number): string {
  const rand = seeded(w + h);
  const stars = Array.from({ length: Math.round((w * h) / 5200) }, () => {
    const r = rand();
    return `<circle cx="${(rand() * w).toFixed(1)}" cy="${(rand() * h).toFixed(1)}" r="${r > 0.93 ? 1.8 : r > 0.7 ? 1.2 : 0.8}" opacity="${(0.2 + rand() * 0.55).toFixed(2)}"/>`;
  }).join("");
  // The Loaf, top right, as it looks once every star in it has shipped.
  const lx = w - 330, ly = 36, ls = 0.95;
  const P = LOAF.stars.map(([x, y]) => [lx + x * ls, ly + y * ls]);
  const loaf = `<g stroke="${C["Cat Orange"]}" stroke-opacity="0.55" stroke-width="2">${LOAF.lines.map(([a, b]) => `<line x1="${P[a][0]}" y1="${P[a][1]}" x2="${P[b][0]}" y2="${P[b][1]}"/>`).join("")}</g>`
    + `<g fill="${C["Cat Orange"]}">${P.map(([x, y]) => `<circle cx="${x}" cy="${y}" r="5"/><circle cx="${x}" cy="${y}" r="12" opacity="0.18"/>`).join("")}</g>`;
  // The lockup in white, then the pitch.
  const m = Math.round(w * 0.075);
  const s = (w * 0.6) / LOCK_VB[2];
  const top = h * 0.3;
  const pitchY = top + LOCK_VB[3] * s + 70 * (h / 640);
  const fs1 = Math.round(34 * (w / 1280));
  const fs2 = Math.round(24 * (w / 1280));
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}">
  <defs><radialGradient id="sky" cx="0.5" cy="0" r="1.15"><stop offset="0" stop-color="${C.Dusk}"/><stop offset="0.55" stop-color="${C.Night}"/><stop offset="1" stop-color="${C.Midnight}"/></radialGradient></defs>
  <rect width="${w}" height="${h}" fill="url(#sky)"/>
  <g fill="#fff">${stars}</g>
  ${loaf}
  <g transform="translate(${m} ${top}) scale(${s}) translate(${-LOCK_VB[0]} ${-LOCK_VB[1]})">${lockupBody(C.Paper)}</g>
  <text x="${m}" y="${pitchY}" font-family="System Font, SF Pro Display, Helvetica Neue, sans-serif" font-weight="600" font-size="${fs1}" fill="${C.Paper}" letter-spacing="-0.3">Run Claude Code, Codex and 14 more coding agents at once.</text>
  <text x="${m}" y="${pitchY + fs1 * 1.5}" font-family="System Font, SF Pro Text, Helvetica Neue, sans-serif" font-size="${fs2}" fill="${C.Paper}" fill-opacity="0.66">Each task in its own git worktree. Free, open source, local. macOS.</text>
</svg>`;
}

// ---------------------------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------------------------
const svg = (viewBox: string, body: string, label: string, size?: [number, number]) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}"${size ? ` width="${size[0]}" height="${size[1]}"` : ""} role="img" aria-label="${label}">${body}</svg>\n`;

const MARK_VB = "4 4 112 100"; // the drawing plus a little air, so the mark sits centred
const mark = {
  color: svg(MARK_VB, markColor(), "NekoTrail"),
  dark: svg(MARK_VB, markMono(C.Night, "nt-cut-d"), "NekoTrail"),
  light: svg(MARK_VB, markMono(C.Paper, "nt-cut-l"), "NekoTrail"),
};
const wordmark = {
  dark: svg(WORDMARK.viewBox, wordmarkBody(C.Night), "NekoTrail"),
  light: svg(WORDMARK.viewBox, wordmarkBody(C.Paper), "NekoTrail"),
};
// Lockup: the mark at 1.15x the cap height, sitting on the baseline, a stroke and a half from the N.
const LOCK = { sc: 1.47, gap: 30 };
const LOCK_WM_X = (112 - 8) * LOCK.sc + LOCK.gap + 7.5;
const LOCK_VB = [-4, -28, LOCK_WM_X + 632.5 + 8, 139] as const;
function lockupBody(ink: string): string {
  const { sc } = LOCK;
  return `<g transform="translate(${-8 * sc} ${107.5 - 100 * sc}) scale(${sc})">${markColor()}</g><g transform="translate(${LOCK_WM_X} 0)">${wordmarkBody(ink)}</g>`;
}
const lockup = (ink: string) => svg(LOCK_VB.join(" "), lockupBody(ink), "NekoTrail");

function rsvg(svgFile: string, pngFile: string, w: number, h?: number) {
  const args = ["-w", String(w), ...(h ? ["-h", String(h)] : []), "-a", svgFile, "-o", pngFile];
  const r = spawnSync("rsvg-convert", args, { cwd: ROOT, stdio: "inherit" });
  if (r.status !== 0) throw new Error(`rsvg-convert failed for ${svgFile}`);
}

function write(rel: string, content: string | Buffer) {
  const file = path.join(ROOT, rel);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, content);
}

function main() {
  const files: [string, string][] = [
    ["brand/mark/nekotrail-mark.svg", mark.color],
    ["brand/mark/nekotrail-mark-dark.svg", mark.dark],
    ["brand/mark/nekotrail-mark-light.svg", mark.light],
    ["brand/wordmark/nekotrail-wordmark-dark.svg", wordmark.dark],
    ["brand/wordmark/nekotrail-wordmark-light.svg", wordmark.light],
    ["brand/lockup/nekotrail-lockup-dark.svg", lockup(C.Night)],
    ["brand/lockup/nekotrail-lockup-light.svg", lockup(C.Paper)],
    ["brand/icon/nekotrail-icon.svg", appIcon()],
  ];
  for (const [rel, body] of files) write(rel, body);

  // PNGs at a useful size for slides and docs.
  rsvg("brand/mark/nekotrail-mark.svg", "brand/mark/nekotrail-mark.png", 1024);
  rsvg("brand/mark/nekotrail-mark-dark.svg", "brand/mark/nekotrail-mark-dark.png", 1024);
  rsvg("brand/mark/nekotrail-mark-light.svg", "brand/mark/nekotrail-mark-light.png", 1024);
  rsvg("brand/wordmark/nekotrail-wordmark-dark.svg", "brand/wordmark/nekotrail-wordmark-dark.png", 2000);
  rsvg("brand/wordmark/nekotrail-wordmark-light.svg", "brand/wordmark/nekotrail-wordmark-light.png", 2000);
  rsvg("brand/lockup/nekotrail-lockup-dark.svg", "brand/lockup/nekotrail-lockup-dark.png", 2400);
  rsvg("brand/lockup/nekotrail-lockup-light.svg", "brand/lockup/nekotrail-lockup-light.png", 2400);
  rsvg("brand/icon/nekotrail-icon.svg", "brand/icon/nekotrail-icon-1024.png", 1024);
  rsvg("brand/icon/nekotrail-icon.svg", "brand/icon/nekotrail-icon-512.png", 512);

  fs.mkdirSync(path.join(BRAND, "mascot"), { recursive: true });
  // Mascot: copied from the site set (drawn by src/components/Mascot.tsx).
  for (const m of ["idle", "happy"]) {
    fs.copyFileSync(path.join(ROOT, `public/mascot-${m}.svg`), path.join(BRAND, `mascot/nekotrail-cat-${m}.svg`));
    rsvg(`brand/mascot/nekotrail-cat-${m}.svg`, `brand/mascot/nekotrail-cat-${m}.png`, 1024);
  }

  // Social cards.
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "nekotrail-brand-"));
  fs.writeFileSync(path.join(tmp, "social.svg"), card(1280, 640));
  fs.writeFileSync(path.join(tmp, "og.svg"), card(1200, 630));
  rsvg(path.join(tmp, "social.svg"), "brand/social-preview.png", 1280, 640);
  rsvg(path.join(tmp, "og.svg"), "site/public/og.png", 1200, 630);

  // Palette and wordmark geometry, for the site (brand kit page and the hero reveal).
  // Site tokens: every brand colour as a custom property (global.css builds on these).
  write("site/src/styles/tokens.css", [
    "/* Generated by scripts/brand.ts from brand/palette.json. Do not edit by hand. */",
    ":root {",
    ...PALETTE.map((p) => `  --brand-${p.token}: ${p.hex}; /* ${p.name} */`),
    "}",
    "",
  ].join("\n"));
  write("brand/wordmark/geometry.json", JSON.stringify({ ...WORDMARK, starPath: STAR_PATH }, null, 2) + "\n");

  // ---- derived copies --------------------------------------------------------------------
  // Site
  write("site/public/favicon.svg", mark.color);
  write("site/public/brand/n-cat.svg", mark.color);
  write("site/public/brand/icon-macos.svg", fs.readFileSync(path.join(BRAND, "icon/nekotrail-icon.svg")));
  rsvg("brand/mark/nekotrail-mark.svg", "site/public/favicon-32.png", 32, 32);
  rsvg("brand/icon/nekotrail-icon.svg", "site/public/apple-touch-icon.png", 180, 180);
  // App (Vite public folder: the favicon of the web preview)
  write("public/favicon.svg", mark.color);
  // VS Code extension: marketplace icon and the one-colour activity bar icon
  rsvg("brand/icon/nekotrail-icon.svg", "extensions/vscode/media/icon.png", 128, 128);
  write("extensions/vscode/media/activity.svg", svg(MARK_VB, markMono("#C5C5C5", "nt-cut-a") /* VS Code recolours activity icons; grey is its convention */, "NekoTrail", [24, 24]));

  // Tauri icon set (icns, ico and the PNG sizes) from the 1024 icon.
  const out = path.join(tmp, "tauri");
  const t = spawnSync("bunx", ["tauri", "icon", path.join(BRAND, "icon/nekotrail-icon-1024.png"), "-o", out], { cwd: ROOT, stdio: "inherit" });
  if (t.status === 0) {
    for (const f of fs.readdirSync(path.join(ROOT, "src-tauri/icons"))) {
      const src = path.join(out, f);
      if (fs.existsSync(src) && fs.statSync(src).isFile()) fs.copyFileSync(src, path.join(ROOT, "src-tauri/icons", f));
    }
  } else console.warn("tauri icon failed; src-tauri/icons left as they were");

  fs.rmSync(tmp, { recursive: true, force: true });
  console.log("Brand kit written to brand/ and copied to the app, site and extension.");
}

if (import.meta.main) main();
