//! A client for the Agent Client Protocol (<https://agentclientprotocol.com>, protocol version 1):
//! JSON-RPC 2.0 over the agent's stdin and stdout, one message per line.
//!
//! A turn is still one process, like every other agent, so queueing, Stop and resume work the
//! same way: start the agent, `initialize`, open the chat (`session/resume` or `session/load` to
//! continue one, else `session/new` in the worktree), set model and mode, send `session/prompt`,
//! turn its `session/update` notifications into chat events, and close stdin once the prompt is
//! answered. Stop sends `session/cancel`. `session/request_permission` becomes the "Ask before
//! changes" card (Allow, Deny, Allow for this chat).
//!
//! nekotrail offers the agent no file system and no terminal (both capabilities are off): the
//! agent uses its own tools in the worktree, and any `fs/*` or `terminal/*` request is answered
//! "method not found". OpenCode 1.19 still sends `fs/write_text_file` after an approved edit, to
//! sync an editor; it has written the file itself by then and carries on.
//!
//! What differs between agents is handled generically: plan mode may be a URI (`...#plan`);
//! "Ask before changes" is an env var or a session config option the entry names, and a turn
//! that can't get one stops rather than run without asking; "Auto-accept edits" answers read and
//! edit requests inside the worktree itself and shows a card for the rest; an agent that wants
//! `authenticate` before `session/new` gets it once, only after it says "auth required".
//!
//! Protected paths (see `guard`) hold for every agent here: any permission request to change a
//! protected file is refused, with the reason in the chat, before Full auto or "Auto-accept edits"
//! could allow it. That only works when the agent asks. An agent that edits without asking (its
//! own default in most modes) is caught by the check after the turn instead. OpenCode can be told
//! to ask before every edit (`edit_ask_env`), so for it the refusal holds in every mode.
//!
//! Adding an ACP agent is one `AcpAgent` entry (command, args, name, repo config) in
//! `agent::ALL`, plus its line in `src/agents.ts` and `models.json`.

use crate::agent::{clip, Agent, Checked, Parser, TurnSpec};
use crate::agentcfg::{env_file, scan, whole, Item, RepoFile};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicU64, Ordering};

pub const PROTOCOL_VERSION: u64 = 1;

/// JSON-RPC "method not found", for requests nekotrail does not serve.
const METHOD_NOT_FOUND: i64 = -32601;
/// ACP's "auth_required" error code.
const AUTH_REQUIRED: i64 = -32000;

/// One agent CLI driven over ACP. Everything else is shared.
pub struct AcpAgent {
    /// Stored on each chat, and the key in `src/agents.ts` and `models.json`.
    pub id: &'static str,
    /// For messages: "OpenCode".
    pub name: &'static str,
    /// The CLI on PATH.
    pub bin: &'static str,
    /// What starts its ACP server on stdio.
    pub args: &'static [&'static str],
    /// What to run to sign in, for the error when it isn't.
    pub login: &'static str,
    /// Environment that makes the agent skip the repo's own config (what could run commands),
    /// used until the user trusts it. Empty: the agent can't, so untrusted config stops the turn.
    pub config_off_env: &'static [(&'static str, &'static str)],
    /// Environment for "Ask before changes": the agent asks before edits, commands and fetches.
    pub ask_env: &'static [(&'static str, &'static str)],
    /// Environment that makes the agent ask before every edit, used in the other modes when the
    /// workspace has protected paths: nekotrail answers each request itself, refusing edits to a
    /// protected path and allowing the rest (see `guard`). Empty: protected paths are refused only
    /// when the agent asks on its own, and checked after the turn.
    pub edit_ask_env: &'static [(&'static str, &'static str)],
    /// A session config option set for "Ask before changes" (`configId`, `value`), when the agent
    /// offers one that makes it ask (Copilot's `allow_all` off). A turn whose agent doesn't offer
    /// it stops instead of running without asking.
    pub ask_option: Option<(&'static str, &'static str)>,
    /// The ACP auth method that signs in with the CLI's own saved login, for agents that answer
    /// `session/new` with "auth required" until the client calls `authenticate` (Cursor). Only
    /// tried after that error, so a signed-out CLI never starts a browser login unasked.
    pub auth_method: Option<&'static str>,
    /// What the setup check runs to see the CLI is there (empty: `bin --version`).
    pub probe: &'static [&'static str],
    /// What the repo's config for this agent would run, and its fingerprint (see `agentcfg`).
    pub repo_config: RepoConfig,
}

/// What a repo's config for an agent would run, and its fingerprint (`agentcfg::check_*`).
pub type ConfigCheck = fn(&Path) -> Option<(Vec<Item>, String)>;

/// How nekotrail finds the repo config an ACP agent would run.
pub enum RepoConfig {
    /// It reads none that can run commands.
    None,
    /// A check of its own (OpenCode).
    Check(ConfigCheck),
    /// The files its docs say it reads, read conservatively (`agentcfg::check_files`).
    Files(&'static [RepoFile]),
}

impl AcpAgent {
    /// Defaults for a new entry: `AcpAgent { id: "cursor", name: "Cursor", bin: "cursor-agent", ..AcpAgent::BASE }`.
    /// Give it a `repo_config` before shipping if its repo config can run commands.
    pub const BASE: AcpAgent = AcpAgent {
        id: "",
        name: "",
        bin: "",
        args: &["acp"],
        login: "",
        config_off_env: &[],
        ask_env: &[],
        edit_ask_env: &[],
        ask_option: None,
        auth_method: None,
        probe: &[],
        repo_config: RepoConfig::None,
    };

    /// Can "Ask before changes" be promised for this agent?
    pub fn asks(&self) -> bool {
        !self.ask_env.is_empty() || self.ask_option.is_some()
    }
}

/// OpenCode (`opencode acp`). `OPENCODE_DISABLE_PROJECT_CONFIG` skips the repo's `opencode.json`,
/// `.opencode/` (plugins, custom tools) and `AGENTS.md`; `OPENCODE_PERMISSION` is merged over
/// every other config, so the repo can't switch "ask" off. Both read from OpenCode 1.19.5.
pub static OPENCODE: AcpAgent = AcpAgent {
    id: "opencode",
    name: "OpenCode",
    bin: "opencode",
    args: &["acp"],
    login: "opencode auth login",
    config_off_env: &[("OPENCODE_DISABLE_PROJECT_CONFIG", "1")],
    ask_env: &[("OPENCODE_PERMISSION", r#"{"edit":"ask","bash":"ask","webfetch":"ask"}"#)],
    edit_ask_env: &[("OPENCODE_PERMISSION", r#"{"edit":"ask"}"#)],
    repo_config: RepoConfig::Check(crate::agentcfg::check_opencode),
    ..AcpAgent::BASE
};

// The agents below come from the ACP registry (github.com/agentclientprotocol/registry) and each
// vendor's own docs, checked 2026-11-06. Only Copilot (1.0.103) and Grok Build (1.0.70) were on
// this Mac; their `initialize` and `session/new` answers were read live. None documents a switch
// to load a folder without its repo config, so `config_off_env` is empty for all of them and an
// untrusted config stops their turn; `agentcfg` says how their files are read.

/// Cursor CLI. `cursor-agent`, not `agent`: Grok Build installs a binary called `agent` too.
/// Cursor's documented flow calls `authenticate` with `cursor_login` before `session/new`.
pub static CURSOR: AcpAgent = AcpAgent {
    id: "cursor",
    name: "Cursor",
    bin: "cursor-agent",
    login: "cursor-agent login",
    auth_method: Some("cursor_login"),
    repo_config: RepoConfig::Files(&[scan(".cursor/cli.json"), scan(".cursor/mcp.json"), scan(".cursor/hooks.json")]),
    ..AcpAgent::BASE
};

/// GitHub Copilot CLI (`copilot --acp`). Offers `allow_all` (off: it asks before writes and
/// commands), plan mode as `...session-modes#plan`, and its models over ACP. It also keeps its
/// own folder trust: in a folder it hasn't trusted, `copilot mcp list` skipped the repo's
/// `.mcp.json` and `.github/mcp.json`.
pub static COPILOT: AcpAgent = AcpAgent {
    id: "copilot",
    name: "Copilot",
    bin: "copilot",
    args: &["--acp"],
    login: "copilot login",
    ask_option: Some(("allow_all", "off")),
    repo_config: RepoConfig::Files(&[
        scan(".github/hooks/"),
        scan(".github/copilot/settings.json"),
        scan(".github/copilot/settings.local.json"),
        scan(".mcp.json"),
        scan(".github/mcp.json"),
        scan(".github/lsp.json"),
    ]),
    ..AcpAgent::BASE
};

/// Goose (`goose acp`). Its extensions live in the user's config; recipes in `.goose/` can name
/// commands, so that folder is read whole.
pub static GOOSE: AcpAgent = AcpAgent {
    id: "goose",
    name: "Goose",
    bin: "goose",
    login: "goose configure",
    repo_config: RepoConfig::Files(&[whole(".goose/")]),
    ..AcpAgent::BASE
};

/// Qwen Code (`qwen --acp`), a Gemini CLI fork: the same settings shape, under `.qwen/`.
pub static QWEN: AcpAgent = AcpAgent {
    id: "qwen",
    name: "Qwen Code",
    bin: "qwen",
    args: &["--acp"],
    login: "qwen",
    repo_config: RepoConfig::Files(&[scan(".qwen/settings.json"), env_file(".qwen/.env"), env_file(".env")]),
    ..AcpAgent::BASE
};

/// Moonshot's Kimi Code CLI (`kimi acp`). Sign-in is `/login` inside `kimi`.
pub static KIMI: AcpAgent = AcpAgent {
    id: "kimi",
    name: "Kimi",
    bin: "kimi",
    login: "kimi",
    repo_config: RepoConfig::Files(&[scan(".kimi/")]),
    ..AcpAgent::BASE
};

/// Augment's Auggie CLI (`auggie --acp`).
pub static AUGGIE: AcpAgent = AcpAgent {
    id: "auggie",
    name: "Auggie",
    bin: "auggie",
    args: &["--acp"],
    login: "auggie login",
    repo_config: RepoConfig::Files(&[scan(".augment/settings.json"), scan(".augment/settings.local.json"), scan(".augment/mcp.json"), whole(".augment/hooks/")]),
    ..AcpAgent::BASE
};

/// xAI's Grok Build (`grok agent stdio`). `grok inspect` shows it also loads Claude Code's and
/// Cursor's repo config (MCP servers, hooks), so those files count as its own too.
pub static GROK: AcpAgent = AcpAgent {
    id: "grok",
    name: "Grok",
    bin: "grok",
    args: &["agent", "stdio"],
    login: "grok login",
    repo_config: RepoConfig::Files(&[
        scan(".grok/"),
        scan(".claude/settings.json"),
        scan(".claude/settings.local.json"),
        scan(".mcp.json"),
        scan(".cursor/mcp.json"),
        scan(".cursor/hooks.json"),
    ]),
    ..AcpAgent::BASE
};

/// Amazon's Kiro CLI (`kiro-cli acp`).
pub static KIRO: AcpAgent = AcpAgent {
    id: "kiro",
    name: "Kiro",
    bin: "kiro-cli",
    login: "kiro-cli login",
    repo_config: RepoConfig::Files(&[scan(".kiro/settings/"), scan(".kiro/agents/"), scan(".kiro/hooks/")]),
    ..AcpAgent::BASE
};

/// Factory's Droid (`droid exec --output-format acp`, as Factory's own Zed guide runs it).
pub static DROID: AcpAgent = AcpAgent {
    id: "droid",
    name: "Droid",
    bin: "droid",
    args: &["exec", "--output-format", "acp"],
    login: "droid",
    repo_config: RepoConfig::Files(&[scan(".factory/settings.json"), scan(".factory/settings.local.json"), scan(".factory/mcp.json"), whole(".factory/hooks/")]),
    ..AcpAgent::BASE
};

/// Mistral Vibe. Its ACP server is its own binary, `vibe-acp`, installed with `vibe`.
pub static VIBE: AcpAgent = AcpAgent {
    id: "vibe",
    name: "Vibe",
    bin: "vibe-acp",
    args: &[],
    login: "vibe --setup",
    probe: &["vibe", "--version"],
    repo_config: RepoConfig::Files(&[scan(".vibe/config.toml"), scan(".vibe/hooks.toml"), scan(".vibe/agents/")]),
    ..AcpAgent::BASE
};

/// Cline CLI (`cline --acp`). Its hooks are scripts in `.clinerules/hooks/`.
pub static CLINE: AcpAgent = AcpAgent {
    id: "cline",
    name: "Cline",
    bin: "cline",
    args: &["--acp"],
    login: "cline auth",
    repo_config: RepoConfig::Files(&[whole(".clinerules/hooks/")]),
    ..AcpAgent::BASE
};

/// Kilo CLI (`kilo acp`), an OpenCode fork: its own files, and OpenCode's in case it still reads them.
pub static KILO: AcpAgent = AcpAgent {
    id: "kilo",
    name: "Kilo",
    bin: "kilo",
    login: "kilo auth login",
    repo_config: RepoConfig::Files(&[
        scan("kilo.json"),
        scan("kilo.jsonc"),
        scan(".kilo/"),
        scan(".kilocode/mcp.json"),
        scan("opencode.json"),
        scan("opencode.jsonc"),
        scan(".opencode/"),
    ]),
    ..AcpAgent::BASE
};

/// The ACP agents after OpenCode, in the order the UI lists them.
pub static MORE: [&AcpAgent; 12] = [&CURSOR, &COPILOT, &GOOSE, &QWEN, &KIMI, &AUGGIE, &GROK, &KIRO, &DROID, &VIBE, &CLINE, &KILO];

impl Agent for AcpAgent {
    fn id(&self) -> &'static str {
        self.id
    }
    fn bin(&self) -> &'static str {
        self.bin
    }
    fn build(&self, spec: &TurnSpec, _ok: &Checked) -> Command {
        let mut c = Command::new(self.bin);
        c.args(self.args);
        if !spec.project_config {
            c.envs(self.config_off_env.iter().copied());
        }
        if spec.permission_mode == "ask" {
            c.envs(self.ask_env.iter().copied());
        } else if guards_edits(self, spec) {
            c.envs(self.edit_ask_env.iter().copied());
        }
        c
    }
    fn guard_level(&self) -> &'static str {
        // "asks": refused whenever the agent asks, which nekotrail can't make it do in every mode.
        if self.edit_ask_env.is_empty() { "asks" } else { "blocked" }
    }
    fn parser(&self, spec: &TurnSpec, ok: &Checked) -> Box<dyn Parser> {
        Box::new(Client::new(self, spec, ok))
    }
    fn interactive(&self, _permission_mode: &str) -> bool {
        true
    }
    fn first_input(&self, _prompt: &str) -> Option<String> {
        None
    }
    fn repo_config(&self, dir: &Path) -> Option<(Vec<Item>, String)> {
        match self.repo_config {
            RepoConfig::None => None,
            RepoConfig::Check(f) => f(dir),
            RepoConfig::Files(files) => crate::agentcfg::check_files(self.id, self.name, files, dir),
        }
    }
    fn probe(&self) -> (&'static str, &'static [&'static str]) {
        match self.probe.split_first() {
            Some((bin, args)) => (bin, args),
            None => (self.bin, &["--version"]),
        }
    }
    fn needs_trusted_config(&self) -> Option<String> {
        self.config_off_env.is_empty().then(|| {
            format!("This repo's {0} settings run commands, and {0} can't run with them switched off. Send again and choose Trust, or use another agent for this task.", self.name)
        })
    }
    fn config_off_env(&self) -> &'static [(&'static str, &'static str)] {
        self.config_off_env
    }
    fn stopped_by_user(&self, _status: Option<&ExitStatus>, _stderr: &str) -> bool {
        // Stop goes through `session/cancel` and the client reports it itself (`Client::finish`),
        // so a process that ends without an answer crashed.
        false
    }
}

/// Outside "Ask before changes" and Plan, with protected paths, for an agent that can be told to
/// ask before each edit: it asks, and nekotrail answers for the user (`Client::on_request`).
fn guards_edits(agent: &AcpAgent, spec: &TurnSpec) -> bool {
    !agent.edit_ask_env.is_empty() && !matches!(spec.permission_mode, "ask" | "plan") && crate::agent::active_guard(spec).is_some()
}

/// What we are waiting for an answer to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    Initialize,
    New,
    /// `session/resume` or `session/load`.
    Continue,
    /// `authenticate`, after the chat couldn't open without it.
    Auth,
    Config,
    Prompt,
}

/// Is this mode id plan mode? Plain `plan`, or a URI ending in `#plan` (Copilot).
fn is_plan(mode: &str) -> bool {
    mode == "plan" || mode.ends_with("#plan") || mode.ends_with("/plan")
}

/// Modes that approve everything by themselves. Leaving plan mode never lands on one of these.
fn runs_unattended(mode: &str) -> bool {
    let m = mode.to_ascii_lowercase();
    ["autopilot", "yolo", "bypass", "allow-all", "allow_all", "auto-approve", "full"].iter().any(|w| m.contains(w))
}

/// Tool kinds that only read or edit files in the worktree: what "Auto-accept edits" lets through
/// without a card. Commands, deletes, moves and fetches still ask.
const EDIT_KINDS: [&str; 4] = ["edit", "read", "search", "think"];

/// A tool call as the agent has described it so far. Agents announce a call first and fill in
/// its input with updates, so it reaches the chat once there is something to show.
#[derive(Default)]
struct Tool {
    /// The tool's own name when the agent gives one (`name`, or OpenCode's first `title`).
    hint: String,
    kind: String,
    title: String,
    input: Value,
    locations: Value,
    content: Value,
    output: Value,
    status: String,
    shown: bool,
    done: bool,
}

/// Unique across chats and launches: approvals from every running chat share one map.
fn approval_id() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("acp-{ms}-{}", N.fetch_add(1, Ordering::Relaxed))
}

