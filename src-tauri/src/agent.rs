//! Runs one agent turn (Claude Code, Codex, Gemini CLI, or an ACP agent such as OpenCode) and
//! turns its JSON stream into a small, agent-neutral event list the UI can render. Each CLI is an
//! `Agent` (see `ALL`); agents that speak the Agent Client Protocol share one client in `acp.rs`.
//!
//! Event kinds: `user`, `text`, `thinking`, `tool`, `tool_result`, `approval`, `note`, `usage`,
//! `session`, `done`, `error`.

use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, ExitStatus};

const MAX_OUTPUT: usize = 4000;

pub(crate) fn clip(s: &str) -> String {
    if s.len() <= MAX_OUTPUT {
        return s.to_string();
    }
    let mut end = MAX_OUTPUT;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… ({} more bytes)", &s[..end], s.len() - end)
}

/// One-line summary of a tool call's input, for the collapsed tool row.
pub fn summarize_input(name: &str, input: &Value) -> String {
    let pick = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    let s = match name {
        "Bash" => pick("command"),
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => pick("file_path"),
        "Grep" | "Glob" => pick("pattern"),
        "WebFetch" => pick("url"),
        "WebSearch" => pick("query"),
        "Task" | "Agent" => pick("description"),
        // Shown as a plan card / todo list, so keep the whole thing.
        "ExitPlanMode" => return pick("plan").unwrap_or_default(),
        "TodoWrite" => return input["todos"].to_string(),
        _ => None,
    };
    s.unwrap_or_else(|| clip(&input.to_string()))
}

/// Claude Code `--output-format stream-json` line -> zero or more events.
pub fn parse_claude(line: &str) -> Vec<Value> {
    let Ok(j) = serde_json::from_str::<Value>(line) else { return vec![] };
    parse_claude_value(&j)
}

