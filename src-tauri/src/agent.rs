//! Runs one agent turn (Claude Code or Codex CLI) and turns its JSON stream into
//! a small, agent-neutral event list the UI can render.
//!
//! Event kinds: `user`, `text`, `thinking`, `tool`, `tool_result`, `session`, `done`, `error`.

use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

const MAX_OUTPUT: usize = 4000;

fn clip(s: &str) -> String {
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
fn summarize_input(name: &str, input: &Value) -> String {
    let pick = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    let s = match name {
        "Bash" => pick("command"),
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => pick("file_path"),
        "Grep" | "Glob" => pick("pattern"),
        "WebFetch" => pick("url"),
        "WebSearch" => pick("query"),
        "Task" | "Agent" => pick("description"),
        _ => None,
    };
    s.unwrap_or_else(|| clip(&input.to_string()))
}

/// Claude Code `--output-format stream-json` line -> zero or more events.
pub fn parse_claude(line: &str) -> Vec<Value> {
    let Ok(j) = serde_json::from_str::<Value>(line) else { return vec![] };
    let mut out = vec![];
    match j["type"].as_str() {
        Some("system") if j["subtype"] == "init" => {
            if let Some(id) = j["session_id"].as_str() {
                out.push(json!({"kind": "session", "agent_session_id": id}));
            }
        }
        Some("assistant") => {
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
        Some("result") => out.push(json!({
            "kind": "done",
            "is_error": j["is_error"].as_bool().unwrap_or(false),
            "cost_usd": j["total_cost_usd"],
            "duration_ms": j["duration_ms"],
        })),
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

pub struct TurnSpec<'a> {
    pub agent: &'a str,
    pub prompt: &'a str,
    pub agent_session_id: Option<&'a str>,
    pub model: Option<&'a str>,
    /// `plan`, `default`, `acceptEdits` or `bypassPermissions` (Claude Code names).
    pub permission_mode: &'a str,
    pub cwd: &'a Path,
}

/// Build the CLI command for one turn.
pub fn command(spec: &TurnSpec) -> Command {
    let mut c;
    if spec.agent == "codex" {
        let sandbox = match spec.permission_mode {
            "plan" => "read-only",
            "bypassPermissions" => "danger-full-access",
            _ => "workspace-write",
        };
        c = Command::new("codex");
        c.arg("exec");
        if let Some(id) = spec.agent_session_id {
            c.args(["resume", id]);
        }
        c.args(["--json", "--skip-git-repo-check", "-c", &format!("sandbox_mode=\"{sandbox}\"")]);
        if let Some(m) = spec.model {
            c.args(["-m", m]);
        }
        c.arg(spec.prompt);
    } else {
        c = Command::new("claude");
        c.args(["-p", spec.prompt, "--output-format", "stream-json", "--verbose"]);
        c.args(["--permission-mode", spec.permission_mode]);
        if let Some(m) = spec.model {
            c.args(["--model", m]);
        }
        if let Some(id) = spec.agent_session_id {
            c.args(["--resume", id]);
        }
    }
    c.current_dir(spec.cwd);
    c.stdin(std::process::Stdio::null());
    c
}

pub fn parse(agent: &str, line: &str) -> Vec<Value> {
    if agent == "codex" { parse_codex(line) } else { parse_claude(line) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: &str = include_str!("../tests/fixtures/claude.jsonl");
    const CODEX: &str = include_str!("../tests/fixtures/codex.jsonl");

    fn kinds(evs: &[Value]) -> Vec<&str> {
        evs.iter().map(|e| e["kind"].as_str().unwrap()).collect()
    }

    #[test]
    fn claude_stream() {
        let evs: Vec<Value> = CLAUDE.lines().flat_map(parse_claude).collect();
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
    fn commands() {
        let cwd = Path::new("/tmp");
        let c = command(&TurnSpec { agent: "claude", prompt: "hi", agent_session_id: Some("s1"), model: Some("opus"), permission_mode: "plan", cwd });
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.windows(2).any(|w| w == ["--resume", "s1"]));
        assert!(args.windows(2).any(|w| w == ["--permission-mode", "plan"]));

        let c = command(&TurnSpec { agent: "codex", prompt: "hi", agent_session_id: Some("t1"), model: None, permission_mode: "plan", cwd });
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert_eq!(&args[..3], ["exec", "resume", "t1"]);
        assert!(args.contains(&"sandbox_mode=\"read-only\"".to_string()));
        assert_eq!(args.last().unwrap(), "hi");
    }
}
