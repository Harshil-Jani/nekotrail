//! Protected paths: files agents should leave alone (`.env*`, `migrations/**`, `infra/**`).
//!
//! The list comes from the repo's settings in the app plus the `protected` list in its
//! `nekotrail.json`. The two are kept as separate sets and a path is protected when either set
//! protects it, so a `!` exception in the repo file can only carve out of the repo file's own
//! globs, never out of what the user set in the app.
//!
//! Globs follow gitignore rules: a glob with no `/` (other than a trailing one) matches a name at
//! any depth, anything else is anchored at the workspace root, `*` and `?` stay inside one folder,
//! `**` crosses folders, a trailing `/` means folders only, and `!` makes an exception (the last
//! glob that matches wins). One difference from git, on purpose: an exception works even inside
//! a protected folder, so `migrations/**` then `!migrations/README.md` frees that one file.
//!
//! Enforcement differs per agent (`Agent::guard_level`):
//! - Claude: deny rules for its file tools, passed per turn with `--settings` (see `claude_deny`).
//! - ACP agents (OpenCode): edits are routed through `session/request_permission` and any edit to
//!   a protected path is refused (see `acp::Client`).
//! - Every agent, Codex and Gemini included: after each turn `check` compares the files with the
//!   turn's checkpoint, and `put_back` restores only the protected files that changed.
//!
//! Checkpoints skip git-ignored files, and `.env` usually is one. So before each turn the ignored
//! files that match a protected glob are saved too (`snapshot_ignored`), in a commit under a
//! private ref (`refs/nekotrail/guard/<workspace>/`) that is never pushed and never shows as a
//! branch. It is pruned with the checkpoints and dropped when the workspace is archived.

use crate::git;
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path};

/// One glob, parsed.
#[derive(Debug, Clone, PartialEq)]
struct Rule {
    /// `!`: an exception.
    neg: bool,
    /// Matched against the path from the root (it had a `/`), else against each name in it.
    anchored: bool,
    /// Trailing `/`: matches folders (and so everything in them), never a file of that name.
    dir_only: bool,
    pat: String,
}

fn parse(line: &str) -> Option<Rule> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let (neg, rest) = match t.strip_prefix('!') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let mut anchored = false;
    let mut p = rest;
    while let Some(r) = p.strip_prefix("./") {
        p = r;
        anchored = true;
    }
    if let Some(r) = p.strip_prefix('/') {
        p = r;
        anchored = true;
    }
    let dir_only = p.ends_with('/');
    let p = p.trim_end_matches('/');
    if p.is_empty() || p.split('/').any(|s| s == ".." || s == ".") {
        return None;
    }
    if p.contains('/') {
        anchored = true;
    }
    Some(Rule { neg, anchored, dir_only, pat: p.to_string() })
}

/// Does one glob match `rel` (a file path from the root, `/`-separated) or a folder it sits in?
fn rule_hits(r: &Rule, rel: &str) -> bool {
    let comps: Vec<&str> = rel.split('/').collect();
    let n = comps.len();
    (1..=n).any(|k| {
        let is_dir = k < n;
        if r.dir_only && !is_dir {
            return false;
        }
        if r.anchored { glob(&r.pat, &comps[..k].join("/")) } else { glob(&r.pat, comps[k - 1]) }
    })
}

/// Last matching glob wins.
fn set_protects(rules: &[Rule], rel: &str) -> bool {
    let mut on = false;
    for r in rules {
        if rule_hits(r, rel) {
            on = !r.neg;
        }
    }
    on
}

/// gitignore-style glob over a whole string: `*`, `?`, `[a-z]` / `[!x]`, `\` escapes, and `**`
/// as a whole path segment.
pub fn glob(pat: &str, s: &str) -> bool {
    glob_at(pat.as_bytes(), s.as_bytes(), true)
}