/// One parsed Claude Code line -> zero or more events. The same shapes appear in Claude's own
/// session files (`~/.claude/projects`), which the terminal import reads.
pub fn parse_claude_value(j: &Value) -> Vec<Value> {
    let mut out = vec![];
    match j["type"].as_str() {
        Some("system") if j["subtype"] == "init" => {
            if let Some(id) = j["session_id"].as_str() {
                out.push(json!({"kind": "session", "agent_session_id": id}));
            }
        }
        Some("assistant") => {
            // Tokens in the model's context right now: what this request read plus what it wrote.
            let u = &j["message"]["usage"];
            if u.is_object() {
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                let tokens = n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens") + n("output_tokens");
                if tokens > 0 {
                    out.push(json!({"kind": "usage", "context_tokens": tokens}));
                }
            }
            for c in j["message"]["content"].as_array().into_iter().flatten() {
                match c["type"].as_str() {
                    Some("text") => out.push(json!({"kind": "text", "text": c["text"]})),
                    Some("thinking") => {
                        if let Some(t) = c["thinking"].as_str().filter(|t| !t.is_empty()) {
                            out.push(json!({"kind": "thinking", "text": t}));
                        }
                    }
                    Some("tool_use") => {
                        let name = c["name"].as_str().unwrap_or("tool");
                        out.push(json!({
                            "kind": "tool", "id": c["id"], "name": name,
                            "input": summarize_input(name, &c["input"]),
                        }));
                    }
                    _ => {}
                }
            }
        }
        Some("user") => {
            for c in j["message"]["content"].as_array().into_iter().flatten() {
                if c["type"] == "tool_result" {
                    let text = match &c["content"] {
                        Value::String(s) => s.clone(),
                        Value::Array(parts) => parts
                            .iter()
                            .filter_map(|p| p["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        _ => String::new(),
                    };
                    out.push(json!({
                        "kind": "tool_result", "id": c["tool_use_id"],
                        "output": clip(&text), "is_error": c["is_error"].as_bool().unwrap_or(false),
                    }));
                }
            }
        }
        Some("result") => {
            let window = j["modelUsage"]
                .as_object()
                .and_then(|m| m.values().filter_map(|v| v["contextWindow"].as_u64()).max());
            out.push(json!({
                "kind": "done",
                "is_error": j["is_error"].as_bool().unwrap_or(false),
                "cost_usd": j["total_cost_usd"],
                "duration_ms": j["duration_ms"],
                "context_window": window,
            }))
        }
        _ => {}
    }
    out
}

/// Codex `exec --json` line -> zero or more events.
pub fn parse_codex(line: &str) -> Vec<Value> {
    let Ok(j) = serde_json::from_str::<Value>(line) else { return vec![] };
    let item = &j["item"];
    match (j["type"].as_str(), item["type"].as_str()) {
        (Some("thread.started"), _) => vec![json!({"kind": "session", "agent_session_id": j["thread_id"]})],
        (Some("item.started"), Some("command_execution")) => {
            vec![json!({"kind": "tool", "id": item["id"], "name": "Bash", "input": item["command"]})]
        }
        (Some("item.completed"), Some("command_execution")) => vec![json!({
            "kind": "tool_result", "id": item["id"],
            "output": clip(item["aggregated_output"].as_str().unwrap_or("")),
            "is_error": item["exit_code"].as_i64().unwrap_or(0) != 0,
        })],
        (Some("item.completed"), Some("file_change")) => {
            let files: Vec<String> = item["changes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c["path"].as_str().map(str::to_string))
                .collect();
            vec![
                json!({"kind": "tool", "id": item["id"], "name": "Edit", "input": files.join(", ")}),
                json!({"kind": "tool_result", "id": item["id"], "output": "", "is_error": false}),
            ]
        }
        (Some("item.completed"), Some("agent_message")) => vec![json!({"kind": "text", "text": item["text"]})],
        (Some("item.completed"), Some("reasoning")) => vec![json!({"kind": "thinking", "text": item["text"]})],
        (Some("turn.completed"), _) => vec![json!({"kind": "done", "is_error": false, "usage": j["usage"]})],
        (Some("turn.failed"), _) | (Some("error"), _) => {
            let msg = j["error"]["message"].as_str().or(j["message"].as_str()).unwrap_or("Codex error");
            vec![json!({"kind": "error", "text": msg}), json!({"kind": "done", "is_error": true})]
        }
        _ => vec![],
    }
}

/// Gemini CLI `--output-format stream-json` -> events. Stateful: assistant text arrives as
/// `delta` chunks, which are joined into one message and sent when a tool call, an error or the
/// end of the turn comes.
#[derive(Default)]
pub struct GeminiParser {
    text: String,
}

impl GeminiParser {
    fn flush(&mut self, out: &mut Vec<Value>) {
        let t = std::mem::take(&mut self.text);
        if !t.trim().is_empty() {
            out.push(json!({"kind": "text", "text": t}));
        }
    }
}

/// Gemini's built-in tools under the names the chat already draws (icons, todo list).
pub(crate) fn gemini_tool(name: &str, input: &Value) -> (String, String) {
    let mapped = match name {
        "run_shell_command" => "Bash",
        "read_file" => "Read",
        "write_file" => "Write",
        "replace" => "Edit",
        "glob" => "Glob",
        "grep_search" | "search_file_content" => "Grep",
        "web_fetch" => "WebFetch",
        "google_web_search" => "WebSearch",
        "write_todos" => "TodoWrite",
        other => other,
    };
    let summary = match mapped {
        // Gemini's todos are `{description, status}`; the chat reads `{content, status}`.
        "TodoWrite" => {
            let todos: Vec<Value> = input["todos"].as_array().into_iter().flatten()
                .map(|t| json!({"content": t["description"].as_str().or(t["content"].as_str()).unwrap_or(""), "status": t["status"]}))
                .collect();
            Value::Array(todos).to_string()
        }
        "list_directory" => input["dir_path"].as_str().map(str::to_string).unwrap_or_else(|| summarize_input(mapped, input)),
        _ => summarize_input(mapped, input),
    };
    (mapped.to_string(), summary)
}

impl Parser for GeminiParser {
    fn line(&mut self, line: &str) -> Vec<Value> {
        let Ok(j) = serde_json::from_str::<Value>(line) else { return vec![] };
        let mut out = vec![];
        match j["type"].as_str() {
            Some("init") => {
                if let Some(id) = j["session_id"].as_str() {
                    out.push(json!({"kind": "session", "agent_session_id": id}));
                }
            }
            Some("message") if j["role"] == "assistant" => {
                if let Some(t) = j["content"].as_str() {
                    self.text.push_str(t);
                }
                if j["delta"] != true {
                    self.flush(&mut out);
                }
            }
            Some("tool_use") => {
                self.flush(&mut out);
                let (name, input) = gemini_tool(j["tool_name"].as_str().unwrap_or("tool"), &j["parameters"]);
                out.push(json!({"kind": "tool", "id": j["tool_id"], "name": name, "input": input}));
            }
            Some("tool_result") => {
                self.flush(&mut out);
                let is_error = j["status"] == "error";
                let output = j["output"].as_str().filter(|o| !o.is_empty())
                    .or_else(|| j["error"]["message"].as_str())
                    .unwrap_or("");
                out.push(json!({"kind": "tool_result", "id": j["tool_id"], "output": clip(output), "is_error": is_error}));
            }
            Some("error") => {
                self.flush(&mut out);
                if let Some(m) = j["message"].as_str() {
                    out.push(json!({"kind": "error", "text": m}));
                }
            }
            Some("result") => {
                self.flush(&mut out);
                // Stop sends SIGINT; Gemini answers with this result and exits 130.
                if j["error"]["type"] == "FatalCancellationError" {
                    out.push(json!({"kind": "done", "is_error": true, "stopped": true}));
                    return out;
                }
                let failed = j["status"] != "success";
                if failed {
                    let m = j["error"]["message"].as_str().unwrap_or("Gemini stopped with an error.");
                    out.push(json!({"kind": "error", "text": m}));
                }
                out.push(json!({"kind": "done", "is_error": failed, "duration_ms": j["stats"]["duration_ms"], "usage": j["stats"]}));
            }
            _ => {}
        }
        out
    }

    fn finish(&mut self) -> Vec<Value> {
        let mut out = vec![];
        self.flush(&mut out);
        out
    }
}

pub struct TurnSpec<'a> {
    /// Resume `agent_session_id` as a new branch of the conversation (Claude `--fork-session`).
    pub fork: bool,
    pub agent: &'a str,
    pub prompt: &'a str,
    pub agent_session_id: Option<&'a str>,
    pub model: Option<&'a str>,
    /// `plan`, `default`, `acceptEdits` or `bypassPermissions` (Claude Code names).
    pub permission_mode: &'a str,
    pub cwd: &'a Path,
    /// low | medium | high | xhigh | max (Claude); Codex gets the closest reasoning effort.
    pub effort: Option<&'a str>,
    /// Repo-wide instructions added to the agent's system prompt.
    pub instructions: Option<&'a str>,
    /// Extra environment variables from the repo's settings.
    pub env: &'a [(String, String)],
    /// Other folders the agent may use (/add-dir).
    pub dirs: &'a [String],
    /// Load the agent config the repo commits (`.claude/settings.json`, `.mcp.json`, `.codex/`).
    /// False until the user trusted what it runs (see `agentcfg`).
    pub project_config: bool,
    /// Protected paths for this workspace (see `guard`). None or empty: nothing is protected.
    pub guard: Option<&'a crate::guard::Rules>,
}

/// Values from a `TurnSpec` that land on a command line, already checked. The prompt can carry
/// text an agent or a GitHub issue wrote, and a leading `-` would be read as an option
/// (`--settings=...` = hooks = shell).
pub struct Checked<'a> {
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub session: Option<&'a str>,
}

/// Reads one turn's stdout, line by line, into chat events. Parsers that talk back to the agent
/// (ACP) also hand over what to write to its stdin, and answer approvals and Stop themselves.
pub trait Parser: Send {
    fn line(&mut self, line: &str) -> Vec<Value>;
    /// Called once when stdout closes, for anything still held back.
    fn finish(&mut self) -> Vec<Value> {
        vec![]
    }
    /// Lines for the agent's stdin: read once when the turn starts and after every `line`.
    fn outgoing(&mut self) -> Vec<String> {
        vec![]
    }
    /// The reply to an `approval` event this parser raised. None: not this parser's to answer
    /// (Claude's `can_use_tool` requests go through `approval_response`).
    fn answer(&mut self, _request_id: &str, _allow: bool, _always: bool) -> Option<Result<String, String>> {
        None
    }
    /// Lines that ask the agent to stop this turn. None: stop it with SIGINT instead.
    fn cancel(&mut self) -> Option<Vec<String>> {
        None
    }
    /// The turn has its answer: close stdin so the agent exits.
    fn finished(&self) -> bool {
        false
    }
}

/// A parser that needs no memory between lines (Claude, Codex).
struct Lines(fn(&str) -> Vec<Value>);

impl Parser for Lines {
    fn line(&mut self, line: &str) -> Vec<Value> {
        (self.0)(line)
    }
}

/// One agent CLI nekotrail can drive. Each turn is one headless process: nekotrail builds the
/// command, reads its stdout through `parser`, and stops it by killing the process. Adding an
/// agent is one more struct here and one line in `ALL`.
///
/// A trait rather than an enum: every agent is a unit struct, the turn code only ever holds a
/// `&'static dyn Agent`, and the per-turn state lives in the boxed `Parser`, so nothing async or
/// generic leaks into `lib.rs`.
pub trait Agent: Sync {
    /// Stored on each chat: "claude", "codex", "gemini".
    fn id(&self) -> &'static str;
    /// The CLI on PATH (doctor, `mcp list`).
    fn bin(&self) -> &'static str {
        self.id()
    }
    /// What the setup check runs to see the CLI is installed: `<bin> --version` by default.
    fn probe(&self) -> (&'static str, &'static [&'static str]) {
        (self.bin(), &["--version"])
    }
    /// The CLI and its arguments for one turn. `command` adds env, cwd and stdin.
    fn build(&self, spec: &TurnSpec, ok: &Checked) -> Command;
    /// A fresh reader for one turn's stdout.
    fn parser(&self, spec: &TurnSpec, ok: &Checked) -> Box<dyn Parser>;
    /// Stdin stays open for the whole turn: the prompt and the answers to permission requests go
    /// in there (Claude's "ask" mode, every ACP turn).
    fn interactive(&self, _permission_mode: &str) -> bool {
        false
    }
    /// What an interactive turn writes first. None: the parser says what to write (`outgoing`).
    fn first_input(&self, prompt: &str) -> Option<String> {
        Some(user_message(prompt))
    }
    /// Can continue a chat as a new branch (`fork_session`) without changing the original.
    fn forks(&self) -> bool {
        false
    }
    /// What the repo's own config for this agent would run, and its fingerprint (see `agentcfg`).
    fn repo_config(&self, dir: &Path) -> Option<(Vec<crate::agentcfg::Item>, String)> {
        crate::agentcfg::check(dir)
    }
    /// What to tell the user when the repo's config is untrusted and this agent has no way to run
    /// with it switched off. None: it runs with the config off (`TurnSpec::project_config`).
    fn needs_trusted_config(&self) -> Option<String> {
        None
    }
    /// Environment that keeps the repo's own config for this agent switched off, for anything
    /// else nekotrail runs with this CLI in the workspace (`mcp list`). Empty: nothing to add.
    fn config_off_env(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
    /// The process ended without finishing its turn: was that the user pressing Stop (SIGINT)?
    /// Claude and Codex exit quietly when interrupted, so a failure with nothing on stderr is a stop.
    fn stopped_by_user(&self, status: Option<&ExitStatus>, stderr: &str) -> bool {
        status.map(|s| !s.success()).unwrap_or(true) && stderr.is_empty()
    }
    /// How protected paths hold for this agent: "blocked" (the agent is stopped before it writes
    /// one, and every turn is checked after too) or "after" (only checked after each turn).
    fn guard_level(&self) -> &'static str {
        "after"
    }
}

/// The protected paths of a turn, when there are any.
pub fn active_guard<'a>(spec: &TurnSpec<'a>) -> Option<&'a crate::guard::Rules> {
    spec.guard.filter(|g| !g.is_empty())
}

