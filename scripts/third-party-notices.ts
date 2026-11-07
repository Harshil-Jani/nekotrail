// Third-party licences: every component that ships inside the app, with its license.
//   bun scripts/third-party-notices.ts           writes both files below
//   bun scripts/third-party-notices.ts --check   fails if THIRD_PARTY_NOTICES.md is out of date (CI)
// Outputs:
//   THIRD_PARTY_NOTICES.md                         the summary table, committed to the repo
//   src-tauri/resources/THIRD_PARTY_LICENSES.txt   the full licence texts, bundled into the app as a
//                                                  resource and shown in Settings, About. Generated at
//                                                  build time (tauri.conf.json beforeBuildCommand), not committed.
// Rust: normal (not build or dev) dependencies for the macOS and Linux release targets, without proc macros,
// which only run at compile time. JavaScript: the closure of package.json "dependencies" (what Vite bundles),
// read from node_modules, so run `bun install` first.
// Full texts come from each package's own LICENSE, COPYING, NOTICE and similar files. A package that ships
// none gets the standard text of its license from scripts/licenses/, filled in with the authors it lists.
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const root = join(import.meta.dir, "..");
const TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu"];
const NOTICES = join(root, "THIRD_PARTY_NOTICES.md");
const LICENSES = join(root, "src-tauri", "resources", "THIRD_PARTY_LICENSES.txt");
const LICENSE_FILE = /^(licen[cs]e|copying|notice|copyright|unlicense)([-._ ].*)?$/i;
const TEMPLATES = ["MIT", "Apache-2.0", "MPL-2.0", "BSD-3-Clause"];
// Licence policy for bundled JavaScript (the Rust side is enforced by src-tauri/deny.toml): every package
// must offer at least one of these. No GPL or AGPL. EPL-2.0 is for elkjs, which we take under EPL-2.0.
const JS_ALLOWED = ["MIT", "MIT-0", "ISC", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "0BSD", "Zlib", "Unlicense", "CC0-1.0", "MPL-2.0", "EPL-2.0"];

type Component = { kind: "rust" | "js"; name: string; version: string; license: string; dir: string | null; authors: string[] };

function run(cmd: string[], cwd: string): string {
  const r = Bun.spawnSync(cmd, { cwd });
  if (r.exitCode !== 0) throw new Error(`${cmd.join(" ")} failed: ${r.stderr.toString()}`);
  return r.stdout.toString();
}

