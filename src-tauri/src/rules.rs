//! One set of project rules for every agent. Each agent CLI reads its own instructions file, so a
//! repo that wants the same rules everywhere ends up with CLAUDE.md, AGENTS.md, GEMINI.md and
//! friends drifting apart. This module lists which of those files a folder has, which agent reads
//! which, where they differ from the one file the user picks as the source of truth, and plans the
//! change that makes every other file point at it.
//!
//! What each agent reads in a repo's top folder, from its docs (checked 2026-11-06):
//! - Claude Code (code.claude.com/docs/en/memory): `CLAUDE.md`, `.claude/CLAUDE.md`,
//!   `CLAUDE.local.md`, `.claude/rules/*.md`. `AGENTS.md` only when none of the three CLAUDE files
//!   exist (v2.1.277 and later). `@path` imports, relative to the importing file; the docs
//!   recommend a `CLAUDE.md` holding `@AGENTS.md`, or a symlink.
//! - Codex (developers.openai.com/codex/guides/agents-md): `AGENTS.override.md`, else `AGENTS.md`,
//!   one per folder from the project root down. No imports.
//! - Gemini CLI (geminicli.com/docs/cli/gemini-md): `GEMINI.md` unless `context.fileName` names
//!   others (e.g. `AGENTS.md`). `@./file.md` imports.
//! - OpenCode (opencode.ai/docs/rules): `AGENTS.md`, else `CLAUDE.md`.
//! - Cursor CLI (cursor.com/docs/cli/using, cursor.com/docs/context/rules): `AGENTS.md`,
//!   `CLAUDE.md` and `.cursor/rules/*.mdc`.
//! - Copilot CLI (docs.github.com, "Adding custom instructions for Copilot CLI"):
//!   `.github/copilot-instructions.md`, `.github/instructions/**/*.instructions.md`, `AGENTS.md`,
//!   `CLAUDE.md`, `.claude/CLAUDE.md`, `GEMINI.md`, all combined, no precedence.
//! - Goose (goose-docs.ai, "Using goosehints"): `AGENTS.md` and `.goosehints`.
//! - Qwen Code (packages/core/src/utils/memory-constants.ts): `QWEN.md` and `AGENTS.md`.
//! - Kimi CLI (kimi-cli docs, slash commands): `/init` writes `AGENTS.md`, which it reads.
//! - Auggie (docs.augmentcode.com/cli/rules): `AGENTS.md`, else `CLAUDE.md`, plus
//!   `.augment/rules/` and `.augment-guidelines`.
//! - Grok Build (secondary sources only, e.g. DataCamp's tutorial): `AGENTS.md`.
//! - Kiro (kiro.dev/docs/steering): `AGENTS.md` (always included) and `.kiro/steering/*.md`.
//! - Droid (docs.factory.com/cli/configuration/agents-md): `AGENTS.md` and `CLAUDE.md`.
//! - Mistral Vibe (docs.mistral.ai, Vibe agents): `AGENTS.md`, only in trusted folders.
//! - Cline (docs.cline.bot/features/cline-rules): `.clinerules/`, `.cline/rules/`, `AGENTS.md`,
//!   `.cursorrules`, `.windsurfrules`.
//! - Kilo (kilo.ai/docs/agent-behavior/agents-md): `AGENTS.md`, else `AGENT.md`; `.kilo/rules/`
//!   and the older `.kilocode/rules/`.
//!
//! Folders of scoped rules (`.cursor/rules/`, `.kiro/steering/`, ...) are listed but never
//! rewritten: their files carry their own front matter. Files in subfolders are not covered.
//!
//! Writes follow mcpshare.rs: the exact change is shown first, with a token that Apply checks, the
//! old file is backed up (outside the repo, so the backup never shows in Changes), and each file is
//! swapped in with one rename. Nothing is committed. A file someone wrote by hand is only replaced
//! when the user confirmed it after seeing the diff.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// When an agent reads a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum When {
    Always,
    /// Claude reads AGENTS.md only with no CLAUDE.md, .claude/CLAUDE.md or CLAUDE.local.md.
    NoClaudeMd,
    /// OpenCode and Auggie read CLAUDE.md, Kilo AGENT.md, only with no AGENTS.md.
    NoAgentsMd,
    /// Codex reads AGENTS.md only with no (non-empty) AGENTS.override.md.
    NoOverride,
    /// Gemini reads AGENTS.md only when `context.fileName` lists it.
    GeminiSetting,
}
use When::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Project rules: can be the source, or be pointed at it.
    Shared,
    /// Yours alone (CLAUDE.local.md): listed, never touched.
    Personal,
    /// AGENTS.override.md: Codex reads it instead of AGENTS.md.
    Override,
    /// A folder of scoped rules with their own front matter: listed, never touched.
    Scoped,
}

struct Known {
    path: &'static str,
    dir: bool,
    role: Role,
    readers: &'static [(&'static str, When)],
    /// How the file imports another: `Claude` style (`@AGENTS.md`) or `Gemini` style (`@./AGENTS.md`).
    import: Option<ImportStyle>,
    note: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImportStyle {
    Claude,
    Gemini,
}

const AGENTS_MD_READERS: &[(&str, When)] = &[
    ("claude", NoClaudeMd), ("codex", NoOverride), ("gemini", GeminiSetting), ("opencode", Always), ("cursor", Always), ("copilot", Always),
    ("goose", Always), ("qwen", Always), ("kimi", Always), ("auggie", Always), ("grok", Always), ("kiro", Always), ("droid", Always),
    ("vibe", Always), ("cline", Always), ("kilo", Always),
];

const KNOWN: &[Known] = &[
    Known { path: "AGENTS.md", dir: false, role: Role::Shared, readers: AGENTS_MD_READERS, import: None, note: "" },
    Known { path: "CLAUDE.md", dir: false, role: Role::Shared, readers: &[("claude", Always), ("opencode", NoAgentsMd), ("cursor", Always), ("copilot", Always), ("auggie", NoAgentsMd), ("droid", Always)], import: Some(ImportStyle::Claude), note: "" },
    Known { path: ".claude/CLAUDE.md", dir: false, role: Role::Shared, readers: &[("claude", Always), ("copilot", Always)], import: Some(ImportStyle::Claude), note: "" },
    Known { path: "GEMINI.md", dir: false, role: Role::Shared, readers: &[("gemini", Always), ("copilot", Always)], import: Some(ImportStyle::Gemini), note: "" },
    Known { path: "QWEN.md", dir: false, role: Role::Shared, readers: &[("qwen", Always)], import: None, note: "" },
    Known { path: ".github/copilot-instructions.md", dir: false, role: Role::Shared, readers: &[("copilot", Always)], import: None, note: "" },
    Known { path: ".goosehints", dir: false, role: Role::Shared, readers: &[("goose", Always)], import: None, note: "" },
    Known { path: ".clinerules", dir: false, role: Role::Shared, readers: &[("cline", Always)], import: None, note: "" },
    Known { path: ".cursorrules", dir: false, role: Role::Shared, readers: &[("cline", Always)], import: None, note: "Cursor's old rules file. Cursor's docs no longer list it; Cline still reads it." },
    Known { path: ".windsurfrules", dir: false, role: Role::Shared, readers: &[("cline", Always)], import: None, note: "" },
    Known { path: ".augment-guidelines", dir: false, role: Role::Shared, readers: &[("auggie", Always)], import: None, note: "" },
    Known { path: "AGENT.md", dir: false, role: Role::Shared, readers: &[("kilo", NoAgentsMd)], import: None, note: "Kilo reads it only when there is no AGENTS.md." },
    Known { path: "AGENTS.override.md", dir: false, role: Role::Override, readers: &[("codex", Always)], import: None, note: "Codex reads this instead of AGENTS.md in this folder." },
    Known { path: "CLAUDE.local.md", dir: false, role: Role::Personal, readers: &[("claude", Always)], import: None, note: "Your own Claude notes. While it exists, Claude stops reading AGENTS.md by itself." },
    Known { path: ".claude/rules", dir: true, role: Role::Scoped, readers: &[("claude", Always)], import: None, note: "" },
    Known { path: ".cursor/rules", dir: true, role: Role::Scoped, readers: &[("cursor", Always)], import: None, note: "" },
    Known { path: ".github/instructions", dir: true, role: Role::Scoped, readers: &[("copilot", Always)], import: None, note: "" },
    Known { path: ".kiro/steering", dir: true, role: Role::Scoped, readers: &[("kiro", Always)], import: None, note: "" },
    Known { path: ".augment/rules", dir: true, role: Role::Scoped, readers: &[("auggie", Always)], import: None, note: "" },
    Known { path: ".clinerules", dir: true, role: Role::Scoped, readers: &[("cline", Always)], import: None, note: "" },
    Known { path: ".cline/rules", dir: true, role: Role::Scoped, readers: &[("cline", Always)], import: None, note: "" },
    Known { path: ".kilo/rules", dir: true, role: Role::Scoped, readers: &[("kilo", Always)], import: None, note: "" },
    Known { path: ".kilocode/rules", dir: true, role: Role::Scoped, readers: &[("kilo", Always)], import: None, note: "Kilo's older rules folder." },
];

