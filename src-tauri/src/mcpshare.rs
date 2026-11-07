//! Share MCP servers and skills between Claude Code, Codex, Gemini CLI and OpenCode, in each CLI's own
//! user config. nekotrail never proxies a server: it copies the definition into the other CLI's
//! file, in that CLI's format, and the CLI runs it as if you had added it yourself.
//!
//! Formats, checked on 2026-11-06 against each CLI's docs and the files on a real Mac
//! (Claude Code 2.1, codex-cli 0.164.0, Gemini CLI 0.65.0 source):
//! - Claude Code: `mcpServers` at the top of `~/.claude.json` (user scope, or
//!   `$CLAUDE_CONFIG_DIR/.claude.json`), under `projects.<path>.mcpServers` (local scope) and in the
//!   repo's `.mcp.json` (project scope). Entries are `{"type": "stdio", "command", "args", "env"}`
//!   or `{"type": "http" | "sse" | "ws", "url", "headers"}`; no `type` with a `command` is stdio, a
//!   `url` without a `type` is skipped. `${VAR}` and `${VAR:-default}` expand in command, args,
//!   env, url and headers. No working-folder setting.
//! - Codex: `[mcp_servers.<name>]` in `~/.codex/config.toml` (or `$CODEX_HOME`). Stdio: `command`,
//!   `args`, `env` (literal values, never expanded), `env_vars` (names passed on from Codex's own
//!   environment), `cwd`. Streamable HTTP: `url`, `bearer_token_env_var`, `http_headers` (literal),
//!   `env_http_headers` (header to variable name). No SSE or WebSocket. `enabled = false` turns
//!   one off.
//! - Gemini CLI: `mcpServers` in `~/.gemini/settings.json` (or `$GEMINI_CLI_HOME/.gemini`), which
//!   may contain comments. `command`, `args`, `env`, `cwd` for stdio; `httpUrl`, or `url` with
//!   `"type": "http"` (or no type), for streamable HTTP; `url` with `"type": "sse"` for SSE;
//!   `headers`. `$VAR` and `${VAR}` expand in env and headers, but Gemini hides variables whose
//!   names contain TOKEN, KEY, SECRET, PASSWORD, AUTH, CREDENTIAL and similar from MCP servers
//!   unless they are listed in `security.environmentVariableRedaction.allowed`.
//! - Skills: Claude Code reads `~/.claude/skills/<name>/SKILL.md`. Codex reads
//!   `~/.agents/skills/<name>/SKILL.md` and follows symlinked folders; Gemini CLI reads both
//!   `~/.gemini/skills` and `~/.agents/skills` (skills are on by default in 0.65). Codex needs `name`
//!   and `description` in the front matter, and Gemini skips a skill without them. So one symlink
//!   in `~/.agents/skills` shares a Claude skill with both, and the skill keeps one home.
//! - OpenCode (1.17 binary and opencode.ai/docs, checked 2026-11-06): `mcp` in its global config
//!   folder, `$XDG_CONFIG_HOME/opencode` (default `~/.config/opencode`), read from `config.json`,
//!   then `opencode.json`, then `opencode.jsonc` (a later file wins). OpenCode writes its own changes
//!   to the first of `opencode.jsonc`, `opencode.json`, `config.json` that exists (a new
//!   `opencode.jsonc` if none), and so does nekotrail. `$OPENCODE_CONFIG` names one more file, loaded
//!   after those. Project config is `opencode.json(c)` in the repo or its `.opencode/`. All of them
//!   may hold comments and trailing commas. Entries are `{"type": "local", "command": [cmd, ...args],
//!   "environment", "cwd", "enabled"}` (a relative `cwd` is from the workspace) or `{"type":
//!   "remote", "url", "headers", "oauth", "enabled"}`; remote tries streamable HTTP, then SSE.
//!   `{env:NAME}` (empty when unset) and `{file:path}` are replaced in the raw file text before it
//!   is parsed, so they work in every string, and a literal `{env:` in any value would be read as
//!   one. Skills: OpenCode reads `~/.config/opencode/skills`, `~/.claude/skills` (off with
//!   `OPENCODE_DISABLE_CLAUDE_CODE(_SKILLS)`) and `~/.agents/skills` (both off with
//!   `OPENCODE_DISABLE_EXTERNAL_SKILLS`), and skips a SKILL.md with no `name`.
//!
//! Every write shows the exact text first, backs the file up next to itself
//! (`<file>.bak-nekotrail-<time>`), keeps everything else byte for byte (TOML through `toml_edit`,
//! JSON by inserting text, both checked by parsing the result), and never replaces a server or
//! skill that already has the name.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentId {
    Claude,
    Codex,
    Gemini,
    OpenCode,
}
use AgentId::*;

pub const AGENTS: [AgentId; 4] = [Claude, Codex, Gemini, OpenCode];

impl AgentId {
    pub fn label(self) -> &'static str {
        match self {
            Claude => "Claude Code",
            Codex => "Codex",
            Gemini => "Gemini CLI",
            OpenCode => "OpenCode",
        }
    }
}

/// Where each CLI keeps its user config. `from_env` for the app, a temp dir in tests.
#[derive(Clone, Debug)]
pub struct Homes {
    pub home: PathBuf,
    pub claude_json: PathBuf,
    pub claude_skills: PathBuf,
    pub codex_config: PathBuf,
    pub gemini_settings: PathBuf,
    /// `~/.agents/skills`: read by Codex and Gemini CLI.
    pub agents_skills: PathBuf,
    pub gemini_skills: PathBuf,
    /// OpenCode's global config folder (`~/.config/opencode`).
    pub opencode_dir: PathBuf,
    /// `$OPENCODE_CONFIG`: one more config file OpenCode loads.
    pub opencode_custom: Option<PathBuf>,
    pub opencode_skills: PathBuf,
    /// OpenCode also reads `~/.claude/skills` and `~/.agents/skills` unless told not to.
    pub opencode_reads_claude_skills: bool,
    pub opencode_reads_agents_skills: bool,
}

/// OpenCode's global config files, in the order it merges them (the last one wins).
const OPENCODE_GLOBAL: [&str; 3] = ["config.json", "opencode.json", "opencode.jsonc"];

impl Homes {
    pub fn under(home: &Path) -> Homes {
        Homes {
            home: home.into(),
            claude_json: home.join(".claude.json"),
            claude_skills: home.join(".claude/skills"),
            codex_config: home.join(".codex/config.toml"),
            gemini_settings: home.join(".gemini/settings.json"),
            agents_skills: home.join(".agents/skills"),
            gemini_skills: home.join(".gemini/skills"),
            opencode_dir: home.join(".config/opencode"),
            opencode_custom: None,
            opencode_skills: home.join(".config/opencode/skills"),
            opencode_reads_claude_skills: true,
            opencode_reads_agents_skills: true,
        }
    }

    pub fn from_env() -> Result<Homes, String> {
        let home = dirs::home_dir().ok_or("Couldn't find your home folder.")?;
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let mut h = Homes::under(&home);
        if let Some(d) = var("CLAUDE_CONFIG_DIR") {
            h.claude_json = d.join(".claude.json");
            h.claude_skills = d.join("skills");
        }
        if let Some(d) = var("CODEX_HOME") {
            h.codex_config = d.join("config.toml");
        }
        if let Some(d) = var("GEMINI_CLI_HOME") {
            h.gemini_settings = d.join(".gemini/settings.json");
            h.gemini_skills = d.join(".gemini/skills");
        }
        if let Some(d) = var("XDG_CONFIG_HOME") {
            h.opencode_dir = d.join("opencode");
            h.opencode_skills = d.join("opencode/skills");
        }
        h.opencode_custom = var("OPENCODE_CONFIG");
        // OpenCode's flags are on for "1" or "true".
        let flag = |k: &str| std::env::var(k).is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
        h.opencode_reads_agents_skills = !flag("OPENCODE_DISABLE_EXTERNAL_SKILLS");
        h.opencode_reads_claude_skills = h.opencode_reads_agents_skills && !flag("OPENCODE_DISABLE_CLAUDE_CODE") && !flag("OPENCODE_DISABLE_CLAUDE_CODE_SKILLS");
        Ok(h)
    }

    /// The file "Add to" writes. For OpenCode, the global file it writes itself: the first of
    /// opencode.jsonc, opencode.json, config.json that exists, else a new opencode.jsonc.
    pub fn user_file(&self, a: AgentId) -> PathBuf {
        match a {
            Claude => self.claude_json.clone(),
            Codex => self.codex_config.clone(),
            Gemini => self.gemini_settings.clone(),
            OpenCode => OPENCODE_GLOBAL.iter().rev().map(|f| self.opencode_dir.join(f)).find(|p| p.exists()).unwrap_or_else(|| self.opencode_dir.join("opencode.jsonc")),
        }
    }

    fn tilde(&self, p: &Path) -> String {
        let real = std::fs::canonicalize(&self.home).unwrap_or_else(|_| self.home.clone());
        match p.strip_prefix(&self.home).or_else(|_| p.strip_prefix(&real)) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => p.display().to_string(),
        }
    }
}

// ---------- a server, in no CLI's format ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    #[default]
    Stdio,
    Http,
    Sse,
    Ws,
}

/// An env or header value.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    /// Used as written.
    Lit(String),
    /// The whole value is one environment variable.
    Var(String),
    /// `Bearer <variable>`, for an Authorization header.
    Bearer(String),
    /// Text with variables in it, in the source CLI's syntax.
    Template(String),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Def {
    /// The CLI whose syntax the strings are in.
    pub flavor: Option<AgentId>,
    pub transport: Transport,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, Val)>,
    pub cwd: Option<String>,
    pub url: String,
    pub headers: Vec<(String, Val)>,
    /// Turned off in the source (Codex `enabled = false`).
    pub off: bool,
    /// Settings with no counterpart that are not copied (timeouts, tool filters, trust...).
    pub left_out: Vec<String>,
}

fn is_ident(s: &str) -> bool {
    !s.is_empty() && !s.starts_with(|c: char| c.is_ascii_digit()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether `s` would be expanded by the CLI that wrote it.
fn expands(flavor: AgentId, s: &str) -> bool {
    match flavor {
        Codex => false,
        Claude => s.contains("${"),
        Gemini => s.char_indices().any(|(i, c)| c == '$' && s[i + 1..].starts_with(|n: char| n == '{' || n == '_' || n.is_ascii_alphabetic())),
        OpenCode => s.contains("{env:") || s.contains("{file:"),
    }
}

/// A string split into plain text and variables, in no CLI's syntax.
#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Text(String),
    Var(String),
}

/// `s` as the CLI that wrote it reads it. Err names the part no other CLI can write.
fn pieces(flavor: AgentId, s: &str) -> Result<Vec<Piece>, String> {
    let mut out = vec![];
    let (mut text, mut rest) = (String::new(), s);
    let flush = |text: &mut String, out: &mut Vec<Piece>| {
        if !text.is_empty() {
            out.push(Piece::Text(std::mem::take(text)));
        }
    };
    while !rest.is_empty() {
        let found: Option<(usize, usize, String)> = match flavor {
            Codex => None,
            Claude | Gemini => rest.match_indices('$').find_map(|(i, _)| {
                let after = &rest[i + 1..];
                if let Some(inner) = after.strip_prefix('{') {
                    let end = inner.find('}')?;
                    Some((i, i + 3 + end, inner[..end].to_string()))
                } else if flavor == Gemini {
                    let n = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
                    (n > 0 && !after.starts_with(|c: char| c.is_ascii_digit())).then(|| (i, i + 1 + n, after[..n].to_string()))
                } else {
                    None
                }
            }),
            OpenCode => {
                let i = [rest.find("{env:"), rest.find("{file:")].into_iter().flatten().min();
                i.and_then(|i| rest[i..].find('}').map(|e| (i, i + e + 1, rest[i + 1..i + e].to_string())))
            }
        };
        let Some((start, end, inner)) = found else {
            text.push_str(rest);
            break;
        };
        text.push_str(&rest[..start]);
        let name = match flavor {
            OpenCode => inner.strip_prefix("env:").filter(|n| is_ident(n)),
            _ => Some(inner.as_str()).filter(|n| is_ident(n)),
        };
        match name {
            Some(n) => {
                flush(&mut text, &mut out);
                out.push(Piece::Var(n.to_string()));
            }
            // A default (`${X:-y}`), a file, or a name OpenCode would look up as it is.
            None => return Err(rest[start..end].to_string()),
        }
        rest = &rest[end..];
    }
    flush(&mut text, &mut out);
    Ok(out)
}

/// `s`, written by `from`, in `to`'s variable syntax. Only for moves to or from OpenCode; the
/// other CLIs keep their own rules in `check`. Err says why it can't be written.
fn convert(from: AgentId, to: AgentId, s: &str) -> Result<String, String> {
    if from == to {
        return Ok(s.to_string());
    }
    let parts = pieces(from, s).map_err(|bad| match from {
        OpenCode if bad.starts_with("{file:") => format!("OpenCode reads {bad} from a file, and {} can't do that.", to.label()),
        _ => format!("{} has no way to write {bad} (from {}).", to.label(), from.label()),
    })?;
    let mut out = String::new();
    for p in parts {
        match p {
            Piece::Var(n) => match to {
                Codex => return Err(format!("Codex doesn't fill in variables inside text, and this uses {n}.")),
                OpenCode => out.push_str(&format!("{{env:{n}}}")),
                _ => out.push_str(&format!("${{{n}}}")),
            },
            Piece::Text(t) if expands(to, &t) => {
                return Err(match to {
                    OpenCode => format!("OpenCode would read the {{env:...}} or {{file:...}} in {s} as a variable, so it can't be copied as it is."),
                    _ => format!("{} would read the $ in {s} as a variable, so it can't be copied as it is.", to.label()),
                });
            }
            Piece::Text(t) => out.push_str(&t),
        }
    }
    Ok(out)
}

/// Whether moving between these two CLIs goes through `convert`.
fn converts(from: AgentId, to: AgentId) -> bool {
    from != to && (from == OpenCode || to == OpenCode)
}

fn parse_val(flavor: AgentId, s: &str, header: bool) -> Val {
    let var = |t: &str| -> Option<String> {
        if flavor == Codex {
            return None;
        }
        if flavor == OpenCode {
            return t.strip_prefix("{env:").and_then(|r| r.strip_suffix('}')).filter(|n| is_ident(n)).map(str::to_string);
        }
        if let Some(inner) = t.strip_prefix("${").and_then(|r| r.strip_suffix('}'))
            && is_ident(inner)
        {
            return Some(inner.into());
        }
        if flavor == Gemini
            && let Some(inner) = t.strip_prefix('$')
            && is_ident(inner)
        {
            return Some(inner.into());
        }
        None
    };
    if let Some(n) = var(s) {
        return Val::Var(n);
    }
    if header && let Some(n) = s.strip_prefix("Bearer ").and_then(var) {
        return Val::Bearer(n);
    }
    if expands(flavor, s) { Val::Template(s.into()) } else { Val::Lit(s.into()) }
}

fn text(v: &Value) -> String {
    v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())
}