fn glob_at(p: &[u8], s: &[u8], seg_start: bool) -> bool {
    if p.is_empty() {
        return s.is_empty();
    }
    if seg_start && p.starts_with(b"**") && (p.len() == 2 || p[2] == b'/') {
        if p.len() == 2 {
            return true;
        }
        let rest = &p[3..];
        if glob_at(rest, s, true) {
            return true;
        }
        return s.iter().enumerate().any(|(i, c)| *c == b'/' && glob_at(rest, &s[i + 1..], true));
    }
    match p[0] {
        b'*' => {
            // A run of stars inside a segment is one star.
            let mut rest = p;
            while rest.first() == Some(&b'*') {
                rest = &rest[1..];
            }
            let mut i = 0;
            loop {
                if glob_at(rest, &s[i..], false) {
                    return true;
                }
                if i >= s.len() || s[i] == b'/' {
                    return false;
                }
                i += 1;
            }
        }
        b'?' => !s.is_empty() && s[0] != b'/' && glob_at(&p[1..], &s[1..], false),
        b'[' => match class(p, s.first().copied()) {
            Some((true, used)) => glob_at(&p[used..], &s[1..], false),
            Some((false, _)) => false,
            // No closing `]`: a plain `[`.
            None => s.first() == Some(&b'[') && glob_at(&p[1..], &s[1..], false),
        },
        b'\\' if p.len() > 1 => s.first() == Some(&p[1]) && glob_at(&p[2..], &s[1..], p[1] == b'/'),
        c => s.first() == Some(&c) && glob_at(&p[1..], &s[1..], c == b'/'),
    }
}

/// `[...]` at the start of `p` against `c`: (matched, bytes of pattern used). None if unclosed.
fn class(p: &[u8], c: Option<u8>) -> Option<(bool, usize)> {
    let mut i = 1;
    let negate = matches!(p.get(i), Some(b'!') | Some(b'^'));
    if negate {
        i += 1;
    }
    let start = i;
    let mut hit = false;
    while i < p.len() {
        if p[i] == b']' && i > start {
            let ok = c.is_some_and(|c| c != b'/') && (hit != negate);
            return Some((ok, i + 1));
        }
        let lo = p[i];
        if i + 2 < p.len() && p[i + 1] == b'-' && p[i + 2] != b']' {
            let hi = p[i + 2];
            hit |= c.is_some_and(|c| lo <= c && c <= hi);
            i += 3;
        } else {
            hit |= c == Some(lo);
            i += 1;
        }
    }
    None
}

/// The protected paths for one workspace: what the user set in the app, and what the repo's
/// `nekotrail.json` adds.
#[derive(Debug, Clone, Default)]
pub struct Rules {
    app: Vec<Rule>,
    repo: Vec<Rule>,
}

impl Rules {
    pub fn new(app: &[String], repo: &[String]) -> Rules {
        Rules { app: app.iter().filter_map(|g| parse(g)).collect(), repo: repo.iter().filter_map(|g| parse(g)).collect() }
    }

    /// Nothing is protected (no globs, or only exceptions).
    pub fn is_empty(&self) -> bool {
        !self.app.iter().chain(&self.repo).any(|r| !r.neg)
    }

    /// Is this file (path from the workspace root) protected?
    pub fn protects(&self, rel: &str) -> bool {
        let rel = rel.trim_start_matches("./");
        set_protects(&self.app, rel) || set_protects(&self.repo, rel)
    }

    /// Could a file inside this folder be protected because of the folder itself?
    fn dir_protected(&self, dir: &str) -> bool {
        self.protects(&format!("{}/\u{1}", dir.trim_end_matches('/')))
    }

    /// Claude Code deny rules (`Edit(...)` covers Edit, Write, MultiEdit and NotebookEdit, and the
    /// file commands Claude recognises in Bash such as `sed -i`, `tee` and `>`).
    ///
    /// Claude matches gitignore-style too, with two differences handled here (both checked with
    /// Claude Code 2.1.319): a relative `dir/**` deny matches that folder at any depth, so
    /// anchored globs are written as absolute `//<workspace>/...` paths; and a `!` exception only
    /// carves out of relative rules, so only exceptions with no `/` are passed on. An exception
    /// Claude can't express makes Claude stricter than the list, never looser, and the check after
    /// the turn still follows it. The repo file's rules come first, so its exceptions can't carve
    /// out of the app's rules that follow.
    pub fn claude_deny(&self, roots: &[String]) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        let mut push = |s: String| {
            if !out.contains(&s) {
                out.push(s);
            }
        };
        for set in [&self.repo, &self.app] {
            for r in set {
                if r.neg {
                    if !r.anchored && !r.dir_only {
                        push(format!("Edit(!{})", r.pat));
                    }
                    continue;
                }
                let inside = r.pat.ends_with("/**") || r.pat == "**";
                if r.anchored {
                    for root in roots {
                        let base = format!("/{}/{}", escape(root.trim_end_matches('/')), r.pat);
                        if !r.dir_only {
                            push(format!("Edit({base})"));
                        }
                        if !inside {
                            push(format!("Edit({base}/**)"));
                        }
                    }
                } else {
                    if !r.dir_only {
                        push(format!("Edit({})", r.pat));
                    }
                    if !inside {
                        push(format!("Edit({}/**)", r.pat));
                    }
                }
            }
        }
        out
    }
}