/// The workspace folder as written and with symlinks resolved, when that differs.
pub fn roots(cwd: &Path) -> Vec<String> {
    let mut v = vec![cwd.to_string_lossy().to_string()];
    if let Ok(real) = cwd.canonicalize()
        && real != cwd {
            v.push(real.to_string_lossy().to_string());
        }
    v
}

pub struct Claude;
pub struct Codex;
pub struct Gemini;

/// Every agent, in the order the UI lists them. ACP agents are `acp::AcpAgent` entries.
pub static ALL: [&dyn Agent; 16] = {
    use crate::acp::*;
    [&Claude, &Codex, &Gemini, &OPENCODE, &CURSOR, &COPILOT, &GOOSE, &QWEN, &KIMI, &AUGGIE, &GROK, &KIRO, &DROID, &VIBE, &CLINE, &KILO]
};

/// The agent for a stored id. Anything unknown is Claude, as it always was.
pub fn get(id: &str) -> &'static dyn Agent {
    ALL.iter().copied().find(|a| a.id() == id).unwrap_or(&Claude)
}

impl Agent for Claude {
    fn id(&self) -> &'static str {
        "claude"
    }
    fn parser(&self, _: &TurnSpec, _: &Checked) -> Box<dyn Parser> {
        Box::new(Lines(parse_claude))
    }
    fn interactive(&self, permission_mode: &str) -> bool {
        permission_mode == "ask"
    }
    fn forks(&self) -> bool {
        true
    }
    fn build(&self, spec: &TurnSpec, ok: &Checked) -> Command {
        let mut c = Command::new("claude");
        if self.interactive(spec.permission_mode) {
            // "Ask before changes": the prompt goes in on stdin, and every permission prompt comes
            // back to nekotrail as a `can_use_tool` control request for the user to answer.
            c.args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"]);
            c.args(["--permission-mode", "default", "--permission-prompts", "host", "--permission-prompt-tool", "stdio"]);
        } else {
            c.args(["-p", &crate::safe::positional(spec.prompt), "--output-format", "stream-json", "--verbose"]);
            c.args(["--permission-mode", spec.permission_mode]);
        }
        if !spec.project_config {
            // Project and local settings carry the hooks, apiKeyHelper and env; `user` alone also
            // keeps the repo's .mcp.json servers from starting. The user's own settings and MCP stay.
            c.args(["--setting-sources", "user"]);
        }
        if let Some(m) = ok.model {
            c.args(["--model", m]);
        }
        if let Some(id) = ok.session {
            c.args(["--resume", id]);
            if spec.fork {
                c.arg("--fork-session");
            }
        }
        if let Some(e) = ok.effort {
            c.args(["--effort", e]);
        }
        if let Some(i) = spec.instructions {
            c.args(["--append-system-prompt", i]);
        }
        for d in spec.dirs {
            c.args(["--add-dir", d]);
        }
        if let Some(g) = active_guard(spec) {
            // Deny rules hold in every permission mode, Full auto included, and add to the user's
            // own. `--settings` takes JSON, so paths with spaces or commas stay one rule each, and
            // nothing is written into the worktree. It still applies with `--setting-sources user`.
            let deny = g.claude_deny(&roots(spec.cwd));
            if !deny.is_empty() {
                c.args(["--settings", &json!({"permissions": {"deny": deny}}).to_string()]);
            }
        }
        c
    }
    fn guard_level(&self) -> &'static str {
        "blocked"
    }
}

