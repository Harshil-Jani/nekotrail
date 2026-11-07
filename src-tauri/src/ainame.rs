//! Branch and chat names from a model, the way a person would name the task.
//!
//! The first message names the branch at once, locally (`branch_slug` in lib.rs): "Apne ko Cityflo
//! mai CAP and PACELC ke concept ko apply krna hain" becomes `apne-ko-cityflo-mai-cap`. Then, in
//! the background, the chat's own agent CLI is asked for a better name with its cheapest model
//! (`claude -p --model haiku`, or `codex exec` at low effort): `cityflo-cap-pacelc-backend` and
//! "Cityflo CAP PACELC Backend". When it answers in time with something valid, the branch, the
//! workspace title and the chat tab are renamed again, under the same guards as the first rename.
//! On any failure (no CLI, not logged in, slow, garbage) the local names simply stay.
//!
//! Privacy: the prompt only goes to the provider the chat's agent sends it to anyway. Other
//! agents are not asked. No tools, no MCP servers, no hooks, no saved session.
//!
//! Never in tests or end-to-end runs (their CLIs are fakes that log every call).

use regex::Regex;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

/// How long the model gets. A cold `claude -p` with Haiku answers in 3 to 6 s.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Longest branch name (without the prefix) and title, in characters.
pub const MAX_BRANCH: usize = 40;
pub const MAX_TITLE: usize = 40;
/// The message is cut to this many characters before it is sent: the name only needs the gist.
const MAX_PROMPT: usize = 2000;

/// The setting, `ai_names`: "on" unless turned off.
pub const SETTING: &str = "ai_names";

const SYSTEM: &str = "You name a coding task from the first message a developer sent to a coding agent. \
The message may be in Hinglish, Hindi or any other language: always answer in plain English. \
Build the name from the most specific nouns in the message: company and product names it mentions, and key technical terms (like CAP, PACELC, OAuth). \
Drop filler words, and generic verbs like apply, add or implement when the nouns say enough. \
The repository name is context only: don't repeat it unless the message names it. \
Reply with only one JSON object and nothing else: \
{\"branch\": \"2 to 5 lowercase ascii words joined by hyphens, at most 40 characters, no prefix, no slash\", \
\"title\": \"2 to 6 words in Title Case, at most 40 characters\"}. \
If the message names no task (a greeting, small talk), reply {\"branch\": \"\", \"title\": \"\"}.";

/// What the model named the task. Either may be missing when it failed validation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Names {
    pub branch: Option<String>,
    pub title: Option<String>,
}

/// Whether to ask a model at all: the setting is on and this isn't a test or an end-to-end run.
pub fn enabled(setting: Option<&str>) -> bool {
    setting != Some("off") && !cfg!(test) && std::env::var_os("NEKOTRAIL_E2E").is_none() && std::env::var_os("NEKOTRAIL_E2E_PATH").is_none()
}

/// The CLI to ask for a chat on `agent`: its own, so the prompt goes nowhere new. None for
/// agents without a cheap one-shot mode.
pub fn supported(agent: &str) -> bool {
    matches!(agent, "claude" | "codex")
}

/// The user message the model gets.
fn message(text: &str, repo: &str) -> String {
    let t: String = text.trim().chars().take(MAX_PROMPT).collect();
    format!("Repository: {repo}\n\nFirst message:\n\"\"\"\n{t}\n\"\"\"")
}