/// An OpenCode `mcp` entry.
pub fn from_opencode(v: &Value) -> Result<Def, String> {
    let o = v.as_object().ok_or("It isn't a JSON object.")?;
    let s = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
    let known = ["type", "command", "environment", "cwd", "url", "headers", "enabled"];
    let mut d = Def { flavor: Some(OpenCode), left_out: o.keys().filter(|k| !known.contains(&k.as_str())).cloned().collect(), ..Def::default() };
    d.off = o.get("enabled").and_then(Value::as_bool) == Some(false);
    match s("type").as_deref() {
        Some("local") => {
            let cmd: Vec<String> = o.get("command").and_then(Value::as_array).into_iter().flatten().map(text).collect();
            let (first, rest) = cmd.split_first().ok_or("Its command is empty.")?;
            d.command = first.clone();
            d.args = rest.to_vec();
            d.cwd = s("cwd");
            for (k, val) in o.get("environment").and_then(Value::as_object).into_iter().flatten() {
                d.env.push((k.clone(), parse_val(OpenCode, &text(val), false)));
            }
        }
        Some("remote") => {
            d.url = s("url").ok_or("It's a remote server with no url.")?;
            // OpenCode tries streamable HTTP first and falls back to SSE.
            d.transport = Transport::Http;
            for (k, val) in o.get("headers").and_then(Value::as_object).into_iter().flatten() {
                d.headers.push((k.clone(), parse_val(OpenCode, &text(val), true)));
            }
        }
        None if o.len() == 1 && o.contains_key("enabled") => return Err("It only switches a server from another OpenCode config on or off.".into()),
        None => return Err("It has no type, so OpenCode skips it.".into()),
        Some(t) => return Err(format!("Its type {t} isn't one OpenCode knows.")),
    }
    Ok(d)
}

/// A Claude Code or Gemini CLI `mcpServers` entry, or an OpenCode `mcp` entry.
pub fn from_json(flavor: AgentId, v: &Value) -> Result<Def, String> {
    if flavor == OpenCode {
        return from_opencode(v);
    }
    let o = v.as_object().ok_or("It isn't a JSON object.")?;
    let s = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
    let known: &[&str] = match flavor {
        Gemini => &["type", "command", "args", "env", "cwd", "url", "httpUrl", "headers"],
        _ => &["type", "command", "args", "env", "url", "headers"],
    };
    let mut d = Def { flavor: Some(flavor), left_out: o.keys().filter(|k| !known.contains(&k.as_str())).cloned().collect(), ..Def::default() };
    let ty = s("type");
    if let Some(cmd) = s("command") {
        if flavor == Claude && !matches!(ty.as_deref(), None | Some("stdio")) {
            return Err(format!("It has a command but its type is {}.", ty.unwrap_or_default()));
        }
        d.command = cmd;
        d.args = o.get("args").and_then(Value::as_array).into_iter().flatten().map(text).collect();
        d.cwd = if flavor == Gemini { s("cwd") } else { None };
    } else if flavor == Gemini {
        if let Some(u) = s("httpUrl") {
            d.url = u;
            d.transport = Transport::Http;
        } else if let Some(u) = s("url") {
            d.url = u;
            // Gemini 0.65 connects to a url with no type over streamable HTTP.
            d.transport = if ty.as_deref() == Some("sse") { Transport::Sse } else { Transport::Http };
        } else {
            return Err("It has no command, url or httpUrl.".into());
        }
    } else {
        d.url = s("url").ok_or("It has no command or url.")?;
        d.transport = match ty.as_deref() {
            Some("http") | Some("streamable-http") => Transport::Http,
            Some("sse") => Transport::Sse,
            Some("ws") => Transport::Ws,
            None => return Err("It has a url but no type, so Claude Code skips it.".into()),
            Some(t) => return Err(format!("Its type {t} isn't one NekoTrail knows.")),
        };
    }
    for (k, val) in o.get("env").and_then(Value::as_object).into_iter().flatten() {
        d.env.push((k.clone(), parse_val(flavor, &text(val), false)));
    }
    if d.transport != Transport::Stdio {
        for (k, val) in o.get("headers").and_then(Value::as_object).into_iter().flatten() {
            d.headers.push((k.clone(), parse_val(flavor, &text(val), true)));
        }
    }
    Ok(d)
}

/// A Codex `[mcp_servers.<name>]` table.
pub fn from_toml(t: &toml::Table) -> Result<Def, String> {
    let s = |k: &str| t.get(k).and_then(toml::Value::as_str).map(str::to_string);
    let tstr = |v: &toml::Value| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
    let map = |k: &str| t.get(k).and_then(toml::Value::as_table).into_iter().flatten().map(|(k, v)| (k.clone(), tstr(v)));
    let known = ["command", "args", "env", "env_vars", "cwd", "url", "bearer_token_env_var", "http_headers", "env_http_headers", "enabled"];
    let mut d = Def { flavor: Some(Codex), left_out: t.keys().filter(|k| !known.contains(&k.as_str())).cloned().collect(), ..Def::default() };
    d.off = t.get("enabled").and_then(toml::Value::as_bool) == Some(false);
    if let Some(cmd) = s("command") {
        d.command = cmd;
        d.args = t.get("args").and_then(toml::Value::as_array).into_iter().flatten().map(tstr).collect();
        d.cwd = s("cwd");
        d.env = map("env").map(|(k, v)| (k, Val::Lit(v))).collect();
        for v in t.get("env_vars").and_then(toml::Value::as_array).into_iter().flatten() {
            match v.as_str().or_else(|| v.get("name").and_then(toml::Value::as_str)) {
                Some(n) if !d.env.iter().any(|(k, _)| k == n) => d.env.push((n.into(), Val::Var(n.into()))),
                Some(_) => {}
                None => d.left_out.push("env_vars".into()),
            }
        }
    } else if let Some(url) = s("url") {
        d.url = url;
        d.transport = Transport::Http;
        d.headers = map("http_headers").map(|(k, v)| (k, Val::Lit(v))).collect();
        d.headers.extend(map("env_http_headers").map(|(k, v)| (k, Val::Var(v))));
        if let Some(b) = s("bearer_token_env_var") {
            d.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
            d.headers.push(("Authorization".into(), Val::Bearer(b)));
        }
    } else {
        return Err("It has no command or url.".into());
    }
    Ok(d)
}

// ---------- secrets ----------

const SECRET_NAMES: [&str; 12] = ["TOKEN", "SECRET", "PASSWORD", "PASSWD", "KEY", "AUTH", "CREDENTIAL", "PRIVATE", "BEARER", "COOKIE", "SESSION", "SIGNATURE"];
const SECRET_PREFIXES: [&str; 11] = ["sk-", "sk_", "ghp_", "gho_", "github_pat_", "xox", "glpat-", "AKIA", "AIza", "pk_", "rk_"];

fn looks_like_path(v: &str) -> bool {
    v.starts_with('/') || v.starts_with("~/") || v.starts_with("./") || v.starts_with("../")
}

/// A value that reads like a token on its own: a known prefix, or 20+ characters of letters and
/// digits with no spaces that isn't a path or a URL.
fn token_like(v: &str) -> bool {
    let v = v.trim();
    if v.is_empty() || looks_like_path(v) {
        return false;
    }
    if let Some(rest) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) {
        return !rest.trim().is_empty();
    }
    if SECRET_PREFIXES.iter().any(|p| v.starts_with(p)) && v.len() >= 12 {
        return true;
    }
    if v.contains("://") {
        return url_secret(v);
    }
    // Package names and paths have / @ . in them; tokens are letters, digits, - and _ with lots
    // of digits or mixed case.
    let digits = v.chars().filter(|c| c.is_ascii_digit()).count();
    let mixed = v.chars().any(|c| c.is_ascii_uppercase()) && v.chars().any(|c| c.is_ascii_lowercase());
    v.len() >= 20 && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && v.chars().any(|c| c.is_ascii_alphabetic()) && (digits >= 4 || (mixed && digits >= 1))
}

fn url_secret(u: &str) -> bool {
    let lower = u.to_ascii_lowercase();
    let userinfo = lower.split("://").nth(1).and_then(|r| r.split('/').next()).is_some_and(|host| host.contains('@'));
    let query = lower.split_once('?').map(|(_, q)| q).unwrap_or("");
    userinfo || ["token", "key", "secret", "sig", "password", "auth"].iter().any(|k| query.contains(k))
}

/// An env or header value that should not be copied without asking.
pub fn looks_secret(key: &str, value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() || looks_like_path(v) || matches!(v, "true" | "false") || v.parse::<f64>().is_ok() {
        return false;
    }
    let k = key.to_ascii_uppercase().replace('-', "_");
    SECRET_NAMES.iter().any(|n| k.contains(n)) || token_like(v)
}

/// One value that needs your say before it is copied.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Secret {
    /// `env:NAME`, `header:Name`, `arg:3` or `url`.
    pub id: String,
    pub kind: &'static str,
    pub key: String,
    /// Can be brought across as a variable instead of the value.
    pub can_ref: bool,
}

fn secret_flag(a: &str) -> bool {
    let a = a.trim_start_matches('-').to_ascii_uppercase().replace('-', "_");
    a.len() < 40 && SECRET_NAMES.iter().any(|n| a.contains(n))
}

pub fn secrets(d: &Def) -> Vec<Secret> {
    let mut out = vec![];
    for (k, v) in &d.env {
        if let Val::Lit(v) = v && looks_secret(k, v) {
            out.push(Secret { id: format!("env:{k}"), kind: "env", key: k.clone(), can_ref: true });
        }
    }
    for (k, v) in &d.headers {
        if let Val::Lit(v) = v && looks_secret(k, v) {
            out.push(Secret { id: format!("header:{k}"), kind: "header", key: k.clone(), can_ref: true });
        }
    }
    for (i, a) in d.args.iter().enumerate() {
        let after_flag = i > 0 && d.args[i - 1].starts_with('-') && secret_flag(&d.args[i - 1]) && !a.starts_with('-');
        let inline = a.starts_with('-') && a.split_once('=').is_some_and(|(f, v)| secret_flag(f) && !v.is_empty());
        if (after_flag || inline || token_like(a)) && !expands(d.flavor.unwrap_or(Codex), a) {
            out.push(Secret { id: format!("arg:{i}"), kind: "arg", key: a.split_once('=').map(|(f, _)| f.to_string()).unwrap_or_else(|| format!("argument {}", i + 1)), can_ref: false });
        }
    }
    if !d.url.is_empty() && url_secret(&d.url) {
        out.push(Secret { id: "url".into(), kind: "url", key: "url".into(), can_ref: false });
    }
    out
}

fn mask(v: &str) -> String {
    format!("<hidden: your value, {} characters>", v.chars().count())
}

/// The variable a secret header becomes: `LOGFIRE_AUTHORIZATION` for `logfire`'s Authorization.
fn header_var(server: &str, header: &str) -> String {
    format!("{server}_{header}").chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect()
}

/// Display form of a server: the command line or url, with secrets hidden.
fn summary(d: &Def) -> String {
    let hidden: Vec<String> = secrets(d).into_iter().map(|s| s.id).collect();
    if d.transport == Transport::Stdio {
        std::iter::once(d.command.clone())
            .chain(d.args.iter().enumerate().map(|(i, a)| {
                if !hidden.contains(&format!("arg:{i}")) {
                    a.clone()
                } else if let Some((f, _)) = a.split_once('=') {
                    format!("{f}=••••")
                } else {
                    "••••".into()
                }
            }))
            .collect::<Vec<_>>()
            .join(" ")
    } else if hidden.iter().any(|h| h == "url") {
        format!("{}?••••", d.url.split('?').next().unwrap_or(""))
    } else {
        d.url.clone()
    }
}

// ---------- converting ----------

