//! Brings in agent sessions you ran straight in a terminal. Claude Code, Codex and Gemini CLI each
//! keep every session on disk; this finds them, groups them by the repo and folder they ran in, and
//! copies the ones you pick in as chats.
//!
//! Read-only: the CLIs' folders are only ever opened for reading, a line at a time, so a 100 MB
//! session never sits in memory whole. A session that ran in a linked worktree lands in a workspace
//! pointing at that folder; one that ran in the main checkout lands in a read-only "History"
//! workspace for the repo. No worktree is ever created. Chats keep the CLI's session id, so sending
//! a message continues the same CLI session.
//!
//! Formats (checked against the files on disk and the CLIs' own source, 2026-11):
//! - Claude Code: `<config>/projects/<cwd with every non-alphanumeric as ->/<session id>.jsonl`, one
//!   record per line (`user`, `assistant`, titles, and many bookkeeping types that are skipped).
//! - Codex: `<CODEX_HOME>/sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl`. The first line is
//!   `session_meta` (id, cwd). Newer CLIs log each finished step as `event_msg/item_completed`;
//!   older ones log `user_message` / `agent_message` events and `response_item` tool calls.
//! - Gemini CLI: `~/.gemini/tmp/<project>/chats/session-<time>-<id8>.jsonl` (older: `.json`, one
//!   object), with the folder in `<project>/.project_root`. JSONL records are message upserts by id
//!   plus `$set`, `$patch` and `$rewindTo` records, replayed here the way the CLI replays them.

use crate::store::{new_id, Repo, Session, Store, Workspace};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const CLAUDE: &str = "claude";
pub const CODEX: &str = "codex";
pub const GEMINI: &str = "gemini";

/// `imported_from` of a workspace made for a linked worktree.
pub const FROM_TERMINAL: &str = "terminal";
/// `imported_from` of a repo's read-only History workspace (it points at the main checkout).
pub const FROM_HISTORY: &str = "terminal-history";

/// Where each CLI keeps its sessions. Several roots per CLI: the env override and the default can
/// both hold sessions (a CLAUDE_CONFIG_DIR set in one shell only); the same folder is read once.
#[derive(Debug, Clone, Default)]
pub struct Roots {
    pub claude: Vec<PathBuf>,
    pub codex: Vec<PathBuf>,
    pub gemini: Vec<PathBuf>,
}

impl Roots {
    pub fn from_env() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let both = |over: Option<PathBuf>, default: PathBuf, sub: &str| -> Vec<PathBuf> {
            let mut v: Vec<PathBuf> = over.into_iter().chain([default]).map(|d| d.join(sub)).collect();
            dedupe_dirs(&mut v);
            v
        };
        Roots {
            claude: both(env("CLAUDE_CONFIG_DIR"), home.join(".claude"), "projects"),
            codex: both(env("CODEX_HOME"), home.join(".codex"), "sessions"),
            gemini: both(env("GEMINI_CLI_HOME").map(|h| h.join(".gemini")), home.join(".gemini"), "tmp"),
        }
    }
}

/// Keep folders that exist, once each (a symlinked folder counts as its target).
fn dedupe_dirs(v: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    v.retain(|d| d.is_dir() && seen.insert(d.canonicalize().unwrap_or_else(|_| d.clone())));
}

/// One session file found on disk.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Found {
    pub agent: &'static str,
    /// The CLI's own session id: what `--resume` takes.
    pub id: String,
    /// The folder the session started in.
    pub cwd: String,
    #[serde(skip)]
    pub file: PathBuf,
    pub title: String,
    /// Your prompts plus the agent's replies (tool calls and their output are not counted).
    pub messages: u64,
    pub first: String,
    pub last: String,
    pub bytes: u64,
    /// Codex only: the file logs `item_completed` steps (newer CLIs), so the older events are skipped.
    #[serde(skip)]
    pub codex_items: bool,
}

// ---------- reading files a line at a time ----------

/// Calls `f` with every non-empty line. A line cut short (a session still being written, a crash)
/// is passed on too; it just fails to parse and is skipped.
fn for_lines(path: &Path, mut f: impl FnMut(&[u8])) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let mut r = BufReader::with_capacity(256 * 1024, file);
    let mut buf = Vec::with_capacity(64 * 1024);
    loop {
        buf.clear();
        if r.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        let line = buf.trim_ascii();
        if !line.is_empty() {
            f(line);
        }
    }
}

/// ISO timestamps as the store writes them, so date filters compare like with like.
fn norm_ts(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|d| d.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|_| ts.to_string())
}

fn title_from(prompt: &str) -> String {
    let t = crate::title::task_title(prompt);
    if !t.is_empty() {
        return t;
    }
    prompt.lines().next().unwrap_or("").chars().take(crate::title::MAX).collect()
}

// ---------- Claude Code ----------

#[derive(Deserialize)]
struct ClaudeHead<'a> {
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    cwd: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(rename = "isSidechain", default)]
    sidechain: Option<bool>,
    #[serde(rename = "isMeta", default)]
    meta: Option<bool>,
    #[serde(rename = "isCompactSummary", default)]
    compact: Option<bool>,
    #[serde(borrow, default)]
    message: Option<MessageHead<'a>>,
    #[serde(rename = "aiTitle", default)]
    ai_title: Option<String>,
    #[serde(rename = "customTitle", default)]
    custom_title: Option<String>,
}

#[derive(Deserialize)]
struct MessageHead<'a> {
    #[serde(borrow, default)]
    content: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct PartHead<'a> {
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    text: Option<Cow<'a, str>>,
}

/// Text Claude Code put in your turn that you never typed: command output, reminders, notices.
const INJECTED: &[&str] = &[
    "<local-command-stdout>", "<local-command-stderr>", "<local-command-caveat>", "<bash-stdout>", "<bash-stderr>",
    "<task-notification>", "<system-reminder>", "Caveat: The messages below were generated", "[Request interrupted",
    "<environment_context>", "<user_instructions>", "<turn_aborted>",
];

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let a = s.find(open)? + open.len();
    let b = s[a..].find(close)? + a;
    Some(&s[a..b])
}

/// What you typed, as the chat shows it: slash commands as `/name args`, shell escapes as `! cmd`.
/// None for text the CLI injected.
pub fn typed_text(raw: &str) -> Option<String> {
    let mut t = raw.trim();
    // Apps that drive the CLI (Conductor, scripts) put their instructions ahead of your message.
    if t.starts_with("<system_instruction>")
        && let Some(end) = t.find("</system_instruction>") {
            t = t[end + "</system_instruction>".len()..].trim();
        }
    if t.is_empty() || INJECTED.iter().any(|p| t.starts_with(p)) {
        return None;
    }
    if let Some(name) = between(t, "<command-name>", "</command-name>") {
        let args = between(t, "<command-args>", "</command-args>").unwrap_or("").trim();
        let name = name.trim();
        let name = if name.starts_with('/') { name.to_string() } else { format!("/{name}") };
        return Some(if args.is_empty() { name } else { format!("{name} {args}") });
    }
    if let Some(cmd) = between(t, "<bash-input>", "</bash-input>") {
        return Some(format!("! {}", cmd.trim()));
    }
    Some(t.to_string())
}

/// The prompt in a Claude `user` record, or None when it only carries tool results or injected text.
fn claude_prompt(content: &RawValue) -> Option<String> {
    let s = content.get().trim_start();
    if s.starts_with('"') {
        let text: String = serde_json::from_str(s).ok()?;
        return typed_text(&text);
    }
    let parts: Vec<PartHead> = serde_json::from_str(s).ok()?;
    let mut out = vec![];
    for p in &parts {
        match p.kind.as_deref() {
            Some("tool_result") => return None,
            Some("text") => out.extend(p.text.as_deref().and_then(typed_text)),
            Some("image") => out.push("[image]".to_string()),
            _ => {}
        }
    }
    let text = out.join("\n");
    (!text.trim().is_empty() && text != "[image]" || out.len() > 1).then_some(text)
}

fn has_text(content: &RawValue) -> bool {
    serde_json::from_str::<Vec<PartHead>>(content.get())
        .map(|parts| parts.iter().any(|p| p.kind.as_deref() == Some("text") && p.text.as_deref().is_some_and(|t| !t.trim().is_empty())))
        .unwrap_or(false)
}

/// A transcript record that is part of the conversation you saw (not a subagent, not injected).
fn claude_visible(h: &ClaudeHead) -> bool {
    !h.sidechain.unwrap_or(false) && !h.meta.unwrap_or(false) && !h.compact.unwrap_or(false)
}

