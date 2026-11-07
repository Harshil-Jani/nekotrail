//! Driving the app from outside it: the `nekotrail` command (over a Unix socket) and
//! `nekotrail://` links.
//!
//! Trust differs by door. The socket lives in a folder only you can open, and the app also checks
//! that the process on the other end runs as you, so the command line can create workspaces and
//! send prompts directly: anything that can reach it could already run `claude` itself. A link can
//! come from any web page, so a link may focus a workspace but never start work on its own: it
//! parks the task here and the app asks you first (`Pending`, `link_confirm`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Bumped when a request or answer changes shape, so an old app and a new command say so plainly.
/// 2 added `hello`, `propose`, `draft` and `changes` (the editor extension), and `path` and
/// `target_branch` on each workspace row. 3 added `handoff`. A request carries the lowest version
/// that has its command, so a new command still reaches an older app for anything that app knows.
pub const PROTOCOL: u32 = 3;
/// The oldest request version the app still answers.
pub const MIN_PROTOCOL: u32 = 1;
/// A request is one JSON line. Prompts can be long, but not this long.
const MAX_REQUEST: u64 = 1 << 20;
/// Prompts from a link are shown in full before they run, so they stay readable.
pub const MAX_LINK_PROMPT: usize = 2000;
/// A task proposed from your editor carries the selected code, so it may be longer than a link's.
/// It is still shown in full (scrolling) before it runs.
pub const MAX_EDITOR_PROMPT: usize = 20_000;
/// Text put in a workspace's message box from your editor.
pub const MAX_DRAFT: usize = 100_000;
/// A link waits this long for your answer, then it's gone.
const PENDING_FOR: Duration = Duration::from_secs(10 * 60);

/// `<app data>/cli/nekotrail.sock`. The folder is owner-only (0700), the socket 0600.
pub fn socket_path(data: &Path) -> PathBuf {
    data.join("cli").join("nekotrail.sock")
}

/// The app's data folder, the same one `run` uses.
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("dev.nekotrail.app"))
}

// ---------- protocol ----------

/// What the command line asks for. One JSON object per line: `{"v":1,"cmd":"ls",...}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase")]
pub enum Request {
    /// Bring the window forward.
    Show,
    /// Add the repo at `path` if it's new, and focus it.
    Add { path: String },
    /// Make a workspace in the repo at `cwd` and send `prompt` to its agent.
    New { cwd: String, task: Task },
    /// Workspaces with their state, agent and PR. Only the repo at `cwd` unless `all`.
    Ls { cwd: Option<String>, all: bool },
    /// Focus the workspace matching `query` (name, branch or title).
    Open { cwd: Option<String>, query: String },
    /// What needs you right now.
    Status,
    /// Line up a task in the queue for the repo at `repo` (or the one at `cwd`).
    #[serde(rename = "queue_add")]
    QueueAdd { cwd: String, repo: Option<String>, task: QueueTask },
    /// The queue, in order.
    #[serde(rename = "queue_ls")]
    QueueLs,
    /// (v2) Which protocol and app version answer.
    Hello,
    /// (v2) Ask in the app before making a workspace for the repo at `cwd` and sending `task`.
    /// Unlike `new`, nothing starts until you press Start in nekotrail (the same question a link gets).
    Propose { cwd: String, task: Task },
    /// (v2) Put `text` in the message box of `workspace` (its id, name or branch) and show it.
    /// It is not sent: you read it and press Send.
    Draft { workspace: String, text: String },
    /// (v2) The files `workspace` changed against its target branch, and the commit they're compared to.
    Changes { workspace: String },
    /// (v3) Hand `workspace`'s latest chat to the agent `to` (see `handoff`). `workspace` "." is the
    /// one `cwd` is in. Without `now` the hand-off opens in the app with its brief, and nothing is
    /// sent until you press Hand off there.
    Handoff { cwd: String, workspace: String, to: String, model: Option<String>, now: bool },
}

impl Request {
    /// The protocol version that added this command.
    pub fn since(&self) -> u32 {
        match self {
            Request::Hello | Request::Propose { .. } | Request::Draft { .. } | Request::Changes { .. } => 2,
            Request::Handoff { .. } => 3,
            _ => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    #[serde(flatten)]
    pub req: Request,
}

/// A task to start: the prompt, and optionally the agent, the base branch and plan mode.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub prompt: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub plan: bool,
    /// A Linear issue ("ENG-123") to start from: its branch, title and text. `prompt` is then
    /// optional and goes after the issue's text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear: Option<String>,
}

impl Task {
    /// The checks every door applies. `max` is the longest prompt allowed.
    pub fn validate(&self, max: usize) -> Result<(), String> {
        let prompt = self.prompt.trim();
        if let Some(l) = &self.linear
            && crate::linear::issue_ref(l).is_none() {
                return Err(format!("\"{l}\" isn't a Linear issue. Use its identifier, for example: nekotrail new --linear ENG-123"));
            }
        if prompt.is_empty() && self.linear.is_none() {
            return Err("Write the task, for example: nekotrail new \"fix the login bug\"".into());
        }
        if prompt.chars().count() > max {
            return Err(format!("That task is too long (over {max} characters)."));
        }
        if let Some(a) = &self.agent
            && !crate::agent::ALL.iter().any(|x| x.id() == a) {
                return Err(format!("There's no agent called \"{a}\". Use one of: {}.", agent_ids().join(", ")));
            }
        if let Some(b) = &self.base
            && !crate::safe::branch_name(b) {
                return Err(format!("\"{b}\" isn't a valid branch name."));
            }
        Ok(())
    }
}

/// A task for the queue from the command line. The repo comes from where you run it (or --repo).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueTask {
    pub prompt: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
    /// plan, ask, acceptEdits or bypassPermissions. None: your default for that agent.
    #[serde(default)]
    pub mode: Option<String>,
    /// The agent to use while `agent` is out of usage.
    #[serde(default)]
    pub fallback: Option<String>,
    #[serde(default)]
    pub checks: bool,
    #[serde(default)]
    pub pr: bool,
    #[serde(default)]
    pub autopilot: bool,
}

impl QueueTask {
    pub fn validate(&self) -> Result<(), String> {
        Task { prompt: self.prompt.clone(), agent: self.agent.clone(), base: self.base.clone(), plan: false, linear: None }
            .validate(crate::queue::MAX_PROMPT)
            .map_err(|e| e.replace("nekotrail new", "nekotrail queue add"))?;
        if let Some(f) = &self.fallback {
            if !crate::agent::ALL.iter().any(|x| x.id() == f) {
                return Err(format!("There's no agent called \"{f}\". Use one of: {}.", agent_ids().join(", ")));
            }
            if Some(f) == self.agent.as_ref() || (self.agent.is_none() && f == "claude") {
                return Err("The fallback has to be a different agent from the one running the task.".into());
            }
        }
        if let Some(m) = &self.mode
            && !crate::queue::MODES.contains(&m.as_str()) {
                return Err(format!("\"{m}\" isn't a mode. Use plan, ask, edits or auto."));
            }
        Ok(())
    }
}

pub fn agent_ids() -> Vec<&'static str> {
    crate::agent::ALL.iter().map(|a| a.id()).collect()
}

/// Encode a request as the line the command sends, at the lowest version that has its command.
pub fn encode(req: &Request) -> String {
    let mut s = serde_json::to_string(&Envelope { v: req.since(), req: req.clone() }).unwrap_or_default();
    s.push('\n');
    s
}

/// Decode one request line. Errors are written for a person.
pub fn decode(line: &str) -> Result<Request, String> {
    let v: Value = serde_json::from_str(line.trim()).map_err(|_| "That request isn't valid JSON.".to_string())?;
    let Some(ver) = v["v"].as_u64().filter(|n| (MIN_PROTOCOL as u64..=PROTOCOL as u64).contains(n)) else {
        return Err("This nekotrail command and the running app are different versions. Quit and reopen NekoTrail, then try again.".into());
    };
    let req = serde_json::from_value::<Envelope>(v).map(|e| e.req).map_err(|err| format!("NekoTrail didn't understand that request: {err}"))?;
    // A command belongs to the versions from the one that added it.
    if ver < req.since() as u64 {
        return Err(format!("NekoTrail didn't understand that request: \"{}\" needs protocol {} or newer.", command_name(&req), req.since()));
    }
    Ok(req)
}

