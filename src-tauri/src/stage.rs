//! The commit composer in Changes: stage and unstage whole files, hunks or single lines, commit,
//! amend, undo, and the hunk list "Split into commits" works from.
//!
//! Hunks and lines go into the staging area as patches built here and applied with
//! `git apply --cached`, so the working tree is never written. Unstaging applies the same kind of
//! patch in reverse. Every diff is read with fixed options (prefixes, no colour, no external diff
//! tool), so the user's git config can't change what the patch builder sees.

use crate::git::{git, git_bytes};
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

/// Options every diff here is read with. `-c` settings go before the subcommand.
const CFG: &[&str] = &["-c", "diff.noprefix=false", "-c", "diff.mnemonicPrefix=false", "-c", "core.quotePath=true"];
const OPTS: &[&str] = &["--no-color", "--no-ext-diff", "--src-prefix=a/", "--dst-prefix=b/"];

fn literal(p: &str) -> String {
    format!(":(literal){p}")
}

/// One changed file in the staged or the unstaged list.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct StageFile {
    pub path: String,
    /// The path before a staged rename.
    pub old_path: Option<String>,
    /// A, M, D, R, T, U (conflict) or ? (untracked).
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
    /// Matches the repo's protected paths. Filled in by the caller, which knows the rules.
    pub protected: bool,
}

#[derive(Debug, Serialize, Default)]
pub struct StageStatus {
    pub staged: Vec<StageFile>,
    pub unstaged: Vec<StageFile>,
}

/// A `-z` name-status + numstat pair, run with the same extra arguments.
fn listing(ws: &Path, extra: &[&str]) -> Result<Vec<StageFile>> {
    let run = |kind: &str| -> Result<Vec<u8>> {
        let mut args: Vec<&str> = CFG.to_vec();
        args.extend(["diff", kind, "-z"]);
        args.extend(extra);
        git_bytes(ws, None, &args, None)
    };
    let names = run("--name-status")?;
    let mut out: Vec<StageFile> = vec![];
    let mut it = names.split(|b| *b == 0).filter(|s| !s.is_empty());
    while let Some(code) = it.next() {
        let kind = code.first().copied().unwrap_or(b'M') as char;
        let (old, path) = if kind == 'R' || kind == 'C' {
            let o = it.next().map(|b| String::from_utf8_lossy(b).to_string());
            (o, it.next())
        } else {
            (None, it.next())
        };
        let Some(path) = path else { continue };
        out.push(StageFile {
            path: String::from_utf8_lossy(path).to_string(),
            old_path: old,
            status: kind.to_string(),
            additions: 0,
            deletions: 0,
            binary: false,
            protected: false,
        });
    }
    let nums = run("--numstat")?;
    let mut parts = nums.split(|b| *b == 0);
    while let Some(entry) = parts.next() {
        if entry.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(entry);
        let mut f = text.splitn(3, '\t');
        let (a, d) = (f.next().unwrap_or("0").to_string(), f.next().unwrap_or("0").to_string());
        let mut path = f.next().unwrap_or("").to_string();
        if path.is_empty() {
            parts.next();
            path = parts.next().map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
        }
        if let Some(file) = out.iter_mut().find(|x| x.path == path) {
            file.binary = a == "-";
            file.additions = a.parse().unwrap_or(0);
            file.deletions = d.parse().unwrap_or(0);
        }
    }
    Ok(out)
}

/// What is staged (index against HEAD, renames found) and what isn't (worktree against the
/// index, plus untracked files).
pub fn status(ws: &Path) -> Result<StageStatus> {
    let staged = listing(ws, &["--cached", "-M"])?;
    let mut unstaged = listing(ws, &["--no-renames"])?;
    let untracked = git_bytes(ws, None, &["ls-files", "-z", "--others", "--exclude-standard"], None)?;
    for p in untracked.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = String::from_utf8_lossy(p).to_string();
        let full = ws.join(&path);
        let binary = is_binary_file(&full);
        unstaged.push(StageFile {
            additions: if binary { 0 } else { crate::git::count_lines(&full) },
            path,
            old_path: None,
            status: "?".into(),
            deletions: 0,
            binary,
            protected: false,
        });
    }
    Ok(StageStatus { staged, unstaged })
}

fn is_binary_file(p: &Path) -> bool {
    use std::io::Read;
    let Ok(f) = std::fs::File::open(p) else { return false };
    let mut buf = Vec::new();
    f.take(8000).read_to_end(&mut buf).is_ok() && buf.contains(&0)
}

fn tracked(ws: &Path, path: &str) -> Result<bool> {
    let out = git_bytes(ws, None, &["ls-files", "-z", "--", &literal(path)], None)?;
    Ok(!out.is_empty())
}

/// The diff of one file as the composer shows it: staged (index against HEAD; `old` is the
/// path before a rename) or unstaged (worktree against the index, or all of an untracked file).
pub fn diff(ws: &Path, path: &str, staged: bool, old: Option<&str>) -> Result<Vec<u8>> {
    let mut args: Vec<String> = CFG.iter().map(|s| s.to_string()).collect();
    args.push("diff".into());
    args.extend(OPTS.iter().map(|s| s.to_string()));
    if staged {
        args.extend(["--cached".into(), "-M".into(), "--".into(), literal(path)]);
        if let Some(o) = old.filter(|o| *o != path) {
            args.push(literal(o));
        }
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        return git_bytes(ws, None, &a, None);
    }
    if tracked(ws, path)? {
        args.extend(["--no-renames".into(), "--".into(), literal(path)]);
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        return git_bytes(ws, None, &a, None);
    }
    // Untracked: all of it as new. `--no-index` exits 1 when the files differ, which they do.
    args.extend(["--no-index".into(), "--".into(), "/dev/null".into(), path.into()]);
    let out = Command::new("git").arg("-C").arg(ws).args(&args).output()?;
    if !out.status.success() && out.status.code() != Some(1) {
        bail!("git diff failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

/// A short fingerprint of a diff, so a stage request made against an older view is refused.
pub fn sig(diff: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(diff);
    h.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone)]
pub struct Hunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    /// What follows the second `@@` (the function name git found), with its leading space.
    pub section: Vec<u8>,
    /// Body lines, each starting with ' ', '+', '-' or '\\', without the newline.
    pub lines: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub header: Vec<Vec<u8>>,
    pub hunks: Vec<Hunk>,
    pub binary: bool,
}

fn num(b: &[u8]) -> Option<u32> {
    std::str::from_utf8(b).ok()?.parse().ok()
}

/// "-12,3" or "+7" into (start, count).
fn range(b: &[u8]) -> Option<(u32, u32)> {
    let b = &b[1..];
    match b.iter().position(|c| *c == b',') {
        Some(i) => Some((num(&b[..i])?, num(&b[i + 1..])?)),
        None => Some((num(b)?, 1)),
    }
}

fn parse_header(line: &[u8]) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    let rest = line.strip_prefix(b"@@ ")?;
    let end = rest.windows(3).position(|w| w == b" @@")?;
    let mid = &rest[..end];
    let section = rest[end + 3..].to_vec();
    let sp = mid.iter().position(|c| *c == b' ')?;
    let (o, n) = (range(&mid[..sp])?, range(&mid[sp + 1..])?);
    Some((o.0, o.1, n.0, n.1, section))
}