/// Where each agent documents the files it reads.
pub const DOCS: &[(&str, &str)] = &[
    ("claude", "https://code.claude.com/docs/en/memory"),
    ("codex", "https://developers.openai.com/codex/guides/agents-md"),
    ("gemini", "https://geminicli.com/docs/cli/gemini-md/"),
    ("opencode", "https://opencode.ai/docs/rules/"),
    ("cursor", "https://cursor.com/docs/cli/using"),
    ("copilot", "https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-custom-instructions"),
    ("goose", "https://goose-docs.ai/docs/guides/context-engineering/using-goosehints"),
    ("qwen", "https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/utils/memory-constants.ts"),
    ("kimi", "https://moonshotai.github.io/kimi-cli/en/reference/slash-commands.html"),
    ("auggie", "https://docs.augmentcode.com/cli/rules"),
    ("grok", "https://www.datacamp.com/tutorial/grok-build-tutorial"),
    ("kiro", "https://kiro.dev/docs/steering/"),
    ("droid", "https://docs.factory.com/cli/configuration/agents-md"),
    ("vibe", "https://docs.mistral.ai/fr/vibe/code/cli/agents"),
    ("cline", "https://docs.cline.bot/features/cline-rules"),
    ("kilo", "https://kilo.ai/docs/agent-behavior/agents-md"),
];

/// The agents, in the app's order (`AGENTS` in src/agents.ts).
const ORDER: [&str; 16] = ["claude", "codex", "gemini", "opencode", "cursor", "copilot", "goose", "qwen", "kimi", "auggie", "grok", "kiro", "droid", "vibe", "cline", "kilo"];

/// What a reader caveat says, in plain words.
fn when_text(w: When) -> &'static str {
    match w {
        Always => "",
        NoClaudeMd => "only when there's no CLAUDE.md or CLAUDE.local.md",
        NoAgentsMd => "only when there's no AGENTS.md",
        NoOverride => "only when there's no AGENTS.override.md",
        GeminiSetting => "only when context.fileName in .gemini/settings.json lists it",
    }
}

fn known_file(path: &str) -> Option<&'static Known> {
    KNOWN.iter().find(|k| !k.dir && k.path == path)
}

// ---------- reading a folder ----------

/// The shape of a path on disk, without following a final symlink.
#[derive(Clone, Debug, PartialEq)]
enum Disk {
    Missing,
    File(Vec<u8>),
    Link(String),
    Dir,
}

fn disk(root: &Path, rel: &str) -> Disk {
    let p = root.join(rel);
    match p.symlink_metadata() {
        Err(_) => Disk::Missing,
        Ok(m) if m.file_type().is_symlink() => Disk::Link(std::fs::read_link(&p).map(|t| t.to_string_lossy().to_string()).unwrap_or_default()),
        Ok(m) if m.is_dir() => Disk::Dir,
        Ok(_) => Disk::File(std::fs::read(&p).unwrap_or_default()),
    }
}

/// The text an agent sees at `rel` (through a symlink, if it is one).
fn text_at(root: &Path, rel: &str) -> Option<String> {
    std::fs::read(root.join(rel)).ok().map(|b| String::from_utf8_lossy(&b).to_string())
}

fn present(root: &Path, rel: &str) -> bool {
    !matches!(disk(root, rel), Disk::Missing | Disk::Dir)
}

/// The file names `context.fileName` in the repo's `.gemini/settings.json` lists, if any.
fn gemini_names(root: &Path) -> Vec<String> {
    let Some(t) = text_at(root, ".gemini/settings.json") else { return vec![] };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&crate::mcpshare::blank_jsonc(&t)) else { return vec![] };
    match &v["context"]["fileName"] {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        _ => vec![],
    }
}

/// Facts that decide the conditional readers.
struct Facts {
    claude_md: bool,
    agents_md: bool,
    override_md: bool,
    gemini_names: Vec<String>,
}

impl Facts {
    fn read(root: &Path) -> Facts {
        Facts {
            claude_md: ["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"].iter().any(|f| present(root, f)),
            agents_md: present(root, "AGENTS.md"),
            override_md: text_at(root, "AGENTS.override.md").is_some_and(|t| !t.trim().is_empty()),
            gemini_names: gemini_names(root),
        }
    }

    fn active(&self, w: When, path: &str) -> bool {
        match w {
            Always => true,
            NoClaudeMd => !self.claude_md,
            NoAgentsMd => !self.agents_md,
            NoOverride => !self.override_md,
            GeminiSetting => self.gemini_names.iter().any(|n| n == path),
        }
    }
}

// ---------- the marks nekotrail leaves ----------

const GEN_PREFIX: &str = "<!-- Generated by nekotrail from ";
const NOTE_MARK: &str = "<!-- nekotrail:points-to ";