fn command_name(req: &Request) -> String {
    serde_json::to_value(req).ok().and_then(|v| v["cmd"].as_str().map(String::from)).unwrap_or_default()
}

/// The one answer line: `{"ok":true,"data":...}` or `{"ok":false,"error":"..."}`.
pub fn answer(res: &Result<Value, String>) -> String {
    let v = match res {
        Ok(data) => json!({"ok": true, "data": data}),
        Err(err) => json!({"ok": false, "error": err}),
    };
    format!("{v}\n")
}

/// Read an answer line back into a result.
pub fn parse_answer(line: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(line.trim()).map_err(|_| "NekoTrail sent an answer this command can't read.".to_string())?;
    if v["ok"].as_bool() == Some(true) {
        Ok(v["data"].clone())
    } else {
        Err(v["error"].as_str().unwrap_or("NekoTrail couldn't do that.").to_string())
    }
}

// ---------- links ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// `nekotrail://new?repo=...&prompt=...[&agent=...][&base=...][&plan=1]`
    New { repo: String, task: Task },
    /// `nekotrail://open?workspace=...`
    Open { workspace: String },
}

impl Link {
    /// The confirm rule: a link that would create a workspace or send a prompt waits for you.
    /// Only focusing something that already exists goes straight through.
    pub fn needs_confirm(&self) -> bool {
        match self {
            Link::New { .. } => true,
            Link::Open { .. } => false,
        }
    }
}

/// `%41`, `+` and UTF-8 in query values. None for a broken escape.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// Parse a `nekotrail://` link. Pure: it decides nothing about what happens next.
pub fn parse_link(url: &str) -> Result<Link, String> {
    let bad = || "That NekoTrail link isn't one NekoTrail understands.".to_string();
    let rest = url.get(..10).filter(|p| p.eq_ignore_ascii_case("nekotrail:")).map(|_| &url[10..]).ok_or_else(bad)?;
    let rest = rest.trim_start_matches('/');
    let rest = rest.split('#').next().unwrap_or("");
    let (action, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut params: HashMap<String, String> = HashMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let k = percent_decode(k).ok_or_else(bad)?;
        let v = percent_decode(v).ok_or_else(bad)?;
        // Two values for one key would leave a doubt about which one you approved.
        if params.insert(k.clone(), v).is_some() {
            return Err(format!("That NekoTrail link sets \"{k}\" twice."));
        }
    }
    let get = |k: &str| params.get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    match action.trim_end_matches('/').to_ascii_lowercase().as_str() {
        "new" => {
            let repo = get("repo").ok_or("That NekoTrail link doesn't say which repo.")?;
            let plan = matches!(get("plan").as_deref(), Some("1" | "true" | "yes"));
            let task = Task { prompt: params.get("prompt").cloned().unwrap_or_default(), agent: get("agent"), base: get("base"), plan, linear: None };
            task.validate(MAX_LINK_PROMPT)?;
            Ok(Link::New { repo, task })
        }
        "open" => Ok(Link::Open { workspace: get("workspace").ok_or("That NekoTrail link doesn't say which workspace.")? }),
        _ => Err(bad()),
    }
}

/// Tasks from links waiting for your OK, each behind a one-time token.
pub struct Pending<T> {
    items: Vec<(String, Instant, T)>,
    ttl: Duration,
}

impl<T: PartialEq + Clone> Pending<T> {
    pub fn new(ttl: Duration) -> Self {
        Self { items: Vec::new(), ttl }
    }

    /// Park `item` and return its token. None when the same item is already waiting
    /// (a page firing one link twice asks once).
    pub fn push(&mut self, item: T) -> Option<String> {
        self.expire();
        if self.items.iter().any(|(_, _, x)| *x == item) {
            return None;
        }
        // A page can't pile up questions: only the newest few wait.
        if self.items.len() >= 5 {
            self.items.remove(0);
        }
        let token = crate::store::new_id();
        self.items.push((token.clone(), Instant::now(), item));
        Some(token)
    }

    /// The item behind `token`, once. Unknown or expired tokens give None.
    pub fn take(&mut self, token: &str) -> Option<T> {
        self.expire();
        let i = self.items.iter().position(|(t, _, _)| t == token)?;
        Some(self.items.remove(i).2)
    }

    fn expire(&mut self) {
        let ttl = self.ttl;
        self.items.retain(|(_, at, _)| at.elapsed() < ttl);
    }
}

/// What the window should do next, queued here so nothing is lost if it isn't listening yet
/// (the app was just launched by the command or a link). The window drains it with `remote_take`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Job {
    /// Open this workspace.
    Focus { workspace_id: String },
    /// Show this repo (`added` when it was new).
    Repo { repo_id: String, added: bool },
    /// A new workspace from the command line or an approved link: run setup, then send the prompt.
    Start { workspace_id: String, session_id: String, prompt: String },
    /// A link (or your editor) wants to start an agent. Ask, then call `link_confirm` with the token.
    /// `from` is `link` or `editor`, so the question says who asked.
    Confirm { token: String, from: String, repo: String, repo_path: String, adds_repo: bool, prompt: String, agent: String, base: Option<String>, plan: bool },
    /// Your editor sent text for this workspace's message box. Show it there; don't send it.
    /// `note`: what to tell you instead of the editor's "your selection" line.
    Draft { workspace_id: String, text: String, note: Option<String> },
    /// `nekotrail handoff`: open the hand-off of this chat to `agent`, with its brief to read first.
    Handoff { workspace_id: String, session_id: String, agent: String, model: Option<String> },
    /// A link nekotrail couldn't use.
    Error { text: String },
}

/// Where a link's repo points: a repo you have, or a folder that would be added.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkTask {
    pub repo_id: Option<String>,
    pub repo_path: String,
    pub task: Task,
}

pub struct Inbox {
    jobs: Mutex<VecDeque<Job>>,
    pending: Mutex<Pending<LinkTask>>,
}

impl Default for Inbox {
    fn default() -> Self {
        Self { jobs: Default::default(), pending: Mutex::new(Pending::new(PENDING_FOR)) }
    }
}

impl Inbox {
    pub fn push(&self, job: Job) {
        let mut q = self.jobs.lock().unwrap();
        if q.len() >= 50 {
            q.pop_front();
        }
        q.push_back(job);
    }
    pub fn take(&self) -> Vec<Job> {
        self.jobs.lock().unwrap().drain(..).collect()
    }
}

// ---------- matching ----------

/// The fields `open <query>` matches on.
pub struct Candidate<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub branch: &'a str,
    pub title: &'a str,
    pub in_cwd_repo: bool,
}

/// Find the workspace a person meant: an exact name, branch (with or without its `you/` prefix)
/// or title first, then a unique piece of a title. Ties prefer the repo you're standing in.
pub fn find_workspace(query: &str, cands: &[Candidate]) -> Result<usize, String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Err("Say which workspace, for example: nekotrail open mochi".into());
    }
    let exact = |c: &Candidate| {
        c.id == query.trim()
            || c.name.to_lowercase() == q
            || c.branch.to_lowercase() == q
            || c.branch.rsplit_once('/').is_some_and(|(_, tail)| tail.to_lowercase() == q)
            || (!c.title.is_empty() && c.title.to_lowercase() == q)
    };
    let partial = |c: &Candidate| c.title.to_lowercase().contains(&q) || c.branch.to_lowercase().contains(&q);
    for test in [&exact as &dyn Fn(&Candidate) -> bool, &partial] {
        let hits: Vec<usize> = (0..cands.len()).filter(|&i| test(&cands[i])).collect();
        let here: Vec<usize> = hits.iter().copied().filter(|&i| cands[i].in_cwd_repo).collect();
        match (hits.len(), here.len()) {
            (0, _) => continue,
            (1, _) => return Ok(hits[0]),
            (_, 1) => return Ok(here[0]),
            _ => {
                let names: Vec<String> = hits.iter().take(6).map(|&i| label(&cands[i])).collect();
                return Err(format!("\"{query}\" matches {} workspaces: {}. Use the name or branch.", hits.len(), names.join(", ")));
            }
        }
    }
    Err(format!("No active workspace matches \"{query}\". Run nekotrail ls to see them."))
}