impl Agent for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }
    fn parser(&self, _: &TurnSpec, _: &Checked) -> Box<dyn Parser> {
        Box::new(Lines(parse_codex))
    }
    fn build(&self, spec: &TurnSpec, ok: &Checked) -> Command {
        let sandbox = match spec.permission_mode {
            "plan" => "read-only",
            "bypassPermissions" => "danger-full-access",
            _ => "workspace-write",
        };
        let mut c = Command::new("codex");
        c.arg("exec");
        if let Some(id) = ok.session {
            c.args(["resume", id]);
        }
        c.args(["--json", "--skip-git-repo-check", "-c", &format!("sandbox_mode=\"{sandbox}\"")]);
        if !spec.project_config {
            c.args(["-c", &codex_untrusted(spec.cwd)]);
        }
        if let Some(m) = ok.model {
            c.args(["-m", m]);
        }
        for d in spec.dirs {
            c.args(["--add-dir", d]);
        }
        if let Some(e) = ok.effort {
            // The model catalog only offers levels the chosen model supports, so pass them through.
            let e = if e.chars().all(|ch| ch.is_ascii_lowercase()) { e } else { "high" };
            c.args(["-c", &format!("model_reasoning_effort=\"{e}\"")]);
        }
        // Codex has no flag for extra instructions, so they lead the first message of a chat.
        match spec.instructions.filter(|_| ok.session.is_none()) {
            Some(i) => c.arg(crate::safe::positional(&format!("{i}\n\n{}", spec.prompt))),
            None => c.arg(crate::safe::positional(spec.prompt)),
        };
        c
    }
}

impl Agent for Gemini {
    fn id(&self) -> &'static str {
        "gemini"
    }
    fn parser(&self, _: &TurnSpec, _: &Checked) -> Box<dyn Parser> {
        Box::new(GeminiParser::default())
    }
    fn build(&self, spec: &TurnSpec, ok: &Checked) -> Command {
        let mode = match spec.permission_mode {
            "plan" => "plan",
            "bypassPermissions" => "yolo",
            "default" => "default",
            // acceptEdits, and "ask", which only Claude has.
            _ => "auto_edit",
        };
        let mut c = Command::new("gemini");
        // Headless Gemini refuses to run in a folder it was never told to trust, and every
        // workspace is a new folder. The repo's own `.gemini/` config and `.env` are covered by
        // nekotrail's trust check instead (`repo_config`, `needs_trusted_config`).
        c.args(["--output-format", "stream-json", "--approval-mode", mode, "--skip-trust"]);
        if let Some(m) = ok.model {
            c.args(["-m", m]);
        }
        if let Some(id) = ok.session {
            c.args(["--resume", id]);
        }
        for d in spec.dirs {
            c.args(["--include-directories", d]);
        }
        // No flag for extra instructions or effort. Instructions lead the first message, like Codex.
        let prompt = match spec.instructions.filter(|_| ok.session.is_none()) {
            Some(i) => format!("{i}\n\n{}", spec.prompt),
            None => spec.prompt.to_string(),
        };
        // `--prompt=` keeps a prompt that starts with `-` a value, never an option.
        c.arg(format!("--prompt={prompt}"));
        c
    }
    fn stopped_by_user(&self, status: Option<&ExitStatus>, stderr: &str) -> bool {
        // Gemini prints notices to stderr on every run ("YOLO mode is enabled", cached
        // credentials), and exits 130 when interrupted.
        status.and_then(ExitStatus::code) == Some(130) || status.map(|s| !s.success()).unwrap_or(true) && stderr.is_empty()
    }
    fn repo_config(&self, dir: &Path) -> Option<(Vec<crate::agentcfg::Item>, String)> {
        crate::agentcfg::check_gemini(dir)
    }
    fn needs_trusted_config(&self) -> Option<String> {
        Some("This repo's Gemini settings run commands (hooks, MCP servers or env), and Gemini can't run with them switched off. Send again and choose Trust, or use Claude or Codex for this task.".into())
    }
}

