//! Moving imported Conductor workspaces out of Conductor's folder and into nekotrail's own.
//!
//! Conductor keeps every worktree inside its own folder, and says it will delete all user data
//! when its app shuts down in early 2027. A move here is `git worktree move` and nothing else: it
//! never deletes, copies or cleans anything in Conductor's folder. Each move is checked afterwards against a snapshot taken
//! just before it: git still lists the worktree at its new path, the branch and its head commit are
//! the same, and the uncommitted files (staged, unstaged and untracked) are the same, byte for byte.

use crate::git::git;
use anyhow::Result;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// One worktree that lives outside nekotrail's folder, as the import sheet lists it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Item {
    /// nekotrail's workspace id, or empty while it isn't imported yet.
    pub id: String,
    pub repo: String,
    pub label: String,
    pub branch: String,
    pub from: String,
    /// Where the move would put it.
    pub to: String,
    pub kb: u64,
    /// Files `git status` lists: staged, unstaged and untracked.
    pub uncommitted: usize,
    pub imported: bool,
}

/// What one move did.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Outcome {
    pub id: String,
    pub label: String,
    pub from: String,
    pub to: String,
    /// The folder is at `to` now.
    pub moved: bool,
    /// Why it wasn't moved, in plain words. It is still where it was, untouched.
    pub error: Option<String>,
    /// Differences the check after the move found. Empty means everything matched.
    pub problems: Vec<String>,
    /// Uncommitted files the check compared.
    pub uncommitted: usize,
}

/// A worktree's state, to compare before and after a move.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct State {
    pub head: String,
    pub branch: String,
    /// `git status --porcelain` entries, one per file.
    pub status: Vec<String>,
    /// Hash of the uncommitted content: `git diff HEAD` plus every untracked file's blob id.
    pub content: u64,
}

/// `git status --porcelain=v1 -z` split into one entry per file. A rename's second path (its old
/// name) belongs to the entry before it.
pub fn status_entries(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut parts = raw.split('\0').filter(|p| !p.is_empty());
    while let Some(p) = parts.next() {
        let renamed = p.starts_with('R') || p.starts_with('C');
        if renamed && let Some(old) = parts.next() {
            out.push(format!("{p} <- {old}"));
            continue;
        }
        out.push(p.to_string());
    }
    out
}

