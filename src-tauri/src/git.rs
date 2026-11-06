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
fn count_lines(p: &Path) -> u32 {
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
    let out = Command::new("git").arg("-C").arg(dir).args(args).output()?;
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
    Command::new("git").arg("-C").arg(ws).args(["check-ignore", "-q", "--", rel]).status().map(|s| s.success()).unwrap_or(false)
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