fn label(c: &Candidate) -> String {
    if c.title.is_empty() { c.name.to_string() } else { format!("{} ({})", c.name, c.title) }
}

/// One line about a PR from the poller's cache: "#12 open, checks failing".
pub fn pr_summary(pr: &Value) -> Option<String> {
    let n = pr["number"].as_u64()?;
    let state = match pr["state"].as_str() {
        Some("MERGED") => return Some(format!("#{n} merged")),
        Some("CLOSED") => return Some(format!("#{n} closed")),
        _ if pr["isDraft"].as_bool() == Some(true) => "draft",
        _ => "open",
    };
    let next = match crate::autopilot_next(pr).0 {
        "fix" => ", checks failing",
        "conflicts" => ", conflicts",
        "review" => ", changes requested",
        "wait" if pr["statusCheckRollup"].as_array().is_some_and(|a| !a.is_empty()) => ", checks running",
        "ready" => ", ready to merge",
        _ => "",
    };
    Some(format!("#{n} {state}{next}"))
}

/// Why an open PR needs you, if it does.
pub fn pr_needs(pr: &Value) -> Option<&'static str> {
    if pr["number"].as_u64().is_none() || pr["isDraft"].as_bool() == Some(true) {
        return None;
    }
    match crate::autopilot_next(pr).0 {
        "fix" => Some("PR checks are failing"),
        "conflicts" => Some("PR has merge conflicts"),
        "review" => Some("PR has changes requested"),
        "ready" => Some("PR is ready to merge"),
        _ => None,
    }
}

// ---------- the app side ----------

#[cfg(unix)]
mod server {
    use super::*;
    use crate::{store::Workspace, AppState};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::io::AsRawFd;
    use std::os::unix::net::{UnixListener, UnixStream};
    use tauri::{AppHandle, Emitter, Manager};

