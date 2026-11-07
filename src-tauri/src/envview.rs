//! The Environment view: every variable a workspace's agents, terminals and scripts get, where
//! each one comes from, and which value wins when a name is set twice.
//!
//! What runs in a workspace starts from your login shell's env (loginenv.rs), then gets
//! `workspace_env` on top: the workspace's own facts, its port block, the repo's variables from
//! Repo settings and, in a review workspace, the read-only git switches. Later wins, except the
//! named ports and `PORT`, which step aside for one you set yourself (ports.rs).
//!
//! `.env` files are not loaded by NekoTrail: your tools read them (dotenv, Vite, Next.js...). They
//! are shown with the usual rules: a variable already in the environment wins over the file, a
//! later line wins over an earlier one, and `.env` < `.env.local` < `.env.<mode>` < `.env.<mode>.local`.
//!
//! Read only. Nothing here writes to the database, hands out ports or logs a value: the values
//! only travel back to the window that asked, which masks them until you reveal one.

use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// Where a value comes from, as the view names it.
pub const WORKSPACE: &str = "Workspace";
pub const PORTS: &str = "Port block";
pub const REVIEW: &str = "Review workspace";
pub const REPO: &str = "Repo settings";
pub const LOGIN: &str = "Login shell";

/// Biggest `.env` file read. Anything larger is not a hand-written env file.
const MAX_FILE: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry {
    pub key: String,
    pub value: String,
    /// `WORKSPACE`, `PORTS`, `REVIEW`, `REPO`, `LOGIN`, or a `.env` file's path in the workspace.
    pub source: String,
    /// Hidden until you reveal it: everything read from a `.env` file, and any value whose name
    /// or text looks like a secret.
    pub masked: bool,
    /// Set when another value for this name is the one that applies: that value's source.
    pub overridden_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Section {
    pub id: &'static str,
    pub title: String,
    pub note: String,
    pub entries: Vec<Entry>,
    /// Variables that apply but are not listed (login shell secrets).
    pub hidden: usize,
}

#[derive(Debug, Serialize)]
pub struct View {
    pub workspace: String,
    pub repo_id: String,
    pub repo: String,
    pub sections: Vec<Section>,
}

/// Everything the view is built from, gathered by the caller (lib.rs), so this part stays pure.
#[derive(Default)]
pub struct Inputs {
    /// `NEKOTRAIL_WORKSPACE_NAME` and friends.
    pub workspace: Vec<(String, String)>,
    /// Every variable of the port block, set or not (`ports::env` with nothing of yours).
    pub ports: Vec<(String, String)>,
    /// The ones actually set: a named port or `PORT` you set yourself is left out of these.
    pub ports_set: Vec<String>,
    /// None when the workspace holds no port block yet (or is archived): says why.
    pub ports_note: Option<String>,
    pub repo: Vec<(String, String)>,
    pub review: Vec<(String, String)>,
    /// `.env` files in the workspace: path relative to it, and its text.
    pub files: Vec<(String, String)>,
    /// The login shell's env as passed on (secrets included: counted, never listed).
    pub login: Vec<(String, String)>,
}

/// A value to hide by default: its name or its text looks like a secret.
pub fn looks_secret(key: &str, value: &str) -> bool {
    crate::redact::secret_key(key) || crate::loginenv::secret_like(key) || !crate::redact::scan(value).is_empty()
}

/// Parse a `.env` file: `KEY=value`, an optional `export `, `#` comments, and single, double or
/// backtick quotes (which may span lines; `\n` and friends are expanded in double quotes only).
/// An unquoted value ends at ` #`. Lines that are not assignments are skipped. In file order,
/// duplicates kept (the caller decides which wins).
pub fn parse_dotenv(text: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut lines = text.lines();
    while let Some(raw) = lines.next() {
        let line = raw.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map(str::trim_start).unwrap_or(line);
        let Some((k, rest)) = line.split_once('=') else { continue };
        let key = k.trim();
        let ok = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-');
        if !ok {
            continue;
        }
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'' | '`')) => {
                // Quoted: up to the closing quote, reading on to the next lines if it is not on this one.
                let mut body = rest[1..].to_string();
                let close = |s: &str| -> Option<usize> {
                    let mut esc = false;
                    for (i, c) in s.char_indices() {
                        if q == '"' && c == '\\' && !esc {
                            esc = true;
                            continue;
                        }
                        if c == q && !esc {
                            return Some(i);
                        }
                        esc = false;
                    }
                    None
                };
                loop {
                    if let Some(i) = close(&body) {
                        body.truncate(i);
                        break;
                    }
                    match lines.next() {
                        Some(next) => {
                            body.push('\n');
                            body.push_str(next);
                        }
                        // Never closed: dotenv takes the rest of the line as written.
                        None => {
                            body = rest.lines().next().unwrap_or("").to_string();
                            break;
                        }
                    }
                }
                if q == '"' { unescape(&body) } else { body }
            }
            _ => {
                let end = rest.find(" #").or_else(|| rest.find("\t#")).unwrap_or(rest.len());
                rest[..end].trim().to_string()
            }
        };
        out.push((key.to_string(), value));
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