/// Parse one file's unified diff, bytes exact (CR before the newline stays part of the line).
pub fn parse(diff: &[u8]) -> FileDiff {
    let mut fd = FileDiff { header: vec![], hunks: vec![], binary: false };
    let mut lines: Vec<&[u8]> = diff.split(|b| *b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    for l in lines {
        if let Some((os, oc, ns, nc, section)) = l.starts_with(b"@@ ").then(|| parse_header(l)).flatten() {
            fd.hunks.push(Hunk { old_start: os, old_count: oc, new_start: ns, new_count: nc, section, lines: vec![] });
        } else if let Some(h) = fd.hunks.last_mut() {
            h.lines.push(if l.is_empty() { b" ".to_vec() } else { l.to_vec() });
        } else {
            if l.starts_with(b"Binary files ") || l == b"GIT binary patch" {
                fd.binary = true;
            }
            fd.header.push(l.to_vec());
        }
    }
    fd
}

/// How many `+` and `-` lines a diff has: the line numbers a selection uses run 0..this.
pub fn change_count(fd: &FileDiff) -> usize {
    fd.hunks.iter().flat_map(|h| &h.lines).filter(|l| matches!(l.first(), Some(b'+') | Some(b'-'))).count()
}

/// The change lines of hunk `i`, as selection numbers.
pub fn hunk_lines(fd: &FileDiff, i: usize) -> Vec<usize> {
    let before: usize = fd.hunks[..i].iter().flat_map(|h| &h.lines).filter(|l| matches!(l.first(), Some(b'+') | Some(b'-'))).count();
    let n = fd.hunks[i].lines.iter().filter(|l| matches!(l.first(), Some(b'+') | Some(b'-'))).count();
    (before..before + n).collect()
}

/// A path token from a `---`/`+++` line with its a/ or b/ prefix swapped for `to`, quotes kept.
fn swap_prefix(token: &[u8], to: u8) -> Vec<u8> {
    let (quoted, body) = match token.strip_prefix(b"\"") {
        Some(b) => (true, b),
        None => (false, token),
    };
    let mut out = vec![];
    if quoted {
        out.push(b'"');
    }
    out.push(to);
    out.extend_from_slice(body.get(1..).unwrap_or_default());
    out
}

fn header_value<'a>(header: &'a [Vec<u8>], prefix: &[u8]) -> Option<&'a [u8]> {
    header.iter().find_map(|l| l.strip_prefix(prefix))
}

/// The header of a patch that stages part of a file. Hash lines go (the result is a different
/// blob), and so do mode changes, which only whole-file staging carries. A rename becomes an
/// edit of the new path, so undoing part of it keeps the rename. A new file stays new when
/// staging forward and a deleted one stays deleted when unstaging; the other way round they are
/// plain edits, since the file still exists afterwards.
fn partial_header(fd: &FileDiff, reverse: bool) -> Result<Vec<Vec<u8>>> {
    let minus = header_value(&fd.header, b"--- ").unwrap_or_default();
    let plus = header_value(&fd.header, b"+++ ").unwrap_or_default();
    let is_new = minus == b"/dev/null";
    let is_del = plus == b"/dev/null";
    let mut out = vec![];
    if is_new && !reverse {
        out.push([b"diff --git ".as_slice(), &swap_prefix(plus, b'a'), b" ", plus].concat());
        let mode = header_value(&fd.header, b"new file mode ").unwrap_or(b"100644");
        out.push([b"new file mode ".as_slice(), mode].concat());
        out.push(b"--- /dev/null".to_vec());
        out.push([b"+++ ".as_slice(), plus].concat());
        return Ok(out);
    }
    if is_del && reverse {
        out.push([b"diff --git ".as_slice(), minus, b" ", &swap_prefix(minus, b'b')].concat());
        let mode = header_value(&fd.header, b"deleted file mode ").unwrap_or(b"100644");
        out.push([b"deleted file mode ".as_slice(), mode].concat());
        out.push([b"--- ".as_slice(), minus].concat());
        out.push(b"+++ /dev/null".to_vec());
        return Ok(out);
    }
    // A plain edit of the path that exists on both sides afterwards.
    let b_side = if is_del { swap_prefix(minus, b'b') } else { plus.to_vec() };
    if b_side.is_empty() {
        bail!("git's diff for this file has no paths, so it can't be staged in parts.");
    }
    let a_side = swap_prefix(&b_side, b'a');
    out.push([b"diff --git ".as_slice(), &a_side, b" ", &b_side].concat());
    out.push([b"--- ".as_slice(), &a_side].concat());
    out.push([b"+++ ".as_slice(), &b_side].concat());
    Ok(out)
}

/// The text of the error for a selection git could never apply.
pub const NO_NEWLINE: &str = "This part touches the last line of the file, which has no newline at the end. Select the lines next to it too, or stage the whole hunk.";