pub fn scan_claude(path: &Path) -> Option<Found> {
    let id = path.file_stem()?.to_string_lossy().to_string();
    let bytes = std::fs::metadata(path).ok()?.len();
    let (mut cwd, mut first, mut last, mut messages) = (None::<String>, String::new(), String::new(), 0u64);
    let (mut ai, mut custom, mut prompt) = (None, None, None::<String>);
    for_lines(path, |line| {
        let Ok(h) = serde_json::from_slice::<ClaudeHead>(line) else { return };
        if cwd.is_none()
            && let Some(c) = h.cwd.as_deref().filter(|c| !c.is_empty()) {
                cwd = Some(c.to_string());
            }
        if let Some(t) = h.custom_title.clone() {
            custom = Some(t);
        }
        if let Some(t) = h.ai_title.clone() {
            ai = Some(t);
        }
        let counted = match (h.kind.as_deref(), h.message.as_ref().and_then(|m| m.content)) {
            (Some("user"), Some(c)) if claude_visible(&h) => match claude_prompt(c) {
                Some(p) => {
                    prompt.get_or_insert(p);
                    true
                }
                None => false,
            },
            (Some("assistant"), Some(c)) if claude_visible(&h) => has_text(c),
            _ => false,
        };
        if counted {
            messages += 1;
        }
        if let Some(ts) = h.timestamp.as_deref() {
            if first.is_empty() {
                first = ts.to_string();
            }
            last.clear();
            last.push_str(ts);
        }
    })
    .ok()?;
    let title = custom.or(ai).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| prompt.as_deref().map(title_from).unwrap_or_default());
    Some(Found { agent: CLAUDE, id, cwd: cwd?, file: path.to_path_buf(), title, messages, first: norm_ts(&first), last: norm_ts(&last), bytes, codex_items: false })
}

// ---------- Codex ----------

#[derive(Deserialize)]
struct CodexHead<'a> {
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    payload: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct CodexEventHead<'a> {
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    message: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    item: Option<CodexItemHead<'a>>,
}

#[derive(Deserialize)]
struct CodexItemHead<'a> {
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    content: Option<&'a RawValue>,
}

/// The text parts of a Codex message (`[{type: text|Text|input_text|output_text, text}]`).
fn codex_text(content: Option<&RawValue>) -> String {
    let Some(c) = content else { return String::new() };
    serde_json::from_str::<Vec<PartHead>>(c.get())
        .map(|parts| parts.iter().filter_map(|p| p.text.as_deref()).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

fn codex_text_value(content: &Value) -> String {
    content.as_array().into_iter().flatten().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n")
}

/// Thread names Codex keeps beside its sessions (`session_index.jsonl`), by thread id.
pub fn codex_names(sessions_root: &Path) -> HashMap<String, String> {
    let mut names = HashMap::new();
    if let Some(index) = sessions_root.parent().map(|d| d.join("session_index.jsonl")) {
        let _ = for_lines(&index, |line| {
            if let Ok(v) = serde_json::from_slice::<Value>(line)
                && let (Some(id), Some(name)) = (v["id"].as_str(), v["thread_name"].as_str())
                && !name.trim().is_empty() {
                    names.insert(id.to_string(), name.to_string());
                }
        });
    }
    names
}

pub fn scan_codex(path: &Path, names: &HashMap<String, String>) -> Option<Found> {
    let bytes = std::fs::metadata(path).ok()?.len();
    let (mut id, mut cwd, mut subagent) = (None::<String>, None::<String>, false);
    let (mut first, mut last, mut items, mut legacy) = (String::new(), String::new(), 0u64, 0u64);
    let (mut item_prompt, mut legacy_prompt) = (None::<String>, None::<String>);
    let mut has_items = false;
    for_lines(path, |line| {
        let Ok(h) = serde_json::from_slice::<CodexHead>(line) else { return };
        if let Some(ts) = h.timestamp.as_deref() {
            if first.is_empty() {
                first = ts.to_string();
            }
            last.clear();
            last.push_str(ts);
        }
        let Some(p) = h.payload else { return };
        match h.kind.as_deref() {
            // A fork logs its parent's meta after its own; the first one is this session.
            Some("session_meta") if id.is_none() => {
                if let Ok(m) = serde_json::from_str::<Value>(p.get()) {
                    id = m["id"].as_str().map(str::to_string);
                    cwd = m["cwd"].as_str().map(str::to_string);
                    subagent = m["source"].get("subagent").is_some();
                    if let Some(ts) = m["timestamp"].as_str() {
                        first = ts.to_string();
                    }
                }
            }
            Some("event_msg") => {
                let Ok(e) = serde_json::from_str::<CodexEventHead>(p.get()) else { return };
                match e.kind.as_deref() {
                    Some("item_completed") => {
                        has_items = true;
                        let Some(item) = e.item else { return };
                        match item.kind.as_deref() {
                            Some("UserMessage") => {
                                if let Some(t) = typed_text(&codex_text(item.content)) {
                                    items += 1;
                                    item_prompt.get_or_insert(t);
                                }
                            }
                            Some("AgentMessage") => items += 1,
                            _ => {}
                        }
                    }
                    Some("user_message") => {
                        if let Some(t) = e.message.as_deref().and_then(typed_text) {
                            legacy += 1;
                            legacy_prompt.get_or_insert(t);
                        }
                    }
                    Some("agent_message") => legacy += 1,
                    _ => {}
                }
            }
            _ => {}
        }
    })
    .ok()?;
    if subagent {
        return None;
    }
    let id = id?;
    let prompt = if has_items { item_prompt } else { legacy_prompt };
    let title = names.get(&id).cloned().unwrap_or_else(|| prompt.as_deref().map(title_from).unwrap_or_default());
    Some(Found {
        agent: CODEX, id, cwd: cwd?, file: path.to_path_buf(), title,
        messages: if has_items { items } else { legacy },
        first: norm_ts(&first), last: norm_ts(&last), bytes, codex_items: has_items,
    })
}

// ---------- Gemini CLI ----------

/// One Gemini session, replayed from its records.
pub struct GeminiSession {
    pub id: String,
    pub start: String,
    pub updated: String,
    pub summary: String,
    pub messages: Vec<Value>,
}

/// Replays a Gemini session file the way the CLI loads it: messages are upserted by id, `$rewindTo`
/// drops a message and everything after it, `$patch` updates content and tool results, `$set`
/// updates the metadata. An older `.json` file is one object with a `messages` list.
pub fn gemini_load(path: &Path) -> Option<GeminiSession> {
    let mut meta = serde_json::Map::new();
    let mut order: Vec<String> = vec![];
    let mut msgs: HashMap<String, Value> = HashMap::new();
    let upsert = |m: Value, order: &mut Vec<String>, msgs: &mut HashMap<String, Value>| {
        let Some(id) = m["id"].as_str().map(str::to_string) else { return };
        if msgs.insert(id.clone(), m).is_none() {
            order.push(id);
        }
    };
    if path.extension().is_some_and(|e| e == "json") {
        let v: Value = serde_json::from_reader(BufReader::new(std::fs::File::open(path).ok()?)).ok()?;
        for m in v["messages"].as_array().cloned().unwrap_or_default() {
            upsert(m, &mut order, &mut msgs);
        }
        meta = v.as_object().cloned().unwrap_or_default();
    } else {
        for_lines(path, |line| {
            let Ok(mut r) = serde_json::from_slice::<Value>(line) else { return };
            if let Some(to) = r["$rewindTo"].as_str() {
                match order.iter().position(|id| id == to) {
                    Some(i) => order.drain(i..).for_each(|id| { msgs.remove(&id); }),
                    None => { order.clear(); msgs.clear(); }
                }
            } else if r["$patch"].is_object() {
                let patch = r["$patch"].take();
                let updates: Vec<Value> = std::iter::once(patch.clone()).chain(patch["updates"].as_array().cloned().unwrap_or_default()).collect();
                for u in updates {
                    let Some(m) = u["id"].as_str().and_then(|id| msgs.get_mut(id)) else { continue };
                    if !u["content"].is_null() {
                        m["content"] = u["content"].clone();
                    }
                    for tc in u["toolCalls"].as_array().into_iter().flatten() {
                        if let Some(t) = m["toolCalls"].as_array_mut().into_iter().flatten().find(|t| t["id"] == tc["id"]) {
                            t["result"] = tc["result"].clone();
                        }
                    }
                }
                for id in patch["removeIds"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                    if msgs.remove(id).is_some() {
                        order.retain(|o| o != id);
                    }
                }
            } else if let Some(set) = r["$set"].as_object() {
                if let Some(list) = set.get("messages").and_then(Value::as_array) {
                    order.clear();
                    msgs.clear();
                    for m in list.clone() {
                        upsert(m, &mut order, &mut msgs);
                    }
                }
                for (k, v) in set {
                    if k != "messages" {
                        meta.insert(k.clone(), v.clone());
                    }
                }
            } else if r["id"].is_string() {
                upsert(r, &mut order, &mut msgs);
            } else if r["sessionId"].is_string() && r["projectHash"].is_string() {
                for m in r["messages"].as_array().cloned().unwrap_or_default() {
                    upsert(m, &mut order, &mut msgs);
                }
                if let Some(o) = r.as_object() {
                    for (k, v) in o {
                        if k != "messages" {
                            meta.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
        })
        .ok()?;
    }
    let id = meta.get("sessionId")?.as_str()?.to_string();
    let s = |k: &str| meta.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Some(GeminiSession { id, start: s("startTime"), updated: s("lastUpdated"), summary: s("summary"), messages: order.into_iter().filter_map(|id| msgs.remove(&id)).collect() })
}

fn gemini_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(""),
        Value::Object(_) => content["text"].as_str().unwrap_or("").to_string(),
        _ => String::new(),
    }
}

/// The folder a Gemini project directory belongs to: its `.project_root` file, else the CLI's
/// `projects.json` registry (folder -> directory name).
fn gemini_folder(project_dir: &Path) -> Option<String> {
    if let Ok(s) = std::fs::read_to_string(project_dir.join(".project_root")) {
        let s = s.trim();
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    let name = project_dir.file_name()?.to_string_lossy().to_string();
    let reg: Value = serde_json::from_str(&std::fs::read_to_string(project_dir.parent()?.parent()?.join("projects.json")).ok()?).ok()?;
    reg["projects"].as_object()?.iter().find(|(_, v)| v.as_str() == Some(name.as_str())).map(|(k, _)| k.clone())
}

pub fn scan_gemini(path: &Path, cwd: &str) -> Option<Found> {
    let bytes = std::fs::metadata(path).ok()?.len();
    let s = gemini_load(path)?;
    let mut prompt = None;
    let mut messages = 0;
    for m in &s.messages {
        let text = gemini_text(&m["content"]);
        match m["type"].as_str() {
            Some("user") => {
                if let Some(t) = typed_text(&text) {
                    messages += 1;
                    prompt.get_or_insert(t);
                }
            }
            Some("gemini") if !text.trim().is_empty() => messages += 1,
            _ => {}
        }
    }
    if messages == 0 {
        return None;
    }
    let title = if s.summary.trim().is_empty() { prompt.as_deref().map(title_from).unwrap_or_default() } else { s.summary.clone() };
    let first = s.messages.first().and_then(|m| m["timestamp"].as_str()).unwrap_or(&s.start).to_string();
    let last = s.messages.last().and_then(|m| m["timestamp"].as_str()).unwrap_or(&s.updated).to_string();
    Some(Found { agent: GEMINI, id: s.id, cwd: cwd.to_string(), file: path.to_path_buf(), title, messages, first: norm_ts(&first), last: norm_ts(&last), bytes, codex_items: false })
}

// ---------- finding every session ----------

enum Job {
    Claude(PathBuf),
    Codex(PathBuf),
    Gemini(PathBuf, String),
}

fn walk_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk_jsonl(&p, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
}

fn jobs(roots: &Roots) -> (Vec<(Job, u64)>, HashMap<String, String>) {
    let mut jobs = vec![];
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    // Claude: only `<project>/<id>.jsonl`; subagent transcripts sit one folder deeper.
    for root in &roots.claude {
        for proj in std::fs::read_dir(root).into_iter().flatten().flatten() {
            for f in std::fs::read_dir(proj.path()).into_iter().flatten().flatten() {
                let p = f.path();
                if p.extension().is_some_and(|x| x == "jsonl") && f.file_type().is_ok_and(|t| t.is_file()) {
                    let n = size(&p);
                    jobs.push((Job::Claude(p), n));
                }
            }
        }
    }
    let mut names = HashMap::new();
    for root in &roots.codex {
        names.extend(codex_names(root));
        let mut files = vec![];
        walk_jsonl(root, &mut files);
        for p in files {
            let n = size(&p);
            jobs.push((Job::Codex(p), n));
        }
    }
    // Gemini: `<project>/chats/session-*.json(l)`; subagent chats sit in a folder below.
    for root in &roots.gemini {
        for proj in std::fs::read_dir(root).into_iter().flatten().flatten() {
            let Some(folder) = gemini_folder(&proj.path()) else { continue };
            for f in std::fs::read_dir(proj.path().join("chats")).into_iter().flatten().flatten() {
                let p = f.path();
                let name = f.file_name().to_string_lossy().to_string();
                if name.starts_with("session-") && (name.ends_with(".jsonl") || name.ends_with(".json")) {
                    let n = size(&p);
                    jobs.push((Job::Gemini(p, folder.clone()), n));
                }
            }
        }
    }
    // Biggest first, so one huge file doesn't finish last on its own.
    jobs.sort_by_key(|j| std::cmp::Reverse(j.1));
    (jobs, names)
}

/// Runs `f` over `items` on every core and keeps the results in order.
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(items.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out: Mutex<Vec<(usize, R)>> = Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(item) = items.get(i) else { break };
                let r = f(item);
                out.lock().unwrap().push((i, r));
            });
        }
    });
    let mut v = out.into_inner().unwrap();
    v.sort_by_key(|(i, _)| *i);
    v.into_iter().map(|(_, r)| r).collect()
}

