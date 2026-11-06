//! The prompt library: saved prompts you insert with `/name` in the chat box.
//!
//! Three places they come from, highest first when two share a name:
//! - **repo**: `.nekotrail/prompts/*.md` (front matter for the name and description), then the
//!   `prompts` list in `nekotrail.json`. Shared through git, so read-only in the app.
//! - **personal**: saved in the app's own store.
//! - **claude**: Claude Code's own `.claude/commands/*.md` (repo, then `~/.claude`). Claude runs these
//!   itself, so the app only suggests them in a Claude chat and never expands them.
//!
//! A prompt's text can hold blanks: `{{ticket}}`, or `{{ticket:NEK-12}}` with a starting value.
//! `{{branch}}`, `{{base}}`, `{{files_changed}}` and `{{selection}}` are filled in by the app.
//! `\{{` is a literal `{{`. Anything that doesn't look like a blank (`{{ }}`, `{{a b}}`, no
//! closing braces) stays as text, so prompts that quote templates don't break.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Blanks the app fills by itself.
pub const BUILTINS: [&str; 4] = ["branch", "base", "files_changed", "selection"];

/// The chat box's own commands. A prompt can't take one of these names.
/// Kept in step with the chat box by `every_chat_command_is_reserved`.
pub const RESERVED: [&str; 22] = [
    "mcp", "model", "intelligence", "effort", "plan", "auto", "review", "second-opinion", "pr", "merge", "pull", "fix-ci",
    "run", "diff", "terminal", "preview", "checks", "fork", "clear", "compact", "add-dir", "prompts",
];

const MAX_NAME: usize = 40;
const MAX_TEXT: usize = 20_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Prompt {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Personal,
    Repo,
    Claude,
}

/// A piece of a prompt's text: plain text, or a blank to fill in.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Seg {
    Text { text: String },
    Field { name: String, default: Option<String>, builtin: bool },
}

/// One prompt as the app lists it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry {
    pub name: String,
    pub description: String,
    pub text: String,
    pub segs: Vec<Seg>,
    pub scope: Scope,
    /// Where it lives: "nekotrail.json", ".nekotrail/prompts/x.md", "~/.claude/commands/x.md", or "" for personal.
    pub source: String,
    pub read_only: bool,
    /// Set when a prompt higher up has the same name, so `/name` inserts that one instead.
    pub hidden_by: Option<Scope>,
}

fn valid_name_chars(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Why a personal prompt's name can't be used, or Ok.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Give the prompt a name.".into());
    }
    if name.len() > MAX_NAME {
        return Err(format!("Keep the name under {MAX_NAME} characters."));
    }
    if !valid_name_chars(name) {
        return Err(format!("\"{name}\" can only use letters, numbers, - and _."));
    }
    if RESERVED.contains(&name) {
        return Err(format!("/{name} is already a chat command. Pick another name."));
    }
    Ok(())
}

/// Checks a whole personal list before it is saved: good names, no two alike, text in each.
pub fn validate(list: Vec<Prompt>) -> Result<Vec<Prompt>, String> {
    let mut out: Vec<Prompt> = vec![];
    for p in list {
        let p = Prompt { name: p.name.trim().to_string(), description: p.description.trim().to_string(), text: p.text };
        check_name(&p.name)?;
        if p.text.trim().is_empty() {
            return Err(format!("/{} has no text.", p.name));
        }
        if p.text.len() > MAX_TEXT {
            return Err(format!("/{} is too long. Keep a prompt under {MAX_TEXT} characters.", p.name));
        }
        if out.iter().any(|o| o.name.eq_ignore_ascii_case(&p.name)) {
            return Err(format!("There are two prompts called /{}.", p.name));
        }
        out.push(p);
    }
    Ok(out)
}

// ---- Blanks ----------------------------------------------------------------------------------

