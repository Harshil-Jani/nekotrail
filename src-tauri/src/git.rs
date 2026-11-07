//! Thin wrappers over the `git` CLI. Every workspace is a git worktree on its own branch.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Every git this app runs: in `dir`, with no prompts, stopped at its deadline (proc.rs), with
/// `stdin` fed while its output is read.
pub(crate) fn run(dir: &Path, args: &[&str], stdin: Option<&[u8]>, env: &[(&str, &str)]) -> std::io::Result<std::process::Output> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    crate::proc::no_prompts(&mut cmd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    crate::proc::output(&mut cmd, stdin, crate::proc::git_limit(args))
}

pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = run(dir, args, None, &[]).with_context(|| format!("running git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Top-level directory of the repo containing `dir`.
pub fn toplevel(dir: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?))
}

/// The branch new workspaces start from: origin's HEAD if known, else main/master, else the current branch.
pub fn default_branch(repo: &Path) -> Result<String> {
    if let Ok(r) = git(repo, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        && let Some(b) = r.strip_prefix("origin/") {
            return Ok(b.to_string());
        }
    for b in ["main", "master"] {
        if git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{b}")]).is_ok() {
            return Ok(b.to_string());
        }
    }
    git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
}

pub fn remote_url(repo: &Path) -> Option<String> {
    git(repo, &["remote", "get-url", "origin"]).ok().filter(|s| !s.is_empty())
}

/// Start point for a new workspace: `origin/<base>` when it exists (fresh), else the local branch.
fn start_point(repo: &Path, base: &str) -> String {
    let remote = format!("origin/{base}");
    if git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/{remote}")]).is_ok() {
        remote
    } else {
        base.to_string()
    }
}

pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok()
}

/// Create a worktree at `path` on a new `branch` from `base`.
pub fn add_worktree(repo: &Path, path: &Path, branch: &str, base: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = git(repo, &["fetch", "--quiet", "origin", base]);
    let start = start_point(repo, base);
    let p = path.to_string_lossy();
    git(repo, &["worktree", "add", "--no-track", "-b", branch, &p, &start])?;
    Ok(())
}

/// Create a worktree at `path` on a new `branch` starting exactly at `start` (a local ref), with
/// no fetch. Stacked workspaces use it to start from the parent's newest local commit.
pub fn add_worktree_at(repo: &Path, path: &Path, branch: &str, start: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let p = path.to_string_lossy();
    git(repo, &["worktree", "add", "--no-track", "-b", branch, &p, start])?;
    Ok(())
}

/// Remove the worktree directory but keep the branch, so archived work can be restored.
pub fn remove_worktree(repo: &Path, path: &Path) -> Result<()> {
    let p = path.to_string_lossy();
    git(repo, &["worktree", "remove", "--force", &p])?;
    Ok(())
}