/// Why `d` can't go to `target` as it is, and what to know if it can.
fn check(d: &Def, target: AgentId, name: &str) -> (Vec<String>, Vec<String>) {
    let (mut no, mut notes) = (vec![], vec![]);
    let to = target.label();
    let from = d.flavor.unwrap_or(Codex);
    match (target, d.transport) {
        (Codex, Transport::Sse) => no.push("Codex only connects to command servers and streamable HTTP servers. This one uses SSE, so Codex can't use it.".into()),
        (_, Transport::Ws) if target != Claude => no.push(format!("This server uses WebSocket, which {to} doesn't support.")),
        _ => {}
    }
    if target == Codex && !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        no.push("Codex server names can only use letters, numbers, - and _.".into());
    }
    if target == Claude && d.cwd.is_some() {
        no.push(format!("This server starts in {}, and Claude Code has no setting for an MCP server's working folder.", d.cwd.as_deref().unwrap_or_default()));
    }
    // OpenCode starts a relative cwd from the workspace; the others don't say, so only full paths move.
    if converts(from, target)
        && target != Claude
        && let Some(c) = &d.cwd
        && !c.starts_with('/')
    {
        no.push(format!("It starts in {c}, a folder relative to where {} runs, so it could start somewhere else in {to}. Change it to a full path first.", from.label()));
    }
    // Text with variables in it means something else in a CLI that expands differently.
    let templated: Vec<&str> = std::iter::once(d.command.as_str()).chain(d.args.iter().map(String::as_str)).chain([d.url.as_str()]).chain(d.cwd.as_deref()).filter(|s| expands(from, s)).collect();
    if converts(from, target) && target != Codex {
        // To or from OpenCode: every string is rewritten in the other syntax, or refused.
        let values = d.env.iter().chain(&d.headers).filter_map(|(_, v)| match v {
            Val::Lit(s) | Val::Template(s) => Some(s.as_str()),
            _ => None,
        });
        for s in std::iter::once(d.command.as_str()).chain(d.args.iter().map(String::as_str)).chain([d.url.as_str()]).chain(d.cwd.as_deref()).chain(values) {
            if let Err(e) = convert(from, target, s) {
                no.push(e);
            }
        }
        if target == Gemini && !templated.is_empty() {
            no.push(format!("Gemini CLI fills in variables only in env and headers, and this server uses {}.", templated[0]));
        }
    } else if target != from && !templated.is_empty() {
        match target {
            Codex => no.push(format!("Codex doesn't fill in variables in commands or urls, and this server uses {}.", templated[0])),
            Claude if from == Gemini && templated.iter().any(|s| !s.contains("${")) => no.push(format!("Claude Code only fills in variables written as ${{NAME}}, and this server uses {}.", templated[0])),
            _ => {}
        }
    }
    for (k, v) in d.env.iter().chain(&d.headers) {
        match (target, v) {
            (Codex, Val::Template(t)) => no.push(format!("Codex can't fill in {k}={t}: it only passes a variable on whole.")),
            (Claude, Val::Template(t)) if from == Gemini && !t.contains("${") => no.push(format!("Claude Code only fills in variables written as ${{NAME}}, and {k} uses {t}.")),
            _ => {}
        }
    }
    for (k, v) in &d.env {
        if target == Codex && let Val::Var(n) = v && n != k {
            no.push(format!("Codex passes a variable on under its own name only, so {k}=${{{n}}} can't move. Set {k} in your shell profile, change it there, and try again."));
        }
    }
    for (k, v) in &d.headers {
        if target == Codex && matches!(v, Val::Bearer(_)) && !k.eq_ignore_ascii_case("authorization") {
            no.push(format!("Codex can add \"Bearer\" to the Authorization header only, not to {k}."));
        }
    }
    let vars: Vec<&str> = d.env.iter().chain(&d.headers).filter_map(|(_, v)| match v {
        Val::Var(n) | Val::Bearer(n) => Some(n.as_str()),
        _ => None,
    }).collect();
    if !vars.is_empty() {
        notes.push(format!("{to} reads {} from the environment it starts in, so set {} in your shell profile.", vars.join(", "), if vars.len() == 1 { "it" } else { "them" }));
        let hidden: Vec<&str> = vars.iter().copied().filter(|n| { let u = n.to_ascii_uppercase(); ["TOKEN", "SECRET", "PASSWORD", "PASSWD", "KEY", "AUTH", "CREDENTIAL", "CREDS", "PRIVATE", "CERT"].iter().any(|p| u.contains(p)) }).collect();
        if target == Gemini && !hidden.is_empty() {
            notes.push(format!("Gemini CLI keeps variables with names like {} away from MCP servers unless you list them under security.environmentVariableRedaction.allowed in its settings. Until then they arrive empty.", hidden.join(", ")));
        }
        if target == OpenCode {
            notes.push("If one isn't set, OpenCode fills in an empty value instead of stopping.".into());
        }
    }
    if target == OpenCode && d.transport == Transport::Sse {
        notes.push("OpenCode tries streamable HTTP first and falls back to SSE, so it needs no type for this.".into());
    }
    if target == Gemini && d.transport == Transport::Stdio {
        notes.push("Gemini CLI starts command servers only in folders you trust.".into());
    }
    if d.transport != Transport::Stdio {
        notes.push(match target {
            Claude => format!("If {name} asks you to sign in, do it once in Claude Code with /mcp."),
            Codex => format!("If {name} asks you to sign in, run `codex mcp login {name}` once."),
            Gemini => format!("If {name} asks you to sign in, do it once in Gemini CLI with /mcp auth {name}."),
            OpenCode => format!("If {name} asks you to sign in, run `opencode mcp auth {name}` once."),
        });
    }
    if d.off {
        notes.push(format!("It's switched off where it comes from. In {to} it will be on."));
    }
    if !d.left_out.is_empty() {
        notes.push(format!("Not copied, because {to} has no matching setting or it only makes sense where it is: {}.", d.left_out.join(", ")));
    }
    let mut seen = std::collections::HashSet::new();
    no.retain(|n| seen.insert(n.clone()));
    (no, notes)
}

/// Brings secrets across the way you chose. `mask` hides copied values (for the preview).
fn with_choices(d: &Def, name: &str, choices: &HashMap<String, String>, mask_values: bool) -> (Def, Vec<String>, Vec<String>) {
    let (mut out, mut no, mut notes) = (d.clone(), vec![], vec![]);
    for s in secrets(d) {
        let choice = choices.get(&s.id).map(String::as_str);
        let copy = choice == Some("copy");
        let reference = choice == Some("ref") && s.can_ref;
        if !copy && !reference {
            no.push(match s.kind {
                "env" | "header" => format!("{} looks like a secret. Choose whether to bring it across as a variable or copy the value.", s.key),
                "url" => "The url looks like it has a secret in it. Tick Copy to bring it across as it is.".into(),
                _ => format!("The command line has what looks like a secret ({}). Tick Copy to bring it across as it is.", s.key),
            });
            continue;
        }
        match s.kind {
            "env" => {
                let slot = out.env.iter_mut().find(|(k, _)| *k == s.key).unwrap();
                if reference {
                    slot.1 = Val::Var(s.key.clone());
                } else if mask_values && let Val::Lit(v) = &slot.1 {
                    slot.1 = Val::Lit(mask(v));
                }
            }
            "header" => {
                let slot = out.headers.iter_mut().find(|(k, _)| *k == s.key).unwrap();
                if reference {
                    let var = header_var(name, &s.key);
                    let bearer = matches!(&slot.1, Val::Lit(v) if v.starts_with("Bearer "));
                    notes.push(format!("Put the value of {} in a variable called {var} in your shell profile (export {var}=...){}.", s.key, if bearer { ", without the word Bearer" } else { "" }));
                    slot.1 = if bearer { Val::Bearer(var) } else { Val::Var(var) };
                } else if mask_values && let Val::Lit(v) = &slot.1 {
                    slot.1 = Val::Lit(mask(v));
                }
            }
            "url" if mask_values => out.url = format!("{}?<hidden>", out.url.split('?').next().unwrap_or("")),
            "arg" if mask_values => {
                let i: usize = s.id[4..].parse().unwrap();
                out.args[i] = match out.args[i].split_once('=') {
                    Some((f, v)) if f.starts_with('-') => format!("{f}={}", mask(v)),
                    _ => mask(&out.args[i]),
                };
            }
            _ => {}
        }
    }
    (out, no, notes)
}

/// `${NAME}` in Claude Code and Gemini CLI.
fn json_val(v: &Val) -> String {
    match v {
        Val::Lit(s) | Val::Template(s) => s.clone(),
        Val::Var(n) => format!("${{{n}}}"),
        Val::Bearer(n) => format!("Bearer ${{{n}}}"),
    }
}

/// `{env:NAME}` in OpenCode.
fn opencode_val(v: &Val) -> String {
    match v {
        Val::Lit(s) | Val::Template(s) => s.clone(),
        Val::Var(n) => format!("{{env:{n}}}"),
        Val::Bearer(n) => format!("Bearer {{env:{n}}}"),
    }
}

/// The entry for Claude Code, Gemini CLI or OpenCode.
pub fn to_json(d: &Def, target: AgentId) -> Value {
    if target == OpenCode {
        return to_opencode(d);
    }
    let from = d.flavor.unwrap_or(Codex);
    // Plain text and variables in the source's syntax, written in Claude's and Gemini's `${NAME}`.
    let s = |t: &str| if converts(from, target) { convert(from, target, t).unwrap_or_else(|_| t.to_string()) } else { t.to_string() };
    let pairs = |p: &[(String, Val)]| {
        Value::Object(p.iter().map(|(k, v)| (k.clone(), Value::String(match v {
            Val::Lit(t) | Val::Template(t) => s(t),
            other => json_val(other),
        }))).collect())
    };
    let mut o = serde_json::Map::new();
    if d.transport == Transport::Stdio {
        if target == Claude {
            o.insert("type".into(), json!("stdio"));
        }
        o.insert("command".into(), json!(s(&d.command)));
        if !d.args.is_empty() {
            o.insert("args".into(), json!(d.args.iter().map(|a| s(a)).collect::<Vec<_>>()));
        }
        if !d.env.is_empty() {
            o.insert("env".into(), pairs(&d.env));
        }
        if target == Gemini && let Some(c) = &d.cwd {
            o.insert("cwd".into(), json!(s(c)));
        }
    } else {
        o.insert("type".into(), json!(match d.transport {
            Transport::Sse => "sse",
            Transport::Ws => "ws",
            _ => "http",
        }));
        o.insert("url".into(), json!(s(&d.url)));
        if !d.headers.is_empty() {
            o.insert("headers".into(), pairs(&d.headers));
        }
    }
    Value::Object(o)
}

/// The `mcp.<name>` entry for OpenCode.
pub fn to_opencode(d: &Def) -> Value {
    let from = d.flavor.unwrap_or(Codex);
    let s = |t: &str| convert(from, OpenCode, t).unwrap_or_else(|_| t.to_string());
    let pairs = |p: &[(String, Val)]| {
        Value::Object(p.iter().map(|(k, v)| (k.clone(), Value::String(match v {
            Val::Lit(t) | Val::Template(t) => s(t),
            other => opencode_val(other),
        }))).collect())
    };
    let mut o = serde_json::Map::new();
    if d.transport == Transport::Stdio {
        o.insert("type".into(), json!("local"));
        o.insert("command".into(), json!(std::iter::once(&d.command).chain(&d.args).map(|a| s(a)).collect::<Vec<_>>()));
        if !d.env.is_empty() {
            o.insert("environment".into(), pairs(&d.env));
        }
        if let Some(c) = &d.cwd {
            o.insert("cwd".into(), json!(s(c)));
        }
    } else {
        o.insert("type".into(), json!("remote"));
        o.insert("url".into(), json!(s(&d.url)));
        if !d.headers.is_empty() {
            o.insert("headers".into(), pairs(&d.headers));
        }
    }
    Value::Object(o)
}

/// The `[mcp_servers.<name>]` table for Codex.
pub fn to_toml(d: &Def) -> toml_edit::Table {
    use toml_edit::{value, Array, Item, Table};
    let mut t = Table::new();
    let sub = |pairs: Vec<(&String, String)>| {
        let mut s = Table::new();
        for (k, v) in pairs {
            s[k.as_str()] = value(v);
        }
        Item::Table(s)
    };
    if d.transport == Transport::Stdio {
        t["command"] = value(d.command.clone());
        if !d.args.is_empty() {
            t["args"] = value(d.args.iter().collect::<Array>());
        }
        if let Some(c) = &d.cwd {
            t["cwd"] = value(c.clone());
        }
        let passed: Vec<&String> = d.env.iter().filter_map(|(k, v)| matches!(v, Val::Var(_)).then_some(k)).collect();
        if !passed.is_empty() {
            t["env_vars"] = value(passed.into_iter().collect::<Array>());
        }
        let lit: Vec<(&String, String)> = d.env.iter().filter_map(|(k, v)| match v {
            Val::Lit(s) => Some((k, s.clone())),
            _ => None,
        }).collect();
        if !lit.is_empty() {
            t.insert("env", sub(lit));
        }
    } else {
        t["url"] = value(d.url.clone());
        for (k, v) in &d.headers {
            if let Val::Bearer(n) = v && k.eq_ignore_ascii_case("authorization") {
                t["bearer_token_env_var"] = value(n.clone());
            }
        }
        let lit: Vec<(&String, String)> = d.headers.iter().filter_map(|(k, v)| match v {
            Val::Lit(s) => Some((k, s.clone())),
            _ => None,
        }).collect();
        if !lit.is_empty() {
            t.insert("http_headers", sub(lit));
        }
        let vars: Vec<(&String, String)> = d.headers.iter().filter_map(|(k, v)| match v {
            Val::Var(n) => Some((k, n.clone())),
            _ => None,
        }).collect();
        if !vars.is_empty() {
            t.insert("env_http_headers", sub(vars));
        }
    }
    t
}

// ---------- editing JSON without rewriting it ----------