/// Build a patch that stages (or with `reverse`, unstages) only the selected change lines.
///
/// Staging applies forward to the index: selected lines stay, an unselected `+` is left out and
/// an unselected `-` becomes context (it stays in the index). Unstaging applies in reverse: an
/// unselected `+` becomes context (it stays staged) and an unselected `-` is left out. Hunk
/// headers are counted again, and the side the patch is matched against keeps its line numbers.
/// Returns None when nothing is selected.
pub fn build_patch(fd: &FileDiff, selected: &HashSet<usize>, reverse: bool) -> Result<Option<Vec<u8>>> {
    if fd.binary {
        bail!("This is a binary file. Stage or unstage it whole.");
    }
    let mut body: Vec<u8> = vec![];
    let mut idx = 0usize;
    // Sum of (new count - old count) over the hunks already written.
    let mut offset: i64 = 0;
    for h in &fd.hunks {
        let mut out: Vec<(u8, &[u8])> = vec![];
        let mut any = false;
        let mut last_kept = false;
        for l in &h.lines {
            let c = l.first().copied().unwrap_or(b' ');
            let rest = l.get(1..).unwrap_or_default();
            match c {
                b'+' | b'-' => {
                    let sel = selected.contains(&idx);
                    idx += 1;
                    any |= sel;
                    let kind = match (sel, c, reverse) {
                        (true, _, _) => Some(c),
                        (false, b'-', false) => Some(b' '),
                        (false, b'+', true) => Some(b' '),
                        _ => None,
                    };
                    if let Some(k) = kind {
                        out.push((k, rest));
                    }
                    last_kept = kind.is_some();
                }
                b'\\' => {
                    if last_kept {
                        out.push((b'\\', rest));
                    }
                }
                _ => {
                    out.push((b' ', rest));
                    last_kept = true;
                }
            }
        }
        if !any {
            continue;
        }
        // "\ No newline" may only follow the last line of the side it belongs to.
        for (i, (k, _)) in out.iter().enumerate() {
            if *k != b'\\' || i == 0 {
                continue;
            }
            let prev = out[i - 1].0;
            let later = out[i + 1..].iter().map(|(k, _)| *k).filter(|k| *k != b'\\');
            let clash = later.into_iter().any(|k| prev == b' ' || k == b' ' || k == prev);
            if clash {
                bail!(NO_NEWLINE);
            }
        }
        let old_n = out.iter().filter(|(k, _)| *k == b' ' || *k == b'-').count() as i64;
        let new_n = out.iter().filter(|(k, _)| *k == b' ' || *k == b'+').count() as i64;
        let (old_start, new_start) = if reverse {
            let before_new = if h.new_count > 0 { h.new_start as i64 - 1 } else { h.new_start as i64 };
            let before_old = before_new - offset;
            (if old_n > 0 { before_old + 1 } else { before_old }, h.new_start as i64)
        } else {
            let before_old = if h.old_count > 0 { h.old_start as i64 - 1 } else { h.old_start as i64 };
            let before_new = before_old + offset;
            (h.old_start as i64, if new_n > 0 { before_new + 1 } else { before_new })
        };
        offset += new_n - old_n;
        body.extend_from_slice(format!("@@ -{},{} +{},{} @@", old_start.max(0), old_n, new_start.max(0), new_n).as_bytes());
        body.extend_from_slice(&h.section);
        body.push(b'\n');
        for (k, rest) in out {
            body.push(k);
            body.extend_from_slice(rest);
            body.push(b'\n');
        }
    }
    if body.is_empty() {
        return Ok(None);
    }
    // Every line selected and no rename: git's own header, so a new file unstaged in full leaves
    // the index and a deletion staged in full deletes (not an empty file either way).
    let renamed = fd.header.iter().any(|l| l.starts_with(b"rename from ") || l.starts_with(b"copy from "));
    let all = (0..change_count(fd)).all(|i| selected.contains(&i));
    let header = if all && !renamed {
        fd.header.iter().filter(|l| !l.starts_with(b"old mode ") && !l.starts_with(b"new mode ")).cloned().collect()
    } else {
        partial_header(fd, reverse)?
    };
    let mut patch = vec![];
    for l in header {
        patch.extend_from_slice(&l);
        patch.push(b'\n');
    }
    patch.extend_from_slice(&body);
    Ok(Some(patch))
}

/// Apply a patch to the index only. The working tree is never read or written by this.
pub fn apply_cached(ws: &Path, patch: &[u8], reverse: bool) -> Result<()> {
    let mut args = vec!["apply", "--cached", "--whitespace=nowarn"];
    if reverse {
        args.push("--reverse");
    }
    args.push("-");
    git_bytes(ws, None, &args, Some(patch)).map(|_| ())
}