/// One ACP turn: a state machine fed the agent's stdout, line by line.
pub struct Client {
    name: &'static str,
    login: &'static str,
    prompt: String,
    cwd: String,
    /// The same folder with symlinks resolved (/var is /private/var), to shorten paths either way.
    cwd_real: String,
    resume: Option<String>,
    model: Option<String>,
    plan: bool,
    /// Full auto: permission requests are allowed without asking.
    auto_allow: bool,
    /// "Auto-accept edits": requests to read or edit files are allowed without asking.
    auto_edits: bool,
    /// Protected paths: requests to change them are refused without asking, in every mode.
    guard: Option<crate::guard::Rules>,
    /// The agent was told to ask before every edit only for the guard (`guards_edits`), so edits
    /// to anything else are allowed without asking, as they would have been.
    guard_edits: bool,
    /// "Ask before changes", and the config option that makes this agent ask.
    ask: Option<(&'static str, &'static str)>,
    /// "Ask before changes" for an agent with no way to promise it.
    ask_missing: bool,
    auth_method: Option<&'static str>,
    /// `authenticate` was tried once this turn.
    authed: bool,
    next_id: u64,
    out: Vec<String>,
    waiting: HashMap<u64, Step>,
    config: VecDeque<(&'static str, Value)>,
    caps: Value,
    /// `authMethods` from `initialize`.
    auth_methods: Value,
    session: Option<String>,
    /// `session/load` replays the whole chat as updates first; the chat already shows it.
    replaying: bool,
    prompt_sent: bool,
    text: String,
    thought: String,
    message_id: Value,
    tools: HashMap<String, Tool>,
    plans: u32,
    /// Open permission requests: our request id -> (JSON-RPC id, options).
    asks: HashMap<String, (Value, Vec<Value>)>,
    cancelled: bool,
    done: bool,
    size: Option<u64>,
    cost: Option<f64>,
    cost_before: Option<f64>,
    started: std::time::Instant,
}

impl Client {
    pub fn new(agent: &AcpAgent, spec: &TurnSpec, ok: &Checked) -> Client {
        // No flag for extra instructions, so they lead the first message of a chat, like Codex.
        let prompt = match spec.instructions.filter(|_| ok.session.is_none()) {
            Some(i) => format!("{i}\n\n{}", spec.prompt),
            None => spec.prompt.to_string(),
        };
        let cwd = spec.cwd.to_string_lossy().to_string();
        let cwd_real = spec.cwd.canonicalize().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| cwd.clone());
        let mut c = Client {
            name: agent.name,
            login: agent.login,
            prompt,
            cwd,
            cwd_real,
            resume: ok.session.map(str::to_string),
            // "default" in the picker means: whatever the agent is set to.
            model: ok.model.filter(|m| *m != "default").map(str::to_string),
            plan: spec.permission_mode == "plan",
            auto_allow: spec.permission_mode == "bypassPermissions",
            auto_edits: spec.permission_mode == "acceptEdits",
            guard: crate::agent::active_guard(spec).cloned(),
            guard_edits: guards_edits(agent, spec),
            ask: agent.ask_option.filter(|_| spec.permission_mode == "ask"),
            ask_missing: spec.permission_mode == "ask" && !agent.asks(),
            auth_method: agent.auth_method,
            authed: false,
            next_id: 0,
            out: vec![],
            waiting: HashMap::new(),
            config: VecDeque::new(),
            caps: Value::Null,
            auth_methods: Value::Null,
            session: None,
            replaying: false,
            prompt_sent: false,
            text: String::new(),
            thought: String::new(),
            message_id: Value::Null,
            tools: HashMap::new(),
            plans: 0,
            asks: HashMap::new(),
            cancelled: false,
            done: false,
            size: None,
            cost: None,
            cost_before: None,
            started: std::time::Instant::now(),
        };
        c.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                "clientInfo": {"name": "nekotrail", "version": env!("CARGO_PKG_VERSION")},
            }),
            Step::Initialize,
        );
        c
    }

    fn send(&mut self, msg: Value) {
        self.out.push(msg.to_string() + "\n");
    }

    fn request(&mut self, method: &str, params: Value, step: Step) {
        self.next_id += 1;
        let id = self.next_id;
        self.waiting.insert(id, step);
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    }

    fn reply(&mut self, id: &Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn reply_error(&mut self, id: &Value, code: i64, message: &str) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}));
    }

    /// The turn can't go on: say why and end it.
    fn fail(&mut self, text: String, ev: &mut Vec<Value>) {
        self.flush(ev);
        ev.push(json!({"kind": "error", "text": text}));
        ev.push(json!({"kind": "done", "is_error": true}));
        self.done = true;
    }

    /// A JSON-RPC error, in words the chat can show (and `classify_error` can read).
    fn error_text(&self, e: &Value) -> String {
        let msg = e["message"].as_str().unwrap_or("unknown error");
        if e["code"].as_i64() == Some(AUTH_REQUIRED) || msg.to_lowercase().contains("auth") && msg.to_lowercase().contains("required") {
            return format!("{} is not logged in. Run `{}` in a terminal, then send again.", self.name, self.login);
        }
        // Internal errors carry the real reason in `data`, or after "Internal error: ".
        let detail = e["data"]["message"].as_str().or(e["data"].as_str());
        match detail {
            Some(d) if !msg.contains(d) => format!("{}: {msg} ({d})", self.name),
            _ => format!("{}: {}", self.name, msg.strip_prefix("Internal error: ").unwrap_or(msg)),
        }
    }

    fn flush(&mut self, ev: &mut Vec<Value>) {
        let t = std::mem::take(&mut self.thought);
        if !t.trim().is_empty() {
            ev.push(json!({"kind": "thinking", "text": t}));
        }
        let t = std::mem::take(&mut self.text);
        if !t.trim().is_empty() {
            ev.push(json!({"kind": "text", "text": t}));
        }
    }

    fn open(&mut self, ev: &mut Vec<Value>) {
        if let Some(id) = self.resume.clone() {
            let params = json!({"sessionId": id, "cwd": self.cwd, "mcpServers": []});
            if !self.caps["sessionCapabilities"]["resume"].is_null() {
                return self.request("session/resume", params, Step::Continue);
            }
            if self.caps["loadSession"] == true {
                self.replaying = true;
                return self.request("session/load", params, Step::Continue);
            }
            ev.push(json!({"kind": "note", "text": format!("{} can't pick up an earlier conversation, so this message starts a new one.", self.name)}));
        }
        self.new_session();
    }

    /// After "auth required": sign in once with the agent's saved-login method, if it has one and
    /// offered it, then open the chat again. False when there is nothing to try.
    fn authenticate(&mut self) -> bool {
        let Some(method) = self.auth_method.filter(|_| !self.authed) else { return false };
        if !self.auth_methods.as_array().is_some_and(|all| all.iter().any(|m| m["id"] == method)) {
            return false;
        }
        self.authed = true;
        self.request("authenticate", json!({"methodId": method}), Step::Auth);
        true
    }

    fn new_session(&mut self) {
        self.resume = None;
        let params = json!({"cwd": self.cwd, "mcpServers": []});
        self.request("session/new", params, Step::New);
    }

    /// Model and mode for this turn, from what `session/new` (or resume) said the agent offers:
    /// `configOptions` (current ACP), or the older `models` / `modes` objects.
    fn configure(&mut self, r: &Value, ev: &mut Vec<Value>) {
        let sid = self.session.clone().unwrap_or_default();
        let opts: Vec<Value> = r["configOptions"].as_array().cloned().unwrap_or_default();
        let find = |cat: &str| opts.iter().find(|o| o["category"] == cat || o["id"] == cat).cloned();
        if let Some(m) = self.model.clone() {
            if let Some(o) = find("model") {
                if o["currentValue"] != m.as_str() {
                    self.config.push_back(("session/set_config_option", json!({"sessionId": sid, "configId": o["id"], "value": m})));
                }
            } else if r["models"].is_object() {
                if r["models"]["currentModelId"] != m.as_str() {
                    self.config.push_back(("session/set_model", json!({"sessionId": sid, "modelId": m})));
                }
            } else {
                ev.push(json!({"kind": "note", "text": format!("{} doesn't let nekotrail pick its model, so it uses its own default.", self.name)}));
            }
        }
        // The mode option's id (None: the older `modes` object), the current mode, every mode.
        let modes: Option<(Option<Value>, String, Vec<String>)> = if let Some(o) = find("mode") {
            let all = o["options"].as_array().into_iter().flatten().filter_map(|v| v["value"].as_str().map(str::to_string)).collect();
            Some((Some(o["id"].clone()), o["currentValue"].as_str().unwrap_or("").to_string(), all))
        } else if r["modes"].is_object() {
            let all = r["modes"]["availableModes"].as_array().into_iter().flatten().filter_map(|v| v["id"].as_str().map(str::to_string)).collect();
            Some((None, r["modes"]["currentModeId"].as_str().unwrap_or("").to_string(), all))
        } else {
            None
        };
        let plan_mode = modes.as_ref().and_then(|(_, _, all)| all.iter().find(|m| is_plan(m)).cloned());
        if self.plan && plan_mode.is_none() {
            return self.fail(format!("{} has no plan mode, so it can't promise to only read. Pick another mode for this chat.", self.name), ev);
        }
        if self.ask_missing {
            return self.fail(format!("{} can't promise to ask before every change. Pick another mode for this chat.", self.name), ev);
        }
        if let Some((id, value)) = self.ask {
            match find(id) {
                Some(o) if o["options"].as_array().is_some_and(|all| all.iter().any(|v| v["value"] == value)) => {
                    if o["currentValue"] != value {
                        self.config.push_back(("session/set_config_option", json!({"sessionId": sid, "configId": o["id"], "value": value})));
                    }
                }
                _ => return self.fail(format!("{} didn't offer its \"ask first\" setting this time, so it can't promise to ask before every change. Pick another mode for this chat.", self.name), ev),
            }
        }
        if let Some((option, current, all)) = modes {
            // Plan mode stays with the agent's chat, so a later turn in another mode switches back,
            // to a mode that still asks (never one that approves everything by itself).
            let want = if self.plan {
                plan_mode
            } else if is_plan(&current) {
                all.iter().find(|m| !is_plan(m) && !runs_unattended(m)).or_else(|| all.iter().find(|m| !is_plan(m))).cloned()
            } else {
                None
            };
            if let Some(w) = want.filter(|w| *w != current) {
                self.config.push_back(match option {
                    Some(id) => ("session/set_config_option", json!({"sessionId": sid, "configId": id, "value": w})),
                    None => ("session/set_mode", json!({"sessionId": sid, "modeId": w})),
                });
            }
        }
        self.next_config();
    }

    fn next_config(&mut self) {
        match self.config.pop_front() {
            Some((method, params)) => self.request(method, params, Step::Config),
            None => {
                self.prompt_sent = true;
                // Cost is reported for the whole chat; this turn's share is what it grows by.
                self.cost_before = self.cost.or(if self.resume.is_none() { Some(0.0) } else { None });
                let params = json!({"sessionId": self.session, "prompt": [{"type": "text", "text": self.prompt}]});
                self.request("session/prompt", params, Step::Prompt);
            }
        }
    }

    fn on_response(&mut self, step: Step, j: &Value, ev: &mut Vec<Value>) {
        let err = &j["error"];
        let r = &j["result"];
        match step {
            Step::Initialize => {
                if !err.is_null() {
                    return self.fail(self.error_text(err), ev);
                }
                let v = r["protocolVersion"].as_u64().unwrap_or(0);
                if v != PROTOCOL_VERSION {
                    return self.fail(format!("{} speaks ACP version {v}, and nekotrail speaks version {PROTOCOL_VERSION}. Update both to the latest version.", self.name), ev);
                }
                self.caps = r["agentCapabilities"].clone();
                self.auth_methods = r["authMethods"].clone();
                self.open(ev);
            }
            Step::Continue => {
                self.replaying = false;
                if !err.is_null() {
                    if err["code"].as_i64() == Some(AUTH_REQUIRED) {
                        if self.authenticate() {
                            return;
                        }
                        return self.fail(self.error_text(err), ev);
                    }
                    ev.push(json!({"kind": "note", "text": format!("Couldn't pick up the earlier {} conversation ({}), so this message starts a new one.", self.name, err["message"].as_str().unwrap_or("unknown error"))}));
                    return self.new_session();
                }
                self.session = self.resume.clone();
                self.configure(r, ev);
            }
            Step::New => {
                if !err.is_null() {
                    if err["code"].as_i64() == Some(AUTH_REQUIRED) && self.authenticate() {
                        return;
                    }
                    return self.fail(self.error_text(err), ev);
                }
                let Some(id) = r["sessionId"].as_str().filter(|s| !s.is_empty()) else {
                    return self.fail(format!("{} didn't start a chat.", self.name), ev);
                };
                self.session = Some(id.to_string());
                ev.push(json!({"kind": "session", "agent_session_id": id}));
                self.configure(r, ev);
            }
            Step::Auth => {
                if !err.is_null() {
                    // The saved login didn't work either: the user has to sign in again.
                    let mut e = err.clone();
                    e["code"] = json!(AUTH_REQUIRED);
                    return self.fail(self.error_text(&e), ev);
                }
                self.open(ev);
            }
            Step::Config => {
                if !err.is_null() {
                    return self.fail(format!("{} didn't take the model or mode for this chat: {}", self.name, err["message"].as_str().unwrap_or("unknown error")), ev);
                }
                self.next_config();
            }
            Step::Prompt => {
                self.flush(ev);
                let mut done = json!({"kind": "done", "is_error": false, "duration_ms": self.started.elapsed().as_millis() as u64});
                if let (Some(now), Some(before)) = (self.cost, self.cost_before) {
                    done["cost_usd"] = json!((now - before).max(0.0));
                }
                if let Some(s) = self.size {
                    done["context_window"] = json!(s);
                }
                let reason = r["stopReason"].as_str().unwrap_or("");
                if self.cancelled || reason == "cancelled" {
                    done["is_error"] = json!(true);
                    done["stopped"] = json!(true);
                } else if !err.is_null() {
                    ev.push(json!({"kind": "error", "text": self.error_text(err)}));
                    done["is_error"] = json!(true);
                } else {
                    let why = match reason {
                        "max_tokens" => Some(format!("{} hit its output limit and stopped.", self.name)),
                        "max_turn_requests" => Some(format!("{} reached its step limit for one message and stopped. Send \"go on\" to continue.", self.name)),
                        "refusal" => Some(format!("{} declined to go on with this.", self.name)),
                        _ => None,
                    };
                    if let Some(w) = why {
                        ev.push(json!({"kind": "error", "text": w}));
                        done["is_error"] = json!(true);
                    }
                }
                ev.push(done);
                self.done = true;
            }
        }
    }

    fn on_request(&mut self, id: &Value, method: &str, params: &Value, ev: &mut Vec<Value>) {
        if method != "session/request_permission" {
            // No file system, terminal or anything else was offered (see the module docs).
            return self.reply_error(id, METHOD_NOT_FOUND, &format!("nekotrail does not offer {method}"));
        }
        self.flush(ev);
        let tc = &params["toolCall"];
        let tid = tc["toolCallId"].as_str().unwrap_or("").to_string();
        self.merge(&tid, tc, false);
        self.show(&tid, true, ev);
        let options: Vec<Value> = params["options"].as_array().cloned().unwrap_or_default();
        if self.cancelled {
            return self.reply(id, json!({"outcome": {"outcome": "cancelled"}}));
        }
        // First, before Full auto or "Auto-accept edits" could allow it.
        let write = self.tools.get(&tid).filter(|t| self.is_write(t)).map(|t| self.write_paths(t));
        if let Some(paths) = write {
            let hit: Vec<String> = match &self.guard {
                Some(g) => paths.into_iter().filter(|p| g.protects(p)).collect(),
                None => vec![],
            };
            if !hit.is_empty() {
                ev.push(json!({"kind": "blocked", "files": hit, "text": format!("{} tried to change a protected file, so nekotrail said no.", self.name)}));
                let outcome = match pick(&options, &["reject_once", "reject_always"]) {
                    Some(o) => json!({"outcome": "selected", "optionId": o}),
                    None => json!({"outcome": "cancelled"}),
                };
                return self.reply(id, json!({"outcome": outcome}));
            }
            if self.guard_edits
                && let Some(o) = pick(&options, &["allow_once", "allow_always"]) {
                    return self.reply(id, json!({"outcome": {"outcome": "selected", "optionId": o}}));
                }
        }
        if self.auto_allow
            && let Some(o) = pick(&options, &["allow_once", "allow_always"]) {
                return self.reply(id, json!({"outcome": {"outcome": "selected", "optionId": o}}));
            }
        if self.auto_edits
            && self.tools.get(&tid).is_some_and(|t| self.in_worktree_edit(t))
            && let Some(o) = pick(&options, &["allow_once"]) {
                return self.reply(id, json!({"outcome": {"outcome": "selected", "optionId": o}}));
            }
        let rid = approval_id();
        let (tool, input) = self.tools.get(&tid).map(|t| self.view(t)).unwrap_or_else(|| ("tool".into(), String::new()));
        let title = tc["title"].as_str().map(|t| self.short(t));
        let can_always = options.iter().any(|o| o["kind"] == "allow_always");
        self.asks.insert(rid.clone(), (id.clone(), options));
        ev.push(json!({"kind": "approval", "request_id": rid, "tool": tool, "input": input, "description": title, "can_always": can_always}));
    }

    /// Fold a `tool_call` or `tool_call_update` into what we know about that call.
    fn merge(&mut self, id: &str, u: &Value, fresh: bool) {
        let t = self.tools.entry(id.to_string()).or_default();
        let s = |k: &str| u[k].as_str().filter(|v| !v.is_empty()).map(str::to_string);
        if let Some(n) = s("name") {
            t.hint = n;
        }
        if let Some(title) = s("title") {
            if fresh && t.hint.is_empty() {
                t.hint = title.clone();
            }
            t.title = title;
        }
        if let Some(k) = s("kind") {
            t.kind = k;
        }
        if let Some(st) = s("status") {
            t.status = st;
        }
        if u["rawInput"].as_object().is_some_and(|o| !o.is_empty()) || u["rawInput"].as_str().is_some_and(|v| !v.is_empty()) {
            t.input = u["rawInput"].clone();
        }
        if u["locations"].as_array().is_some_and(|l| !l.is_empty()) {
            t.locations = u["locations"].clone();
        }
        if u["content"].as_array().is_some_and(|c| !c.is_empty()) {
            t.content = u["content"].clone();
        }
        if !u["rawOutput"].is_null() {
            t.output = u["rawOutput"].clone();
        }
    }

    /// Send the call to the chat once it runs (or `force`, when it asks for permission), then its result.
    fn show(&mut self, id: &str, force: bool, ev: &mut Vec<Value>) {
        let Some(t) = self.tools.get(id) else { return };
        // Not on input alone: OpenCode's first `pending` call carries only `{"cwd": ...}`.
        let ready = force || matches!(t.status.as_str(), "in_progress" | "completed" | "failed");
        let finished = matches!(t.status.as_str(), "completed" | "failed");
        let (name, input) = self.view(t);
        let (shown, done) = (t.shown, t.done);
        if !shown && (ready || finished) {
            ev.push(json!({"kind": "tool", "id": id, "name": name, "input": input}));
        }
        let output = finished.then(|| self.output(&self.tools[id]));
        let t = self.tools.get_mut(id).unwrap();
        t.shown |= ready || finished;
        if let Some(out) = output.filter(|_| !done) {
            t.done = true;
            ev.push(json!({"kind": "tool_result", "id": id, "output": clip(&out), "is_error": t.status == "failed"}));
        }
    }

    /// A read or an edit ("Auto-accept edits" lets it through) whose every file is in the worktree.
    /// No locations means we can't tell where it writes, so it asks.
    fn in_worktree_edit(&self, t: &Tool) -> bool {
        let Some(locs) = t.locations.as_array().filter(|l| !l.is_empty()) else { return false };
        EDIT_KINDS.contains(&t.kind.as_str())
            && locs.iter().all(|l| {
                let p = l["path"].as_str().unwrap_or("");
                // A relative path, or one with `..` in it, could point anywhere.
                !p.split('/').any(|c| c == "..") && [&self.cwd_real, &self.cwd].iter().any(|root| p.strip_prefix(root.as_str()).is_some_and(|r| r.starts_with('/')))
            })
    }

    /// A call that changes files: by its kind, or by the name the chat gives it.
    fn is_write(&self, t: &Tool) -> bool {
        matches!(t.kind.as_str(), "edit" | "delete" | "move") || matches!(self.view(t).0.as_str(), "Write" | "Edit" | "Delete" | "Move")
    }

    /// Every file a writing call names, relative to the worktree: its input, its locations, the
    /// diffs it carries, and the files in an `apply_patch` body. Paths outside the worktree are
    /// left out (nothing there is protected).
    fn write_paths(&self, t: &Tool) -> Vec<String> {
        let mut raw: Vec<String> = vec![];
        for k in ["filePath", "filepath", "file_path", "path", "newPath", "new_path", "destination"] {
            if let Some(p) = t.input[k].as_str() {
                raw.push(p.to_string());
            }
        }
        for l in t.locations.as_array().into_iter().flatten() {
            if let Some(p) = l["path"].as_str() {
                raw.push(p.to_string());
            }
        }
        for c in t.content.as_array().into_iter().flatten() {
            if let Some(p) = c["path"].as_str() {
                raw.push(p.to_string());
            }
        }
        for k in ["patchText", "patch", "input"] {
            for line in t.input[k].as_str().unwrap_or("").lines() {
                for head in ["*** Add File: ", "*** Update File: ", "*** Delete File: ", "*** Move to: "] {
                    if let Some(p) = line.strip_prefix(head) {
                        raw.push(p.trim().to_string());
                    }
                }
            }
        }
        let mut out: Vec<String> = vec![];
        for p in raw {
            let rel = self.short(&p);
            let rel = rel.trim_start_matches("./").to_string();
            if rel.is_empty() || rel == "." || rel.starts_with('/') || rel.split('/').any(|s| s == "..") {
                continue;
            }
            if !out.contains(&rel) {
                out.push(rel);
            }
        }
        out
    }

    /// A path inside the worktree, relative to it.
    fn short(&self, p: &str) -> String {
        for root in [&self.cwd_real, &self.cwd] {
            if let Some(rest) = p.strip_prefix(root.as_str()).and_then(|r| r.strip_prefix('/')) {
                return rest.to_string();
            }
            if p == root.as_str() {
                return ".".into();
            }
        }
        p.to_string()
    }

    /// The tool under a name the chat already draws, and its one-line summary.
    fn view(&self, t: &Tool) -> (String, String) {
        let hint = t.hint.to_lowercase();
        let by_name = match hint.as_str() {
            "bash" | "shell" | "run_shell_command" | "terminal" => Some("Bash"),
            "read" | "read_file" | "view" => Some("Read"),
            "write" | "write_file" | "create" => Some("Write"),
            "edit" | "multiedit" | "patch" | "apply_patch" | "replace" | "str_replace" => Some("Edit"),
            "grep" | "grep_search" => Some("Grep"),
            "glob" | "find" => Some("Glob"),
            "list" | "ls" | "list_directory" => Some("List"),
            "webfetch" | "web_fetch" | "fetch" => Some("WebFetch"),
            "websearch" | "web_search" => Some("WebSearch"),
            "todowrite" | "todo_write" | "write_todos" => Some("TodoWrite"),
            "task" => Some("Task"),
            _ => None,
        };
        let by_kind = match t.kind.as_str() {
            "execute" => Some("Bash"),
            "read" => Some("Read"),
            "edit" => Some("Edit"),
            "delete" => Some("Delete"),
            "move" => Some("Move"),
            "search" => Some("Grep"),
            "fetch" => Some("WebFetch"),
            _ => None,
        };
        let todos = t.input["todos"].as_array();
        let name = if todos.is_some() { "TodoWrite".to_string() } else {
            by_name.or(by_kind).map(str::to_string).unwrap_or_else(|| if t.hint.is_empty() { "tool".into() } else { t.hint.clone() })
        };
        if let Some(todos) = todos {
            let list: Vec<Value> = todos.iter().map(|x| json!({"content": x["content"].as_str().or(x["description"].as_str()).unwrap_or(""), "status": x["status"]})).collect();
            return (name, Value::Array(list).to_string());
        }
        let pick = |keys: &[&str]| keys.iter().find_map(|k| t.input[*k].as_str().filter(|v| !v.is_empty()).map(str::to_string));
        let summary = if name == "Bash" {
            pick(&["command", "cmd"])
        } else {
            pick(&["filePath", "filepath", "file_path", "path", "pattern", "query", "url", "description"])
                .or_else(|| t.locations[0]["path"].as_str().map(str::to_string))
        };
        let summary = summary
            .or_else(|| (!t.title.is_empty() && t.title.to_lowercase() != hint).then(|| t.title.clone()))
            .or_else(|| (!t.input.is_null()).then(|| t.input.to_string()))
            .unwrap_or_default();
        (name, clip(&self.short(&summary)))
    }

    /// What a finished call printed: its text content, else the raw output.
    fn output(&self, t: &Tool) -> String {
        let mut parts = vec![];
        for c in t.content.as_array().into_iter().flatten() {
            match c["type"].as_str() {
                Some("content") => {
                    if let Some(s) = c["content"]["text"].as_str() {
                        parts.push(s.to_string());
                    }
                }
                Some("diff") => {
                    let p = self.short(c["path"].as_str().unwrap_or(""));
                    let new = c["oldText"].as_str().is_none_or(str::is_empty);
                    parts.push(format!("{} {p}", if new { "Created" } else { "Changed" }));
                }
                _ => {}
            }
        }
        if parts.is_empty() {
            let o = &t.output;
            if let Some(s) = o["output"].as_str().or(o.as_str()).or(o["error"].as_str()).or(o["error"]["message"].as_str()) {
                parts.push(s.to_string());
            }
        }
        parts.join("\n")
    }

    fn on_update(&mut self, params: &Value, ev: &mut Vec<Value>) {
        if self.replaying || self.session.as_deref().is_some_and(|s| params["sessionId"] != s) {
            return;
        }
        let u = &params["update"];
        match u["sessionUpdate"].as_str().unwrap_or("") {
            "agent_message_chunk" => {
                if !self.thought.is_empty() || (u["messageId"] != self.message_id && !self.text.is_empty()) {
                    self.flush(ev);
                }
                self.message_id = u["messageId"].clone();
                if let Some(t) = u["content"]["text"].as_str() {
                    self.text.push_str(t);
                }
            }
            "agent_thought_chunk" => {
                if !self.text.is_empty() {
                    self.flush(ev);
                }
                if let Some(t) = u["content"]["text"].as_str() {
                    self.thought.push_str(t);
                }
            }
            kind @ ("tool_call" | "tool_call_update") => {
                self.flush(ev);
                let id = u["toolCallId"].as_str().unwrap_or("").to_string();
                self.merge(&id, u, kind == "tool_call");
                self.show(&id, false, ev);
            }
            "plan" => {
                self.flush(ev);
                // The whole plan every time: the chat's todo list reads the latest one.
                self.plans += 1;
                let id = format!("plan-{}", self.plans);
                let list: Vec<Value> = u["entries"].as_array().into_iter().flatten().map(|e| json!({"content": e["content"], "status": e["status"]})).collect();
                ev.push(json!({"kind": "tool", "id": id, "name": "TodoWrite", "input": Value::Array(list).to_string()}));
                ev.push(json!({"kind": "tool_result", "id": id, "output": "", "is_error": false}));
            }
            "usage_update" => {
                self.size = u["size"].as_u64().or(self.size);
                if u["cost"]["currency"].as_str().is_none_or(|c| c == "USD") {
                    self.cost = u["cost"]["amount"].as_f64().or(self.cost);
                }
                if let Some(used) = u["used"].as_u64() {
                    ev.push(json!({"kind": "usage", "context_tokens": used}));
                }
            }
            // Echoes of the user's own message, slash commands, mode and title changes.
            _ => {}
        }
    }
}