function rustCrates(): Map<string, Component> {
  const tauri = join(root, "src-tauri");
  // Where each crate's source is, and who wrote it. `cargo metadata` downloads any crate not yet fetched.
  type MetaPkg = { name: string; version: string; manifest_path: string; authors: string[] };
  const meta = JSON.parse(run(["cargo", "metadata", "--format-version", "1", "--locked"], tauri)) as { packages: MetaPkg[] };
  const where = new Map(meta.packages.map((p) => [`${p.name} ${p.version}`, p]));
  const out = new Map<string, Component>();
  for (const target of TARGETS) {
    const tree = run(["cargo", "tree", "--color", "never", "-e", "normal", "--prefix", "none", "--format", "{p}|{l}", "--target", target], tauri);
    for (const line of tree.split("\n")) {
      if (!line || line.includes("(proc-macro)")) continue;
      // CI sets CARGO_TERM_COLOR; strip any colour codes before reading the line.
      const [p, l] = line.replace(/\x1b\[[0-9;]*m/g, "").replace(" (*)", "").split("|");
      const [name, v] = p.trim().split(" ");
      if (name === "nekotrail") continue;
      const version = v.replace(/^v/, "");
      const m = where.get(`${name} ${version}`);
      out.set(`${name} ${version}`, { kind: "rust", name, version, license: l?.trim() || "UNKNOWN", dir: m ? dirname(m.manifest_path) : null, authors: m?.authors ?? [] });
    }
  }
  return out;
}

type Pkg = {
  name: string; version: string; license?: unknown; licenses?: unknown; author?: unknown; contributors?: unknown;
  dependencies?: Record<string, string>; optionalDependencies?: Record<string, string>;
};

function licenseOf(p: Pkg): string {
  const l = p.license ?? p.licenses;
  if (typeof l === "string") return l;
  if (Array.isArray(l)) return l.map((x) => (typeof x === "string" ? x : x?.type)).join(" OR ");
  if (l && typeof l === "object" && "type" in l) return String((l as { type: string }).type);
  return "UNKNOWN";
}

function person(x: unknown): string | null {
  if (typeof x === "string") return x.replace(/\s*[<(].*$/, "").trim() || null;
  if (x && typeof x === "object" && "name" in x) return String((x as { name: string }).name);
  return null;
}

function findPkg(name: string, from: string | null): string | null {
  // Walk up from the requiring package's folder, as Node resolves.
  let dir = from;
  while (dir && dir.startsWith(root)) {
    const c = join(dir, "node_modules", name, "package.json");
    if (existsSync(c)) return c;
    if (dir === root) break;
    dir = dirname(dir);
  }
  const c = join(root, "node_modules", name, "package.json");
  return existsSync(c) ? c : null;
}

function jsPackages(): Map<string, Component> {
  const out = new Map<string, Component>();
  const top = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as Pkg;
  const stack: [string, string | null][] = Object.keys(top.dependencies ?? {}).map((n) => [n, null]);
  while (stack.length) {
    const [name, from] = stack.pop()!;
    const path = findPkg(name, from);
    if (!path) continue; // an optional dependency for another platform
    const p = JSON.parse(readFileSync(path, "utf8")) as Pkg;
    const key = `${p.name} ${p.version}`;
    if (out.has(key)) continue;
    const dir = dirname(path);
    let lic = licenseOf(p);
    if (lic === "UNKNOWN") {
      const f = ["LICENSE", "license", "LICENSE.md", "LICENSE.txt"].map((n) => join(dir, n)).find(existsSync);
      if (f && /MIT License/i.test(readFileSync(f, "utf8").slice(0, 200))) lic = "MIT (from its LICENSE file)";
    }
    const authors = [p.author, ...(Array.isArray(p.contributors) ? p.contributors : [])].map(person).filter((x): x is string => !!x);
    out.set(key, { kind: "js", name: p.name, version: p.version, license: lic, dir, authors });
    for (const n of Object.keys({ ...p.dependencies, ...p.optionalDependencies })) stack.push([n, dir]);
  }
  return out;
}

/** The component's own licence files, or the standard text of one license it offers, with its authors filled in. */
function licenseText(c: Component): string {
  const files = c.dir ? readdirSync(c.dir).filter((n) => LICENSE_FILE.test(n)).sort() : [];
  const texts = files.map((n) => ({ n, t: readFileSync(join(c.dir!, n), "utf8").replace(/\r\n/g, "\n").trim() })).filter((x) => x.t);
  if (texts.length) return texts.map(({ n, t }) => (texts.length > 1 ? `--- ${n} ---\n${t}` : t)).join("\n\n");
  const offered = c.license.replace(/[()]/g, " ").split(/\s+OR\s+|\s*\/\s*/).map((s) => s.trim());
  const id = TEMPLATES.find((t) => offered.includes(t));
  if (!id) throw new Error(`${c.name} ${c.version} (${c.license}) ships no licence file and has no template in scripts/licenses/`);
  const holders = c.authors.map((a) => a.replace(/\s*<.*?>/, "").trim()).filter(Boolean).join(", ") || `the ${c.name} authors`;
  const body = readFileSync(join(import.meta.dir, "licenses", `${id}.txt`), "utf8").replace("{holders}", holders).trim();
  return `(The package ships no licence file. This is the standard ${id} text, which it offers, with the authors it lists.)\n\n${body}`;
}

function table(m: Map<string, Component>): string {
  const rows = [...m.values()].sort((a, b) => `${a.name} ${a.version}`.localeCompare(`${b.name} ${b.version}`)).map((c) => `| ${c.name} | ${c.version} | ${c.license} |`);
  return ["| Component | Version | License |", "|---|---|---|", ...rows].join("\n");
}

function summary(m: Map<string, Component>): string {
  const c = new Map<string, number>();
  for (const { license: l } of m.values()) c.set(l, (c.get(l) ?? 0) + 1);
  return [...c].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([l, n]) => `${n} × ${l}`).join(", ");
}

// The weak-copyleft components and where their source is. Shared by both outputs.
const COPYLEFT: [string, string, string][] = [
  ["elkjs (diagram layout, bundled with Mermaid)", "EPL-2.0 (offered as EPL-2.0 OR GPL-3.0-or-later; we take EPL-2.0)", "https://github.com/kieler/elkjs"],
  ["cssparser, selectors (Servo, used by Tauri to read HTML)", "MPL-2.0", "https://github.com/servo/rust-cssparser, https://github.com/servo/stylo"],
  ["dtoa-short", "MPL-2.0", "https://github.com/upsuper/dtoa-short"],
  ["option-ext", "MPL-2.0", "https://github.com/soc/option-ext"],
  ["dompurify", "Apache-2.0 (offered as MPL-2.0 OR Apache-2.0)", "https://github.com/cure53/DOMPurify"],
];
const SYSTEM_LIBS = `On Linux, the app links at run time to system libraries such as WebKitGTK and GTK, which are LGPL licensed.
The \`.deb\` uses the copies your distribution installs; NekoTrail does not ship them. SQLite is compiled in
through rusqlite's \`bundled\` feature and is in the public domain.`;

function noticesMd(rust: Map<string, Component>, js: Map<string, Component>): string {
  return `# Third-party notices

NekoTrail is MIT licensed (see [LICENSE](LICENSE)). The app ships with the open-source components listed
below, each under its own license. The full license text and copyright notices of every component ship
inside the app: Settings, About, Open source licences. They are generated at build time into
\`src-tauri/resources/THIRD_PARTY_LICENSES.txt\` from each package's own license files.

This file and that one are generated by \`bun scripts/third-party-notices.ts\` (\`bun run licenses\`). CI fails
if this file is out of date, so regenerate it when dependencies change.

## Copyleft check

No component is used under the GPL or AGPL. A few are under file-level ("weak") copyleft licenses. We use
them unmodified, and their source is available at the links below:

| Component | License we use it under | Source |
|---|---|---|
${COPYLEFT.map((r) => `| ${r.join(" | ")} |`).join("\n")}

${SYSTEM_LIBS}

## Rust crates (${rust.size})

${summary(rust)}

${table(rust)}

## JavaScript packages (${js.size})

${summary(js)}

${table(js)}
`;
}

function licensesTxt(rust: Map<string, Component>, js: Map<string, Component>): string {
  const version = (JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as Pkg).version;
  const rule = "-".repeat(78);
  // Components with exactly the same licence files share one copy of the text.
  const groups = new Map<string, Component[]>();
  for (const c of [...rust.values(), ...js.values()].sort((a, b) => a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name) || a.version.localeCompare(b.version))) {
    const t = licenseText(c);
    groups.set(t, [...(groups.get(t) ?? []), c]);
  }
  const where = (c: Component) => (c.kind === "rust" ? `https://crates.io/crates/${c.name}/${c.version}` : `https://www.npmjs.com/package/${c.name}/v/${c.version}`);
  const blocks = [...groups].map(([text, cs]) => [
    rule,
    ...cs.map((c) => `${c.name} ${c.version} (${c.kind === "rust" ? "Rust crate" : "JavaScript package"}, ${c.license})\n  Source: ${where(c)}`),
    "",
    text,
  ].join("\n"));
  return `NekoTrail ${version}: open source licences
${"=".repeat(78)}

NekoTrail is free and open source under the MIT License (the first text below).
It ships with ${rust.size} Rust crates and ${js.size} JavaScript packages, each under its own license.
This file holds the license text and copyright notices of every one of them, taken from
each package's own files. Components with identical texts share one copy.

Source code for the weak-copyleft components (used unmodified):
${COPYLEFT.map(([c, l, s]) => `  ${c}\n    License: ${l}\n    Source: ${s}`).join("\n")}

${SYSTEM_LIBS.replace(/`/g, "")}

