//! Guards for values that come from the web view, the agents or GitHub before they reach the
//! file system or a command line.

use std::path::{Component, Path, PathBuf};

/// `rel` inside `root`, refusing absolute paths, `..`, and symlinks that lead out of `root`.
/// Returns the resolved path. A file that does not exist yet is checked through its folder.
pub fn confine(root: &Path, rel: &str) -> Result<PathBuf, String> {
    const OUTSIDE: &str = "That file is outside the workspace.";
    let p = Path::new(rel);
    if rel.is_empty() || p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err(OUTSIDE.into());
    }
    let root = root.canonicalize().map_err(|_| "The workspace folder is missing.".to_string())?;
    let full = root.join(p);
    let resolved = match full.canonicalize() {
        Ok(r) => r,
        // New file: its folder must exist and be inside, and the name itself must not be a dangling link.
        Err(_) => {
            if full.symlink_metadata().is_ok() {
                return Err(OUTSIDE.into());
            }
            let (Some(dir), Some(name)) = (full.parent(), full.file_name()) else { return Err(OUTSIDE.into()) };
            dir.canonicalize().map_err(|_| OUTSIDE.to_string())?.join(name)
        }
    };
    if !resolved.starts_with(&root) {
        return Err(OUTSIDE.into());
    }
    Ok(resolved)
}

/// True when `rel` names a real folder inside `root` with no symlink anywhere on the way, so
/// deleting it can only ever touch files under `root`.
pub fn is_real_dir_inside(root: &Path, rel: &Path) -> bool {
    let Ok(root) = root.canonicalize() else { return false };
    if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return false;
    }
    let full = root.join(rel);
    let Ok(meta) = full.symlink_metadata() else { return false };
    // canonicalize resolves every symlink, so it equals the plain join only when there are none.
    meta.is_dir() && !meta.file_type().is_symlink() && full.canonicalize().is_ok_and(|c| c == full)
}

/// A value passed as a command-line argument that must never be read as an option.
pub fn not_a_flag(s: &str) -> bool {
    !s.trim_start().starts_with('-')
}

/// Free text (a prompt) passed as a positional argument. CLIs parse a leading `-` as an option
/// (`claude -p "--settings=..."`), so such text gets a leading space, which reads the same to the model.
pub fn positional(s: &str) -> String {
    if s.starts_with('-') { format!(" {s}") } else { s.to_string() }
}

/// A branch or ref name we will hand to git or gh: what git allows, and never something like an option.
pub fn branch_name(s: &str) -> bool {
    !s.is_empty()
        && not_a_flag(s)
        && !s.contains("..")
        && !s.starts_with('/')
        && !s.ends_with('/')
        && !s.ends_with(".lock")
        && !s.contains("//")
        && !s.contains("@{")
        && s.chars().all(|c| !c.is_control() && !matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
}

/// A model or effort id from settings: letters, digits and the few separators model names use.
pub fn model_id(s: &str) -> bool {
    !s.is_empty() && not_a_flag(s) && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || "._:/@-[]".contains(c))
}

/// A workspace folder name: one plain path segment.
pub fn folder_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && !s.starts_with('.') && not_a_flag(s) && !s.contains(['/', '\\', '\0'])
}

/// A git object id we created (checkpoints).
pub fn object_id(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confine_refuses_escapes_and_symlinks_out() {
        let ws = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let r = ws.path();
        std::fs::create_dir_all(r.join("src")).unwrap();
        std::fs::write(r.join("src/a.txt"), "a").unwrap();
        std::fs::write(outside.path().join("secret"), "s").unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret"), r.join("leak")).unwrap();
        std::os::unix::fs::symlink(outside.path(), r.join("out")).unwrap();
        std::os::unix::fs::symlink(r.join("src/a.txt"), r.join("ok-link")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("nope"), r.join("dangling")).unwrap();

        let canon = r.canonicalize().unwrap();
        assert_eq!(confine(r, "src/a.txt").unwrap(), canon.join("src/a.txt"));
        assert_eq!(confine(r, "./src/new.txt").unwrap(), canon.join("src/new.txt"));
        assert_eq!(confine(r, "ok-link").unwrap(), canon.join("src/a.txt"), "links that stay inside are fine");
        for bad in ["", "/etc/passwd", "../x", "src/../../x", "leak", "out/secret", "out/new.txt", "dangling", "missing-dir/x"] {
            assert!(confine(r, bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn only_real_folders_inside_can_be_deleted() {
        let ws = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let r = ws.path();
        std::fs::create_dir_all(r.join("web/node_modules")).unwrap();
        std::fs::create_dir_all(outside.path().join("node_modules")).unwrap();
        std::os::unix::fs::symlink(outside.path(), r.join("linked")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("node_modules"), r.join("node_modules")).unwrap();
        assert!(is_real_dir_inside(r, Path::new("web/node_modules")));
        assert!(!is_real_dir_inside(r, Path::new("linked/node_modules")), "a symlinked parent leads outside");
        assert!(!is_real_dir_inside(r, Path::new("node_modules")), "the folder itself is a symlink");
        assert!(!is_real_dir_inside(r, Path::new("../x")));
        assert!(!is_real_dir_inside(r, Path::new("")));
        assert!(!is_real_dir_inside(r, Path::new("missing")));
    }

    #[test]
    fn argument_guards() {
        assert_eq!(positional("--settings={}"), " --settings={}");
        assert_eq!(positional("fix the bug"), "fix the bug");
        assert!(branch_name("me/fix-502") && branch_name("release/v1.2"));
        for bad in ["", "-x", "--upload-pack=evil", "a..b", "a b", "a~1", "/a", "a/", "x.lock", "a//b", "a@{1}", "a\nb"] {
            assert!(!branch_name(bad), "{bad:?}");
        }
        assert!(model_id("claude-opus-4-1[1m]") && model_id("gpt-5.1-codex") && model_id("high"));
        assert!(!model_id("--dangerously-skip-permissions") && !model_id("a b") && !model_id(""));
        assert!(folder_name("mochi-2") && !folder_name("../x") && !folder_name("/tmp") && !folder_name(".git") && !folder_name("-x"));
        assert!(object_id(&"a".repeat(40)) && !object_id("--output=x") && !object_id("HEAD"));
    }
}