fn has_head(ws: &Path) -> bool {
    git(ws, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok()
}

/// Stage one whole file: its content, mode, deletion, or a new file.
pub fn stage_file(ws: &Path, path: &str) -> Result<()> {
    git(ws, &["add", "-A", "--", &literal(path)]).map(|_| ())
}

/// Unstage one whole file (both paths of a rename). The working tree keeps every change.
pub fn unstage_file(ws: &Path, path: &str, old: Option<&str>) -> Result<()> {
    let mut specs = vec![literal(path)];
    if let Some(o) = old.filter(|o| *o != path) {
        specs.push(literal(o));
    }
    let mut args: Vec<&str> = vec![];
    if has_head(ws) {
        args.extend(["reset", "-q", "--"]);
    } else {
        args.extend(["rm", "--cached", "-q", "-r", "--ignore-unmatch", "--"]);
    }
    args.extend(specs.iter().map(String::as_str));
    git(ws, &args).map(|_| ())
}

/// Stage or unstage part of one file: the change lines numbered in `lines` of the diff the user
/// saw. `expected` is that diff's `sig`; if the file changed since, nothing happens.
pub fn apply_lines(ws: &Path, path: &str, staged: bool, old: Option<&str>, expected: &str, lines: &[usize]) -> Result<()> {
    let d = diff(ws, path, staged, old)?;
    if sig(&d) != expected {
        bail!("{path} changed since you looked at it. Check the lines again, then stage them.");
    }
    let fd = parse(&d);
    let selected: HashSet<usize> = lines.iter().copied().collect();
    match build_patch(&fd, &selected, staged)? {
        Some(p) => apply_cached(ws, &p, staged),
        None => bail!("No lines are selected."),
    }
}

// ---------- commits ----------

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct HeadInfo {
    pub sha: String,
    pub short: String,
    pub subject: String,
    pub message: String,
    pub pushed: bool,
    /// Why Amend and Undo are off, in words. None: both are allowed.
    pub block: Option<String>,
}

/// Is HEAD on a remote already? Checked against the branch's upstream first, then against every
/// remote-tracking branch (an agent may push without setting one).
pub fn pushed(ws: &Path) -> bool {
    if let Ok(up) = git(ws, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"])
        && git(ws, &["merge-base", "--is-ancestor", "HEAD", &up]).is_ok()
    {
        return true;
    }
    git(ws, &["for-each-ref", "--contains", "HEAD", "--count=1", "--format=%(refname)", "refs/remotes"])
        .map(|o| !o.is_empty())
        .unwrap_or(false)
}

/// The last commit, and whether it may be amended or undone.
pub fn head_info(ws: &Path, target: &str) -> Result<Option<HeadInfo>> {
    if !has_head(ws) {
        return Ok(None);
    }
    let out = git(ws, &["log", "-1", "--format=%H%x00%h%x00%P%x00%B"])?;
    let mut f = out.splitn(4, '\0');
    let (sha, short, parents, message) = (f.next().unwrap_or(""), f.next().unwrap_or(""), f.next().unwrap_or(""), f.next().unwrap_or("").trim().to_string());
    let is_pushed = pushed(ws);
    let n_parents = parents.split_whitespace().count();
    let on_target = !target.is_empty()
        && git(ws, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{target}")]).is_ok()
        && git(ws, &["merge-base", "--is-ancestor", "HEAD", target]).is_ok();
    let block = if is_pushed {
        Some("The last commit is already pushed. Changing it would rewrite what's on the remote.".to_string())
    } else if on_target {
        Some(format!("The last commit is already on {target}, so it isn't this workspace's to change."))
    } else if n_parents == 0 {
        Some("This is the repo's first commit.".to_string())
    } else if n_parents > 1 {
        Some("The last commit is a merge. Change it in the terminal if you need to.".to_string())
    } else {
        None
    };
    Ok(Some(HeadInfo {
        sha: sha.into(),
        short: short.into(),
        subject: message.lines().next().unwrap_or("").to_string(),
        message,
        pushed: is_pushed,
        block,
    }))
}

/// Commit what is staged. With `amend`, replaces the last commit (refused once it's pushed).
/// Returns the new commit's short hash.
pub fn commit(ws: &Path, message: &str, amend: bool, target: &str) -> Result<String> {
    let msg = message.trim();
    if msg.is_empty() {
        bail!("Write a commit message first.");
    }
    if amend {
        match head_info(ws, target)? {
            None => bail!("There's no commit to amend yet."),
            Some(h) => {
                if let Some(why) = h.block {
                    bail!(why);
                }
            }
        }
    } else if git(ws, &["diff", "--cached", "--quiet"]).is_ok() && has_head(ws) {
        bail!("Nothing is staged. Stage a file, a hunk or some lines first.");
    }
    let mut args = vec!["commit", "-q", "--cleanup=whitespace", "-F", "-"];
    if amend {
        args.push("--amend");
    }
    git_bytes(ws, None, &args, Some(format!("{msg}\n").as_bytes()))?;
    git(ws, &["rev-parse", "--short", "HEAD"])
}

/// Undo the last commit with a soft reset: its changes come back staged, nothing is lost.
/// Returns the subject of the commit that was undone.
pub fn undo(ws: &Path, target: &str) -> Result<String> {
    let Some(h) = head_info(ws, target)? else { bail!("There's no commit to undo.") };
    if let Some(why) = h.block {
        bail!(why);
    }
    git(ws, &["reset", "-q", "--soft", "HEAD~1"])?;
    Ok(h.subject)
}

// ---------- messages without the agent ----------

/// A conventional commit message made from the staged file list alone, for when the agent is
/// busy or can't answer. Plain and honest: it names what changed, not why.
pub fn local_message(files: &[StageFile]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let is_test = |p: &str| p.contains("test") || p.contains("spec") || p.contains("__tests__");
    let is_doc = |p: &str| p.ends_with(".md") || p.ends_with(".mdx") || p.ends_with(".txt") || p.starts_with("docs/");
    let kind = if files.iter().all(|f| is_test(&f.path)) {
        "test"
    } else if files.iter().all(|f| is_doc(&f.path)) {
        "docs"
    } else if files.iter().all(|f| f.status == "A" || f.status == "?") {
        "feat"
    } else {
        "chore"
    };
    // Scope: the one top folder every file sits in, if there is one.
    let tops: HashSet<&str> = files.iter().map(|f| f.path.split('/').next().unwrap_or("")).collect();
    let scope = match (tops.len(), files.iter().all(|f| f.path.contains('/'))) {
        (1, true) => format!("({})", tops.into_iter().next().unwrap_or("")),
        _ => String::new(),
    };
    let name = |f: &StageFile| f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
    let verb = if files.iter().all(|f| f.status == "A" || f.status == "?") {
        "add"
    } else if files.iter().all(|f| f.status == "D") {
        "remove"
    } else if files.iter().all(|f| f.status == "R") {
        "rename"
    } else {
        "update"
    };
    let what = match files.len() {
        1 => name(&files[0]),
        2 => format!("{} and {}", name(&files[0]), name(&files[1])),
        n => format!("{n} files"),
    };
    let mut msg = format!("{kind}{scope}: {verb} {what}");
    if files.len() > 1 {
        msg.push_str("\n\n");
        for f in files {
            let what = match f.status.as_str() {
                "A" | "?" => "added".to_string(),
                "D" => "removed".to_string(),
                "R" => format!("renamed from {}", f.old_path.as_deref().unwrap_or("?")),
                _ if f.binary => "changed".to_string(),
                _ => format!("+{} -{}", f.additions, f.deletions),
            };
            msg.push_str(&format!("- {}: {what}\n", f.path));
        }
    }
    msg.trim_end().to_string()
}

/// Pull a commit message out of an agent's reply: code fences and a leading "Commit message:"
/// label go, the rest is kept as written.
pub fn clean_reply(reply: &str) -> String {
    let mut t = reply.trim();
    if let Some(start) = t.find("```") {
        let after = &t[start + 3..];
        let after = after.split_once('\n').map(|(_, r)| r).unwrap_or(after);
        t = after.split("```").next().unwrap_or(after).trim();
    }
    let t = t.trim_start_matches(|c: char| c == '`' || c.is_whitespace());
    let lower = t.to_lowercase();
    let t = if lower.starts_with("commit message:") { t["commit message:".len()..].trim_start() } else { t };
    t.trim().trim_matches('`').trim().to_string()
}

// ---------- split into commits ----------

/// One hunk of the uncommitted work, as "Split into commits" lists it.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct SplitHunk {
    pub path: String,
    /// Finds this hunk again after earlier groups are committed (its lines don't change).
    pub key: String,
    pub header: String,
    /// The first lines of the hunk, for the review sheet and the agent.
    pub preview: String,
    pub additions: u32,
    pub deletions: u32,
    /// Stages the whole file: binary files and changes with no lines (a mode change).
    pub whole: bool,
    pub protected: bool,
}

fn hunk_key(h: &Hunk) -> String {
    let mut all = vec![];
    for l in &h.lines {
        all.extend_from_slice(l);
        all.push(b'\n');
    }
    sig(&all)
}

/// Every hunk of the uncommitted work against HEAD (staged or not, untracked included), file by
/// file. This is what the index looks like after it is reset to HEAD, which `split_commit` does.
pub fn split_hunks(ws: &Path) -> Result<Vec<SplitHunk>> {
    let mut args: Vec<&str> = CFG.to_vec();
    args.extend(["diff", "HEAD", "--name-only", "-z", "--no-renames"]);
    let names = git_bytes(ws, None, &args, None)?;
    let mut paths: Vec<String> = names.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).to_string()).collect();
    let untracked = git_bytes(ws, None, &["ls-files", "-z", "--others", "--exclude-standard"], None)?;
    paths.extend(untracked.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).to_string()));
    let mut out = vec![];
    for path in paths {
        let d = head_diff(ws, &path)?;
        let fd = parse(&d);
        if fd.binary || fd.hunks.is_empty() {
            out.push(SplitHunk { key: format!("file:{}", sig(&d)), header: if fd.binary { "Binary file".into() } else { "File mode".into() }, preview: String::new(), additions: 0, deletions: 0, whole: true, protected: false, path });
            continue;
        }
        for h in &fd.hunks {
            let count = |c: u8| h.lines.iter().filter(|l| l.first() == Some(&c)).count() as u32;
            let preview: Vec<String> = h.lines.iter().take(40).map(|l| String::from_utf8_lossy(l).to_string()).collect();
            out.push(SplitHunk {
                path: path.clone(),
                key: hunk_key(h),
                header: format!("@@ -{},{} +{},{} @@{}", h.old_start, h.old_count, h.new_start, h.new_count, String::from_utf8_lossy(&h.section)),
                preview: preview.join("\n"),
                additions: count(b'+'),
                deletions: count(b'-'),
                whole: false,
                protected: false,
            });
        }
    }
    Ok(out)
}