fn checked<'a>(spec: &TurnSpec<'a>) -> Checked<'a> {
    Checked {
        model: spec.model.filter(|m| crate::safe::model_id(m)),
        effort: spec.effort.filter(|e| crate::safe::model_id(e)),
        session: spec.agent_session_id.filter(|id| crate::safe::not_a_flag(id)),
    }
}

/// Build the CLI command for one turn.
pub fn command(spec: &TurnSpec) -> Command {
    let ok = checked(spec);
    let mut c = get(spec.agent).build(spec, &ok);
    // What the agent sets itself (an ACP agent's "repo config off" switch) beats the repo's env.
    let own: Vec<std::ffi::OsString> = c.get_envs().map(|(k, _)| k.to_os_string()).collect();
    c.envs(spec.env.iter().filter(|(k, _)| !own.iter().any(|o| o == k.as_str())).map(|(k, v)| (k.as_str(), v.as_str())));
    c.current_dir(spec.cwd);
    c.stdin(if interactive(spec) { std::process::Stdio::piped() } else { std::process::Stdio::null() });
    c
}

/// The reader for one turn's stdout, with the same checked values the command got.
pub fn parser(spec: &TurnSpec) -> Box<dyn Parser> {
    get(spec.agent).parser(spec, &checked(spec))
}

/// A reader for a stream recorded elsewhere (Conductor's chat history), outside any turn.
pub fn replay_parser(agent: &str) -> Box<dyn Parser> {
    let spec = TurnSpec { fork: false, agent, prompt: "", agent_session_id: None, model: None, permission_mode: "default", cwd: Path::new("."), effort: None, instructions: None, env: &[], dirs: &[], project_config: false, guard: None };
    parser(&spec)
}

/// `projects={"<cwd>"={trust_level="untrusted"}}`: Codex then skips the folder's `.codex/` layer
/// (config, rules, hooks) for this run, even inside a folder the user trusted. The dotted form
/// `projects."<path>".trust_level` is not read as one key, so the table is written inline.
fn codex_untrusted(cwd: &Path) -> String {
    let mut paths = vec![cwd.to_string_lossy().to_string()];
    if let Ok(real) = cwd.canonicalize()
        && real != cwd {
            paths.insert(0, real.to_string_lossy().to_string());
        }
    // A JSON string is a valid TOML basic string, so quotes in a path stay inside the key.
    let entries: Vec<String> = paths.into_iter().map(|p| format!("{}={{trust_level=\"untrusted\"}}", Value::String(p))).collect();
    format!("projects={{{}}}", entries.join(","))
}

/// Turns where the user approves each risky tool call (Claude's "ask before changes" mode).
pub fn interactive(spec: &TurnSpec) -> bool {
    get(spec.agent).interactive(spec.permission_mode)
}

/// The first message of an interactive turn, as a stream-json line for stdin.
pub fn user_message(prompt: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": prompt}}).to_string() + "\n"
}