fn short_hash(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// A generated copy: `(source, body, intact)`. `intact` is false once someone edited the body.
fn generated(text: &str) -> Option<(String, String, bool)> {
    let first = text.lines().next()?;
    let rest = first.strip_prefix(GEN_PREFIX)?;
    let source = rest.split(". ").next()?.to_string();
    let hash = rest.split("nekotrail:sha256=").nth(1)?.trim_end_matches(" -->").trim().to_string();
    let body = text[first.len()..].strip_prefix('\n').unwrap_or(&text[first.len()..]);
    let body = body.strip_prefix('\n').unwrap_or(body).to_string();
    let intact = short_hash(&body) == hash;
    Some((source, body, intact))
}

fn copy_text(source: &str, body: &str) -> String {
    format!("{GEN_PREFIX}{source}. Do not edit this file: edit {source}, then sync it from Rules in nekotrail. nekotrail:sha256={} -->\n\n{body}", short_hash(body))
}

/// The path from `from`'s folder to `to`, both relative to the repo root.
fn relative(from: &str, to: &str) -> String {
    let depth = from.matches('/').count();
    format!("{}{to}", "../".repeat(depth))
}

fn import_line(style: ImportStyle, target: &str, source: &str) -> String {
    let rel = relative(target, source);
    match style {
        ImportStyle::Claude => format!("@{rel}"),
        ImportStyle::Gemini if rel.starts_with("../") => format!("@{rel}"),
        ImportStyle::Gemini => format!("@./{rel}"),
    }
}

fn import_text(style: ImportStyle, target: &str, source: &str, agent: &str) -> String {
    format!("<!-- {agent} reads this file, so it imports {source}. Edit {source}, not this file. -->\n{}\n", import_line(style, target, source))
}

fn note_text(source: &str, readers: &str, many: bool) -> String {
    let verb = if many { "read" } else { "reads" };
    format!("{NOTE_MARK}{source} -->\nThis project's rules live in {source}, which {readers} also {verb}. Edit {source}, not this file.\n")
}

/// Strip HTML comments, for telling what a file says apart from its marks.
fn without_comments(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        match rest[i..].find("-->") {
            Some(j) => rest = &rest[i + j + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The repo path a file only imports (`@AGENTS.md`, `@./AGENTS.md`, `@../AGENTS.md`), if that is
/// all it says.
fn pointer_of(rel: &str, text: &str) -> Option<String> {
    let bare = without_comments(text);
    let lines: Vec<&str> = bare.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let [only] = lines.as_slice() else { return None };
    let target = only.strip_prefix('@')?;
    if target.contains(char::is_whitespace) || target.starts_with('/') || target.starts_with('~') {
        return None;
    }
    Some(resolve(rel, target))
}

/// `target` as written in `from`, as a path from the repo root.
fn resolve(from: &str, target: &str) -> String {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for seg in target.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Who a note says reads the source, from the readers of the file it replaces.
fn note_mark(text: &str) -> Option<String> {
    let first = text.lines().next()?;
    Some(first.strip_prefix(NOTE_MARK)?.trim_end_matches(" -->").trim().to_string())
}

fn same_text(a: &str, b: &str) -> bool {
    a.trim_end() == b.trim_end()
}

// ---------- diff ----------

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DiffLine {
    /// "same", "add", "del", or "gap" (lines left out; `text` says how many).
    pub op: &'static str,
    pub text: String,
}

const DIFF_MAX: usize = 4000;
const CONTEXT: usize = 3;

/// A line diff from `a` to `b`, with 3 lines of context around each change.
pub fn diff(a: &str, b: &str) -> Vec<DiffLine> {
    let x: Vec<&str> = a.lines().collect();
    let y: Vec<&str> = b.lines().collect();
    if x.len() > DIFF_MAX || y.len() > DIFF_MAX {
        return vec![DiffLine { op: "gap", text: "Too long to compare here.".into() }];
    }
    let (n, m) = (x.len(), y.len());
    // Longest common subsequence, from the end.
    let mut t = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[at(i, j)] = if x[i] == y[j] { t[at(i + 1, j + 1)] + 1 } else { t[at(i + 1, j)].max(t[at(i, j + 1)]) };
        }
    }
    let mut all = vec![];
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && x[i] == y[j] {
            all.push(("same", x[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || t[at(i + 1, j)] >= t[at(i, j + 1)]) {
            // On a tie the old line goes first, as in git.
            all.push(("del", x[i]));
            i += 1;
        } else {
            all.push(("add", y[j]));
            j += 1;
        }
    }
    // Keep CONTEXT same lines around each change, and say how many were left out.
    let changed: Vec<usize> = all.iter().enumerate().filter(|(_, (op, _))| *op != "same").map(|(k, _)| k).collect();
    if changed.is_empty() {
        return vec![];
    }
    let near = |k: usize| changed.iter().any(|&c| k + CONTEXT >= c && k <= c + CONTEXT);
    let mut out = vec![];
    let mut skipped = 0;
    for (k, (op, text)) in all.iter().enumerate() {
        if *op == "same" && !near(k) {
            skipped += 1;
            continue;
        }
        if skipped > 0 {
            out.push(DiffLine { op: "gap", text: format!("{skipped} same {}", if skipped == 1 { "line" } else { "lines" }) });
            skipped = 0;
        }
        out.push(DiffLine { op, text: text.to_string() });
    }
    if skipped > 0 {
        out.push(DiffLine { op: "gap", text: format!("{skipped} same {}", if skipped == 1 { "line" } else { "lines" }) });
    }
    out
}

// ---------- overview ----------

#[derive(Clone, Debug, Serialize)]
pub struct Reader {
    pub agent: &'static str,
    /// Reads it now, given the other files here.
    pub active: bool,
    /// The condition, in plain words, when there is one.
    pub when: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuleFile {
    pub path: String,
    pub role: Role,
    pub dir: bool,
    pub exists: bool,
    pub readers: Vec<Reader>,
    pub note: String,
    /// "source", "same", "points" (imports or links to the source), "copy" (generated, up to
    /// date), "stale" (generated from an older source), "edited" (generated, then edited by hand),
    /// "differs", "empty", "missing", "folder".
    pub state: &'static str,
    /// Symlink target, as written.
    pub link: Option<String>,
    /// The file it imports, links or points to, from the repo root.
    pub points_to: Option<String>,
    /// How: "import", "link" or "note" (says where the rules are; the agent reads them anyway).
    pub via: Option<&'static str>,
    /// From the source to this file, when they differ.
    pub diff: Vec<DiffLine>,
    pub lines: usize,
    /// Files in a folder of scoped rules.
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentRow {
    pub agent: &'static str,
    /// The files it reads here now, e.g. "CLAUDE.md" or "AGENTS.md (through CLAUDE.md)".
    pub reads: Vec<String>,
    /// Reads the source of truth (directly or through an import or link).
    pub gets_source: bool,
    pub doc: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct Overview {
    pub source: String,
    pub source_exists: bool,
    /// Files that can be the source (existing shared files that aren't pointers), plus AGENTS.md.
    pub sources: Vec<String>,
    pub files: Vec<RuleFile>,
    pub agents: Vec<AgentRow>,
    pub warnings: Vec<String>,
}

fn dir_files(root: &Path, rel: &str) -> Vec<String> {
    let mut out = vec![];
    let mut stack = vec![rel.to_string()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(root.join(&d)) else { continue };
        for e in rd.flatten() {
            let name = format!("{d}/{}", e.file_name().to_string_lossy());
            match e.file_type() {
                Ok(t) if t.is_dir() && out.len() < 200 => stack.push(name),
                Ok(_) => out.push(name),
                Err(_) => {}
            }
        }
    }
    out.sort();
    out.truncate(200);
    out
}

/// What the default source is: AGENTS.md, which most agents read.
pub const DEFAULT_SOURCE: &str = "AGENTS.md";

pub fn overview(root: &Path, source: Option<&str>) -> Overview {
    let facts = Facts::read(root);
    let source = source.filter(|s| known_file(s).is_some_and(|k| k.role == Role::Shared)).unwrap_or(DEFAULT_SOURCE).to_string();
    let source_text = match disk(root, &source) {
        Disk::File(b) => Some(String::from_utf8_lossy(&b).to_string()),
        _ => None,
    };
    let mut files = vec![];
    let mut warnings = vec![];
    for k in KNOWN {
        let d = disk(root, k.path);
        if k.dir != matches!(d, Disk::Dir) && !matches!(d, Disk::Missing) {
            continue; // `.clinerules` is a file or a folder; show the one it is.
        }
        let exists = !matches!(d, Disk::Missing);
        if !exists && (k.dir || k.role != Role::Shared || !matches!(k.path, "AGENTS.md" | "CLAUDE.md" | "GEMINI.md")) {
            continue;
        }
        let readers: Vec<Reader> = k.readers.iter().map(|&(agent, w)| Reader { agent, active: exists && facts.active(w, k.path), when: when_text(w) }).collect();
        let text = text_at(root, k.path);
        let link = if let Disk::Link(t) = &d { Some(t.clone()) } else { None };
        let imported = text.as_deref().and_then(|t| pointer_of(k.path, t));
        let noted = text.as_deref().and_then(note_mark);
        let (points_to, via) = if let Some(t) = &link {
            (Some(resolve(k.path, t)), Some("link"))
        } else if let Some(t) = imported {
            (Some(t), Some("import"))
        } else if let Some(t) = noted {
            (Some(t), Some("note"))
        } else {
            (None, None)
        };
        let mut diff_lines = vec![];
        let state = if k.dir {
            "folder"
        } else if !exists {
            "missing"
        } else if k.path == source {
            "source"
        } else if points_to.as_deref() == Some(source.as_str()) {
            "points"
        } else if text.as_deref().is_none_or(|t| t.trim().is_empty()) {
            "empty"
        } else {
            let t = text.clone().unwrap_or_default();
            match (generated(&t), &source_text) {
                (Some((from, body, intact)), Some(src)) if from == source => {
                    if !intact {
                        diff_lines = diff(src, &body);
                        "edited"
                    } else if same_text(&body, src) {
                        "copy"
                    } else {
                        diff_lines = diff(src, &body);
                        "stale"
                    }
                }
                (_, Some(src)) if same_text(&t, src) => "same",
                (_, Some(src)) => {
                    diff_lines = diff(src, &t);
                    "differs"
                }
                (_, None) => "differs",
            }
        };
        if k.role == Role::Override && exists {
            warnings.push("AGENTS.override.md is here, so Codex reads it instead of AGENTS.md.".into());
        }
        files.push(RuleFile {
            path: k.path.into(),
            role: k.role,
            dir: k.dir,
            exists,
            readers,
            note: k.note.into(),
            state,
            link,
            points_to,
            via,
            diff: diff_lines,
            lines: text.as_deref().map(|t| t.lines().count()).unwrap_or(0),
            files: if k.dir { dir_files(root, k.path) } else { vec![] },
        });
    }
    // Which files each agent reads now, following imports and links one step.
    let agents = ORDER
        .iter()
        .map(|&agent| {
            let mut reads = vec![];
            let mut gets = false;
            for f in files.iter().filter(|f| f.exists && f.readers.iter().any(|r| r.agent == agent && r.active)) {
                let label = if f.dir { format!("{}/", f.path) } else { f.path.clone() };
                match (&f.points_to, f.via) {
                    (Some(t), Some("import" | "link")) => {
                        gets |= t == &source;
                        reads.push(format!("{t} (through {label})"));
                    }
                    _ => {
                        gets |= f.path == source || matches!(f.state, "same" | "copy");
                        reads.push(label);
                    }
                }
            }
            let doc = DOCS.iter().find(|(a, _)| *a == agent).map(|(_, d)| *d).unwrap_or("");
            AgentRow { agent, reads, gets_source: gets, doc }
        })
        .collect();
    if facts.claude_md && present(root, "CLAUDE.local.md") && source == "AGENTS.md" {
        let claude_imports = text_at(root, "CLAUDE.md").as_deref().and_then(|t| pointer_of("CLAUDE.md", t)).as_deref() == Some("AGENTS.md");
        if !claude_imports {
            warnings.push("CLAUDE.local.md stops Claude from reading AGENTS.md by itself. A CLAUDE.md that imports AGENTS.md fixes that.".into());
        }
    }
    let mut sources: Vec<String> = files
        .iter()
        .filter(|f| f.role == Role::Shared && !f.dir && f.exists && f.link.is_none() && !matches!(f.state, "points" | "copy" | "stale" | "edited" | "empty"))
        .filter(|f| text_at(root, &f.path).as_deref().is_none_or(|t| pointer_of(&f.path, t).is_none() && note_mark(t).is_none()))
        .map(|f| f.path.clone())
        .collect();
    if !sources.iter().any(|s| s == DEFAULT_SOURCE) {
        sources.insert(0, DEFAULT_SOURCE.into());
    }
    Overview { source_exists: source_text.is_some(), source, sources, files, agents, warnings }
}

// ---------- the sync plan ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum How {
    /// A one-line `@source` import (Claude, Gemini).
    Import,
    /// A symlink to the source.
    Symlink,
    /// A short note saying where the rules are, for files whose agents read the source anyway.
    Note,
    /// The source's text with a "generated, do not edit" header.
    Copy,
    /// Leave the file alone.
    Keep,
    /// The source itself, made from another file (`seed`).
    Source,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Request {
    pub source: String,
    /// When the source doesn't exist yet: the file to start it from.
    #[serde(default)]
    pub seed: Option<String>,
    /// Per file; missing means the suggested choice.
    #[serde(default)]
    pub choices: HashMap<String, How>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub path: String,
    pub how: How,
    /// The choices that make sense for this file, best first.
    pub options: Vec<How>,
    /// "create", "update" (nekotrail's own earlier output), "replace" (someone's text), "same"
    /// (already right, nothing to write) or "keep".
    pub status: &'static str,
    /// Replacing text someone wrote: Apply needs this path in `confirmed`.
    pub needs_confirm: bool,
    /// Why this choice, in plain words.
    pub why: String,
    /// Old to new.
    pub diff: Vec<DiffLine>,
    /// What lands on disk: the text, or the link target for a symlink.
    pub new: String,
    pub exists: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    pub source: String,
    pub items: Vec<Item>,
    pub notes: Vec<String>,
    /// Hash of every file involved and every change; Apply refuses if any moved since.
    pub token: String,
}

fn label(agent: &str) -> &'static str {
    match agent {
        "claude" => "Claude",
        "codex" => "Codex",
        "gemini" => "Gemini",
        "opencode" => "OpenCode",
        "cursor" => "Cursor",
        "copilot" => "Copilot",
        "goose" => "Goose",
        "qwen" => "Qwen",
        "kimi" => "Kimi",
        "auggie" => "Auggie",
        "grok" => "Grok",
        "kiro" => "Kiro",
        "droid" => "Droid",
        "vibe" => "Vibe",
        "cline" => "Cline",
        "kilo" => "Kilo",
        _ => "the agent",
    }
}

fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Agents that read `source` on their own, whatever else is in the folder. Conditional readers
/// don't count (Claude skips AGENTS.md when a CLAUDE.md exists), except Gemini when the repo's
/// settings list the file and Codex when there is no override.
fn reads_source(facts: &Facts, source: &str) -> BTreeSet<&'static str> {
    known_file(source)
        .map(|k| k.readers.iter().filter(|(_, w)| matches!(w, Always) || (matches!(w, GeminiSetting | NoOverride) && facts.active(*w, source))).map(|(a, _)| *a).collect())
        .unwrap_or_default()
}

/// Agents that read `target` no matter what (its conditional readers fall back to the source).
fn must_reach(k: &Known) -> Vec<&'static str> {
    k.readers.iter().filter(|(_, w)| *w == Always).map(|(a, _)| *a).collect()
}

fn options_for(k: &Known, covered: bool) -> Vec<How> {
    if covered {
        return vec![How::Note, How::Keep];
    }
    let mut o = vec![];
    if k.import.is_some() {
        o.push(How::Import);
    }
    o.extend([How::Copy, How::Symlink, How::Keep]);
    o
}

/// Body for comparison: a generated copy's body, otherwise the whole text.
fn body_of(text: &str) -> String {
    generated(text).map(|(_, b, _)| b).unwrap_or_else(|| text.to_string())
}

/// Text that is nekotrail's own earlier output, or only points somewhere: safe to replace
/// without asking, since nobody's words are lost.
fn ours(rel: &str, text: &str) -> bool {
    generated(text).is_some_and(|(_, _, intact)| intact) || (note_mark(text).is_some() && without_comments(text).lines().filter(|l| !l.trim().is_empty()).count() <= 1)
        || (pointer_of(rel, text).is_some() && text.contains("Edit ") && text.starts_with("<!-- "))
}

fn fingerprint(d: &Disk) -> Vec<u8> {
    match d {
        Disk::Missing => b"\0missing".to_vec(),
        Disk::Dir => b"\0dir".to_vec(),
        Disk::Link(t) => [b"\0link:".as_slice(), t.as_bytes()].concat(),
        Disk::File(b) => [b"\0file:".as_slice(), b].concat(),
    }
}

pub fn plan(root: &Path, req: &Request) -> Result<Plan, String> {
    let source = req.source.as_str();
    known_file(source).filter(|k| k.role == Role::Shared).ok_or_else(|| format!("{source} can't be the source of truth."))?;
    let facts = Facts::read(root);
    let mut items = vec![];
    let mut notes = vec![];
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    // The source's text: the file itself, or the seed it starts from.
    let src_disk = disk(root, source);
    hasher.update(fingerprint(&src_disk));
    let body = match &src_disk {
        Disk::File(b) => {
            let t = String::from_utf8_lossy(b).to_string();
            if pointer_of(source, &t).is_some() || note_mark(&t).is_some() {
                return Err(format!("{source} only points to another file. Pick that file as the source."));
            }
            if generated(&t).is_some() {
                return Err(format!("{source} is a generated copy. Pick the file it was made from as the source."));
            }
            t
        }
        Disk::Link(t) => return Err(format!("{source} is a link to {t}. Pick {t} as the source, or replace the link first.")),
        Disk::Dir => return Err(format!("{source} is a folder.")),
        Disk::Missing => {
            let seed = req.seed.as_deref().ok_or_else(|| format!("There's no {source} yet. Pick a file to start it from, or write it in Edit rules."))?;
            known_file(seed).filter(|k| k.role == Role::Shared && k.path != source).ok_or_else(|| format!("{seed} can't start {source}."))?;
            let seed_disk = disk(root, seed);
            hasher.update(fingerprint(&seed_disk));
            let Disk::File(b) = seed_disk else { return Err(format!("{seed} isn't a file here.")) };
            let t = String::from_utf8_lossy(&b).to_string();
            let t = body_of(&t);
            if t.trim().is_empty() {
                return Err(format!("{seed} is empty."));
            }
            items.push(Item {
                path: source.into(),
                how: How::Source,
                options: vec![How::Source],
                status: "create",
                needs_confirm: false,
                why: format!("Starts from what {seed} says now."),
                diff: diff("", &t),
                new: t.clone(),
                exists: false,
            });
            t
        }
    };
    if body.contains("\n@") || body.starts_with('@') {
        notes.push(format!("{source} has @ imports. Claude and Gemini follow them; agents reading a generated copy see the @ line as text."));
    }
    let src_readers = reads_source(&facts, source);
    for k in KNOWN.iter().filter(|k| !k.dir && k.role == Role::Shared && k.path != source) {
        let d = disk(root, k.path);
        if d == Disk::Dir {
            continue;
        }
        let exists = d != Disk::Missing;
        let reach = must_reach(k);
        let missing: Vec<&str> = reach.iter().copied().filter(|a| !src_readers.contains(a)).collect();
        let covered = missing.is_empty();
        // A file that doesn't exist only matters when an agent would otherwise miss the rules.
        let seeded = req.seed.as_deref() == Some(k.path);
        if !exists && (covered || !matches!(k.path, "AGENTS.md" | "CLAUDE.md" | "GEMINI.md")) {
            continue;
        }
        let options = options_for(k, covered);
        let suggested = if !exists && k.path == "GEMINI.md" { How::Keep } else { options[0] };
        let how = req.choices.get(k.path).copied().filter(|h| options.contains(h)).unwrap_or(suggested);
        let who = and_list(&missing.iter().map(|a| label(a)).collect::<Vec<_>>());
        let why = match how {
            How::Note => {
                let all = and_list(&reach.iter().map(|a| label(a)).collect::<Vec<_>>());
                if reach.is_empty() { format!("Nothing reads it while {source} exists.") } else { format!("{all} {} {source} too, so this only needs to say where the rules are.", if reach.len() > 1 { "read" } else { "reads" }) }
            }
            How::Import => format!("{who} {} {source}, so this file imports it.", if missing.len() > 1 { "don't read" } else { "doesn't read" }),
            How::Copy => format!("{who} {} {source} and can't import it, so this is a copy that NekoTrail keeps in step.", if missing.len() > 1 { "don't read" } else { "doesn't read" }),
            How::Symlink => format!("A link to {source}. Fine on Mac and Linux; on Windows git checks it out as a one-line file."),
            How::Keep if exists => "Left as it is.".into(),
            How::Keep if missing.is_empty() => format!("Not needed: its agents read {source}."),
            How::Keep => format!("{who} {} {source}. Pick {} to make this file.", if missing.len() > 1 { "don't read" } else { "doesn't read" }, if k.import.is_some() { "Import it" } else { "Generated copy" }),
            How::Source => String::new(),
        };
        let (new, wanted): (String, Disk) = match how {
            How::Keep | How::Source => (String::new(), d.clone()),
            How::Symlink => {
                let t = relative(k.path, source);
                (t.clone(), Disk::Link(t))
            }
            How::Import => {
                let first = reach.first().map(|a| label(a)).unwrap_or("The agent");
                let t = import_text(k.import.unwrap_or(ImportStyle::Claude), k.path, source, first);
                (t.clone(), Disk::File(t.into_bytes()))
            }
            How::Note => {
                let who = and_list(&reach.iter().map(|a| label(a)).collect::<Vec<_>>());
                let t = note_text(source, if who.is_empty() { "the agents" } else { &who }, reach.len() != 1);
                (t.clone(), Disk::File(t.into_bytes()))
            }
            How::Copy => {
                let t = copy_text(source, &body);
                (t.clone(), Disk::File(t.into_bytes()))
            }
        };
        let old_text = match &d {
            Disk::File(b) => Some(String::from_utf8_lossy(b).to_string()),
            _ => None,
        };
        // Already right: the same bytes, the same link, or a hand-made pointer to the source.
        let already = how != How::Keep
            && (d == wanted
                || (how == How::Import && old_text.as_deref().and_then(|t| pointer_of(k.path, t)).as_deref() == Some(source))
                || (how == How::Symlink && matches!(&d, Disk::Link(t) if resolve(k.path, t) == source)));
        let (status, needs_confirm, diff_lines) = if how == How::Keep {
            ("keep", false, vec![])
        } else if already {
            ("same", false, vec![])
        } else {
            match &d {
                Disk::Missing => ("create", false, if how == How::Symlink { vec![] } else { diff("", &new) }),
                Disk::Link(t) => {
                    // A link to the source holds no words of its own.
                    let to_source = resolve(k.path, t) == source;
                    ("replace", !to_source, if how == How::Symlink { vec![] } else { diff(&text_at(root, k.path).unwrap_or_default(), &new) })
                }
                Disk::File(_) => {
                    let old = old_text.clone().unwrap_or_default();
                    let lossless = ours(k.path, &old) || same_text(&body_of(&old), &body) || old.trim().is_empty() || seeded;
                    let shown_new = if how == How::Symlink { format!("(a link to {source}, showing its text)\n{body}") } else { new.clone() };
                    (if ours(k.path, &old) { "update" } else { "replace" }, !lossless, diff(&old, &shown_new))
                }
                Disk::Dir => continue,
            }
        };
        hasher.update(k.path.as_bytes());
        hasher.update(fingerprint(&d));
        hasher.update(format!("{how:?}").as_bytes());
        hasher.update(new.as_bytes());
        items.push(Item { path: k.path.into(), how, options, status, needs_confirm, why, diff: diff_lines, new, exists });
    }
    if facts.override_md && source == "AGENTS.md" {
        notes.push("AGENTS.override.md is here, so Codex reads it instead of AGENTS.md. NekoTrail leaves it alone.".into());
    }
    let token = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(Plan { source: source.into(), items, notes, token })
}

// ---------- writing ----------

#[derive(Clone, Debug, Serialize)]
pub struct Applied {
    pub written: Vec<String>,
    /// Where the old files went, if any were replaced.
    pub backup: Option<String>,
}

/// `rel`'s folder, made if needed, checked to be inside `root` with no link leading out.
fn parent_inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|_| "The workspace folder is missing.".to_string())?;
    let full = root.join(rel);
    let parent = full.parent().ok_or("Bad path.")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("Couldn't make the folder for {rel}: {e}"))?;
    let real = parent.canonicalize().map_err(|e| e.to_string())?;
    if !real.starts_with(&root) {
        return Err(format!("{rel} is in a folder that links outside the workspace. NekoTrail won't write there."));
    }
    Ok(real.join(full.file_name().ok_or("Bad path.")?))
}

fn backup(root: &Path, rel: &str, into: &Path) -> Result<(), String> {
    let dest = into.join(rel);
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    match disk(root, rel) {
        Disk::File(b) => std::fs::write(&dest, b).map_err(|e| format!("Couldn't back up {rel}: {e}")),
        Disk::Link(t) => std::fs::write(dest.with_file_name(format!("{}.symlink", dest.file_name().unwrap_or_default().to_string_lossy())), t).map_err(|e| format!("Couldn't back up {rel}: {e}")),
        _ => Ok(()),
    }
}

/// Swap `bytes` (or a link) in at `full` with one rename, keeping a regular file's permissions.
fn swap_in(full: &Path, what: Result<&[u8], &str>) -> Result<(), String> {
    let name = full.file_name().unwrap_or_default().to_string_lossy().to_string();
    let tmp = full.with_file_name(format!(".{name}.nekotrail-tmp"));
    let _ = std::fs::remove_file(&tmp);
    match what {
        Ok(bytes) => {
            std::fs::write(&tmp, bytes).map_err(|e| format!("Couldn't write {name}: {e}"))?;
            if let Ok(m) = full.symlink_metadata()
                && m.is_file()
            {
                let _ = std::fs::set_permissions(&tmp, m.permissions());
            }
        }
        #[cfg(unix)]
        Err(link) => std::os::unix::fs::symlink(link, &tmp).map_err(|e| format!("Couldn't link {name}: {e}"))?,
        // Symlinks need Developer Mode on Windows (#13).
        #[cfg(not(unix))]
        Err(_) => return Err(format!("Linking {name} isn't on Windows yet.")),
    }
    std::fs::rename(&tmp, full).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Couldn't write {name}: {e}")
    })
}