/// Every session in the CLIs' folders, newest first. Sessions with nothing you or the agent said
/// (a CLI opened and closed) are left out.
pub fn find_all(roots: &Roots) -> Vec<Found> {
    let (jobs, names) = jobs(roots);
    let mut found: Vec<Found> = par_map(&jobs, |(job, _)| match job {
        Job::Claude(p) => scan_claude(p),
        Job::Codex(p) => scan_codex(p, &names),
        Job::Gemini(p, cwd) => scan_gemini(p, cwd),
    })
    .into_iter()
    .flatten()
    .filter(|f| f.messages > 0)
    .collect();
    // The same session reached through two roots is one session.
    let mut seen = HashSet::new();
    found.retain(|f| seen.insert((f.agent, f.id.clone())));
    found.sort_by(|a, b| b.last.cmp(&a.last));
    found
}

// ---------- where each session belongs ----------

/// The git folder a session ran in.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// The worktree (or main checkout) holding the session's folder.
    pub folder: String,
    /// The repo's main checkout.
    pub main: String,
    pub branch: String,
}

fn canon(p: &str) -> String {
    Path::new(p).canonicalize().map(|c| c.to_string_lossy().to_string()).unwrap_or_else(|_| p.to_string())
}

/// Read-only git lookups: which worktree and repo a folder belongs to.
pub fn place(cwd: &str) -> Result<Place, String> {
    let dir = Path::new(cwd);
    if !dir.is_dir() {
        return Err("The folder is gone.".into());
    }
    let out = crate::git::git(dir, &["rev-parse", "--path-format=absolute", "--show-toplevel", "--git-common-dir"])
        .map_err(|_| "Not in a git repo.".to_string())?;
    let mut lines = out.lines();
    let (Some(top), Some(common)) = (lines.next(), lines.next()) else { return Err("Not in a git repo.".into()) };
    let common = Path::new(common);
    let main = if common.file_name().is_some_and(|n| n == ".git") { common.parent().unwrap_or(common).to_string_lossy().to_string() } else { top.to_string() };
    let branch = crate::git::git(Path::new(top), &["branch", "--show-current"]).unwrap_or_default();
    Ok(Place { folder: canon(top), main: canon(&main), branch: if branch.is_empty() { "HEAD".into() } else { branch } })
}

/// The last scan, kept so Import works on exactly what the preview showed.
#[derive(Debug, Clone, Default)]
pub struct Scan {
    pub found: Vec<Found>,
    /// By session cwd: where it belongs, or why it can't be imported.
    pub places: HashMap<String, Result<Place, String>>,
    pub took_ms: u128,
}

pub fn scan(roots: &Roots) -> Scan {
    let t = std::time::Instant::now();
    let found = find_all(roots);
    let cwds: Vec<String> = found.iter().map(|f| f.cwd.clone()).collect::<HashSet<_>>().into_iter().collect();
    let places = cwds.iter().cloned().zip(par_map(&cwds, |c| place(c))).collect();
    Scan { found, places, took_ms: t.elapsed().as_millis() }
}