/// The command that asks `agent`'s CLI, run in `cwd` (an empty folder, so no project config,
/// CLAUDE.md or AGENTS.md is read).
pub fn command(agent: &str, text: &str, repo: &str, cwd: &Path) -> Option<Command> {
    let msg = message(text, repo);
    let mut c = match agent {
        "claude" => {
            let mut c = Command::new("claude");
            c.args(["-p", &crate::safe::positional(&msg), "--model", "haiku", "--output-format", "json"]);
            c.args(["--system-prompt", SYSTEM, "--tools", "", "--strict-mcp-config", "--no-session-persistence"]);
            c.args(["--setting-sources", "user", "--settings", r#"{"disableAllHooks":true}"#]);
            c
        }
        "codex" => {
            let mut c = Command::new("codex");
            c.args(["exec", "--ephemeral", "--skip-git-repo-check", "--sandbox", "read-only"]);
            c.args(["-c", "model_reasoning_effort=\"low\"", "-c", "mcp_servers={}"]);
            c.arg(crate::safe::positional(&format!("{SYSTEM}\n\n{msg}")));
            c
        }
        _ => return None,
    };
    c.current_dir(cwd);
    Some(c)
}

/// The model's answer out of the CLI's stdout. Claude prints one JSON result; codex prints the
/// final message.
pub fn reply_text(agent: &str, stdout: &str) -> Option<String> {
    match agent {
        "claude" => {
            let j: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
            if j["is_error"].as_bool().unwrap_or(false) {
                return None;
            }
            j["result"].as_str().map(str::to_string)
        }
        _ => Some(stdout.to_string()),
    }
}

/// The names in a model's reply, validated (a branch prefix the model added is taken off). None
/// when neither name is usable.
pub fn parse(reply: &str) -> Option<Names> {
    let j = last_object(reply)?;
    let names = Names {
        branch: j["branch"].as_str().and_then(branch),
        title: j["title"].as_str().and_then(title),
    };
    (names.branch.is_some() || names.title.is_some()).then_some(names)
}

/// The last JSON object in `s` that has a "branch" or "title" (models wrap it in fences or prose).
fn last_object(s: &str) -> Option<serde_json::Value> {
    let starts: Vec<usize> = s.match_indices('{').map(|(i, _)| i).collect();
    for &i in starts.iter().rev() {
        let mut de = serde_json::Deserializer::from_str(&s[i..]).into_iter::<serde_json::Value>();
        if let Some(Ok(v)) = de.next()
            && v.is_object()
            && (v.get("branch").is_some() || v.get("title").is_some())
        {
            return Some(v);
        }
    }
    None
}

static BRANCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9]+(-[a-z0-9]+){1,5}$").unwrap());

/// A branch name the model gave, made strict: lowercase ascii words joined by single hyphens,
/// 2 to 6 words, at most `MAX_BRANCH` characters. A prefix ("harshil/", "feature/") is taken off.
pub fn branch(raw: &str) -> Option<String> {
    let mut s = raw.trim().trim_matches(|c| c == '`' || c == '"' || c == '\'').to_string();
    // A prefix, the user's ("harshil/") or any other ("feature/", "fix/"): we add our own.
    if let Some(i) = s.rfind('/') {
        s = s[i + 1..].to_string();
    }
    if !s.is_ascii() {
        return None;
    }
    let s = s.to_ascii_lowercase().replace(['_', ' ', '.'], "-");
    let words: Vec<&str> = s.split('-').filter(|w| !w.is_empty()).collect();
    if words.iter().any(|w| !w.chars().all(|c| c.is_ascii_alphanumeric())) {
        return None;
    }
    // Too long: whole words from the front, as many as fit.
    let mut out = String::new();
    for w in words.iter().take(6) {
        let next = if out.is_empty() { w.to_string() } else { format!("{out}-{w}") };
        if next.len() > MAX_BRANCH {
            break;
        }
        out = next;
    }
    BRANCH.is_match(&out).then_some(out)
}

/// A title the model gave: one line, plain English (ascii), whitespace collapsed, quotes and a
/// trailing full stop taken off, at most `MAX_TITLE` characters (cut at a word).
pub fn title(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    let t = line.trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c == '*').trim().trim_end_matches('.');
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if !t.is_ascii() || t.chars().any(|c| c.is_ascii_control() || "{}<>[]|\\".contains(c)) || !t.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut out = String::new();
    for w in t.split(' ') {
        let next = if out.is_empty() { w.to_string() } else { format!("{out} {w}") };
        if next.len() > MAX_TITLE {
            break;
        }
        out = next;
    }
    (out.len() >= 3).then_some(out)
}