/// Splits text into plain runs and blanks. Never fails: anything odd stays as text.
pub fn parse(text: &str) -> Vec<Seg> {
    let mut out: Vec<Seg> = vec![];
    let mut buf = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("\\{{") {
            buf.push_str("{{");
            rest = after;
            continue;
        }
        if let Some(after) = rest.strip_prefix("{{") {
            if let Some(end) = after.find("}}")
                && let Some((name, default)) = field(&after[..end])
            {
                if !buf.is_empty() {
                    out.push(Seg::Text { text: std::mem::take(&mut buf) });
                }
                let builtin = BUILTINS.contains(&name.as_str());
                out.push(Seg::Field { name, default, builtin });
                rest = &after[end + 2..];
                continue;
            }
            buf.push_str("{{");
            rest = after;
            continue;
        }
        let ch = rest.chars().next().unwrap();
        buf.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    if !buf.is_empty() {
        out.push(Seg::Text { text: buf });
    }
    out
}

/// `name` or `name:default`, spaces around either allowed. None if it isn't a blank.
fn field(inner: &str) -> Option<(String, Option<String>)> {
    if inner.contains('\n') {
        return None;
    }
    let (name, default) = match inner.split_once(':') {
        Some((n, d)) => (n.trim(), Some(d.trim().to_string())),
        None => (inner.trim(), None),
    };
    if !valid_name_chars(name) || name.len() > MAX_NAME {
        return None;
    }
    Some((name.to_string(), default.filter(|d| !d.is_empty())))
}

/// The text with each blank replaced: the given value, else its default, else nothing.
pub fn expand(text: &str, values: &HashMap<String, String>) -> String {
    parse(text)
        .into_iter()
        .map(|s| match s {
            Seg::Text { text } => text,
            Seg::Field { name, default, .. } => values.get(&name).cloned().or(default).unwrap_or_default(),
        })
        .collect()
}

// ---- Files -----------------------------------------------------------------------------------

/// Splits `---` front matter from the body. Only flat `key: value` lines are read (quotes
/// trimmed); anything else in it is ignored. No front matter: empty map, whole text as body.
pub fn front_matter(src: &str) -> (HashMap<String, String>, String) {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut map = HashMap::new();
    let first = src.lines().next().unwrap_or("");
    if first.trim_end() != "---" {
        return (map, src.to_string());
    }
    let after_first = &src[src.find('\n').map(|i| i + 1).unwrap_or(src.len())..];
    let mut offset = 0;
    for line in after_first.split_inclusive('\n') {
        offset += line.len();
        let l = line.trim_end_matches(['\n', '\r']);
        if l.trim_end() == "---" {
            let body = &after_first[offset..];
            return (map, body.trim_start_matches(['\n', '\r']).to_string());
        }
        if let Some((k, v)) = l.split_once(':') {
            let k = k.trim();
            if !k.is_empty() && !k.starts_with(' ') && !l.starts_with([' ', '\t', '-', '#']) {
                let v = v.trim();
                let v = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\''))).unwrap_or(v);
                map.insert(k.to_string(), v.to_string());
            }
        }
    }
    // Never closed: it wasn't front matter after all.
    (HashMap::new(), src.to_string())
}

/// A prompt from a markdown file. The name is the front matter's `name`, else the file name.
pub fn from_markdown(stem: &str, src: &str) -> Prompt {
    let (fm, body) = front_matter(src);
    let name = fm.get("name").map(|n| n.trim().trim_start_matches('/').to_string()).filter(|n| valid_name_chars(n)).unwrap_or_else(|| stem.to_string());
    let description = fm.get("description").cloned().unwrap_or_else(|| first_line(&body));
    Prompt { name, description, text: body.trim_end().to_string() }
}

