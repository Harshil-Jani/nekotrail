import fs from "node:fs";
import path from "node:path";

export const SITE = {
  name: "nekotrail",
  origin: "https://harshil-jani.github.io",
  repo: "https://github.com/Harshil-Jani/nekotrail",
  releases: "https://github.com/Harshil-Jani/nekotrail/releases/latest",
  allReleases: "https://github.com/Harshil-Jani/nekotrail/releases",
  issues: "https://github.com/Harshil-Jani/nekotrail/issues",
  githubSponsors: "https://github.com/sponsors/Harshil-Jani",
  buyMeACoffee: "https://buymeacoffee.com/harshiljani",
  author: "Harshil Jani",
  description:
    "Run Claude Code, Codex and 14 more coding agents in parallel on your Mac, each in its own git worktree and branch. Free, open source (MIT), local, no account.",
};

const BASE = import.meta.env.BASE_URL.replace(/\/?$/, "/");

/** A base-aware link to a page or file of this site: href("blog/") -> /nekotrail/blog/ */
export function href(p = ""): string {
  const clean = p.replace(/^\/+/, "");
  return BASE + clean;
}

/** The absolute URL of a page, for canonical links, Open Graph and the feed. */
export function absolute(p = ""): string {
  return SITE.origin + href(p);
}

/** The repo root (the site lives in site/ and reads CHANGELOG.md, docs and package.json from it). */
export function repoRoot(): string {
  let dir = process.cwd();
  for (let i = 0; i < 5; i++) {
    if (fs.existsSync(path.join(dir, "CHANGELOG.md")) && fs.existsSync(path.join(dir, "src-tauri"))) return dir;
    dir = path.dirname(dir);
  }
  throw new Error("Could not find the nekotrail repo root from " + process.cwd());
}

export function readRepoFile(rel: string): string {
  return fs.readFileSync(path.join(repoRoot(), rel), "utf8");
}

/** The app's version, from the repo's package.json. */
export function appVersion(): string {
  return JSON.parse(readRepoFile("package.json")).version as string;
}

export const AGENTS = [
  "Claude Code", "Codex", "Gemini CLI", "OpenCode", "Cursor", "GitHub Copilot", "Goose", "Qwen Code",
  "Kimi", "Auggie", "Grok", "Kiro", "Droid", "Mistral Vibe", "Cline", "Kilo",
];

export function formatDate(d: Date): string {
  return d.toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });
}