/// Ask `agent`'s CLI to name the task. Blocks for up to `TIMEOUT`: call it on its own thread.
pub fn ask(agent: &str, text: &str, repo: &str) -> Option<Names> {
    let cwd = std::env::temp_dir().join("nekotrail-namer");
    std::fs::create_dir_all(&cwd).ok()?;
    let mut cmd = command(agent, text, repo, &cwd)?;
    let out = crate::proc::output(&mut cmd, None, TIMEOUT).ok()?;
    if !out.status.success() {
        return None;
    }
    parse(&reply_text(agent, &String::from_utf8_lossy(&out.stdout))?)
}

/// May the AI's branch name replace `current`? Only while it is still one nekotrail picked:
/// the star-name placeholder or the slug the first message gave it. A branch the user or the
/// agent renamed, or one checked out by hand, is left alone.
pub fn may_rename(current: &str, placeholder: &str, slug: Option<&str>) -> bool {
    current == placeholder || slug.is_some_and(|s| s == current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hinglish_prompt_gets_an_english_name() {
        // What Haiku answered for the founder's prompt, verbatim.
        let reply = r#"{"branch": "apply-cap-pacelc-cityflo-backend", "title": "Apply CAP and PACELC to Cityflo Backend"}"#;
        let n = parse(reply).unwrap();
        assert_eq!(n.branch.as_deref(), Some("apply-cap-pacelc-cityflo-backend"));
        assert_eq!(n.title.as_deref(), Some("Apply CAP and PACELC to Cityflo Backend"));
    }

    #[test]
    fn titles_are_cut_at_a_word() {
        assert_eq!(title("Apply CAP and PACELC to Cityflo Backend").as_deref(), Some("Apply CAP and PACELC to Cityflo Backend"));
        assert_eq!(title("Apply CAP and PACELC to the Cityflo lets Backend").as_deref(), Some("Apply CAP and PACELC to the Cityflo lets"));
        assert_eq!(title("  \"Cityflo CAP PACELC backend.\"  ").as_deref(), Some("Cityflo CAP PACELC backend"));
        assert_eq!(title("Fix   the\tlogin loop").as_deref(), Some("Fix the login loop"));
    }

    #[test]
    fn bad_titles_are_refused() {
        assert_eq!(title(""), None);
        assert_eq!(title("ok"), None);
        assert_eq!(title("123 456"), None);
        assert_eq!(title("कैप लागू करें"), None, "not English");
        assert_eq!(title("{\"x\": 1}"), None);
    }

    #[test]
    fn branch_prefix_case_and_separators_are_cleaned() {
        assert_eq!(branch("Harshil-Jani/cityflo-cap-pacelc").as_deref(), Some("cityflo-cap-pacelc"));
        assert_eq!(branch("harshil-jani/cityflo-cap-pacelc").as_deref(), Some("cityflo-cap-pacelc"));
        assert_eq!(branch("feature/Cityflo_CAP PACELC").as_deref(), Some("cityflo-cap-pacelc"));
        assert_eq!(branch("`fix--login--loop`").as_deref(), Some("fix-login-loop"));
        assert_eq!(branch("Cityflo-CAP-PACELC-Backend").as_deref(), Some("cityflo-cap-pacelc-backend"));
    }

    #[test]
    fn long_branches_keep_whole_words() {
        let b = branch("apply-cap-and-pacelc-consistency-models-to-the-cityflo-lets-backend").unwrap();
        assert_eq!(b, "apply-cap-and-pacelc-consistency-models");
        assert!(b.len() <= MAX_BRANCH);
        // Six words at most, even when short.
        assert_eq!(branch("a-b-c-d-e-f-g-h").as_deref(), Some("a-b-c-d-e-f"));
    }

    #[test]
    fn garbage_branches_are_refused() {
        assert_eq!(branch(""), None);
        assert_eq!(branch("backend"), None, "one word says nothing");
        assert_eq!(branch("cityflo-कैप"), None);
        assert_eq!(branch("fix:login;rm -rf"), None);
        assert_eq!(branch("..-.."), None);
        assert_eq!(branch("thisisonewordthatisfartoolongtobeabranchnameatall-x"), None);
    }

    #[test]
    fn replies_in_fences_or_prose_still_parse() {
        let reply = "Sure! Here you go:\n```json\n{\"branch\": \"Me/cityflo-cap-pacelc-backend\", \"title\": \"Cityflo CAP PACELC Backend\"}\n```";
        assert_eq!(
            parse(reply),
            Some(Names { branch: Some("cityflo-cap-pacelc-backend".into()), title: Some("Cityflo CAP PACELC Backend".into()) })
        );
        // One good field is enough.
        assert_eq!(parse(r#"{"branch": "x", "title": "Fix the login loop"}"#), Some(Names { branch: None, title: Some("Fix the login loop".into()) }));
    }

    #[test]
    fn garbage_replies_give_nothing() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("I can't help with that."), None);
        assert_eq!(parse("{\"branch\": 42, \"title\": null}"), None);
        assert_eq!(parse(r#"{"branch": "", "title": ""}"#), None, "a greeting names nothing");
        assert_eq!(parse("{not json"), None);
        assert_eq!(parse(r#"{"other": "cityflo-cap"}"#), None);
    }

    #[test]
    fn claude_json_output_is_read_and_errors_are_dropped() {
        let ok = r#"{"type":"result","is_error":false,"result":"{\"branch\":\"cityflo-cap-pacelc\",\"title\":\"Cityflo CAP PACELC\"}"}"#;
        assert_eq!(parse(&reply_text("claude", ok).unwrap()).unwrap().branch.as_deref(), Some("cityflo-cap-pacelc"));
        assert_eq!(reply_text("claude", r#"{"type":"result","is_error":true,"result":"Not logged in"}"#), None);
        assert_eq!(reply_text("claude", "Error: not json"), None);
    }

    #[test]
    fn only_the_chats_own_cli_is_asked_without_tools() {
        let cwd = Path::new("/tmp/namer");
        let args = |c: &Command| c.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let c = command("claude", "--fix it", "lets-backend", cwd).unwrap();
        assert_eq!(c.get_program(), "claude");
        let a = args(&c);
        assert!(a[1].starts_with("Repository: lets-backend"));
        for f in ["haiku", "--no-session-persistence", "--strict-mcp-config", "--tools"] {
            assert!(a.iter().any(|x| x == f), "{f} missing: {a:?}");
        }
        assert_eq!(c.get_current_dir(), Some(cwd));
        let c = command("codex", "fix it", "lets-backend", cwd).unwrap();
        assert_eq!(c.get_program(), "codex");
        assert!(args(&c).iter().any(|x| x == "--ephemeral") && args(&c).iter().any(|x| x == "read-only"));
        assert!(command("gemini", "fix it", "r", cwd).is_none());
        assert!(supported("claude") && supported("codex") && !supported("gemini"));
    }

    #[test]
    fn long_prompts_are_cut_before_sending() {
        let m = message(&"x".repeat(10_000), "r");
        assert!(m.len() < MAX_PROMPT + 100);
    }

    #[test]
    fn never_runs_under_test() {
        assert!(!enabled(None));
        assert!(!enabled(Some("on")));
        assert!(!enabled(Some("off")));
    }

    #[test]
    fn only_our_own_branch_names_are_renamed() {
        assert!(may_rename("me/mochi", "me/mochi", None));
        assert!(may_rename("me/apne-ko-cityflo-mai-cap", "me/mochi", Some("me/apne-ko-cityflo-mai-cap")));
        assert!(!may_rename("me/my-own-name", "me/mochi", Some("me/apne-ko-cityflo-mai-cap")));
        assert!(!may_rename("main", "me/mochi", None));
    }
}