fn first_line(body: &str) -> String {
    body.lines().find(|l| !l.trim().is_empty()).map(|l| l.trim_start_matches('#').trim().chars().take(80).collect()).unwrap_or_default()
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonPrompts {
    List(Vec<Prompt>),
    Map(HashMap<String, JsonOne>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonOne {
    Text(String),
    Full { #[serde(default)] description: String, text: String },
}

#[derive(Deserialize)]
struct RepoFile {
    #[serde(default)]
    prompts: Option<JsonPrompts>,
}

/// `.md` files in a folder, sorted, each read as a prompt. Files that aren't valid UTF-8 or have
/// a name that can't be typed after `/` are skipped.
fn md_dir(dir: &Path) -> Vec<(Prompt, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md") && p.is_file()).collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let stem = p.file_stem()?.to_str()?.to_string();
            let src = std::fs::read_to_string(&p).ok()?;
            let pr = from_markdown(&stem, &src);
            (valid_name_chars(&pr.name) && !pr.text.trim().is_empty()).then_some((pr, p))
        })
        .collect()
}

/// The repo's prompts, `.nekotrail/prompts/*.md` first, then `nekotrail.json`. The second of two
/// with the same name is dropped. A broken `nekotrail.json` adds none.
pub fn repo_prompts(dir: &Path) -> Vec<(Prompt, String)> {
    let mut out: Vec<(Prompt, String)> = md_dir(&dir.join(".nekotrail/prompts"))
        .into_iter()
        .map(|(p, path)| {
            let rel = path.strip_prefix(dir).map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
            (p, rel)
        })
        .collect();
    let from_json = std::fs::read_to_string(dir.join("nekotrail.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<RepoFile>(&t).ok())
        .and_then(|f| f.prompts);
    let mut json: Vec<Prompt> = match from_json {
        Some(JsonPrompts::List(l)) => l,
        Some(JsonPrompts::Map(m)) => m
            .into_iter()
            .map(|(name, one)| match one {
                JsonOne::Text(text) => Prompt { name, description: String::new(), text },
                JsonOne::Full { description, text } => Prompt { name, description, text },
            })
            .collect(),
        None => vec![],
    };
    json.sort_by(|a, b| a.name.cmp(&b.name));
    for mut p in json {
        p.name = p.name.trim().trim_start_matches('/').to_string();
        if !valid_name_chars(&p.name) || p.text.trim().is_empty() {
            continue;
        }
        if p.description.is_empty() {
            p.description = first_line(&p.text);
        }
        out.push((p, "nekotrail.json".into()));
    }
    dedupe(out)
}

/// Claude Code's custom commands: the repo's `.claude/commands`, then `~/.claude/commands`.
pub fn claude_commands(dir: &Path, home: Option<&Path>) -> Vec<(Prompt, String)> {
    let mut out = vec![];
    for (base, label) in [(Some(dir), ""), (home, "~/")] {
        let Some(base) = base else { continue };
        for (p, path) in md_dir(&base.join(".claude/commands")) {
            let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            out.push((p, format!("{label}.claude/commands/{file}")));
        }
    }
    dedupe(out)
}

fn dedupe(list: Vec<(Prompt, String)>) -> Vec<(Prompt, String)> {
    let mut out: Vec<(Prompt, String)> = vec![];
    for x in list {
        if !out.iter().any(|(p, _)| p.name == x.0.name) {
            out.push(x);
        }
    }
    out
}

/// Every prompt, with what `/name` picks marked. Repo beats personal (the team agreed on it),
/// personal beats Claude's commands, and a name the chat box already uses is hidden in every
/// scope. Sorted by name, the one in use first.
pub fn merge(personal: Vec<Prompt>, repo: Vec<(Prompt, String)>, claude: Vec<(Prompt, String)>) -> Vec<Entry> {
    let mut all: Vec<Entry> = vec![];
    let tiers = [
        (Scope::Repo, repo),
        (Scope::Personal, personal.into_iter().map(|p| (p, String::new())).collect()),
        (Scope::Claude, claude),
    ];
    for (scope, list) in tiers {
        for (p, source) in list {
            let hidden_by = all.iter().find(|e| e.name == p.name && e.hidden_by.is_none()).map(|e| e.scope);
            all.push(Entry {
                segs: if scope == Scope::Claude { vec![Seg::Text { text: p.text.clone() }] } else { parse(&p.text) },
                name: p.name,
                description: p.description,
                text: p.text,
                scope,
                source,
                read_only: scope != Scope::Personal,
                hidden_by,
            });
        }
    }
    all.sort_by(|a, b| a.name.cmp(&b.name).then(a.hidden_by.is_some().cmp(&b.hidden_by.is_some())));
    all
}

/// A personal copy of any prompt, named `name-copy` (then `-copy-2`...) so it never clashes. Claude
/// commands' `$ARGUMENTS` becomes a `{{arguments}}` blank, and a literal `{{` in their text is escaped.
pub fn duplicate(e: &Entry, taken: &[String]) -> Prompt {
    let text = if e.scope == Scope::Claude { e.text.replace("{{", "\\{{").replace("$ARGUMENTS", "{{arguments}}") } else { e.text.clone() };
    let base = format!("{}-copy", e.name.chars().take(MAX_NAME - 8).collect::<String>());
    let mut name = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) || RESERVED.contains(&name.as_str()) {
        name = format!("{base}-{n}");
        n += 1;
    }
    Prompt { name, description: e.description.clone(), text }
}

// ---- Export and import -----------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct Export {
    nekotrail_prompts: u32,
    prompts: Vec<Prompt>,
}

pub fn export(list: &[Prompt]) -> String {
    serde_json::to_string_pretty(&Export { nekotrail_prompts: 1, prompts: list.to_vec() }).unwrap_or_default()
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Imported {
    pub added: Vec<String>,
    /// (name in the file, name it was saved as) when a different prompt already had the name.
    pub renamed: Vec<(String, String)>,
    /// Same name and same text as one you have.
    pub skipped: Vec<String>,
}

/// Adds the prompts in an export file to `current`. Never overwrites: a clash with different text
/// is saved under a new name. Accepts our export, or a bare list of prompts.
pub fn import(current: &[Prompt], json: &str) -> Result<(Vec<Prompt>, Imported), String> {
    let incoming: Vec<Prompt> = serde_json::from_str::<Export>(json)
        .map(|x| x.prompts)
        .or_else(|_| serde_json::from_str::<Vec<Prompt>>(json))
        .map_err(|_| "That file isn't a prompts export.".to_string())?;
    let mut out = current.to_vec();
    let mut rep = Imported::default();
    for p in incoming {
        let p = Prompt { name: p.name.trim().trim_start_matches('/').to_string(), ..p };
        if p.text.trim().is_empty() || !valid_name_chars(&p.name) {
            continue;
        }
        if out.iter().any(|o| o.name.eq_ignore_ascii_case(&p.name) && o.text == p.text) {
            rep.skipped.push(p.name);
            continue;
        }
        let clash = |n: &str| out.iter().any(|o| o.name.eq_ignore_ascii_case(n)) || RESERVED.contains(&n);
        let mut name = p.name.clone();
        let mut n = 2;
        while clash(&name) {
            name = format!("{}-{n}", p.name);
            n += 1;
        }
        if name == p.name { rep.added.push(name.clone()) } else { rep.renamed.push((p.name.clone(), name.clone())) }
        out.push(Prompt { name, ..p });
    }
    Ok((validate(out)?, rep))
}

/// Store key for the personal list.
pub const STORE_KEY: &str = "prompts.personal";

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, text: &str) -> Prompt {
        Prompt { name: name.into(), description: String::new(), text: text.into() }
    }
    fn f(name: &str, default: Option<&str>) -> Seg {
        Seg::Field { name: name.into(), default: default.map(Into::into), builtin: BUILTINS.contains(&name) }
    }
    fn t(s: &str) -> Seg {
        Seg::Text { text: s.into() }
    }

    #[test]
    fn front_matter_name_description_and_body() {
        let (fm, body) = front_matter("---\nname: ship\ndescription: \"Bump, tag: and release\"\nargument-hint: [version]\n---\n\nRun the tests.\n");
        assert_eq!(fm["name"], "ship");
        assert_eq!(fm["description"], "Bump, tag: and release", "quotes trimmed, colons in the value kept");
        assert_eq!(fm["argument-hint"], "[version]");
        assert_eq!(body, "Run the tests.\n");
        // CRLF files and a BOM.
        let (fm, body) = front_matter("\u{feff}---\r\ndescription: x\r\n---\r\nbody");
        assert_eq!(fm["description"], "x");
        assert_eq!(body, "body");
        // No front matter, or one that's never closed: everything is body.
        assert_eq!(front_matter("Just text\n---\n").1, "Just text\n---\n");
        let (fm, body) = front_matter("---\nname: x\nno end");
        assert!(fm.is_empty());
        assert_eq!(body, "---\nname: x\nno end");
    }

    #[test]
    fn markdown_prompt_names() {
        assert_eq!(from_markdown("fix", "---\nname: /triage\n---\nLabel {{issue}}"), Prompt { name: "triage".into(), description: "Label {{issue}}".into(), text: "Label {{issue}}".into() });
        // A name that can't follow `/` falls back to the file name; no description uses the first line.
        let pr = from_markdown("review-api", "---\nname: two words\n---\n# Review the API\n\nCheck {{file}}.");
        assert_eq!(pr.name, "review-api");
        assert_eq!(pr.description, "Review the API");
    }

    #[test]
    fn blanks_defaults_builtins_and_escapes() {
        assert_eq!(parse("Fix {{ticket}} on {{branch}}"), vec![t("Fix "), f("ticket", None), t(" on "), f("branch", None)]);
        assert_eq!(parse("{{ env : staging }}!"), vec![f("env", Some("staging")), t("!")]);
        assert_eq!(parse("{{url:http://x:8080}}"), vec![f("url", Some("http://x:8080"))], "only the first colon splits");
        // Escaped, malformed or unclosed braces stay text.
        assert_eq!(parse(r"Use \{{name}} in Handlebars"), vec![t("Use {{name}} in Handlebars")]);
        assert_eq!(parse("{{ }} {{a b}} {{x"), vec![t("{{ }} {{a b}} {{x")]);
        assert_eq!(parse("{{a\nb}}"), vec![t("{{a\nb}}")]);
        assert_eq!(parse("{{{x}}}"), vec![t("{{{x}}}")], "Handlebars' triple braces stay text");
        assert_eq!(parse("ünï {{x}} ✓"), vec![t("ünï "), f("x", None), t(" ✓")]);

        let v: HashMap<String, String> = [("ticket".to_string(), "NEK-7".to_string())].into();
        assert_eq!(expand("Fix {{ticket}} in {{env:staging}}{{missing}}.", &v), "Fix NEK-7 in staging.");
        assert_eq!(expand(r"\{{ticket}} is {{ticket}}", &v), "{{ticket}} is NEK-7", "an escaped blank is never filled");
        // A value that itself looks like a blank goes in as is, not expanded again.
        let v: HashMap<String, String> = [("a".to_string(), "{{b}}".to_string())].into();
        assert_eq!(expand("{{a}}", &v), "{{b}}");
    }

    #[test]
    fn repo_prompts_from_both_places() {
        let d = tempfile::tempdir().unwrap();
        assert!(repo_prompts(d.path()).is_empty());
        std::fs::create_dir_all(d.path().join(".nekotrail/prompts")).unwrap();
        std::fs::write(d.path().join(".nekotrail/prompts/release.md"), "---\ndescription: Cut a release\n---\nRelease {{version}}").unwrap();
        std::fs::write(d.path().join(".nekotrail/prompts/notes.txt"), "ignored").unwrap();
        std::fs::write(d.path().join(".nekotrail/prompts/empty.md"), "---\nname: empty\n---\n").unwrap();
        std::fs::write(d.path().join("nekotrail.json"), r#"{"scripts":{"run":"x"},"prompts":[{"name":"release","text":"older"},{"name":"/lint","description":"","text":"Run the linter"},{"name":"bad name","text":"x"}]}"#).unwrap();
        let got = repo_prompts(d.path());
        assert_eq!(got.iter().map(|(p, s)| (p.name.as_str(), s.as_str())).collect::<Vec<_>>(), [("release", ".nekotrail/prompts/release.md"), ("lint", "nekotrail.json")]);
        assert_eq!(got[0].0.text, "Release {{version}}");
        assert_eq!(got[1].0.description, "Run the linter");
        // The map form works too.
        std::fs::write(d.path().join("nekotrail.json"), r#"{"prompts":{"docs":"Update the docs","qa":{"description":"QA pass","text":"Test {{area}}"}}}"#).unwrap();
        let names: Vec<_> = repo_prompts(d.path()).into_iter().map(|(p, _)| p.name).collect();
        assert_eq!(names, ["release", "docs", "qa"]);
        std::fs::write(d.path().join("nekotrail.json"), "{oops").unwrap();
        assert_eq!(repo_prompts(d.path()).len(), 1, "a broken file adds nothing and breaks nothing");
    }

    #[test]
    fn scope_precedence_and_read_only() {
        let personal = vec![p("release", "mine"), p("tidy", "Tidy up"), p("ship", "my ship")];
        let repo = vec![(p("release", "team"), "nekotrail.json".into()), (p("lint", "Lint"), "nekotrail.json".into())];
        let claude = vec![(p("ship", "claude ship"), ".claude/commands/ship.md".into()), (p("triage", "Triage $ARGUMENTS {{x}}"), "~/.claude/commands/triage.md".into())];
        let all = merge(personal, repo, claude);
        let row = |n: &str, s: Scope| all.iter().find(|e| e.name == n && e.scope == s).unwrap();
        // Repo beats personal; personal beats Claude.
        assert_eq!(row("release", Scope::Repo).hidden_by, None);
        assert_eq!(row("release", Scope::Personal).hidden_by, Some(Scope::Repo));
        assert_eq!(row("ship", Scope::Personal).hidden_by, None);
        assert_eq!(row("ship", Scope::Claude).hidden_by, Some(Scope::Personal));
        // The one in use comes first.
        let releases: Vec<_> = all.iter().filter(|e| e.name == "release").map(|e| e.scope).collect();
        assert_eq!(releases, [Scope::Repo, Scope::Personal]);
        // Only personal prompts can be edited.
        for e in &all {
            assert_eq!(e.read_only, e.scope != Scope::Personal, "{} {:?}", e.name, e.scope);
        }
        // Claude runs its own commands: their text is never split into blanks.
        assert_eq!(row("triage", Scope::Claude).segs, vec![t("Triage $ARGUMENTS {{x}}")]);
    }

    #[test]
    fn duplicating_a_read_only_prompt_makes_an_editable_copy() {
        let all = merge(vec![p("lint-copy", "x")], vec![(p("lint", "Lint {{dir:src}}"), "nekotrail.json".into())], vec![(p("triage", "Triage $ARGUMENTS, keep {{this}}"), "~/.claude/commands/triage.md".into())]);
        let taken: Vec<String> = all.iter().map(|e| e.name.clone()).collect();
        let lint = all.iter().find(|e| e.name == "lint").unwrap();
        assert!(lint.read_only);
        assert_eq!(duplicate(lint, &taken), p("lint-copy-2", "Lint {{dir:src}}"));
        let triage = all.iter().find(|e| e.name == "triage").unwrap();
        let copy = duplicate(triage, &taken);
        assert_eq!(copy.text, r"Triage {{arguments}}, keep \{{this}}");
        assert_eq!(expand(&copy.text, &[("arguments".into(), "#12".into())].into()), "Triage #12, keep {{this}}");
        assert!(validate(vec![copy]).is_ok());
    }

    /// Every command the chat box handles itself (its `case "/x"` lines and its `/` menu) must be
    /// a name no prompt can take, or the prompt would shadow it. Reads Chat.tsx, so a new command
    /// there fails this test until it is added to RESERVED.
    #[test]
    fn every_chat_command_is_reserved() {
        let chat = include_str!("../../src/components/Chat.tsx");
        let mut found = vec![];
        for marker in ["case \"/", "[\"/"] {
            for part in chat.split(marker).skip(1) {
                let name: String = part.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
                if !name.is_empty() && part[name.len()..].starts_with('"') && !found.contains(&name) {
                    found.push(name);
                }
            }
        }
        assert!(found.len() >= 20 && found.iter().any(|n| n == "checks"), "parsed the chat commands: {found:?}");
        let missing: Vec<_> = found.iter().filter(|n| !RESERVED.contains(&n.as_str())).collect();
        assert!(missing.is_empty(), "chat commands a prompt could shadow: {missing:?}");
        assert!(check_name("checks").unwrap_err().contains("already a chat command"));
    }

    #[test]
    fn personal_list_is_checked_before_saving() {
        assert!(validate(vec![p("tests", "Write tests"), p("tidy", "Tidy")]).is_ok());
        assert!(validate(vec![p("a", "x"), p("A", "y")]).unwrap_err().contains("two prompts"));
        assert!(validate(vec![p("review", "x")]).unwrap_err().contains("already a chat command"));
        assert!(validate(vec![p("my prompt", "x")]).is_err());
        assert!(validate(vec![p("x", "   ")]).unwrap_err().contains("no text"));
        assert_eq!(validate(vec![Prompt { name: " x ".into(), description: " d ".into(), text: "t".into() }]).unwrap()[0], Prompt { name: "x".into(), description: "d".into(), text: "t".into() });
    }

    #[test]
    fn export_then_import_never_overwrites() {
        let mine = vec![p("tests", "Write tests"), p("tidy", "Tidy up")];
        let file = export(&[p("tests", "Write tests"), p("tidy", "Different tidy"), p("docs", "Docs"), p("review", "clashes with a command")]);
        let (out, rep) = import(&mine, &file).unwrap();
        assert_eq!(rep.skipped, ["tests"]);
        assert_eq!(rep.added, ["docs"]);
        assert_eq!(rep.renamed, [("tidy".to_string(), "tidy-2".to_string()), ("review".to_string(), "review-2".to_string())]);
        assert_eq!(out.iter().find(|x| x.name == "tidy").unwrap().text, "Tidy up", "yours is kept");
        assert_eq!(out.len(), 5);
        // A bare list works too; anything else is refused.
        assert_eq!(import(&[], r#"[{"name":"a","text":"b"}]"#).unwrap().1.added, ["a"]);
        assert!(import(&[], r#"{"hello":1}"#).is_err());
    }

    #[test]
    fn claude_commands_repo_first() {
        let repo = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        for (base, name, body) in [(repo.path(), "ship", "repo ship"), (home.path(), "ship", "home ship"), (home.path(), "triage", "---\ndescription: Triage\n---\nTriage $ARGUMENTS")] {
            std::fs::create_dir_all(base.join(".claude/commands")).unwrap();
            std::fs::write(base.join(".claude/commands").join(format!("{name}.md")), body).unwrap();
        }
        let got = claude_commands(repo.path(), Some(home.path()));
        assert_eq!(got.iter().map(|(p, s)| (p.name.as_str(), p.text.as_str(), s.as_str())).collect::<Vec<_>>(), [
            ("ship", "repo ship", ".claude/commands/ship.md"),
            ("triage", "Triage $ARGUMENTS", "~/.claude/commands/triage.md"),
        ]);
        assert_eq!(got[1].0.description, "Triage");
    }
}