/// The same text with comments and trailing commas turned into spaces, so it parses as JSON and
/// every byte offset still matches the original.
pub fn blank_jsonc(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = b.to_vec();
    let (mut i, mut in_str) = (0, false);
    while i < b.len() {
        let c = b[i];
        if in_str {
            match c {
                b'\\' => i += 1,
                b'"' => in_str = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
            continue;
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                if b[i] != b'\n' {
                    out[i] = b' ';
                }
                i += 1;
            }
            for o in out.iter_mut().skip(i).take(2) {
                *o = b' ';
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    // Trailing commas: a comma with only whitespace before the closing bracket.
    let (mut i, mut in_str) = (0, false);
    while i < out.len() {
        let c = out[i];
        if in_str {
            match c {
                b'\\' => i += 1,
                b'"' => in_str = false,
                _ => {}
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b',' {
            let next = out[i + 1..].iter().find(|c| !c.is_ascii_whitespace());
            if matches!(next, Some(b'}') | Some(b']')) {
                out[i] = b' ';
            }
        }
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn skip_string(b: &[u8], mut i: usize) -> Option<usize> {
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// End (exclusive) of the JSON value starting at `i`, in blanked text.
fn skip_value(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'"' => skip_string(b, i),
        open @ (b'{' | b'[') => {
            let close = if open == b'{' { b'}' } else { b']' };
            let mut j = skip_ws(b, i + 1);
            if b.get(j) == Some(&close) {
                return Some(j + 1);
            }
            loop {
                if open == b'{' {
                    j = skip_ws(b, skip_string(b, j)?);
                    if b.get(j) != Some(&b':') {
                        return None;
                    }
                    j = skip_ws(b, j + 1);
                }
                j = skip_ws(b, skip_value(b, j)?);
                match b.get(j)? {
                    b',' => j = skip_ws(b, j + 1),
                    c if *c == close => return Some(j + 1),
                    _ => return None,
                }
            }
        }
        _ => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']') && !b[j].is_ascii_whitespace() {
                j += 1;
            }
            (j > i).then_some(j)
        }
    }
}

/// Start and end of the root object, and of the value of its last `key` member.
/// Byte range `start..end` in the file.
type Span = (usize, usize);

fn root_spans(blank: &str, key: &str) -> Option<(Span, Option<Span>)> {
    let b = blank.as_bytes();
    let start = skip_ws(b, 0);
    if b.get(start) != Some(&b'{') {
        return None;
    }
    let end = skip_value(b, start)?;
    let (mut found, mut j) = (None, skip_ws(b, start + 1));
    while j < end - 1 {
        let ke = skip_string(b, j)?;
        let k: String = serde_json::from_str(&blank[j..ke]).ok()?;
        let vs = skip_ws(b, skip_ws(b, ke) + 1);
        let ve = skip_value(b, vs)?;
        if k == key {
            found = Some((vs, ve));
        }
        j = skip_ws(b, ve);
        if b.get(j) == Some(&b',') {
            j = skip_ws(b, j + 1);
        }
    }
    Some(((start, end), found))
}

fn line_indent(s: &str, pos: usize) -> String {
    let ls = s[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    s[ls..].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

/// The order the CLIs' own docs write a server's keys in; anything else follows, A to Z.
const KEY_ORDER: [&str; 9] = ["type", "command", "args", "env", "environment", "cwd", "url", "httpUrl", "headers"];

/// Writes a JSON value with server keys in `KEY_ORDER` (serde_json's map is sorted A to Z).
struct InOrder<'a>(&'a Value);

impl Serialize for InOrder<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self.0 {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort_by_key(|k| (KEY_ORDER.iter().position(|o| o == k).unwrap_or(KEY_ORDER.len()), k.to_string()));
                let mut map = s.serialize_map(Some(m.len()))?;
                for k in keys {
                    map.serialize_entry(k, &InOrder(&m[k]))?;
                }
                map.end()
            }
            other => other.serialize(s),
        }
    }
}

fn pretty(v: &Value, unit: &str, indent: &str) -> String {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, serde_json::ser::PrettyFormatter::with_indent(unit.as_bytes()));
    InOrder(v).serialize(&mut ser).unwrap();
    String::from_utf8(buf).unwrap().replace('\n', &format!("\n{indent}"))
}

/// `orig` with `"key": value` added as the last member of the object at `s..e`.
fn insert_member(orig: &str, blank: &str, (s, e): (usize, usize), key: &str, val: &Value, unit: &str) -> String {
    let close = e - 1;
    let k = serde_json::to_string(key).unwrap();
    let inner = &blank[s + 1..close];
    match (inner.find(|c: char| !c.is_whitespace()), inner.rfind(|c: char| !c.is_whitespace())) {
        (Some(first), Some(last)) => {
            let (first, last_end) = (s + 1 + first, s + 1 + last + 1);
            if orig[s..first].contains('\n') {
                let mi = line_indent(orig, first);
                format!("{},\n{mi}{k}: {}{}", &orig[..last_end], pretty(val, unit, &mi), &orig[last_end..])
            } else {
                format!("{},{k}:{}{}", &orig[..last_end], serde_json::to_string(&InOrder(val)).unwrap(), &orig[last_end..])
            }
        }
        _ => {
            let pi = line_indent(orig, s);
            let mi = format!("{pi}{unit}");
            format!("{}\n{mi}{k}: {}\n{pi}{}", orig[..close].trim_end(), pretty(val, unit, &mi), &orig[close..])
        }
    }
}

/// `text` with `mcpServers.<name>` added and nothing else changed. Refuses a name that is there.
pub fn json_add(text: &str, name: &str, val: &Value) -> Result<String, String> {
    json_add_at(text, "mcpServers", name, val)
}

/// `text` with `<key>.<name>` added at the top level (`mcp` for OpenCode), nothing else changed.
pub fn json_add_at(text: &str, key: &str, name: &str, val: &Value) -> Result<String, String> {
    if text.trim().is_empty() {
        return Ok(format!("{}\n", pretty(&json!({ key: { name: val } }), "  ", "")));
    }
    let blank = blank_jsonc(text);
    let before: Value = serde_json::from_str(&blank).map_err(|e| format!("It isn't valid JSON ({e}), so NekoTrail won't change it."))?;
    let root = before.as_object().ok_or("It isn't a JSON object, so NekoTrail won't change it.")?;
    let ((rs, re), servers) = root_spans(&blank, key).ok_or("NekoTrail couldn't find its way around this file, so it won't change it.")?;
    let first = skip_ws(blank.as_bytes(), rs + 1);
    let unit = match line_indent(text, first) {
        u if u.is_empty() || !text[rs..first].contains('\n') => "  ".to_string(),
        u => u,
    };
    let new = match (root.get(key), servers) {
        (Some(Value::Object(m)), Some(span)) => {
            if m.contains_key(name) {
                return Err(format!("There's already a server called {name} there. NekoTrail won't replace it."));
            }
            insert_member(text, &blank, span, name, val, &unit)
        }
        (Some(_), _) => return Err(format!("Its {key} isn't a list of servers, so NekoTrail won't change it.")),
        (None, _) => insert_member(text, &blank, (rs, re), key, &json!({ name: val }), &unit),
    };
    // Parse the result: the new server is there, and everything else is exactly as before.
    let mut after: Value = serde_json::from_str(&blank_jsonc(&new)).map_err(|_| "NekoTrail couldn't add it without breaking the file, so it left it alone.")?;
    let servers = after.get_mut(key).and_then(Value::as_object_mut).ok_or("Couldn't add it safely.")?;
    if servers.remove(name).as_ref() != Some(val) {
        return Err("Couldn't add it safely.".into());
    }
    if servers.is_empty() && root.get(key).is_none() {
        after.as_object_mut().unwrap().remove(key);
    }
    if after != before {
        return Err("Adding it would have changed other settings, so NekoTrail left the file alone.".into());
    }
    Ok(new)
}

/// `text` with `[mcp_servers.<name>]` added through toml_edit: comments, order and formatting of
/// everything else stay as they are. Refuses a name that is there.
pub fn toml_add(text: &str, name: &str, server: toml_edit::Table) -> Result<String, String> {
    use toml_edit::{DocumentMut, Item, Table};
    let before: toml::Table = text.parse().map_err(|e| format!("It isn't valid TOML ({e}), so NekoTrail won't change it."))?;
    let mut doc: DocumentMut = text.parse().map_err(|e| format!("It isn't valid TOML ({e}), so NekoTrail won't change it."))?;
    let root = doc.as_table_mut();
    if !root.contains_key("mcp_servers") {
        let mut t = Table::new();
        t.set_implicit(true);
        root.insert("mcp_servers", Item::Table(t));
    }
    let servers = root["mcp_servers"].as_table_mut().ok_or("Its mcp_servers is written inline, so NekoTrail won't change it. Add the server by hand.")?;
    if servers.contains_key(name) {
        return Err(format!("There's already a server called {name} there. NekoTrail won't replace it."));
    }
    servers.insert(name, Item::Table(server.clone()));
    let new = doc.to_string();
    let want = toml_snippet(name, server).parse::<toml::Table>().map_err(|e| e.to_string())?;
    let want = &want["mcp_servers"][name];
    let mut after: toml::Table = new.parse().map_err(|_| "NekoTrail couldn't add it without breaking the file, so it left it alone.")?;
    let servers = after.get_mut("mcp_servers").and_then(toml::Value::as_table_mut).ok_or("Couldn't add it safely.")?;
    if servers.remove(name).as_ref() != Some(want) {
        return Err("Couldn't add it safely.".into());
    }
    if servers.is_empty() && !before.contains_key("mcp_servers") {
        after.remove("mcp_servers");
    }
    if after != before {
        return Err("Adding it would have changed other settings, so NekoTrail left the file alone.".into());
    }
    Ok(new)
}

fn toml_snippet(name: &str, server: toml_edit::Table) -> String {
    let mut doc = toml_edit::DocumentMut::new();
    let mut parent = toml_edit::Table::new();
    parent.set_implicit(true);
    parent.insert(name, toml_edit::Item::Table(server));
    doc.insert("mcp_servers", toml_edit::Item::Table(parent));
    doc.to_string()
}

fn snippet(target: AgentId, name: &str, d: &Def) -> String {
    match target {
        Codex => toml_snippet(name, to_toml(d)).trim_end().to_string(),
        _ => format!("{}: {}", serde_json::to_string(name).unwrap(), pretty(&to_json(d, target), "  ", "")),
    }
}

// ---------- what each CLI has ----------

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Has {
    pub agent: AgentId,
    /// "user", "local" (Claude, this folder only) or "project".
    pub scope: &'static str,
    pub file: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Target {
    pub agent: AgentId,
    pub ok: bool,
    pub why: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub name: String,
    pub transport: Option<Transport>,
    /// Command line or url, secrets hidden.
    pub summary: String,
    pub env: Vec<String>,
    pub has: Vec<Has>,
    /// The definition "Add to" copies.
    pub from: Has,
    pub secrets: Vec<Secret>,
    pub targets: Vec<Target>,
    pub problem: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Overview {
    pub servers: Vec<Row>,
    pub skills: Vec<SkillRow>,
    /// Each CLI's user config, for the footer.
    pub files: HashMap<AgentId, String>,
    /// The skills folders OpenCode reads, so the sheet can say it already sees those skills.
    pub opencode_skill_dirs: Vec<String>,
    /// Files that exist but can't be read.
    pub problems: Vec<String>,
}

fn read_text(p: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(p) {
        Ok(t) if t.trim().is_empty() => Ok(None),
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn read_json(p: &Path) -> Result<Option<Value>, String> {
    read_text(p)?.map(|t| serde_json::from_str(&blank_jsonc(&t)).map_err(|e| e.to_string())).transpose()
}

fn read_toml(p: &Path) -> Result<Option<toml::Table>, String> {
    read_text(p)?.map(|t| t.parse::<toml::Table>().map_err(|e| e.to_string())).transpose()
}

type Found = (String, Has, Result<Def, String>);

/// Every server every CLI has, in the order "Add to" prefers a source: user config first.
fn found(h: &Homes, ws: Option<&Path>, problems: &mut Vec<String>) -> Vec<Found> {
    let mut out: Vec<Found> = vec![];
    let json_file = |agent: AgentId, scope: &'static str, path: &Path, pick: &dyn Fn(&Value) -> Option<Value>, out: &mut Vec<Found>, problems: &mut Vec<String>| match read_json(path) {
        Ok(Some(v)) => {
            for (name, s) in pick(&v).as_ref().and_then(Value::as_object).into_iter().flatten() {
                out.push((name.clone(), Has { agent, scope, file: h.tilde(path) }, from_json(agent, s)));
            }
        }
        Ok(None) => {}
        Err(e) => problems.push(format!("Couldn't read {}: {e}", h.tilde(path))),
    };
    let top = |v: &Value| v.get("mcpServers").cloned();
    json_file(Claude, "user", &h.claude_json, &top, &mut out, problems);
    if let Some(ws) = ws {
        let key = ws.to_string_lossy().to_string();
        json_file(Claude, "local", &h.claude_json, &move |v: &Value| v.get("projects")?.get(&key)?.get("mcpServers").cloned(), &mut out, problems);
        json_file(Claude, "project", &ws.join(".mcp.json"), &top, &mut out, problems);
    }
    let toml_file = |scope: &'static str, path: &Path, out: &mut Vec<Found>, problems: &mut Vec<String>| match read_toml(path) {
        Ok(Some(t)) => {
            for (name, s) in t.get("mcp_servers").and_then(toml::Value::as_table).into_iter().flatten() {
                let def = s.as_table().ok_or_else(|| "It isn't a table.".to_string()).and_then(from_toml);
                out.push((name.clone(), Has { agent: Codex, scope, file: h.tilde(path) }, def));
            }
        }
        Ok(None) => {}
        Err(e) => problems.push(format!("Couldn't read {}: {e}", h.tilde(path))),
    };
    toml_file("user", &h.codex_config, &mut out, problems);
    if let Some(ws) = ws {
        toml_file("project", &ws.join(".codex/config.toml"), &mut out, problems);
    }
    json_file(Gemini, "user", &h.gemini_settings, &top, &mut out, problems);
    if let Some(ws) = ws {
        json_file(Gemini, "project", &ws.join(".gemini/settings.json"), &top, &mut out, problems);
    }
    // OpenCode: the file a later merge wins with comes first, so "Add to" copies what OpenCode runs.
    let mcp = |v: &Value| v.get("mcp").cloned();
    let mut user: Vec<PathBuf> = h.opencode_custom.iter().cloned().collect();
    user.extend(OPENCODE_GLOBAL.iter().rev().map(|f| h.opencode_dir.join(f)));
    for p in user {
        json_file(OpenCode, "user", &p, &mcp, &mut out, problems);
    }
    if let Some(ws) = ws {
        for f in [".opencode/opencode.jsonc", ".opencode/opencode.json", "opencode.jsonc", "opencode.json"] {
            json_file(OpenCode, "project", &ws.join(f), &mcp, &mut out, problems);
        }
    }
    out
}

fn user_readable(h: &Homes, a: AgentId) -> Result<(), String> {
    let p = h.user_file(a);
    let ok = match a {
        Codex => read_toml(&p).map(|_| ()),
        _ => read_json(&p).map(|_| ()),
    };
    ok.map_err(|_| format!("NekoTrail can't read {}, so it won't change it.", h.tilde(&p)))
}

pub fn overview(h: &Homes, ws: Option<&Path>) -> Overview {
    let mut problems = vec![];
    let all = found(h, ws, &mut problems);
    let mut names: Vec<&String> = all.iter().map(|(n, _, _)| n).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    let servers = names
        .into_iter()
        .map(|name| {
            let mine: Vec<&Found> = all.iter().filter(|(n, _, _)| n == name).collect();
            let has: Vec<Has> = mine.iter().map(|(_, h, _)| h.clone()).collect();
            // Prefer a definition that reads cleanly.
            let (_, from, def) = mine.iter().find(|(_, _, d)| d.is_ok()).unwrap_or(&mine[0]);
            let (def, problem) = match def {
                Ok(d) => (Some(d), None),
                Err(e) => (None, Some(e.clone())),
            };
            let targets = AGENTS
                .iter()
                .filter(|a| !has.iter().any(|h| h.agent == **a))
                .map(|&agent| {
                    let why = match def {
                        None => Some("NekoTrail can't read this server's settings.".to_string()),
                        Some(d) => user_readable(h, agent).err().or_else(|| check(d, agent, name).0.into_iter().next()),
                    };
                    Target { agent, ok: why.is_none(), why }
                })
                .collect();
            Row {
                name: name.clone(),
                transport: def.map(|d| d.transport),
                summary: def.map(summary).unwrap_or_default(),
                env: def.map(|d| d.env.iter().map(|(k, _)| k.clone()).collect()).unwrap_or_default(),
                has,
                from: from.clone(),
                secrets: def.map(secrets).unwrap_or_default(),
                targets,
                problem,
            }
        })
        .collect();
    Overview {
        servers,
        skills: skills(h),
        files: AGENTS.iter().map(|&a| (a, h.tilde(&h.user_file(a)))).collect(),
        opencode_skill_dirs: opencode_skill_dirs(h).iter().map(|p| h.tilde(p)).collect(),
        problems,
    }
}

// ---------- adding a server ----------

#[derive(Clone, Debug, Serialize)]
pub struct Preview {
    pub file: String,
    /// The file doesn't exist yet and will be made.
    pub creates: bool,
    /// Where the backup will go.
    pub backup: Option<String>,
    /// Exactly what is added, with copied secret values hidden.
    pub snippet: String,
    pub notes: Vec<String>,
    /// Reasons it can't be added. Empty (with `asks` empty) means ready.
    pub blockers: Vec<String>,
    /// Secrets still waiting for "as a variable" or "copy the value".
    pub asks: Vec<String>,
    pub secrets: Vec<Secret>,
    /// Hash of the file and the change; Add refuses if either moved since the preview.
    pub token: String,
}

fn backup_path(real: &Path, stamp: &str) -> PathBuf {
    let name = real.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut p = real.with_file_name(format!("{name}.bak-nekotrail-{stamp}"));
    let mut n = 2;
    while p.symlink_metadata().is_ok() {
        p = real.with_file_name(format!("{name}.bak-nekotrail-{stamp}-{n}"));
        n += 1;
    }
    p
}

/// The real file behind the config path (a dotfiles symlink stays a symlink).
fn real_path(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

struct Plan {
    preview: Preview,
    new_text: Option<String>,
}

fn plan(h: &Homes, ws: Option<&Path>, name: &str, target: AgentId, choices: &HashMap<String, String>, stamp: &str) -> Result<Plan, String> {
    let mut problems = vec![];
    let all = found(h, ws, &mut problems);
    let mine: Vec<&Found> = all.iter().filter(|(n, _, _)| n == name).collect();
    if mine.is_empty() {
        return Err(format!("No server called {name} any more. Check again."));
    }
    let def = mine.iter().find_map(|(_, _, d)| d.as_ref().ok()).ok_or_else(|| format!("NekoTrail can't read {name}'s settings."))?;
    let path = h.user_file(target);
    let real = real_path(&path);
    let old = std::fs::read(&real).ok();
    // Checked after your choices, so a secret you bring as a variable gets the variable notes.
    let (real_def, mut no, more) = with_choices(def, name, choices, false);
    let (mut blockers, mut notes) = check(&real_def, target, name);
    if mine.iter().any(|(_, has, _)| has.agent == target) {
        blockers.insert(0, format!("{} already has a server called {name}. NekoTrail won't replace it.", target.label()));
    }
    notes.extend(more);
    let (shown, _, _) = with_choices(def, name, choices, true);
    let mut new_text = None;
    let creates = old.is_none();
    let mut snip = snippet(target, name, &shown);
    if blockers.is_empty() && no.is_empty() {
        let mut old_text = old.as_ref().map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
        // A new OpenCode file starts the way OpenCode starts its own.
        if target == OpenCode && old_text.trim().is_empty() {
            old_text = "{\n  \"$schema\": \"https://opencode.ai/config.json\"\n}\n".into();
        }
        let edit = |d: &Def| match target {
            Codex => toml_add(&old_text, name, to_toml(d)),
            OpenCode => json_add_at(&old_text, "mcp", name, &to_opencode(d)),
            _ => json_add(&old_text, name, &to_json(d, target)),
        };
        match edit(&real_def) {
            Ok(t) => {
                // A new file is shown whole, so the preview is exactly what lands on disk.
                if creates && let Ok(whole) = edit(&shown) {
                    snip = whole.trim_end().to_string();
                }
                new_text = Some(t);
            }
            Err(e) => blockers.push(e),
        }
    }
    let token = {
        use sha2::{Digest, Sha256};
        let mut s = Sha256::new();
        s.update(old.as_deref().unwrap_or(b"\0none"));
        s.update(b"\0");
        s.update(new_text.as_deref().unwrap_or("").as_bytes());
        s.finalize().iter().map(|b| format!("{b:02x}")).collect()
    };
    Ok(Plan {
        preview: Preview {
            file: h.tilde(&path),
            creates,
            backup: (!creates).then(|| h.tilde(&backup_path(&real, stamp))),
            snippet: snip,
            notes,
            blockers,
            asks: std::mem::take(&mut no),
            secrets: secrets(def),
            token,
        },
        new_text,
    })
}

pub fn preview(h: &Homes, ws: Option<&Path>, name: &str, target: AgentId, choices: &HashMap<String, String>, stamp: &str) -> Result<Preview, String> {
    plan(h, ws, name, target, choices, stamp).map(|p| p.preview)
}

#[derive(Clone, Debug, Serialize)]
pub struct Applied {
    pub file: String,
    pub backup: Option<String>,
}

/// Adds the server exactly as previewed: backs the file up next to itself, then swaps in the new
/// text in one rename, keeping the file's permissions (new files are private to you).
pub fn apply(h: &Homes, ws: Option<&Path>, name: &str, target: AgentId, choices: &HashMap<String, String>, token: &str, stamp: &str) -> Result<Applied, String> {
    let p = plan(h, ws, name, target, choices, stamp)?;
    if let Some(b) = p.preview.blockers.first().or(p.preview.asks.first()) {
        return Err(b.clone());
    }
    if p.preview.token != token {
        return Err("The file changed since the preview. Look at it again before adding.".into());
    }
    let new = p.new_text.ok_or("Nothing to add.")?;
    let path = h.user_file(target);
    let real = real_path(&path);
    let backup = if real.exists() {
        let b = backup_path(&real, stamp);
        std::fs::copy(&real, &b).map_err(|e| format!("Couldn't back up {}: {e}", h.tilde(&path)))?;
        Some(b)
    } else {
        if let Some(dir) = real.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        None
    };
    let tmp = real.with_file_name(format!(".{}.nekotrail-tmp", real.file_name().unwrap_or_default().to_string_lossy()));
    std::fs::write(&tmp, &new).map_err(|e| format!("Couldn't write {}: {e}", h.tilde(&path)))?;
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&real).map(|m| m.permissions()).unwrap_or_else(|_| std::fs::Permissions::from_mode(0o600));
        let _ = std::fs::set_permissions(&tmp, perms);
    }
    std::fs::rename(&tmp, &real).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Couldn't write {}: {e}", h.tilde(&path))
    })?;
    Ok(Applied { file: h.tilde(&path), backup: backup.map(|b| h.tilde(&b)) })
}

// ---------- skills ----------

#[derive(Clone, Debug, Serialize)]
pub struct SkillShare {
    /// "agents" (Codex, Gemini CLI and OpenCode, through ~/.agents/skills) or "claude".
    pub target: &'static str,
    pub label: String,
    /// The link that will be made, and where it points.
    pub link: String,
    pub to: String,
    pub ok: bool,
    pub why: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SkillRow {
    pub name: String,
    pub description: String,
    pub has: Vec<AgentId>,
    pub share: Vec<SkillShare>,
}

/// `name` and `description` from SKILL.md front matter, when both are there.
fn front_matter(skill_md: &Path) -> (Option<String>, Option<String>) {
    let Ok(t) = std::fs::read_to_string(skill_md) else { return (None, None) };
    let mut lines = t.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }
    let (mut name, mut desc) = (None, None);
    for l in lines.take_while(|l| l.trim() != "---") {
        let clean = |v: &str| Some(v.trim().trim_matches(|c| c == '"' || c == '\'').to_string()).filter(|v| !v.is_empty() && v != "|" && v != ">");
        if let Some(v) = l.strip_prefix("name:") {
            name = clean(v);
        } else if let Some(v) = l.strip_prefix("description:") {
            // A folded description (`description: >`) still counts.
            desc = clean(v).or(Some("(more lines)".into()));
        }
    }
    (name, desc)
}

fn skill_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_str()?.to_string();
            (!n.starts_with('.') && e.path().join("SKILL.md").is_file()).then(|| (n, e.path()))
        })
        .collect();
    out.sort();
    out
}