/// The first option of the first kind that has one.
fn pick(options: &[Value], kinds: &[&str]) -> Option<Value> {
    kinds.iter().find_map(|k| options.iter().find(|o| o["kind"] == *k).map(|o| o["optionId"].clone()))
}

impl Parser for Client {
    fn line(&mut self, line: &str) -> Vec<Value> {
        let Ok(j) = serde_json::from_str::<Value>(line) else { return vec![] };
        let mut ev = vec![];
        let method = j["method"].as_str().map(str::to_string);
        match (method, j.get("id")) {
            (Some(m), Some(id)) => {
                let id = id.clone();
                self.on_request(&id, &m, &j["params"], &mut ev);
            }
            (Some(m), None) => {
                if m == "session/update" {
                    self.on_update(&j["params"], &mut ev);
                }
            }
            (None, Some(id)) => {
                if let Some(step) = id.as_u64().and_then(|n| self.waiting.remove(&n))
                    && !self.done {
                        self.on_response(step, &j, &mut ev);
                    }
            }
            (None, None) => {}
        }
        ev
    }

    fn finish(&mut self) -> Vec<Value> {
        let mut ev = vec![];
        self.flush(&mut ev);
        if !self.done && self.cancelled {
            ev.push(json!({"kind": "done", "is_error": true, "stopped": true}));
            self.done = true;
        }
        ev
    }

