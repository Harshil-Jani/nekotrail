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
}

#[tauri::command]
fn snapshot(st: State<AppState>, include_archived: bool) -> Res<Snapshot> {
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
    Ok(Snapshot { repos: db.repos().map_err(e)?, workspaces: db.workspaces(include_archived).map_err(e)?, running, running_workspaces, lit, shining })
}

// ---------- repos ----------

#[tauri::command]
fn add_repo(st: State<AppState>, path: String) -> Res<Repo> {
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
    };
    db.insert_repo(&r).map_err(e)?;
    Ok(r)
}

#[tauri::command]
fn save_repo_scripts(st: State<AppState>, id: String, setup: Option<String>, run: Option<String>, archive: Option<String>) -> Res<()> {
    st.db().update_repo_scripts(&id, setup.as_deref(), run.as_deref(), archive.as_deref()).map_err(e)
}

#[tauri::command]
fn repo_branches(st: State<AppState>, id: String) -> Res<Vec<String>> {
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
fn workspace_scripts(st: State<AppState>, id: String) -> Res<config::Scripts> {
    let ws = st.db().workspace(&id).map_err(e)?;
    scripts_for(&st, &ws)
}

// ---------- workspaces ----------

#[tauri::command]
fn create_workspace(st: State<AppState>, repo_id: String, base: Option<String>, name: Option<String>) -> Res<Workspace> {
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
    let branch = format!("{}/{name}", st.branch_prefix);
    let path = dir.join(&name);
    git::add_worktree(&root, &path, &branch, &base).map_err(e)?;
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
    };
    st.db().insert_workspace(&ws).map_err(e)?;
    Ok(ws)
}

/// Runs the archive script, removes the worktree folder and keeps the branch.
/// Refuses when there is uncommitted work unless `force` is set.
#[tauri::command]
fn archive_workspace(st: State<AppState>, id: String, force: bool) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let path = Path::new(&ws.path);
    if path.exists() {
        if !force && git::is_dirty(path).unwrap_or(false) {
            return Err("DIRTY".into());
        }
        if let Some(script) = scripts_for(&st, &ws)?.archive {
            let _ = Command::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()))
                .args(["-lc", &script])
                .current_dir(path)
                .status();
        }
        let repo = st.db().repo(&ws.repo_id).map_err(e)?;
        git::remove_worktree(Path::new(&repo.root_path), path).map_err(e)?;
    }
    st.ptys.close(&format!("{id}:term"));
    st.db().set_workspace_state(&id, "archived").map_err(e)
}

#[tauri::command]
fn restore_workspace(st: State<AppState>, id: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let repo = st.db().repo(&ws.repo_id).map_err(e)?;
    if !Path::new(&ws.path).exists() {
        git::restore_worktree(Path::new(&repo.root_path), Path::new(&ws.path), &ws.branch).map_err(e)?;
    }
    st.db().set_workspace_state(&id, "active").map_err(e)
}

#[tauri::command]
fn set_pinned(st: State<AppState>, id: String, pinned: bool) -> Res<()> {
    st.db().set_pinned(&id, pinned).map_err(e)
}
#[tauri::command]
fn set_unread(st: State<AppState>, id: String, unread: bool) -> Res<()> {
    st.db().set_unread(&id, unread).map_err(e)
}
#[tauri::command]
fn set_notes(st: State<AppState>, id: String, notes: String) -> Res<()> {
    st.db().set_notes(&id, &notes).map_err(e)
}
#[tauri::command]
fn set_target_branch(st: State<AppState>, id: String, branch: String) -> Res<()> {
    st.db().set_target_branch(&id, &branch).map_err(e)
}

#[tauri::command]
fn changes(st: State<AppState>, id: String) -> Res<Vec<git::FileChange>> {
    let ws = st.db().workspace(&id).map_err(e)?;
    git::changes(Path::new(&ws.path), &ws.target_branch).map_err(e)
}

#[tauri::command]
fn file_diff(st: State<AppState>, id: String, file: String) -> Res<String> {
    let ws = st.db().workspace(&id).map_err(e)?;
    git::file_diff(Path::new(&ws.path), &ws.target_branch, &file).map_err(e)
}