/// Re-create the worktree for an existing branch (unarchive).
pub fn restore_worktree(repo: &Path, path: &Path, branch: &str) -> Result<()> {
    let p = path.to_string_lossy();
    git(repo, &["worktree", "add", &p, branch])?;
    Ok(())
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FileChange {
    pub path: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
}

/// Merge base between the workspace HEAD and its target branch (remote copy preferred).
pub fn merge_base(ws: &Path, target: &str) -> Result<String> {
    let base = start_point(ws, target);
    git(ws, &["merge-base", "HEAD", &base])
}

/// Files changed in the workspace versus the target branch, including uncommitted and untracked work.
pub fn changes(ws: &Path, target: &str) -> Result<Vec<FileChange>> {
    changes_since(ws, &merge_base(ws, target)?)
}

/// Files changed versus `base` (a commit), plus untracked files. `HEAD` gives uncommitted work only.
pub fn changes_since(ws: &Path, base: &str) -> Result<Vec<FileChange>> {
    let mut out: Vec<FileChange> = Vec::new();
    // NUL-separated output, matched by path: exact for renames, spaces and odd file names.
    let names = git_raw(ws, &["diff", "--name-status", "-z", base])?;
    let mut status: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut it = names.split('\0').filter(|s| !s.is_empty());
    while let Some(code) = it.next() {
        let kind = code.chars().next().unwrap_or('M');
        // Renames and copies carry two paths: old, then new.
        let path = if kind == 'R' || kind == 'C' { it.next(); it.next() } else { it.next() };
        if let Some(p) = path {
            status.insert(p.to_string(), kind.to_string());
        }
    }
    let numstat = git_raw(ws, &["diff", "--numstat", "-z", base])?;
    let mut parts = numstat.split('\0').peekable();
    while let Some(entry) = parts.next() {
        if entry.is_empty() { continue; }
        let mut f = entry.splitn(3, '\t');
        let add = f.next().unwrap_or("0").parse().unwrap_or(0);
        let del = f.next().unwrap_or("0").parse().unwrap_or(0);
        let mut path = f.next().unwrap_or("").to_string();
        if path.is_empty() {
            // Rename: "add\tdel\t" then old and new paths as separate fields.
            parts.next();
            path = parts.next().unwrap_or("").to_string();
        }
        let st = status.get(&path).cloned().unwrap_or_else(|| "M".into());
        out.push(FileChange { path, status: st, additions: add, deletions: del });
    }

    let untracked = git(ws, &["ls-files", "--others", "--exclude-standard"])?;
    for path in untracked.lines().filter(|l| !l.is_empty()) {
        out.push(FileChange { path: path.to_string(), status: "A".into(), additions: count_lines(&ws.join(path)), deletions: 0 });
    }
    Ok(out)
}

/// Lines in a new file, without reading huge ones whole (counts the first 2 MB).
pub(crate) fn count_lines(p: &Path) -> u32 {
    use std::io::Read;
    let Ok(f) = std::fs::File::open(p) else { return 0 };
    let mut buf = Vec::new();
    if f.take(2 * 1024 * 1024).read_to_end(&mut buf).is_err() || buf.contains(&0) {
        return 0; // unreadable or binary
    }
    buf.iter().filter(|b| **b == b'\n').count() as u32 + u32::from(!buf.is_empty() && buf.last() != Some(&b'\n'))
}

/// Like `git`, but keeps the output exactly (for `-z` formats).
fn git_raw(dir: &Path, args: &[&str]) -> Result<String> {
    let out = run(dir, args, None, &[])?;
    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Unified diff of one file versus the target branch's merge base.
pub fn file_diff(ws: &Path, target: &str, file: &str) -> Result<String> {
    file_diff_since(ws, &merge_base(ws, target)?, file)
}

pub fn file_diff_since(ws: &Path, base: &str, file: &str) -> Result<String> {
    let d = git(ws, &["diff", base, "--", file])?;
    if !d.is_empty() {
        return Ok(d);
    }
    // Untracked file: show it as all-new.
    let out = run(ws, &["diff", "--no-index", "--", "/dev/null", file], None, &[])?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn current_branch(ws: &Path) -> Result<String> {
    git(ws, &["rev-parse", "--abbrev-ref", "HEAD"])
}

pub fn is_dirty(ws: &Path) -> Result<bool> {
    Ok(!git(ws, &["status", "--porcelain"])?.is_empty())
}

pub fn branches(repo: &Path) -> Result<Vec<String>> {
    let out = git(repo, &["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes/origin"])?;
    let mut v: Vec<String> = out
        .lines()
        .map(|l| l.strip_prefix("origin/").unwrap_or(l).to_string())
        .filter(|l| l != "HEAD" && l != "origin")
        .collect();
    v.sort();
    v.dedup();
    Ok(v)
}

/// Run git with a private index file, so snapshots never touch the user's staging area.
pub(crate) fn git_idx(dir: &Path, index: &Path, args: &[&str]) -> Result<String> {
    let index = index.to_string_lossy();
    let out = run(dir, args, None, &[("GIT_INDEX_FILE", &index)]).with_context(|| format!("running git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

pub(crate) struct TempIndex(pub(crate) PathBuf);
impl TempIndex {
    pub(crate) fn new() -> Self {
        TempIndex(std::env::temp_dir().join(format!("nekotrail-index-{}", uuid::Uuid::new_v4())))
    }
}
impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Run git and return raw stdout, optionally with its own index and `stdin`. Paths from `-z`
/// output are bytes, not UTF-8, so file names with any characters round-trip exactly.
/// Its stdin is written while its output is read: written first, a long list (`hash-object
/// --stdin-paths` for a few thousand files) filled the output pipe and both sides waited forever.
pub(crate) fn git_bytes(dir: &Path, index: Option<&Path>, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>> {
    let index = index.map(|i| i.to_string_lossy().to_string());
    let env: Vec<(&str, &str)> = index.as_deref().map(|i| ("GIT_INDEX_FILE", i)).into_iter().collect();
    let out = run(dir, args, stdin, &env).with_context(|| format!("running git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

/// How many checkpoints each workspace keeps. Older ones are dropped as new turns start.
pub const CHECKPOINT_KEEP: usize = 50;
const CHECKPOINT_SUBJECT: &str = "nekotrail checkpoint";

fn ref_safe(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect()
}

/// Private ref folder holding one workspace's checkpoints. Refs are shared by every worktree of a
/// repo, so the workspace id keeps them apart. They never show up as branches or tags.
pub fn checkpoint_prefix(workspace_id: &str) -> String {
    format!("refs/nekotrail/checkpoints/{}/", ref_safe(workspace_id))
}

/// The ref the first version of checkpoints used: one chain per workspace.
pub fn legacy_checkpoint_ref(workspace_id: &str) -> String {
    format!("refs/nekotrail/{}", ref_safe(workspace_id))
}

/// Trees for the worktree (tracked + untracked, not ignored) and for the user's staging area
/// (`None` when it matches HEAD or has conflicts), plus HEAD. Works on a copy of the real index,
/// so the user's index is never written.
fn snapshot(ws: &Path) -> Result<(String, Option<String>, String)> {
    let idx = TempIndex::new();
    let info = git(ws, &["rev-parse", "--path-format=absolute", "--git-path", "index", "HEAD", "HEAD^{tree}"])?;
    let mut lines = info.lines().map(String::from);
    let (real, head, head_tree) = (lines.next().unwrap_or_default(), lines.next().unwrap_or_default(), lines.next().unwrap_or_default());
    // Starting from a copy keeps its stat cache: `add -A` only re-hashes files that changed,
    // which is what keeps this fast in large repos.
    let staged = if std::fs::copy(&real, &idx.0).is_ok() {
        // Fails while a merge has conflicts; then there is no single staged tree to keep.
        git_idx(ws, &idx.0, &["write-tree"]).ok().filter(|t| *t != head_tree)
    } else {
        git_idx(ws, &idx.0, &["read-tree", "HEAD"])?;
        None
    };
    git_idx(ws, &idx.0, &["add", "-A", "."])?;
    let tree = git_idx(ws, &idx.0, &["write-tree"])?;
    Ok((tree, staged, head))
}

/// The tree of every tracked and untracked (not ignored) file as it is right now, without
/// touching the user's staging area. What a checkpoint would save, without saving one.
pub fn worktree_tree(ws: &Path) -> Result<String> {
    Ok(snapshot(ws)?.0)
}

pub(crate) fn commit_tree(ws: &Path, args: &[&str]) -> Result<String> {
    let all: Vec<&str> = std::iter::once("commit-tree").chain(args.iter().copied()).collect();
    let me = [("GIT_AUTHOR_NAME", "nekotrail"), ("GIT_AUTHOR_EMAIL", "nekotrail@localhost"), ("GIT_COMMITTER_NAME", "nekotrail"), ("GIT_COMMITTER_EMAIL", "nekotrail@localhost")];
    let out = run(ws, &all, None, &me)?;
    if !out.status.success() {
        bail!("git commit-tree failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Snapshot every tracked and untracked (not ignored) file, the staging area and HEAD as a commit
/// under `<prefix><time>-<id>`. Returns the commit id. Call `prune_checkpoints` afterwards (off
/// the hot path) to keep only the newest `CHECKPOINT_KEEP`.
/// The user's index, branch, stash and ignored files (.env, node_modules) are never touched.
/// About 6 git calls: measured 0.11 s on a 120-file repo and 0.25 s on a 15,000-file repo with
/// 30,000 ignored files, roughly two `git status` runs.
pub fn checkpoint(ws: &Path, prefix: &str) -> Result<String> {
    let (tree, staged, head) = snapshot(ws)?;
    // A staging area that differs from HEAD rides along as a second parent, so it stays reachable
    // as long as the checkpoint does, and a rewind can put it back exactly.
    let staged_commit = match staged.as_deref() {
        Some(t) => Some(commit_tree(ws, &[t, "-p", &head, "-m", "nekotrail staged"])?),
        None => None,
    };
    // "staged" names the tree to restore into the index; "staged HEAD" means nothing was staged.
    let message = format!("{CHECKPOINT_SUBJECT}\n\nstaged {}", staged.as_deref().unwrap_or("HEAD"));
    let mut args = vec![tree.as_str(), "-p", head.as_str()];
    if let Some(s) = staged_commit.as_deref() {
        args.extend(["-p", s]);
    }
    args.extend(["-m", message.as_str()]);
    let commit = commit_tree(ws, &args)?;
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    git(ws, &["update-ref", &format!("{prefix}{millis:015}-{}", &commit[..12]), &commit])?;
    Ok(commit)
}

/// Checkpoint refs under `prefix` as (ref, commit), oldest first (names start with a zero-padded
/// timestamp, so name order is time order).
pub fn checkpoint_refs(dir: &Path, prefix: &str) -> Result<Vec<(String, String)>> {
    let out = git(dir, &["for-each-ref", "--format=%(refname) %(objectname)", prefix])?;
    let mut v: Vec<(String, String)> = out
        .lines()
        .filter_map(|l| l.split_once(' ').map(|(r, o)| (r.to_string(), o.to_string())))
        .collect();
    v.sort();
    Ok(v)
}

/// Drop all but the newest `keep` checkpoints under `prefix`. Returns how many were dropped.
/// Only refs are deleted; git's own garbage collection frees the snapshots later.
pub fn prune_checkpoints(dir: &Path, prefix: &str, keep: usize) -> Result<usize> {
    let refs = checkpoint_refs(dir, prefix)?;
    let drop = refs.len().saturating_sub(keep);
    if drop == 0 {
        return Ok(0);
    }
    let script: String = refs[..drop].iter().map(|(r, o)| format!("delete {r} {o}\n")).collect();
    git_bytes(dir, None, &["update-ref", "--stdin"], Some(script.as_bytes()))?;
    Ok(drop)
}

/// True when `id` is a checkpoint commit nekotrail made, not just any commit the caller names.
pub fn is_checkpoint(ws: &Path, id: &str) -> bool {
    git(ws, &["log", "-1", "--format=%an%x09%s", id, "--"])
        .map(|o| o == format!("nekotrail\t{CHECKPOINT_SUBJECT}"))
        .unwrap_or(false)
}

/// Which of `ids` still exist in the repo, in one git call however many there are.
pub fn existing_commits(ws: &Path, ids: &[String]) -> Result<Vec<String>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let input: String = ids.iter().map(|i| format!("{i}\n")).collect();
    let out = git_bytes(ws, None, &["cat-file", "--batch-check=%(objectname) %(objecttype)"], Some(input.as_bytes()))?;
    Ok(String::from_utf8_lossy(&out).lines().filter_map(|l| l.strip_suffix(" commit").map(String::from)).collect())
}

/// Paths that differ between two trees as (status letter, path). With `renames`, a rename
/// counts once (its new path).
pub(crate) fn tree_changes(ws: &Path, from: &str, to: &str, renames: bool) -> Result<Vec<(u8, Vec<u8>)>> {
    let args = ["diff-tree", "-r", "-z", "--name-status", if renames { "-M" } else { "--no-renames" }, from, to];
    let out = git_bytes(ws, None, &args, None)?;
    let mut parts = out.split(|b| *b == 0).filter(|p| !p.is_empty());
    let mut v = vec![];
    while let Some(status) = parts.next() {
        let s = status.first().copied().unwrap_or(b'M');
        let mut path = parts.next().unwrap_or_default().to_vec();
        if s == b'R' || s == b'C' {
            path = parts.next().unwrap_or_default().to_vec();
        }
        v.push((s, path));
    }
    Ok(v)
}

/// (path, blob before, blob after) per file, and whether more files changed than were listed.
pub type BlobChanges = (Vec<(String, String, String)>, bool);

/// Each file that differs between two trees, with its blob in each ("" where it doesn't exist),
/// at most `max` of them, plus whether there were more. For telling when an agent puts a file
/// back the way it was (runaway.rs).
pub fn changed_blobs(ws: &Path, from: &str, to: &str, max: usize) -> Result<BlobChanges> {
    let out = git_bytes(ws, None, &["diff-tree", "-r", "-z", "--no-renames", from, to], None)?;
    // Raw format: ":<mode> <mode> <blob> <blob> <status>\0<path>\0" per file.
    let mut parts = out.split(|b| *b == 0).filter(|p| !p.is_empty());
    let mut v = vec![];
    let mut more = false;
    while let Some(meta) = parts.next() {
        let Some(path) = parts.next() else { break };
        if v.len() == max {
            more = true;
            break;
        }
        let meta = String::from_utf8_lossy(meta);
        let f: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
        let blob = |s: Option<&&str>| s.filter(|b| !b.chars().all(|c| c == '0')).map(|b| b.to_string()).unwrap_or_default();
        v.push((String::from_utf8_lossy(path).into_owned(), blob(f.get(2)), blob(f.get(3))));
    }
    Ok((v, more))
}

/// What rewinding to `checkpoint` would do: (files that change, commits taken off the branch).
pub fn checkpoint_preview(ws: &Path, checkpoint: &str) -> Result<(usize, usize)> {
    let (now, _, head) = snapshot(ws)?;
    let files = tree_changes(ws, &now, &format!("{checkpoint}^{{tree}}"), true)?.len();
    let base = git(ws, &["rev-parse", &format!("{checkpoint}^1")])?;
    let commits = if base == head {
        0
    } else {
        git(ws, &["rev-list", "--count", &format!("{base}..{head}")])?.parse().unwrap_or(0)
    };
    Ok((files, commits))
}

/// Put the worktree back exactly as it was at `checkpoint`: same files, same staging area, same
/// HEAD. First saves a fresh checkpoint of the current state under `prefix`, so the rewind can
/// itself be undone; if that fails, nothing is touched. Only files that differ are written, so
/// editors and file watchers see the smallest change. Ignored files are never read or touched.
/// Returns (undo checkpoint, files changed).
pub fn rewind(ws: &Path, prefix: &str, to: &str) -> Result<(String, usize)> {
    use std::os::unix::ffi::OsStrExt;
    let undo = checkpoint(ws, prefix).context("couldn't save the current files first, so nothing was changed")?;
    let _ = prune_checkpoints(ws, prefix, CHECKPOINT_KEEP);
    let target = format!("{to}^{{tree}}");
    let changes = tree_changes(ws, &format!("{undo}^{{tree}}"), &target, false)?;
    let mut write: Vec<u8> = vec![];
    for (status, path) in &changes {
        let rel = Path::new(std::ffi::OsStr::from_bytes(path));
        if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            continue;
        }
        if *status == b'D' {
            // Created after the checkpoint: remove it (files and links only, never a folder).
            let p = ws.join(rel);
            if std::fs::symlink_metadata(&p).map(|m| !m.is_dir()).unwrap_or(false) {
                std::fs::remove_file(&p)?;
                // Tidy folders this left empty, up to the workspace root.
                let mut dir = p.parent();
                while let Some(d) = dir.filter(|d| d.starts_with(ws) && *d != ws) {
                    if std::fs::remove_dir(d).is_err() {
                        break;
                    }
                    dir = d.parent();
                }
            }
        } else {
            write.extend_from_slice(path);
            write.push(0);
        }
    }
    if !write.is_empty() {
        let idx = TempIndex::new();
        git_idx(ws, &idx.0, &["read-tree", &target])?;
        git_bytes(ws, Some(&idx.0), &["checkout-index", "-f", "-z", "--stdin"], Some(&write))?;
    }
    // If the agent committed since, move the branch back too (those commits stay reachable from
    // the undo checkpoint). Then put the staging area back as it was.
    let head = git(ws, &["rev-parse", &format!("{to}^1")])?;
    if git(ws, &["rev-parse", "HEAD"])? != head {
        git(ws, &["update-ref", "-m", "nekotrail: rewind", "HEAD", &head])?;
    }
    let body = git(ws, &["log", "-1", "--format=%b", to])?;
    match body.lines().find_map(|l| l.strip_prefix("staged ")) {
        Some("HEAD") => {
            git(ws, &["reset", "-q", "--mixed", &head])?;
        }
        Some(staged) => {
            git(ws, &["read-tree", staged])?;
        }
        // Checkpoints from 0.10 and earlier did not keep the staging area: match HEAD instead.
        None => {
            git(ws, &["reset", "-q", "--mixed", &head])?;
        }
    }
    let _ = git(ws, &["update-index", "-q", "--refresh"]);
    Ok((undo, changes.len()))
}

/// Fetch `origin/<target>` and merge it in. Returns "up-to-date", "merged" or "conflicts"
/// (conflicts are left in the worktree for the agent or the user to resolve).
pub fn merge_target(ws: &Path, target: &str) -> Result<&'static str> {
    git(ws, &["fetch", "--quiet", "origin", target])?;
    let before = git(ws, &["rev-parse", "HEAD"])?;
    match git(ws, &["merge", "--no-edit", &format!("origin/{target}")]) {
        Ok(_) => Ok(if git(ws, &["rev-parse", "HEAD"])? == before { "up-to-date" } else { "merged" }),
        Err(err) => {
            if git(ws, &["diff", "--name-only", "--diff-filter=U"]).map(|o| !o.is_empty()).unwrap_or(false) {
                Ok("conflicts")
            } else {
                Err(err)
            }
        }
    }
}

/// Move a worktree folder somewhere else, keeping git's bookkeeping right (`git worktree move`).
pub fn move_worktree(repo: &Path, from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    git(repo, &["worktree", "move", &from.to_string_lossy(), &to.to_string_lossy()])?;
    Ok(())
}

/// Build output and dependency folders that are safe to delete because they are regenerated.
const HEAVY: &[&str] = &[
    "node_modules", "target", "dist", "build", ".next", ".nuxt", ".turbo", ".parcel-cache", ".svelte-kit",
    ".venv", "venv", "__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache", ".gradle", "coverage", "DerivedData", "Pods",
];

/// Folders under `ws` (up to 4 levels deep) named like build output AND ignored by git, with their
/// size in KB. Only ignored folders count, so nothing tracked or hand-made is ever offered.
pub fn cleanable(ws: &Path) -> Vec<(String, u64)> {
    fn walk(root: &Path, dir: &Path, depth: u32, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if !ft.is_dir() || ft.is_symlink() { continue; }
            let name = e.file_name().to_string_lossy().to_string();
            if name == ".git" || name == ".nekotrail" { continue; }
            if HEAVY.contains(&name.as_str()) {
                out.push(p.strip_prefix(root).unwrap_or(&p).to_path_buf());
            } else if depth < 4 {
                walk(root, &p, depth + 1, out);
            }
        }
    }
    let mut found = vec![];
    walk(ws, ws, 1, &mut found);
    found
        .into_iter()
        .filter(|rel| is_ignored(ws, &rel.to_string_lossy()))
        .map(|rel| { let kb = dir_kb(&ws.join(&rel)); (rel.to_string_lossy().to_string(), kb) })
        .collect()
}

pub fn is_ignored(ws: &Path, rel: &str) -> bool {
    run(ws, &["check-ignore", "-q", "--", rel], None, &[]).map(|o| o.status.success()).unwrap_or(false)
}

/// Size of a folder in KB (`du -sk`), 0 if it can't be read.
pub fn dir_kb(p: &Path) -> u64 {
    Command::new("du").arg("-sk").arg(p).output().ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or(0)
}

/// Delete the cleanable folders, re-checking each right before removing it: still git-ignored, and
/// still a real folder inside the workspace with no symlink on the way (so a link swapped in since
/// the scan can never point the delete somewhere else).
pub fn clean(ws: &Path) -> u64 {
    let mut freed = 0;
    for (rel, kb) in cleanable(ws) {
        if is_ignored(ws, &rel) && crate::safe::is_real_dir_inside(ws, Path::new(&rel)) && std::fs::remove_dir_all(ws.join(&rel)).is_ok() {
            freed += kb;
        }
    }
    freed
}

/// Tracked and untracked (not ignored) files, for @-mentions.
pub fn files(ws: &Path) -> Result<Vec<String>> {
    Ok(git(ws, &["ls-files", "-co", "--exclude-standard"])?.lines().map(String::from).collect())
}

/// Stacked PRs: move a branch that started from `parent` (a branch ref) onto `new_base` once the
/// parent merged there. Replays only this branch's own commits, so it works after a squash or
/// rebase merge too, where the parent's commits never reach `new_base` by hash.
/// "up-to-date": nothing to replay (a merge-commit merge, or already moved).
/// "rebased": done. "conflicts": git stopped, the rebase was aborted and HEAD is as it was.
/// Refuses to start with uncommitted changes to tracked files, so a rebase can't touch them.
pub fn restack(ws: &Path, new_base: &str, parent: &str) -> Result<&'static str> {
    if !git(ws, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        bail!("This workspace has uncommitted changes. Commit them first, then try again.");
    }
    let _ = git(ws, &["fetch", "--quiet", "origin", new_base]);
    let onto = start_point(ws, new_base);
    // The newest parent commit this branch contains: everything after it is this branch's own work.
    let fork = restack_fork(ws, parent)?;
    if git(ws, &["merge-base", "--is-ancestor", &fork, &onto]).is_ok() {
        return Ok("up-to-date");
    }
    if git(ws, &["rebase", "--onto", &onto, &fork]).is_err() {
        let _ = git(ws, &["rebase", "--abort"]);
        return Ok("conflicts");
    }
    Ok("rebased")
}

/// Where a stacked branch left its parent: the commit to replay from.
pub fn restack_fork(ws: &Path, parent: &str) -> Result<String> {
    git(ws, &["merge-base", "HEAD", parent])
}

/// What discarding one file did.
#[derive(Debug, PartialEq)]
pub enum Discarded {
    /// The file was new (untracked or only staged): it went to the Trash and out of the index.
    Trashed,
    /// The file is back to its last commit; the edited copy, if there was one, went to the Trash.
    Restored,
}

/// Drop the uncommitted changes to one file and nothing else. The current copy goes through
/// `trash` first, so the discarded work can be fished out again; if that fails, git is never
/// touched. Committed changes stay: this only undoes what isn't in a commit yet.
pub fn discard_file(ws: &Path, rel: &str, trash: impl Fn(&Path) -> Result<()>) -> Result<Discarded> {
    // `:(literal)` so a name like `*.ts` or `[id].tsx` means that one file, never a pattern.
    let spec = format!(":(literal){rel}");
    let status = git_raw(ws, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", &spec])?;
    if status.split('\0').all(|s| s.is_empty()) {
        bail!("{rel} has no uncommitted changes. Its committed changes stay as they are.");
    }
    let in_head = git(ws, &["cat-file", "-e", &format!("HEAD:{rel}")]).is_ok();
    let full = ws.join(rel);
    if full.symlink_metadata().is_ok() {
        trash(&full).with_context(|| format!("moving {rel} to the Trash"))?;
    }
    if in_head {
        // Index and working copy back to HEAD, for this path only.
        git(ws, &["checkout", "HEAD", "--", &spec])?;
        Ok(Discarded::Restored)
    } else {
        // New file: out of the index if it was staged. The file itself is already in the Trash.
        let _ = git(ws, &["rm", "--cached", "--quiet", "--ignore-unmatch", "--", &spec]);
        Ok(Discarded::Trashed)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn a_long_path_list_on_stdin_does_not_deadlock() {
        // 5,000 paths of ~100 bytes in, 41 bytes out each: git fills its output pipe long before
        // it has read the list. Writing all of stdin first waited forever (the protected-files
        // check at the end of a turn does exactly this).
        let d = tempfile::tempdir().unwrap();
        let mut list = Vec::new();
        for i in 0..5000 {
            let name = format!("{}-{i:05}.txt", "f".repeat(90));
            std::fs::write(d.path().join(&name), i.to_string()).unwrap();
            list.extend_from_slice(name.as_bytes());
            list.push(b'\n');
        }
        assert!(Command::new("git").args(["init", "-q"]).arg(d.path()).status().unwrap().success());
        let dir = d.path().to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(git_bytes(&dir, None, &["hash-object", "--stdin-paths"], Some(&list)).map(|o| o.len()));
        });
        let out = rx.recv_timeout(std::time::Duration::from_secs(30)).expect("git and nekotrail waited on each other");
        assert_eq!(out.unwrap(), 5000 * 41);
    }

    pub fn init_repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        git(p, &["init", "-q", "-b", "main"]).unwrap();
        git(p, &["config", "user.email", "t@example.com"]).unwrap();
        git(p, &["config", "user.name", "T"]).unwrap();
        std::fs::write(p.join("a.txt"), "one\n").unwrap();
        git(p, &["add", "."]).unwrap();
        git(p, &["commit", "-qm", "init"]).unwrap();
        d
    }

    #[test]
    fn changes_handle_renames_spaces_and_binary_files() {
        let repo = init_repo();
        let r = repo.path();
        std::fs::write(r.join("old name.txt"), "a\nb\nc\n").unwrap();
        std::fs::write(r.join("img.bin"), [0u8, 1, 2]).unwrap();
        git(r, &["add", "."]).unwrap();
        git(r, &["commit", "-qm", "files"]).unwrap();
        let base = git(r, &["rev-parse", "HEAD"]).unwrap();
        git(r, &["mv", "old name.txt", "new name.txt"]).unwrap();
        std::fs::write(r.join("new name.txt"), "a\nb\nc\nd\n").unwrap();
        std::fs::write(r.join("img.bin"), [0u8, 9, 9, 9]).unwrap();
        std::fs::write(r.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(r.join("untracked.txt"), "x\ny").unwrap();
        git(r, &["add", "-A"]).unwrap();
        git(r, &["reset", "-q", "untracked.txt"]).unwrap();
        let c = changes_since(r, &base).unwrap();
        let get = |p: &str| c.iter().find(|f| f.path == p).unwrap_or_else(|| panic!("{p} missing in {c:?}"));
        assert_eq!((get("new name.txt").status.as_str(), get("new name.txt").additions), ("R", 1));
        assert_eq!(get("a.txt").additions, 1);
        assert_eq!(get("img.bin").additions, 0);
        assert_eq!(get("untracked.txt").additions, 2);
        assert!(!c.iter().any(|f| f.path.contains("old name")));
    }

    /// An origin repo and a clone with a parent branch `p` (pushed) and a child branch `child`
    /// stacked on it, checked out. Returns (origin, clone dir holder, clone path).
    fn stacked() -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
        let origin = init_repo();
        let holder = tempfile::tempdir().unwrap();
        let c = holder.path().join("c");
        git(origin.path(), &["clone", "-q", &origin.path().to_string_lossy(), &c.to_string_lossy()]).unwrap();
        git(&c, &["config", "user.email", "t@example.com"]).unwrap();
        git(&c, &["config", "user.name", "T"]).unwrap();
        git(&c, &["checkout", "-qb", "p"]).unwrap();
        std::fs::write(c.join("p.txt"), "parent 1\n").unwrap();
        git(&c, &["add", "."]).unwrap();
        git(&c, &["commit", "-qm", "p1"]).unwrap();
        std::fs::write(c.join("p.txt"), "parent 1\nparent 2\n").unwrap();
        git(&c, &["commit", "-qam", "p2"]).unwrap();
        git(&c, &["push", "-q", "origin", "p"]).unwrap();
        git(&c, &["checkout", "-qb", "child"]).unwrap();
        std::fs::write(c.join("child.txt"), "child\n").unwrap();
        git(&c, &["add", "."]).unwrap();
        git(&c, &["commit", "-qm", "c1"]).unwrap();
        (origin, holder, c)
    }

    #[test]
    fn restack_after_a_squash_merge_keeps_only_the_childs_commits() {
        let (origin, _h, c) = stacked();
        let o = origin.path();
        git(o, &["merge", "-q", "--squash", "p"]).unwrap();
        git(o, &["commit", "-qm", "Parent PR (#1)"]).unwrap();
        assert_eq!(restack(&c, "main", "p").unwrap(), "rebased");
        // Only c1 on top of origin/main; the squashed parent is not replayed.
        assert_eq!(git(&c, &["log", "--format=%s", "origin/main..HEAD"]).unwrap(), "c1");
        assert_eq!(std::fs::read_to_string(c.join("p.txt")).unwrap(), "parent 1\nparent 2\n");
        assert!(c.join("child.txt").exists());
        // Running it again finds nothing to do.
        assert_eq!(restack(&c, "main", "p").unwrap(), "up-to-date");
    }

    #[test]
    fn restack_after_a_merge_commit_needs_no_rebase() {
        let (origin, _h, c) = stacked();
        git(origin.path(), &["merge", "-q", "--no-ff", "-m", "Merge p", "p"]).unwrap();
        let head = git(&c, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(restack(&c, "main", "p").unwrap(), "up-to-date");
        assert_eq!(git(&c, &["rev-parse", "HEAD"]).unwrap(), head);
    }

    #[test]
    fn restack_aborts_on_conflicts_and_refuses_uncommitted_work() {
        let (origin, _h, c) = stacked();
        let o = origin.path();
        git(o, &["merge", "-q", "--squash", "p"]).unwrap();
        std::fs::write(o.join("child.txt"), "main wrote this first\n").unwrap();
        git(o, &["add", "."]).unwrap();
        git(o, &["commit", "-qm", "squash plus a clash"]).unwrap();
        let head = git(&c, &["rev-parse", "HEAD"]).unwrap();

        std::fs::write(c.join("child.txt"), "unsaved\n").unwrap();
        assert!(restack(&c, "main", "p").is_err());
        assert_eq!(std::fs::read_to_string(c.join("child.txt")).unwrap(), "unsaved\n");
        git(&c, &["checkout", "--", "child.txt"]).unwrap();

        assert_eq!(restack(&c, "main", "p").unwrap(), "conflicts");
        assert_eq!(git(&c, &["rev-parse", "HEAD"]).unwrap(), head);
        assert_eq!(current_branch(&c).unwrap(), "child");
        assert!(git(&c, &["rev-parse", "--verify", "--quiet", "REBASE_HEAD"]).is_err(), "no rebase left in progress");
        assert_eq!(git(&c, &["status", "--porcelain"]).unwrap(), "");
    }

    /// A stand-in Trash: moves files into `bin`, numbered, so tests can read what was "trashed".
    fn fake_trash(bin: &Path) -> impl Fn(&Path) -> Result<()> + '_ {
        move |p: &Path| {
            let n = std::fs::read_dir(bin)?.count();
            std::fs::rename(p, bin.join(format!("{n}-{}", p.file_name().unwrap().to_string_lossy())))?;
            Ok(())
        }
    }

    #[test]
    fn discard_touches_only_the_one_file_and_keeps_a_copy() {
        let repo = init_repo();
        let r = repo.path();
        let bin = tempfile::tempdir().unwrap();
        std::fs::write(r.join("b.txt"), "b\n").unwrap();
        std::fs::write(r.join("[id].tsx"), "page\n").unwrap();
        git(r, &["add", "."]).unwrap();
        git(r, &["commit", "-qm", "more"]).unwrap();

        // Edits everywhere; discard only a.txt.
        std::fs::write(r.join("a.txt"), "agent edit\n").unwrap();
        std::fs::write(r.join("b.txt"), "keep me\n").unwrap();
        std::fs::write(r.join("i.tsx"), "matches [id].tsx as a glob\n").unwrap();
        std::fs::write(r.join("[id].tsx"), "edited page\n").unwrap();
        git(r, &["add", "a.txt"]).unwrap(); // staged too: both index and file go back
        assert_eq!(discard_file(r, "a.txt", fake_trash(bin.path())).unwrap(), Discarded::Restored);
        assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "one\n");
        assert_eq!(git(r, &["diff", "--cached", "--name-only"]).unwrap(), "");
        assert_eq!(std::fs::read_to_string(bin.path().join("0-a.txt")).unwrap(), "agent edit\n");
        assert_eq!(std::fs::read_to_string(r.join("b.txt")).unwrap(), "keep me\n");

        // A name with glob characters means only itself.
        assert_eq!(discard_file(r, "[id].tsx", fake_trash(bin.path())).unwrap(), Discarded::Restored);
        assert_eq!(std::fs::read_to_string(r.join("[id].tsx")).unwrap(), "page\n");
        assert!(r.join("i.tsx").exists());
    }

    #[test]
    fn discard_sends_new_files_to_the_trash_and_brings_deleted_ones_back() {
        let repo = init_repo();
        let r = repo.path();
        let bin = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(r.join("src")).unwrap();
        std::fs::write(r.join("src/new.rs"), "fn x() {}\n").unwrap();
        std::fs::write(r.join("staged.rs"), "staged\n").unwrap();
        git(r, &["add", "staged.rs"]).unwrap();

        assert_eq!(discard_file(r, "src/new.rs", fake_trash(bin.path())).unwrap(), Discarded::Trashed);
        assert!(!r.join("src/new.rs").exists());
        assert_eq!(std::fs::read_to_string(bin.path().join("0-new.rs")).unwrap(), "fn x() {}\n");

        assert_eq!(discard_file(r, "staged.rs", fake_trash(bin.path())).unwrap(), Discarded::Trashed);
        assert!(!r.join("staged.rs").exists());
        assert_eq!(git(r, &["status", "--porcelain"]).unwrap(), "");

        std::fs::remove_file(r.join("a.txt")).unwrap();
        assert_eq!(discard_file(r, "a.txt", fake_trash(bin.path())).unwrap(), Discarded::Restored);
        assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "one\n");
    }

    #[test]
    fn discard_refuses_committed_only_files_and_stops_when_trash_fails() {
        let repo = init_repo();
        let r = repo.path();
        let bin = tempfile::tempdir().unwrap();
        std::fs::write(r.join("a.txt"), "committed\n").unwrap();
        git(r, &["commit", "-qam", "c"]).unwrap();
        assert!(discard_file(r, "a.txt", fake_trash(bin.path())).is_err());
        assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "committed\n");

        // If the Trash refuses, the edit stays exactly where it was.
        std::fs::write(r.join("a.txt"), "unsaved\n").unwrap();
        let broken = |_: &Path| -> Result<()> { bail!("no trash") };
        assert!(discard_file(r, "a.txt", broken).is_err());
        assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "unsaved\n");
    }

    #[test]
    fn cleaning_only_removes_ignored_build_folders() {
        let repo = init_repo();
        let r = repo.path();
        std::fs::write(r.join(".gitignore"), "node_modules/\napps/web/.next/\n.env\n").unwrap();
        git(r, &["add", ".gitignore"]).unwrap();
        git(r, &["commit", "-qm", "ignore"]).unwrap();
        for d in ["node_modules/pkg", "apps/web/.next/cache", "build"] {
            std::fs::create_dir_all(r.join(d)).unwrap();
            std::fs::write(r.join(d).join("f.bin"), vec![7u8; 4096]).unwrap();
        }
        std::fs::write(r.join(".env"), "SECRET=1").unwrap();
        let found: Vec<String> = cleanable(r).into_iter().map(|(p, _)| p).collect();
        assert!(found.contains(&"node_modules".to_string()) && found.contains(&"apps/web/.next".to_string()), "{found:?}");
        assert!(!found.contains(&"build".to_string()), "build/ is not ignored here, so it must stay");
        assert!(clean(r) > 0);
        assert!(!r.join("node_modules").exists() && !r.join("apps/web/.next").exists());
        assert!(r.join("build/f.bin").exists() && r.join(".env").exists() && r.join("a.txt").exists());
    }

    #[test]
    fn worktrees_can_move_with_their_uncommitted_work() {
        let repo = init_repo();
        let r = repo.path();
        let elsewhere = tempfile::tempdir().unwrap();
        let from = elsewhere.path().join("conductor/workspaces/demo/lome");
        add_worktree(r, &from, "me/lome", "main").unwrap();
        std::fs::write(from.join("wip.txt"), "unsaved work\n").unwrap();
        let to = elsewhere.path().join("nekotrail/workspaces/demo/lome");
        move_worktree(r, &from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read_to_string(to.join("wip.txt")).unwrap(), "unsaved work\n");
        assert_eq!(current_branch(&to).unwrap(), "me/lome");
        assert!(git(r, &["worktree", "list"]).unwrap().contains("nekotrail/workspaces/demo/lome"));
    }

    #[test]
    fn merge_target_reports_merged_and_conflicts() {
        let origin = init_repo();
        let o = origin.path();
        let clone = tempfile::tempdir().unwrap();
        let c = clone.path().join("c");
        git(o, &["clone", "-q", &o.to_string_lossy(), &c.to_string_lossy()]).unwrap();
        git(&c, &["config", "user.email", "t@example.com"]).unwrap();
        git(&c, &["config", "user.name", "T"]).unwrap();
        git(&c, &["checkout", "-qb", "feature"]).unwrap();
        assert_eq!(merge_target(&c, "main").unwrap(), "up-to-date");

        std::fs::write(o.join("b.txt"), "new\n").unwrap();
        git(o, &["add", "."]).unwrap();
        git(o, &["commit", "-qm", "b"]).unwrap();
        assert_eq!(merge_target(&c, "main").unwrap(), "merged");
        assert!(c.join("b.txt").exists());

        std::fs::write(o.join("a.txt"), "theirs\n").unwrap();
        git(o, &["commit", "-qam", "theirs"]).unwrap();
        std::fs::write(c.join("a.txt"), "ours\n").unwrap();
        git(&c, &["commit", "-qam", "ours"]).unwrap();
        assert_eq!(merge_target(&c, "main").unwrap(), "conflicts");
    }

    #[test]
    fn checkpoint_round_trip() {
        let repo = init_repo();
        let r = repo.path();
        let prefix = checkpoint_prefix("ws-1");
        std::fs::write(r.join("a.txt"), "user edit\n").unwrap();
        std::fs::write(r.join("draft.txt"), "untracked\n").unwrap();
        git(r, &["add", "a.txt"]).unwrap(); // something staged, to prove the real index is untouched
        let staged_before = git(r, &["diff", "--cached", "--name-only"]).unwrap();
        let status_before = git(r, &["status", "--porcelain"]).unwrap();
        let head_before = git(r, &["rev-parse", "HEAD"]).unwrap();
        let branch_before = current_branch(r).unwrap();

        let cp = checkpoint(r, &prefix).unwrap();
        assert!(is_checkpoint(r, &cp));
        assert!(!is_checkpoint(r, &head_before), "an ordinary commit is not a checkpoint");
        assert_eq!(git(r, &["diff", "--cached", "--name-only"]).unwrap(), staged_before);
        assert_eq!(git(r, &["status", "--porcelain"]).unwrap(), status_before);
        assert_eq!(git(r, &["branch", "--list"]).unwrap().lines().count(), 1, "no new branches");
        assert!(git(r, &["stash", "list"]).unwrap().is_empty());
        assert_eq!(checkpoint_refs(r, &prefix).unwrap().len(), 1);
        assert_eq!(checkpoint_preview(r, &cp).unwrap(), (0, 0));

        // The "agent" edits, deletes, creates (in a new folder) and commits.
        std::fs::write(r.join("a.txt"), "agent edit\n").unwrap();
        std::fs::remove_file(r.join("draft.txt")).unwrap();
        std::fs::create_dir_all(r.join("src/deep")).unwrap();
        std::fs::write(r.join("src/deep/new.rs"), "fn main() {}\n").unwrap();
        git(r, &["add", "-A"]).unwrap();
        git(r, &["commit", "-qm", "agent"]).unwrap();
        std::fs::write(r.join("later.txt"), "x\n").unwrap();
        assert_eq!(checkpoint_preview(r, &cp).unwrap(), (4, 1));

        let (undo, n) = rewind(r, &prefix, &cp).unwrap();
        assert_eq!(n, 4);
        assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "user edit\n");
        assert_eq!(std::fs::read_to_string(r.join("draft.txt")).unwrap(), "untracked\n");
        assert!(!r.join("src/deep/new.rs").exists() && !r.join("src").exists(), "empty folders are tidied");
        assert!(!r.join("later.txt").exists());
        assert_eq!(git(r, &["rev-parse", "HEAD"]).unwrap(), head_before);
        assert_eq!(current_branch(r).unwrap(), branch_before);
        // Exactly as before: same staged file, draft.txt untracked again.
        assert_eq!(git(r, &["status", "--porcelain"]).unwrap(), status_before);

        // The rewind is itself undoable: back to the agent's state, commit included.
        assert!(is_checkpoint(r, &undo));
        rewind(r, &prefix, &undo).unwrap();
        assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "agent edit\n");
        assert!(r.join("src/deep/new.rs").exists() && r.join("later.txt").exists());
        assert!(!r.join("draft.txt").exists());
        assert_eq!(git(r, &["log", "-1", "--format=%s"]).unwrap(), "agent");
        assert_eq!(checkpoint_refs(r, &prefix).unwrap().len(), 3);
    }

    #[test]
    fn rewind_handles_renames_and_odd_names() {
        let repo = init_repo();
        let r = repo.path();
        let prefix = checkpoint_prefix("ws-2");
        std::fs::write(r.join("old name.txt"), "content\n").unwrap();
        std::fs::write(r.join("café ü.txt"), "unicode\n").unwrap();
        git(r, &["add", "."]).unwrap();
        git(r, &["commit", "-qm", "files"]).unwrap();
        let cp = checkpoint(r, &prefix).unwrap();

        git(r, &["mv", "old name.txt", "new name.txt"]).unwrap(); // staged rename
        std::fs::rename(r.join("café ü.txt"), r.join("naïve.txt")).unwrap(); // unstaged rename
        std::fs::write(r.join("ünï.txt"), "created\n").unwrap();
        // A rename counts once in the preview.
        assert_eq!(checkpoint_preview(r, &cp).unwrap().0, 3);

        rewind(r, &prefix, &cp).unwrap();
        assert_eq!(std::fs::read_to_string(r.join("old name.txt")).unwrap(), "content\n");
        assert_eq!(std::fs::read_to_string(r.join("café ü.txt")).unwrap(), "unicode\n");
        assert!(!r.join("new name.txt").exists() && !r.join("naïve.txt").exists() && !r.join("ünï.txt").exists());
        assert!(git(r, &["status", "--porcelain"]).unwrap().is_empty(), "index matches HEAD again");
    }

    #[test]
    fn checkpoints_leave_ignored_files_alone() {
        let repo = init_repo();
        let r = repo.path();
        let prefix = checkpoint_prefix("ws-3");
        std::fs::write(r.join(".gitignore"), ".env\nnode_modules/\n").unwrap();
        git(r, &["add", ".gitignore"]).unwrap();
        git(r, &["commit", "-qm", "ignore"]).unwrap();
        std::fs::write(r.join(".env"), "SECRET=before\n").unwrap();
        std::fs::create_dir_all(r.join("node_modules/pkg")).unwrap();
        std::fs::write(r.join("node_modules/pkg/index.js"), "before").unwrap();

        let cp = checkpoint(r, &prefix).unwrap();
        let files = git(r, &["ls-tree", "-r", "--name-only", &cp]).unwrap();
        assert!(!files.contains(".env") && !files.contains("node_modules"), "ignored files are never snapshotted: {files}");

        // Ignored files change and appear after the checkpoint; a rewind must not touch any of them.
        std::fs::write(r.join(".env"), "SECRET=after\n").unwrap();
        std::fs::write(r.join("node_modules/pkg/index.js"), "after").unwrap();
        std::fs::write(r.join("node_modules/pkg/new.js"), "new").unwrap();
        std::fs::write(r.join("a.txt"), "agent\n").unwrap();
        assert_eq!(checkpoint_preview(r, &cp).unwrap().0, 1);

        rewind(r, &prefix, &cp).unwrap();
        assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "one\n");
        assert_eq!(std::fs::read_to_string(r.join(".env")).unwrap(), "SECRET=after\n");
        assert_eq!(std::fs::read_to_string(r.join("node_modules/pkg/index.js")).unwrap(), "after");
        assert!(r.join("node_modules/pkg/new.js").exists());
    }

    #[test]
    fn checkpoints_are_capped_and_pruned() {
        let repo = init_repo();
        let r = repo.path();
        let prefix = checkpoint_prefix("ws-4");
        let other = checkpoint_prefix("ws-5");
        let theirs = checkpoint(r, &other).unwrap();
        let mut ids = vec![];
        for i in 0..(CHECKPOINT_KEEP + 3) {
            std::fs::write(r.join("a.txt"), format!("{i}\n")).unwrap();
            ids.push(checkpoint(r, &prefix).unwrap());
            prune_checkpoints(r, &prefix, CHECKPOINT_KEEP).unwrap();
        }
        let refs = checkpoint_refs(r, &prefix).unwrap();
        assert_eq!(refs.len(), CHECKPOINT_KEEP);
        // The newest are the ones kept, in order.
        let kept: Vec<&String> = refs.iter().map(|(_, o)| o).collect();
        assert_eq!(kept, ids[3..].iter().collect::<Vec<_>>());
        // Archiving drops the workspace's checkpoints and nobody else's.
        assert_eq!(prune_checkpoints(r, &prefix, 0).unwrap(), CHECKPOINT_KEEP);
        assert!(checkpoint_refs(r, &prefix).unwrap().is_empty());
        assert_eq!(checkpoint_refs(r, &other).unwrap(), vec![(checkpoint_refs(r, &other).unwrap()[0].0.clone(), theirs.clone())]);
        // Existence check in one call (pruned snapshots stay until git gc).
        let missing = "0".repeat(40);
        assert_eq!(existing_commits(r, &[theirs.clone(), missing]).unwrap(), vec![theirs]);
    }

    /// Timing, not correctness: `NEKOTRAIL_BENCH=<worktree> cargo test checkpoint_speed -- --ignored --nocapture`.
    /// Takes checkpoints on a real repo and removes its refs again afterwards.
    #[test]
    #[ignore]
    fn checkpoint_speed() {
        let Ok(dir) = std::env::var("NEKOTRAIL_BENCH") else { return };
        let r = Path::new(&dir);
        let prefix = checkpoint_prefix("bench");
        let time = |label: &str, f: &mut dyn FnMut()| {
            let mut ms = vec![];
            for _ in 0..5 {
                let t = std::time::Instant::now();
                f();
                ms.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("{label}: median {:.0} ms, worst {:.0} ms", ms[2], ms[4]);
        };
        let files = git(r, &["ls-files"]).unwrap().lines().count();
        println!("{dir}: {files} tracked files");
        time("checkpoint", &mut || { checkpoint(r, &prefix).unwrap(); });
        let cp = checkpoint_refs(r, &prefix).unwrap()[0].1.clone();
        time("preview", &mut || { checkpoint_preview(r, &cp).unwrap(); });
        prune_checkpoints(r, &prefix, 0).unwrap();
    }

    #[test]
    fn changed_blobs_between_turns() {
        let repo = init_repo();
        let r = repo.path();
        let prefix = checkpoint_prefix("ws-blobs");
        let cp = checkpoint(r, &prefix).unwrap();
        let start = format!("{cp}^{{tree}}");
        assert_eq!(changed_blobs(r, &start, &worktree_tree(r).unwrap(), 10).unwrap(), (vec![], false));
        std::fs::write(r.join("a.txt"), "two\n").unwrap();
        std::fs::write(r.join("new file.txt"), "n\n").unwrap();
        let end = worktree_tree(r).unwrap();
        let (files, more) = changed_blobs(r, &start, &end, 10).unwrap();
        assert!(!more);
        assert_eq!(files.len(), 2);
        let a = files.iter().find(|f| f.0 == "a.txt").unwrap();
        assert_eq!(a.1, git(r, &["rev-parse", "HEAD:a.txt"]).unwrap());
        assert_eq!(a.2, git(r, &["hash-object", "a.txt"]).unwrap());
        let n = files.iter().find(|f| f.0 == "new file.txt").unwrap();
        assert_eq!(n.1, "", "it didn't exist before");
        // Back as it was: the same blob as before the turn.
        std::fs::write(r.join("a.txt"), "one\n").unwrap();
        let back = changed_blobs(r, &end, &worktree_tree(r).unwrap(), 10).unwrap().0;
        assert_eq!(back.iter().find(|f| f.0 == "a.txt").unwrap().2, a.1);
        assert!(changed_blobs(r, &start, &end, 1).unwrap().1, "more than it keeps");
        prune_checkpoints(r, &prefix, 0).unwrap();
    }

    #[test]
    fn checkpoints_work_in_a_worktree() {
        let repo = init_repo();
        let wsroot = tempfile::tempdir().unwrap();
        let ws = wsroot.path().join("tabby");
        add_worktree(repo.path(), &ws, "me/tabby", "main").unwrap();
        let prefix = checkpoint_prefix("ws-6");
        let cp = checkpoint(&ws, &prefix).unwrap();
        std::fs::write(ws.join("a.txt"), "changed\n").unwrap();
        std::fs::write(ws.join("b.txt"), "new\n").unwrap();
        rewind(&ws, &prefix, &cp).unwrap();
        assert_eq!(std::fs::read_to_string(ws.join("a.txt")).unwrap(), "one\n");
        assert!(!ws.join("b.txt").exists());
        // Refs live in the shared repo, so archiving can prune them from the main checkout.
        assert_eq!(checkpoint_refs(repo.path(), &prefix).unwrap().len(), 2);
        assert!(git(repo.path(), &["status", "--porcelain"]).unwrap().is_empty());
    }

    #[test]
    fn worktree_lifecycle_and_changes() {
        let repo = init_repo();
        let r = repo.path();
        assert_eq!(default_branch(r).unwrap(), "main");

        let wsroot = tempfile::tempdir().unwrap();
        let ws = wsroot.path().join("mittens");
        add_worktree(r, &ws, "me/mittens", "main").unwrap();
        assert_eq!(current_branch(&ws).unwrap(), "me/mittens");
        assert!(changes(&ws, "main").unwrap().is_empty());

        // Committed edit + uncommitted edit + untracked file all show up.
        std::fs::write(ws.join("a.txt"), "one\ntwo\n").unwrap();
        git(&ws, &["commit", "-qam", "edit"]).unwrap();
        std::fs::write(ws.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(ws.join("new.txt"), "x\ny\n").unwrap();

        let c = changes(&ws, "main").unwrap();
        assert_eq!(
            c,
            vec![
                FileChange { path: "a.txt".into(), status: "M".into(), additions: 2, deletions: 0 },
                FileChange { path: "new.txt".into(), status: "A".into(), additions: 2, deletions: 0 },
            ]
        );
        assert!(file_diff(&ws, "main", "a.txt").unwrap().contains("+three"));
        assert!(file_diff(&ws, "main", "new.txt").unwrap().contains("+y"));
        // Uncommitted only: the committed "two" is not in it, the uncommitted "three" is.
        let unc = file_diff_since(&ws, "HEAD", "a.txt").unwrap();
        assert!(unc.contains("+three") && !unc.contains("+two"));
        assert_eq!(changes_since(&ws, "HEAD").unwrap()[0].additions, 1);

        // Archive keeps the branch; restore brings the worktree back.
        git(&ws, &["add", "."]).unwrap();
        git(&ws, &["commit", "-qm", "wip"]).unwrap();
        remove_worktree(r, &ws).unwrap();
        assert!(!ws.exists());
        assert!(branch_exists(r, "me/mittens"));
        restore_worktree(r, &ws, "me/mittens").unwrap();
        assert!(ws.join("new.txt").exists());
    }
}