/// One file's diff against HEAD, all of it new when HEAD doesn't have it.
fn head_diff(ws: &Path, path: &str) -> Result<Vec<u8>> {
    let mut args: Vec<String> = CFG.iter().map(|s| s.to_string()).collect();
    args.push("diff".into());
    args.extend(OPTS.iter().map(|s| s.to_string()));
    args.extend(["HEAD".into(), "--no-renames".into(), "--".into(), literal(path)]);
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let d = git_bytes(ws, None, &a, None)?;
    if !d.is_empty() {
        return Ok(d);
    }
    diff(ws, path, false, None)
}

/// Commit one group of a split: the index goes back to HEAD (files are untouched), the group's
/// hunks are staged, and they are committed with `message`. Hunks are found by their `key`, so
/// groups committed before don't move them. Returns the new commit's short hash.
pub fn split_commit(ws: &Path, message: &str, hunks: &[(String, String)]) -> Result<String> {
    if message.trim().is_empty() {
        bail!("Each commit needs a message.");
    }
    if hunks.is_empty() {
        bail!("This commit has no changes in it.");
    }
    if !has_head(ws) {
        bail!("Split into commits needs at least one commit in the repo.");
    }
    git(ws, &["reset", "-q"])?;
    let mut files: Vec<&str> = hunks.iter().map(|(p, _)| p.as_str()).collect();
    files.dedup();
    let mut seen = HashSet::new();
    files.retain(|f| seen.insert(*f));
    for path in files {
        let keys: Vec<&str> = hunks.iter().filter(|(p, _)| p == path).map(|(_, k)| k.as_str()).collect();
        if keys.iter().any(|k| k.starts_with("file:")) {
            stage_file(ws, path)?;
            continue;
        }
        let d = diff(ws, path, false, None)?;
        let fd = parse(&d);
        let mut used = HashSet::new();
        let mut lines = HashSet::new();
        for k in &keys {
            let Some(i) = (0..fd.hunks.len()).find(|i| !used.contains(i) && hunk_key(&fd.hunks[*i]) == *k) else {
                bail!("A change in {path} isn't there anymore. The files changed since the plan was made, so make a new plan.");
            };
            used.insert(i);
            lines.extend(hunk_lines(&fd, i));
        }
        if let Some(p) = build_patch(&fd, &lines, false)? {
            apply_cached(ws, &p, false)?;
        }
    }
    commit(ws, message, false, "")
}

/// One proposed commit of a split: its message and its hunks (indexes into `split_hunks`).
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct SplitGroup {
    pub message: String,
    pub hunks: Vec<usize>,
}

/// Read the agent's plan: `{"commits":[{"message":"...","hunks":[1,3]}]}`, hunks numbered from 1
/// as the prompt lists them. Each hunk lands in one commit only (its first); unknown numbers are
/// dropped, empty commits go, and anything the agent left out ends up in a last commit for the
/// user to name. None: the reply held no plan.
pub fn parse_plan(reply: &str, n: usize) -> Option<Vec<SplitGroup>> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let v: serde_json::Value = serde_json::from_str(reply.get(start..=end)?).ok()?;
    let commits = v["commits"].as_array()?;
    let mut taken = HashSet::new();
    let mut out = vec![];
    for c in commits {
        let message = c["message"].as_str().unwrap_or("").trim().to_string();
        let hunks: Vec<usize> = c["hunks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|h| h.as_u64().map(|x| x as usize).or_else(|| h.as_str().and_then(|s| s.trim_start_matches(['H', 'h', '#']).parse().ok())))
            .filter(|h| *h >= 1 && *h <= n && taken.insert(*h))
            .map(|h| h - 1)
            .collect();
        if !hunks.is_empty() {
            out.push(SplitGroup { message, hunks });
        }
    }
    if out.is_empty() {
        return None;
    }
    let left: Vec<usize> = (0..n).filter(|i| !taken.contains(&(i + 1))).collect();
    if !left.is_empty() {
        out.push(SplitGroup { message: String::new(), hunks: left });
    }
    Some(out)
}

/// A split without the agent: one commit per top folder, messages from the file list.
pub fn local_plan(hunks: &[SplitHunk]) -> Vec<SplitGroup> {
    let mut order: Vec<String> = vec![];
    let mut groups: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
    for (i, h) in hunks.iter().enumerate() {
        let top = h.path.split_once('/').map(|(t, _)| t.to_string()).unwrap_or_default();
        if !groups.contains_key(&top) {
            order.push(top.clone());
        }
        groups.entry(top).or_default().push(i);
    }
    order
        .into_iter()
        .map(|top| {
            let idx = groups.remove(&top).unwrap_or_default();
            let mut files: Vec<StageFile> = vec![];
            for &i in &idx {
                let h = &hunks[i];
                match files.iter_mut().find(|f| f.path == h.path) {
                    Some(f) => {
                        f.additions += h.additions;
                        f.deletions += h.deletions;
                    }
                    None => files.push(StageFile {
                        path: h.path.clone(),
                        old_path: None,
                        status: if h.header.starts_with("@@ -0,0 ") { "A".into() } else { "M".into() },
                        additions: h.additions,
                        deletions: h.deletions,
                        binary: h.whole,
                        protected: false,
                    }),
                }
            }
            SplitGroup { message: local_message(&files), hunks: idx }
        })
        .collect()
}

const DIFF_BUDGET: usize = 60_000;