fn git_bytes(dir: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = crate::git::run(dir, args, None, &[])?;
    if !out.status.success() {
        anyhow::bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

/// Blob ids of the untracked (not ignored) files, so their content counts and not just their names.
fn untracked_ids(ws: &Path) -> Result<Vec<u8>> {
    let list = git_bytes(ws, &["ls-files", "--others", "--exclude-standard"])?;
    if list.is_empty() {
        return Ok(vec![]);
    }
    // The list goes in while the ids come out (proc.rs): written first, a long list deadlocked.
    let out = crate::git::run(ws, &["hash-object", "--stdin-paths"], Some(&list), &[])?;
    if !out.status.success() {
        anyhow::bail!("git hash-object failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok([list, out.stdout].concat())
}

pub fn snapshot(ws: &Path) -> Result<State> {
    let head = git(ws, &["rev-parse", "--verify", "--quiet", "HEAD"]).unwrap_or_default();
    let branch = git(ws, &["symbolic-ref", "--short", "-q", "HEAD"]).unwrap_or_default();
    let raw = git_bytes(ws, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    let status = status_entries(&String::from_utf8_lossy(&raw));
    let mut h = DefaultHasher::new();
    if !head.is_empty() {
        git_bytes(ws, &["diff", "HEAD", "--binary", "--no-ext-diff", "--no-color"])?.hash(&mut h);
    }
    untracked_ids(ws)?.hash(&mut h);
    Ok(State { head, branch, status, content: h.finish() })
}

/// Paths in `git worktree list --porcelain`.
pub fn listed_paths(porcelain: &str) -> Vec<PathBuf> {
    porcelain.lines().filter_map(|l| l.strip_prefix("worktree ")).map(PathBuf::from).collect()
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// What differs between before and after a move, in plain words. Empty means it all matched.
pub fn compare(before: &State, after: &State, listed: bool) -> Vec<String> {
    let short = |s: &str| s.chars().take(7).collect::<String>();
    let mut out = vec![];
    if !listed {
        out.push("git worktree list doesn't show the new folder.".to_string());
    }
    if before.branch != after.branch {
        out.push(format!("The branch was {} and is now {}.", or_detached(&before.branch), or_detached(&after.branch)));
    }
    if before.head != after.head {
        out.push(format!("The branch head was {} and is now {}.", short(&before.head), short(&after.head)));
    }
    if before.status != after.status {
        out.push(format!(
            "Uncommitted files differ: {} before the move, {} after.",
            before.status.len(),
            after.status.len()
        ));
    } else if before.content != after.content {
        out.push("The uncommitted files are the same ones, but their content changed.".to_string());
    }
    out
}

fn or_detached(b: &str) -> &str {
    if b.is_empty() { "detached" } else { b }
}

/// A git error, turned into what it means for you.
pub fn plain_error(err: &str) -> String {
    let e = err.to_lowercase();
    if e.contains("is locked") || e.contains("locked working tree") {
        "Git has this worktree locked (Conductor may have locked it). Run `git worktree unlock` on it, then move it again.".into()
    } else if e.contains("submodule") {
        "It has submodules, and git can't move a worktree with submodules.".into()
    } else if e.contains("cross-device") || e.contains("invalid cross") {
        "It is on a different disk from your home folder, and git can only move a worktree within one disk.".into()
    } else if e.contains("already exists") {
        "Something is already at the new path. Move it again and it picks another name.".into()
    } else if e.contains("is a main working tree") {
        "This is a repo's main checkout, not a worktree, so it stays where it is.".into()
    } else if e.contains("not a working tree") || e.contains("is not a git") || e.contains("not a git repository") {
        "Git doesn't know this folder as a worktree of its repo any more.".into()
    } else {
        let line = err.lines().find(|l| l.contains("fatal:") || l.contains("error:")).unwrap_or(err.lines().next().unwrap_or(err));
        let tail = line.rsplit_once("fatal: ").or_else(|| line.rsplit_once("error: ")).map(|(_, t)| t).unwrap_or(line);
        format!("Git said: {}", tail.trim())
    }
}

/// A free folder `dir/base`, or `dir/base-2`, `dir/base-3`… when it is taken.
pub fn destination(dir: &Path, base: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let mut to = dir.join(base);
    let mut n = 2;
    while taken(&to) {
        to = dir.join(format!("{base}-{n}"));
        n += 1;
    }
    to
}

/// Move a worktree with `git worktree move` and check it afterwards. `Err` means it wasn't moved
/// and is still where it was; `Ok` carries what the check found different (empty when all matched).
pub fn move_checked(repo: &Path, from: &Path, to: &Path) -> std::result::Result<(Vec<String>, usize), String> {
    let before = snapshot(from).map_err(|e| plain_error(&e.to_string()))?;
    crate::git::move_worktree(repo, from, to).map_err(|e| plain_error(&e.to_string()))?;
    let after = match snapshot(to) {
        Ok(s) => s,
        Err(e) => return Ok((vec![format!("Moved, but it couldn't be checked: {}", plain_error(&e.to_string()))], before.status.len())),
    };
    let listed = git(repo, &["worktree", "list", "--porcelain"])
        .map(|l| listed_paths(&l).iter().any(|p| same_path(p, to)))
        .unwrap_or(false);
    Ok((compare(&before, &after, listed), before.status.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{add_worktree, tests::init_repo};

    #[test]
    fn status_entries_keep_renames_whole() {
        let raw = " M a.txt\0R  new.txt\0old.txt\0?? notes/wip.md\0";
        assert_eq!(status_entries(raw), vec![" M a.txt", "R  new.txt <- old.txt", "?? notes/wip.md"]);
        assert!(status_entries("").is_empty());
    }

    #[test]
    fn listed_paths_reads_porcelain() {
        let p = "worktree /code/demo\nHEAD abc\nbranch refs/heads/main\n\nworktree /Users/sam/nekotrail/workspaces/demo/lome\nHEAD def\nbranch refs/heads/me/lome\n";
        assert_eq!(listed_paths(p), vec![PathBuf::from("/code/demo"), PathBuf::from("/Users/sam/nekotrail/workspaces/demo/lome")]);
    }

    #[test]
    fn compare_names_each_difference() {
        let s = State { head: "a1b2c3d4e5".into(), branch: "me/fix".into(), status: vec![" M a.txt".into()], content: 7 };
        assert!(compare(&s, &s.clone(), true).is_empty());
        assert_eq!(compare(&s, &s, false), vec!["git worktree list doesn't show the new folder."]);
        let moved_head = State { head: "ffff000011".into(), ..s.clone() };
        assert_eq!(compare(&s, &moved_head, true), vec!["The branch head was a1b2c3d and is now ffff000."]);
        let lost = State { status: vec![], ..s.clone() };
        assert_eq!(compare(&s, &lost, true), vec!["Uncommitted files differ: 1 before the move, 0 after."]);
        let edited = State { content: 8, ..s.clone() };
        assert_eq!(compare(&s, &edited, true), vec!["The uncommitted files are the same ones, but their content changed."]);
        let detached = State { branch: String::new(), ..s.clone() };
        assert_eq!(compare(&s, &detached, true), vec!["The branch was me/fix and is now detached."]);
    }

    #[test]
    fn plain_error_says_what_it_means() {
        assert!(plain_error("git worktree move a b failed: fatal: cannot move a locked working tree, lock reason: x").starts_with("Git has this worktree locked"));
        assert!(plain_error("fatal: working trees containing submodules cannot be moved or removed").contains("submodules"));
        assert!(plain_error("fatal: failed to move 'a' to 'b': Invalid cross-device link").contains("different disk"));
        assert!(plain_error("fatal: '/x' already exists").contains("already at the new path"));
        assert_eq!(plain_error("git worktree move a b failed: fatal: something new"), "Git said: something new");
    }

    #[test]
    fn destination_skips_taken_names() {
        let dir = Path::new("/n/workspaces/demo");
        let taken = [dir.join("lome"), dir.join("lome-2")];
        assert_eq!(destination(dir, "lome", |p| taken.iter().any(|t| t == p)), dir.join("lome-3"));
        assert_eq!(destination(dir, "tunis", |_| false), dir.join("tunis"));
    }

    #[test]
    fn move_keeps_branch_head_and_uncommitted_work() {
        let repo = init_repo();
        let r = repo.path();
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("conductor/workspaces/demo/lome");
        add_worktree(r, &from, "me/lome", "main").unwrap();
        std::fs::write(from.join("a.txt"), "edited\n").unwrap();
        std::fs::write(from.join("staged.txt"), "staged\n").unwrap();
        git(&from, &["add", "staged.txt"]).unwrap();
        std::fs::create_dir_all(from.join("notes")).unwrap();
        std::fs::write(from.join("notes/wip.md"), "untracked\n").unwrap();
        let before = snapshot(&from).unwrap();
        assert_eq!(before.status.len(), 3, "{:?}", before.status);

        let to = tmp.path().join("nekotrail/workspaces/demo/lome");
        let (problems, n) = move_checked(r, &from, &to).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(n, 3);
        assert!(!from.exists());
        assert_eq!(snapshot(&to).unwrap(), before);
        assert_eq!(std::fs::read_to_string(to.join("notes/wip.md")).unwrap(), "untracked\n");
    }

    #[test]
    fn snapshot_sees_content_changes_not_just_names() {
        let repo = init_repo();
        let r = repo.path();
        std::fs::write(r.join("new.txt"), "one\n").unwrap();
        let a = snapshot(r).unwrap();
        std::fs::write(r.join("new.txt"), "two\n").unwrap();
        let b = snapshot(r).unwrap();
        assert_eq!(a.status, b.status);
        assert_ne!(a.content, b.content);
    }

    #[test]
    fn a_failed_move_leaves_the_folder_alone() {
        let repo = init_repo();
        let r = repo.path();
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("conductor/workspaces/demo/lome");
        add_worktree(r, &from, "me/lome", "main").unwrap();
        std::fs::write(from.join("wip.txt"), "keep me\n").unwrap();
        git(r, &["worktree", "lock", &from.to_string_lossy()]).unwrap();
        let to = tmp.path().join("nekotrail/workspaces/demo/lome");
        let err = move_checked(r, &from, &to).unwrap_err();
        assert!(err.starts_with("Git has this worktree locked"), "{err}");
        assert_eq!(std::fs::read_to_string(from.join("wip.txt")).unwrap(), "keep me\n");
        assert!(!to.exists());
    }
}