/// A file name that holds variables: `.env`, `.env.local`, `.env.production`, `staging.env`.
fn env_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.starts_with(".env") || lower.ends_with(".env")) && !["example", "sample", "template"].iter().any(|t| lower.contains(t))
}

/// The `.env` files in a workspace, as paths relative to it: those at its top, and those the
/// repo's "Copy into each workspace" globs point at (`apps/web/.env*`). Templates like
/// `.env.example` are left out: nothing reads them.
pub fn env_files(ws: &Path, globs: &[String]) -> Vec<String> {
    let mut found: Vec<String> = vec![];
    let mut look = |dir: &str, pat: &str| {
        let Ok(entries) = std::fs::read_dir(ws.join(dir)) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !entry.path().is_file() || !env_name(&name) || !crate::glob_match(pat, &name) {
                continue;
            }
            let rel = if dir.is_empty() { name } else { format!("{dir}/{name}") };
            if !found.contains(&rel) {
                found.push(rel);
            }
        }
    };
    look("", ".env*");
    for g in globs.iter().map(|g| g.trim()).filter(|g| !g.is_empty() && !g.contains("..") && !g.starts_with('/')) {
        let (dir, pat) = g.rsplit_once('/').unwrap_or(("", g));
        look(dir, pat);
    }
    found.sort();
    found
}

/// Read the files `env_files` found. One that can't be read, or is too big, is skipped.
pub fn read_files(ws: &Path, rels: &[String]) -> Vec<(String, String)> {
    rels.iter()
        .filter_map(|rel| {
            let p = ws.join(rel);
            let small = std::fs::metadata(&p).map(|m| m.len() <= MAX_FILE).unwrap_or(false);
            small.then(|| std::fs::read_to_string(&p).ok().map(|t| (rel.clone(), t))).flatten()
        })
        .collect()
}

/// Where a `.env` file sits in the usual load order: (folder, rank, mode). None for a name
/// outside the `.env[.mode][.local]` family, which is never weighed against other files.
fn file_order(rel: &str) -> Option<(&str, u8, Option<&str>)> {
    let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    let rest = name.strip_prefix(".env")?;
    if rest.is_empty() {
        return Some((dir, 0, None));
    }
    let rest = rest.strip_prefix('.')?;
    if rest == "local" {
        return Some((dir, 1, None));
    }
    match rest.strip_suffix(".local") {
        Some(mode) if !mode.is_empty() => Some((dir, 3, Some(mode))),
        _ => Some((dir, 2, Some(rest))),
    }
}

/// `b` (file, line) is applied over `a` by the usual `.env` loaders.
fn file_beats(a: (&str, usize), b: (&str, usize)) -> bool {
    if a.0 == b.0 {
        return b.1 > a.1;
    }
    match (file_order(a.0), file_order(b.0)) {
        (Some((da, ra, ma)), Some((db, rb, mb))) => da == db && (ma.is_none() || mb.is_none() || ma == mb) && rb > ra,
        _ => false,
    }
}

fn mk(key: &str, value: &str, source: &str, masked: bool) -> Entry {
    Entry { key: key.into(), value: value.into(), source: source.into(), masked: masked && !value.is_empty(), overridden_by: None }
}