/// A folder path as a literal inside a gitignore-style glob.
fn escape(path: &str) -> String {
    let mut s = String::with_capacity(path.len());
    for c in path.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s
}

/// Private ref folder for one workspace's saved ignored files.
pub fn prefix(workspace_id: &str) -> String {
    let safe: String = workspace_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    format!("refs/nekotrail/guard/{safe}/")
}

const GUARD_SUBJECT: &str = "nekotrail guard";
/// Ignored files bigger than this are left out of the check (both before and after a turn).
const MAX_FILE: u64 = 5_000_000;
/// At most this many ignored protected files are watched per workspace.
const MAX_FILES: usize = 1000;
/// Ignored folders that aren't protected themselves are looked into, but only this far in total,
/// and never these: they can hold hundreds of thousands of files.
const WALK_BUDGET: usize = 2000;
const HEAVY: &[&str] = &["node_modules", "target", ".venv", "venv", "dist", "build", ".next", ".turbo", ".cache", "vendor", "Pods", "DerivedData", "__pycache__", ".gradle", "coverage", ".git"];

/// Git-ignored files in the workspace that a protected glob matches, sorted. Uses
/// `ls-files --directory`, so a whole ignored folder costs one line: it is only looked into
/// when the folder itself is protected (then fully), or within a small budget otherwise.
fn ignored_protected(ws: &Path, rules: &Rules) -> Result<Vec<String>> {
    let out = git::git_bytes(ws, None, &["ls-files", "-z", "-o", "-i", "--exclude-standard", "--directory"], None)?;
    let mut found: Vec<String> = vec![];
    let mut budget = WALK_BUDGET;
    for entry in out.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let Ok(entry) = std::str::from_utf8(entry) else { continue };
        if let Some(dir) = entry.strip_suffix('/') {
            if rules.dir_protected(dir) {
                let mut all = usize::MAX;
                walk(ws, dir, &mut all, false, &mut found);
            } else if !HEAVY.contains(&dir.rsplit('/').next().unwrap_or(dir)) {
                walk(ws, dir, &mut budget, true, &mut found);
            }
        } else {
            found.push(entry.to_string());
        }
    }
    let mut v: Vec<String> = found
        .into_iter()
        .filter(|p| rules.protects(p))
        .filter(|p| std::fs::symlink_metadata(ws.join(p)).is_ok_and(|m| m.is_file() && m.len() <= MAX_FILE))
        .collect();
    v.sort();
    v.dedup();
    v.truncate(MAX_FILES);
    Ok(v)
}

fn walk(ws: &Path, dir: &str, budget: &mut usize, skip_heavy: bool, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(ws.join(dir)) else { return };
    for e in entries.flatten() {
        if *budget == 0 || out.len() > MAX_FILES * 4 {
            return;
        }
        *budget = budget.saturating_sub(1);
        let Some(name) = e.file_name().to_str().map(str::to_string) else { continue };
        let rel = format!("{dir}/{name}");
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if name != ".git" && !(skip_heavy && HEAVY.contains(&name.as_str())) {
                walk(ws, &rel, budget, skip_heavy, out);
            }
        } else if ft.is_file() {
            out.push(rel);
        }
    }
}

/// Blob ids for files in the workspace, in order. `write` also stores them.
fn hash_files(ws: &Path, files: &[String], write: bool) -> Result<Vec<String>> {
    if files.is_empty() {
        return Ok(vec![]);
    }
    let input: String = files.iter().map(|f| format!("{f}\n")).collect();
    let mut args = vec!["hash-object", "--no-filters", "--stdin-paths"];
    if write {
        args.push("-w");
    }
    let out = git::git_bytes(ws, None, &args, Some(input.as_bytes()))?;
    let ids: Vec<String> = String::from_utf8_lossy(&out).lines().map(str::to_string).collect();
    if ids.len() != files.len() {
        bail!("git hash-object returned {} ids for {} files", ids.len(), files.len());
    }
    Ok(ids)
}

fn mode_of(ws: &Path, rel: &str) -> &'static str {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(ws.join(rel)) {
        Ok(m) if m.permissions().mode() & 0o111 != 0 => "100755",
        _ => "100644",
    }
}

