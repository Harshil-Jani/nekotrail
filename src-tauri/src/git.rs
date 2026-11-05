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

/// Run git with a private index file, so snapshots never touch the user's staging area.
fn git_idx(dir: &Path, index: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_INDEX_FILE", index)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

struct TempIndex(PathBuf);
impl TempIndex {
    fn new() -> Self {
        TempIndex(std::env::temp_dir().join(format!("nekotrail-index-{}", uuid::Uuid::new_v4())))
    }
}
impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Snapshot every tracked and untracked (not ignored) file plus HEAD, as a commit kept alive under
/// `refname`. Returns the checkpoint commit id.
pub fn checkpoint(ws: &Path, refname: &str) -> Result<String> {
    let idx = TempIndex::new();
    // Start from a copy of the real index: its stat cache means `add -A` only re-hashes files
    // that changed, which keeps snapshots fast in large repos.
    let real = git(ws, &["rev-parse", "--path-format=absolute", "--git-path", "index"])?;
    if std::fs::copy(&real, &idx.0).is_err() {
        git_idx(ws, &idx.0, &["read-tree", "HEAD"])?;
    }
    git_idx(ws, &idx.0, &["add", "-A", "."])?;
    let tree = git_idx(ws, &idx.0, &["write-tree"])?;
    let head = git(ws, &["rev-parse", "HEAD"])?;
    let mut args = vec!["commit-tree", tree.as_str(), "-p", head.as_str()];
    let prev = git(ws, &["rev-parse", "--verify", "--quiet", refname]).ok();
    if let Some(p) = prev.as_deref() {
        args.extend(["-p", p]);
    }
    args.extend(["-m", "nekotrail checkpoint"]);
    let commit = Command::new("git")
        .arg("-C")
        .arg(ws)
        .args(&args)
        .env("GIT_AUTHOR_NAME", "nekotrail")
        .env("GIT_AUTHOR_EMAIL", "nekotrail@localhost")
        .env("GIT_COMMITTER_NAME", "nekotrail")
        .env("GIT_COMMITTER_EMAIL", "nekotrail@localhost")
        .output()?;
    if !commit.status.success() {
        bail!("git commit-tree failed: {}", String::from_utf8_lossy(&commit.stderr).trim());
    }
    let commit = String::from_utf8_lossy(&commit.stdout).trim().to_string();
    git(ws, &["update-ref", refname, &commit])?;
    Ok(commit)
}

/// Put the worktree back exactly as it was at `checkpoint`: same files, same contents, same HEAD.
/// Files created since are deleted; ignored files (build output, node_modules) are left alone.
pub fn restore_checkpoint(ws: &Path, checkpoint: &str) -> Result<()> {
    let tree = format!("{checkpoint}^{{tree}}");
    let head = git(ws, &["rev-parse", &format!("{checkpoint}^1")])?;
    let keep: std::collections::HashSet<String> =
        git(ws, &["ls-tree", "-r", "--name-only", &tree])?.lines().map(String::from).collect();
    let now = git(ws, &["ls-files", "-co", "--exclude-standard"])?;
    for f in now.lines().filter(|f| !f.is_empty() && !keep.contains(*f)) {
        let _ = std::fs::remove_file(ws.join(f));
    }
    let idx = TempIndex::new();
    git_idx(ws, &idx.0, &["read-tree", &tree])?;
    git_idx(ws, &idx.0, &["checkout-index", "-a", "-f"])?;
    // If the agent committed since, move the branch back too. The worktree already matches.
    if git(ws, &["rev-parse", "HEAD"])? != head {
        git(ws, &["reset", "-q", "--mixed", &head])?;
    }
    Ok(())
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

/// Tracked and untracked (not ignored) files, for @-mentions.
pub fn files(ws: &Path) -> Result<Vec<String>> {
    Ok(git(ws, &["ls-files", "-co", "--exclude-standard"])?.lines().map(String::from).collect())
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
        std::fs::write(r.join("a.txt"), "user edit\n").unwrap();
        std::fs::write(r.join("draft.txt"), "untracked\n").unwrap();
        git(r, &["add", "a.txt"]).unwrap(); // something staged, to prove the real index is untouched
        let staged_before = git(r, &["diff", "--cached", "--name-only"]).unwrap();
        let head_before = git(r, &["rev-parse", "HEAD"]).unwrap();

        let cp = checkpoint(r, "refs/nekotrail/test").unwrap();
        assert_eq!(git(r, &["diff", "--cached", "--name-only"]).unwrap(), staged_before);

        // The "agent" edits, deletes, creates and commits.
        std::fs::write(r.join("a.txt"), "agent edit\n").unwrap();
        std::fs::remove_file(r.join("draft.txt")).unwrap();
        std::fs::write(r.join("new.rs"), "fn main() {}\n").unwrap();
        git(r, &["add", "-A"]).unwrap();
        git(r, &["commit", "-qm", "agent"]).unwrap();
        std::fs::write(r.join("later.txt"), "x\n").unwrap();

        restore_checkpoint(r, &cp).unwrap();
        assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "user edit\n");
        assert_eq!(std::fs::read_to_string(r.join("draft.txt")).unwrap(), "untracked\n");
        assert!(!r.join("new.rs").exists());
        assert!(!r.join("later.txt").exists());
        assert_eq!(git(r, &["rev-parse", "HEAD"]).unwrap(), head_before);

        // A second checkpoint chains onto the first, so both stay reachable.
        let cp2 = checkpoint(r, "refs/nekotrail/test").unwrap();
        assert!(git(r, &["merge-base", "--is-ancestor", &cp, &cp2]).is_ok());
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