NekoTrail's own source: https://github.com/Harshil-Jani/nekotrail

${rule}
NekoTrail ${version}
  Source: https://github.com/Harshil-Jani/nekotrail

${readFileSync(join(root, "LICENSE"), "utf8").trim()}

${blocks.join("\n\n")}
`;
}

const rust = rustCrates();
const js = jsPackages();
const md = noticesMd(rust, js);

const offers = (license: string) => license.replace(/\s*\(from its LICENSE file\)/, "").replace(/[()]/g, " ").split(/\s+OR\s+|\s*\/\s*/).map((s) => s.trim());
const outside = [...js.values()].filter((c) => !offers(c.license).some((l) => JS_ALLOWED.includes(l)));
if (outside.length) {
  console.error(`Outside the licence policy (no GPL or AGPL in the app):\n${outside.map((c) => `  ${c.name} ${c.version}: ${c.license}`).join("\n")}`);
  process.exit(1);
}

if (process.argv.includes("--check")) {
  const now = existsSync(NOTICES) ? readFileSync(NOTICES, "utf8") : "";
  if (now !== md) {
    console.error("THIRD_PARTY_NOTICES.md is out of date. Run `bun run licenses` and commit it.");
    process.exit(1);
  }
  licensesTxt(rust, js); // still fails if a component has no licence text
  console.log(`THIRD_PARTY_NOTICES.md is current: ${rust.size} crates, ${js.size} JS packages.`);
} else {
  writeFileSync(NOTICES, md);
  mkdirSync(dirname(LICENSES), { recursive: true });
  const txt = licensesTxt(rust, js);
  writeFileSync(LICENSES, txt);
  console.log(`Wrote THIRD_PARTY_NOTICES.md and ${LICENSES.slice(root.length + 1)} (${Math.round(txt.length / 1024)} KB): ${rust.size} crates, ${js.size} JS packages.`);
}