#[tauri::command]
fn open_in(st: State<AppState>, id: String, app: String) -> Res<()> {
    let ws = st.db().workspace(&id).map_err(e)?;
    let mut c = Command::new("open");
    match app.as_str() {
        "finder" => c.arg(&ws.path),
        "vscode" => c.args(["-a", "Visual Studio Code", &ws.path]),
        "cursor" => c.args(["-a", "Cursor", &ws.path]),
        "terminal" => c.args(["-a", "Terminal", &ws.path]),
        other => return Err(format!("unknown app {other}")),
    };
    c.spawn().map_err(e)?;
    Ok(())
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
fn pr_status(st: State<AppState>, id: String) -> Res<Value> {
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
    Ok(pr)
}

#[tauri::command]
fn create_pr(st: State<AppState>, id: String, title: String, body: String, draft: bool) -> Res<String> {
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
fn github_profile(st: State<AppState>) -> Value {
    st.github.lock().unwrap().get_or_insert_with(|| {
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
    }).clone()
}

/// GitHub facts for a repo: owner avatar, description, stars, default branch.
#[tauri::command]
fn repo_github(st: State<AppState>, id: String) -> Value {
    let Ok(r) = st.db().repo(&id) else { return Value::Null };
    if let Some(v) = st.repo_meta.lock().unwrap().get(&id) {
        return v.clone();
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
    v
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
fn sessions(st: State<AppState>, workspace_id: String) -> Res<Vec<Session>> {
    st.db().sessions(&workspace_id).map_err(e)
}

#[tauri::command]
fn new_session(st: State<AppState>, workspace_id: String, agent: String) -> Res<Session> {
    let s = Session {
        id: new_id(),
        workspace_id,
        agent: if agent == "codex" { "codex".into() } else { "claude".into() },
        agent_session_id: None,
        title: "New chat".into(),
        model: None,
        permission_mode: "acceptEdits".into(),
    };
    st.db().insert_session(&s).map_err(e)?;
    Ok(s)
}

#[tauri::command]
fn update_session(st: State<AppState>, id: String, title: String, model: Option<String>, permission_mode: String) -> Res<()> {
    st.db().update_session_settings(&id, &title, model.as_deref().filter(|m| !m.is_empty()), &permission_mode).map_err(e)
}

#[tauri::command]
fn session_events(st: State<AppState>, id: String) -> Res<Vec<Value>> {
    st.db().events(&id).map_err(e)
}

#[tauri::command]
fn queued(st: State<AppState>, id: String) -> Vec<String> {
    st.runs.lock().unwrap().queued.get(&id).map(|q| q.iter().cloned().collect()).unwrap_or_default()
}

#[tauri::command]
fn clear_queue(app: AppHandle, st: State<AppState>, id: String) {
    st.runs.lock().unwrap().queued.remove(&id);
    let _ = app.emit("queue", json!({"session_id": id, "queued": []}));
}

/// Send a prompt. If the agent is busy, the prompt waits in the queue and runs next.
#[tauri::command]
fn send(app: AppHandle, st: State<AppState>, id: String, text: String) -> Res<()> {
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
fn stop(st: State<AppState>, id: String) {
    let runs = st.runs.lock().unwrap();
    if let Some(pid) = runs.running.get(&id) {
        let _ = Command::new("kill").args(["-INT", &pid.to_string()]).status();
    }
}

fn emit_event(app: &AppHandle, session_id: &str, ev: &Value) {
    let _ = app.emit("agent", json!({"session_id": session_id, "event": ev}));
}

fn start_turn(app: AppHandle, st: &AppState, id: String, text: String) -> Res<()> {
    let (session, ws) = {
        let db = st.db();
        let s = db.session(&id).map_err(e)?;
        let ws = db.workspace(&s.workspace_id).map_err(e)?;
        if s.title == "New chat" {
            let t: String = text.lines().next().unwrap_or("").chars().take(48).collect();
            db.update_session_settings(&id, if t.is_empty() { "New chat" } else { &t }, s.model.as_deref(), &s.permission_mode).map_err(e)?;
        }
        (s, ws)
    };
    let user = json!({"kind": "user", "text": text});
    st.db().append_event(&id, &user).map_err(e)?;
    emit_event(&app, &id, &user);

    let mut cmd = agent::command(&agent::TurnSpec {
        agent: &session.agent,
        prompt: &text,
        agent_session_id: session.agent_session_id.as_deref(),
        model: session.model.as_deref(),
        permission_mode: &session.permission_mode,
        cwd: Path::new(&ws.path),
    });
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(err) => {
            let ev = json!({"kind": "error", "text": format!("Could not start {}: {err}", session.agent)});
            st.db().append_event(&id, &ev).map_err(e)?;
            emit_event(&app, &id, &ev);
            return Ok(());
        }
    };
    st.runs.lock().unwrap().running.insert(id.clone(), child.id());
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
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            for ev in agent::parse(&agent_name, &line) {
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
                json!({"kind": "error", "text": if stderr.is_empty() { "Agent exited without a result".to_string() } else { stderr }})
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
fn pty_open(app: AppHandle, st: State<AppState>, id: String, cwd: String, cols: u16, rows: u16) -> Res<()> {
    if st.ptys.is_open(&id) {
        return Ok(());
    }
    let (a1, a2, id1, id2) = (app.clone(), app, id.clone(), id.clone());
    st.ptys
        .open(
            &id,
            &cwd,
            cols,
            rows,
            move |data| { let _ = a1.emit("pty", json!({"id": id1, "data": data})); },
            move || {
                let st = a2.state::<AppState>();
                st.ptys.close(&id2);
                let _ = a2.emit("pty-exit", json!({"id": id2}));
            },
        )
        .map_err(e)
}

#[tauri::command]
fn pty_write(st: State<AppState>, id: String, data: String) -> Res<()> {
    st.ptys.write(&id, &data).map_err(e)
}

#[tauri::command]
fn pty_resize(st: State<AppState>, id: String, cols: u16, rows: u16) -> Res<()> {
    st.ptys.resize(&id, cols, rows).map_err(e)
}

#[tauri::command]
fn pty_close(st: State<AppState>, id: String) {
    st.ptys.close(&id);
}

// ---------- import ----------

#[tauri::command]
fn conductor_available() -> bool {
    importer::default_db_path().is_some()
}

#[tauri::command]
fn import_conductor(st: State<AppState>) -> Res<importer::Report> {
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
            pty_open, pty_write, pty_resize, pty_close, conductor_available, import_conductor,
            github_profile, repo_github, sky
        ])
        .run(tauri::generate_context!())
        .expect("error while running nekotrail");
}