/// Every skills folder OpenCode reads, its own first.
pub fn opencode_skill_dirs(h: &Homes) -> Vec<PathBuf> {
    let mut out = vec![h.opencode_skills.clone()];
    if h.opencode_reads_claude_skills {
        out.push(h.claude_skills.clone());
    }
    if h.opencode_reads_agents_skills {
        out.push(h.agents_skills.clone());
    }
    out
}

/// "Codex", "Codex and Gemini", "Codex, Gemini and OpenCode".
fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

pub fn skills(h: &Homes) -> Vec<SkillRow> {
    let claude = skill_dirs(&h.claude_skills);
    let agents = skill_dirs(&h.agents_skills);
    let gemini = skill_dirs(&h.gemini_skills);
    let opencode = skill_dirs(&h.opencode_skills);
    let mut names: Vec<&String> = claude.iter().chain(&agents).chain(&gemini).chain(&opencode).map(|(n, _)| n).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    let find = |list: &[(String, PathBuf)], n: &str| list.iter().find(|(m, _)| m == n).map(|(_, p)| p.clone());
    names
        .into_iter()
        .map(|name| {
            let (c, a, g, o) = (find(&claude, name), find(&agents, name), find(&gemini, name), find(&opencode, name));
            let src = c.clone().or(a.clone()).or(g.clone()).or(o.clone()).unwrap();
            let (fm_name, desc) = front_matter(&src.join("SKILL.md"));
            let mut has = vec![];
            if c.is_some() {
                has.push(Claude);
            }
            if a.is_some() {
                has.push(Codex);
            }
            if a.is_some() || g.is_some() {
                has.push(Gemini);
            }
            // OpenCode skips a SKILL.md with no name.
            let opencode_sees = o.is_some() || (c.is_some() && h.opencode_reads_claude_skills) || (a.is_some() && h.opencode_reads_agents_skills);
            if opencode_sees && fm_name.is_some() {
                has.push(OpenCode);
            }
            let mut share = vec![];
            // Into ~/.agents/skills from Claude's or OpenCode's own folder.
            if a.is_none() && let Some(from) = c.as_ref().or(o.as_ref()) {
                let link = h.agents_skills.join(name);
                let why = if fm_name.is_none() || desc.is_none() {
                    Some("Codex and Gemini CLI need a name and a description at the top of SKILL.md, and this one is missing them.".into())
                } else if link.symlink_metadata().is_ok() {
                    Some(format!("Something called {name} is already in {}.", h.tilde(&h.agents_skills)))
                } else {
                    None
                };
                let mut gain = vec!["Codex"];
                if g.is_none() {
                    gain.push("Gemini");
                }
                if !has.contains(&OpenCode) && h.opencode_reads_agents_skills {
                    gain.push("OpenCode");
                }
                share.push(SkillShare { target: "agents", label: format!("Add to {}", and_list(&gain)), link: h.tilde(&link), to: h.tilde(&real_path(from)), ok: why.is_none(), why });
            }
            if c.is_none() {
                let from = a.clone().or(g.clone()).or(o.clone()).unwrap();
                let link = h.claude_skills.join(name);
                let why = link.symlink_metadata().is_ok().then(|| format!("Something called {name} is already in {}.", h.tilde(&h.claude_skills)));
                share.push(SkillShare { target: "claude", label: "Add to Claude".into(), link: h.tilde(&link), to: h.tilde(&real_path(&from)), ok: why.is_none(), why });
            }
            SkillRow { name: name.clone(), description: desc.filter(|d| d != "(more lines)").unwrap_or_default(), has, share }
        })
        .collect()
}