pub static LAST: Mutex<Option<Scan>> = Mutex::new(None);

// ---------- the preview ----------

#[derive(Debug, Serialize, PartialEq)]
pub struct Row {
    pub agent: &'static str,
    pub id: String,
    pub title: String,
    pub messages: u64,
    pub first: String,
    pub last: String,
    pub bytes: u64,
    /// Already in nekotrail (imported before, or a chat nekotrail ran itself).
    pub imported: bool,
    /// Sending a message continues this CLI session. False when it ran in a subfolder, since
    /// Claude and Gemini only find a session from the folder it started in.
    pub resumable: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FolderView {
    pub path: String,
    /// "history" (the main checkout), "worktree", or "workspace" (one nekotrail already has).
    pub kind: &'static str,
    pub branch: String,
    pub workspace_id: Option<String>,
    pub sessions: Vec<Row>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct RepoView {
    pub root: String,
    pub name: String,
    /// None: not added to nekotrail yet; importing adds it.
    pub repo_id: Option<String>,
    pub folders: Vec<FolderView>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SkippedView {
    pub folder: String,
    pub reason: String,
    pub sessions: usize,
    pub messages: u64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Preview {
    pub repos: Vec<RepoView>,
    pub skipped: Vec<SkippedView>,
    /// Sessions found per CLI.
    pub found: HashMap<&'static str, usize>,
    /// The folders that were read.
    pub sources: Vec<String>,
    pub took_ms: u128,
}

/// (agent, CLI session id) pairs nekotrail already has.
pub fn known_ids(store: &Store) -> Result<HashSet<(String, String)>> {
    store.terminal_known()
}

fn by_canon<T: Clone>(items: impl IntoIterator<Item = (String, T)>) -> HashMap<String, T> {
    items.into_iter().map(|(p, t)| (canon(&p), t)).collect()
}

/// The store's repos and workspaces by real folder path.
struct Have {
    repos: HashMap<String, Repo>,
    workspaces: HashMap<String, Workspace>,
}

fn have(store: &Store) -> Result<Have> {
    Ok(Have {
        repos: by_canon(store.repos()?.into_iter().map(|r| (r.root_path.clone(), r))),
        workspaces: by_canon(store.workspaces(true)?.into_iter().map(|w| (w.path.clone(), w))),
    })
}

/// Where a folder's chats go: a workspace nekotrail has at that path, else History (main checkout)
/// or a new workspace pointing at the worktree.
fn folder_kind(have: &Have, p: &Place) -> (&'static str, Option<String>) {
    match have.workspaces.get(&p.folder) {
        Some(w) if w.imported_from.as_deref() == Some(FROM_HISTORY) => ("history", Some(w.id.clone())),
        Some(w) => ("workspace", Some(w.id.clone())),
        None if p.folder == p.main => ("history", None),
        None => ("worktree", None),
    }
}

fn resumable(f: &Found, p: &Place) -> bool {
    f.agent == CODEX || canon(&f.cwd) == p.folder
}

pub fn preview(store: &Store, scan: &Scan, roots: &Roots) -> Result<Preview> {
    let known = known_ids(store)?;
    let have = have(store)?;
    let mut repos: Vec<RepoView> = vec![];
    let mut skipped: HashMap<String, SkippedView> = HashMap::new();
    let mut found = HashMap::new();
    for f in &scan.found {
        *found.entry(f.agent).or_insert(0) += 1;
        let row = |p: &Place| Row {
            agent: f.agent, id: f.id.clone(), title: f.title.clone(), messages: f.messages, first: f.first.clone(), last: f.last.clone(),
            bytes: f.bytes, imported: known.contains(&(f.agent.to_string(), f.id.clone())), resumable: resumable(f, p),
        };
        match scan.places.get(&f.cwd) {
            Some(Ok(p)) => {
                let ri = match repos.iter().position(|r| r.root == p.main) {
                    Some(i) => i,
                    None => {
                        let existing = have.repos.get(&p.main);
                        repos.push(RepoView {
                            root: p.main.clone(),
                            name: existing.map(|r| r.name.clone()).unwrap_or_else(|| Path::new(&p.main).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                            repo_id: existing.map(|r| r.id.clone()),
                            folders: vec![],
                        });
                        repos.len() - 1
                    }
                };
                let folders = &mut repos[ri].folders;
                let fi = match folders.iter().position(|x| x.path == p.folder) {
                    Some(i) => i,
                    None => {
                        let (kind, workspace_id) = folder_kind(&have, p);
                        folders.push(FolderView { path: p.folder.clone(), kind, branch: p.branch.clone(), workspace_id, sessions: vec![] });
                        folders.len() - 1
                    }
                };
                folders[fi].sessions.push(row(p));
            }
            other => {
                let reason = match other { Some(Err(r)) => r.clone(), _ => "Not in a git repo.".into() };
                let s = skipped.entry(f.cwd.clone()).or_insert(SkippedView { folder: f.cwd.clone(), reason, sessions: 0, messages: 0 });
                s.sessions += 1;
                s.messages += f.messages;
            }
        }
    }
    // Repos you already have first, then the busiest.
    repos.sort_by(|a, b| b.repo_id.is_some().cmp(&a.repo_id.is_some()).then_with(|| total(b).cmp(&total(a))));
    for r in &mut repos {
        r.folders.sort_by(|a, b| (a.kind != "history").cmp(&(b.kind != "history")).then_with(|| b.sessions.len().cmp(&a.sessions.len())));
    }
    let mut skipped: Vec<SkippedView> = skipped.into_values().collect();
    skipped.sort_by(|a, b| b.sessions.cmp(&a.sessions).then_with(|| a.folder.cmp(&b.folder)));
    let sources = roots.claude.iter().chain(&roots.codex).chain(&roots.gemini).map(|p| p.to_string_lossy().to_string()).collect();
    Ok(Preview { repos, skipped, found, sources, took_ms: scan.took_ms })
}

fn total(r: &RepoView) -> usize {
    r.folders.iter().map(|f| f.sessions.len()).sum()
}

// ---------- turning a session into chat events ----------

/// Collects events with their times, and closes each turn with a `done` the way a live turn ends,
/// so the chat groups replies under the prompt that asked for them.
#[derive(Default)]
struct Turns {
    out: Vec<(Value, String)>,
    open: bool,
    stopped: bool,
    at: String,
}

impl Turns {
    fn at(&mut self, ts: Option<&str>) {
        if let Some(ts) = ts.filter(|t| !t.is_empty()) {
            self.at = norm_ts(ts);
        }
    }
    fn close(&mut self) {
        if self.open {
            let done = if self.stopped { json!({"kind": "done", "is_error": true, "stopped": true}) } else { json!({"kind": "done", "is_error": false}) };
            self.out.push((done, self.at.clone()));
        }
        self.open = false;
        self.stopped = false;
    }
    fn user(&mut self, text: String) {
        self.close();
        self.out.push((json!({"kind": "user", "text": text}), self.at.clone()));
    }
    fn push(&mut self, ev: Value) {
        if ev["kind"] == "session" || ev["kind"] == "usage" || ev["kind"] == "done" {
            return;
        }
        self.open = true;
        self.out.push((ev, self.at.clone()));
    }
    fn finish(mut self) -> Vec<(Value, String)> {
        self.close();
        for (ev, _) in self.out.iter_mut() {
            // Kept out of spend totals and the agent scorecard, like Conductor's history.
            ev["imported"] = json!(true);
        }
        self.out
    }
}

pub fn claude_events(path: &Path) -> Result<Vec<(Value, String)>> {
    let mut t = Turns::default();
    for_lines(path, |line| {
        let Ok(h) = serde_json::from_slice::<ClaudeHead>(line) else { return };
        let kind = h.kind.as_deref();
        if !matches!(kind, Some("user" | "assistant")) || h.sidechain.unwrap_or(false) {
            return;
        }
        t.at(h.timestamp.as_deref());
        let prompt = match (kind, claude_visible(&h), h.message.as_ref().and_then(|m| m.content)) {
            (Some("user"), true, Some(c)) => claude_prompt(c),
            _ => None,
        };
        if let Some(p) = prompt {
            t.user(p);
            return;
        }
        if h.meta.unwrap_or(false) || h.compact.unwrap_or(false) {
            return;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line) else { return };
        for ev in crate::agent::parse_claude_value(&v) {
            t.push(ev);
        }
    })?;
    Ok(t.finish())
}

/// A shell command as one line: `["/bin/zsh", "-lc", "ls"]` is `ls`.
fn shell_line(cmd: &Value) -> String {
    match cmd {
        Value::String(s) => s.clone(),
        Value::Array(a) => {
            let parts: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
            match parts.as_slice() {
                [_, flag, script] if flag.starts_with('-') && flag.ends_with('c') => script.to_string(),
                _ => parts.join(" "),
            }
        }
        _ => String::new(),
    }
}

/// Files an `apply_patch` touches.
fn patch_files(patch: &str) -> String {
    patch
        .lines()
        .filter_map(|l| ["*** Update File: ", "*** Add File: ", "*** Delete File: "].iter().find_map(|p| l.strip_prefix(p)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn codex_output(output: &Value) -> (String, bool) {
    match output {
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(j) if j["output"].is_string() => (j["output"].as_str().unwrap_or("").to_string(), j["metadata"]["exit_code"].as_i64().unwrap_or(0) != 0),
            _ => (s.clone(), false),
        },
        Value::Array(_) => (codex_text_value(output), false),
        Value::Object(_) => (output["content"].as_str().map(str::to_string).unwrap_or_else(|| codex_text_value(&output["content"])), output["success"] == false),
        _ => (String::new(), false),
    }
}

fn tool(id: &Value, name: &str, input: String) -> Value {
    json!({"kind": "tool", "id": id, "name": name, "input": input})
}

fn result(id: &Value, output: &str, is_error: bool) -> Value {
    json!({"kind": "tool_result", "id": id, "output": crate::agent::clip(output), "is_error": is_error})
}

/// One `item_completed` step (newer Codex CLIs).
fn codex_item(t: &mut Turns, item: &Value) {
    let id = &item["id"];
    match item["type"].as_str() {
        Some("UserMessage") => {
            if let Some(p) = typed_text(&codex_text_value(&item["content"])) {
                t.user(p);
            }
        }
        Some("AgentMessage") => {
            let text = codex_text_value(&item["content"]);
            if !text.trim().is_empty() {
                t.push(json!({"kind": "text", "text": text}));
            }
        }
        Some("Reasoning") => {
            let text = item["summary_text"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join("\n");
            if !text.trim().is_empty() {
                t.push(json!({"kind": "thinking", "text": text}));
            }
        }
        Some("CommandExecution") => {
            t.push(tool(id, "Bash", shell_line(&item["command"])));
            let out = item["aggregated_output"].as_str().or(item["stdout"].as_str()).unwrap_or("");
            t.push(result(id, out, item["exit_code"].as_i64().unwrap_or(0) != 0 || item["status"] == "failed"));
        }
        Some("FileChange") => {
            let files: Vec<String> = match &item["changes"] {
                Value::Object(m) => m.keys().cloned().collect(),
                Value::Array(a) => a.iter().filter_map(|c| c["path"].as_str().map(str::to_string)).collect(),
                _ => vec![],
            };
            t.push(tool(id, "Edit", files.join(", ")));
            t.push(result(id, "", item["status"] == "failed"));
        }
        Some("WebSearch") => {
            t.push(tool(id, "WebSearch", item["query"].as_str().unwrap_or("").to_string()));
            t.push(result(id, "", false));
        }
        Some("ImageView") => {
            t.push(tool(id, "Read", item["path"].as_str().unwrap_or("").trim_start_matches("file://").to_string()));
            t.push(result(id, "", false));
        }
        Some("McpToolCall") => {
            let name = format!("mcp__{}__{}", item["server"].as_str().unwrap_or("mcp"), item["tool"].as_str().unwrap_or("tool"));
            t.push(tool(id, &name, crate::agent::summarize_input(&name, &item["arguments"])));
            let out = match &item["result"]["content"] {
                Value::Array(_) => codex_text_value(&item["result"]["content"]),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            t.push(result(id, &out, item["status"] == "failed"));
        }
        _ => {}
    }
}

pub fn codex_events(path: &Path, items: bool) -> Result<Vec<(Value, String)>> {
    let mut t = Turns::default();
    for_lines(path, |line| {
        let Ok(v) = serde_json::from_slice::<Value>(line) else { return };
        t.at(v["timestamp"].as_str());
        let p = &v["payload"];
        match (v["type"].as_str(), p["type"].as_str()) {
            (Some("event_msg"), Some("item_completed")) if items => codex_item(&mut t, &p["item"]),
            (Some("event_msg"), Some("turn_aborted")) => t.stopped = true,
            (Some("event_msg"), Some("user_message")) if !items => {
                if let Some(text) = p["message"].as_str().and_then(typed_text) {
                    t.user(text);
                }
            }
            (Some("event_msg"), Some("agent_message")) if !items => t.push(json!({"kind": "text", "text": p["message"]})),
            (Some("event_msg"), Some("agent_reasoning")) if !items => t.push(json!({"kind": "thinking", "text": p["text"]})),
            (Some("response_item"), Some("function_call")) if !items => {
                let name = p["name"].as_str().unwrap_or("tool");
                let args: Value = p["arguments"].as_str().and_then(|a| serde_json::from_str(a).ok()).unwrap_or(Value::Null);
                if matches!(name, "shell" | "exec_command" | "container.exec" | "local_shell") {
                    let cmd = if args["command"].is_null() { &args["cmd"] } else { &args["command"] };
                    t.push(tool(&p["call_id"], "Bash", shell_line(cmd)));
                } else {
                    t.push(tool(&p["call_id"], name, crate::agent::summarize_input(name, &args)));
                }
            }
            (Some("response_item"), Some("local_shell_call")) if !items => {
                t.push(tool(&p["call_id"], "Bash", shell_line(&p["action"]["command"])));
            }
            (Some("response_item"), Some("custom_tool_call")) if !items => {
                let name = p["name"].as_str().unwrap_or("tool");
                let input = p["input"].as_str().unwrap_or("");
                if name == "apply_patch" {
                    t.push(tool(&p["call_id"], "Edit", patch_files(input)));
                } else {
                    t.push(tool(&p["call_id"], name, crate::agent::clip(input)));
                }
            }
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) if !items => {
                let (out, failed) = codex_output(&p["output"]);
                t.push(result(&p["call_id"], &out, failed));
            }
            _ => {}
        }
    })?;
    Ok(t.finish())
}

pub fn gemini_events(path: &Path) -> Result<Vec<(Value, String)>> {
    let s = gemini_load(path).ok_or_else(|| anyhow::anyhow!("unreadable Gemini session"))?;
    let mut t = Turns::default();
    for m in &s.messages {
        t.at(m["timestamp"].as_str());
        match m["type"].as_str() {
            Some("user") => {
                if let Some(p) = typed_text(&gemini_text(&m["content"])) {
                    t.user(p);
                }
            }
            Some("gemini") => {
                for th in m["thoughts"].as_array().into_iter().flatten() {
                    let text = [th["subject"].as_str(), th["description"].as_str()].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(": ");
                    if !text.is_empty() {
                        t.push(json!({"kind": "thinking", "text": text}));
                    }
                }
                let text = gemini_text(&m["content"]);
                if !text.trim().is_empty() {
                    t.push(json!({"kind": "text", "text": text}));
                }
                for tc in m["toolCalls"].as_array().into_iter().flatten() {
                    let (name, input) = crate::agent::gemini_tool(tc["name"].as_str().unwrap_or("tool"), &tc["args"]);
                    t.push(tool(&tc["id"], &name, input));
                    let out = match &tc["resultDisplay"] {
                        Value::String(s) => s.clone(),
                        _ => tc["result"].as_array().into_iter().flatten()
                            .filter_map(|r| { let resp = &r["functionResponse"]["response"]; resp["output"].as_str().or(resp["error"].as_str()) })
                            .collect::<Vec<_>>().join("\n"),
                    };
                    t.push(result(&tc["id"], &out, matches!(tc["status"].as_str(), Some("error" | "cancelled"))));
                }
            }
            Some("error") => {
                let text = gemini_text(&m["content"]);
                if !text.trim().is_empty() {
                    t.push(json!({"kind": "error", "text": text}));
                }
            }
            _ => {}
        }
    }
    Ok(t.finish())
}

pub fn events(f: &Found) -> Result<Vec<(Value, String)>> {
    match f.agent {
        CLAUDE => claude_events(&f.file),
        CODEX => codex_events(&f.file, f.codex_items),
        _ => gemini_events(&f.file),
    }
}

// ---------- import ----------

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Pick {
    pub agent: String,
    pub id: String,
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Report {
    pub repos: usize,
    pub workspaces: usize,
    pub chats: usize,
    pub messages: usize,
    /// Picked sessions left out: already here, or their folder can't be used.
    pub skipped: usize,
    pub problems: Vec<String>,
}

fn new_repo(main: &str) -> Result<Repo> {
    let root = Path::new(main);
    Ok(Repo {
        id: new_id(),
        name: root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".into()),
        // A repo with no commits yet has no branch to name; main is what git would make.
        default_branch: crate::git::default_branch(root).unwrap_or_else(|_| "main".into()),
        remote_url: crate::git::remote_url(root),
        root_path: main.to_string(),
        setup_script: None,
        run_script: None,
        archive_script: None,
        // .env files are git-ignored, so a fresh worktree would not have them (as when you add a repo).
        prefs: crate::store::RepoPrefs { copy_files: vec![".env*".into()], ..Default::default() },
    })
}

fn new_workspace(repo: &Repo, p: &Place) -> Workspace {
    let history = p.folder == p.main;
    Workspace {
        id: new_id(),
        repo_id: repo.id.clone(),
        name: if history { "history".into() } else { Path::new(&p.folder).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() },
        branch: p.branch.clone(),
        path: p.folder.clone(),
        target_branch: repo.default_branch.clone(),
        state: "active".into(),
        pinned: false,
        unread: false,
        notes: String::new(),
        imported_from: Some(if history { FROM_HISTORY } else { FROM_TERMINAL }.into()),
        created_at: crate::store::now(),
        title: if history { "History".into() } else { String::new() },
        status: String::new(),
        pair_id: String::new(),
        section_id: String::new(),
        parent_id: String::new(),
        title_source: if history { crate::title::USER.into() } else { String::new() },
        review_pr: 0,
        group_id: String::new(),
    }
}

/// Imports the picked sessions from `scan`. `db` hands out the store for one short step at a time,
/// so the app stays responsive while big histories are read: files are parsed on every core with
/// the store unlocked, and each chat is written in its own transaction (all of it or none).
pub fn import<D: Deref<Target = Store>>(db: impl Fn() -> D, scan: &Scan, picks: &[Pick]) -> Result<Report> {
    let mut report = Report::default();
    let known = known_ids(&db())?;
    let want: HashSet<(&str, &str)> = picks.iter().map(|p| (p.agent.as_str(), p.id.as_str())).collect();
    let mut todo: Vec<(&Found, &Place)> = vec![];
    for f in scan.found.iter().filter(|f| want.contains(&(f.agent, f.id.as_str()))) {
        match scan.places.get(&f.cwd) {
            Some(Ok(p)) if !known.contains(&(f.agent.to_string(), f.id.clone())) => todo.push((f, p)),
            _ => report.skipped += 1,
        }
    }
    report.skipped += picks.len().saturating_sub(todo.len() + report.skipped);
    // Oldest first, so chats line up in the order they happened.
    todo.sort_by(|a, b| a.0.first.cmp(&b.0.first));

    // Repos and workspaces first.
    let mut ws_for: HashMap<String, Workspace> = HashMap::new();
    {
        let mut have = have(&db())?;
        for (_, p) in &todo {
            if ws_for.contains_key(&p.folder) {
                continue;
            }
            let repo = match have.repos.get(&p.main) {
                Some(r) => r.clone(),
                None => {
                    let r = match new_repo(&p.main) {
                        Ok(r) => r,
                        Err(err) => {
                            report.problems.push(format!("{}: {err}", p.main));
                            continue;
                        }
                    };
                    db().insert_repo(&r)?;
                    report.repos += 1;
                    have.repos.insert(p.main.clone(), r.clone());
                    r
                }
            };
            let ws = match have.workspaces.get(&p.folder) {
                Some(w) => w.clone(),
                None => {
                    let w = new_workspace(&repo, p);
                    db().insert_workspace(&w)?;
                    report.workspaces += 1;
                    have.workspaces.insert(p.folder.clone(), w.clone());
                    w
                }
            };
            ws_for.insert(p.folder.clone(), ws);
        }
    }

    let todo: Vec<(&Found, &Place)> = todo.into_iter().filter(|(_, p)| ws_for.contains_key(&p.folder)).collect();
    // Parse a batch on every core with the store unlocked, then write it in order. One batch at a
    // time keeps memory bounded however big the history is.
    let batch = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4) * 2;
    for chunk in todo.chunks(batch) {
        let parsed = par_map(chunk, |(f, _)| events(f));
        for (&(f, p), evs) in chunk.iter().zip(parsed) {
            let evs = match evs {
                Ok(e) => e,
                Err(err) => {
                    report.problems.push(format!("{}: {err}", f.file.display()));
                    continue;
                }
            };
            let ws = &ws_for[&p.folder];
            let session = Session {
                id: new_id(),
                workspace_id: ws.id.clone(),
                agent: f.agent.into(),
                agent_session_id: resumable(f, p).then(|| f.id.clone()),
                title: if f.title.is_empty() { "Imported chat".into() } else { f.title.clone() },
                model: None,
                permission_mode: "acceptEdits".into(),
                effort: None,
            };
            let created = if f.first.is_empty() { crate::store::now() } else { f.first.clone() };
            if db().import_terminal_chat(&session, f.agent, &f.id, &f.file.to_string_lossy(), &created, &evs)? {
                report.chats += 1;
                report.messages += evs.len();
            } else {
                report.skipped += 1;
            }
        }
    }
    // New worktree workspaces are named after their first prompt.
    db().backfill_titles()?;
    Ok(report)
}

/// The refusal for anything that would change a History workspace's folder: it is your main
/// checkout, which nekotrail never edits.
pub fn refuse_in_history(ws: &Workspace) -> Result<(), String> {
    if ws.imported_from.as_deref() == Some(FROM_HISTORY) {
        return Err("This is your main checkout's chat history, kept read-only so nothing in that folder changes. Create a workspace to keep working.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_FIX: &str = include_str!("../tests/fixtures/terminal/claude-session.jsonl");
    const CODEX_FIX: &str = include_str!("../tests/fixtures/terminal/codex-rollout.jsonl");
    const CODEX_OLD_FIX: &str = include_str!("../tests/fixtures/terminal/codex-legacy.jsonl");
    const GEMINI_FIX: &str = include_str!("../tests/fixtures/terminal/gemini-session.jsonl");

    fn kinds(evs: &[(Value, String)]) -> Vec<String> {
        evs.iter().map(|(e, _)| e["kind"].as_str().unwrap().to_string()).collect()
    }

    /// A home with a git repo, a linked worktree and a subfolder, and each CLI's folders.
    struct Home {
        _tmp: tempfile::TempDir,
        roots: Roots,
        repo: PathBuf,
        wt: PathBuf,
        claude_dir: PathBuf,
    }

    fn sh(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(ok.status.success(), "{args:?}: {}", String::from_utf8_lossy(&ok.stderr));
    }

    fn escaped(p: &Path) -> String {
        p.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
    }

    fn home() -> Home {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let repo = base.join("code").join("demo");
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        sh(&repo, &["init", "-q", "-b", "main"]);
        sh(&repo, &["-c", "user.email=a@b", "-c", "user.name=a", "commit", "-q", "--allow-empty", "-m", "init"]);
        let wt = base.join("code").join("demo-fix");
        sh(&repo, &["worktree", "add", "-q", "-b", "me/fix", wt.to_str().unwrap()]);
        let claude = base.join(".claude").join("projects");
        let codex = base.join(".codex").join("sessions");
        let gemini = base.join(".gemini").join("tmp");
        let put = |p: PathBuf, body: &str| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        let at = |dir: &Path| CLAUDE_FIX.replace("/REPO", dir.to_str().unwrap());
        // Main checkout, worktree, subfolder, and a folder outside any repo.
        put(claude.join(escaped(&repo)).join("c-main.jsonl"), &at(&repo).replace("SID", "c-main"));
        put(claude.join(escaped(&wt)).join("c-wt.jsonl"), &at(&wt).replace("SID", "c-wt"));
        put(claude.join(escaped(&repo.join("sub"))).join("c-sub.jsonl"), &at(&repo.join("sub")).replace("SID", "c-sub"));
        put(claude.join(escaped(&base)).join("c-out.jsonl"), &at(&base).replace("SID", "c-out"));
        // A subagent transcript one folder down is not a session of its own.
        put(claude.join(escaped(&repo)).join("c-main").join("subagents").join("agent-1.jsonl"), &at(&repo).replace("SID", "agent-1"));
        put(codex.join("2026/11/05").join("rollout-2026-11-05T10-00-00-x-new.jsonl"), &CODEX_FIX.replace("/REPO", wt.to_str().unwrap()));
        put(codex.join("2026/01/02").join("rollout-2026-01-02T10-00-00-x-old.jsonl"), &CODEX_OLD_FIX.replace("/REPO", repo.to_str().unwrap()));
        put(base.join(".codex").join("session_index.jsonl"), "{\"id\":\"x-new\",\"thread_name\":\"Fix the login bug\"}\n");
        put(gemini.join("demo").join(".project_root"), repo.to_str().unwrap());
        put(gemini.join("demo").join("chats").join("session-2026-11-05T10-00-g1.jsonl"), GEMINI_FIX);
        let roots = Roots { claude: vec![claude.clone()], codex: vec![codex], gemini: vec![gemini] };
        Home { _tmp: tmp, roots, repo, wt, claude_dir: claude }
    }

    /// Every file under `dir` with its bytes and modification time.
    fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
        let mut v = vec![];
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    v.push((p.clone(), std::fs::read(&p).unwrap(), std::fs::metadata(&p).unwrap().modified().unwrap()));
                }
            }
        }
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    #[test]
    fn claude_session_reads_prompts_tools_and_titles() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("SID.jsonl");
        std::fs::write(&f, CLAUDE_FIX).unwrap();
        let found = scan_claude(&f).unwrap();
        assert_eq!(found.cwd, "/REPO");
        assert_eq!(found.title, "Fix the flaky login test", "the custom title wins over the AI title");
        // Two prompts (one a slash command) and two text replies; tool calls and injected text don't count.
        assert_eq!(found.messages, 4);
        assert_eq!(found.first, "2026-11-01T09:00:00+00:00");
        assert_eq!(found.last, "2026-11-01T09:05:00+00:00");

        let evs = claude_events(&f).unwrap();
        assert_eq!(kinds(&evs), ["user", "thinking", "tool", "tool_result", "text", "done", "user", "text", "done"]);
        assert_eq!(evs[0].0["text"], "Fix the flaky login test in auth.spec.ts");
        assert_eq!(evs[6].0["text"], "/review src");
        assert!(evs.iter().all(|(e, _)| e["imported"] == true));
        assert_eq!(evs[0].1, "2026-11-01T09:00:00+00:00", "each event keeps its own time");
        // A huge tool output is clipped, not copied whole.
        let out = evs[3].0["output"].as_str().unwrap();
        assert!(out.len() < 5000 && out.contains("more bytes"), "{}", out.len());
    }

    #[test]
    fn typed_text_drops_injected_text_and_shows_commands() {
        assert_eq!(typed_text("<command-message>x</command-message>\n<command-name>/mcp</command-name>"), Some("/mcp".into()));
        assert_eq!(typed_text("<bash-input>ls -la</bash-input>"), Some("! ls -la".into()));
        assert_eq!(typed_text("<local-command-stdout>ok</local-command-stdout>"), None);
        assert_eq!(typed_text("<task-notification>\n<task-id>1</task-id>"), None);
        assert_eq!(typed_text("   "), None);
        assert_eq!(typed_text("<system_instruction>\nYou are in an app.\n</system_instruction>\n\nShip the fix"), Some("Ship the fix".into()));
        assert_eq!(typed_text("<system_instruction>only this</system_instruction>"), None);
        assert_eq!(typed_text(" fix it "), Some("fix it".into()));
    }

    #[test]
    fn codex_new_and_old_formats() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("new.jsonl");
        std::fs::write(&f, CODEX_FIX).unwrap();
        let found = scan_codex(&f, &HashMap::new()).unwrap();
        assert_eq!((found.id.as_str(), found.cwd.as_str(), found.messages, found.codex_items), ("x-new", "/REPO", 3, true));
        assert_eq!(found.title, "Add retries to the webhook sender");
        let evs = codex_events(&f, true).unwrap();
        assert_eq!(kinds(&evs), ["user", "thinking", "text", "tool", "tool_result", "tool", "tool_result", "text", "done"]);
        assert_eq!(evs[3].0["input"], "rg -n retry");
        assert_eq!(evs[4].0["is_error"], true);
        assert_eq!(evs[5].0["input"], "/REPO/src/send.ts");

        let f = tmp.path().join("old.jsonl");
        std::fs::write(&f, CODEX_OLD_FIX).unwrap();
        let found = scan_codex(&f, &HashMap::from([("x-old".to_string(), "Old thread".to_string())])).unwrap();
        assert_eq!((found.id.as_str(), found.messages, found.codex_items, found.title.as_str()), ("x-old", 4, false, "Old thread"));
        let evs = codex_events(&f, false).unwrap();
        assert_eq!(kinds(&evs), ["user", "thinking", "tool", "tool_result", "tool", "tool_result", "text", "done", "user", "text", "done"]);
        assert_eq!(evs[2].0["input"], "ls src");
        assert_eq!(evs[3].0["output"], "a.ts\n");
        assert_eq!(evs[4].0["input"], "src/a.ts");
        assert_eq!(evs[10].0["stopped"], true, "an interrupted turn ends as stopped");
    }

    #[test]
    fn codex_subagent_threads_are_not_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("sub.jsonl");
        std::fs::write(&f, CODEX_FIX.replace("\"source\":\"cli\"", "\"source\":{\"subagent\":{\"thread_spawn\":{}}}")).unwrap();
        assert_eq!(scan_codex(&f, &HashMap::new()), None);
    }

    #[test]
    fn gemini_replays_rewinds_and_patches() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("session-x.jsonl");
        std::fs::write(&f, GEMINI_FIX).unwrap();
        let s = gemini_load(&f).unwrap();
        assert_eq!(s.id, "g1-session");
        // m3 and m4 were rewound away; m2's tool result came in a patch.
        assert_eq!(s.messages.iter().map(|m| m["id"].as_str().unwrap()).collect::<Vec<_>>(), ["m1", "m2", "m5", "m6"]);
        let found = scan_gemini(&f, "/REPO").unwrap();
        assert_eq!((found.messages, found.title.as_str()), (4, "Rename the config loader"));
        let evs = gemini_events(&f).unwrap();
        assert_eq!(kinds(&evs), ["user", "thinking", "text", "tool", "tool_result", "done", "user", "text", "done"]);
        assert_eq!(evs[3].0["name"], "Bash");
        assert_eq!(evs[4].0["output"], "renamed");

        // The older single-object file.
        let old = tmp.path().join("session-old.json");
        std::fs::write(&old, r#"{"sessionId":"g0","projectHash":"h","startTime":"2025-09-01T00:00:00Z","lastUpdated":"2025-09-01T00:10:00Z",
            "messages":[{"id":"a","type":"user","content":"hello","timestamp":"2025-09-01T00:00:00Z"},
                        {"id":"b","type":"gemini","content":"hi there","timestamp":"2025-09-01T00:01:00Z"}]}"#).unwrap();
        let found = scan_gemini(&old, "/x").unwrap();
        assert_eq!((found.id.as_str(), found.messages), ("g0", 2));
    }

    #[test]
    fn truncated_and_unknown_lines_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("t.jsonl");
        // A line from a future CLI version, a non-object line, and a last line cut off mid-write.
        let body = format!("{CLAUDE_FIX}{{\"type\":\"hologram\",\"x\":1}}\n[1,2]\n{{\"type\":\"user\",\"cwd\":\"/REPO\",\"message\":{{\"content\":\"cut o");
        std::fs::write(&f, body).unwrap();
        assert_eq!(scan_claude(&f).unwrap().messages, 4);
        assert_eq!(claude_events(&f).unwrap().len(), 9);
        let g = tmp.path().join("g.jsonl");
        std::fs::write(&g, format!("{GEMINI_FIX}{{\"id\":\"m9\",\"type\":\"user\",\"content\":\"half")).unwrap();
        assert_eq!(gemini_load(&g).unwrap().messages.len(), 4);
    }

    #[test]
    fn groups_by_repo_and_worktree() {
        let h = home();
        let s = scan(&h.roots);
        let ids: HashSet<&str> = s.found.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, HashSet::from(["c-main", "c-wt", "c-sub", "c-out", "x-new", "x-old", "g1-session"]), "subagent transcripts are skipped");
        let store = Store::in_memory().unwrap();
        let p = preview(&store, &s, &h.roots).unwrap();
        assert_eq!(p.repos.len(), 1);
        let r = &p.repos[0];
        assert_eq!((r.root.as_str(), r.repo_id.is_none()), (h.repo.to_str().unwrap(), true));
        let main = r.folders.iter().find(|f| f.kind == "history").unwrap();
        assert_eq!(main.path, h.repo.to_str().unwrap());
        let mut main_ids: Vec<&str> = main.sessions.iter().map(|x| x.id.as_str()).collect();
        main_ids.sort();
        assert_eq!(main_ids, ["c-main", "c-sub", "g1-session", "x-old"], "a subfolder belongs to its checkout");
        assert!(!main.sessions.iter().find(|x| x.id == "c-sub").unwrap().resumable, "Claude only resumes from the folder it started in");
        let wt = r.folders.iter().find(|f| f.kind == "worktree").unwrap();
        assert_eq!((wt.path.as_str(), wt.branch.as_str()), (h.wt.to_str().unwrap(), "me/fix"));
        assert_eq!(p.skipped.len(), 1, "{:?}", p.skipped);
        assert_eq!(p.skipped[0].reason, "Not in a git repo.");
    }

    #[test]
    fn import_is_read_only_and_idempotent() {
        let h = home();
        let cli_dirs = [h.claude_dir.parent().unwrap().to_path_buf(), h.roots.codex[0].parent().unwrap().to_path_buf(), h.roots.gemini[0].parent().unwrap().to_path_buf()];
        let before: Vec<_> = cli_dirs.iter().map(|d| snapshot(d)).collect();
        let s = scan(&h.roots);
        let store = Store::in_memory().unwrap();
        let picks: Vec<Pick> = s.found.iter().map(|f| Pick { agent: f.agent.into(), id: f.id.clone() }).collect();
        let r = import(|| &store, &s, &picks).unwrap();
        assert_eq!((r.repos, r.workspaces, r.chats, r.skipped), (1, 2, 6, 1), "{r:?}");
        assert!(r.problems.is_empty(), "{:?}", r.problems);

        // The main checkout became one read-only History workspace; no worktree was made.
        let wss = store.workspaces(false).unwrap();
        let hist = wss.iter().find(|w| w.path == h.repo.to_str().unwrap()).unwrap();
        assert_eq!((hist.imported_from.as_deref(), hist.title.as_str()), (Some(FROM_HISTORY), "History"));
        assert!(refuse_in_history(hist).is_err());
        let wt = wss.iter().find(|w| w.path == h.wt.to_str().unwrap()).unwrap();
        assert_eq!((wt.branch.as_str(), wt.imported_from.as_deref()), ("me/fix", Some(FROM_TERMINAL)));
        assert_eq!(wt.title, "Fix the flaky login test in auth.spec.ts", "named after its first prompt");
        assert_eq!(std::fs::read_dir(h.repo.parent().unwrap()).unwrap().count(), 2, "no new folders next to the repo");

        // Chats keep the CLI's session id, so sending continues it.
        let chats = store.sessions(&wt.id).unwrap();
        let codex = chats.iter().find(|c| c.agent == "codex").unwrap();
        assert_eq!(codex.agent_session_id.as_deref(), Some("x-new"));
        assert_eq!(codex.title, "Fix the login bug", "Codex's own thread name");
        let sub = store.sessions(&hist.id).unwrap().into_iter().find(|c| c.title.contains("flaky") && c.agent_session_id.is_none());
        assert!(sub.is_some(), "the subfolder session has no id to resume from here");
        assert!(store.events(&codex.id).unwrap().iter().all(|e| e["imported"] == true));

        // It went into the search index.
        let hits = store.search("webhook", &Default::default()).unwrap();
        assert!(!hits.is_empty());

        // Again: nothing new. Then one more session appears and only it comes in.
        let again = import(|| &store, &scan(&h.roots), &picks).unwrap();
        assert_eq!((again.repos, again.workspaces, again.chats), (0, 0, 0), "{again:?}");
        let f = h.claude_dir.join(escaped(&h.wt)).join("c-new.jsonl");
        std::fs::write(&f, CLAUDE_FIX.replace("/REPO", h.wt.to_str().unwrap())).unwrap();
        let s = scan(&h.roots);
        let p = preview(&store, &s, &h.roots).unwrap();
        let rows: Vec<&Row> = p.repos.iter().flat_map(|r| &r.folders).flat_map(|f| &f.sessions).collect();
        assert_eq!(rows.iter().filter(|r| !r.imported).map(|r| r.id.as_str()).collect::<Vec<_>>(), ["c-new"]);
        let more = import(|| &store, &s, &[Pick { agent: "claude".into(), id: "c-new".into() }]).unwrap();
        assert_eq!((more.chats, more.workspaces), (1, 0));
        std::fs::remove_file(&f).unwrap();

        // The CLIs' folders are exactly as they were: same files, same bytes, same modification times.
        let after: Vec<_> = cli_dirs.iter().map(|d| snapshot(d)).collect();
        assert_eq!(after, before);
    }

    #[test]
    fn a_chat_nekotrail_ran_itself_is_not_imported_twice() {
        let h = home();
        let s = scan(&h.roots);
        let store = Store::in_memory().unwrap();
        let repo = new_repo(h.wt.to_str().unwrap()).map(|mut r| { r.root_path = h.repo.to_string_lossy().to_string(); r }).unwrap();
        store.insert_repo(&repo).unwrap();
        let p = place(h.wt.to_str().unwrap()).unwrap();
        let mut ws = new_workspace(&repo, &p);
        ws.imported_from = None;
        store.insert_workspace(&ws).unwrap();
        store.insert_session(&Session { id: new_id(), workspace_id: ws.id.clone(), agent: "claude".into(), agent_session_id: Some("c-wt".into()), title: "x".into(), model: None, permission_mode: "acceptEdits".into(), effort: None }).unwrap();
        let pv = preview(&store, &s, &h.roots).unwrap();
        let folder = pv.repos[0].folders.iter().find(|f| f.path == p.folder).unwrap();
        assert_eq!((folder.kind, folder.workspace_id.as_deref()), ("workspace", Some(ws.id.as_str())), "matched to the workspace you have");
        assert!(folder.sessions.iter().find(|x| x.id == "c-wt").unwrap().imported);
        assert_eq!(pv.repos[0].repo_id.as_deref(), Some(repo.id.as_str()), "matched to the repo you added");
        let r = import(|| &store, &s, &[Pick { agent: "claude".into(), id: "c-wt".into() }, Pick { agent: "codex".into(), id: "x-new".into() }]).unwrap();
        assert_eq!((r.chats, r.skipped, r.workspaces, r.repos), (1, 1, 0, 0), "{r:?}");
    }

    #[test]
    #[cfg(unix)]
    fn roots_read_each_folder_once() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        std::os::unix::fs::symlink(&a, tmp.path().join("b")).unwrap();
        let mut v = vec![a.clone(), tmp.path().join("b"), tmp.path().join("missing")];
        dedupe_dirs(&mut v);
        assert_eq!(v, [a]);
    }

