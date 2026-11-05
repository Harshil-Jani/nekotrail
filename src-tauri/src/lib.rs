pub mod agent;
pub mod config;
pub mod git;
pub mod importer;
pub mod names;
pub mod pty;
pub mod store;

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
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
    /// Chats whose next turn should fork the agent's conversation instead of continuing it.
    forks: std::collections::HashSet<String>,
}

pub struct AppState {
    store: Mutex<Store>,
    runs: Arc<Mutex<Runs>>,
    ptys: pty::Ptys,
    home: PathBuf,
    branch_prefix: String,
    github: Mutex<Option<Value>>,
    repo_meta: Mutex<HashMap<String, Value>>,
}

impl AppState {
    fn db(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }
}

/// GUI apps on macOS start without your shell's PATH and env (CLAUDE_CONFIG_DIR, API keys...).
/// Copy them from a login shell so `claude`, `codex`, `gh` and `git` behave like in your terminal.
fn load_login_env() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
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
    let name = spec.rsplit(['/', ':']).next().filter(|n| !n.is_empty()).ok_or("Type a repo like owner/name")?.to_string();
    let dest = st.home.join("repos").join(&name);
    if dest.exists() {
        return add_repo_at(&st, &dest.to_string_lossy());
    }
    std::fs::create_dir_all(dest.parent().unwrap()).map_err(e)?;
    gh(".", &["repo", "clone", &spec, &dest.to_string_lossy()])?;
    add_repo_at(&st, &dest.to_string_lossy())
}