/// The answer to a `can_use_tool` request. `always` also applies the CLI's own suggestion (for
/// edits: accept edits for the rest of this chat).
pub fn approval_response(request_id: &str, allow: bool, input: &Value, suggestions: &Value, always: bool) -> String {
    let response = if allow {
        let mut r = json!({"behavior": "allow", "updatedInput": input});
        if always && suggestions.is_array() {
            r["updatedPermissions"] = suggestions.clone();
        }
        r
    } else {
        json!({"behavior": "deny", "message": "The user said no. Don't retry this; ask what they want instead."})
    };
    json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response}}).to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: &str = include_str!("../tests/fixtures/claude.jsonl");
    const CODEX: &str = include_str!("../tests/fixtures/codex.jsonl");
    const GEMINI: &str = include_str!("../tests/fixtures/gemini.jsonl");

    fn kinds(evs: &[Value]) -> Vec<&str> {
        evs.iter().map(|e| e["kind"].as_str().unwrap()).collect()
    }

    #[test]
    fn claude_stream() {
        let evs: Vec<Value> = CLAUDE.lines().flat_map(parse_claude).collect();
        let evs: Vec<Value> = evs.into_iter().filter(|e| e["kind"] != "usage").collect();
        assert_eq!(kinds(&evs), ["session", "tool", "tool_result", "text", "done"]);
        assert_eq!(evs[0]["agent_session_id"], "11111111-2222-3333-4444-555555555555");
        assert_eq!(evs[1]["input"], "cat a.txt");
        assert_eq!(evs[2]["output"], "hi");
        assert_eq!(evs[3]["text"], "DONE");
        assert_eq!(evs[4]["is_error"], false);
    }

    #[test]
    fn codex_stream() {
        let evs: Vec<Value> = CODEX.lines().flat_map(parse_codex).collect();
        assert_eq!(kinds(&evs), ["session", "tool", "tool_result", "text", "done"]);
        assert_eq!(evs[0]["agent_session_id"], "thread-1");
        assert_eq!(evs[2]["output"], "hi\n");
        assert_eq!(evs[3]["text"], "DONE");
    }

    #[test]
    fn junk_lines_are_ignored() {
        assert!(parse_claude("not json").is_empty());
        assert!(parse_codex("Reading additional input from stdin...").is_empty());
    }

    #[test]
    fn long_output_is_clipped_on_a_char_boundary() {
        let s = "é".repeat(3000);
        let c = clip(&s);
        assert!(c.contains("more bytes"));
    }

    #[test]
    fn ask_mode_uses_the_control_channel() {
        let cwd = Path::new("/tmp");
        let spec = TurnSpec { fork: false, agent: "claude", prompt: "hi", agent_session_id: None, model: None, permission_mode: "ask", cwd, effort: None, instructions: None, env: &[], dirs: &[], project_config: true, guard: None };
        assert!(interactive(&spec));
        let args: Vec<_> = command(&spec).get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.windows(2).any(|w| w == ["--permission-prompt-tool", "stdio"]));
        assert!(args.windows(2).any(|w| w == ["--permission-mode", "default"]));
        assert!(!args.contains(&"hi".to_string()), "the prompt goes in on stdin");
        let ok: Value = serde_json::from_str(&approval_response("r1", true, &json!({"a": 1}), &json!([{"type": "setMode"}]), true)).unwrap();
        assert_eq!(ok["response"]["request_id"], "r1");
        assert_eq!(ok["response"]["response"]["behavior"], "allow");
        assert_eq!(ok["response"]["response"]["updatedPermissions"][0]["type"], "setMode");
        let no: Value = serde_json::from_str(&approval_response("r2", false, &json!({}), &Value::Null, false)).unwrap();
        assert_eq!(no["response"]["response"]["behavior"], "deny");
    }

    #[test]
    fn commands() {
        let cwd = Path::new("/tmp");
        let c = command(&TurnSpec { fork: false, agent: "claude", prompt: "hi", agent_session_id: Some("s1"), model: Some("opus"), permission_mode: "plan", cwd, effort: Some("high"), instructions: Some("Use pnpm."), env: &[], dirs: &["/code/shared".into()], project_config: true, guard: None });
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.windows(2).any(|w| w == ["--add-dir", "/code/shared"]));
        assert!(args.windows(2).any(|w| w == ["--effort", "high"]));
        assert!(args.windows(2).any(|w| w == ["--append-system-prompt", "Use pnpm."]));
        assert!(args.windows(2).any(|w| w == ["--resume", "s1"]));
        assert!(args.windows(2).any(|w| w == ["--permission-mode", "plan"]));

        let c = command(&TurnSpec { fork: false, agent: "codex", prompt: "hi", agent_session_id: Some("t1"), model: None, permission_mode: "plan", cwd, effort: None, instructions: Some("Use pnpm."), env: &[], dirs: &[], project_config: true, guard: None });
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert_eq!(&args[..3], ["exec", "resume", "t1"]);
        assert!(args.contains(&"sandbox_mode=\"read-only\"".to_string()));
        assert_eq!(args.last().unwrap(), "hi", "instructions only lead a chat's first message");
        assert!(!args.iter().any(|a| a.starts_with("projects=")), "trusted repo config loads as usual");
    }

    #[test]
    fn untrusted_repo_config_is_switched_off() {
        let d = tempfile::tempdir().unwrap();
        let cwd = d.path();
        let spec = |agent, mode, project_config| TurnSpec { fork: false, agent, prompt: "hi", agent_session_id: None, model: None, permission_mode: mode, cwd, effort: None, instructions: None, env: &[], dirs: &[], project_config, guard: None };
        let args = |s: &TurnSpec| command(s).get_args().map(|a| a.to_string_lossy().to_string()).collect::<Vec<_>>();
        for mode in ["default", "ask", "bypassPermissions"] {
            assert!(args(&spec("claude", mode, false)).windows(2).any(|w| w == ["--setting-sources", "user"]), "{mode}");
            assert!(!args(&spec("claude", mode, true)).contains(&"--setting-sources".to_string()));
        }
        let a = args(&spec("codex", "default", false));
        let i = a.iter().position(|x| x.starts_with("projects=")).expect("codex override");
        assert_eq!(a[i - 1], "-c");
        assert_eq!(a.last().unwrap(), "hi");
        // It has to parse as TOML to this exact table, or codex would read it as a plain string.
        let t: toml::Table = a[i].parse().unwrap();
        let p = t["projects"].as_table().unwrap();
        // The temp dir is under /var, which is /private/var: both spellings are listed.
        for path in [cwd.to_path_buf(), cwd.canonicalize().unwrap()] {
            assert_eq!(p[path.to_str().unwrap()]["trust_level"].as_str(), Some("untrusted"), "{path:?}");
        }
        assert!(!args(&spec("codex", "default", true)).iter().any(|x| x.starts_with("projects=")));
        // Quotes in a path stay inside the key.
        let t: toml::Table = codex_untrusted(Path::new("/tmp/a\"b\\c]}")).parse().unwrap();
        assert_eq!(t["projects"].as_table().unwrap().keys().collect::<Vec<_>>(), ["/tmp/a\"b\\c]}"]);
    }

    #[test]
    fn protected_paths_reach_claude_as_deny_rules() {
        let cwd = Path::new("/tmp/ws one");
        let g = crate::guard::Rules::new(&[".env*".into(), "migrations/**".into()], &[]);
        let none = crate::guard::Rules::new(&["!x".into()], &[]);
        let args = |agent, mode, guard| {
            let s = TurnSpec { fork: false, agent, prompt: "hi", agent_session_id: None, model: None, permission_mode: mode, cwd, effort: None, instructions: None, env: &[], dirs: &[], project_config: false, guard };
            command(&s).get_args().map(|a| a.to_string_lossy().to_string()).collect::<Vec<_>>()
        };
        for mode in ["default", "ask", "acceptEdits", "bypassPermissions", "plan"] {
            let a = args("claude", mode, Some(&g));
            let i = a.iter().position(|x| x == "--settings").expect(mode);
            let v: Value = serde_json::from_str(&a[i + 1]).unwrap();
            let deny: Vec<&str> = v["permissions"]["deny"].as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
            assert!(deny.contains(&"Edit(.env*)"), "{mode}");
            // /tmp is /private/tmp on macOS: both spellings, the space kept inside one rule.
            assert!(deny.contains(&"Edit(//tmp/ws one/migrations/**)"), "{deny:?}");
            // Repo config stays off as before.
            assert!(a.windows(2).any(|w| w == ["--setting-sources", "user"]));
        }
        assert!(!args("claude", "default", Some(&none)).contains(&"--settings".to_string()), "only exceptions: nothing to deny");
        assert!(!args("claude", "default", None).contains(&"--settings".to_string()));
        // Codex and Gemini have nothing to pass: they are checked after the turn.
        for agent in ["codex", "gemini"] {
            assert_eq!(args(agent, "default", Some(&g)), args(agent, "default", None), "{agent}");
            assert_eq!(get(agent).guard_level(), "after");
        }
        assert_eq!(get("claude").guard_level(), "blocked");
        assert_eq!(get("opencode").guard_level(), "blocked");
    }

    // ---------- agent harness ----------

    fn run(p: &mut dyn Parser, text: &str) -> Vec<Value> {
        let mut evs: Vec<Value> = text.lines().flat_map(|l| p.line(l)).collect();
        evs.extend(p.finish());
        evs
    }

    fn spec_args(agent: &str, mode: &str, session: Option<&str>, prompt: &str) -> Vec<String> {
        let dirs = ["/code/shared".to_string()];
        let s = TurnSpec { fork: true, agent, prompt, agent_session_id: session, model: Some("flash"), permission_mode: mode, cwd: Path::new("/tmp"), effort: Some("high"), instructions: Some("Use pnpm."), env: &[], dirs: &dirs, project_config: true, guard: None };
        command(&s).get_args().map(|a| a.to_string_lossy().to_string()).collect()
    }

    #[test]
    fn registry() {
        let ids: Vec<_> = ALL.iter().map(|a| a.id()).collect();
        let acp = ["opencode", "cursor", "copilot", "goose", "qwen", "kimi", "auggie", "grok", "kiro", "droid", "vibe", "cline", "kilo"];
        assert_eq!(ids, [&["claude", "codex", "gemini"][..], &acp[..]].concat());
        for a in ALL {
            assert_eq!(get(a.id()).id(), a.id());
        }
        // Ids are unique, and so are the CLIs (Cursor is `cursor-agent`: Grok Build owns `agent`).
        let mut bins: Vec<_> = ALL.iter().map(|a| a.bin()).collect();
        bins.sort();
        bins.dedup();
        assert_eq!(bins.len(), ALL.len());
        assert!(!bins.contains(&"agent"));
        // Old or unknown values keep meaning Claude, as before.
        assert_eq!(get("").id(), "claude");
        assert_eq!(get("amp").id(), "claude");
        // Claude asks mid-turn in "ask" mode; ACP agents keep stdin open for every turn.
        let ask: Vec<_> = ALL.iter().filter(|a| a.interactive("ask")).map(|a| a.id()).collect();
        assert_eq!(ask, [&["claude"][..], &acp[..]].concat());
        assert_eq!(ALL.iter().filter(|a| a.interactive("acceptEdits")).map(|a| a.id()).collect::<Vec<_>>(), acp);
        // Only Claude forks.
        assert!(!get("claude").interactive("acceptEdits"));
        let forks: Vec<_> = ALL.iter().filter(|a| a.forks()).map(|a| a.id()).collect();
        assert_eq!(forks, ["claude"]);
        // Only OpenCode can run with an untrusted repo config switched off; the rest stop instead.
        let stop: Vec<_> = ALL.iter().filter(|a| a.needs_trusted_config().is_some()).map(|a| a.id()).collect();
        assert_eq!(stop, [&["gemini"][..], &acp[1..]].concat());
        // The setup check asks each CLI for its version; Vibe's ACP server is a second binary.
        assert_eq!(get("cursor").probe(), ("cursor-agent", &["--version"][..]));
        assert_eq!(get("vibe").probe(), ("vibe", &["--version"][..]));
        assert_eq!((get("vibe").bin(), get("claude").probe().0), ("vibe-acp", "claude"));
    }

    #[test]
    fn gemini_stream() {
        let evs = run(&mut GeminiParser::default(), GEMINI);
        assert_eq!(kinds(&evs), ["session", "text", "tool", "tool_result", "text", "done"]);
        assert_eq!(evs[0]["agent_session_id"], "9b2f3c1e-0000-4000-8000-000000000001");
        assert_eq!(evs[1]["text"], "I'll read the file.", "deltas join into one message");
        assert_eq!((evs[2]["name"].as_str(), evs[2]["input"].as_str(), evs[2]["id"].as_str()), (Some("Bash"), Some("cat a.txt"), Some("run_shell_command-1")));
        assert_eq!((evs[3]["id"].as_str(), evs[3]["output"].as_str(), evs[3]["is_error"].as_bool()), (Some("run_shell_command-1"), Some("hi"), Some(false)));
        assert_eq!(evs[4]["text"], "DONE");
        assert_eq!(evs[5]["is_error"], false);
        assert_eq!(evs[5]["duration_ms"], 2100);
        assert!(evs[5]["stopped"].is_null());
    }

    #[test]
    fn gemini_text_left_at_the_end_is_kept() {
        // Killed mid-answer: no result line, the text still reaches the chat.
        let evs = run(&mut GeminiParser::default(), r#"{"type":"message","role":"assistant","content":"half an an","delta":true}
{"type":"message","role":"assistant","content":"swer","delta":true}"#);
        assert_eq!(kinds(&evs), ["text"]);
        assert_eq!(evs[0]["text"], "half an answer");
        // A whole (non-delta) message goes out at once.
        let mut p = GeminiParser::default();
        assert_eq!(kinds(&p.line(r#"{"type":"message","role":"assistant","content":"all of it"}"#)), ["text"]);
        assert!(p.finish().is_empty());
    }

    #[test]
    fn gemini_errors_stops_and_tools() {
        let mut p = GeminiParser::default();
        let evs = p.line(r#"{"type":"result","status":"error","error":{"type":"FatalCancellationError","message":"Operation cancelled."},"stats":{}}"#);
        assert_eq!(kinds(&evs), ["done"]);
        assert_eq!((evs[0]["stopped"].as_bool(), evs[0]["is_error"].as_bool()), (Some(true), Some(true)));

        let evs = p.line(r#"{"type":"result","status":"error","error":{"type":"Error","message":"Usage limit reached for gemini-2.5-pro."},"stats":{"duration_ms":5}}"#);
        assert_eq!(kinds(&evs), ["error", "done"]);
        assert_eq!(evs[0]["text"], "Usage limit reached for gemini-2.5-pro.");
        assert_eq!(evs[1]["is_error"], true);

        let evs = p.line(r#"{"type":"error","severity":"warning","message":"Loop detected"}"#);
        assert_eq!((kinds(&evs), evs[0]["text"].as_str()), (vec!["error"], Some("Loop detected")));

        let evs = p.line(r#"{"type":"tool_result","tool_id":"t1","status":"error","output":"","error":{"type":"x","message":"File not found"}}"#);
        assert_eq!((evs[0]["output"].as_str(), evs[0]["is_error"].as_bool()), (Some("File not found"), Some(true)));

        let evs = p.line(r#"{"type":"tool_use","tool_name":"replace","tool_id":"t2","parameters":{"file_path":"src/a.rs","old_string":"a","new_string":"b"}}"#);
        assert_eq!((evs[0]["name"].as_str(), evs[0]["input"].as_str()), (Some("Edit"), Some("src/a.rs")));
        // Todos come out in the shape the chat's todo list reads.
        let evs = p.line(r#"{"type":"tool_use","tool_name":"write_todos","tool_id":"t3","parameters":{"todos":[{"description":"Read code","status":"completed"},{"description":"Fix bug","status":"in_progress"}]}}"#);
        assert_eq!(evs[0]["name"], "TodoWrite");
        let todos: Value = serde_json::from_str(evs[0]["input"].as_str().unwrap()).unwrap();
        assert_eq!(todos, json!([{"content": "Read code", "status": "completed"}, {"content": "Fix bug", "status": "in_progress"}]));
        // Unknown tools keep their own name.
        let evs = p.line(r#"{"type":"tool_use","tool_name":"mcp_docs_search","tool_id":"t4","parameters":{"q":"x"}}"#);
        assert_eq!(evs[0]["name"], "mcp_docs_search");
        assert!(p.line("Loaded cached credentials.").is_empty());
        assert!(p.line(r#"{"type":"message","role":"user","content":"hi"}"#).is_empty());
    }

    #[test]
    fn gemini_command() {
        let a = spec_args("gemini", "acceptEdits", None, "-rf everything");
        assert!(a.windows(2).any(|w| w == ["--output-format", "stream-json"]));
        assert!(a.windows(2).any(|w| w == ["--approval-mode", "auto_edit"]));
        assert!(a.contains(&"--skip-trust".to_string()));
        assert!(a.windows(2).any(|w| w == ["-m", "flash"]));
        assert!(a.windows(2).any(|w| w == ["--include-directories", "/code/shared"]));
        assert!(!a.iter().any(|x| x.contains("high")), "Gemini has no effort flag");
        assert!(!a.contains(&"--resume".to_string()));
        // The prompt is one `--prompt=` value, so a leading dash is never an option.
        assert_eq!(a.last().unwrap(), "--prompt=Use pnpm.\n\n-rf everything");
        assert_eq!(a.iter().filter(|x| x.starts_with("--prompt")).count(), 1);

        let a = spec_args("gemini", "plan", Some("9b2f"), "go on");
        assert!(a.windows(2).any(|w| w == ["--resume", "9b2f"]));
        assert!(a.windows(2).any(|w| w == ["--approval-mode", "plan"]));
        assert_eq!(a.last().unwrap(), "--prompt=go on", "instructions only lead a chat's first message");
        for (mode, want) in [("bypassPermissions", "yolo"), ("default", "default"), ("ask", "auto_edit")] {
            assert!(spec_args("gemini", mode, None, "x").windows(2).any(|w| w == ["--approval-mode", want]), "{mode}");
        }
        // A session id that looks like a flag is dropped, as for the others.
        assert!(!spec_args("gemini", "plan", Some("--yolo"), "x").contains(&"--yolo".to_string()));
        let s = TurnSpec { fork: false, agent: "gemini", prompt: "x", agent_session_id: None, model: None, permission_mode: "ask", cwd: Path::new("/tmp"), effort: None, instructions: None, env: &[], dirs: &[], project_config: true, guard: None };
        assert!(!interactive(&s), "no stdin channel for Gemini");
        assert_eq!(command(&s).get_program(), "gemini");
    }

    #[test]
    fn claude_and_codex_commands_are_unchanged() {
        // The exact argv each built before the harness, for the same turn.
        let a = spec_args("claude", "acceptEdits", Some("s1"), "hi");
        assert_eq!(a, ["-p", "hi", "--output-format", "stream-json", "--verbose", "--permission-mode", "acceptEdits", "--model", "flash", "--resume", "s1", "--fork-session", "--effort", "high", "--append-system-prompt", "Use pnpm.", "--add-dir", "/code/shared"]);
        let a = spec_args("codex", "acceptEdits", None, "hi");
        assert_eq!(a, ["exec", "--json", "--skip-git-repo-check", "-c", "sandbox_mode=\"workspace-write\"", "-m", "flash", "--add-dir", "/code/shared", "-c", "model_reasoning_effort=\"high\"", "Use pnpm.\n\nhi"]);
    }

    #[test]
    fn stop_detection() {
        use std::os::unix::process::ExitStatusExt;
        let exit = |code: i32| ExitStatus::from_raw(code << 8);
        let signal = ExitStatus::from_raw(2);
        for a in [get("claude"), get("codex")] {
            assert!(a.stopped_by_user(Some(&signal), ""));
            assert!(a.stopped_by_user(None, ""));
            assert!(!a.stopped_by_user(Some(&exit(1)), "boom"));
            assert!(!a.stopped_by_user(Some(&exit(0)), ""));
            assert!(!a.stopped_by_user(Some(&exit(130)), "noise"), "unchanged: stderr means an error");
        }
        let g = get("gemini");
        assert!(g.stopped_by_user(Some(&exit(130)), "YOLO mode is enabled."));
        assert!(!g.stopped_by_user(Some(&exit(41)), "Please set an Auth method"));
    }
}