/// Links a skill folder into the other CLIs' skills folder. A link, not a copy: the skill keeps
/// one home, and editing it updates it everywhere. Never replaces anything.
pub fn share_skill(h: &Homes, name: &str, target: &str) -> Result<String, String> {
    if name.is_empty() || name.contains('/') || name.starts_with('.') {
        return Err("That isn't a skill name.".into());
    }
    let row = skills(h).into_iter().find(|s| s.name == name).ok_or_else(|| format!("No skill called {name} any more."))?;
    let s = row.share.into_iter().find(|s| s.target == target).ok_or("It's already there.")?;
    if let Some(why) = s.why {
        return Err(why);
    }
    // The first folder that has it, in the order `skills` picks a source.
    let first = |dirs: &[&PathBuf]| dirs.iter().map(|d| d.join(name)).find(|p| p.join("SKILL.md").is_file());
    let (dir, from) = match target {
        "agents" => (&h.agents_skills, first(&[&h.claude_skills, &h.opencode_skills])),
        _ => (&h.claude_skills, first(&[&h.agents_skills, &h.gemini_skills, &h.opencode_skills])),
    };
    let from = real_path(&from.ok_or_else(|| format!("No skill called {name} any more."))?);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let link = dir.join(name);
    std::os::unix::fs::symlink(&from, &link).map_err(|e| format!("Couldn't link {}: {e}", h.tilde(&link)))?;
    Ok(h.tilde(&link))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn choices(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// Every line of `old` is still in `new`, in order.
    fn kept_lines(old: &str, new: &str) {
        let mut rest = new.lines();
        for l in old.lines() {
            // JSON gains a comma after what used to be the last member.
            assert!(rest.any(|n| n == l || n == format!("{l},")), "lost line {l:?} in:\n{new}");
        }
    }

    const CODEX: &str = "# my codex config\nmodel = \"gpt-5\" # the default\n\n[mcp_servers.docs]\n# OpenAI docs\nurl = \"https://developers.openai.com/mcp\"\n\n[tui]\npet = \"fireball\"\n";

    const CLAUDE: &str = r#"{
  "numStartups": 12,
  "mcpServers": {
    "grafana": {
      "type": "stdio",
      "command": "uvx",
      "args": ["mcp-grafana"],
      "env": {
        "GRAFANA_URL": "https://grafana.example.com",
        "GRAFANA_SERVICE_ACCOUNT_TOKEN": "glsa_abcdefghijklmnop1234567890",
        "DEBUG_LEVEL": "${DEBUG_LEVEL}"
      }
    },
    "cloudflare": { "type": "http", "url": "https://mcp.cloudflare.com/mcp" }
  },
  "projects": {"/tmp/x": {"mcpServers": {"local-one": {"type": "stdio", "command": "node", "args": ["x.js"]}}}}
}
"#;

    fn homes() -> (tempfile::TempDir, Homes) {
        let d = tempfile::tempdir().unwrap();
        let h = Homes::under(d.path());
        (d, h)
    }

    #[test]
    fn json_to_toml_and_back_keeps_everything() {
        let (_d, h) = homes();
        put(&h.claude_json, CLAUDE);
        put(&h.codex_config, CODEX);
        let o = overview(&h, None);
        let row = o.servers.iter().find(|r| r.name == "grafana").unwrap();
        assert_eq!(row.has, vec![Has { agent: Claude, scope: "user", file: "~/.claude.json".into() }]);
        assert!(row.targets.iter().all(|t| t.ok), "{:?}", row.targets);
        assert_eq!(row.secrets.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["env:GRAFANA_SERVICE_ACCOUNT_TOKEN"]);
        assert!(!row.summary.contains("glsa"));

        // The token needs a decision first.
        let p = preview(&h, None, "grafana", Codex, &HashMap::new(), "t1").unwrap();
        assert!(p.blockers.is_empty() && p.asks[0].contains("GRAFANA_SERVICE_ACCOUNT_TOKEN looks like a secret"), "{:?}", p.asks);
        assert!(apply(&h, None, "grafana", Codex, &HashMap::new(), &p.token, "t1").is_err());

        // As a variable: Codex passes it on from its environment, and the value is copied nowhere.
        let c = choices(&[("env:GRAFANA_SERVICE_ACCOUNT_TOKEN", "ref")]);
        let p = preview(&h, None, "grafana", Codex, &c, "t1").unwrap();
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert_eq!(p.backup.as_deref(), Some("~/.codex/config.toml.bak-nekotrail-t1"));
        assert!(p.snippet.contains("env_vars = [\"DEBUG_LEVEL\", \"GRAFANA_SERVICE_ACCOUNT_TOKEN\"]"), "{}", p.snippet);
        assert!(p.notes.iter().any(|n| n.contains("reads DEBUG_LEVEL, GRAFANA_SERVICE_ACCOUNT_TOKEN from the environment")), "{:?}", p.notes);
        let g = preview(&h, None, "grafana", Gemini, &c, "t1").unwrap();
        assert!(g.notes.iter().any(|n| n.contains("GRAFANA_SERVICE_ACCOUNT_TOKEN") && n.contains("environmentVariableRedaction")), "{:?}", g.notes);
        // No Gemini file yet: the preview is the whole new file, keys in the docs' order.
        assert!(g.creates && g.snippet.starts_with("{\n  \"mcpServers\": {\n    \"grafana\": {\n      \"command\": \"uvx\",\n      \"args\""), "{}", g.snippet);
        let done = apply(&h, None, "grafana", Codex, &c, &p.token, "t1").unwrap();
        assert_eq!(done.backup.as_deref(), Some("~/.codex/config.toml.bak-nekotrail-t1"));
        let new = std::fs::read_to_string(&h.codex_config).unwrap();
        assert_eq!(std::fs::read_to_string(h.home.join(".codex/config.toml.bak-nekotrail-t1")).unwrap(), CODEX);
        kept_lines(CODEX, &new);
        assert!(new.contains(&p.snippet), "the file has exactly what was shown:\n{new}");
        assert!(!new.contains("glsa_"));
        // Existing servers and settings untouched.
        let t: toml::Table = new.parse().unwrap();
        assert_eq!(t["mcp_servers"]["docs"]["url"].as_str(), Some("https://developers.openai.com/mcp"));
        assert_eq!(t["tui"]["pet"].as_str(), Some("fireball"));

        // And back: Codex's table reads as the same server, and renders as the same Claude entry.
        let codex_def = from_toml(t["mcp_servers"]["grafana"].as_table().unwrap()).unwrap();
        let claude_def = from_json(Claude, &serde_json::from_str::<Value>(CLAUDE).unwrap()["mcpServers"]["grafana"]).unwrap();
        let (claude_ref, _, _) = with_choices(&claude_def, "grafana", &c, false);
        assert_eq!(to_json(&codex_def, Claude), to_json(&claude_ref, Claude));
        assert_eq!(to_json(&codex_def, Claude)["env"]["GRAFANA_SERVICE_ACCOUNT_TOKEN"], "${GRAFANA_SERVICE_ACCOUNT_TOKEN}");

        // Now Codex has it: no second copy, and the same name is refused.
        let row = overview(&h, None).servers.into_iter().find(|r| r.name == "grafana").unwrap();
        assert!(!row.targets.iter().any(|t| t.agent == Codex));
        let p = preview(&h, None, "grafana", Codex, &c, "t2").unwrap();
        assert!(p.blockers[0].contains("already has a server called grafana"));
    }

    #[test]
    fn toml_to_json_and_back() {
        let (_d, h) = homes();
        let codex = "[mcp_servers.figma]\nurl = \"https://mcp.figma.com/mcp\"\nbearer_token_env_var = \"FIGMA_OAUTH_TOKEN\"\nhttp_headers = { \"X-Figma-Region\" = \"us-east-1\" }\nstartup_timeout_sec = 20\n\n[mcp_servers.ctx]\ncommand = \"npx\"\nargs = [\"-y\", \"@upstash/context7-mcp\"]\nenv_vars = [\"LOCAL_TOKEN\"]\ncwd = \"/tmp\"\n\n[mcp_servers.ctx.env]\nMY_ENV_VAR = \"MY_ENV_VALUE\"\n";
        put(&h.codex_config, codex);
        put(&h.claude_json, "{\n  \"numStartups\": 3,\n  \"projects\": {}\n}\n");
        let none = HashMap::new();
        let p = preview(&h, None, "figma", Claude, &none, "s").unwrap();
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert!(p.notes.iter().any(|n| n.contains("startup_timeout_sec")), "left-out settings are named: {:?}", p.notes);
        assert!(p.notes.iter().any(|n| n.contains("sign in")));
        apply(&h, None, "figma", Claude, &none, &p.token, "s").unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&h.claude_json).unwrap()).unwrap();
        assert_eq!(v["numStartups"], 3);
        assert_eq!(v["mcpServers"]["figma"], json!({"type": "http", "url": "https://mcp.figma.com/mcp", "headers": {"X-Figma-Region": "us-east-1", "Authorization": "Bearer ${FIGMA_OAUTH_TOKEN}"}}));
        // Back to TOML: the same keys Codex started with.
        let back = to_toml(&from_json(Claude, &v["mcpServers"]["figma"]).unwrap());
        let t: toml::Table = toml_snippet("figma", back).parse().unwrap();
        let orig: toml::Table = codex.parse().unwrap();
        let mut want = orig["mcp_servers"]["figma"].as_table().unwrap().clone();
        want.remove("startup_timeout_sec");
        assert_eq!(t["mcp_servers"]["figma"].as_table().unwrap(), &want);

        // Claude has no working-folder setting, so ctx can't go there; Gemini can take it.
        let p = preview(&h, None, "ctx", Claude, &none, "s").unwrap();
        assert!(p.blockers.iter().any(|b| b.contains("working folder")), "{:?}", p.blockers);
        let p = preview(&h, None, "ctx", Gemini, &none, "s").unwrap();
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert!(p.creates && p.backup.is_none());
        assert!(p.notes.iter().any(|n| n.contains("LOCAL_TOKEN") && n.contains("environmentVariableRedaction")), "{:?}", p.notes);
        apply(&h, None, "ctx", Gemini, &none, &p.token, "s").unwrap();
        let g: Value = serde_json::from_str(&std::fs::read_to_string(&h.gemini_settings).unwrap()).unwrap();
        assert_eq!(g["mcpServers"]["ctx"], json!({"command": "npx", "args": ["-y", "@upstash/context7-mcp"], "cwd": "/tmp", "env": {"MY_ENV_VAR": "MY_ENV_VALUE", "LOCAL_TOKEN": "${LOCAL_TOKEN}"}}));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&h.gemini_settings).unwrap().permissions().mode() & 0o777, 0o600, "a new config is private");
        let round = to_toml(&from_json(Gemini, &g["mcpServers"]["ctx"]).unwrap());
        let t: toml::Table = toml_snippet("ctx", round).parse().unwrap();
        assert_eq!(t["mcp_servers"]["ctx"], orig["mcp_servers"]["ctx"]);
    }

    #[test]
    fn copying_a_secret_is_explicit_and_hidden_in_the_preview() {
        let (_d, h) = homes();
        put(&h.claude_json, CLAUDE);
        let c = choices(&[("env:GRAFANA_SERVICE_ACCOUNT_TOKEN", "copy")]);
        let p = preview(&h, None, "grafana", Gemini, &c, "x").unwrap();
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert!(!p.snippet.contains("glsa_"), "{}", p.snippet);
        assert!(p.snippet.contains("<hidden: your value, 31 characters>"));
        assert!(p.snippet.contains("\"DEBUG_LEVEL\": \"${DEBUG_LEVEL}\""));
        apply(&h, None, "grafana", Gemini, &c, &p.token, "x").unwrap();
        let g: Value = serde_json::from_str(&std::fs::read_to_string(&h.gemini_settings).unwrap()).unwrap();
        assert_eq!(g["mcpServers"]["grafana"]["env"]["GRAFANA_SERVICE_ACCOUNT_TOKEN"], "glsa_abcdefghijklmnop1234567890");
        // A secret on the command line or in a url can only be copied, never silently.
        let d = Def { flavor: Some(Claude), command: "srv".into(), args: vec!["--api-key".into(), "abc123".into(), "--token=xyz".into(), "--port".into(), "8080".into()], ..Def::default() };
        assert_eq!(secrets(&d).iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["arg:1", "arg:2"]);
        assert_eq!(summary(&d), "srv --api-key •••• --token=•••• --port 8080");
        let (_, no, _) = with_choices(&d, "srv", &choices(&[("arg:1", "ref")]), false);
        assert_eq!(no.len(), 2, "ref isn't on offer for arguments: {no:?}");
        let u = Def { transport: Transport::Http, url: "https://x.dev/mcp?api_key=abc".into(), ..Def::default() };
        assert_eq!(summary(&u), "https://x.dev/mcp?••••");
        // Paths, ports, flags and references are not secrets.
        for (k, v) in [("GOOGLE_APPLICATION_CREDENTIALS", "/Users/me/key.json"), ("PORT", "8080"), ("DEBUG", "true"), ("GRAFANA_URL", "https://grafana.example.com")] {
            assert!(!looks_secret(k, v), "{k}");
        }
        for (k, v) in [("X", "sk-proj-abcdefghijklmnop"), ("Authorization", "Bearer abc"), ("MY_API_KEY", "x1"), ("X", "a1b2c3d4e5f6g7h8i9j0k1")] {
            assert!(looks_secret(k, v), "{k}");
        }
    }

    #[test]
    fn header_secret_as_a_variable() {
        let (_d, h) = homes();
        put(&h.claude_json, r#"{"mcpServers":{"logfire":{"type":"http","url":"https://logfire.dev/mcp","headers":{"Authorization":"Bearer pylf_v1_us_abcdefghijklmnop"}}}}"#);
        let c = choices(&[("header:Authorization", "ref")]);
        let p = preview(&h, None, "logfire", Codex, &c, "x").unwrap();
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert!(p.snippet.contains("bearer_token_env_var = \"LOGFIRE_AUTHORIZATION\""), "{}", p.snippet);
        assert!(p.notes.iter().any(|n| n.contains("export LOGFIRE_AUTHORIZATION=") && n.contains("without the word Bearer")));
        assert!(!p.snippet.contains("pylf"));
    }

    #[test]
    fn refuses_what_a_cli_cant_run() {
        let sse = Def { flavor: Some(Claude), transport: Transport::Sse, url: "https://x/sse".into(), ..Def::default() };
        assert!(check(&sse, Codex, "x").0[0].contains("SSE"));
        assert!(check(&sse, Gemini, "x").0.is_empty());
        assert_eq!(to_json(&sse, Gemini), json!({"type": "sse", "url": "https://x/sse"}));
        let ws = Def { transport: Transport::Ws, url: "wss://x".into(), ..sse.clone() };
        assert!(!check(&ws, Gemini, "x").0.is_empty());
        assert!(check(&sse, Codex, "bad name").0.iter().any(|b| b.contains("letters, numbers")));
        let renamed = Def { flavor: Some(Claude), command: "x".into(), env: vec![("TOKEN".into(), Val::Var("OTHER".into()))], ..Def::default() };
        assert!(check(&renamed, Codex, "x").0[0].contains("under its own name"));
        assert!(check(&renamed, Gemini, "x").0.is_empty());
        let templ = Def { flavor: Some(Claude), transport: Transport::Http, url: "${BASE:-https://a}/mcp".into(), ..Def::default() };
        assert!(check(&templ, Codex, "x").0[0].contains("doesn't fill in variables"));
        assert!(check(&templ, Gemini, "x").0.is_empty());
        let gem = from_json(Gemini, &json!({"command": "x", "env": {"A": "$HOME/bin:$PATH", "B": "$TOKEN"}})).unwrap();
        assert_eq!(gem.env[1].1, Val::Var("TOKEN".into()));
        assert!(check(&gem, Claude, "x").0.iter().any(|b| b.contains("${NAME}")));
        // A url with no type is something Claude Code itself skips.
        assert!(from_json(Claude, &json!({"url": "https://x"})).is_err());
        assert_eq!(from_json(Gemini, &json!({"httpUrl": "https://x"})).unwrap().transport, Transport::Http);
        assert_eq!(from_json(Gemini, &json!({"url": "https://x", "type": "sse"})).unwrap().transport, Transport::Sse);
    }

    #[test]
    fn json_insertion_keeps_the_file() {
        let v = json!({"command": "x"});
        // Pretty, with the Claude indentation.
        let out = json_add(CLAUDE, "new", &v).unwrap();
        kept_lines(CLAUDE, &out);
        assert!(out.contains("    \"cloudflare\": { \"type\": \"http\", \"url\": \"https://mcp.cloudflare.com/mcp\" },\n    \"new\": {\n      \"command\": \"x\"\n    }\n  },"), "{out}");
        // No mcpServers yet.
        let out = json_add("{\n    \"a\": 1\n}\n", "new", &v).unwrap();
        assert_eq!(out, "{\n    \"a\": 1,\n    \"mcpServers\": {\n        \"new\": {\n            \"command\": \"x\"\n        }\n    }\n}\n");
        // Compact, empty, and JSON with comments and trailing commas (Gemini allows them).
        assert_eq!(json_add(r#"{"a":1}"#, "n", &v).unwrap(), r#"{"a":1,"mcpServers":{"n":{"command":"x"}}}"#);
        assert_eq!(json_add("{\n  \"mcpServers\": {}\n}", "n", &v).unwrap(), "{\n  \"mcpServers\": {\n    \"n\": {\n      \"command\": \"x\"\n    }\n  }\n}");
        let jsonc = "{\n  // my settings\n  \"theme\": \"dark\", /* keep */\n  \"mcpServers\": {\n    \"a\": {\"command\": \"y\"}, // old one\n  },\n}\n";
        let out = json_add(jsonc, "n", &v).unwrap();
        assert!(out.contains("// my settings") && out.contains("/* keep */") && out.contains("// old one"), "{out}");
        let parsed: Value = serde_json::from_str(&blank_jsonc(&out)).unwrap();
        assert_eq!(parsed["mcpServers"]["n"], v);
        assert_eq!(parsed["mcpServers"]["a"], json!({"command": "y"}));
        // Strings that look like comments are left alone.
        assert_eq!(blank_jsonc(r#"{"u": "http://x/*y*/", "e": "a\"//b"}"#), r#"{"u": "http://x/*y*/", "e": "a\"//b"}"#);
        // Clashes and odd shapes are refused.
        assert!(json_add(CLAUDE, "grafana", &v).unwrap_err().contains("already a server called grafana"));
        assert!(json_add(r#"{"mcpServers": []}"#, "n", &v).is_err());
        assert!(json_add("[1]", "n", &v).is_err());
        assert!(json_add("{oops", "n", &v).is_err());
        // Only the top-level mcpServers counts, not a project's.
        let out = json_add(r#"{"projects":{"/p":{"mcpServers":{"n":{}}}}}"#, "n", &v).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["mcpServers"]["n"], v);
    }

    #[test]
    fn toml_insertion_keeps_comments_and_refuses_clashes() {
        let t = to_toml(&Def { command: "node".into(), args: vec!["s.js".into()], ..Def::default() });
        let out = toml_add(CODEX, "x", t.clone()).unwrap();
        kept_lines(CODEX, &out);
        assert!(out.contains("[mcp_servers.x]\ncommand = \"node\"\nargs = [\"s.js\"]"), "{out}");
        assert!(toml_add(CODEX, "docs", t.clone()).unwrap_err().contains("already a server called docs"));
        // No mcp_servers yet: no empty [mcp_servers] header appears.
        let out = toml_add("# hi\nmodel = \"o3\"\n", "x", t.clone()).unwrap();
        assert!(out.starts_with("# hi\nmodel = \"o3\"\n") && !out.contains("[mcp_servers]\n"), "{out}");
        assert!(toml_add("", "x", t.clone()).unwrap().contains("[mcp_servers.x]"));
        // Inline or broken configs are left alone.
        assert!(toml_add("mcp_servers = { a = { command = \"x\" } }\n", "x", t.clone()).is_err());
        assert!(toml_add("[mcp_servers.x\n", "x", t).is_err());
    }

    #[test]
    fn apply_refuses_a_file_that_changed() {
        let (_d, h) = homes();
        put(&h.claude_json, CLAUDE);
        put(&h.codex_config, CODEX);
        let none = HashMap::new();
        let p = preview(&h, None, "cloudflare", Codex, &none, "a").unwrap();
        put(&h.codex_config, &format!("{CODEX}\n# edited\n"));
        assert!(apply(&h, None, "cloudflare", Codex, &none, &p.token, "a").unwrap_err().contains("changed since the preview"));
        assert_eq!(std::fs::read_to_string(&h.codex_config).unwrap(), format!("{CODEX}\n# edited\n"));
        // An unreadable target is never offered or written.
        put(&h.gemini_settings, "{ not json");
        let row = overview(&h, None).servers.into_iter().find(|r| r.name == "cloudflare").unwrap();
        let g = row.targets.iter().find(|t| t.agent == Gemini).unwrap();
        assert!(!g.ok && g.why.as_deref().unwrap().contains("can't read"));
        // Two backups in the same second don't overwrite each other.
        let p = preview(&h, None, "cloudflare", Codex, &none, "a").unwrap();
        put(&h.home.join(".codex/config.toml.bak-nekotrail-a"), "older");
        let done = apply(&h, None, "cloudflare", Codex, &none, &p.token, "a").unwrap();
        assert_eq!(done.backup.as_deref(), Some("~/.codex/config.toml.bak-nekotrail-a-2"));
        assert_eq!(std::fs::read_to_string(h.home.join(".codex/config.toml.bak-nekotrail-a")).unwrap(), "older");
    }

    #[test]
    fn sees_project_and_local_scopes() {
        let (_d, h) = homes();
        let ws = h.home.join("repo");
        put(&h.claude_json, &CLAUDE.replace("/tmp/x", &ws.to_string_lossy()));
        put(&ws.join(".mcp.json"), r#"{"mcpServers":{"team":{"type":"http","url":"https://team.dev/mcp"}}}"#);
        put(&ws.join(".gemini/settings.json"), r#"{"mcpServers":{"team":{"httpUrl":"https://team.dev/mcp"}}}"#);
        let o = overview(&h, Some(&ws));
        let local = o.servers.iter().find(|r| r.name == "local-one").unwrap();
        assert_eq!(local.has[0].scope, "local");
        let team = o.servers.iter().find(|r| r.name == "team").unwrap();
        assert_eq!(team.has.iter().map(|h| (h.agent, h.scope)).collect::<Vec<_>>(), [(Claude, "project"), (Gemini, "project")]);
        assert_eq!(team.targets.iter().map(|t| t.agent).collect::<Vec<_>>(), [Codex, OpenCode]);
        assert_eq!(o.files[&Codex], "~/.codex/config.toml");
        assert_eq!(o.files[&OpenCode], "~/.config/opencode/opencode.jsonc");
    }

    #[test]
    fn skills_are_linked_not_copied() {
        let (_d, h) = homes();
        put(&h.claude_skills.join("review/SKILL.md"), "---\nname: review\ndescription: Review a diff\n---\nBody");
        put(&h.claude_skills.join("bare/SKILL.md"), "Just text");
        put(&h.agents_skills.join("deploy/SKILL.md"), "---\nname: deploy\ndescription: >\n  Ship it\n---\n");
        // Already shared: a Claude skill that is a link into ~/.agents/skills.
        put(&h.agents_skills.join("both/SKILL.md"), "---\nname: both\ndescription: x\n---\n");
        std::os::unix::fs::symlink(h.agents_skills.join("both"), h.claude_skills.join("both")).unwrap();
        let rows = skills(&h);
        let get = |n: &str| rows.iter().find(|r| r.name == n).unwrap().clone();
        assert_eq!(get("both").has, [Claude, Codex, Gemini, OpenCode]);
        assert!(get("both").share.is_empty());
        // OpenCode reads ~/.claude/skills itself.
        assert_eq!(get("review").has, [Claude, OpenCode]);
        assert_eq!(get("review").share[0].label, "Add to Codex and Gemini");
        assert_eq!(get("bare").has, [Claude], "OpenCode skips a skill with no name");
        assert_eq!(get("review").description, "Review a diff");
        assert!(get("review").share[0].ok && get("review").share[0].link == "~/.agents/skills/review");
        assert!(!get("bare").share[0].ok, "no front matter, Codex would skip it");
        assert_eq!(get("deploy").share[0].target, "claude");

        assert_eq!(share_skill(&h, "review", "agents").unwrap(), "~/.agents/skills/review");
        let link = h.agents_skills.join("review");
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(link.join("SKILL.md")).unwrap(), "---\nname: review\ndescription: Review a diff\n---\nBody");
        assert_eq!(skills(&h).iter().find(|r| r.name == "review").unwrap().has, [Claude, Codex, Gemini, OpenCode]);
        share_skill(&h, "deploy", "claude").unwrap();
        assert!(h.claude_skills.join("deploy/SKILL.md").is_file());
        // Never replaces, never shares what the other CLIs would skip.
        assert!(share_skill(&h, "review", "agents").is_err());
        assert!(share_skill(&h, "bare", "agents").unwrap_err().contains("name and a description"));
        put(&h.agents_skills.join("bare"), "a file in the way");
        put(&h.claude_skills.join("bare/SKILL.md"), "---\nname: bare\ndescription: d\n---\n");
        assert!(share_skill(&h, "bare", "agents").unwrap_err().contains("already in"));
        assert!(share_skill(&h, "../x", "agents").is_err());
    }

    const OPENCODE: &str = "{\n  // my opencode\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"theme\": \"tokyonight\", /* keep */\n  \"mcp\": {\n    \"docs\": { \"type\": \"remote\", \"url\": \"https://docs.dev/mcp\" }, // old one\n  },\n}\n";

    fn mode(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn claude_to_opencode_and_back() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, h) = homes();
        put(&h.claude_json, CLAUDE);
        let file = h.opencode_dir.join("opencode.jsonc");
        put(&file, OPENCODE);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        let o = overview(&h, None);
        let docs = o.servers.iter().find(|r| r.name == "docs").unwrap();
        assert_eq!(docs.has, [Has { agent: OpenCode, scope: "user", file: "~/.config/opencode/opencode.jsonc".into() }]);
        assert_eq!(docs.transport, Some(Transport::Http));
        let grafana = o.servers.iter().find(|r| r.name == "grafana").unwrap();
        assert!(grafana.targets.iter().any(|t| t.agent == OpenCode && t.ok), "{:?}", grafana.targets);

        // The token still needs a decision; as a variable it becomes {env:NAME}.
        let p = preview(&h, None, "grafana", OpenCode, &HashMap::new(), "t1").unwrap();
        assert!(p.asks[0].contains("GRAFANA_SERVICE_ACCOUNT_TOKEN looks like a secret"), "{:?}", p.asks);
        let c = choices(&[("env:GRAFANA_SERVICE_ACCOUNT_TOKEN", "ref")]);
        let p = preview(&h, None, "grafana", OpenCode, &c, "t1").unwrap();
        assert!(p.blockers.is_empty() && p.asks.is_empty(), "{:?} {:?}", p.blockers, p.asks);
        assert_eq!(p.file, "~/.config/opencode/opencode.jsonc");
        assert_eq!(p.backup.as_deref(), Some("~/.config/opencode/opencode.jsonc.bak-nekotrail-t1"));
        assert!(p.snippet.starts_with("\"grafana\": {\n  \"type\": \"local\",\n  \"command\": [\n    \"uvx\",\n    \"mcp-grafana\"\n  ],\n  \"environment\""), "{}", p.snippet);
        assert!(p.snippet.contains("\"DEBUG_LEVEL\": \"{env:DEBUG_LEVEL}\"") && p.snippet.contains("{env:GRAFANA_SERVICE_ACCOUNT_TOKEN}"), "{}", p.snippet);
        assert!(p.notes.iter().any(|n| n.contains("OpenCode fills in an empty value")), "{:?}", p.notes);
        apply(&h, None, "grafana", OpenCode, &c, &p.token, "t1").unwrap();

        let new = std::fs::read_to_string(&file).unwrap();
        assert_eq!(std::fs::read_to_string(h.opencode_dir.join("opencode.jsonc.bak-nekotrail-t1")).unwrap(), OPENCODE);
        assert!(new.contains("// my opencode") && new.contains("/* keep */") && new.contains("// old one"), "comments kept:\n{new}");
        assert_eq!(mode(&file), 0o640, "permissions kept");
        let v: Value = serde_json::from_str(&blank_jsonc(&new)).unwrap();
        assert_eq!(v["theme"], "tokyonight");
        assert_eq!(v["mcp"]["docs"], json!({"type": "remote", "url": "https://docs.dev/mcp"}));
        assert_eq!(v["mcp"]["grafana"], json!({"type": "local", "command": ["uvx", "mcp-grafana"], "environment": {
            "GRAFANA_URL": "https://grafana.example.com", "GRAFANA_SERVICE_ACCOUNT_TOKEN": "{env:GRAFANA_SERVICE_ACCOUNT_TOKEN}", "DEBUG_LEVEL": "{env:DEBUG_LEVEL}"}}));
        assert!(!new.contains("glsa_"));

        // And back: OpenCode's entry reads as the same server and renders as the same Claude entry.
        let back = from_opencode(&v["mcp"]["grafana"]).unwrap();
        let claude_def = from_json(Claude, &serde_json::from_str::<Value>(CLAUDE).unwrap()["mcpServers"]["grafana"]).unwrap();
        let (claude_ref, _, _) = with_choices(&claude_def, "grafana", &c, false);
        assert_eq!(to_json(&back, Claude), to_json(&claude_ref, Claude));

        // OpenCode has it now: the same name is refused, and so is a stale preview.
        let p = preview(&h, None, "grafana", OpenCode, &c, "t2").unwrap();
        assert!(p.blockers[0].contains("OpenCode already has a server called grafana"), "{:?}", p.blockers);
        let p = preview(&h, None, "cloudflare", OpenCode, &HashMap::new(), "t2").unwrap();
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert!(p.notes.iter().any(|n| n.contains("opencode mcp auth cloudflare")), "{:?}", p.notes);
        put(&file, &format!("{new}// edited\n"));
        assert!(apply(&h, None, "cloudflare", OpenCode, &HashMap::new(), &p.token, "t2").unwrap_err().contains("changed since the preview"));

        // OpenCode's remote server goes to Claude and Codex.
        let p = preview(&h, None, "docs", Claude, &HashMap::new(), "t3").unwrap();
        apply(&h, None, "docs", Claude, &HashMap::new(), &p.token, "t3").unwrap();
        let cj: Value = serde_json::from_str(&std::fs::read_to_string(&h.claude_json).unwrap()).unwrap();
        assert_eq!(cj["mcpServers"]["docs"], json!({"type": "http", "url": "https://docs.dev/mcp"}));
        assert_eq!(to_opencode(&from_json(Claude, &cj["mcpServers"]["docs"]).unwrap()), json!({"type": "remote", "url": "https://docs.dev/mcp"}));
    }

    #[test]
    fn opencode_to_codex_gemini_and_claude() {
        let (_d, h) = homes();
        let ws = h.home.join("repo");
        let oc = r#"{
  "mcp": {
    "ctx": {"type": "local", "command": ["npx", "-y", "@upstash/context7-mcp"], "environment": {"CONTEXT7_API_KEY": "{env:CONTEXT7_API_KEY}", "MODE": "fast"}, "enabled": false, "timeout": 9000},
    "gh": {"type": "remote", "url": "https://api.github.com/mcp", "headers": {"Authorization": "Bearer {env:GITHUB_TOKEN}"}, "oauth": false},
    "renamed": {"type": "local", "command": ["srv"], "environment": {"TOKEN": "{env:MY_TOKEN}"}},
    "tpl": {"type": "local", "command": ["node", "{env:TOOLS}/srv.js"]},
    "filey": {"type": "remote", "url": "https://x.dev/mcp", "headers": {"X-Key": "{file:~/.x-key}"}},
    "rel": {"type": "local", "command": ["x"], "cwd": "tools"},
    "toggle": {"enabled": false}
  }
}
"#;
        put(&h.opencode_dir.join("opencode.json"), oc);
        put(&h.codex_config, CODEX);
        put(&ws.join("opencode.json"), r#"{"mcp": {"team": {"type": "remote", "url": "https://team.dev/mcp"}}}"#);
        assert_eq!(h.user_file(OpenCode), h.opencode_dir.join("opencode.json"), "writes go where OpenCode's own config is");
        let o = overview(&h, Some(&ws));
        let team = o.servers.iter().find(|r| r.name == "team").unwrap();
        assert_eq!(team.has.iter().map(|h| (h.agent, h.scope)).collect::<Vec<_>>(), [(OpenCode, "project")]);
        assert!(o.servers.iter().find(|r| r.name == "toggle").unwrap().problem.as_deref().unwrap().contains("on or off"));
        let none = HashMap::new();

        // Stdio with a variable under its own name: Codex passes it on, Gemini writes ${NAME}.
        let p = preview(&h, None, "ctx", Codex, &none, "s").unwrap();
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert!(p.notes.iter().any(|n| n.contains("timeout")) && p.notes.iter().any(|n| n.contains("switched off")), "{:?}", p.notes);
        apply(&h, None, "ctx", Codex, &none, &p.token, "s").unwrap();
        let t: toml::Table = std::fs::read_to_string(&h.codex_config).unwrap().parse().unwrap();
        assert_eq!(t["mcp_servers"]["ctx"]["env_vars"].as_array().unwrap()[0].as_str(), Some("CONTEXT7_API_KEY"));
        assert_eq!(t["mcp_servers"]["ctx"]["env"]["MODE"].as_str(), Some("fast"));
        let p = preview(&h, None, "ctx", Gemini, &none, "s").unwrap();
        assert!(p.creates && p.snippet.starts_with("{\n  \"mcpServers\""), "a new file is shown whole: {}", p.snippet);
        apply(&h, None, "ctx", Gemini, &none, &p.token, "s").unwrap();
        let g: Value = serde_json::from_str(&std::fs::read_to_string(&h.gemini_settings).unwrap()).unwrap();
        assert_eq!(g["mcpServers"]["ctx"], json!({"command": "npx", "args": ["-y", "@upstash/context7-mcp"], "env": {"CONTEXT7_API_KEY": "${CONTEXT7_API_KEY}", "MODE": "fast"}}));
        // Round trip: Gemini's and Codex's copies read back as OpenCode's entry, less the settings left out.
        let want = json!({"type": "local", "command": ["npx", "-y", "@upstash/context7-mcp"], "environment": {"CONTEXT7_API_KEY": "{env:CONTEXT7_API_KEY}", "MODE": "fast"}});
        assert_eq!(to_opencode(&from_json(Gemini, &g["mcpServers"]["ctx"]).unwrap()), want);
        let codex_back = to_opencode(&from_toml(t["mcp_servers"]["ctx"].as_table().unwrap()).unwrap());
        assert_eq!(codex_back["environment"], want["environment"]);

        // Remote with Bearer {env:...}: Codex's bearer_token_env_var, and back.
        let p = preview(&h, None, "gh", Codex, &none, "s2").unwrap();
        assert!(p.snippet.contains("bearer_token_env_var = \"GITHUB_TOKEN\""), "{}", p.snippet);
        assert!(p.notes.iter().any(|n| n.contains("oauth")), "{:?}", p.notes);
        apply(&h, None, "gh", Codex, &none, &p.token, "s2").unwrap();
        let t: toml::Table = std::fs::read_to_string(&h.codex_config).unwrap().parse().unwrap();
        assert_eq!(to_opencode(&from_toml(t["mcp_servers"]["gh"].as_table().unwrap()).unwrap()), json!({"type": "remote", "url": "https://api.github.com/mcp", "headers": {"Authorization": "Bearer {env:GITHUB_TOKEN}"}}));
        let gh = from_opencode(&serde_json::from_str::<Value>(oc).unwrap()["mcp"]["gh"]).unwrap();
        assert_eq!(to_json(&gh, Claude), json!({"type": "http", "url": "https://api.github.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}"}}));

        // Variables inside text: converted for Claude, refused where they can't work, with the reason.
        let p = preview(&h, None, "tpl", Claude, &none, "s3").unwrap();
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert!(p.snippet.contains("\"${TOOLS}/srv.js\""), "{}", p.snippet);
        assert!(preview(&h, None, "tpl", Codex, &none, "s3").unwrap().blockers[0].contains("doesn't fill in variables"));
        assert!(preview(&h, None, "tpl", Gemini, &none, "s3").unwrap().blockers.iter().any(|b| b.contains("only in env and headers")));
        assert!(preview(&h, None, "renamed", Codex, &none, "s3").unwrap().blockers[0].contains("under its own name"));
        assert!(preview(&h, None, "renamed", Claude, &none, "s3").unwrap().blockers.is_empty());
        let f = preview(&h, None, "filey", Claude, &none, "s3").unwrap();
        assert!(f.blockers.iter().any(|b| b.contains("reads {file:~/.x-key} from a file")), "{:?}", f.blockers);
        assert!(preview(&h, None, "filey", Codex, &none, "s3").unwrap().blockers.iter().any(|b| b.contains("X-Key")));
        assert!(preview(&h, None, "rel", Gemini, &none, "s3").unwrap().blockers[0].contains("relative"));
        assert!(preview(&h, None, "rel", Claude, &none, "s3").unwrap().blockers[0].contains("working folder"));
        // Nothing refused was written.
        assert!(!std::fs::read_to_string(&h.claude_json).unwrap_or_default().contains("filey"));
    }

    #[test]
    fn into_opencode_converts_or_refuses() {
        let (_d, h) = homes();
        // No OpenCode config yet: a new opencode.jsonc, shown whole, private to you.
        put(&h.claude_json, r#"{"mcpServers": {
            "dflt": {"type": "stdio", "command": "srv", "args": ["--port", "${PORT:-8080}"]},
            "tpl": {"type": "stdio", "command": "srv", "env": {"P": "${A}:${B}"}},
            "events": {"type": "sse", "url": "https://x.dev/sse"},
            "sock": {"type": "ws", "url": "wss://x.dev/ws"}
        }}"#);
        put(&h.codex_config, "[mcp_servers.lit]\ncommand = \"srv\"\nenv = { Q = \"a{env:B}\" }\n");
        put(&h.gemini_settings, r#"{"mcpServers": {"gpath": {"command": "srv", "env": {"PATH": "$HOME/bin:${PATH}"}}}}"#);
        let none = HashMap::new();
        let p = preview(&h, None, "tpl", OpenCode, &none, "n").unwrap();
        assert!(p.blockers.is_empty() && p.creates, "{:?}", p.blockers);
        assert_eq!(p.file, "~/.config/opencode/opencode.jsonc");
        apply(&h, None, "tpl", OpenCode, &none, &p.token, "n").unwrap();
        let file = h.opencode_dir.join("opencode.jsonc");
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(p.snippet, text.trim_end(), "the preview is exactly the new file");
        assert!(text.starts_with("{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"mcp\": {"), "{text}");
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["mcp"]["tpl"]["environment"]["P"], "{env:A}:{env:B}");
        assert_eq!(mode(&file), 0o600);

        let p = preview(&h, None, "gpath", OpenCode, &none, "n").unwrap();
        assert!(p.blockers.is_empty(), "{:?}", p.blockers);
        assert!(p.snippet.contains("\"PATH\": \"{env:HOME}/bin:{env:PATH}\""), "{}", p.snippet);
        assert!(preview(&h, None, "dflt", OpenCode, &none, "n").unwrap().blockers[0].contains("no way to write ${PORT:-8080}"));
        assert!(preview(&h, None, "lit", OpenCode, &none, "n").unwrap().blockers[0].contains("would read the {env:...}"));
        assert!(preview(&h, None, "sock", OpenCode, &none, "n").unwrap().blockers[0].contains("WebSocket"));
        let p = preview(&h, None, "events", OpenCode, &none, "n").unwrap();
        assert!(p.blockers.is_empty() && p.notes.iter().any(|n| n.contains("falls back to SSE")), "{:?}", p.notes);
        assert!(p.snippet.contains("\"type\": \"remote\""));

        // Variable syntax, piece by piece.
        assert_eq!(convert(Claude, OpenCode, "${A}/x/${B}").unwrap(), "{env:A}/x/{env:B}");
        assert_eq!(convert(Claude, OpenCode, "cost $5 and $HOME").unwrap(), "cost $5 and $HOME", "Claude doesn't expand $NAME");
        assert_eq!(convert(OpenCode, Claude, "Bearer {env:T}").unwrap(), "Bearer ${T}");
        assert_eq!(convert(OpenCode, Gemini, "a {env:T} b").unwrap(), "a ${T} b");
        assert!(convert(OpenCode, Gemini, "$HOME").unwrap_err().contains("would read the $"), "a literal $HOME in OpenCode means something else in Gemini");
        assert!(convert(OpenCode, Claude, "{env:not-a-name}").is_err());
        assert_eq!(parse_val(OpenCode, "{env:T}", false), Val::Var("T".into()));
        assert_eq!(parse_val(OpenCode, "Bearer {env:T}", true), Val::Bearer("T".into()));
        assert_eq!(parse_val(OpenCode, "x{env:T}", false), Val::Template("x{env:T}".into()));
        assert_eq!(parse_val(OpenCode, "${T}", false), Val::Lit("${T}".into()), "OpenCode doesn't expand ${{T}}");
    }

    #[test]
    fn opencode_global_file_and_skills() {
        let (_d, h) = homes();
        // OpenCode writes to the first of opencode.jsonc, opencode.json, config.json that exists.
        put(&h.opencode_dir.join("config.json"), r#"{"mcp": {"old": {"type": "local", "command": ["a"]}}}"#);
        assert_eq!(h.user_file(OpenCode), h.opencode_dir.join("config.json"));
        put(&h.opencode_dir.join("opencode.json"), r#"{"mcp": {"old": {"type": "local", "command": ["b"]}}}"#);
        assert_eq!(h.user_file(OpenCode), h.opencode_dir.join("opencode.json"));
        // The later file wins, so it's the one "Add to" copies.
        let o = overview(&h, None);
        let old = o.servers.iter().find(|r| r.name == "old").unwrap();
        assert_eq!(old.summary, "b");
        assert_eq!(old.from.file, "~/.config/opencode/opencode.json");
        assert_eq!(o.opencode_skill_dirs, ["~/.config/opencode/skills", "~/.claude/skills", "~/.agents/skills"]);

        put(&h.opencode_skills.join("oc-only/SKILL.md"), "---\nname: oc-only\ndescription: Mine\n---\n");
        put(&h.claude_skills.join("review/SKILL.md"), "---\nname: review\ndescription: Review a diff\n---\n");
        let rows = skills(&h);
        let oc = rows.iter().find(|r| r.name == "oc-only").unwrap();
        assert_eq!(oc.has, [OpenCode]);
        assert_eq!(oc.share.iter().map(|s| (s.target, s.label.as_str())).collect::<Vec<_>>(), [("agents", "Add to Codex and Gemini"), ("claude", "Add to Claude")]);
        assert_eq!(share_skill(&h, "oc-only", "claude").unwrap(), "~/.claude/skills/oc-only");
        assert_eq!(std::fs::read_link(h.claude_skills.join("oc-only")).unwrap(), real_path(&h.opencode_skills.join("oc-only")));
        share_skill(&h, "oc-only", "agents").unwrap();
        assert_eq!(std::fs::read_link(h.agents_skills.join("oc-only")).unwrap(), real_path(&h.opencode_skills.join("oc-only")));

        // With OPENCODE_DISABLE_CLAUDE_CODE_SKILLS, a Claude skill reaches OpenCode through ~/.agents/skills.
        let off = Homes { opencode_reads_claude_skills: false, ..h.clone() };
        let review = skills(&off).into_iter().find(|r| r.name == "review").unwrap();
        assert_eq!(review.has, [Claude]);
        assert_eq!(review.share[0].label, "Add to Codex, Gemini and OpenCode");
    }
}