/// Ignored protected files as path -> blob id, right now.
fn ignored_now(ws: &Path, rules: &Rules, write: bool) -> Result<BTreeMap<String, (String, &'static str)>> {
    let files: Vec<String> = ignored_protected(ws, rules)?.into_iter().filter(|f| !f.contains('\n')).collect();
    let ids = hash_files(ws, &files, write)?;
    Ok(files.into_iter().zip(ids).map(|(f, id)| { let m = mode_of(ws, &f); (f, (id, m)) }).collect())
}

/// Save the ignored files a protected glob matches, before a turn. Returns the commit id, or ""
/// when there are none (still a valid "nothing was there" for `check`). Call
/// `git::prune_checkpoints(ws, &prefix, KEEP)` afterwards to keep the newest only.
pub fn snapshot_ignored(ws: &Path, prefix: &str, rules: &Rules) -> Result<String> {
    let now = ignored_now(ws, rules, true)?;
    if now.is_empty() {
        return Ok(String::new());
    }
    let idx = git::TempIndex::new();
    let mut info: Vec<u8> = vec![];
    for (path, (id, mode)) in &now {
        info.extend_from_slice(format!("{mode} {id}\t{path}").as_bytes());
        info.push(0);
    }
    git::git_bytes(ws, Some(&idx.0), &["update-index", "-z", "--add", "--index-info"], Some(&info))?;
    let tree = git::git_idx(ws, &idx.0, &["write-tree"])?;
    let commit = git::commit_tree(ws, &[&tree, "-m", GUARD_SUBJECT])?;
    let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    git::git(ws, &["update-ref", &format!("{prefix}{millis:015}-{}", &commit[..12]), &commit])?;
    Ok(commit)
}

/// True for a commit `snapshot_ignored` made.
pub fn is_snapshot(ws: &Path, id: &str) -> bool {
    git::git(ws, &["log", "-1", "--format=%an%x09%s", id, "--"]).map(|o| o == format!("nekotrail\t{GUARD_SUBJECT}")).unwrap_or(false)
}

/// Path -> blob id in a commit's tree.
fn tree_files(ws: &Path, commit: &str) -> Result<BTreeMap<String, String>> {
    let out = git::git_bytes(ws, None, &["ls-tree", "-r", "-z", commit], None)?;
    let mut m = BTreeMap::new();
    for e in out.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let e = String::from_utf8_lossy(e);
        if let Some((meta, path)) = e.split_once('\t')
            && let Some(id) = meta.split(' ').nth(2) {
                m.insert(path.to_string(), id.to_string());
            }
    }
    Ok(m)
}

/// What a turn started from: its checkpoint (tracked and untracked files) and its saved ignored
/// files ("" = none were there; None = not saved, so ignored files are not compared).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Base {
    pub checkpoint: Option<String>,
    pub ignored: Option<String>,
}

/// One protected file a turn changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hit {
    pub path: String,
    /// "added", "changed" or "deleted".
    pub change: &'static str,
}

/// Protected files that differ from `base`: tracked and untracked ones through the checkpoint
/// (a rename shows as its old path deleted and its new path added, so both sides are checked),
/// ignored ones through the saved copies. Sorted by path.
pub fn check(ws: &Path, rules: &Rules, base: &Base) -> Result<Vec<Hit>> {
    let mut hits: BTreeMap<String, &'static str> = BTreeMap::new();
    if rules.is_empty() {
        return Ok(vec![]);
    }
    if let Some(cp) = &base.checkpoint {
        let now = git::worktree_tree(ws)?;
        for (status, path) in git::tree_changes(ws, &format!("{cp}^{{tree}}"), &now, false)? {
            let path = String::from_utf8_lossy(&path).to_string();
            if rules.protects(&path) {
                hits.insert(path, match status {
                    b'A' => "added",
                    b'D' => "deleted",
                    _ => "changed",
                });
            }
        }
    }
    if let Some(ign) = &base.ignored {
        let before = if ign.is_empty() { BTreeMap::new() } else { tree_files(ws, ign)? };
        let now = ignored_now(ws, rules, false)?;
        for (path, (id, _)) in &now {
            if hits.contains_key(path) {
                continue;
            }
            match before.get(path) {
                None => {
                    hits.insert(path.clone(), "added");
                }
                Some(old) if old != id => {
                    hits.insert(path.clone(), "changed");
                }
                _ => {}
            }
        }
        for path in before.keys() {
            if !now.contains_key(path) && !hits.contains_key(path) && rules.protects(path) {
                // Still on disk but no longer ignored: the checkpoint diff speaks for it.
                let tracked_now = git::git(ws, &["ls-files", "--error-unmatch", "--", &format!(":(literal){path}")]).is_ok();
                if !tracked_now {
                    hits.insert(path.clone(), "deleted");
                }
            }
        }
    }
    Ok(hits.into_iter().map(|(path, change)| Hit { path, change }).collect())
}