pub fn build(inp: &Inputs) -> Vec<Section> {
    // Everything a process gets, each with its layer: within a layer a later entry wins, and a
    // higher layer beats a lower one. Pushed in display order; only the order inside a layer counts.
    // Named ports and PORT you set yourself are never set, so they sit below everything (layer 0).
    let yields = |k: &str| !inp.ports_set.iter().any(|s| s == k);
    let mut procs: Vec<(u8, &str, &str, &'static str)> = vec![]; // (layer, key, value, source)
    procs.extend(inp.workspace.iter().map(|(k, v)| (2, k.as_str(), v.as_str(), WORKSPACE)));
    procs.extend(inp.ports.iter().map(|(k, v)| (if yields(k) { 0 } else { 3 }, k.as_str(), v.as_str(), PORTS)));
    procs.extend(inp.review.iter().map(|(k, v)| (5, k.as_str(), v.as_str(), REVIEW)));
    procs.extend(inp.repo.iter().map(|(k, v)| (4, k.as_str(), v.as_str(), REPO)));
    procs.extend(inp.login.iter().map(|(k, v)| (1, k.as_str(), v.as_str(), LOGIN)));
    // The winner per name, as (layer, position).
    let mut win: HashMap<&str, (u8, usize)> = HashMap::new();
    for (i, (layer, k, _, _)) in procs.iter().enumerate() {
        let w = win.entry(k).or_insert((*layer, i));
        if (*layer, i) >= *w {
            *w = (*layer, i);
        }
    }
    let winner = |k: &str| win.get(k).map(|(_, i)| procs[*i].3);
    let entries: Vec<Entry> = procs
        .iter()
        .enumerate()
        .map(|(i, (layer, k, v, src))| {
            let mut e = mk(k, v, src, looks_secret(k, v));
            if win.get(k) != Some(&(*layer, i)) {
                e.overridden_by = winner(k).map(str::to_string);
            } else if *layer == 0 {
                // A port left alone for one of yours that isn't listed (a login shell secret, say).
                e.overridden_by = Some(LOGIN.into());
            }
            e
        })
        .collect();
    let of = |srcs: &[&str]| -> Vec<Entry> { entries.iter().filter(|e| srcs.contains(&e.source.as_str())).cloned().collect() };
    let neko = of(&[WORKSPACE, PORTS, REVIEW]);
    let repo = of(&[REPO]);
    let secrets = inp.login.iter().filter(|(k, _)| crate::loginenv::secret_like(k)).count();
    let mut login = of(&[LOGIN]);
    login.retain(|e| !crate::loginenv::secret_like(&e.key));
    login.sort_by(|a, b| a.key.cmp(&b.key));

    // .env files: what is already in the environment wins, then the usual file order.
    let parsed: Vec<(&str, Vec<(String, String)>)> = inp.files.iter().map(|(rel, text)| (rel.as_str(), parse_dotenv(text))).collect();
    let mut files = vec![];
    for (rel, pairs) in &parsed {
        for (line, (k, v)) in pairs.iter().enumerate() {
            let mut e = mk(k, v, rel, true);
            if let Some(src) = winner(k) {
                e.overridden_by = Some(src.to_string());
            } else {
                let mut best: Option<(&str, usize)> = None;
                for (orel, opairs) in &parsed {
                    for (oline, (ok, _)) in opairs.iter().enumerate() {
                        if ok == k && file_beats((rel, line), (orel, oline)) && best.is_none_or(|b| file_beats(b, (orel, oline))) {
                            best = Some((orel, oline));
                        }
                    }
                }
                e.overridden_by = best.map(|(f, _)| f.to_string());
            }
            files.push(e);
        }
    }

    let neko_note = match &inp.ports_note {
        Some(n) => format!("Set by NekoTrail on every agent, terminal and script in this workspace. {n}"),
        None => "Set by NekoTrail on every agent, terminal and script in this workspace.".into(),
    };
    vec![
        Section { id: "app", title: "NekoTrail".into(), note: neko_note, entries: neko, hidden: 0 },
        Section {
            id: "repo",
            title: "Repo settings".into(),
            note: "Added to everything that runs in this repo's workspaces. They win over your login shell.".into(),
            entries: repo,
            hidden: 0,
        },
        Section {
            id: "files",
            title: ".env files in this workspace".into(),
            note: "NekoTrail doesn't load these: your tools do (dotenv, Vite, Next.js). A variable already set above wins over a file, and .env.local wins over .env.".into(),
            entries: files,
            hidden: 0,
        },
        Section {
            id: "login",
            title: "From your login shell".into(),
            note: if secrets == 0 {
                "Your shell's variables, passed on to everything NekoTrail starts.".into()
            } else {
                format!(
                    "Your shell's variables, passed on to everything NekoTrail starts. {} more look like secrets (keys, tokens, passwords): they are passed on too, but NekoTrail never saves or lists them.",
                    secrets
                )
            },
            entries: login,
            hidden: secrets,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn dotenv_parses_quotes_exports_and_comments() {
        let text = "\u{feff}# a comment\n\
            PLAIN=hello world # trailing note\n\
            export EXPORTED=yes\n\
            DOUBLE=\"line one\\nline two\" # note\n\
            SINGLE='keep \\n as is'\n\
            HASH=abc#def\n\
            EMPTY=\n\
            SPACED = padded \n\
            MULTI=\"first\n\
            second\"\n\
            not an assignment\n\
            1BAD=x\n\
            BACK=`tick`\n\
            OPEN=\"never closed\n";
        assert_eq!(
            parse_dotenv(text),
            kv(&[
                ("PLAIN", "hello world"),
                ("EXPORTED", "yes"),
                ("DOUBLE", "line one\nline two"),
                ("SINGLE", "keep \\n as is"),
                ("HASH", "abc#def"),
                ("EMPTY", ""),
                ("SPACED", "padded"),
                ("MULTI", "first\nsecond"),
                ("BACK", "tick"),
                ("OPEN", "\"never closed"),
            ])
        );
    }

    #[test]
    fn finds_env_files_at_the_top_and_where_copy_globs_point() {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path();
        for f in [".env", ".env.local", ".env.example", "README.md", "apps/web/.env", "apps/web/.env.sample", "apps/web/config.json", "config/master.key"] {
            let p = ws.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "A=1\n").unwrap();
        }
        std::fs::create_dir_all(ws.join(".env.d")).unwrap();
        let found = env_files(ws, &["apps/web/*".into(), "config/master.key".into(), "../x/.env".into()]);
        assert_eq!(found, vec![".env", ".env.local", "apps/web/.env"]);
    }

    fn entry<'a>(s: &'a [Section], id: &str, key: &str, source: &str) -> &'a Entry {
        s.iter().find(|x| x.id == id).unwrap().entries.iter().find(|e| e.key == key && e.source == source).unwrap_or_else(|| panic!("{id} {key} {source}"))
    }

    #[test]
    fn shows_which_value_wins() {
        let inp = Inputs {
            workspace: kv(&[("NEKOTRAIL_WORKSPACE_NAME", "tunis"), ("NEKOTRAIL_ROOT_PATH", "/code/app")]),
            ports: kv(&[("NEKOTRAIL_PORT", "4100"), ("NEKOTRAIL_PORT_1", "4101"), ("PORT", "4100")]),
            // PORT is in Repo settings, so the block leaves it alone.
            ports_set: vec!["NEKOTRAIL_PORT".into(), "NEKOTRAIL_PORT_1".into()],
            ports_note: None,
            repo: kv(&[("PORT", "3000"), ("API_URL", "http://a"), ("API_URL", "http://b"), ("NEKOTRAIL_PORT_1", "9999")]),
            review: kv(&[("GIT_CONFIG_COUNT", "1")]),
            files: vec![
                (".env".into(), "API_URL=from-file\nDB=one\nDB=two\nONLY=x\nSTRIPE_SECRET_KEY=sk_live_abcdefghijklmnop\n".into()),
                (".env.local".into(), "ONLY=local\n".into()),
                (".env.development".into(), "ONLY=dev\n".into()),
                (".env.production".into(), "ONLY=prod\n".into()),
                ("apps/web/.env".into(), "ONLY=web\n".into()),
            ],
            login: kv(&[("PATH", "/usr/bin"), ("API_URL", "http://login"), ("GIT_CONFIG_COUNT", "0"), ("OPENAI_API_KEY", "sk-x"), ("GH_TOKEN", "ghp_x")]),
        };
        let s = build(&inp);
        assert_eq!(s.iter().map(|x| x.id).collect::<Vec<_>>(), vec!["app", "repo", "files", "login"]);

        // Repo settings beat the login shell; a later line beats an earlier one.
        assert_eq!(s[1].entries[1].overridden_by.as_deref(), Some(REPO), "the first API_URL loses to the second");
        assert_eq!(s[1].entries[2].overridden_by, None);
        assert_eq!(entry(&s, "login", "API_URL", LOGIN).overridden_by.as_deref(), Some(REPO));
        // PORT you set yourself: the block's PORT steps aside.
        assert_eq!(entry(&s, "app", "PORT", PORTS).overridden_by.as_deref(), Some(REPO));
        assert_eq!(entry(&s, "repo", "PORT", REPO).overridden_by, None);
        // Repo settings come after the block, so they win for a NEKOTRAIL_PORT_n too.
        assert_eq!(entry(&s, "app", "NEKOTRAIL_PORT_1", PORTS).overridden_by.as_deref(), Some(REPO));
        assert_eq!(entry(&s, "app", "NEKOTRAIL_PORT", PORTS).overridden_by, None);
        // The review switches come last of all.
        assert_eq!(entry(&s, "login", "GIT_CONFIG_COUNT", LOGIN).overridden_by.as_deref(), Some(REVIEW));
        assert_eq!(entry(&s, "app", "GIT_CONFIG_COUNT", REVIEW).overridden_by, None);

        // .env files: the environment wins, then later lines, then the usual file order.
        assert_eq!(entry(&s, "files", "API_URL", ".env").overridden_by.as_deref(), Some(REPO));
        let dbs: Vec<_> = s[2].entries.iter().filter(|e| e.key == "DB").map(|e| e.overridden_by.clone()).collect();
        assert_eq!(dbs, vec![Some(".env".to_string()), None]);
        let only = |src: &str| entry(&s, "files", "ONLY", src).overridden_by.clone();
        // Whichever mode runs, a mode file beats .env and .env.local; the first one is named.
        assert_eq!(only(".env").as_deref(), Some(".env.development"));
        assert_eq!(only(".env.local").as_deref(), Some(".env.development"));
        assert_eq!(only(".env.production"), None, "modes don't override each other");
        assert_eq!(only(".env.development"), None);
        assert_eq!(only("apps/web/.env"), None, "another folder is its own app");

        // Every .env value is masked; NekoTrail's own values are not; secret-looking ones are.
        assert!(s[2].entries.iter().all(|e| e.masked));
        assert!(!entry(&s, "app", "NEKOTRAIL_PORT", PORTS).masked);
        assert!(!entry(&s, "login", "PATH", LOGIN).masked);

        // Login secrets apply but are only counted.
        assert_eq!(s[3].hidden, 2);
        assert!(s[3].entries.iter().all(|e| e.key != "OPENAI_API_KEY" && e.key != "GH_TOKEN"));
        assert!(s[3].note.contains("2 more look like secrets"));
        assert!(!format!("{:?}", s[3]).contains("sk-x"), "a hidden secret's value isn't in the view");
    }

    #[test]
    fn a_login_secret_still_beats_a_dotenv_line() {
        let inp = Inputs { files: vec![(".env".into(), "OPENAI_API_KEY=from-file\n".into())], login: kv(&[("OPENAI_API_KEY", "sk-x")]), ..Default::default() };
        let s = build(&inp);
        assert_eq!(entry(&s, "files", "OPENAI_API_KEY", ".env").overridden_by.as_deref(), Some(LOGIN));
    }

    #[test]
    fn secret_looking_values_are_masked() {
        assert!(looks_secret("DATABASE_URL", "postgres://x"));
        assert!(looks_secret("SOMETHING", "postgres://me:hunter2pass@db/x"));
        assert!(looks_secret("GH_TOKEN", "x"));
        assert!(!looks_secret("NODE_ENV", "development"));
        assert!(!mk("EMPTY_TOKEN", "", REPO, true).masked, "nothing to hide in an empty value");
    }
}