fn clip_to(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// The side question for "Draft message": the staged diff in, one commit message out.
pub fn draft_prompt(stat: &str, diff: &str) -> String {
    let clipped = clip_to(diff, DIFF_BUDGET);
    let more = if clipped.len() < diff.len() { "\n[The diff is cut here. The file list above is complete.]" } else { "" };
    format!(
        "Write a git commit message for the staged changes below. Use the Conventional Commits format: `type(scope): summary` (types: feat, fix, refactor, docs, test, chore, perf, build, ci, style), a subject of 72 characters or fewer in the imperative mood, then a blank line and a short body that says what changed and why, only if it helps. Don't run any tools and don't change any files. Reply with the message only, no code fences, no preamble.\n\nFiles:\n{stat}\n\nDiff:\n{clipped}{more}"
    )
}

/// The side question for "Split into commits": numbered hunks in, a JSON plan out.
pub fn split_prompt(hunks: &[SplitHunk]) -> String {
    let mut list = String::new();
    for (i, h) in hunks.iter().enumerate() {
        let entry = if h.whole {
            format!("[{}] {} ({}, committed whole)\n", i + 1, h.path, h.header.to_lowercase())
        } else {
            format!("[{}] {} {}\n{}\n", i + 1, h.path, h.header, h.preview)
        };
        if list.len() + entry.len() > DIFF_BUDGET {
            list.push_str(&format!("[{}] to [{}]: more hunks, cut for length. Leave them out; the user places them.\n", i + 1, hunks.len()));
            break;
        }
        list.push_str(&entry);
    }
    format!(
        "Split these uncommitted changes into a few focused commits, in the order they should be committed (each commit should build on the ones before it). Every hunk goes in exactly one commit. Give each commit a Conventional Commits message: `type(scope): summary`, subject of 72 characters or fewer, imperative mood, and a short body only if it helps. Don't run any tools and don't change any files.\n\nReply with JSON only, in exactly this shape:\n{{\"commits\": [{{\"message\": \"feat(api): add retries\", \"hunks\": [1, 3]}}]}}\n\nHunks:\n{list}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_plans_from_the_agent_are_checked() {
        let reply = "Here you go:\n```json\n{\"commits\":[{\"message\":\"fix: a\",\"hunks\":[2, 9, 2]},{\"message\":\"feat: b\",\"hunks\":[\"H1\", 2]},{\"message\":\"empty\",\"hunks\":[]}]}\n```";
        let plan = parse_plan(reply, 4).unwrap();
        assert_eq!(
            plan,
            vec![
                SplitGroup { message: "fix: a".into(), hunks: vec![1] },
                SplitGroup { message: "feat: b".into(), hunks: vec![0] },
                SplitGroup { message: "".into(), hunks: vec![2, 3] },
            ]
        );
        assert!(parse_plan("no plan here", 3).is_none());
        assert!(parse_plan("{\"commits\": []}", 3).is_none());

        let h = |p: &str| SplitHunk { path: p.into(), key: "k".into(), header: "@@ -1,1 +1,2 @@".into(), preview: String::new(), additions: 1, deletions: 0, whole: false, protected: false };
        let local = local_plan(&[h("src/a.ts"), h("docs/x.md"), h("src/b.ts")]);
        assert_eq!(local.len(), 2);
        assert_eq!(local[0].hunks, vec![0, 2]);
        assert_eq!(local[0].message.lines().next().unwrap(), "chore(src): update a.ts and b.ts");
        assert_eq!(local[1].message, "docs(docs): update x.md");
        assert!(split_prompt(&[h("src/a.ts")]).contains("[1] src/a.ts @@ -1,1 +1,2 @@"));
        let big = "x".repeat(DIFF_BUDGET + 10);
        assert!(draft_prompt("a | 1", &big).contains("The diff is cut here"));
    }
    use crate::git::tests::init_repo;

    fn write(r: &Path, p: &str, s: &[u8]) {
        if let Some(d) = Path::new(p).parent() {
            std::fs::create_dir_all(r.join(d)).unwrap();
        }
        std::fs::write(r.join(p), s).unwrap();
    }
    fn commit_all(r: &Path, m: &str) {
        git(r, &["add", "-A"]).unwrap();
        git(r, &["commit", "-qm", m]).unwrap();
    }
    fn index_of(r: &Path, p: &str) -> Vec<u8> {
        git_bytes(r, None, &["show", &format!(":{p}")], None).unwrap()
    }
    /// Stage the given change lines of the unstaged diff of `p`.
    fn stage(r: &Path, p: &str, lines: &[usize]) -> Result<()> {
        let d = diff(r, p, false, None).unwrap();
        apply_lines(r, p, false, None, &sig(&d), lines)
    }
    fn unstage(r: &Path, p: &str, old: Option<&str>, lines: &[usize]) -> Result<()> {
        let d = diff(r, p, true, old).unwrap();
        apply_lines(r, p, true, old, &sig(&d), lines)
    }

    const TEN: &str = "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16\nl17\nl18\nl19\nl20\n";

    #[test]
    fn hunks_and_lines_stage_without_touching_the_working_tree() {
        let repo = init_repo();
        let r = repo.path();
        write(r, "f.txt", TEN.as_bytes());
        commit_all(r, "ten");
        let edited = TEN.replace("l2\n", "two\n").replace("l18\n", "eighteen\nextra\n");
        write(r, "f.txt", edited.as_bytes());
        let fd = parse(&diff(r, "f.txt", false, None).unwrap());
        assert_eq!(fd.hunks.len(), 2);
        assert_eq!(change_count(&fd), 5); // -2 +two, -18 +eighteen +extra

        // Second hunk only, with the earlier hunk left out: line numbers still line up.
        stage(r, "f.txt", &hunk_lines(&fd, 1)).unwrap();
        let idx = String::from_utf8(index_of(r, "f.txt")).unwrap();
        assert!(idx.contains("eighteen\nextra\n") && idx.contains("\nl2\n"), "{idx}");
        assert_eq!(std::fs::read_to_string(r.join("f.txt")).unwrap(), edited, "the file itself is untouched");

        // One line of what's left: the "+two" without removing "2" (the "-2" becomes context).
        let fd = parse(&diff(r, "f.txt", false, None).unwrap());
        assert_eq!(change_count(&fd), 2);
        stage(r, "f.txt", &[1]).unwrap();
        let idx = String::from_utf8(index_of(r, "f.txt")).unwrap();
        assert!(idx.starts_with("l1\nl2\ntwo\nl3\n"), "{idx}");

        // Unstage one line of the staged diff: "+extra" goes, "eighteen" stays staged.
        let fd = parse(&diff(r, "f.txt", true, None).unwrap());
        let extra = fd.hunks.iter().flat_map(|h| &h.lines).filter(|l| matches!(l[0], b'+' | b'-')).position(|l| l == b"+extra").unwrap();
        unstage(r, "f.txt", None, &[extra]).unwrap();
        let idx = String::from_utf8(index_of(r, "f.txt")).unwrap();
        assert!(idx.contains("eighteen\nl19") && !idx.contains("extra"), "{idx}");
        assert_eq!(std::fs::read_to_string(r.join("f.txt")).unwrap(), edited);
    }

    #[test]
    fn a_stale_view_is_refused() {
        let repo = init_repo();
        let r = repo.path();
        write(r, "a.txt", b"one\ntwo\n");
        let old = sig(&diff(r, "a.txt", false, None).unwrap());
        write(r, "a.txt", b"one\nthree\n");
        assert!(apply_lines(r, "a.txt", false, None, &old, &[0]).unwrap_err().to_string().contains("changed since"));
        assert_eq!(index_of(r, "a.txt"), b"one\n");
    }

    #[test]
    fn untracked_files_stage_in_parts_and_new_files_unstage_in_parts() {
        let repo = init_repo();
        let r = repo.path();
        write(r, "src/new.rs", b"a\nb\nc\n");
        stage(r, "src/new.rs", &[0, 2]).unwrap();
        assert_eq!(index_of(r, "src/new.rs"), b"a\nc\n");
        // Now tracked: the rest is a normal edit.
        stage(r, "src/new.rs", &[0]).unwrap();
        assert_eq!(index_of(r, "src/new.rs"), b"a\nb\nc\n");
        // Unstage one line of a staged new file: it stays staged, one line shorter.
        unstage(r, "src/new.rs", None, &[1]).unwrap();
        assert_eq!(index_of(r, "src/new.rs"), b"a\nc\n");
        // Unstage everything left of it, line by line: then it's gone from the index.
        unstage(r, "src/new.rs", None, &[0, 1]).unwrap();
        assert!(!tracked(r, "src/new.rs").unwrap());
        assert_eq!(std::fs::read(r.join("src/new.rs")).unwrap(), b"a\nb\nc\n");
    }

    #[test]
    fn deletions_stage_in_parts() {
        let repo = init_repo();
        let r = repo.path();
        write(r, "gone.txt", b"x\ny\nz\n");
        commit_all(r, "gone");
        std::fs::remove_file(r.join("gone.txt")).unwrap();
        stage(r, "gone.txt", &[1]).unwrap();
        assert_eq!(index_of(r, "gone.txt"), b"x\nz\n");
        stage_file(r, "gone.txt").unwrap();
        assert!(!tracked(r, "gone.txt").unwrap());
        // Unstage one line of a staged deletion: the file comes back with that line only.
        unstage(r, "gone.txt", None, &[0]).unwrap();
        assert_eq!(index_of(r, "gone.txt"), b"x\n");
        assert!(!r.join("gone.txt").exists());
    }

    #[test]
    fn renames_keep_the_rename_when_part_of_it_is_unstaged() {
        let repo = init_repo();
        let r = repo.path();
        let body = "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\n";
        write(r, "old name.txt", body.as_bytes());
        commit_all(r, "file");
        git(r, &["mv", "old name.txt", "new name.txt"]).unwrap();
        write(r, "new name.txt", body.replace("beta", "BETA").as_bytes());
        git(r, &["add", "new name.txt"]).unwrap();
        let st = status(r).unwrap();
        let f = st.staged.iter().find(|f| f.path == "new name.txt").unwrap();
        assert_eq!((f.status.as_str(), f.old_path.as_deref()), ("R", Some("old name.txt")));
        let fd = parse(&diff(r, "new name.txt", true, Some("old name.txt")).unwrap());
        assert!(fd.header.iter().any(|l| l.starts_with(b"rename from")));
        unstage(r, "new name.txt", Some("old name.txt"), &hunk_lines(&fd, 0)).unwrap();
        // Still renamed, content back to the original.
        assert_eq!(String::from_utf8(index_of(r, "new name.txt")).unwrap(), body);
        assert!(!tracked(r, "old name.txt").unwrap());
        // Whole-file unstage of a rename puts both paths back.
        unstage_file(r, "new name.txt", Some("old name.txt")).unwrap();
        assert!(tracked(r, "old name.txt").unwrap() && !tracked(r, "new name.txt").unwrap());
        assert!(r.join("new name.txt").exists());
    }

    #[test]
    fn binary_files_only_move_whole() {
        let repo = init_repo();
        let r = repo.path();
        write(r, "img.bin", &[0, 1, 2, 3]);
        commit_all(r, "bin");
        write(r, "img.bin", &[0, 9, 9]);
        let st = status(r).unwrap();
        assert!(st.unstaged.iter().find(|f| f.path == "img.bin").unwrap().binary);
        let fd = parse(&diff(r, "img.bin", false, None).unwrap());
        assert!(fd.binary);
        assert!(build_patch(&fd, &HashSet::from([0]), false).unwrap_err().to_string().contains("binary"));
        stage_file(r, "img.bin").unwrap();
        assert_eq!(index_of(r, "img.bin"), vec![0, 9, 9]);
        unstage_file(r, "img.bin", None).unwrap();
        assert_eq!(index_of(r, "img.bin"), vec![0, 1, 2, 3]);
        write(r, "new.bin", &[0, 7]);
        assert!(status(r).unwrap().unstaged.iter().find(|f| f.path == "new.bin").unwrap().binary);
    }

    #[test]
    fn no_newline_at_end_of_file() {
        let repo = init_repo();
        let r = repo.path();
        write(r, "n.txt", b"a\nb");
        commit_all(r, "no newline");
        // Change the last line: both sides end without a newline. Whole hunk works.
        write(r, "n.txt", b"a\nB");
        stage(r, "n.txt", &[0, 1]).unwrap();
        assert_eq!(index_of(r, "n.txt"), b"a\nB");
        git(r, &["reset", "-q"]).unwrap();
        // Only the removal: the file ends at "a\n".
        stage(r, "n.txt", &[0]).unwrap();
        assert_eq!(index_of(r, "n.txt"), b"a\n");
        git(r, &["reset", "-q"]).unwrap();
        // Only the addition can't be expressed: refused with a plain reason, index unchanged.
        let err = stage(r, "n.txt", &[1]).unwrap_err().to_string();
        assert_eq!(err, NO_NEWLINE);
        assert_eq!(index_of(r, "n.txt"), b"a\nb");

        // Append lines to a file with no final newline, stage the first new line only.
        write(r, "n.txt", b"a\nb\nc\nd\n");
        let fd = parse(&diff(r, "n.txt", false, None).unwrap());
        assert_eq!(change_count(&fd), 4); // -b, +b, +c, +d
        stage(r, "n.txt", &[0, 1, 2]).unwrap();
        assert_eq!(index_of(r, "n.txt"), b"a\nb\nc\n");
        // A new file with no final newline stages whole through lines.
        write(r, "m.txt", b"x\ny");
        stage(r, "m.txt", &[0, 1]).unwrap();
        assert_eq!(index_of(r, "m.txt"), b"x\ny");
    }

    #[test]
    fn crlf_lines_stay_crlf() {
        let repo = init_repo();
        let r = repo.path();
        git(r, &["config", "core.autocrlf", "false"]).unwrap();
        write(r, "w.txt", b"one\r\ntwo\r\nthree\r\n");
        commit_all(r, "crlf");
        write(r, "w.txt", b"one\r\nTWO\r\nthree\r\nfour\r\n");
        let fd = parse(&diff(r, "w.txt", false, None).unwrap());
        assert_eq!(change_count(&fd), 3); // -two +TWO +four
        stage(r, "w.txt", &[2]).unwrap();
        assert_eq!(index_of(r, "w.txt"), b"one\r\ntwo\r\nthree\r\nfour\r\n");
        stage(r, "w.txt", &[0, 1]).unwrap();
        assert_eq!(index_of(r, "w.txt"), b"one\r\nTWO\r\nthree\r\nfour\r\n");
        unstage(r, "w.txt", None, &[0, 1]).unwrap();
        assert_eq!(index_of(r, "w.txt"), b"one\r\ntwo\r\nthree\r\nfour\r\n");
    }

    #[test]
    fn odd_names_and_user_diff_config_dont_break_patches() {
        let repo = init_repo();
        let r = repo.path();
        git(r, &["config", "diff.noprefix", "true"]).unwrap();
        git(r, &["config", "diff.mnemonicPrefix", "true"]).unwrap();
        write(r, "café [id].txt", b"1\n2\n");
        commit_all(r, "odd");
        write(r, "café [id].txt", b"1\nzwei\n");
        write(r, "c.txt", b"glob trap\n");
        stage(r, "café [id].txt", &[0, 1]).unwrap();
        assert_eq!(index_of(r, "café [id].txt"), b"1\nzwei\n");
        unstage_file(r, "café [id].txt", None).unwrap();
        assert_eq!(index_of(r, "café [id].txt"), b"1\n2\n");
        stage_file(r, "café [id].txt").unwrap();
        assert!(!tracked(r, "c.txt").unwrap(), "a name with brackets is never a pattern");
    }

    #[test]
    fn commit_amend_and_undo_refuse_once_pushed() {
        let origin = init_repo();
        let holder = tempfile::tempdir().unwrap();
        let c = holder.path().join("c");
        git(origin.path(), &["clone", "-q", &origin.path().to_string_lossy(), &c.to_string_lossy()]).unwrap();
        git(&c, &["config", "user.email", "t@example.com"]).unwrap();
        git(&c, &["config", "user.name", "T"]).unwrap();
        git(&c, &["checkout", "-qb", "feature"]).unwrap();

        // HEAD is main's commit: it's on the remote, so neither amend nor undo.
        let h = head_info(&c, "main").unwrap().unwrap();
        assert!(h.pushed && h.block.is_some());
        assert!(commit(&c, "x", true, "main").is_err());

        assert!(commit(&c, "feat: x", false, "main").unwrap_err().to_string().contains("Nothing is staged"));
        write(&c, "b.txt", b"b\n");
        stage_file(&c, "b.txt").unwrap();
        commit(&c, "feat: add b\n\n#123 stays in the body", false, "main").unwrap();
        assert_eq!(git(&c, &["log", "-1", "--format=%B"]).unwrap().trim(), "feat: add b\n\n#123 stays in the body");
        let h = head_info(&c, "main").unwrap().unwrap();
        assert!(!h.pushed && h.block.is_none(), "{h:?}");

        // Amend with more staged.
        write(&c, "c.txt", b"c\n");
        stage_file(&c, "c.txt").unwrap();
        commit(&c, "feat: add b and c", true, "main").unwrap();
        assert_eq!(git(&c, &["rev-list", "--count", "main..HEAD"]).unwrap(), "1");
        assert_eq!(head_info(&c, "main").unwrap().unwrap().subject, "feat: add b and c");

        // Pushed (without -u, so only the remote-tracking ref knows): amend and undo refuse.
        git(&c, &["push", "-q", "origin", "feature"]).unwrap();
        let h = head_info(&c, "main").unwrap().unwrap();
        assert!(h.pushed && h.block.as_deref().unwrap().contains("pushed"));
        write(&c, "d.txt", b"d\n");
        stage_file(&c, "d.txt").unwrap();
        assert!(commit(&c, "amend me", true, "main").unwrap_err().to_string().contains("pushed"));
        assert!(undo(&c, "main").is_err());
        // With an upstream set, the same.
        git(&c, &["branch", "-q", "--set-upstream-to", "origin/feature"]).unwrap();
        assert!(pushed(&c));

        // A new local commit on top can be undone: its changes come back staged.
        commit(&c, "feat: add d", false, "main").unwrap();
        assert_eq!(undo(&c, "main").unwrap(), "feat: add d");
        assert_eq!(git(&c, &["diff", "--cached", "--name-only"]).unwrap(), "d.txt");
        assert!(c.join("d.txt").exists());
    }

    #[test]
    fn local_messages_are_conventional_and_plain() {
        let f = |p: &str, s: &str| StageFile { path: p.into(), old_path: None, status: s.into(), additions: 3, deletions: 1, binary: false, protected: false };
        assert_eq!(local_message(&[f("src/a.ts", "M")]), "chore(src): update a.ts");
        assert_eq!(local_message(&[f("README.md", "M")]), "docs: update README.md");
        assert_eq!(local_message(&[f("src/x.ts", "A"), f("src/y.ts", "?")]), "feat(src): add x.ts and y.ts\n\n- src/x.ts: added\n- src/y.ts: added");
        assert!(local_message(&[f("a.ts", "M"), f("b/c.ts", "D"), f("tests/t.ts", "M")]).starts_with("chore: update 3 files\n\n- a.ts: +3 -1"));
        assert_eq!(local_message(&[]), "");
        assert_eq!(clean_reply("```\nfix(api): retry on 502\n\nBody.\n```"), "fix(api): retry on 502\n\nBody.");
        assert_eq!(clean_reply("Commit message: feat: x"), "feat: x");
    }

    #[test]
    fn split_commits_groups_in_order() {
        let repo = init_repo();
        let r = repo.path();
        write(r, "f.txt", TEN.as_bytes());
        commit_all(r, "ten");
        write(r, "f.txt", TEN.replace("l2\n", "two\n").replace("l18\n", "eighteen\n").as_bytes());
        write(r, "new.txt", b"new\n");
        write(r, "img.bin", &[0, 1]);
        git(r, &["add", "new.txt"]).unwrap(); // staged by hand: split starts from HEAD anyway
        let hunks = split_hunks(r).unwrap();
        assert_eq!(hunks.len(), 4, "{hunks:?}");
        let pick = |p: &str, n: usize| hunks.iter().filter(|h| h.path == p).nth(n).map(|h| (h.path.clone(), h.key.clone())).unwrap();
        assert!(hunks.iter().any(|h| h.path == "img.bin" && h.whole));

        // Second hunk of f.txt first, then the rest.
        split_commit(r, "fix: eighteen", &[pick("f.txt", 1)]).unwrap();
        assert_eq!(git(r, &["show", "--stat", "--format=", "HEAD"]).unwrap().lines().count(), 2);
        assert!(git(r, &["show", "HEAD:f.txt"]).unwrap().contains("\nl2\n"));
        split_commit(r, "feat: two, new and image", &[pick("f.txt", 0), pick("new.txt", 0), pick("img.bin", 0)]).unwrap();
        assert_eq!(git(r, &["status", "--porcelain"]).unwrap(), "");
        assert_eq!(git(r, &["log", "--format=%s", "-3"]).unwrap(), "feat: two, new and image\nfix: eighteen\nten");
        // A key that's gone is refused.
        assert!(split_commit(r, "x", &[("f.txt".into(), "nope".into())]).is_err());
    }
}