    /// Listen on the socket for the life of the app. A copy of nekotrail already listening keeps it.
    pub fn serve(app: AppHandle, data: PathBuf) {
        let path = socket_path(&data);
        let dir = path.parent().unwrap().to_path_buf();
        if std::fs::create_dir_all(&dir).is_err() || std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).is_err() {
            eprintln!("nekotrail: can't prepare {}; the nekotrail command won't reach this window", dir.display());
            return;
        }
        if UnixStream::connect(&path).is_ok() {
            eprintln!("nekotrail: another nekotrail window already answers the nekotrail command");
            return;
        }
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(err) => {
                eprintln!("nekotrail: can't listen on {}: {err}", path.display());
                return;
            }
        };
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let app = app.clone();
                std::thread::spawn(move || {
                    let _ = handle_conn(&app, conn);
                });
            }
        });
    }

    /// The other end must be you (the folder already says so; this checks the process too).
    fn same_user(conn: &UnixStream) -> bool {
        // SAFETY: geteuid has no preconditions.
        peer_uid(conn).is_some_and(|uid| uid == unsafe { libc::geteuid() })
    }

    #[cfg(not(target_os = "linux"))]
    fn peer_uid(conn: &UnixStream) -> Option<libc::uid_t> {
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: getpeereid only writes the two out-parameters for a valid socket descriptor.
        (unsafe { libc::getpeereid(conn.as_raw_fd(), &mut uid, &mut gid) } == 0).then_some(uid)
    }

    #[cfg(target_os = "linux")]
    fn peer_uid(conn: &UnixStream) -> Option<libc::uid_t> {
        let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: SO_PEERCRED fills a ucred of the given length for a valid socket descriptor.
        let ok = unsafe { libc::getsockopt(conn.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut libc::c_void, &mut len) } == 0;
        ok.then_some(cred.uid)
    }

    fn handle_conn(app: &AppHandle, mut conn: UnixStream) -> std::io::Result<()> {
        if !same_user(&conn) {
            return Ok(());
        }
        conn.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut line = String::new();
        BufReader::new((&conn).take(MAX_REQUEST)).read_line(&mut line)?;
        #[cfg(debug_assertions)]
        if crate::e2e_mode() && line.trim_start().starts_with("{\"e2e\"") {
            return conn.write_all(answer(&crate::e2e::handle(app, &line)).as_bytes());
        }
        let res = decode(&line).and_then(|req| handle(app, req));
        conn.write_all(answer(&res).as_bytes())
    }

    fn show(app: &AppHandle) {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }

    pub fn deliver(app: &AppHandle, job: Job) {
        app.state::<AppState>().inbox.push(job);
        let _ = app.emit("remote", ());
        show(app);
    }

    fn display(w: &Workspace) -> &str {
        if w.title.is_empty() { &w.name } else { &w.title }
    }

    /// The repo a folder belongs to: a workspace's repo, a repo you added, or (with `add`) the
    /// git repo there, added now. A linked worktree counts as its main checkout.
    pub fn repo_for(st: &AppState, dir: &str, add: bool) -> Result<Option<(crate::store::Repo, bool)>, String> {
        let p = Path::new(dir);
        let p = p.canonicalize().map_err(|_| format!("{dir} doesn't exist."))?;
        let canon = |s: &str| Path::new(s).canonicalize().unwrap_or_else(|_| PathBuf::from(s));
        {
            let db = st.db();
            let repos = db.repos().map_err(crate::e)?;
            for w in db.workspaces(true).map_err(crate::e)? {
                if p.starts_with(canon(&w.path)) && let Some(r) = repos.iter().find(|r| r.id == w.repo_id) {
                    return Ok(Some((r.clone(), false)));
                }
            }
            if let Some(r) = repos.iter().find(|r| p.starts_with(canon(&r.root_path))) {
                return Ok(Some((r.clone(), false)));
            }
        }
        if !add {
            return Ok(None);
        }
        let top = crate::git::toplevel(&p).map_err(|_| format!("{} isn't in a git repo. Run this inside a repo, or git init first.", p.display()))?;
        // In a linked worktree, --git-common-dir is the main checkout's .git.
        let main = crate::git::git(&top, &["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .ok()
            .map(PathBuf::from)
            .filter(|c| c.file_name().is_some_and(|n| n == ".git"))
            .and_then(|c| c.parent().map(Path::to_path_buf))
            .unwrap_or(top);
        let before = st.db().repo_by_path(&main.to_string_lossy()).map_err(crate::e)?.is_some();
        let r = crate::add_repo_at(st, &main.to_string_lossy())?;
        Ok(Some((r, !before)))
    }

    /// Make the workspace and its chat; the window runs setup and sends the prompt (`Job::Start`).
    /// From a Linear issue, the prompt is the issue's text, then anything typed after it.
    pub fn start(app: &AppHandle, repo_id: String, mut task: Task) -> Result<(Workspace, String), String> {
        let ws = match task.linear.clone() {
            Some(id) => {
                let (ws, issue) = crate::create_from_linear(&app.state::<AppState>(), repo_id, task.base.clone(), &id)?;
                let extra = task.prompt.trim();
                let seed = crate::linear::seed_text(&issue);
                task.prompt = if extra.is_empty() { seed } else { format!("{seed}\n\n{extra}") };
                ws
            }
            None => tauri::async_runtime::block_on(crate::create_workspace(app.clone(), app.state::<AppState>(), repo_id, task.base.clone(), None, None))?,
        };
        let st = app.state::<AppState>();
        let agent = crate::agent::get(task.agent.as_deref().unwrap_or("claude")).id();
        let pref = |k: &str| st.db().setting(&format!("{agent}_{k}")).ok().flatten().filter(|v| !v.is_empty());
        let s = crate::store::Session {
            id: crate::store::new_id(),
            workspace_id: ws.id.clone(),
            agent: agent.into(),
            agent_session_id: None,
            title: "New chat".into(),
            model: pref("model"),
            permission_mode: if task.plan { "plan".into() } else { pref("mode").unwrap_or_else(|| "acceptEdits".into()) },
            effort: pref("effort"),
        };
        st.db().insert_session(&s).map_err(crate::e)?;
        deliver(app, Job::Start { workspace_id: ws.id.clone(), session_id: s.id.clone(), prompt: task.prompt.trim().to_string() });
        Ok((ws, agent.to_string()))
    }

    fn handle(app: &AppHandle, req: Request) -> Result<Value, String> {
        let st = app.state::<AppState>();
        match req {
            Request::Show => {
                show(app);
                Ok(json!({}))
            }
            Request::Add { path } => {
                let (r, added) = repo_for(&st, &path, true)?.ok_or("That folder isn't a git repo.")?;
                deliver(app, Job::Repo { repo_id: r.id.clone(), added });
                Ok(json!({"repo": r.name, "path": r.root_path, "added": added}))
            }
            Request::New { cwd, task } => {
                task.validate(100_000)?;
                let (r, _) = repo_for(&st, &cwd, true)?.ok_or("That folder isn't a git repo.")?;
                let issue = task.linear.as_deref().and_then(crate::linear::issue_ref);
                let (ws, agent) = start(app, r.id.clone(), task)?;
                Ok(json!({"repo": r.name, "workspace": ws.name, "branch": ws.branch, "path": ws.path, "agent": agent, "issue": issue}))
            }
            Request::Ls { cwd, all } => {
                let here = if all { None } else { cwd.as_deref().and_then(|c| repo_for(&st, c, false).ok().flatten()).map(|(r, _)| r) };
                let rows = rows(&st)?.into_iter().filter(|r| here.as_ref().is_none_or(|h| r["repo_id"] == h.id.as_str())).collect::<Vec<_>>();
                Ok(json!({"repo": here.map(|h| h.name), "workspaces": rows}))
            }
            Request::Open { cwd, query } => {
                let here = cwd.as_deref().and_then(|c| repo_for(&st, c, false).ok().flatten()).map(|(r, _)| r.id);
                let wss = st.db().workspaces(false).map_err(crate::e)?;
                let cands: Vec<Candidate> = wss.iter().map(|w| Candidate { id: &w.id, name: &w.name, branch: &w.branch, title: &w.title, in_cwd_repo: here.as_deref() == Some(w.repo_id.as_str()) }).collect();
                let w = &wss[find_workspace(&query, &cands)?];
                deliver(app, Job::Focus { workspace_id: w.id.clone() });
                Ok(json!({"workspace": w.name, "title": w.title, "branch": w.branch}))
            }
            Request::Status => {
                let rows = rows(&st)?;
                let working = rows.iter().filter(|r| r["state"] == "working").count();
                let needs: Vec<Value> = rows.into_iter().filter(|r| !r["needs"].as_array().is_none_or(|a| a.is_empty())).collect();
                Ok(json!({"working": working, "needs": needs}))
            }
            Request::QueueAdd { cwd, repo, task } => queue_add(app, &st, cwd, repo, task),
            Request::QueueLs => {
                let repos = st.db().repos().map_err(crate::e)?;
                let q = st.queue.lock().unwrap();
                let tasks: Vec<Value> = q.tasks.iter().map(|t| json!({
                    "id": t.id, "title": t.title(), "state": t.state, "step": t.step, "note": t.note,
                    "agent": t.ran_with.clone().unwrap_or_else(|| t.agent.clone()),
                    "repo": repos.iter().find(|r| r.id == t.repo_id).map(|r| r.name.clone()),
                })).collect();
                Ok(json!({"paused": q.paused, "tasks": tasks}))
            }
            Request::Hello => Ok(json!({"protocol": PROTOCOL, "min_protocol": MIN_PROTOCOL, "app": env!("CARGO_PKG_VERSION")})),
            Request::Propose { cwd, task } => match propose(&st, &cwd, task)? {
                Some(job) => {
                    deliver(app, job);
                    Ok(json!({"asked": true}))
                }
                None => {
                    show(app);
                    Ok(json!({"asked": false}))
                }
            },
            Request::Draft { workspace, text } => {
                let job = draft(&st, &workspace, text)?;
                let Job::Draft { workspace_id, .. } = &job else { unreachable!("draft makes a Draft") };
                let w = st.db().workspace(workspace_id).map_err(crate::e)?;
                deliver(app, job);
                Ok(json!({"workspace": w.name, "title": w.title}))
            }
            Request::Changes { workspace } => {
                let w = &find_active(&st, &workspace)?;
                let dir = Path::new(&w.path);
                let base = crate::git::merge_base(dir, &w.target_branch).map_err(|_| format!("Couldn't compare {} with {}. Is the branch still there?", w.name, w.target_branch))?;
                let files = crate::git::changes_since(dir, &base).map_err(crate::e)?;
                Ok(json!({"workspace": w.name, "path": w.path, "target_branch": w.target_branch, "base": base, "files": files}))
            }
            Request::Handoff { cwd, workspace, to, model, now } => handoff(app, &st, &cwd, &workspace, &to, model, now),
        }
    }

    /// `nekotrail handoff`: the workspace's latest chat (the one you last wrote in) goes to `to`.
    fn handoff(app: &AppHandle, st: &AppState, cwd: &str, workspace: &str, to: &str, model: Option<String>, now: bool) -> Result<Value, String> {
        if !crate::agent::ALL.iter().any(|a| a.id() == to) {
            return Err(format!("There's no agent called \"{to}\". Use one of: {}.", agent_ids().join(", ")));
        }
        let w = if workspace == "." {
            let here = Path::new(cwd).canonicalize().map_err(|_| format!("{cwd} doesn't exist."))?;
            let wss = st.db().workspaces(false).map_err(crate::e)?;
            wss.into_iter()
                .find(|w| Path::new(&w.path).canonicalize().is_ok_and(|p| here.starts_with(p)))
                .ok_or("This folder isn't one of NekoTrail's workspaces. Name the workspace instead: nekotrail handoff <name> --to codex")?
        } else {
            find_active(st, workspace)?
        };
        let sid = st.db().latest_chat(&w.id).map_err(crate::e)?.ok_or_else(|| format!("{} has no chat with a message yet, so there's nothing to hand off.", display(&w)))?;
        let from = st.db().session(&sid).map_err(crate::e)?.agent;
        let names = json!({"workspace": w.name, "title": w.title, "from": from, "to": to});
        if !now {
            crate::handoff_check(st, &sid, to)?;
            deliver(app, Job::Handoff { workspace_id: w.id.clone(), session_id: sid, agent: to.to_string(), model });
            return Ok(json!({"started": false, "names": names}));
        }
        let brief = crate::handoff_build(st, &sid)?;
        let s = crate::handoff_begin(app, st, &sid, to, model, None, &brief.text)?;
        deliver(app, Job::Focus { workspace_id: w.id.clone() });
        Ok(json!({"started": true, "names": names, "session_id": s.id, "mode": s.permission_mode, "chars": brief.text.chars().count(), "hidden": brief.hidden}))
    }

    /// An active workspace by id, name, branch or title.
    fn find_active(st: &AppState, query: &str) -> Result<Workspace, String> {
        let wss = st.db().workspaces(false).map_err(crate::e)?;
        let cands: Vec<Candidate> = wss.iter().map(|w| Candidate { id: &w.id, name: &w.name, branch: &w.branch, title: &w.title, in_cwd_repo: false }).collect();
        let i = find_workspace(query, &cands)?;
        Ok(wss[i].clone())
    }

    /// Text from your editor for a workspace's message box. Checked here; the window only shows it.
    pub fn draft(st: &AppState, workspace: &str, text: String) -> Result<Job, String> {
        if text.trim().is_empty() {
            return Err("There's nothing to send. Select some code first.".into());
        }
        if text.chars().count() > MAX_DRAFT {
            return Err(format!("That's too long for a message (over {MAX_DRAFT} characters)."));
        }
        let w = find_active(st, workspace)?;
        Ok(Job::Draft { workspace_id: w.id, text, note: None })
    }

    /// A task proposed from your editor for the repo at `cwd`. Like a link, it only parks the task
    /// and returns the question; nothing is created or sent until you answer it in the window.
    /// None when the same task is already waiting.
    pub fn propose(st: &AppState, cwd: &str, task: Task) -> Result<Option<Job>, String> {
        task.validate(MAX_EDITOR_PROMPT)?;
        let (repo_id, name, path) = match repo_for(st, cwd, false)? {
            Some((r, _)) => (Some(r.id), r.name, r.root_path),
            None => {
                let top = crate::git::toplevel(Path::new(cwd)).map_err(|_| format!("{cwd} isn't in a git repo."))?;
                let name = top.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                (None, name, top.to_string_lossy().to_string())
            }
        };
        Ok(ask_first(st, repo_id, name, path, task, "editor"))
    }

    /// Park a task behind a one-time token and return the question for the window.
    fn ask_first(st: &AppState, repo_id: Option<String>, name: String, path: String, task: Task, from: &str) -> Option<Job> {
        let item = LinkTask { repo_id: repo_id.clone(), repo_path: path.clone(), task: task.clone() };
        let token = st.inbox.pending.lock().unwrap().push(item)?;
        Some(Job::Confirm {
            token, from: from.into(), repo: name, repo_path: path, adds_repo: repo_id.is_none(), prompt: task.prompt.trim().to_string(),
            agent: task.agent.clone().unwrap_or_else(|| "claude".into()), base: task.base.clone(), plan: task.plan,
        })
    }

    fn queue_add(app: &AppHandle, st: &AppState, cwd: String, repo: Option<String>, task: QueueTask) -> Result<Value, String> {
        use crate::queue::{State, Then};
        task.validate()?;
        let (r, _) = repo_for(st, repo.as_deref().unwrap_or(&cwd), true)?.ok_or("That folder isn't a git repo.")?;
        let agent = crate::agent::get(task.agent.as_deref().unwrap_or("claude")).id().to_string();
        // Your default mode for that agent (Settings, Agents), like a new workspace.
        let mode = task.mode.clone()
            .or_else(|| st.db().setting(&format!("{agent}_mode")).ok().flatten().filter(|m| crate::queue::MODES.contains(&m.as_str())))
            .unwrap_or_else(|| "acceptEdits".into());
        let new = crate::queue::NewTask {
            prompt: task.prompt, repo_id: r.id.clone(), base: task.base, agent: agent.clone(), model: None, effort: None, mode,
            fallback: task.fallback, then: Then { checks: task.checks, draft_pr: task.pr, autopilot: task.autopilot },
        };
        let t = crate::queuerun::add(app, new)?;
        let q = st.queue.lock().unwrap();
        let ahead = q.tasks.iter().take_while(|x| x.id != t.id).filter(|x| x.state == State::Queued).count();
        Ok(json!({"repo": r.name, "agent": agent, "state": t.state, "ahead": ahead, "paused": q.paused}))
    }

    /// Every active workspace with its state, agent, PR and what it needs from you.
    pub fn rows(st: &AppState) -> Result<Vec<Value>, String> {
        let (running, approvals): (Vec<String>, Vec<String>) = {
            let runs = st.runs.lock().unwrap();
            (runs.running.keys().cloned().collect(), runs.approvals.values().map(|(sid, _, _)| sid.clone()).collect())
        };
        let db = st.db();
        let repos = db.repos().map_err(crate::e)?;
        let ws_of = |sid: &String| db.session(sid).ok().map(|s| s.workspace_id);
        let running_ws: Vec<String> = running.iter().filter_map(ws_of).collect();
        let asking_ws: Vec<String> = approvals.iter().filter_map(ws_of).collect();
        let mut out = Vec::new();
        for w in db.workspaces(false).map_err(crate::e)? {
            let agent = db.sessions(&w.id).ok().and_then(|s| s.last().map(|s| s.agent.clone()));
            let pr = crate::cached_pr(st, &w).filter(|p| !p.is_null());
            let working = running_ws.contains(&w.id);
            let asking = asking_ws.contains(&w.id);
            let mut needs: Vec<&str> = Vec::new();
            if asking {
                needs.push("waiting for your OK");
            }
            if w.unread && !working {
                needs.push("agent finished, not read yet");
            }
            if let Some(why) = pr.as_ref().and_then(pr_needs) {
                needs.push(why);
            }
            let state = if asking { "needs OK" } else if working { "working" } else if w.unread { "done, unread" } else { "idle" };
            out.push(json!({
                "id": w.id, "name": w.name, "title": w.title, "display": display(&w), "branch": w.branch,
                "path": w.path, "target_branch": w.target_branch,
                "repo_id": w.repo_id, "repo": repos.iter().find(|r| r.id == w.repo_id).map(|r| r.name.clone()),
                "state": state, "agent": agent, "pr": pr.as_ref().and_then(pr_summary), "pr_url": pr.as_ref().and_then(|p| p["url"].as_str().map(String::from)),
                "needs": needs,
            }));
        }
        Ok(out)
    }

    /// A `nekotrail://` link arrived. Open goes straight through; New waits for your OK.
    pub fn on_link(app: &AppHandle, url: &str) {
        match route_link(&app.state::<AppState>(), url) {
            Some(job) => deliver(app, job),
            None => show(app),
        }
    }

    /// What a link turns into. This is where the confirm rule lives: a link that would start work
    /// only parks it in `pending` and returns a question; nothing is created or sent here.
    /// None when the same task is already waiting for an answer.
    pub fn route_link(st: &AppState, url: &str) -> Option<Job> {
        let link = match parse_link(url) {
            Ok(l) => l,
            Err(text) => return Some(Job::Error { text }),
        };
        if !link.needs_confirm() {
            let Link::Open { workspace } = link else { unreachable!("only Open skips the question") };
            let wss = st.db().workspaces(false).unwrap_or_default();
            let cands: Vec<Candidate> = wss.iter().map(|w| Candidate { id: &w.id, name: &w.name, branch: &w.branch, title: &w.title, in_cwd_repo: false }).collect();
            return Some(match find_workspace(&workspace, &cands) {
                Ok(i) => Job::Focus { workspace_id: wss[i].id.clone() },
                Err(text) => Job::Error { text },
            });
        }
        let Link::New { repo, task } = link else { unreachable!("only New asks") };
        // A repo you have (by name or folder), or a folder that would be added. Nothing is added yet.
        let canon = Path::new(&repo).canonicalize().ok();
        let known = st.db().repos().unwrap_or_default().into_iter().find(|r| {
            r.name == repo || Path::new(&r.root_path) == Path::new(&repo) || canon.as_ref().is_some_and(|p| Path::new(&r.root_path).canonicalize().is_ok_and(|root| p.starts_with(root)))
        });
        let (repo_id, name, path) = match known {
            Some(r) => (Some(r.id), r.name, r.root_path),
            None if Path::new(&repo).is_absolute() && Path::new(&repo).is_dir() => {
                (None, Path::new(&repo).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), repo.clone())
            }
            None => return Some(Job::Error { text: format!("A link asked for the repo \"{repo}\", which isn't in NekoTrail. Add it first.") }),
        };
        ask_first(st, repo_id, name, path, task, "link")
    }

    /// Spend a link's token. The task comes back only when you said yes.
    pub fn answer_link(st: &AppState, token: &str, ok: bool) -> Result<Option<LinkTask>, String> {
        let item = st.inbox.pending.lock().unwrap().take(token).ok_or("That link's request has expired. Open the link again.")?;
        Ok(ok.then_some(item))
    }

    /// You answered a link's question. Only now does its task start.
    pub fn confirm(app: &AppHandle, token: &str, ok: bool) -> Result<(), String> {
        let st = app.state::<AppState>();
        let Some(item) = answer_link(&st, token, ok)? else { return Ok(()) };
        let repo_id = match item.repo_id {
            Some(id) => id,
            None => repo_for(&st, &item.repo_path, true)?.ok_or("That folder isn't a git repo.")?.0.id,
        };
        start(app, repo_id, item.task).map(|_| ())
    }
}