    /// Times a scan of this Mac's real history (read-only). `cargo test -- --ignored real_history --nocapture`
    #[test]
    #[ignore]
    fn real_history() {
        let roots = Roots::from_env();
        let t = std::time::Instant::now();
        let s = scan(&roots);
        let scan_ms = t.elapsed().as_millis();
        let bytes: u64 = s.found.iter().map(|f| f.bytes).sum();
        let store = Store::in_memory().unwrap();
        let p = preview(&store, &s, &roots).unwrap();
        println!("roots {roots:?}");
        println!("scan: {} sessions ({:?}), {} MB, {} folders, {} repos, {} skipped folders, {scan_ms} ms",
            s.found.len(), p.found, bytes / 1_000_000, s.places.len(), p.repos.len(), p.skipped.len());
        let mut why: HashMap<&str, (usize, usize)> = HashMap::new();
        for k in &p.skipped {
            let e = why.entry(k.reason.as_str()).or_default();
            e.0 += 1;
            e.1 += k.sessions;
        }
        println!("skipped (folders, sessions) by reason: {why:?}");
        for r in &p.repos {
            let n: usize = r.folders.iter().map(|f| f.sessions.len()).sum();
            let kinds: Vec<&str> = r.folders.iter().map(|f| f.kind).collect();
            println!("  {} ({}): {n} sessions in {kinds:?}", r.name, if r.repo_id.is_some() { "added" } else { "new" });
        }
        let t = std::time::Instant::now();
        let n: usize = par_map(&s.found, |f| events(f).map(|e| e.len()).unwrap_or(0)).into_iter().sum();
        println!("parse every session into chat events: {n} events, {} ms", t.elapsed().as_millis());
        let picks: Vec<Pick> = s.found.iter().map(|f| Pick { agent: f.agent.into(), id: f.id.clone() }).collect();
        let t = std::time::Instant::now();
        let r = import(|| &store, &s, &picks).unwrap();
        println!("import everything into an in-memory store: {r:?} in {} ms", t.elapsed().as_millis());
    }
}