/// Every existing folder on the way to `rel` is a real folder inside `ws` (no symlink out).
fn plain_path_inside(ws: &Path, rel: &str) -> bool {
    let p = Path::new(rel);
    if rel.is_empty() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return false;
    }
    let mut cur = ws.to_path_buf();
    let comps: Vec<_> = p.components().collect();
    for c in &comps[..comps.len() - 1] {
        cur.push(c);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            Ok(_) => return false,
            Err(_) => return true,
        }
    }
    true
}

/// Put back the protected files in `paths` that a turn changed, and nothing else: each current
/// copy goes through `trash` first (if that fails, the file is left alone and this stops), then
/// the version from before the turn is written. A protected file the turn created just goes to
/// the Trash. Paths that aren't protected or didn't change since `base` are skipped.
/// Returns the paths put back.
pub fn put_back(ws: &Path, rules: &Rules, base: &Base, paths: &[String], trash: impl Fn(&Path) -> Result<()>) -> Result<Vec<String>> {
    let hits = check(ws, rules, base)?;
    let wanted: Vec<&Hit> = hits.iter().filter(|h| paths.contains(&h.path)).collect();
    if wanted.is_empty() {
        bail!("Those files are already as they were before the turn.");
    }
    let in_cp: HashSet<String> = match &base.checkpoint {
        Some(cp) => tree_files(ws, cp)?.into_keys().collect(),
        None => HashSet::new(),
    };
    let in_ign: HashSet<String> = match base.ignored.as_deref() {
        Some(id) if !id.is_empty() => tree_files(ws, id)?.into_keys().collect(),
        _ => HashSet::new(),
    };
    // One private index per source, read once.
    let load = |commit: &str| -> Result<git::TempIndex> {
        let idx = git::TempIndex::new();
        git::git_idx(ws, &idx.0, &["read-tree", &format!("{commit}^{{tree}}")])?;
        Ok(idx)
    };
    let cp_idx = match &base.checkpoint {
        Some(cp) if wanted.iter().any(|h| in_cp.contains(&h.path)) => Some(load(cp)?),
        _ => None,
    };
    let ign_idx = match base.ignored.as_deref() {
        Some(id) if !id.is_empty() && wanted.iter().any(|h| in_ign.contains(&h.path)) => Some(load(id)?),
        _ => None,
    };
    let mut done = vec![];
    for h in wanted {
        if !plain_path_inside(ws, &h.path) {
            continue;
        }
        let from = if in_cp.contains(&h.path) {
            cp_idx.as_ref()
        } else if in_ign.contains(&h.path) {
            ign_idx.as_ref()
        } else {
            None
        };
        // A file the turn created goes only when the copy that would have held it was saved:
        // the ignored-file snapshot for an ignored file, the checkpoint for any other.
        let known_new = h.change == "added"
            && from.is_none()
            && if git::is_ignored(ws, &h.path) { base.ignored.is_some() } else { base.checkpoint.is_some() };
        if from.is_none() && !known_new {
            continue;
        }
        let full = ws.join(&h.path);
        if full.symlink_metadata().is_ok() {
            trash(&full)?;
        }
        if let Some(idx) = from {
            let mut p = h.path.as_bytes().to_vec();
            p.push(0);
            git::git_bytes(ws, Some(&idx.0), &["checkout-index", "-f", "-z", "--stdin"], Some(&p))?;
        }
        done.push(h.path.clone());
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::init_repo;

    fn rules(app: &[&str], repo: &[&str]) -> Rules {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        Rules::new(&s(app), &s(repo))
    }

    #[test]
    fn globs_follow_gitignore() {
        let r = rules(&[".env*", "migrations/**", "infra/", "/Makefile", "**/secrets/*.pem", "docs/*.md", "*.lock"], &[]);
        // Dotfiles, at any depth for a bare name.
        for p in [".env", ".env.local", "apps/web/.env.production", ".envrc"] {
            assert!(r.protects(p), "{p}");
        }
        assert!(!r.protects("env"));
        assert!(!r.protects("src/dotenv.ts"));
        // Anchored at the root, nested files included.
        assert!(r.protects("migrations/001.sql"));
        assert!(r.protects("migrations/2024/01/x.sql"));
        assert!(!r.protects("db/migrations/001.sql"), "a glob with a / is anchored");
        assert!(!r.protects("migrations"), "a file named migrations is not inside it");
        // Folder-only glob: everything inside, at any depth for a bare name, never a file named so.
        assert!(r.protects("infra/main.tf"));
        assert!(r.protects("deploy/infra/x/y.tf"));
        assert!(!r.protects("infra"));
        // Leading slash anchors a bare name.
        assert!(r.protects("Makefile"));
        assert!(!r.protects("sub/Makefile"));
        // ** in the middle and at the start.
        assert!(r.protects("secrets/a.pem"));
        assert!(r.protects("a/b/secrets/c.pem"));
        assert!(!r.protects("a/secrets/b/c.pem"), "* stays in one folder");
        assert!(r.protects("docs/a.md"));
        assert!(!r.protects("docs/sub/a.md"));
        assert!(r.protects("bun.lock") && r.protects("x/Cargo.lock"));
        assert!(!r.protects("src/main.rs"));
        // Character classes, ? and escapes.
        assert!(glob("v[0-9].txt", "v3.txt") && !glob("v[0-9].txt", "vx.txt"));
        assert!(glob("v[!0-9].txt", "vx.txt") && !glob("v[!0-9].txt", "v3.txt"));
        assert!(glob("a?c", "abc") && !glob("a?c", "a/c"));
        assert!(glob(r"\*.txt", "*.txt") && !glob(r"\*.txt", "a.txt"));
        assert!(glob("[unclosed", "[unclosed"));
        assert!(glob("a/**/b", "a/b") && glob("a/**/b", "a/x/y/b") && !glob("a/**/b", "ab"));
        // Junk lines are ignored, not turned into "protect everything".
        assert!(rules(&["", "  ", "# note", "../x", "./"], &[]).is_empty());
        assert!(rules(&["!foo"], &[]).is_empty(), "only exceptions protect nothing");
        assert!(rules(&["./build.sh"], &[]).protects("build.sh") && !rules(&["./build.sh"], &[]).protects("x/build.sh"));
    }

    #[test]
    fn exceptions_and_order() {
        let r = rules(&[".env*", "!.env.example", "migrations/**", "!migrations/README.md"], &[]);
        assert!(r.protects(".env") && r.protects("app/.env.local"));
        assert!(!r.protects(".env.example") && !r.protects("app/.env.example"));
        assert!(r.protects("migrations/001.sql"));
        assert!(!r.protects("migrations/README.md"), "an exception works inside a protected folder");
        // The last match wins: protecting again after an exception.
        let r = rules(&["*.md", "!docs/*.md", "docs/SECURITY.md"], &[]);
        assert!(r.protects("README.md") && !r.protects("docs/a.md") && r.protects("docs/SECURITY.md"));
    }

    #[test]
    fn repo_file_adds_but_never_removes() {
        // The user protects .env*; the repo file tries to free .env and protects infra.
        let r = rules(&[".env*"], &["!.env", "!*", "infra/**"]);
        assert!(r.protects(".env"), "a repo exception can't undo an app protection");
        assert!(r.protects(".env.local"));
        assert!(r.protects("infra/main.tf"), "the repo file adds its own");
        // Its exceptions still work on its own globs.
        let r = rules(&[], &["infra/**", "!infra/README.md"]);
        assert!(r.protects("infra/main.tf") && !r.protects("infra/README.md"));
        // And the app's exceptions only shape the app's list.
        let r = rules(&["!infra/README.md"], &["infra/**"]);
        assert!(r.protects("infra/README.md"));
        assert!(rules(&[], &["x"]).protects("x") && !rules(&[], &[]).protects("x"));
    }

    #[test]
    fn claude_rules() {
        let r = rules(&[".env*", "!.env.example", "migrations/**", "infra/", "!migrations/keep.sql"], &["secrets", "!.env.local"]);
        let deny = r.claude_deny(&["/w/ws".into(), "/private/w/ws".into()]);
        assert_eq!(deny, [
            "Edit(secrets)", "Edit(secrets/**)", "Edit(!.env.local)",
            "Edit(.env*)", "Edit(.env*/**)", "Edit(!.env.example)",
            "Edit(//w/ws/migrations/**)", "Edit(//private/w/ws/migrations/**)",
            "Edit(infra/**)",
        ]);
        // Glob characters in the workspace path are literal.
        let r = rules(&["a/b"], &[]);
        assert_eq!(r.claude_deny(&["/x/[y]*".into()]), [r"Edit(//x/\[y\]\*/a/b)", r"Edit(//x/\[y\]\*/a/b/**)"]);
        assert!(rules(&[], &[]).claude_deny(&["/w".into()]).is_empty());
    }

    fn fake_trash(bin: &Path) -> impl Fn(&Path) -> Result<()> + '_ {
        move |p: &Path| {
            let n = std::fs::read_dir(bin).unwrap().count();
            std::fs::rename(p, bin.join(format!("{n}-{}", p.file_name().unwrap().to_string_lossy())))?;
            Ok(())
        }
    }

    /// A repo with `.env` ignored (as `copy_files` sets it up), a tracked migration and a README.
    fn setup() -> (tempfile::TempDir, Rules) {
        let d = init_repo();
        let r = d.path();
        std::fs::write(r.join(".gitignore"), ".env*\n!.env.example\nnode_modules/\n").unwrap();
        std::fs::create_dir_all(r.join("migrations")).unwrap();
        std::fs::write(r.join("migrations/001.sql"), "create table a;\n").unwrap();
        std::fs::write(r.join("migrations/002.sql"), "create table b;\n").unwrap();
        git::git(r, &["add", "."]).unwrap();
        git::git(r, &["commit", "-qm", "base"]).unwrap();
        std::fs::write(r.join(".env"), "TOKEN=secret\n").unwrap();
        std::fs::create_dir_all(r.join("apps/web")).unwrap();
        std::fs::write(r.join("apps/web/.env.local"), "PORT=1\n").unwrap();
        std::fs::create_dir_all(r.join("node_modules/pkg")).unwrap();
        std::fs::write(r.join("node_modules/pkg/.env"), "ignored and heavy\n").unwrap();
        (d, rules(&[".env*", "!.env.example", "migrations/**"], &[]))
    }

    fn base(ws: &Path, rules: &Rules) -> Base {
        let cp = git::checkpoint(ws, &git::checkpoint_prefix("g")).unwrap();
        let ign = snapshot_ignored(ws, &prefix("g"), rules).unwrap();
        Base { checkpoint: Some(cp), ignored: Some(ign) }
    }

    #[test]
    fn detects_tracked_untracked_ignored_and_renamed() {
        let (d, rules) = setup();
        let r = d.path();
        assert_eq!(ignored_protected(r, &rules).unwrap(), [".env", "apps/web/.env.local"], "node_modules is never walked");
        let b = base(r, &rules);
        assert!(check(r, &rules, &b).unwrap().is_empty(), "nothing changed yet");
        // The turn: edits an ignored .env, adds an untracked migration, renames a tracked one,
        // deletes an ignored file, touches unprotected files and an exception.
        std::fs::write(r.join(".env"), "TOKEN=leaked\n").unwrap();
        std::fs::write(r.join("migrations/003.sql"), "drop table a;\n").unwrap();
        std::fs::rename(r.join("migrations/002.sql"), r.join("old-002.sql")).unwrap();
        std::fs::remove_file(r.join("apps/web/.env.local")).unwrap();
        std::fs::write(r.join(".env.example"), "TOKEN=\n").unwrap();
        std::fs::write(r.join("a.txt"), "edited\n").unwrap();
        std::fs::write(r.join("node_modules/pkg/.env"), "changed but not watched\n").unwrap();
        let hits = check(r, &rules, &b).unwrap();
        assert_eq!(hits, [
            Hit { path: ".env".into(), change: "changed" },
            Hit { path: "apps/web/.env.local".into(), change: "deleted" },
            Hit { path: "migrations/002.sql".into(), change: "deleted" },
            Hit { path: "migrations/003.sql".into(), change: "added" },
        ]);
        // A rename into a protected path is caught on its new side.
        let b = base(r, &rules);
        std::fs::rename(r.join("old-002.sql"), r.join("migrations/009.sql")).unwrap();
        assert_eq!(check(r, &rules, &b).unwrap(), [Hit { path: "migrations/009.sql".into(), change: "added" }]);
        // Committed changes count too: the checkpoint holds the files, not just the diff.
        let b = base(r, &rules);
        std::fs::write(r.join("migrations/001.sql"), "changed;\n").unwrap();
        git::git(r, &["add", "-A"]).unwrap();
        git::git(r, &["commit", "-qm", "agent"]).unwrap();
        assert_eq!(check(r, &rules, &b).unwrap(), [Hit { path: "migrations/001.sql".into(), change: "changed" }]);
        // New ignored file when none were there before.
        std::fs::remove_file(r.join(".env")).unwrap();
        let b = base(r, &rules);
        assert_eq!(b.ignored.as_deref(), Some(""));
        std::fs::write(r.join(".env.test"), "X=1\n").unwrap();
        assert_eq!(check(r, &rules, &b).unwrap(), [Hit { path: ".env.test".into(), change: "added" }]);
        // Without a saved copy of ignored files, they are not compared at all.
        let blind = Base { checkpoint: b.checkpoint.clone(), ignored: None };
        assert!(check(r, &rules, &blind).unwrap().is_empty());
        assert!(is_snapshot(r, base(r, &rules).ignored.as_deref().unwrap()));
        assert!(!is_snapshot(r, "HEAD"));
    }

    #[test]
    fn put_back_restores_only_protected_files() {
        let (d, rules) = setup();
        let r = d.path();
        let bin = tempfile::tempdir().unwrap();
        let b = base(r, &rules);
        std::fs::write(r.join(".env"), "TOKEN=leaked\n").unwrap();
        std::fs::write(r.join("migrations/001.sql"), "drop table a;\n").unwrap();
        std::fs::write(r.join("migrations/003.sql"), "new\n").unwrap();
        std::fs::remove_file(r.join("apps/web/.env.local")).unwrap();
        std::fs::write(r.join("a.txt"), "the agent's real work\n").unwrap();
        std::fs::write(r.join("src.rs"), "fn main() {}\n").unwrap();
        // Ask for everything, including files that aren't protected: only protected ones move.
        let asked: Vec<String> = [".env", "migrations/001.sql", "migrations/003.sql", "apps/web/.env.local", "a.txt", "src.rs"].map(String::from).to_vec();
        let mut done = put_back(r, &rules, &b, &asked, fake_trash(bin.path())).unwrap();
        done.sort();
        assert_eq!(done, [".env", "apps/web/.env.local", "migrations/001.sql", "migrations/003.sql"]);
        assert_eq!(std::fs::read_to_string(r.join(".env")).unwrap(), "TOKEN=secret\n");
        assert_eq!(std::fs::read_to_string(r.join("apps/web/.env.local")).unwrap(), "PORT=1\n");
        assert_eq!(std::fs::read_to_string(r.join("migrations/001.sql")).unwrap(), "create table a;\n");
        assert!(!r.join("migrations/003.sql").exists(), "a protected file the turn created goes to the Trash");
        // Everything else is untouched.
        assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "the agent's real work\n");
        assert!(r.join("src.rs").exists());
        // The agent's copies are in the Trash, not gone.
        let mut trashed: Vec<String> = std::fs::read_dir(bin.path()).unwrap().map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap()).collect();
        trashed.sort();
        assert_eq!(trashed, ["TOKEN=leaked\n", "drop table a;\n", "new\n"]);
        assert!(check(r, &rules, &b).unwrap().is_empty());
        assert!(put_back(r, &rules, &b, &asked, fake_trash(bin.path())).is_err(), "nothing left to put back");
        // The staging area is not touched.
        assert_eq!(git::git(r, &["diff", "--cached", "--name-only"]).unwrap(), "");
    }

    #[test]
    fn put_back_stops_when_the_trash_fails_and_never_removes_unknown_files() {
        let (d, rules) = setup();
        let r = d.path();
        let b = base(r, &rules);
        std::fs::write(r.join(".env"), "TOKEN=leaked\n").unwrap();
        let err = put_back(r, &rules, &b, &[".env".into()], |_| bail!("no Trash")).unwrap_err();
        assert!(err.to_string().contains("no Trash"));
        assert_eq!(std::fs::read_to_string(r.join(".env")).unwrap(), "TOKEN=leaked\n", "left alone");
        // A new file is only removed when the saved copies prove it wasn't there before.
        let bin = tempfile::tempdir().unwrap();
        let blind = Base { checkpoint: b.checkpoint.clone(), ignored: None };
        std::fs::write(r.join("migrations/004.sql"), "x\n").unwrap();
        let done = put_back(r, &rules, &blind, &["migrations/004.sql".into()], fake_trash(bin.path())).unwrap();
        assert_eq!(done, ["migrations/004.sql"], "the checkpoint knows it is new");
        // No checkpoint: tracked and untracked files can't be compared, so nothing is touched.
        let half = Base { checkpoint: None, ignored: b.ignored.clone() };
        std::fs::write(r.join("migrations/005.sql"), "x\n").unwrap();
        assert!(put_back(r, &rules, &half, &["migrations/005.sql".into()], fake_trash(bin.path())).is_err());
        assert!(r.join("migrations/005.sql").exists());
    }
}