#[cfg(unix)]
pub use server::{confirm, on_link, serve};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_with_a_version() {
        let req = Request::New { cwd: "/r".into(), task: Task { prompt: "fix it".into(), agent: Some("codex".into()), base: None, plan: true, linear: None } };
        let line = encode(&req);
        assert!(line.ends_with('\n') && line.contains("\"v\":1") && line.contains("\"cmd\":\"new\""));
        assert_eq!(decode(&line).unwrap(), req);
        assert_eq!(decode(&encode(&Request::Status)).unwrap(), Request::Status);
        assert!(decode(r#"{"v":99,"cmd":"status"}"#).unwrap_err().contains("different versions"));
        assert!(decode(r#"{"cmd":"status"}"#).is_err());
        assert!(decode(r#"{"v":1,"cmd":"rm"}"#).is_err());
        assert!(decode("not json").is_err());
    }

    #[test]
    fn version_two_commands_round_trip_and_older_requests_still_work() {
        let reqs = [
            Request::Hello,
            Request::Propose { cwd: "/r".into(), task: Task { prompt: "fix src/a.rs:12".into(), ..Default::default() } },
            Request::Draft { workspace: "mochi".into(), text: "look at this".into() },
            Request::Changes { workspace: "mochi".into() },
        ];
        for req in reqs {
            let line = encode(&req);
            assert!(line.contains("\"v\":2"), "{line}");
            assert_eq!(decode(&line).unwrap(), req);
        }
        // Old commands go out as v1, so an app that only speaks v1 still answers them.
        assert!(encode(&Request::Ls { cwd: None, all: true }).contains("\"v\":1"));
        assert_eq!(decode(r#"{"v":2,"cmd":"status"}"#).unwrap(), Request::Status);
        // A v2 command sent as v1 is refused by name, not misread.
        assert!(decode(r#"{"v":1,"cmd":"hello"}"#).unwrap_err().contains("\"hello\" needs protocol 2"));
        assert!(decode(r#"{"v":0,"cmd":"status"}"#).unwrap_err().contains("different versions"));
        assert!(decode(r#"{"v":4,"cmd":"hello"}"#).unwrap_err().contains("different versions"));
        assert!(decode(r#"{"v":2,"cmd":"draft","workspace":"x"}"#).is_err());
        // v3: handoff. An app on v2 says the versions differ; a v2 line naming it is refused by name.
        let h = Request::Handoff { cwd: "/code/ws/mochi".into(), workspace: ".".into(), to: "codex".into(), model: None, now: false };
        let line = encode(&h);
        assert!(line.contains("\"v\":3") && line.contains("\"cmd\":\"handoff\""), "{line}");
        assert_eq!(decode(&line).unwrap(), h);
        assert!(decode(r#"{"v":2,"cmd":"handoff","cwd":"/","workspace":"x","to":"codex","model":null,"now":true}"#).unwrap_err().contains("\"handoff\" needs protocol 3"));
        assert_eq!(decode(r#"{"v":3,"cmd":"hello"}"#).unwrap(), Request::Hello, "older commands still answer at v3");
        // The exact lines the editor extension writes (extensions/vscode/src/protocol.ts).
        assert_eq!(
            decode(r#"{"v":2,"cmd":"propose","cwd":"/r/src","task":{"prompt":"fix it","agent":null}}"#).unwrap(),
            Request::Propose { cwd: "/r/src".into(), task: Task { prompt: "fix it".into(), ..Default::default() } }
        );
        assert_eq!(decode(r#"{"v":1,"cmd":"ls","cwd":"/r","all":false}"#).unwrap(), Request::Ls { cwd: Some("/r".into()), all: false });
        assert_eq!(decode(r#"{"v":1,"cmd":"open","cwd":null,"query":"w1"}"#).unwrap(), Request::Open { cwd: None, query: "w1".into() });
    }

    #[test]
    fn queue_requests_round_trip_and_are_checked() {
        let task = QueueTask { prompt: "fix it".into(), agent: Some("codex".into()), fallback: Some("claude".into()), mode: Some("plan".into()), checks: true, pr: true, ..Default::default() };
        let req = Request::QueueAdd { cwd: "/r".into(), repo: None, task: task.clone() };
        let line = encode(&req);
        assert!(line.contains("\"cmd\":\"queue_add\""));
        assert_eq!(decode(&line).unwrap(), req);
        assert_eq!(decode(&encode(&Request::QueueLs)).unwrap(), Request::QueueLs);
        assert!(task.validate().is_ok());
        let bad = |f: fn(&mut QueueTask)| { let mut t = task.clone(); f(&mut t); t.validate().unwrap_err() };
        assert!(bad(|t| t.prompt = " ".into()).contains("nekotrail queue add"));
        assert!(bad(|t| t.fallback = Some("codex".into())).contains("different agent"));
        assert!(bad(|t| { t.agent = None; t.fallback = Some("claude".into()) }).contains("different agent"));
        assert!(bad(|t| t.fallback = Some("skynet".into())).contains("skynet"));
        assert!(bad(|t| t.mode = Some("yolo".into())).contains("plan, ask, edits or auto"));
        assert!(bad(|t| t.base = Some("-x".into())).contains("branch"));
    }

    #[test]
    fn answers_round_trip() {
        assert_eq!(parse_answer(&answer(&Ok(json!({"a": 1})))).unwrap(), json!({"a": 1}));
        assert_eq!(parse_answer(&answer(&Err("nope".into()))).unwrap_err(), "nope");
        assert!(parse_answer("garbage").is_err());
    }

    #[test]
    fn tasks_are_checked() {
        let t = |p: &str, a: Option<&str>, b: Option<&str>| Task { prompt: p.into(), agent: a.map(Into::into), base: b.map(Into::into), plan: false, linear: None };
        assert!(t("fix", None, None).validate(10).is_ok());
        assert!(t("  ", None, None).validate(10).is_err());
        assert!(t("x".repeat(11).as_str(), None, None).validate(10).unwrap_err().contains("too long"));
        assert!(t("fix", Some("codex"), Some("main")).validate(10).is_ok());
        assert!(t("fix", Some("skynet"), None).validate(10).unwrap_err().contains("codex"));
        assert!(t("fix", None, Some("--upload-pack=x")).validate(10).is_err());
    }

    #[test]
    fn links_parse() {
        assert_eq!(
            parse_link("nekotrail://new?repo=%2FUsers%2Fme%2Fapp&prompt=fix+the%20login%20bug&agent=codex&base=main&plan=1").unwrap(),
            Link::New { repo: "/Users/me/app".into(), task: Task { prompt: "fix the login bug".into(), agent: Some("codex".into()), base: Some("main".into()), plan: true, linear: None } }
        );
        assert_eq!(parse_link("NEKOTRAIL://open/?workspace=mochi#x").unwrap(), Link::Open { workspace: "mochi".into() });
        assert_eq!(parse_link("nekotrail:open?workspace=caf%C3%A9").unwrap(), Link::Open { workspace: "café".into() });
        let new = parse_link("nekotrail://new?repo=app&prompt=go").unwrap();
        assert!(matches!(&new, Link::New { task, .. } if !task.plan && task.agent.is_none()));
    }

    #[test]
    fn bad_links_are_refused() {
        for url in [
            "https://new?repo=a&prompt=b",
            "nekotrail://rm?repo=a",
            "nekotrail://new?prompt=b",
            "nekotrail://new?repo=a",
            "nekotrail://new?repo=a&prompt=%zz",
            "nekotrail://new?repo=a&prompt=one&prompt=two",
            "nekotrail://new?repo=a&prompt=b&agent=skynet",
            "nekotrail://new?repo=a&prompt=b&base=-x",
            "nekotrail://open",
            "nekotrail",
        ] {
            assert!(parse_link(url).is_err(), "{url}");
        }
        let long = format!("nekotrail://new?repo=a&prompt={}", "x".repeat(MAX_LINK_PROMPT + 1));
        assert!(parse_link(&long).unwrap_err().contains("too long"));
    }

    #[test]
    fn links_that_start_work_need_confirmation() {
        assert!(parse_link("nekotrail://new?repo=a&prompt=b").unwrap().needs_confirm());
        assert!(!parse_link("nekotrail://open?workspace=a").unwrap().needs_confirm());
    }

    #[test]
    fn pending_tokens_work_once_and_expire() {
        let mut p = Pending::new(Duration::from_secs(60));
        let t = p.push("a").unwrap();
        assert!(p.push("a").is_none(), "the same task waiting twice asks once");
        assert_eq!(p.take("not-a-token"), None);
        assert_eq!(p.take(&t), Some("a"));
        assert_eq!(p.take(&t), None, "a token works once");
        let t2 = p.push("a").unwrap();
        assert_ne!(t, t2);
        // Only the newest five wait.
        let first = p.push("b").unwrap();
        for x in ["c", "d", "e", "f", "g"] {
            p.push(x);
        }
        assert_eq!(p.take(&first), None);
        let mut gone = Pending::new(Duration::ZERO);
        let t = gone.push(1).unwrap();
        assert_eq!(gone.take(&t), None, "expired");
    }

    #[test]
    fn finds_workspaces_by_name_branch_or_title() {
        let c = |id: &'static str, name: &'static str, branch: &'static str, title: &'static str, here: bool| Candidate { id, name, branch, title, in_cwd_repo: here };
        let all = [
            c("1", "mochi", "me/mochi", "Fix the login bug", false),
            c("2", "luna", "me/luna", "Add dark mode", true),
            c("3", "mochi-2", "me/mochi-2", "Fix the logout bug", true),
        ];
        assert_eq!(find_workspace("mochi", &all), Ok(0));
        assert_eq!(find_workspace("me/luna", &all), Ok(1));
        assert_eq!(find_workspace("Luna", &all), Ok(1));
        assert_eq!(find_workspace("add dark mode", &all), Ok(1));
        assert_eq!(find_workspace("dark", &all), Ok(1));
        assert_eq!(find_workspace("3", &all), Ok(2));
        // Two titles match; the one in your current repo wins.
        assert_eq!(find_workspace("fix the", &all), Ok(2));
        let elsewhere = [c("1", "a", "me/a", "Fix x", false), c("2", "b", "me/b", "Fix y", false)];
        assert!(find_workspace("fix", &elsewhere).unwrap_err().contains("matches 2"));
        assert!(find_workspace("nothing", &all).unwrap_err().contains("nekotrail ls"));
        assert!(find_workspace(" ", &all).is_err());
    }

    #[test]
    fn pr_lines() {
        let pr = |v: Value| v;
        assert_eq!(pr_summary(&pr(json!({"number": 7, "state": "MERGED"}))).unwrap(), "#7 merged");
        assert_eq!(pr_summary(&pr(json!({"number": 7, "state": "OPEN", "statusCheckRollup": [{"conclusion": "FAILURE"}]}))).unwrap(), "#7 open, checks failing");
        assert_eq!(pr_summary(&pr(json!({"number": 7, "state": "OPEN", "isDraft": true, "statusCheckRollup": [{"status": "IN_PROGRESS"}]}))).unwrap(), "#7 draft, checks running");
        assert_eq!(pr_summary(&pr(json!({"number": 7, "state": "OPEN", "statusCheckRollup": []}))).unwrap(), "#7 open, ready to merge");
        assert_eq!(pr_summary(&Value::Null), None);
        assert_eq!(pr_needs(&pr(json!({"number": 7, "state": "OPEN", "mergeable": "CONFLICTING"}))), Some("PR has merge conflicts"));
        assert_eq!(pr_needs(&pr(json!({"number": 7, "state": "OPEN", "isDraft": true, "mergeable": "CONFLICTING"}))), None);
        assert_eq!(pr_needs(&pr(json!({"number": 7, "state": "MERGED"}))), None);
    }

    #[cfg(unix)]
    #[test]
    fn socket_protocol_over_a_real_socket() {
        use std::os::unix::net::{UnixListener, UnixStream};
        let dir = tempfile::tempdir().unwrap();
        let path = socket_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new((&conn).take(MAX_REQUEST)).read_line(&mut line).unwrap();
            let res = decode(&line).map(|r| json!({"echo": format!("{r:?}")}));
            conn.write_all(answer(&res).as_bytes()).unwrap();
        });
        let mut c = UnixStream::connect(&path).unwrap();
        c.write_all(encode(&Request::Open { cwd: None, query: "mochi".into() }).as_bytes()).unwrap();
        let mut back = String::new();
        BufReader::new(&c).read_line(&mut back).unwrap();
        server.join().unwrap();
        assert!(parse_answer(&back).unwrap()["echo"].as_str().unwrap().contains("mochi"));
    }

    #[cfg(unix)]
    fn state(home: &Path) -> crate::AppState {
        use std::sync::{Arc, OnceLock};
        crate::AppState {
            store: crate::lockorder::Ordered::new(crate::store::Store::in_memory().unwrap()), runs: Default::default(), ptys: Default::default(),
            home: home.to_path_buf(), branch_prefix: Arc::new(OnceLock::from("me".to_string())), github: Default::default(), repo_meta: Default::default(), lines: Default::default(),
            prs: Default::default(), pr_wake: Default::default(), inbox: Default::default(), port_failures: Default::default(), checks: crate::checks::Checks::new(home.join("checks")), queue: Default::default(),
        }
    }

    #[cfg(unix)]
    fn run_git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git").arg("-C").arg(dir).args(args)
            .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t")
            .output().unwrap().status.success();
        assert!(ok, "git {args:?}");
    }

    #[cfg(unix)]
    #[test]
    fn finds_the_repo_for_a_folder_and_adds_it_once() {
        use super::server::repo_for;
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let root = base.join("app");
        std::fs::create_dir_all(root.join("src")).unwrap();
        run_git(&root, &["init", "-q", "-b", "main"]);
        run_git(&root, &["commit", "-q", "--allow-empty", "-m", "first"]);
        let st = state(&base);
        let s = |p: &Path| p.to_string_lossy().to_string();
        // Only `add` may add a repo.
        assert!(repo_for(&st, &s(&root), false).unwrap().is_none());
        let (r, added) = repo_for(&st, &s(&root.join("src")), true).unwrap().unwrap();
        assert!(added);
        assert_eq!(Path::new(&r.root_path).canonicalize().unwrap(), root);
        let (again, added) = repo_for(&st, &s(&root), true).unwrap().unwrap();
        assert!(!added && again.id == r.id);
        assert_eq!(repo_for(&st, &s(&root.join("src")), false).unwrap().unwrap().0.id, r.id);
        // A linked worktree is the same repo, not a new one.
        let wt = base.join("wt");
        run_git(&root, &["worktree", "add", "-q", "-b", "side", &s(&wt)]);
        let (from_wt, added) = repo_for(&st, &s(&wt), true).unwrap().unwrap();
        assert!(!added && from_wt.id == r.id);
        assert_eq!(st.db().repos().unwrap().len(), 1);
        // Not a repo, or not there at all.
        let plain = base.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert!(repo_for(&st, &s(&plain), true).unwrap_err().contains("isn't in a git repo"));
        assert!(repo_for(&st, &s(&base.join("nope")), true).unwrap_err().contains("doesn't exist"));
    }

    #[cfg(unix)]
    #[test]
    fn rows_say_what_each_workspace_needs() {
        use super::server::rows;
        use crate::store::{Repo, Session, Workspace};
        let t = tempfile::tempdir().unwrap();
        let st = state(t.path());
        let repo = Repo {
            id: "r".into(), name: "app".into(), root_path: "/x/app".into(), default_branch: "main".into(),
            remote_url: None, setup_script: None, run_script: None, archive_script: None, prefs: Default::default(),
        };
        st.db().insert_repo(&repo).unwrap();
        let ws = |id: &str, unread: bool| Workspace {
            id: id.into(), repo_id: "r".into(), name: format!("ws-{id}"), branch: format!("me/ws-{id}"), path: format!("/x/w/{id}"),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread, notes: String::new(),
            imported_from: None, created_at: String::new(), title: String::new(), status: String::new(), pair_id: String::new(), section_id: String::new(), parent_id: String::new(), title_source: String::new(), review_pr: 0, group_id: String::new(),
        };
        let chat = |id: &str, ws: &str, agent: &str| Session {
            id: id.into(), workspace_id: ws.into(), agent: agent.into(), agent_session_id: None, title: "New chat".into(), model: None, permission_mode: "acceptEdits".into(), effort: None,
        };
        for (w, unread) in [("idle", false), ("unread", true), ("busy", false), ("asks", false), ("pr", false)] {
            st.db().insert_workspace(&ws(w, unread)).unwrap();
            st.db().insert_session(&chat(&format!("s-{w}"), w, "codex")).unwrap();
        }
        {
            let mut runs = st.runs.lock().unwrap();
            runs.running.insert("s-busy".into(), 1);
            runs.running.insert("s-asks".into(), 2);
            runs.approvals.insert("req".into(), ("s-asks".into(), Value::Null, Value::Null));
        }
        st.prs.lock().unwrap().by_branch.insert(("r".into(), "me/ws-pr".into()), json!({"number": 4, "state": "OPEN", "statusCheckRollup": [{"conclusion": "FAILURE"}]}));
        let rows = rows(&st).unwrap();
        let get = |id: &str| rows.iter().find(|r| r["id"] == id).unwrap().clone();
        assert_eq!(get("idle")["state"], "idle");
        assert_eq!(get("idle")["needs"], json!([]));
        assert_eq!(get("idle")["agent"], "codex");
        // v2: the editor extension opens the worktree and diffs it against its target.
        assert_eq!((get("idle")["path"].as_str(), get("idle")["target_branch"].as_str()), (Some("/x/w/idle"), Some("main")));
        assert_eq!(get("unread")["needs"], json!(["agent finished, not read yet"]));
        assert_eq!(get("busy")["state"], "working");
        assert_eq!(get("busy")["needs"], json!([]));
        assert_eq!(get("asks")["state"], "needs OK");
        assert_eq!(get("asks")["needs"], json!(["waiting for your OK"]));
        assert_eq!(get("pr")["pr"], "#4 open, checks failing");
        assert_eq!(get("pr")["needs"], json!(["PR checks are failing"]));
        assert_eq!(get("pr")["repo"], "app");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_never_starts_work_without_your_answer() {
        use super::server::{answer_link, route_link};
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let st = state(&base);
        let repo = crate::store::Repo {
            id: "r".into(), name: "app".into(), root_path: base.to_string_lossy().into(), default_branch: "main".into(),
            remote_url: None, setup_script: None, run_script: None, archive_script: None, prefs: Default::default(),
        };
        st.db().insert_repo(&repo).unwrap();
        let url = "nekotrail://new?repo=app&prompt=delete+everything&agent=codex";
        let Some(Job::Confirm { token, repo, prompt, agent, adds_repo, .. }) = route_link(&st, url) else { panic!("a new-task link must ask") };
        assert_eq!((repo.as_str(), prompt.as_str(), agent.as_str(), adds_repo), ("app", "delete everything", "codex", false));
        // Asking created nothing.
        assert!(st.db().workspaces(true).unwrap().is_empty());
        // The page firing the same link again doesn't stack a second question.
        assert_eq!(route_link(&st, url), None);
        // A made-up token does nothing; "no" spends the real one; it can't be replayed as "yes".
        assert!(answer_link(&st, "guess", true).is_err());
        assert_eq!(answer_link(&st, &token, false), Ok(None));
        assert!(answer_link(&st, &token, true).is_err());
        // "Yes" hands back exactly the task that was shown.
        let Some(Job::Confirm { token, .. }) = route_link(&st, url) else { panic!() };
        let task = answer_link(&st, &token, true).unwrap().unwrap();
        assert_eq!((task.repo_id.as_deref(), task.task.prompt.as_str(), task.task.agent.as_deref()), (Some("r"), "delete everything", Some("codex")));
        // A repo that isn't in nekotrail and isn't a folder is refused up front.
        assert!(matches!(route_link(&st, "nekotrail://new?repo=nope&prompt=x"), Some(Job::Error { .. })));
        // A folder inside a repo you have is that repo; a folder outside any asks, and says it would be added.
        std::fs::create_dir_all(base.join("sub")).unwrap();
        let sub = format!("nekotrail://new?repo={}%2Fsub&prompt=y", base.to_string_lossy().replace('/', "%2F"));
        assert!(matches!(route_link(&st, &sub), Some(Job::Confirm { adds_repo: false, ref repo, .. }) if repo == "app"));
        let elsewhere = tempfile::tempdir().unwrap();
        let other = elsewhere.path().canonicalize().unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let link = format!("nekotrail://new?repo={}&prompt=x", other.to_string_lossy().replace('/', "%2F"));
        assert!(matches!(route_link(&st, &link), Some(Job::Confirm { adds_repo: true, .. })));
        // Open goes straight to the workspace, or says it can't find one.
        assert!(matches!(route_link(&st, "nekotrail://open?workspace=mochi"), Some(Job::Error { .. })));
        assert!(matches!(route_link(&st, "nekotrail://bogus"), Some(Job::Error { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn the_editor_proposes_a_task_and_nothing_starts_until_you_answer() {
        use super::server::{answer_link, propose};
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let root = base.join("app");
        std::fs::create_dir_all(root.join("src")).unwrap();
        run_git(&root, &["init", "-q", "-b", "main"]);
        run_git(&root, &["commit", "-q", "--allow-empty", "-m", "first"]);
        let st = state(&base);
        let s = |p: &Path| p.to_string_lossy().to_string();
        let task = |p: &str| Task { prompt: p.into(), agent: Some("codex".into()), ..Default::default() };
        // A repo nekotrail doesn't have yet: the question says it would be added, and nothing is added.
        let Some(Job::Confirm { token, from, repo, adds_repo, prompt, .. }) = propose(&st, &s(&root.join("src")), task("explain src/a.rs:3")).unwrap() else { panic!("a proposal must ask") };
        assert_eq!((from.as_str(), repo.as_str(), adds_repo, prompt.as_str()), ("editor", "app", true, "explain src/a.rs:3"));
        assert!(st.db().repos().unwrap().is_empty());
        assert!(st.db().workspaces(true).unwrap().is_empty());
        // The same proposal twice asks once; "no" spends the token.
        assert_eq!(propose(&st, &s(&root), task("explain src/a.rs:3")).unwrap(), None);
        assert_eq!(answer_link(&st, &token, false), Ok(None));
        assert!(answer_link(&st, &token, true).is_err());
        // Longer than a link may be, up to the editor's limit; empty or too long is refused.
        assert!(propose(&st, &s(&root), task(&"x".repeat(MAX_LINK_PROMPT + 1))).unwrap().is_some());
        assert!(propose(&st, &s(&root), task(&"x".repeat(MAX_EDITOR_PROMPT + 1))).unwrap_err().contains("too long"));
        assert!(propose(&st, &s(&root), task("  ")).is_err());
        // Outside a repo.
        let plain = base.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert!(propose(&st, &s(&plain), task("x")).unwrap_err().contains("isn't in a git repo"));
    }

    #[cfg(unix)]
    #[test]
    fn the_editor_drafts_into_a_workspace_and_never_sends() {
        use super::server::draft;
        use crate::store::{Repo, Workspace};
        let t = tempfile::tempdir().unwrap();
        let st = state(t.path());
        st.db().insert_repo(&Repo {
            id: "r".into(), name: "app".into(), root_path: "/x/app".into(), default_branch: "main".into(),
            remote_url: None, setup_script: None, run_script: None, archive_script: None, prefs: Default::default(),
        }).unwrap();
        st.db().insert_workspace(&Workspace {
            id: "w1".into(), repo_id: "r".into(), name: "mochi".into(), branch: "me/mochi".into(), path: "/x/w/mochi".into(),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: String::new(), title: String::new(), status: String::new(), pair_id: String::new(), section_id: String::new(), parent_id: String::new(), title_source: String::new(), review_pr: 0, group_id: String::new(),
        }).unwrap();
        assert_eq!(draft(&st, "w1", "see a.rs:3".into()), Ok(Job::Draft { workspace_id: "w1".into(), text: "see a.rs:3".into(), note: None }));
        assert_eq!(draft(&st, "me/mochi", "x".into()).map(|j| matches!(j, Job::Draft { .. })), Ok(true));
        assert!(draft(&st, "w1", " \n".into()).unwrap_err().contains("nothing to send"));
        assert!(draft(&st, "w1", "x".repeat(MAX_DRAFT + 1)).unwrap_err().contains("too long"));
        assert!(draft(&st, "nope", "x".into()).is_err());
        // Nothing ran.
        assert!(st.runs.lock().unwrap().running.is_empty());
    }
}