fn add_repo_at(st: &AppState, path: &str) -> Res<Repo> {
    let root = git::toplevel(Path::new(&path)).map_err(|_| format!("{path} is not inside a git repo"))?;
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

#[tauri::command]
async fn workspace_scripts(st: State<'_, AppState>, id: String) -> Res<config::Scripts> {
    let ws = st.db().workspace(&id).map_err(e)?;
    scripts_for(&st, &ws)
}

// ---------- workspaces ----------

/// Environment for anything running in a workspace: the repo's variables plus workspace facts.
/// Each workspace gets 10 ports from `NEKOTRAIL_PORT`, stable for its lifetime.
fn workspace_env(repo: &Repo, ws: &Workspace) -> Vec<(String, String)> {
    let hash = ws.id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let port = 41000 + (hash % 1800) * 10; // 41000..58990
    let mut env = vec![
        ("NEKOTRAIL_PORT".to_string(), port.to_string()),
        ("NEKOTRAIL_WORKSPACE_NAME".to_string(), ws.name.clone()),
        ("NEKOTRAIL_WORKSPACE_PATH".to_string(), ws.path.clone()),
        ("NEKOTRAIL_ROOT_PATH".to_string(), repo.root_path.clone()),
        ("NEKOTRAIL_DEFAULT_BRANCH".to_string(), ws.target_branch.clone()),
    ];
    env.extend(repo.prefs.env.iter().cloned());
    env
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
    for g in globs.iter().map(|g| g.trim()).filter(|g| !g.is_empty() && !g.contains("..")) {
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
    let dir = st.home.join("workspaces").join(&repo.name);
    while taken.contains(&name) || dir.join(&name).exists() || git::branch_exists(&root, &format!("{}/{name}", st.branch_prefix)) {
        name = names::pick(&taken.iter().cloned().chain([name.clone()]).collect::<Vec<_>>());
    }
    let path = dir.join(&name);
    // Start from an existing PR or branch ("pr:123" / "branch:feature-x"), or a fresh branch.
    let (branch, base) = match from.as_deref().and_then(|f| f.split_once(':')) {
        Some(("pr", n)) => {
            let info: Value = serde_json::from_str(&gh(&repo.root_path, &["pr", "view", n, "--json", "headRefName,isCrossRepository,baseRefName"])?).map_err(e)?;
            let head = info["headRefName"].as_str().unwrap_or("pr").to_string();
            let local = if info["isCrossRepository"].as_bool().unwrap_or(false) { format!("pr-{n}-{head}") } else { head };
            if !git::branch_exists(&root, &local) {
                git::git(&root, &["fetch", "--quiet", "origin", &format!("pull/{n}/head:{local}")]).map_err(e)?;
            }
            git::restore_worktree(&root, &path, &local).map_err(e)?;
            (local, info["baseRefName"].as_str().unwrap_or(&base).to_string())
        }
        Some(("branch", b)) => {
            let _ = git::git(&root, &["fetch", "--quiet", "origin", b]);
            if !git::branch_exists(&root, b) {
                git::git(&root, &["branch", "--track", b, &format!("origin/{b}")]).map_err(e)?;
            }
            git::restore_worktree(&root, &path, b).map_err(e)?;
            (b.to_string(), base)
        }
        _ => {
            let branch = format!("{}/{name}", st.branch_prefix);
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
    };
    let prefs = st.db().repo(&ws.repo_id).map_err(e)?.prefs;
    copy_into_workspace(&root, &path, &prefs.copy_files);
    st.db().insert_workspace(&ws).map_err(e)?;
    Ok(ws)
}

/// Runs the archive script, removes the worktree folder and keeps the branch.
/// Refuses when there is uncommitted work unless `force` is set.
#[tauri::command]
async fn archive_workspace(st: State<'_, AppState>, id: String, force: bool) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let path = Path::new(&ws.path);
    if path.exists() {
        if !force && git::is_dirty(path).unwrap_or(false) {
            return Err("DIRTY".into());
        }
        let repo = st.db().repo(&ws.repo_id).map_err(e)?;
        if let Some(script) = scripts_for(&st, &ws)?.archive {
            let _ = Command::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()))
                .args(["-lc", &script])
                .current_dir(path)
                .envs(workspace_env(&repo, &ws))
                .status();
        }
        git::remove_worktree(Path::new(&repo.root_path), path).map_err(e)?;
    }
    st.ptys.close(&format!("{id}:term"));
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
async fn set_workspace_title(st: State<'_, AppState>, id: String, title: String) -> Res<()> {
    st.db().set_title(&id, title.trim()).map_err(e)
}

#[tauri::command]
async fn set_workspace_status(st: State<'_, AppState>, id: String, status: String) -> Res<()> {
    st.db().set_status(&id, &status).map_err(e)
}

/// Rename the workspace's branch (only before it has been pushed).
#[tauri::command]
async fn rename_branch(st: State<'_, AppState>, id: String, branch: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let dir = Path::new(&ws.path);
    let branch = branch.trim().to_string();
    if branch.is_empty() || branch == ws.branch {
        return Ok(());
    }
    if git::git(dir, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/origin/{}", ws.branch)]).is_ok() {
        return Err("This branch is already pushed. Renaming it would break the PR.".into());
    }
    git::git(dir, &["check-ref-format", "--branch", &branch]).map_err(|_| format!("\"{branch}\" is not a valid branch name"))?;
    git::git(dir, &["branch", "-m", &ws.branch, &branch]).map_err(e)?;
    st.db().set_branch(&id, &branch).map_err(e)
}

#[tauri::command]
async fn set_repo_prefs(st: State<'_, AppState>, id: String, prefs: store::RepoPrefs) -> Res<()> {
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
        return Err(format!("{dir} is not a folder"));
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
    Ok(json!({
        "caffeinate": get("caffeinate"), "auto_branch": get("auto_branch"), "home": st.home.to_string_lossy(),
        "claude_model": raw("claude_model"), "claude_effort": raw("claude_effort"), "claude_mode": raw("claude_mode"),
        "codex_model": raw("codex_model"), "codex_effort": raw("codex_effort"), "codex_mode": raw("codex_mode"),
    }))
}

#[tauri::command]
async fn set_app_setting(st: State<'_, AppState>, key: String, value: String) -> Res<()> {
    const KEYS: &[&str] = &["caffeinate", "auto_branch", "claude_model", "claude_effort", "claude_mode", "codex_model", "codex_effort", "codex_mode"];
    if !KEYS.contains(&key.as_str()) {
        return Err(format!("unknown setting {key}"));
    }
    st.db().set_setting(&key, &value).map_err(e)
}

#[tauri::command]
async fn set_target_branch(st: State<'_, AppState>, id: String, branch: String) -> Res<()> {
    st.db().set_target_branch(&id, &branch).map_err(e)
}

#[tauri::command]
async fn changes(st: State<'_, AppState>, id: String, uncommitted: Option<bool>) -> Res<Vec<git::FileChange>> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let dir = Path::new(&ws.path);
    if uncommitted.unwrap_or(false) { git::changes_since(dir, "HEAD") } else { git::changes(dir, &ws.target_branch) }.map_err(e)
}

#[tauri::command]
async fn file_diff(st: State<'_, AppState>, id: String, file: String, uncommitted: Option<bool>) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let dir = Path::new(&ws.path);
    if uncommitted.unwrap_or(false) { git::file_diff_since(dir, "HEAD", &file) } else { git::file_diff(dir, &ws.target_branch, &file) }.map_err(e)
}

/// A path inside the workspace, refusing anything that escapes it.
fn inside(ws: &Workspace, rel: &str) -> Res<PathBuf> {
    let p = Path::new(rel);
    if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("That file is outside the workspace.".into());
    }
    Ok(Path::new(&ws.path).join(p))
}

#[tauri::command]
async fn read_file(st: State<'_, AppState>, id: String, path: String) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let full = inside(&ws, &path)?;
    if std::fs::metadata(&full).map(|m| m.len()).unwrap_or(0) > 2_000_000 {
        return Err("This file is over 2 MB; open it in your editor instead.".into());
    }
    std::fs::read_to_string(&full).map_err(|_| "This file isn't text, or it can't be read.".to_string())
}

#[tauri::command]
async fn write_file(st: State<'_, AppState>, id: String, path: String, content: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    std::fs::write(inside(&ws, &path)?, content).map_err(e)
}

/// Review comments left on the workspace's PR on GitHub (line comments, not general ones).
#[tauri::command]
async fn pr_review_comments(st: State<'_, AppState>, id: String) -> Res<Value> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let n = gh(&ws.path, &["pr", "view", &ws.branch, "--json", "number", "--jq", ".number"])?;
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

#[tauri::command]
async fn open_in(st: State<'_, AppState>, id: String, app: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let mut c = Command::new("open");
    if app == "finder" {
        c.arg(&ws.path);
    } else {
        let name = OPEN_IN.iter().find(|(k, _)| *k == app).map(|(_, n)| *n).ok_or(format!("unknown app {app}"))?;
        c.args(["-a", name, &ws.path]);
    }
    c.spawn().map_err(e)?;
    Ok(())
}

/// Apps a workspace can open in: (id, macOS app name).
const OPEN_IN: &[(&str, &str)] = &[
    ("cursor", "Cursor"), ("vscode", "Visual Studio Code"), ("zed", "Zed"), ("windsurf", "Windsurf"),
    ("xcode", "Xcode"), ("intellij", "IntelliJ IDEA"), ("webstorm", "WebStorm"), ("sublime", "Sublime Text"),
    ("terminal", "Terminal"), ("iterm", "iTerm"), ("warp", "Warp"), ("ghostty", "Ghostty"),
];

/// Which of those apps are installed, plus Finder.
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

/// PR state (OPEN / MERGED / CLOSED) per active workspace, one `gh` call per repo.
/// Merged ones are recorded, so their stars shine even if you never opened them.
#[tauri::command]
async fn pr_states(st: State<'_, AppState>) -> Res<HashMap<String, String>> {
    let (repos, wss) = {
        let db = st.db();
        (db.repos().map_err(e)?, db.workspaces(false).map_err(e)?)
    };
    let mut out = HashMap::new();
    for r in repos {
        if r.remote_url.is_none() || !wss.iter().any(|w| w.repo_id == r.id) {
            continue;
        }
        let Ok(list) = gh(&r.root_path, &["pr", "list", "--state", "all", "--limit", "300", "--json", "headRefName,state"]) else { continue };
        let prs: Vec<Value> = serde_json::from_str(&list).unwrap_or_default();
        for w in wss.iter().filter(|w| w.repo_id == r.id) {
            // gh lists newest first, so the first match is the current PR for that branch.
            if let Some(state) = prs.iter().find(|p| p["headRefName"] == w.branch.as_str()).and_then(|p| p["state"].as_str()) {
                if state == "MERGED" {
                    let _ = st.db().set_merged(&w.id);
                }
                out.insert(w.id.clone(), state.to_string());
            }
        }
    }
    Ok(out)
}

/// Lines added and removed per active workspace versus its target branch (for the sidebar).
/// Runs the git calls in parallel; a workspace that fails is just left out.
#[tauri::command]
async fn line_counts(st: State<'_, AppState>) -> Res<HashMap<String, (u32, u32)>> {
    let wss = st.db().workspaces(false).map_err(e)?;
    let handles: Vec<_> = wss
        .into_iter()
        .map(|w| std::thread::spawn(move || {
            let c = git::changes(Path::new(&w.path), &w.target_branch).ok()?;
            Some((w.id, (c.iter().map(|f| f.additions).sum(), c.iter().map(|f| f.deletions).sum())))
        }))
        .collect();
    Ok(handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect())
}

// ---------- pull requests (gh) ----------

fn gh(dir: &str, args: &[&str]) -> Res<String> {
    let out = Command::new("gh").args(args).current_dir(dir).output().map_err(e)?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// PR for the workspace branch (open, merged or closed), or null when there is none yet.
/// `gh pr list --head` also finds PRs opened from a fork, which `gh pr view <branch>` misses.
#[tauri::command]
async fn pr_status(st: State<'_, AppState>, id: String) -> Res<Value> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let out = gh(&ws.path, &[
        "pr", "list", "--head", &ws.branch, "--state", "all", "--limit", "1", "--json",
        "number,url,state,title,isDraft,reviewDecision,mergeable,statusCheckRollup,author,latestReviews,reviewRequests,additions,deletions,comments",
    ])?;
    let list: Value = serde_json::from_str(&out).map_err(e)?;
    let pr = list.get(0).cloned().unwrap_or(Value::Null);
    if pr["state"] == "MERGED" {
        st.db().set_merged(&id).map_err(e)?;
    }
    if ws.title.is_empty() && let Some(t) = pr["title"].as_str() {
        st.db().set_title(&id, t).map_err(e)?;
    }
    Ok(pr)
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
    let me = st.branch_prefix.clone();
    v.sort_by_key(|i| !i["assignees"].as_array().is_some_and(|a| a.iter().any(|x| x["login"] == me.as_str())));
    Ok(Value::Array(v))
}

/// Start a new chat in a workspace with a chosen agent and mode, send it a prompt, and show it.
#[tauri::command]
async fn start_chat(app: AppHandle, st: State<'_, AppState>, id: String, agent: String, mode: String, title: String, text: String) -> Res<String> {
    let s = Session {
        id: new_id(), workspace_id: id.clone(), agent: if agent == "codex" { "codex".into() } else { "claude".into() },
        agent_session_id: None, title, model: None, permission_mode: mode, effort: None,
    };
    st.db().insert_session(&s).map_err(e)?;
    let _ = app.emit("focus-session", json!({"workspace_id": id, "session_id": s.id}));
    send(app, st, s.id.clone(), text).await?;
    Ok(s.id)
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
    let bin = if agent == "codex" { "codex" } else { "claude" };
    let out = Command::new(bin).args(["mcp", "list"]).current_dir(&ws.path).stdin(Stdio::null()).output().map_err(|err| format!("Could not run {bin}: {err}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Ok(text.trim().to_string())
}

/// Custom slash commands: `.claude/commands/*.md` in the workspace and in ~/.claude, with their descriptions.
#[tauri::command]
async fn slash_commands(st: State<'_, AppState>, id: String) -> Res<Vec<(String, String)>> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let mut out = vec![];
    let dirs = [Some(Path::new(&ws.path).join(".claude/commands")), dirs::home_dir().map(|h| h.join(".claude/commands"))];
    for dir in dirs.into_iter().flatten() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("md") { continue; }
            let name = p.file_stem().unwrap().to_string_lossy().to_string();
            let body = std::fs::read_to_string(&p).unwrap_or_default();
            let desc = body.lines().find_map(|l| l.strip_prefix("description:")).map(|d| d.trim().trim_matches('"').to_string())
                .or_else(|| body.lines().find(|l| !l.trim().is_empty() && !l.starts_with("---")).map(|l| l.trim_start_matches('#').trim().chars().take(80).collect()))
                .unwrap_or_default();
            if !out.iter().any(|(n, _): &(String, String)| *n == name) {
                out.push((name, desc));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Commit everything and push. An empty message uses the chat's first task as the subject.
#[tauri::command]
async fn commit_push(st: State<'_, AppState>, id: String, message: String) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
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
    Ok(msg)
}

/// Merge the target branch from origin into the workspace. Returns "up-to-date", "merged" or
/// "conflicts" (left in place, for the agent or the user to resolve).
#[tauri::command]
async fn pull_target(st: State<'_, AppState>, id: String) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let dir = Path::new(&ws.path);
    git::merge_target(dir, &ws.target_branch).map(str::to_string).map_err(e)
}

/// Merge the workspace's PR on GitHub. `method` is squash, merge or rebase.
#[tauri::command]
async fn merge_pr(st: State<'_, AppState>, id: String, auto: Option<bool>) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let method = st.db().repo(&ws.repo_id).map(|r| r.prefs.merge_method).unwrap_or_default();
    let flag = match method.as_str() { "merge" => "--merge", "rebase" => "--rebase", _ => "--squash" };
    if auto.unwrap_or(false) {
        // GitHub merges it once the required checks pass.
        gh(&ws.path, &["pr", "merge", &ws.branch, flag, "--auto"])?;
        return Ok(());
    }
    gh(&ws.path, &["pr", "merge", &ws.branch, flag])?;
    st.db().set_merged(&id).map_err(e)?;
    Ok(())
}

/// The last lines of each failing check's log, for "Fix errors".
#[tauri::command]
async fn failing_logs(st: State<'_, AppState>, id: String) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let out = gh(&ws.path, &["pr", "view", &ws.branch, "--json", "statusCheckRollup"])?;
    let v: Value = serde_json::from_str(&out).map_err(e)?;
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
    git::git(Path::new(&ws.path), &["push", "-u", "origin", &ws.branch]).map_err(e)?;
    let mut args = vec!["pr", "create", "--head", &ws.branch, "--base", &ws.target_branch, "--title", &title, "--body", &body];
    if draft {
        args.push("--draft");
    }
    Ok(gh(&ws.path, &args)?.trim().to_string())
}

// ---------- github ----------

/// The signed-in GitHub user (`gh auth`), or null when gh is not logged in.
#[tauri::command]
async fn github_profile(st: State<'_, AppState>) -> Res<Value> {
    Ok(st.github.lock().unwrap().get_or_insert_with(|| {
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
    }).clone())
}

/// GitHub facts for a repo: owner avatar, description, stars, default branch.
#[tauri::command]
async fn repo_github(st: State<'_, AppState>, id: String) -> Res<Value> {
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
    let agent = if agent == "codex" { "codex" } else { "claude" };
    let pref = |k: &str| st.db().setting(&format!("{agent}_{k}")).ok().flatten().filter(|v| !v.is_empty());
    let s = Session {
        id: new_id(),
        workspace_id,
        agent: agent.into(),
        agent_session_id: None,
        title: "New chat".into(),
        model: pref("model"),
        permission_mode: pref("mode").unwrap_or_else(|| "acceptEdits".into()),
        effort: pref("effort"),
    };
    st.db().insert_session(&s).map_err(e)?;
    Ok(s)
}

#[tauri::command]
async fn update_session(st: State<'_, AppState>, id: String, title: String, model: Option<String>, permission_mode: String) -> Res<()> {
    st.db().update_session_settings(&id, &title, model.as_deref().filter(|m| !m.is_empty()), &permission_mode).map_err(e)
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
        agent_session_id: if src.agent == "claude" { src.agent_session_id.clone() } else { None },
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
    start_turn(app, &st, id, text)
}

#[tauri::command]
async fn stop(st: State<'_, AppState>, id: String) -> Res<()> {
    let runs = st.runs.lock().unwrap();
    if let Some(pid) = runs.running.get(&id) {
        let _ = Command::new("kill").args(["-INT", &pid.to_string()]).status();
    }
    Ok(())
}

/// Undo every file change made since `checkpoint` (taken before one of this chat's messages).
/// The agent is told on its next turn, so it does not assume its old edits still exist.
#[tauri::command]
async fn restore_checkpoint(app: AppHandle, st: State<'_, AppState>, id: String, checkpoint: String) -> Res<()> {
    let ws = {
        let db = st.db();
        let s = db.session(&id).map_err(e)?;
        db.workspace(&s.workspace_id).map_err(e)?
    };
    {
        let runs = st.runs.lock().unwrap();
        let busy = st.db().sessions(&ws.id).map_err(e)?.iter().any(|s| runs.running.contains_key(&s.id));
        if busy {
            return Err("An agent is still working in this workspace. Stop it first.".into());
        }
    }
    // Keep a checkpoint of the current state too, so a rewind can itself be undone from the terminal.
    let _ = git::checkpoint(Path::new(&ws.path), &format!("refs/nekotrail/{}", ws.id));
    git::restore_checkpoint(Path::new(&ws.path), &checkpoint).map_err(e)?;
    let ev = json!({"kind": "note", "text": "Files rewound to before this point. Later changes were undone."});
    st.db().append_event(&id, &ev).map_err(e)?;
    emit_event(&app, &id, &ev);
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
    if let Ok(common) = git::git(root, &["rev-parse", "--path-format=absolute", "--git-common-dir"]) {
        let exclude = Path::new(&common).join("info").join("exclude");
        let cur = std::fs::read_to_string(&exclude).unwrap_or_default();
        if !cur.lines().any(|l| l.trim() == ".nekotrail/") {
            let _ = std::fs::create_dir_all(exclude.parent().unwrap());
            let _ = std::fs::write(&exclude, format!("{cur}{}.nekotrail/\n", if cur.is_empty() || cur.ends_with('\n') { "" } else { "\n" }));
        }
    }
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '-' }).collect();
    let rel = format!(".nekotrail/attachments/{}-{}", chrono::Local::now().format("%H%M%S"), safe);
    let full = root.join(&rel);
    std::fs::create_dir_all(full.parent().unwrap()).map_err(e)?;
    std::fs::write(&full, bytes).map_err(e)?;
    Ok(rel)
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
    } else if t.contains("/login") || t.contains("not logged in") || t.contains("invalid api key") || t.contains("authentication") || t.contains("unauthorized") || t.contains("401") {
        Some("login")
    } else if t.contains("usage limit") || t.contains("rate limit") || t.contains("limit reached") || t.contains("quota") {
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

/// A short workspace title from the first message: its first sentence, cut at a word near 48 chars.
fn task_title(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let sentence = line.split_inclusive(['.', '?', '!']).next().unwrap_or(line).trim_end_matches(['.', '?', '!']).trim();
    if sentence.chars().count() <= 48 {
        return sentence.to_string();
    }
    let cut: String = sentence.chars().take(48).collect();
    match cut.rfind(' ') {
        Some(i) if i > 20 => format!("{}…", &cut[..i]),
        _ => format!("{cut}…"),
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

/// First-run check: git, GitHub CLI login, and the agent CLIs.
#[tauri::command]
async fn doctor() -> Res<Value> {
    let (git_ok, git_v) = probe("git", &["--version"]);
    let (gh_installed, _) = probe("gh", &["--version"]);
    let (gh_ok, _) = probe("gh", &["auth", "status"]);
    let login = if gh_ok { github_login() } else { None };
    let (claude_ok, claude_v) = probe("claude", &["--version"]);
    let (codex_ok, codex_v) = probe("codex", &["--version"]);
    Ok(json!({
        "git": {"ok": git_ok, "version": git_v},
        "gh": {"installed": gh_installed, "ok": gh_ok, "login": login},
        "claude": {"ok": claude_ok, "version": claude_v},
        "codex": {"ok": codex_ok, "version": codex_v},
    }))
}

fn maybe_name_branch(app: &AppHandle, st: &AppState, ws: Workspace, text: &str) -> Workspace {
    let placeholder = format!("{}/{}", st.branch_prefix, ws.name);
    if ws.branch != placeholder || ws.imported_from.is_some() || st.db().setting("auto_branch").ok().flatten().as_deref() == Some("off") {
        return ws;
    }
    let slug = branch_slug(text);
    if slug.len() < 3 {
        return ws;
    }
    let dir = Path::new(&ws.path);
    let mut name = format!("{}/{slug}", st.branch_prefix);
    let mut n = 2;
    while git::branch_exists(dir, &name) {
        name = format!("{}/{slug}-{n}", st.branch_prefix);
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
    let env = workspace_env(&repo, &ws);
    let prefs = &repo.prefs;
    let dirs = st.db().extra_dirs(&id).unwrap_or_default();
    let ws = maybe_name_branch(&app, st, ws, &text);
    // Name the workspace after its first task, so 20 workspaces stay readable.
    if ws.title.is_empty() && ws.imported_from.is_none() {
        let t = task_title(&text);
        if !t.is_empty() {
            let _ = st.db().set_title(&ws.id, &t);
            let _ = app.emit("workspaces-changed", json!({}));
        }
    }
    // Snapshot the worktree first, so this turn can be undone with one click.
    let checkpoint = git::checkpoint(Path::new(&ws.path), &format!("refs/nekotrail/{}", ws.id)).ok();
    let user = json!({"kind": "user", "text": text, "checkpoint": checkpoint});
    st.db().append_event(&id, &user).map_err(e)?;
    emit_event(&app, &id, &user);
    let prompt = match st.runs.lock().unwrap().notes.remove(&id) {
        Some(note) => format!("{note}\n\n{text}"),
        None => text.clone(),
    };

    let fork = st.runs.lock().unwrap().forks.remove(&id);
    let mut cmd = agent::command(&agent::TurnSpec {
        fork,
        agent: &session.agent,
        prompt: &prompt,
        agent_session_id: session.agent_session_id.as_deref(),
        model: session.model.as_deref(),
        permission_mode: &session.permission_mode,
        cwd: Path::new(&ws.path),
        effort: session.effort.as_deref(),
        instructions: Some(prefs.general_prompt.trim()).filter(|p| !p.is_empty()),
        env: &env,
        dirs: &dirs,
    });
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(err) => {
            let ev = json!({"kind": "error", "text": format!("Could not start {}: {err}", session.agent), "fix": "install", "agent": session.agent});
            st.db().append_event(&id, &ev).map_err(e)?;
            emit_event(&app, &id, &ev);
            return Ok(());
        }
    };
    st.runs.lock().unwrap().running.insert(id.clone(), child.id());
    // Keep the Mac awake while the agent works (ends by itself when the agent exits).
    if st.db().setting("caffeinate").ok().flatten().as_deref() != Some("off") {
        let _ = Command::new("caffeinate").args(["-i", "-w", &child.id().to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
    let _ = app.emit("running", json!({"session_id": id, "running": true}));

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
    std::thread::spawn(move || {
        let st = app.state::<AppState>();
        let mut saw_done = false;
        let mut ok = true;
        let mut context_tokens: Option<u64> = None;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            for mut ev in agent::parse(&agent_name, &line) {
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
                    ok = !ev["is_error"].as_bool().unwrap_or(false);
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
                }
                let _ = st.db().append_event(&id, &ev);
                emit_event(&app, &id, &ev);
            }
        }
        let status = child.wait();
        if !saw_done {
            let stderr = err_buf.lock().unwrap().trim().to_string();
            let stopped = status.as_ref().map(|s| !s.success()).unwrap_or(true) && stderr.is_empty();
            let ev = if stopped {
                json!({"kind": "done", "is_error": true, "stopped": true})
            } else {
                json!({"kind": "error", "fix": classify_error(&stderr), "agent": agent_name,
                       "text": if stderr.is_empty() { "Agent exited without a result".to_string() } else { stderr }})
            };
            ok = false;
            let _ = st.db().append_event(&id, &ev);
            emit_event(&app, &id, &ev);
        }
        let _ = st.db().set_unread(&workspace_id, true);
        let next = {
            let mut runs = st.runs.lock().unwrap();
            runs.running.remove(&id);
            let next = runs.queued.get_mut(&id).and_then(|q| q.pop_front());
            let rest: Vec<String> = runs.queued.get(&id).map(|q| q.iter().cloned().collect()).unwrap_or_default();
            let _ = app.emit("queue", json!({"session_id": id, "queued": rest}));
            next
        };
        let title = st.db().session(&id).map(|s| s.title).unwrap_or_default();
        let _ = app.emit("running", json!({
            "session_id": id, "running": false, "workspace_id": workspace_id,
            "ok": ok, "title": title, "more_queued": next.is_some(),
        }));
        if let Some(text) = next {
            let _ = start_turn(app.clone(), &st, id, text);
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

/// Run a command (run or setup script) in its own terminal, replacing any earlier run.
/// The terminal ends when the command does; `pty-exit` reports it.
#[tauri::command]
async fn pty_run(app: AppHandle, st: State<'_, AppState>, id: String, cwd: String, cmd: String, cols: u16, rows: u16) -> Res<()> {
    start_pty(&app, &st, &id, &cwd, Some(&cmd), cols, rows)
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
        .and_then(|w| st.db().repo(&w.repo_id).ok().map(|r| workspace_env(&r, &w)))
        .unwrap_or_default();
    let (a1, a2, id1, id2) = (app.clone(), app.clone(), id.to_string(), id.to_string());
    st.ptys
        .open(
            id,
            cwd,
            &env,
            cmd,
            cols,
            rows,
            move |data| { let _ = a1.emit("pty", json!({"id": id1, "data": data})); },
            move |generation| {
                let st = a2.state::<AppState>();
                st.ptys.close_if(&id2, generation);
                let _ = a2.emit("pty-exit", json!({"id": id2}));
            },
        )
        .map(|_| ())
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

// ---------- import ----------

#[tauri::command]
fn conductor_available() -> bool {
    importer::default_db_path().is_some()
}

/// What an import would bring over, without changing anything: a dry run into a scratch database
/// that already knows what you have, so duplicates count as skipped.
#[tauri::command]
async fn import_preview(st: State<'_, AppState>) -> Res<importer::Report> {
    let db = importer::default_db_path().ok_or("No Conductor data found on this Mac")?;
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
    let db = importer::default_db_path().ok_or("No Conductor data found on this Mac")?;
    importer::import(&st.db(), &db).map_err(e)
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
    let branch_prefix = github_login().unwrap_or_else(|| "purr".into());
    // Chats imported before history import existed get their messages now (once; skips filled chats).
    if let Some(db) = importer::default_db_path()
        && let Err(err) = importer::backfill_history(&store, &db) {
            eprintln!("could not copy Conductor chat history: {err}");
        }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState { store: Mutex::new(store), runs: Default::default(), ptys: Default::default(), home, branch_prefix, github: Default::default(), repo_meta: Default::default() })
        .invoke_handler(tauri::generate_handler![
            snapshot, add_repo, save_repo_scripts, repo_branches, workspace_scripts,
            create_workspace, archive_workspace, restore_workspace, set_pinned, set_unread, set_notes, set_target_branch,
            changes, file_diff, open_in, pr_status, create_pr,
            sessions, new_session, update_session, session_events, queued, clear_queue, send, stop,
            restore_checkpoint, workspace_files, save_attachment,
            set_workspace_title, set_workspace_status, rename_branch, set_repo_prefs, set_session_effort, hide_session,
            app_settings, set_app_setting,
            pty_run, pty_attach, open_in_targets, line_counts, pr_states,
            remove_queued, fork_session, add_dir, doctor, github_repos, clone_repo, import_preview, repo_issues, start_chat, set_pair, workspace_stats, usage_summary, model_catalog, mcp_list, slash_commands, read_file, write_file, pr_review_comments,
            repo_prs, commit_push, pull_target, merge_pr, failing_logs, send_to_workspace,
            pty_open, pty_write, pty_resize, pty_close, conductor_available, import_conductor,
            github_profile, repo_github, sky
        ])
        .run(tauri::generate_context!())
        .expect("error while running nekotrail");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_env_has_stable_ports_and_repo_vars() {
        let mut repo = Repo {
            id: "r".into(), name: "demo".into(), root_path: "/code/demo".into(), default_branch: "main".into(),
            remote_url: None, setup_script: None, run_script: None, archive_script: None, prefs: Default::default(),
        };
        repo.prefs.env = vec![("API_URL".into(), "http://localhost".into())];
        let ws = Workspace {
            id: "abc".into(), repo_id: "r".into(), name: "mochi".into(), branch: "b".into(), path: "/w/mochi".into(),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: String::new(), title: String::new(), status: String::new(), pair_id: String::new(),
        };
        let env = workspace_env(&repo, &ws);
        let get = |k: &str| env.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap();
        let port: u32 = get("NEKOTRAIL_PORT").parse().unwrap();
        assert!((41000..59000).contains(&port) && port.is_multiple_of(10));
        assert_eq!(get("NEKOTRAIL_PORT"), workspace_env(&repo, &ws).iter().find(|(k, _)| k == "NEKOTRAIL_PORT").unwrap().1);
        assert_eq!(get("API_URL"), "http://localhost");
        assert_eq!(get("NEKOTRAIL_ROOT_PATH"), "/code/demo");
    }

    #[test]
    fn titles_and_error_fixes() {
        assert_eq!(task_title("Fix the 502 after deploy. Then add a test."), "Fix the 502 after deploy");
        assert_eq!(task_title("\n  Add dark mode?\nmore"), "Add dark mode");
        let long = task_title("The load balancer returns 502 on /v1/tickets right after a deploy and nobody knows why");
        assert!(long.ends_with('…') && long.chars().count() <= 49, "{long}");
        assert_eq!(classify_error("Could not start claude: No such file or directory (os error 2)"), Some("install"));
        assert_eq!(classify_error("Invalid API key · Please run /login"), Some("login"));
        assert_eq!(classify_error("Claude AI usage limit reached|1762400000"), Some("limit"));
        assert_eq!(classify_error("tests failed"), None);
        assert_eq!(classify_cli_notice("The endpoint returns 401 when the token expires, so I added a refresh."), None);
        assert_eq!(classify_cli_notice("Invalid API key · Please run /login"), Some("login"));
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
    fn branch_names_from_first_message() {
        assert_eq!(branch_slug("Fix the 502 on /v1/tickets after a deploy"), "fix-502-v1-tickets-after");
        assert_eq!(branch_slug("Can you please add dark mode?\nmore text"), "add-dark-mode");
        assert_eq!(branch_slug("!!!"), "");
    }
}
