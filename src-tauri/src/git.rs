//! Thin wrappers over the `git` CLI. Every workspace is a git worktree on its own branch.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
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
    let base = merge_base(ws, target)?;
    let mut out: Vec<FileChange> = Vec::new();

    let numstat = git(ws, &["diff", "--numstat", &base])?;
    let names = git(ws, &["diff", "--name-status", &base])?;
    for (ns, st) in numstat.lines().zip(names.lines()) {
        let mut n = ns.split('\t');
        let add = n.next().unwrap_or("0").parse().unwrap_or(0);
        let del = n.next().unwrap_or("0").parse().unwrap_or(0);
        let mut s = st.split('\t');
        let status = s.next().unwrap_or("M").chars().next().unwrap_or('M').to_string();
        let path = s.next_back().unwrap_or_default().to_string();
        out.push(FileChange { path, status, additions: add, deletions: del });
    }

    let untracked = git(ws, &["ls-files", "--others", "--exclude-standard"])?;
    for path in untracked.lines().filter(|l| !l.is_empty()) {
        let lines = std::fs::read_to_string(ws.join(path)).map(|s| s.lines().count() as u32).unwrap_or(0);
        out.push(FileChange { path: path.to_string(), status: "A".into(), additions: lines, deletions: 0 });
    }
    Ok(out)
}

/// Unified diff of one file versus the target branch's merge base.
pub fn file_diff(ws: &Path, target: &str, file: &str) -> Result<String> {
    let base = merge_base(ws, target)?;
    let d = git(ws, &["diff", &base, "--", file])?;
    if !d.is_empty() {
        return Ok(d);
    }
    // Untracked file: show it as all-new.
    let out = Command::new("git")
        .arg("-C")
        .arg(ws)
        .args(["diff", "--no-index", "--", "/dev/null", file])
        .output()?;
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

#[cfg(test)]
pub mod tests {
    use super::*;

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