    fn outgoing(&mut self) -> Vec<String> {
        std::mem::take(&mut self.out)
    }

    fn answer(&mut self, request_id: &str, allow: bool, always: bool) -> Option<Result<String, String>> {
        let Some((id, options)) = self.asks.remove(request_id) else {
            return Some(Err("This request was already answered, or the turn ended.".into()));
        };
        let kinds: &[&str] = match (allow, always) {
            (true, true) => &["allow_always", "allow_once"],
            // Never stretch a one-time OK into "always".
            (true, false) => &["allow_once"],
            (false, _) => &["reject_once", "reject_always"],
        };
        let outcome = match pick(&options, kinds) {
            Some(o) => json!({"outcome": "selected", "optionId": o}),
            None if allow => {
                self.asks.insert(request_id.to_string(), (id, options));
                return Some(Err(format!("{} only offers to allow this for the whole chat. Choose Allow for this chat, or Deny.", self.name)));
            }
            // No way to say no: "cancelled" is never an OK.
            None => json!({"outcome": "cancelled"}),
        };
        Some(Ok(json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": outcome}}).to_string() + "\n"))
    }

    fn cancel(&mut self) -> Option<Vec<String>> {
        self.cancelled = true;
        if self.done {
            return Some(vec![]);
        }
        if !self.prompt_sent {
            // Nothing has happened yet: just end the process.
            return None;
        }
        let mut lines = vec![json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": self.session}}).to_string() + "\n"];
        // The spec: every open permission request is answered "cancelled".
        for (_, (id, _)) in self.asks.drain() {
            lines.push(json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": {"outcome": "cancelled"}}}).to_string() + "\n");
        }
        Some(lines)
    }

    fn finished(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    static FAKE: AcpAgent = AcpAgent { id: "fake", name: "Fake", bin: "fake-acp", login: "fake login", ask_env: &[("FAKE_ASK", "1")], ..AcpAgent::BASE };

    fn spec<'a>(cwd: &'a Path, mode: &'a str, session: Option<&'a str>, model: Option<&'a str>) -> TurnSpec<'a> {
        TurnSpec { fork: false, agent: "fake", prompt: "hi", agent_session_id: session, model, permission_mode: mode, cwd, effort: None, instructions: Some("Use pnpm."), env: &[], dirs: &[], project_config: true, guard: None }
    }

    fn client(cwd: &Path, mode: &str, session: Option<&str>, model: Option<&str>) -> Client {
        let s = spec(cwd, mode, session, model);
        Client::new(&FAKE, &s, &Checked { model, effort: None, session })
    }

    fn sent(c: &mut Client) -> Vec<Value> {
        c.outgoing().iter().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    fn kinds(evs: &[Value]) -> Vec<&str> {
        evs.iter().map(|e| e["kind"].as_str().unwrap()).collect()
    }

    fn feed(c: &mut Client, msgs: &[Value]) -> Vec<Value> {
        msgs.iter().flat_map(|m| c.line(&m.to_string())).collect()
    }

    fn update(u: Value) -> Value {
        json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": u}})
    }

    const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"resume":{}}}}}"#;

    /// Up to the prompt: initialize, session/new with OpenCode's real config options.
    fn started(cwd: &Path, mode: &str, model: Option<&str>) -> (Client, Vec<Value>) {
        let mut c = client(cwd, mode, None, model);
        sent(&mut c);
        let mut ev = c.line(INIT);
        sent(&mut c);
        ev.extend(c.line(&json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "s1", "configOptions": [
            {"id": "model", "category": "model", "type": "select", "currentValue": "opencode/big-pickle", "options": [{"value": "opencode/big-pickle"}]},
            {"id": "mode", "category": "mode", "type": "select", "currentValue": "build", "options": [{"value": "build"}, {"value": "plan"}]}]}}).to_string()));
        (c, ev)
    }

    #[test]
    fn opens_a_chat_and_sends_the_prompt() {
        let cwd = Path::new("/w/ws");
        let mut c = client(cwd, "acceptEdits", None, None);
        let init = sent(&mut c);
        assert_eq!(init[0]["method"], "initialize");
        assert_eq!(init[0]["params"]["protocolVersion"], 1);
        assert_eq!(init[0]["params"]["clientCapabilities"]["fs"]["writeTextFile"], false);
        assert_eq!(init[0]["params"]["clientCapabilities"]["terminal"], false);
        assert!(c.line(INIT).is_empty());
        let new = sent(&mut c);
        assert_eq!(new[0]["method"], "session/new");
        assert_eq!(new[0]["params"]["cwd"], "/w/ws");
        let (mut c, ev) = started(cwd, "acceptEdits", None);
        assert_eq!(kinds(&ev), ["session"]);
        assert_eq!(ev[0]["agent_session_id"], "s1");
        let p = sent(&mut c);
        assert_eq!((p.len(), p[0]["method"].as_str()), (1, Some("session/prompt")), "build mode and the default model need no change");
        assert_eq!(p[0]["params"]["prompt"][0]["text"], "Use pnpm.\n\nhi", "instructions lead a chat's first message");
        assert_eq!(p[0]["params"]["sessionId"], "s1");
    }

    #[test]
    fn model_and_plan_mode_go_through_config_options() {
        let (mut c, _) = started(Path::new("/w"), "plan", Some("anthropic/claude-sonnet-4-5"));
        let m = sent(&mut c);
        assert_eq!(m[0]["method"], "session/set_config_option");
        assert_eq!((m[0]["params"]["configId"].as_str(), m[0]["params"]["value"].as_str()), (Some("model"), Some("anthropic/claude-sonnet-4-5")));
        c.line(&json!({"jsonrpc": "2.0", "id": m[0]["id"], "result": {}}).to_string());
        let m = sent(&mut c);
        assert_eq!((m[0]["params"]["configId"].as_str(), m[0]["params"]["value"].as_str()), (Some("mode"), Some("plan")));
        c.line(&json!({"jsonrpc": "2.0", "id": m[0]["id"], "result": {}}).to_string());
        assert_eq!(sent(&mut c)[0]["method"], "session/prompt");
        // "default" means the agent's own choice.
        let (mut c, _) = started(Path::new("/w"), "acceptEdits", Some("default"));
        assert_eq!(sent(&mut c)[0]["method"], "session/prompt");
        // A model it refuses ends the turn with the reason, not a silent switch.
        let (mut c, _) = started(Path::new("/w"), "acceptEdits", Some("nope/x"));
        let m = sent(&mut c);
        let ev = c.line(&json!({"jsonrpc": "2.0", "id": m[0]["id"], "error": {"code": -32602, "message": "Unknown model"}}).to_string());
        assert_eq!(kinds(&ev), ["error", "done"]);
        assert!(ev[0]["text"].as_str().unwrap().contains("Unknown model"));
        assert!(c.finished());
    }

    #[test]
    fn no_plan_mode_means_no_turn() {
        let mut c = client(Path::new("/w"), "plan", None, None);
        c.line(INIT);
        let ev = c.line(r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s1"}}"#);
        assert_eq!(kinds(&ev), ["session", "error", "done"]);
        assert!(ev[1]["text"].as_str().unwrap().contains("no plan mode"));
        assert!(!sent(&mut c).iter().any(|m| m["method"] == "session/prompt"));
        // Older agents list modes under `modes`; leaving plan switches back.
        let mut c = client(Path::new("/w"), "acceptEdits", None, None);
        c.line(INIT);
        c.line(r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s1","modes":{"currentModeId":"plan","availableModes":[{"id":"plan"},{"id":"code"}]}}}"#);
        let m = sent(&mut c);
        assert_eq!((m.last().unwrap()["method"].as_str(), m.last().unwrap()["params"]["modeId"].as_str()), (Some("session/set_mode"), Some("code")));
    }

    #[test]
    fn continues_a_chat_and_falls_back_to_a_new_one() {
        let mut c = client(Path::new("/w"), "acceptEdits", Some("s0"), None);
        sent(&mut c);
        c.line(INIT);
        let r = sent(&mut c);
        assert_eq!((r[0]["method"].as_str(), r[0]["params"]["sessionId"].as_str()), (Some("session/resume"), Some("s0")));
        let ev = c.line(r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32002,"message":"Session not found"}}"#);
        assert_eq!(kinds(&ev), ["note"]);
        assert_eq!(sent(&mut c)[0]["method"], "session/new");
        // Resumed fine: no new session event, and the prompt has no instructions.
        let mut c = client(Path::new("/w"), "acceptEdits", Some("s1"), None);
        c.line(INIT);
        assert!(c.line(r#"{"jsonrpc":"2.0","id":2,"result":{}}"#).is_empty());
        let p = sent(&mut c);
        assert_eq!(p.last().unwrap()["params"]["prompt"][0]["text"], "hi");
        // Load only: the replayed history is not shown twice.
        let mut c = client(Path::new("/w"), "acceptEdits", Some("s1"), None);
        c.line(r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}}"#);
        assert_eq!(sent(&mut c).last().unwrap()["method"], "session/load");
        assert!(c.line(&update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "old"}})).to_string()).is_empty());
        c.line(r#"{"jsonrpc":"2.0","id":2,"result":{}}"#);
        assert!(c.finish().is_empty(), "replayed text is dropped");
        // Neither: a note, then a new chat.
        let mut c = client(Path::new("/w"), "acceptEdits", Some("s1"), None);
        let ev = c.line(r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{}}}"#);
        assert_eq!(kinds(&ev), ["note"]);
        assert_eq!(sent(&mut c).last().unwrap()["method"], "session/new");
    }

    #[test]
    fn streams_text_tools_and_todos_like_opencode() {
        let cwd = Path::new("/w/ws");
        let (mut c, _) = started(cwd, "acceptEdits", None);
        sent(&mut c);
        // Shapes from a real `opencode acp` 1.19.5 turn.
        let evs = feed(&mut c, &[
            update(json!({"sessionUpdate": "agent_thought_chunk", "messageId": "p1", "content": {"type": "text", "text": "Let me "}})),
            update(json!({"sessionUpdate": "agent_thought_chunk", "messageId": "p1", "content": {"type": "text", "text": "look."}})),
            update(json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1", "content": {"type": "text", "text": "Reading "}})),
            update(json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1", "content": {"type": "text", "text": "it."}})),
            update(json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "bash", "kind": "execute", "status": "pending", "locations": [{"path": "/w/ws"}], "rawInput": {"cwd": "/w/ws"}})),
            update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "in_progress", "kind": "execute", "title": "cat a.txt", "rawInput": {"command": "cat a.txt", "cwd": "/w/ws"}})),
            update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed", "title": "cat a.txt", "content": [{"type": "content", "content": {"type": "text", "text": "hi\n"}}], "rawOutput": {"output": "hi\n"}})),
            update(json!({"sessionUpdate": "tool_call", "toolCallId": "t2", "title": "write", "kind": "edit", "status": "pending", "locations": [], "rawInput": {}})),
            update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t2", "status": "in_progress", "kind": "edit", "title": "write", "rawInput": {"filePath": "/w/ws/b.txt", "content": "bye\n"}})),
            update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t2", "status": "failed", "rawOutput": {"error": "disk full"}})),
            update(json!({"sessionUpdate": "tool_call", "toolCallId": "t3", "title": "todowrite", "kind": "other", "status": "pending", "rawInput": {}})),
            update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t3", "status": "in_progress", "rawInput": {"todos": [{"content": "Read a.txt", "status": "completed", "priority": "high"}]}})),
            update(json!({"sessionUpdate": "plan", "entries": [{"content": "Ship it", "priority": "high", "status": "in_progress"}]})),
            update(json!({"sessionUpdate": "usage_update", "used": 17589, "size": 200000, "cost": {"amount": 0.5, "currency": "USD"}})),
            update(json!({"sessionUpdate": "agent_message_chunk", "messageId": "m2", "content": {"type": "text", "text": "DONE"}})),
            update(json!({"sessionUpdate": "available_commands_update", "availableCommands": []})),
            json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "other", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "not ours"}}}}),
            json!({"jsonrpc": "2.0", "id": 3, "result": {"stopReason": "end_turn"}}),
        ]);
        assert_eq!(kinds(&evs), ["thinking", "text", "tool", "tool_result", "tool", "tool_result", "tool", "tool", "tool_result", "usage", "text", "done"]);
        assert_eq!((evs[0]["text"].as_str(), evs[1]["text"].as_str()), (Some("Let me look."), Some("Reading it.")));
        assert_eq!((evs[2]["name"].as_str(), evs[2]["input"].as_str(), evs[2]["id"].as_str()), (Some("Bash"), Some("cat a.txt"), Some("t1")), "waits for the real input");
        assert_eq!((evs[3]["output"].as_str(), evs[3]["is_error"].as_bool()), (Some("hi\n"), Some(false)));
        assert_eq!((evs[4]["name"].as_str(), evs[4]["input"].as_str()), (Some("Write"), Some("b.txt")), "paths are relative to the worktree");
        assert_eq!((evs[5]["output"].as_str(), evs[5]["is_error"].as_bool()), (Some("disk full"), Some(true)));
        assert_eq!(evs[6]["name"], "TodoWrite");
        let todos: Value = serde_json::from_str(evs[6]["input"].as_str().unwrap()).unwrap();
        assert_eq!(todos, json!([{"content": "Read a.txt", "status": "completed"}]));
        assert_eq!(evs[7]["name"], "TodoWrite");
        assert!(evs[7]["input"].as_str().unwrap().contains("Ship it"));
        assert_eq!(evs[9]["context_tokens"], 17589);
        assert_eq!(evs[10]["text"], "DONE");
        let done = &evs[11];
        assert_eq!((done["is_error"].as_bool(), done["cost_usd"].as_f64(), done["context_window"].as_u64()), (Some(false), Some(0.5), Some(200000)));
        assert!(done["stopped"].is_null());
        assert!(c.finished());
        assert!(c.finish().is_empty());
    }

    #[test]
    fn stop_reasons_and_errors() {
        for (reason, err) in [("end_turn", false), ("max_tokens", true), ("max_turn_requests", true), ("refusal", true)] {
            let (mut c, _) = started(Path::new("/w"), "acceptEdits", None);
            let ev = c.line(&json!({"jsonrpc": "2.0", "id": 3, "result": {"stopReason": reason}}).to_string());
            assert_eq!(ev.last().unwrap()["is_error"], err, "{reason}");
            assert_eq!(ev.len(), if err { 2 } else { 1 });
        }
        // OpenCode 1.17 on the free tier: the reason reaches the chat.
        let (mut c, _) = started(Path::new("/w"), "acceptEdits", None);
        let ev = c.line(r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32603,"message":"Internal error: OpenCode 1.18.0 or newer is required to use the free tier","data":{"service":"session"}}}"#);
        assert_eq!(kinds(&ev), ["error", "done"]);
        assert_eq!(ev[0]["text"], "Fake: OpenCode 1.18.0 or newer is required to use the free tier");
        // Not signed in: worded so the chat offers the sign-in fix.
        let mut c = client(Path::new("/w"), "acceptEdits", None, None);
        c.line(INIT);
        let ev = c.line(r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"Authentication required"}}"#);
        assert_eq!(ev[0]["text"], "Fake is not logged in. Run `fake login` in a terminal, then send again.");
        // A newer protocol than ours.
        let mut c = client(Path::new("/w"), "acceptEdits", None, None);
        let ev = c.line(r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":2}}"#);
        assert_eq!(kinds(&ev), ["error", "done"]);
        assert!(sent(&mut c).iter().all(|m| m["method"] == "initialize"));
        // Junk and log lines are ignored.
        assert!(c.line("Loaded config").is_empty());
    }

    #[test]
    fn permissions_map_onto_the_ask_card() {
        let opts = json!([{"optionId": "once", "kind": "allow_once", "name": "Allow once"}, {"optionId": "always", "kind": "allow_always", "name": "Always allow"}, {"optionId": "reject", "kind": "reject_once", "name": "Reject"}]);
        let ask = |id: u64| json!({"jsonrpc": "2.0", "id": id, "method": "session/request_permission", "params": {"sessionId": "s1", "toolCall": {"toolCallId": "t1", "title": "cat a.txt", "kind": "execute", "status": "pending", "rawInput": {"command": "cat a.txt"}}, "options": opts}});
        let (mut c, _) = started(Path::new("/w"), "ask", None);
        sent(&mut c);
        let ev = c.line(&ask(0).to_string());
        assert_eq!(kinds(&ev), ["tool", "approval"]);
        assert_eq!((ev[1]["tool"].as_str(), ev[1]["input"].as_str(), ev[1]["can_always"].as_bool()), (Some("Bash"), Some("cat a.txt"), Some(true)));
        let rid = ev[1]["request_id"].as_str().unwrap().to_string();
        let reply: Value = serde_json::from_str(&c.answer(&rid, true, true).unwrap().unwrap()).unwrap();
        assert_eq!((reply["id"].as_u64(), reply["result"]["outcome"]["optionId"].as_str()), (Some(0), Some("always")));
        assert!(c.answer(&rid, true, false).unwrap().is_err(), "answered once only");
        for (allow, always, want) in [(true, false, "once"), (false, false, "reject"), (false, true, "reject")] {
            let ev = c.line(&ask(7).to_string());
            let rid = ev.last().unwrap()["request_id"].as_str().unwrap().to_string();
            let reply: Value = serde_json::from_str(&c.answer(&rid, allow, always).unwrap().unwrap()).unwrap();
            assert_eq!(reply["result"]["outcome"]["optionId"], want);
        }
        // Full auto answers by itself; nothing waits for the user.
        let (mut c, _) = started(Path::new("/w"), "bypassPermissions", None);
        sent(&mut c);
        assert_eq!(kinds(&c.line(&ask(4).to_string())), ["tool"]);
        assert_eq!(sent(&mut c)[0]["result"]["outcome"]["optionId"], "once");
        // A one-time OK never becomes "always".
        let (mut c, _) = started(Path::new("/w"), "ask", None);
        let only_always = json!({"jsonrpc": "2.0", "id": 9, "method": "session/request_permission", "params": {"sessionId": "s1", "toolCall": {"toolCallId": "t9"}, "options": [{"optionId": "a", "kind": "allow_always", "name": "Always"}]}});
        let ev = c.line(&only_always.to_string());
        let rid = ev.last().unwrap()["request_id"].as_str().unwrap().to_string();
        assert!(c.answer(&rid, true, false).unwrap().is_err());
        let no: Value = serde_json::from_str(&c.answer(&rid, false, false).unwrap().unwrap()).unwrap();
        assert_eq!(no["result"]["outcome"]["outcome"], "cancelled", "no reject option: cancelled, never an OK");
        // Anything else the agent asks for is refused.
        c.line(r#"{"jsonrpc":"2.0","id":5,"method":"fs/write_text_file","params":{"sessionId":"s1","path":"/etc/x","content":"x"}}"#);
        let r = sent(&mut c);
        assert_eq!((r.last().unwrap()["id"].as_u64(), r.last().unwrap()["error"]["code"].as_i64()), (Some(5), Some(-32601)));
        // Claude-style parsers have no answers of their own.
        assert!(crate::agent::GeminiParser::default().answer("x", true, false).is_none());
    }

    #[test]
    fn cancel_answers_open_requests_and_reports_a_stop() {
        let (mut c, _) = started(Path::new("/w"), "ask", None);
        sent(&mut c);
        c.line(r#"{"jsonrpc":"2.0","id":0,"method":"session/request_permission","params":{"sessionId":"s1","toolCall":{"toolCallId":"t1","title":"rm -rf x","kind":"execute"},"options":[{"optionId":"once","kind":"allow_once","name":"Allow"}]}}"#);
        let lines: Vec<Value> = c.cancel().unwrap().iter().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines[0]["method"], "session/cancel");
        assert_eq!(lines[0]["params"]["sessionId"], "s1");
        assert!(lines[0].get("id").is_none(), "a notification");
        assert_eq!((lines[1]["id"].as_u64(), lines[1]["result"]["outcome"]["outcome"].as_str()), (Some(0), Some("cancelled")));
        // A request that arrives after Stop is turned down at once.
        c.line(r#"{"jsonrpc":"2.0","id":8,"method":"session/request_permission","params":{"sessionId":"s1","toolCall":{"toolCallId":"t2"},"options":[]}}"#);
        assert_eq!(sent(&mut c)[0]["result"]["outcome"]["outcome"], "cancelled");
        let ev = c.line(r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"cancelled"}}"#);
        assert_eq!((ev[0]["stopped"].as_bool(), ev[0]["is_error"].as_bool()), (Some(true), Some(true)));
        // Before the prompt went out there is nothing to cancel: the process is ended instead.
        let mut c = client(Path::new("/w"), "ask", None, None);
        assert!(c.cancel().is_none());
        assert_eq!(kinds(&c.finish()), ["done"]);
    }

    #[test]
    fn opencode_entry() {
        let a = crate::agent::get("opencode");
        assert_eq!((a.id(), a.bin()), ("opencode", "opencode"));
        assert!(a.interactive("acceptEdits") && a.first_input("hi").is_none());
        assert!(a.needs_trusted_config().is_none(), "it can run with the repo's config off");
        assert!(!a.stopped_by_user(None, ""), "Stop is reported by the client, so this is a crash");
        let cwd = Path::new("/tmp");
        let env = [("OPENCODE_DISABLE_PROJECT_CONFIG".to_string(), "0".to_string()), ("FOO".to_string(), "1".to_string())];
        let mk = |mode, project_config| TurnSpec { fork: false, agent: "opencode", prompt: "-rf", agent_session_id: Some("--yolo"), model: Some("a/b"), permission_mode: mode, cwd, effort: Some("high"), instructions: None, env: &env, dirs: &[], project_config, guard: None };
        let c = crate::agent::command(&mk("ask", false));
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert_eq!(args, ["acp"], "the prompt, model and session go over the protocol, never argv");
        let envs: HashMap<String, Option<String>> = c.get_envs().map(|(k, v)| (k.to_string_lossy().to_string(), v.map(|v| v.to_string_lossy().to_string()))).collect();
        assert_eq!(envs["OPENCODE_DISABLE_PROJECT_CONFIG"].as_deref(), Some("1"), "the repo's env can't switch the guard off");
        assert_eq!(envs["FOO"].as_deref(), Some("1"));
        assert!(envs["OPENCODE_PERMISSION"].as_deref().unwrap().contains("\"bash\":\"ask\""));
        let c = crate::agent::command(&mk("acceptEdits", true));
        let names: Vec<String> = c.get_envs().map(|(k, _)| k.to_string_lossy().to_string()).collect();
        assert!(!names.contains(&"OPENCODE_PERMISSION".to_string()));
        assert!(names.contains(&"OPENCODE_DISABLE_PROJECT_CONFIG".to_string()), "repo env passes through when trusted");
        assert_eq!(c.get_envs().find(|(k, _)| *k == "OPENCODE_DISABLE_PROJECT_CONFIG").unwrap().1.unwrap(), "0");
        // A session id that looks like a flag is dropped, as for the others.
        let mut p = crate::agent::parser(&mk("acceptEdits", true));
        p.line(INIT);
        let out: Vec<Value> = p.outgoing().iter().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(out.last().unwrap()["method"], "session/new");
    }

    /// A client for `agent` that has had `initialize` (with `init`) answered.
    fn client_for(agent: &AcpAgent, cwd: &Path, mode: &str, model: Option<&str>, init: &str) -> Client {
        let s = spec(cwd, mode, None, model);
        let mut c = Client::new(agent, &s, &Checked { model, effort: None, session: None });
        c.line(init);
        sent(&mut c);
        c
    }

    /// Copilot CLI 1.0.103's real `initialize` and `session/new` answers (models trimmed).
    const COPILOT_INIT: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"auth":{"logout":{}},"loadSession":true,"sessionCapabilities":{"close":{},"list":{}}},"authMethods":[{"id":"copilot-login","name":"Log in with Copilot CLI","description":"Run `copilot login` in the terminal"}]}}"#;
    fn copilot_new(mode: &str, allow_all: &str) -> String {
        let m = |x: &str| format!("https://agentclientprotocol.com/protocol/session-modes#{x}");
        json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "c1",
            "models": {"availableModels": [{"modelId": "auto", "name": "Auto"}], "currentModelId": "auto"},
            "configOptions": [
                {"type": "select", "id": "mode", "name": "Mode", "category": "mode", "currentValue": m(mode), "options": [{"value": m("autopilot")}, {"value": m("agent")}, {"value": m("plan")}]},
                {"type": "select", "id": "model", "name": "Model", "category": "model", "currentValue": "auto", "options": [{"value": "auto"}]},
                {"type": "select", "id": "allow_all", "name": "Allow All", "category": "permissions", "currentValue": allow_all, "options": [{"value": "on"}, {"value": "off"}]}],
            "modes": {"availableModes": [{"id": m("autopilot")}, {"id": m("agent")}, {"id": m("plan")}], "currentModeId": m(mode)}}}).to_string()
    }

    #[test]
    fn copilot_plan_mode_is_a_uri_and_ask_keeps_allow_all_off() {
        let w = Path::new("/w");
        // Plan mode is `...#plan`, set through its config option.
        let mut c = client_for(&COPILOT, w, "plan", None, COPILOT_INIT);
        c.line(&copilot_new("agent", "off"));
        let m = sent(&mut c);
        assert_eq!((m[0]["method"].as_str(), m[0]["params"]["configId"].as_str()), (Some("session/set_config_option"), Some("mode")));
        assert_eq!(m[0]["params"]["value"], "https://agentclientprotocol.com/protocol/session-modes#plan");
        // Leaving plan goes back to a mode that asks, never autopilot (listed first here).
        let mut c = client_for(&COPILOT, w, "acceptEdits", None, COPILOT_INIT);
        c.line(&copilot_new("plan", "off"));
        let m = sent(&mut c);
        assert_eq!(m[0]["params"]["value"], "https://agentclientprotocol.com/protocol/session-modes#agent");
        // "Ask before changes" switches allow_all off when a chat had it on, and leaves it otherwise.
        let mut c = client_for(&COPILOT, w, "ask", None, COPILOT_INIT);
        c.line(&copilot_new("agent", "on"));
        let m = sent(&mut c);
        assert_eq!((m[0]["params"]["configId"].as_str(), m[0]["params"]["value"].as_str()), (Some("allow_all"), Some("off")));
        let mut c = client_for(&COPILOT, w, "ask", None, COPILOT_INIT);
        c.line(&copilot_new("agent", "off"));
        assert_eq!(sent(&mut c)[0]["method"], "session/prompt");
        // A Copilot that stops offering it can't promise to ask: no turn.
        let mut c = client_for(&COPILOT, w, "ask", None, COPILOT_INIT);
        let ev = c.line(r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"c1","configOptions":[]}}"#);
        assert_eq!(kinds(&ev), ["session", "error", "done"]);
        assert!(ev[1]["text"].as_str().unwrap().contains("can't promise to ask"));
        // A model typed in the picker goes through its model option.
        let mut c = client_for(&COPILOT, w, "acceptEdits", Some("gpt-6-sol"), COPILOT_INIT);
        c.line(&copilot_new("agent", "off"));
        let m = sent(&mut c);
        assert_eq!((m[0]["params"]["configId"].as_str(), m[0]["params"]["value"].as_str()), (Some("model"), Some("gpt-6-sol")));
    }

    #[test]
    fn ask_mode_needs_a_way_to_ask() {
        // Cursor has no env or option nekotrail can rely on, so "Ask before changes" stops the turn.
        assert!(!CURSOR.asks() && COPILOT.asks() && OPENCODE.asks());
        let mut c = client_for(&CURSOR, Path::new("/w"), "ask", None, INIT);
        let ev = c.line(r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s1","modes":{"currentModeId":"agent","availableModes":[{"id":"agent"},{"id":"plan"},{"id":"ask"}]}}}"#);
        assert_eq!(kinds(&ev), ["session", "error", "done"]);
        assert!(!sent(&mut c).iter().any(|m| m["method"] == "session/prompt"));
        // Its plan mode works through the older `modes` object.
        let mut c = client_for(&CURSOR, Path::new("/w"), "plan", None, INIT);
        c.line(r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s1","modes":{"currentModeId":"agent","availableModes":[{"id":"agent"},{"id":"plan"},{"id":"ask"}]}}}"#);
        let m = sent(&mut c);
        assert_eq!((m[0]["method"].as_str(), m[0]["params"]["modeId"].as_str()), (Some("session/set_mode"), Some("plan")));
    }

    #[test]
    fn auth_required_tries_the_saved_login_once() {
        let init = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true},"authMethods":[{"id":"cursor_login","name":"Cursor login"}]}}"#;
        let auth = r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"Authentication required"}}"#;
        let mut c = client_for(&CURSOR, Path::new("/w"), "acceptEdits", None, init);
        assert!(c.line(auth).is_empty());
        let a = sent(&mut c);
        assert_eq!((a[0]["method"].as_str(), a[0]["params"]["methodId"].as_str()), (Some("authenticate"), Some("cursor_login")));
        c.line(&json!({"jsonrpc": "2.0", "id": a[0]["id"], "result": {}}).to_string());
        let n = sent(&mut c);
        assert_eq!(n[0]["method"], "session/new", "the chat opens again after signing in");
        // Still not signed in: one try only, then the sign-in message.
        let ev = c.line(&json!({"jsonrpc": "2.0", "id": n[0]["id"], "error": {"code": -32000, "message": "Authentication required"}}).to_string());
        assert_eq!(kinds(&ev), ["error", "done"]);
        assert_eq!(ev[0]["text"], "Cursor is not logged in. Run `cursor-agent login` in a terminal, then send again.");
        // `authenticate` itself failing says the same.
        let mut c = client_for(&CURSOR, Path::new("/w"), "acceptEdits", None, init);
        c.line(auth);
        let ev = c.line(r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32603,"message":"No saved login"}}"#);
        assert!(ev[0]["text"].as_str().unwrap().contains("not logged in"), "{ev:?}");
        // Agents without a saved-login method (Grok's only method signs in from scratch), or
        // whose `initialize` didn't offer it, never get `authenticate`.
        for (agent, init) in [(&GROK, r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{},"authMethods":[{"id":"grok.com","name":"Grok"}]}}"#), (&CURSOR, INIT)] {
            let mut c = client_for(agent, Path::new("/w"), "acceptEdits", None, init);
            let ev = c.line(auth);
            assert_eq!(kinds(&ev), ["error", "done"]);
            assert!(!sent(&mut c).iter().any(|m| m["method"] == "authenticate"));
        }
    }

    #[test]
    fn accept_edits_answers_worktree_edits_and_asks_for_the_rest() {
        let w = Path::new("/w/ws");
        let opts = json!([{"optionId": "once", "kind": "allow_once", "name": "Allow"}, {"optionId": "always", "kind": "allow_always", "name": "Always"}, {"optionId": "no", "kind": "reject_once", "name": "Reject"}]);
        let ask = |kind: &str, path: &str| json!({"jsonrpc": "2.0", "id": 5, "method": "session/request_permission", "params": {"sessionId": "s1", "toolCall": {"toolCallId": format!("{kind}{path}"), "title": "x", "kind": kind, "locations": if path.is_empty() { json!([]) } else { json!([{"path": path}]) }}, "options": opts}}).to_string();
        for (kind, path, auto) in [
            ("edit", "/w/ws/src/a.rs", true),
            ("read", "/w/ws/README.md", true),
            ("edit", "/w/ws/../other/a.rs", false),
            ("edit", "/w/wsx/a.rs", false),
            ("edit", "/Users/me/.zshrc", false),
            ("edit", "", false),
            ("execute", "/w/ws", false),
            ("delete", "/w/ws/a.rs", false),
        ] {
            let (mut c, _) = started(w, "acceptEdits", None);
            sent(&mut c);
            let ev = c.line(&ask(kind, path));
            if auto {
                assert_eq!(kinds(&ev), ["tool"], "{kind} {path}");
                assert_eq!(sent(&mut c)[0]["result"]["outcome"]["optionId"], "once", "never stretched to always");
            } else {
                assert_eq!(kinds(&ev), ["tool", "approval"], "{kind} {path}");
            }
        }
        // "Ask before changes" asks for edits too.
        let (mut c, _) = started(w, "ask", None);
        sent(&mut c);
        assert_eq!(kinds(&c.line(&ask("edit", "/w/ws/a.rs"))), ["tool", "approval"]);
    }

    #[test]
    fn protected_edits_are_refused_for_every_acp_agent() {
        let g = crate::guard::Rules::new(&[".env*".into(), "migrations/**".into()], &[]);
        let opts = json!([{"optionId": "once", "kind": "allow_once", "name": "Allow"}, {"optionId": "always", "kind": "allow_always", "name": "Always"}, {"optionId": "no", "kind": "reject_once", "name": "Reject"}]);
        let req = |id: u64, tool: Value| json!({"jsonrpc": "2.0", "id": id, "method": "session/request_permission", "params": {"sessionId": "s1", "toolCall": tool, "options": opts}}).to_string();
        let init = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{}}}"#;
        let agents: Vec<&AcpAgent> = std::iter::once(&OPENCODE).chain(MORE.iter().copied()).collect();
        assert_eq!(agents.len(), 13);
        for agent in agents {
            for mode in ["acceptEdits", "bypassPermissions", "default", "ask"] {
                let s = TurnSpec { guard: Some(&g), ..spec(Path::new("/w/ws"), mode, None, None) };
                let mut c = Client::new(agent, &s, &Checked { model: None, effort: None, session: None });
                c.line(init);
                sent(&mut c);
                // In the worktree with a location, so "Auto-accept edits" and Full auto would allow
                // it; by absolute path, through a diff, or inside an apply_patch body.
                for (i, tool) in [
                    json!({"toolCallId": "a", "title": "edit", "kind": "edit", "rawInput": {"filePath": "/w/ws/.env"}, "locations": [{"path": "/w/ws/.env"}]}),
                    json!({"toolCallId": "b", "title": "write", "kind": "edit", "content": [{"type": "diff", "path": "/w/ws/migrations/001.sql", "newText": "x"}], "locations": [{"path": "/w/ws/migrations/001.sql"}]}),
                    json!({"toolCallId": "c", "title": "patch", "kind": "edit", "rawInput": {"patchText": "*** Begin Patch\n*** Update File: src/a.ts\n*** Add File: apps/web/.env.local\n*** End Patch"}}),
                    json!({"toolCallId": "d", "title": "rm", "kind": "delete", "locations": [{"path": "/w/ws/migrations/002.sql"}]}),
                ].into_iter().enumerate() {
                    let ev = c.line(&req(i as u64, tool));
                    assert_eq!(kinds(&ev), ["tool", "blocked"], "{} {mode} {i}", agent.id);
                    assert_eq!(sent(&mut c)[0]["result"]["outcome"]["optionId"], "no", "{} {mode} {i}", agent.id);
                }
                // Anything else is answered exactly as without protected paths.
                let ev = c.line(&req(9, json!({"toolCallId": "e", "title": "edit", "kind": "edit", "rawInput": {"filePath": "/w/ws/src/a.ts"}, "locations": [{"path": "/w/ws/src/a.ts"}]})));
                let asked = kinds(&ev).contains(&"approval");
                let auto = matches!(mode, "acceptEdits" | "bypassPermissions") || (agent.id == "opencode" && mode == "default");
                assert_eq!(asked, !auto, "{} {mode}", agent.id);
                if auto {
                    assert_eq!(sent(&mut c)[0]["result"]["outcome"]["optionId"], "once", "{} {mode}", agent.id);
                }
                // Reading a protected file is not a change.
                let ev = c.line(&req(10, json!({"toolCallId": "f", "title": "read", "kind": "read", "rawInput": {"filePath": "/w/ws/.env"}, "locations": [{"path": "/w/ws/.env"}]})));
                assert!(!kinds(&ev).contains(&"blocked"), "{} {mode}", agent.id);
            }
            // How it holds, for Repo settings: OpenCode can be made to ask in every mode.
            assert_eq!(agent.guard_level(), if agent.id == "opencode" { "blocked" } else { "asks" }, "{}", agent.id);
        }
        // The env that makes OpenCode ask before edits: only with protected paths, outside ask and plan.
        let env_of = |mode: &str, guard: Option<&crate::guard::Rules>| {
            let s = TurnSpec { agent: "opencode", guard, ..spec(Path::new("/tmp"), mode, None, None) };
            crate::agent::command(&s).get_envs().find(|(k, _)| *k == "OPENCODE_PERMISSION").and_then(|(_, v)| v.map(|v| v.to_string_lossy().to_string()))
        };
        assert_eq!(env_of("acceptEdits", Some(&g)).as_deref(), Some(r#"{"edit":"ask"}"#));
        assert_eq!(env_of("bypassPermissions", Some(&g)).as_deref(), Some(r#"{"edit":"ask"}"#));
        assert!(env_of("acceptEdits", None).is_none());
        assert!(env_of("plan", Some(&g)).is_none());
        assert!(env_of("ask", Some(&g)).unwrap().contains("\"bash\":\"ask\""));
        // Without protected paths nothing changes: Full auto allows every edit.
        let mut c = client(Path::new("/w/ws"), "bypassPermissions", None, None);
        sent(&mut c);
        let ev = c.line(&req(1, json!({"toolCallId": "a", "kind": "edit", "rawInput": {"filePath": "/w/ws/.env"}})));
        assert_eq!(kinds(&ev), ["tool"]);
    }

    /// The real OpenCode in Auto-accept with protected paths: its edit to `.env` is refused by
    /// nekotrail, its other edit goes through without asking. Needs an OpenCode sign-in, so it only
    /// runs when asked: `NEKOTRAIL_REAL_OPENCODE=$(which opencode) cargo test real_opencode_guard -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_opencode_guard() {
        let bin = std::env::var("NEKOTRAIL_REAL_OPENCODE").expect("set NEKOTRAIL_REAL_OPENCODE to the opencode binary");
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".env"), "TOKEN=old\n").unwrap();
        let g = crate::guard::Rules::new(&[".env*".into()], &[]);
        let spec = TurnSpec { fork: false, agent: "opencode", prompt: "", agent_session_id: None, model: None, permission_mode: "acceptEdits", cwd: d.path(), effort: None, instructions: None, env: &[], dirs: &[], project_config: false, guard: Some(&g) };
        let built = crate::agent::command(&spec);
        let mut cmd = Command::new(&bin);
        cmd.args(built.get_args());
        for (k, v) in built.get_envs() {
            if let Some(v) = v {
                cmd.env(k, v);
            }
        }
        let mut asked = 0;
        let r = drive_command(cmd, &OPENCODE, d.path(), "acceptEdits", "Use your write tool to set .env to TOKEN=new, then use your write tool to create b.txt containing bye. If one fails, carry on with the other. Then reply with just DONE.", Some(&g), |ev, _| {
            if ev["kind"] == "approval" {
                asked += 1;
            }
            vec![]
        });
        for e in &r.events {
            eprintln!("{e}");
        }
        eprintln!("stderr: {}", r.stderr.lines().take(5).collect::<Vec<_>>().join("\n"));
        assert!(r.events.iter().any(|e| e["kind"] == "blocked" && e["files"][0] == ".env"), "the .env edit was refused");
        assert_eq!(std::fs::read_to_string(d.path().join(".env")).unwrap(), "TOKEN=old\n");
        assert_eq!(std::fs::read_to_string(d.path().join("b.txt")).unwrap().trim(), "bye");
        assert_eq!(asked, 0, "nothing waited for the user");
    }

    #[test]
    fn every_acp_entry_builds_its_command() {
        let cwd = Path::new("/tmp");
        let want: [(&str, &str, &[&str]); 12] = [
            ("cursor", "cursor-agent", &["acp"]),
            ("copilot", "copilot", &["--acp"]),
            ("goose", "goose", &["acp"]),
            ("qwen", "qwen", &["--acp"]),
            ("kimi", "kimi", &["acp"]),
            ("auggie", "auggie", &["--acp"]),
            ("grok", "grok", &["agent", "stdio"]),
            ("kiro", "kiro-cli", &["acp"]),
            ("droid", "droid", &["exec", "--output-format", "acp"]),
            ("vibe", "vibe-acp", &[]),
            ("cline", "cline", &["--acp"]),
            ("kilo", "kilo", &["acp"]),
        ];
        assert_eq!(MORE.map(|a| a.id), want.map(|w| w.0));
        for (id, bin, args) in want {
            let a = crate::agent::get(id);
            assert_eq!((a.id(), a.bin()), (id, bin));
            let entry = MORE.iter().find(|e| e.id == id).unwrap();
            assert!(!entry.name.is_empty() && !entry.login.is_empty(), "{id}");
            assert!(entry.config_off_env.is_empty() && a.needs_trusted_config().unwrap().contains(entry.name), "{id}: no documented way to skip repo config");
            assert!(matches!(entry.repo_config, RepoConfig::Files(f) if !f.is_empty()), "{id} reads repo config conservatively");
            for mode in ["plan", "ask", "acceptEdits", "bypassPermissions"] {
                let env = [("FOO".to_string(), "1".to_string())];
                let s = TurnSpec { fork: false, agent: id, prompt: "--yolo", agent_session_id: Some("--resume"), model: Some("x/y"), permission_mode: mode, cwd, effort: Some("high"), instructions: None, env: &env, dirs: &[], project_config: false, guard: None };
                let c = crate::agent::command(&s);
                assert_eq!(c.get_program(), bin);
                let got: Vec<_> = c.get_args().map(|a| a.to_string_lossy().to_string()).collect();
                assert_eq!(got, args, "{id} {mode}: prompt, model and session go over the protocol, never argv");
                let envs: Vec<_> = c.get_envs().map(|(k, _)| k.to_string_lossy().to_string()).collect();
                assert_eq!(envs, ["FOO"], "{id} {mode}");
                assert!(crate::agent::interactive(&s));
            }
        }
    }

    // ---------- a fake ACP agent over real stdio ----------

    const FAKE_ENV: &str = "NEKOTRAIL_FAKE_ACP";

    /// Not a real test: run as a child process with `NEKOTRAIL_FAKE_ACP=<scenario>`, this test
    /// binary becomes a tiny ACP agent on stdin and stdout. Without it, it does nothing.
    #[test]
    fn fake_acp_agent() {
        let Ok(scenario) = std::env::var(FAKE_ENV) else { return };
        fake_agent(&scenario);
        std::process::exit(0);
    }

    fn fake_agent(scenario: &str) {
        let stdin = std::io::stdin();
        let mut lines = stdin.lock().lines().map_while(Result::ok);
        let mut out = std::io::stdout();
        let mut send = |v: Value| {
            writeln!(out, "{v}").unwrap();
            out.flush().unwrap();
        };
        let up = |u: Value| json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "fake-1", "update": u}});
        let say = |t: &str| up(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": t}}));
        // "auth": like Cursor, no chat until the client calls `authenticate` with the saved login.
        let mut authed = scenario != "auth";
        while let Some(line) = lines.next() {
            let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
            let id = m["id"].clone();
            match m["method"].as_str().unwrap_or("") {
                "initialize" => send(json!({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": false}, "authMethods": [{"id": "fake_login", "name": "Saved login"}]}})),
                "authenticate" => {
                    authed = m["params"]["methodId"] == "fake_login";
                    send(json!({"jsonrpc": "2.0", "id": id, "result": {}}));
                }
                "session/new" if !authed => send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "Authentication required"}})),
                "session/new" => {
                    let cwd = m["params"]["cwd"].as_str().unwrap_or("");
                    send(json!({"jsonrpc": "2.0", "id": id, "result": {"sessionId": "fake-1", "configOptions": [
                        {"id": "mode", "category": "mode", "currentValue": "build", "options": [{"value": "build"}, {"value": "plan"}]}], "_cwd": cwd}}));
                }
                "session/set_config_option" => send(json!({"jsonrpc": "2.0", "id": id, "result": {"configOptions": []}})),
                "session/prompt" => {
                    let text = m["params"]["prompt"][0]["text"].as_str().unwrap_or("").to_string();
                    match scenario {
                        "chat" | "auth" => {
                            send(say("You said: "));
                            send(say(&text));
                            send(up(json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "bash", "kind": "execute", "status": "pending", "rawInput": {}})));
                            send(up(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "in_progress", "rawInput": {"command": "ls"}})));
                            send(up(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed", "content": [{"type": "content", "content": {"type": "text", "text": "a.txt"}}]})));
                            send(up(json!({"sessionUpdate": "plan", "entries": [{"content": "Look around", "priority": "high", "status": "completed"}]})));
                            send(say("DONE"));
                            send(json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": "end_turn"}}));
                        }
                        "permission" => {
                            send(up(json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "write", "kind": "edit", "status": "pending", "rawInput": {"filePath": "notes.md"}})));
                            send(json!({"jsonrpc": "2.0", "id": 0, "method": "session/request_permission", "params": {"sessionId": "fake-1", "toolCall": {"toolCallId": "t1", "kind": "edit"},
                                "options": [{"optionId": "yes", "kind": "allow_once", "name": "Allow"}, {"optionId": "chat", "kind": "allow_always", "name": "Allow for this chat"}, {"optionId": "no", "kind": "reject_once", "name": "Deny"}]}}));
                            send(json!({"jsonrpc": "2.0", "id": 1, "method": "fs/read_text_file", "params": {"sessionId": "fake-1", "path": "/etc/passwd"}}));
                            let mut got = vec![];
                            while got.len() < 2 {
                                let Some(l) = lines.next() else { return };
                                let r: Value = serde_json::from_str(&l).unwrap_or_default();
                                if r["id"] == 0 {
                                    got.push(format!("permission: {}", r["result"]["outcome"]["optionId"].as_str().or(r["result"]["outcome"]["outcome"].as_str()).unwrap_or("?")));
                                } else if r["id"] == 1 {
                                    got.push(format!("fs: {}", r["error"]["code"]));
                                }
                            }
                            got.sort();
                            send(say(&got.join(", ")));
                            send(json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": "end_turn"}}));
                        }
                        "cancel" => {
                            send(up(json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "bash", "kind": "execute", "status": "in_progress", "rawInput": {"command": "sleep 100"}})));
                            send(json!({"jsonrpc": "2.0", "id": 0, "method": "session/request_permission", "params": {"sessionId": "fake-1", "toolCall": {"toolCallId": "t1"}, "options": [{"optionId": "yes", "kind": "allow_once", "name": "Allow"}]}}));
                            let mut seen = vec![];
                            while seen.len() < 2 {
                                let Some(l) = lines.next() else { return };
                                let r: Value = serde_json::from_str(&l).unwrap_or_default();
                                if r["method"] == "session/cancel" {
                                    seen.push("cancel".to_string());
                                } else if r["id"] == 0 {
                                    seen.push(format!("permission {}", r["result"]["outcome"]["outcome"].as_str().unwrap_or("?")));
                                }
                            }
                            seen.sort();
                            send(up(json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "failed", "rawOutput": {"error": "cancelled"}})));
                            send(say(&seen.join(", ")));
                            send(json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": "cancelled"}}));
                        }
                        "crash" => {
                            send(say("half an answ"));
                            eprintln!("fake agent fell over");
                            std::process::exit(3);
                        }
                        _ => {}
                    }
                }
                _ => {
                    if !id.is_null() {
                        send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Method not found"}}));
                    }
                }
            }
        }
    }

    struct Run {
        events: Vec<Value>,
        status: ExitStatus,
        stderr: String,
        finished: bool,
    }

    /// The turn loop `lib.rs` runs, minus the app: write what the client sends, feed it every
    /// line, let `react` answer approvals or press Stop, close stdin once the turn is answered.
    fn drive(scenario: &str, mode: &str, react: impl FnMut(&Value, &mut dyn Parser) -> Vec<String>) -> Run {
        drive_as(&FAKE, scenario, mode, react)
    }

    fn drive_as(agent: &AcpAgent, scenario: &str, mode: &str, react: impl FnMut(&Value, &mut dyn Parser) -> Vec<String>) -> Run {
        let d = tempfile::tempdir().unwrap();
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "acp::tests::fake_acp_agent", "--nocapture", "--test-threads=1", "-q"]).env(FAKE_ENV, scenario);
        drive_command(cmd, agent, d.path(), mode, "hello", None, react)
    }

    fn drive_command(mut cmd: Command, agent: &AcpAgent, cwd: &Path, mode: &str, prompt: &str, guard: Option<&crate::guard::Rules>, mut react: impl FnMut(&Value, &mut dyn Parser) -> Vec<String>) -> Run {
        let s = TurnSpec { fork: false, agent: agent.id, prompt, agent_session_id: None, model: None, permission_mode: mode, cwd, effort: None, instructions: None, env: &[], dirs: &[], project_config: true, guard };
        let mut parser = agent.parser(&s, &Checked { model: None, effort: None, session: None });
        let mut child = cmd
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take();
        let write = |stdin: &mut Option<std::process::ChildStdin>, lines: Vec<String>| {
            if let Some(w) = stdin.as_mut() {
                for l in lines {
                    let _ = w.write_all(l.as_bytes());
                }
                let _ = w.flush();
            }
        };
        write(&mut stdin, parser.outgoing());
        let mut events = vec![];
        for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
            let evs = parser.line(&line);
            write(&mut stdin, parser.outgoing());
            for ev in evs {
                let more = react(&ev, parser.as_mut());
                write(&mut stdin, more);
                events.push(ev);
            }
            if parser.finished() {
                stdin = None;
            }
        }
        events.extend(parser.finish());
        let finished = parser.finished();
        let status = child.wait().unwrap();
        let mut stderr = String::new();
        std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
        Run { events, status, stderr, finished }
    }

    #[test]
    fn stdio_turn_with_chunks_tools_and_plan() {
        let r = drive("chat", "acceptEdits", |_, _| vec![]);
        assert_eq!(kinds(&r.events), ["session", "text", "tool", "tool_result", "tool", "tool_result", "text", "done"]);
        assert_eq!(r.events[0]["agent_session_id"], "fake-1");
        assert_eq!(r.events[1]["text"], "You said: hello");
        assert_eq!((r.events[2]["name"].as_str(), r.events[2]["input"].as_str()), (Some("Bash"), Some("ls")));
        assert_eq!(r.events[3]["output"], "a.txt");
        assert_eq!(r.events[4]["name"], "TodoWrite");
        assert_eq!(r.events[6]["text"], "DONE");
        assert_eq!(r.events[7]["is_error"], false);
        assert!(r.finished);
        assert!(r.status.success(), "the agent exits once stdin closes");
    }

    #[test]
    fn stdio_permission_answers() {
        for (allow, always, want) in [(true, false, "yes"), (true, true, "chat"), (false, false, "no")] {
            let r = drive("permission", "ask", |ev, p| {
                if ev["kind"] != "approval" {
                    return vec![];
                }
                assert_eq!((ev["tool"].as_str(), ev["input"].as_str(), ev["can_always"].as_bool()), (Some("Write"), Some("notes.md"), Some(true)));
                vec![p.answer(ev["request_id"].as_str().unwrap(), allow, always).unwrap().unwrap()]
            });
            assert_eq!(kinds(&r.events), ["session", "tool", "approval", "text", "done"]);
            assert_eq!(r.events[3]["text"], format!("fs: -32601, permission: {want}"), "the fs request is refused");
            assert_eq!(r.events[4]["is_error"], false);
        }
    }

    #[test]
    fn stdio_signs_in_with_the_saved_login_when_asked() {
        static FAKE_AUTH: AcpAgent = AcpAgent { id: "fake", name: "Fake", bin: "fake-acp", login: "fake login", auth_method: Some("fake_login"), ..AcpAgent::BASE };
        let r = drive_as(&FAKE_AUTH, "auth", "acceptEdits", |_, _| vec![]);
        assert_eq!(kinds(&r.events), ["session", "text", "tool", "tool_result", "tool", "tool_result", "text", "done"], "{:?}", r.events);
        assert_eq!(r.events.last().unwrap()["is_error"], false);
        // Without a saved-login method the same agent ends the turn with the sign-in hint.
        let r = drive_as(&FAKE, "auth", "acceptEdits", |_, _| vec![]);
        assert_eq!(kinds(&r.events), ["error", "done"]);
        assert_eq!(r.events[0]["text"], "Fake is not logged in. Run `fake login` in a terminal, then send again.");
        assert!(r.status.success());
    }

    #[test]
    fn stdio_cancel() {
        let mut asked = false;
        let r = drive("cancel", "ask", |ev, p| {
            if ev["kind"] == "approval" {
                asked = true;
                // The user presses Stop while the card is open.
                return p.cancel().unwrap();
            }
            vec![]
        });
        assert!(asked);
        assert_eq!(kinds(&r.events), ["session", "tool", "approval", "tool_result", "text", "done"]);
        assert_eq!(r.events[3]["is_error"], true);
        assert_eq!(r.events[4]["text"], "cancel, permission cancelled");
        assert_eq!((r.events[5]["stopped"].as_bool(), r.events[5]["is_error"].as_bool()), (Some(true), Some(true)));
        assert!(r.status.success());
    }

    /// A real OpenCode turn in "Ask before changes", through the same command and env nekotrail
    /// builds. Needs OpenCode 1.18+ (its free models need no sign-in) and the network, so it only
    /// runs when asked: `NEKOTRAIL_REAL_OPENCODE=$(which opencode) cargo test real_opencode -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_opencode_turn() {
        let bin = std::env::var("NEKOTRAIL_REAL_OPENCODE").expect("set NEKOTRAIL_REAL_OPENCODE to the opencode binary");
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "hi\n").unwrap();
        // Untrusted repo config must stay off: this server would print a marker if it started.
        std::fs::write(d.path().join("opencode.json"), r#"{"mcp":{"evil":{"type":"local","command":["sh","-c","touch pwned"]}}}"#).unwrap();
        let spec = TurnSpec { fork: false, agent: "opencode", prompt: "", agent_session_id: None, model: None, permission_mode: "ask", cwd: d.path(), effort: None, instructions: None, env: &[], dirs: &[], project_config: false, guard: None };
        let built = crate::agent::command(&spec);
        let mut cmd = Command::new(&bin);
        cmd.args(built.get_args());
        for (k, v) in built.get_envs() {
            if let Some(v) = v {
                cmd.env(k, v);
            }
        }
        let mut asked = vec![];
        let r = drive_command(cmd, &OPENCODE, d.path(), "ask", "Run the shell command `cat a.txt`, then create b.txt containing bye with your write tool, then reply with just DONE.", None, |ev, p| {
            if ev["kind"] != "approval" {
                return vec![];
            }
            asked.push(format!("{} {}", ev["tool"].as_str().unwrap_or(""), ev["input"].as_str().unwrap_or("")));
            vec![p.answer(ev["request_id"].as_str().unwrap(), true, false).unwrap().unwrap()]
        });
        for e in &r.events {
            eprintln!("{e}");
        }
        eprintln!("asked: {asked:?}\nstderr: {}", r.stderr.lines().take(5).collect::<Vec<_>>().join("\n"));
        assert_eq!(r.events.last().unwrap()["kind"], "done");
        assert_eq!(r.events.last().unwrap()["is_error"], false);
        assert!(asked.iter().any(|a| a.starts_with("Bash cat a.txt")), "{asked:?}");
        assert!(asked.iter().any(|a| a.contains("b.txt")), "{asked:?}");
        assert_eq!(std::fs::read_to_string(d.path().join("b.txt")).unwrap().trim(), "bye");
        assert!(!d.path().join("pwned").exists(), "the repo's MCP server must not start");
        assert!(r.status.success());
    }

    /// A real turn with any other ACP agent that is installed and signed in, through the command
    /// nekotrail builds: `NEKOTRAIL_REAL_ACP=copilot cargo test real_acp_turn -- --ignored --nocapture`.
    /// `NEKOTRAIL_REAL_MODE` picks the mode (default "ask", where every change is approved here).
    #[test]
    #[ignore]
    fn real_acp_turn() {
        let id = std::env::var("NEKOTRAIL_REAL_ACP").expect("set NEKOTRAIL_REAL_ACP to an agent id");
        let mode = std::env::var("NEKOTRAIL_REAL_MODE").unwrap_or_else(|_| "ask".into());
        let agent = MORE.iter().find(|a| a.id == id).expect("an ACP agent id");
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "hi\n").unwrap();
        let spec = TurnSpec { fork: false, agent: agent.id, prompt: "", agent_session_id: None, model: None, permission_mode: &mode, cwd: d.path(), effort: None, instructions: None, env: &[], dirs: &[], project_config: true, guard: None };
        let built = crate::agent::command(&spec);
        let mut cmd = Command::new(built.get_program());
        cmd.args(built.get_args());
        let mut asked = vec![];
        let r = drive_command(cmd, agent, d.path(), &mode, "Run the shell command `cat a.txt`, then create a file b.txt containing the word bye, then reply with just DONE.", None, |ev, p| {
            if ev["kind"] != "approval" {
                return vec![];
            }
            asked.push(format!("{} {}", ev["tool"].as_str().unwrap_or(""), ev["input"].as_str().unwrap_or("")));
            vec![p.answer(ev["request_id"].as_str().unwrap(), true, false).unwrap().unwrap()]
        });
        for e in &r.events {
            eprintln!("{e}");
        }
        eprintln!("asked: {asked:?}\nstderr: {}", r.stderr.lines().take(8).collect::<Vec<_>>().join("\n"));
        assert_eq!(r.events.last().unwrap()["kind"], "done");
        assert_eq!(r.events.last().unwrap()["is_error"], false);
        assert_eq!(std::fs::read_to_string(d.path().join("b.txt")).unwrap().trim(), "bye");
        if mode == "ask" {
            assert!(asked.iter().any(|a| a.contains("b.txt")), "the edit waited for Allow: {asked:?}");
        }
    }

    #[test]
    fn stdio_agent_crash() {
        let r = drive("crash", "acceptEdits", |_, _| vec![]);
        // The half answer is kept; no `done`, so the turn code reports the crash with stderr.
        assert_eq!(kinds(&r.events), ["session", "text"]);
        assert_eq!(r.events[1]["text"], "half an answ");
        assert!(!r.finished);
        assert_eq!(r.status.code(), Some(3));
        assert!(r.stderr.contains("fake agent fell over"));
        assert!(!FAKE.stopped_by_user(Some(&r.status), &r.stderr));
    }
}
