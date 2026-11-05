//! Per-repo scripts. A `purrfectpr.json` committed in the repo wins over the
//! scripts saved in the app, so the whole team shares one setup.
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
    let text = std::fs::read_to_string(dir.join("purrfectpr.json")).ok()?;
    serde_json::from_str::<File>(&text).ok().map(|f| f.scripts)
}

/// File values first, app-saved values as fallback.
pub fn resolve(dir: &Path, saved: Scripts) -> Scripts {
    let f = from_file(dir).unwrap_or_default();
    let nonempty = |s: Option<String>| s.filter(|s| !s.trim().is_empty());
    Scripts {
        setup: nonempty(f.setup).or(nonempty(saved.setup)),
        run: nonempty(f.run).or(nonempty(saved.run)),
        archive: nonempty(f.archive).or(nonempty(saved.archive)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_wins_and_saved_fills_gaps() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("purrfectpr.json"), r#"{"scripts":{"setup":"bun install","run":""}}"#).unwrap();
        let saved = Scripts { setup: Some("npm i".into()), run: Some("npm start".into()), archive: None };
        assert_eq!(
            resolve(d.path(), saved),
            Scripts { setup: Some("bun install".into()), run: Some("npm start".into()), archive: None }
        );
    }

    #[test]
    fn missing_or_broken_file_uses_saved() {
        let d = tempfile::tempdir().unwrap();
        let saved = Scripts { setup: Some("make".into()), ..Default::default() };
        assert_eq!(resolve(d.path(), saved.clone()), saved);
        std::fs::write(d.path().join("purrfectpr.json"), "{oops").unwrap();
        assert_eq!(resolve(d.path(), saved.clone()), saved);
    }
}