/// Writes exactly what the plan with `token` showed. Every file it replaces is copied to
/// `backup_dir` first. Refuses if anything moved since the preview, or if a file someone wrote is
/// about to be replaced and isn't in `confirmed`.
pub fn apply(root: &Path, req: &Request, token: &str, confirmed: &[String], backup_dir: &Path) -> Result<Applied, String> {
    let p = plan(root, req)?;
    if p.token != token {
        return Err("The rules files changed since the preview. Look at it again before applying.".into());
    }
    if let Some(i) = p.items.iter().find(|i| i.needs_confirm && !confirmed.contains(&i.path)) {
        return Err(format!("{} has its own text. Look at the diff and confirm before replacing it.", i.path));
    }
    let todo: Vec<&Item> = p.items.iter().filter(|i| matches!(i.status, "create" | "update" | "replace")).collect();
    let mut backed = false;
    for i in todo.iter().filter(|i| i.exists) {
        backup(root, &i.path, backup_dir)?;
        backed = true;
    }
    let mut written = vec![];
    for i in todo {
        let full = parent_inside(root, &i.path)?;
        match i.how {
            How::Symlink => swap_in(&full, Err(&i.new))?,
            _ => swap_in(&full, Ok(i.new.as_bytes()))?,
        }
        written.push(i.path.clone());
    }
    Ok(Applied { written, backup: backed.then(|| backup_dir.display().to_string()) })
}

// ---------- editing one file ----------

/// Rules files the editor may open: the known files, and files inside the known folders.
pub fn editable(rel: &str) -> bool {
    known_file(rel).is_some() || KNOWN.iter().any(|k| k.dir && rel.starts_with(&format!("{}/", k.path)) && !rel.contains(".."))
}

#[derive(Clone, Debug, Serialize)]
pub struct Opened {
    pub path: String,
    pub text: String,
    pub exists: bool,
    /// Hash of what was read; Save refuses if the file changed since.
    pub hash: String,
    /// Set when the path is a link: edit the file it points to.
    pub link: Option<String>,
    /// Set for a generated copy: the file it is made from.
    pub generated_from: Option<String>,
}

fn hash_bytes(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn open(root: &Path, rel: &str) -> Result<Opened, String> {
    if !editable(rel) {
        return Err(format!("{rel} isn't a rules file."));
    }
    let d = disk(root, rel);
    let (text, exists, link) = match &d {
        Disk::File(b) => (String::from_utf8_lossy(b).to_string(), true, None),
        Disk::Link(t) => (text_at(root, rel).unwrap_or_default(), true, Some(resolve(rel, t))),
        Disk::Missing => (String::new(), false, None),
        Disk::Dir => return Err(format!("{rel} is a folder.")),
    };
    let generated_from = generated(&text).map(|(s, _, _)| s);
    Ok(Opened { path: rel.into(), hash: hash_bytes(&fingerprint(&d)), text, exists, link, generated_from })
}

/// Saves the editor's text, if the file is still what `open` returned. The old file goes to `backup_dir`.
pub fn save(root: &Path, rel: &str, text: &str, hash: &str, backup_dir: &Path) -> Result<Applied, String> {
    if !editable(rel) {
        return Err(format!("{rel} isn't a rules file."));
    }
    let d = disk(root, rel);
    if hash_bytes(&fingerprint(&d)) != hash {
        return Err(format!("{rel} changed since you opened it (maybe an agent edited it). Open it again to see the new text."));
    }
    if let Disk::Link(t) = &d {
        return Err(format!("{rel} is a link to {}. Edit that file instead.", resolve(rel, t)));
    }
    let existed = matches!(d, Disk::File(_));
    if existed {
        backup(root, rel, backup_dir)?;
    }
    let full = parent_inside(root, rel)?;
    swap_in(&full, Ok(text.as_bytes()))?;
    Ok(Applied { written: vec![rel.into()], backup: existed.then(|| backup_dir.display().to_string()) })
}

// ---------- rules for one task ----------

/// The settings key for a workspace's "Rules for this task" note.
pub fn task_key(workspace_id: &str) -> String {
    format!("task_rules:{workspace_id}")
}

/// The first prompt of a chat, with the task's rules in front.
pub fn with_task_rules(rules: &str, prompt: &str) -> String {
    let r = rules.trim();
    if r.is_empty() {
        return prompt.to_string();
    }
    format!("Rules for this task (from the user, follow them for the whole chat):\n{r}\n\n{prompt}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(dir: &Path, rel: &str, body: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn read(dir: &Path, rel: &str) -> String {
        std::fs::read_to_string(dir.join(rel)).unwrap()
    }

    fn req(source: &str) -> Request {
        Request { source: source.into(), ..Default::default() }
    }

    fn item<'a>(p: &'a Plan, path: &str) -> &'a Item {
        p.items.iter().find(|i| i.path == path).unwrap_or_else(|| panic!("no {path} in {:?}", p.items.iter().map(|i| &i.path).collect::<Vec<_>>()))
    }

    fn agent<'a>(o: &'a Overview, a: &str) -> &'a AgentRow {
        o.agents.iter().find(|r| r.agent == a).unwrap()
    }

    #[test]
    fn detects_files_and_who_reads_them() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        put(r, "AGENTS.md", "# Rules\nUse pnpm.\n");
        put(r, "CLAUDE.md", "# Rules\nUse npm.\n");
        put(r, ".github/copilot-instructions.md", "Be brief.\n");
        put(r, ".cursor/rules/ts.mdc", "---\nglobs: *.ts\n---\nStrict.\n");
        put(r, ".clinerules/one.md", "x");
        let o = overview(r, None);
        assert_eq!(o.source, "AGENTS.md");
        let paths: Vec<&str> = o.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["AGENTS.md", "CLAUDE.md", "GEMINI.md", ".github/copilot-instructions.md", ".cursor/rules", ".clinerules"]);
        // A CLAUDE.md is here, so Claude reads it and not AGENTS.md; OpenCode reads AGENTS.md, not CLAUDE.md.
        assert_eq!(agent(&o, "claude").reads, ["CLAUDE.md"]);
        assert!(!agent(&o, "claude").gets_source);
        assert_eq!(agent(&o, "opencode").reads, ["AGENTS.md"]);
        assert_eq!(agent(&o, "copilot").reads, ["AGENTS.md", "CLAUDE.md", ".github/copilot-instructions.md"]);
        assert_eq!(agent(&o, "cursor").reads, ["AGENTS.md", "CLAUDE.md", ".cursor/rules/"]);
        assert_eq!(agent(&o, "cline").reads, ["AGENTS.md", ".clinerules/"]);
        assert_eq!(agent(&o, "gemini").reads, Vec::<String>::new(), "Gemini reads GEMINI.md only, by default");
        let claude = o.files.iter().find(|f| f.path == "CLAUDE.md").unwrap();
        assert_eq!(claude.state, "differs");
        assert_eq!(claude.diff.iter().map(|l| (l.op, l.text.as_str())).collect::<Vec<_>>(), [("same", "# Rules"), ("del", "Use pnpm."), ("add", "Use npm.")]);
        assert_eq!(o.files.iter().find(|f| f.path == ".cursor/rules").unwrap().files, [".cursor/rules/ts.mdc"]);
    }

    #[test]
    fn precedence_rules() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        // Only AGENTS.md: Claude (2.1.277+) reads it.
        put(r, "AGENTS.md", "a\n");
        assert_eq!(agent(&overview(r, None), "claude").reads, ["AGENTS.md"]);
        // CLAUDE.local.md counts as a CLAUDE.md: Claude stops reading AGENTS.md, and says so.
        put(r, "CLAUDE.local.md", "mine\n");
        let o = overview(r, None);
        assert_eq!(agent(&o, "claude").reads, ["CLAUDE.local.md"]);
        // An override wins for Codex.
        put(r, "AGENTS.override.md", "o\n");
        let o = overview(r, None);
        assert_eq!(agent(&o, "codex").reads, ["AGENTS.override.md"]);
        assert!(o.warnings.iter().any(|w| w.contains("Codex reads it instead")));
        // An empty override doesn't count.
        put(r, "AGENTS.override.md", "  \n");
        assert_eq!(agent(&overview(r, None), "codex").reads, ["AGENTS.md", "AGENTS.override.md"]);
        // Gemini reads AGENTS.md when the repo's settings list it.
        put(r, ".gemini/settings.json", "{ // project\n \"context\": {\"fileName\": [\"AGENTS.md\", \"GEMINI.md\"]},\n}");
        assert_eq!(agent(&overview(r, None), "gemini").reads, ["AGENTS.md"]);
        // OpenCode and Auggie fall back to CLAUDE.md without AGENTS.md.
        let d2 = tempfile::tempdir().unwrap();
        put(d2.path(), "CLAUDE.md", "c\n");
        let o = overview(d2.path(), None);
        assert_eq!(agent(&o, "opencode").reads, ["CLAUDE.md"]);
        assert_eq!(agent(&o, "auggie").reads, ["CLAUDE.md"]);
        assert!(!o.source_exists);
        assert_eq!(o.sources, ["AGENTS.md", "CLAUDE.md"]);
    }

    #[test]
    fn plan_picks_import_note_or_copy() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        put(r, "AGENTS.md", "Use pnpm.\n");
        put(r, ".github/copilot-instructions.md", "Use pnpm.\n");
        put(r, "GEMINI.md", "old\n");
        put(r, ".goosehints", "Use yarn.\n");
        let p = plan(r, &req("AGENTS.md")).unwrap();
        let claude = item(&p, "CLAUDE.md");
        assert_eq!((claude.how, claude.status, claude.needs_confirm), (How::Import, "create", false));
        assert_eq!(claude.new, "<!-- Claude reads this file, so it imports AGENTS.md. Edit AGENTS.md, not this file. -->\n@AGENTS.md\n");
        let gem = item(&p, "GEMINI.md");
        assert_eq!((gem.how, gem.status, gem.needs_confirm), (How::Import, "replace", true), "GEMINI.md has words of its own");
        assert!(gem.new.ends_with("\n@./AGENTS.md\n"));
        // Copilot reads AGENTS.md too: a note is enough, and its text equals the source, so no confirm.
        let cop = item(&p, ".github/copilot-instructions.md");
        assert_eq!((cop.how, cop.needs_confirm), (How::Note, false));
        assert!(cop.new.contains("which Copilot also reads."));
        assert_eq!(cop.why, "Copilot reads AGENTS.md too, so this only needs to say where the rules are.");
        assert!(item(&p, ".goosehints").needs_confirm);
        // A copy when the agent can't import and doesn't read the source: CLAUDE.md as source, AGENTS.md missing.
        let d2 = tempfile::tempdir().unwrap();
        put(d2.path(), "CLAUDE.md", "Rules.\n");
        let p2 = plan(d2.path(), &req("CLAUDE.md")).unwrap();
        let a = item(&p2, "AGENTS.md");
        assert_eq!((a.how, a.status), (How::Copy, "create"));
        assert!(a.new.starts_with("<!-- Generated by nekotrail from CLAUDE.md. Do not edit this file"));
        assert!(a.new.ends_with("\n\nRules.\n"));
        assert_eq!(a.options, [How::Copy, How::Symlink, How::Keep]);
        // GEMINI.md isn't made unless asked.
        assert_eq!(item(&p2, "GEMINI.md").how, How::Keep);
    }

    #[test]
    fn apply_is_idempotent_and_backs_up() {
        let d = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let r = d.path();
        put(r, "AGENTS.md", "Use pnpm.\n");
        put(r, "CLAUDE.md", "Use pnpm.\n");
        let mut rq = req("AGENTS.md");
        rq.choices.insert("GEMINI.md".into(), How::Import);
        let p = plan(r, &rq).unwrap();
        assert!(!item(&p, "CLAUDE.md").needs_confirm, "CLAUDE.md says the same as AGENTS.md");
        let done = apply(r, &rq, &p.token, &[], b.path()).unwrap();
        assert_eq!(done.written, ["CLAUDE.md", "GEMINI.md"]);
        assert_eq!(read(b.path(), "CLAUDE.md"), "Use pnpm.\n");
        assert!(read(r, "CLAUDE.md").ends_with("@AGENTS.md\n"));
        // Again: nothing to write, every item "same".
        let p2 = plan(r, &rq).unwrap();
        assert!(p2.items.iter().all(|i| i.status == "same"), "{:?}", p2.items.iter().map(|i| (&i.path, i.status)).collect::<Vec<_>>());
        let again = apply(r, &rq, &p2.token, &[], b.path()).unwrap();
        assert!(again.written.is_empty());
        assert!(again.backup.is_none());
        // And the overview shows both pointing at the source, and Claude getting it.
        let o = overview(r, None);
        assert_eq!(o.files.iter().find(|f| f.path == "CLAUDE.md").unwrap().state, "points");
        assert_eq!(agent(&o, "claude").reads, ["AGENTS.md (through CLAUDE.md)"]);
        assert!(agent(&o, "claude").gets_source);
        assert!(agent(&o, "gemini").gets_source);
    }

    #[test]
    fn a_hand_written_pointer_counts_as_done() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        put(r, "AGENTS.md", "x\n");
        put(r, "CLAUDE.md", "@AGENTS.md\n");
        put(r, ".claude/CLAUDE.md", "@../AGENTS.md\n");
        let p = plan(r, &req("AGENTS.md")).unwrap();
        assert_eq!(item(&p, "CLAUDE.md").status, "same");
        assert_eq!(item(&p, ".claude/CLAUDE.md").status, "same");
    }

    #[test]
    fn copies_stay_in_step_and_hand_edits_need_a_confirm() {
        let d = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let r = d.path();
        put(r, "CLAUDE.md", "v1\n");
        let rq = req("CLAUDE.md");
        let p = plan(r, &rq).unwrap();
        apply(r, &rq, &p.token, &[], b.path()).unwrap();
        assert_eq!(overview(r, Some("CLAUDE.md")).files.iter().find(|f| f.path == "AGENTS.md").unwrap().state, "copy");
        // The source moves on: the copy is stale and updates without asking.
        put(r, "CLAUDE.md", "v2\n");
        assert_eq!(overview(r, Some("CLAUDE.md")).files.iter().find(|f| f.path == "AGENTS.md").unwrap().state, "stale");
        let p = plan(r, &rq).unwrap();
        let a = item(&p, "AGENTS.md");
        assert_eq!((a.status, a.needs_confirm), ("update", false));
        apply(r, &rq, &p.token, &[], b.path()).unwrap();
        assert!(read(r, "AGENTS.md").ends_with("\n\nv2\n"));
        // Someone edits the copy by hand: the overview says so, and Apply won't replace it unconfirmed.
        let edited = read(r, "AGENTS.md").replace("v2", "v2 and a local tweak");
        put(r, "AGENTS.md", &edited);
        assert_eq!(overview(r, Some("CLAUDE.md")).files.iter().find(|f| f.path == "AGENTS.md").unwrap().state, "edited");
        let p = plan(r, &rq).unwrap();
        let a = item(&p, "AGENTS.md");
        assert_eq!((a.status, a.needs_confirm), ("replace", true));
        assert!(a.diff.iter().any(|l| l.op == "del" && l.text.contains("local tweak")));
        let err = apply(r, &rq, &p.token, &[], b.path()).unwrap_err();
        assert!(err.contains("confirm"), "{err}");
        assert!(read(r, "AGENTS.md").contains("local tweak"), "not clobbered");
        apply(r, &rq, &p.token, &["AGENTS.md".into()], b.path()).unwrap();
        assert!(!read(r, "AGENTS.md").contains("local tweak"));
        assert!(read(b.path(), "AGENTS.md").contains("local tweak"), "backed up");
    }

    #[test]
    fn refuses_when_files_move_after_the_preview() {
        let d = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let r = d.path();
        put(r, "AGENTS.md", "a\n");
        let rq = req("AGENTS.md");
        let p = plan(r, &rq).unwrap();
        put(r, "CLAUDE.md", "written meanwhile\n");
        let err = apply(r, &rq, &p.token, &[], b.path()).unwrap_err();
        assert!(err.contains("changed since the preview"), "{err}");
        assert_eq!(read(r, "CLAUDE.md"), "written meanwhile\n");
        // A change to the source moves the token too.
        let p = plan(r, &rq).unwrap();
        put(r, "AGENTS.md", "b\n");
        assert!(apply(r, &rq, &p.token, &["CLAUDE.md".into()], b.path()).is_err());
    }

    #[test]
    fn seeds_agents_md_from_claude_md() {
        let d = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let r = d.path();
        put(r, "CLAUDE.md", "Long rules.\n");
        assert!(plan(r, &req("AGENTS.md")).unwrap_err().contains("Pick a file to start it from"));
        let rq = Request { source: "AGENTS.md".into(), seed: Some("CLAUDE.md".into()), choices: HashMap::new() };
        let p = plan(r, &rq).unwrap();
        assert_eq!(p.items[0].how, How::Source);
        let c = item(&p, "CLAUDE.md");
        assert_eq!((c.how, c.needs_confirm), (How::Import, false), "its words move into AGENTS.md, nothing lost");
        apply(r, &rq, &p.token, &[], b.path()).unwrap();
        assert_eq!(read(r, "AGENTS.md"), "Long rules.\n");
        assert!(read(r, "CLAUDE.md").ends_with("@AGENTS.md\n"));
        assert_eq!(read(b.path(), "CLAUDE.md"), "Long rules.\n");
    }

    #[test]
    fn symlinks_and_bad_sources() {
        let d = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let r = d.path();
        put(r, "AGENTS.md", "a\n");
        let mut rq = req("AGENTS.md");
        rq.choices.insert("CLAUDE.md".into(), How::Symlink);
        let p = plan(r, &rq).unwrap();
        apply(r, &rq, &p.token, &[], b.path()).unwrap();
        assert_eq!(std::fs::read_link(r.join("CLAUDE.md")).unwrap(), Path::new("AGENTS.md"));
        assert_eq!(item(&plan(r, &rq).unwrap(), "CLAUDE.md").status, "same");
        assert_eq!(overview(r, None).files.iter().find(|f| f.path == "CLAUDE.md").unwrap().state, "points");
        // A linked or pointer file can't be the source.
        assert!(plan(r, &req("CLAUDE.md")).unwrap_err().contains("is a link to AGENTS.md"));
        put(r, "GEMINI.md", "@./AGENTS.md\n");
        assert!(plan(r, &req("GEMINI.md")).unwrap_err().contains("only points"));
        assert!(plan(r, &req("CLAUDE.local.md")).is_err(), "personal files aren't sources");
        assert!(plan(r, &req("../x.md")).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn never_writes_through_a_folder_link_out_of_the_repo() {
        let d = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let r = d.path();
        put(r, "AGENTS.md", "a\n");
        put(out.path(), "copilot-instructions.md", "a\n");
        std::os::unix::fs::symlink(out.path(), r.join(".github")).unwrap();
        let rq = req("AGENTS.md");
        let p = plan(r, &rq).unwrap();
        let err = apply(r, &rq, &p.token, &[], b.path()).unwrap_err();
        assert!(err.contains("links outside"), "{err}");
        assert_eq!(read(out.path(), "copilot-instructions.md"), "a\n");
    }

    #[test]
    fn editor_save_checks_the_hash() {
        let d = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let r = d.path();
        assert!(open(r, "src/main.rs").is_err());
        assert!(open(r, ".cursor/rules/../../x").is_err());
        let o = open(r, "AGENTS.md").unwrap();
        assert!(!o.exists);
        save(r, "AGENTS.md", "new\n", &o.hash, b.path()).unwrap();
        let o = open(r, "AGENTS.md").unwrap();
        put(r, "AGENTS.md", "agent edit\n");
        assert!(save(r, "AGENTS.md", "mine\n", &o.hash, b.path()).unwrap_err().contains("changed since you opened it"));
        let o = open(r, "AGENTS.md").unwrap();
        save(r, "AGENTS.md", "mine\n", &o.hash, b.path()).unwrap();
        assert_eq!(read(b.path(), "AGENTS.md"), "agent edit\n");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("AGENTS.md", r.join("CLAUDE.md")).unwrap();
            let o = open(r, "CLAUDE.md").unwrap();
            assert_eq!(o.link.as_deref(), Some("AGENTS.md"));
            assert!(save(r, "CLAUDE.md", "x", &o.hash, b.path()).unwrap_err().contains("Edit that file instead"));
        }
    }

    #[test]
    fn diff_keeps_context_and_counts_gaps() {
        let a = (1..=20).map(|i| format!("l{i}")).collect::<Vec<_>>().join("\n");
        let b = a.replace("l10", "ten");
        let d = diff(&a, &b);
        assert_eq!(d.first().unwrap().text, "6 same lines");
        assert_eq!(d.iter().filter(|l| l.op == "del").count(), 1);
        assert_eq!(d.last().unwrap().text, "7 same lines");
        assert!(diff("x\n", "x\n").is_empty());
    }

    #[test]
    fn task_rules_go_first() {
        assert_eq!(with_task_rules("  ", "hi"), "hi");
        assert!(with_task_rules("No new deps.", "Fix it").ends_with("No new deps.\n\nFix it"));
    }
}
