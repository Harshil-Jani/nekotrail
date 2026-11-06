//! Runs the task queue (`queue.rs` holds its rules): makes each task's workspace and chat, runs
//! setup, starts the agent's turn, hears how it ended, runs the "then" steps (checks, a draft PR,
//! autopilot), and starts the next task. A tick every 15 seconds starts whatever the rules allow,
//! so a limit's reset time is noticed without anything else happening.

use crate::budget::{self, Budgets, Scope, Spent, Stop, Today};
use crate::queue::{self, After, Halt, NewTask, Queue, State, TurnEnd};
use crate::runaway::{self, Snap};
use crate::store::{new_id, Session, Workspace};
use crate::{e, AppState, Res};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// The longest a setup script or a check run is waited for before the task goes on anyway.
const SETUP_MAX: Duration = Duration::from_secs(30 * 60);
const CHECKS_MAX: Duration = Duration::from_secs(60 * 60);
const TICK: Duration = Duration::from_secs(15);

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Change the queue and save it. The settings write happens under the queue's lock, so two
/// changes can't land on disk in the wrong order. Never called with the database locked.
pub fn update_in<R>(st: &AppState, f: impl FnOnce(&mut Queue) -> R) -> R {
    let mut q = st.queue.lock().unwrap();
    let r = f(&mut q);
    let _ = st.db().set_setting(queue::SETTING, &q.to_json());
    r
}

/// `update_in`, then tell the window.
pub fn update<R>(app: &AppHandle, f: impl FnOnce(&mut Queue) -> R) -> R {
    let r = update_in(&app.state::<AppState>(), f);
    let _ = app.emit("task-queue", ());
    r
}

/// Queue tasks whose agent waits for your OK on something right now.
fn asking(st: &AppState) -> HashSet<String> {
    let sessions: HashSet<String> = st.runs.lock().unwrap().approvals.values().map(|(s, _, _)| s.clone()).collect();
    st.queue.lock().unwrap().tasks.iter()
        .filter(|t| t.session_id.as_ref().is_some_and(|s| sessions.contains(s)))
        .map(|t| t.id.clone())
        .collect()
}

// ---------- spend caps ----------

/// The caps and what was spent, read from the store once per look.
pub struct Caps {
    pub budgets: Budgets,
    pub today: Today,
    /// Workspace id -> what its agents spent there (the per-task caps).
    tasks: HashMap<String, Spent>,
    names: HashMap<String, String>,
}

impl Caps {
    /// `workspaces`: the ones whose spend the per-task caps need.
    pub fn load(st: &AppState, workspaces: &[String]) -> Caps {
        let db = st.db();
        let budgets = Budgets::from_json(db.setting(budget::SETTING).ok().flatten().as_deref());
        let on = budgets.any();
        let today = if on { db.spend_since(&crate::usage::local_midnight(chrono::Local::now().fixed_offset())).unwrap_or_default() } else { Today::default() };
        let tasks = if on { workspaces.iter().filter_map(|w| db.workspace_spend(w).ok().map(|s| (w.clone(), s))).collect() } else { HashMap::new() };
        let names = db.repos().map(|rs| rs.into_iter().map(|r| (r.id, r.name)).collect()).unwrap_or_default();
        Caps { budgets, today, tasks, names }
    }

    /// The cap that holds a queued task back. Only a task that continues in its workspace has
    /// spent anything there.
    pub fn stop(&self, t: &queue::Task) -> Option<Stop> {
        let task = t.workspace_id.as_ref().filter(|_| t.resume).map(|w| self.tasks.get(w).cloned().unwrap_or_default());
        self.budgets.check(&self.today, &t.repo_id, task.as_ref())
    }

    /// The cap that stops another unattended turn in this workspace (autopilot).
    pub fn stop_workspace(&self, ws: &Workspace) -> Option<Stop> {
        let task = self.tasks.get(&ws.id).cloned().unwrap_or_default();
        self.budgets.check(&self.today, &ws.repo_id, Some(&task))
    }

    pub fn text(&self, s: &Stop) -> String {
        s.text(s.repo_id.as_ref().and_then(|r| self.names.get(r)).map(String::as_str))
    }
}

/// Queued tasks that would continue in their workspace (their spend counts toward a task cap).
fn resuming(st: &AppState) -> Vec<String> {
    st.queue.lock().unwrap().tasks.iter()
        .filter(|t| t.state == State::Queued && t.resume)
        .filter_map(|t| t.workspace_id.clone())
        .collect()
}

/// Caps already announced, so each one is said once (per day for daily caps).
static SAID: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Say a cap once, when it first holds something back: a toast and a notification.
fn announce(app: &AppHandle, key: String, text: &str) {
    if SAID.lock().unwrap().insert(key) {
        let _ = app.emit("budget", json!({"text": text}));
        crate::alerts::fire(app, crate::alerts::Event::CapReached, None, None, text);
    }
}

fn day_key(s: &Stop) -> String {
    format!("{}|{}", chrono::Local::now().format("%Y-%m-%d"), s.repo_id.as_deref().unwrap_or("*"))
}

/// Start what the rules allow now.
pub fn tick(app: &AppHandle) {
    let st = app.state::<AppState>();
    let asking = asking(&st);
    let caps = Caps::load(&st, &resuming(&st));
    let gate = |t: &queue::Task| caps.stop(t).map(|s| caps.text(&s));
    let starts = update_in(&st, |q| q.claim_with(&asking, now(), &gate));
    // Away alerts: "the queue is done" once the last task ends.
    let busy = st.queue.lock().unwrap().has_work() || !starts.is_empty();
    crate::alerts::queue_tick(app, busy, |since| st.queue.lock().unwrap().summary_since(since).text());
    if caps.budgets.any() {
        let held: Vec<(String, Stop)> = st.queue.lock().unwrap().tasks.iter()
            .filter(|t| t.state == State::Queued)
            .filter_map(|t| caps.stop(t).map(|s| (t.id.clone(), s)))
            .collect();
        for (id, s) in held {
            let key = if s.scope == Scope::Day { day_key(&s) } else { format!("task|{id}") };
            announce(app, key, &caps.text(&s));
        }
    }
    if starts.is_empty() {
        return;
    }
    let _ = app.emit("task-queue", ());
    for (id, agent) in starts {
        let app = app.clone();
        std::thread::spawn(move || launch(&app, &id, &agent));
    }
}

/// Look again soon, off the caller's thread.
pub fn poke(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || tick(&app));
}

/// The tick for the life of the app.
pub fn start_loop(app: AppHandle) {
    std::thread::spawn(move || loop {
        tick(&app);
        std::thread::sleep(TICK);
    });
}

fn launch(app: &AppHandle, id: &str, agent: &str) {
    if let Err(err) = start_task(app, id, agent) {
        update(app, |q| q.finish(id, State::Failed, &err, now()));
        poke(app);
    }
}

/// A chat for the task. Its own model and effort apply to its own agent; a fallback agent uses
/// your defaults for that agent.
fn new_session(st: &AppState, ws_id: &str, agent: &str, task: &queue::Task) -> Res<String> {
    let pref = |k: &str| st.db().setting(&format!("{agent}_{k}")).ok().flatten().filter(|v| !v.is_empty());
    let own = agent == task.agent;
    let s = Session {
        id: new_id(),
        workspace_id: ws_id.to_string(),
        agent: crate::agent::get(agent).id().into(),
        agent_session_id: None,
        title: "New chat".into(),
        model: if own { task.model.clone().or_else(|| pref("model")) } else { pref("model") },
        permission_mode: task.mode.clone(),
        effort: if own { task.effort.clone().or_else(|| pref("effort")) } else { pref("effort") },
    };
    st.db().insert_session(&s).map_err(e)?;
    Ok(s.id)
}

fn note(app: &AppHandle, st: &AppState, session_id: &str, text: &str) {
    let ev = json!({"kind": "note", "text": text});
    let _ = st.db().append_event(session_id, &ev);
    crate::emit_event(app, session_id, &ev);
}

/// What the chat last said, for a turn that couldn't start.
fn last_words(st: &AppState, session_id: &str) -> String {
    let (events, _) = st.db().events_tail(session_id, 3).unwrap_or_default();
    events.iter().rev().find_map(|ev| ev["text"].as_str().map(|t| t.chars().take(200).collect())).unwrap_or_default()
}

fn start_task(app: &AppHandle, id: &str, agent: &str) -> Res<()> {
    let st = app.state::<AppState>();
    let task = st.queue.lock().unwrap().get(id).cloned().ok_or("That task isn't in the queue anymore.")?;
    // Continue in its workspace when it has one still open; otherwise a new one.
    let existing = task.workspace_id.as_deref().filter(|_| task.resume)
        .and_then(|w| st.db().workspace(w).ok())
        .filter(|w| w.state == "active");
    let fresh = existing.is_none();
    let ws = match existing {
        Some(w) => w,
        None => tauri::async_runtime::block_on(crate::create_workspace(app.clone(), app.state::<AppState>(), task.repo_id.clone(), task.base.clone(), None, None))
            .map_err(|err| format!("Couldn't make its workspace. {err}"))?,
    };
    let old = if fresh { None } else { task.session_id.as_deref().and_then(|s| st.db().session(s).ok()) };
    let (sid, prompt) = match old {
        Some(s) if s.agent == agent => (s.id, queue::resume_prompt(&task)),
        prev => {
            let sid = new_session(&st, &ws.id, agent, &task)?;
            let prompt = match prev {
                Some(p) => queue::handover_prompt(&task, &p.agent),
                None => task.prompt.clone(),
            };
            (sid, prompt)
        }
    };
    update(app, |q| q.started(id, &ws.id, &sid))?;
    let _ = app.emit("workspaces-changed", json!({}));
    if fresh {
        run_setup(app, &st, id, &ws, &sid);
    }
    if crate::start_turn_spawned(app.clone(), &st, sid.clone(), prompt)? {
        return Ok(());
    }
    // The chat says why (the agent isn't installed, or the repo's agent settings wait for your OK).
    let why = last_words(&st, &sid);
    update(app, |q| q.finish(id, State::NeedsYou, format!("The agent couldn't start. {why}").trim(), now()));
    poke(app);
    Ok(())
}

/// The repo's setup script, in the workspace's Setup terminal, before the agent starts. A
/// `nekotrail.json` setup you haven't approved doesn't run: nobody is there to ask.
fn run_setup(app: &AppHandle, st: &AppState, id: &str, ws: &Workspace, session_id: &str) {
    match crate::script_to_run(st, ws, "setup") {
        Ok(Some(cmd)) => {
            update(app, |q| q.set_step(id, "setup"));
            let term = format!("{}:setup", ws.id);
            if crate::start_pty(app, st, &term, &ws.path, Some(&cmd), 120, 32).is_ok() {
                let until = Instant::now() + SETUP_MAX;
                while st.ptys.is_open(&term) && Instant::now() < until {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
            update(app, |q| q.set_step(id, ""));
        }
        Err(err) if err == "UNTRUSTED" => note(app, st, session_id,
            "Setup didn't run: this repo's nekotrail.json setup isn't approved yet. Approve and run it from the Terminal tab."),
        _ => {}
    }
}

/// A turn of any chat ended (called from `start_turn`). Usage limits count from every chat, so
/// the queue never starts a task on an agent that just said it is out.
#[allow(clippy::too_many_arguments)]
pub fn turn_ended(app: &AppHandle, st: &AppState, session_id: &str, agent: &str, ok: bool, stopped: bool, needs: Option<&str>, limit_text: Option<&str>, halt: Option<Halt>) {
    let mine = {
        let q = st.queue.lock().unwrap();
        needs == Some("limit") || q.limits.contains_key(agent) || q.tasks.iter().any(|t| t.session_id.as_deref() == Some(session_id))
    };
    if !mine {
        return;
    }
    let reset = limit_text.and_then(|t| queue::parse_reset(t, chrono::Local::now().fixed_offset()));
    let end = TurnEnd { session_id, agent, ok, stopped, needs, limit_text, reset, halt };
    if let After::Steps(id) = update(app, |q| q.turn_ended(&end, now())) {
        let app = app.clone();
        std::thread::spawn(move || run_steps(&app, &id));
    }
    poke(app);
}

/// Autopilot stopped following a workspace's PR: "ready", "gave_up", "off" or "stopped"; or
/// nekotrail stopped it ("budget", "loop") with a halt that says why.
pub fn autopilot_ended(app: &AppHandle, workspace_id: &str, how: &str, halt: Option<Halt>) {
    let st = app.state::<AppState>();
    let ours = st.queue.lock().unwrap().tasks.iter().any(|t| t.workspace_id.as_deref() == Some(workspace_id) && t.state == State::Then);
    if ours {
        update(app, |q| q.autopilot_ended_with(workspace_id, how, halt, now()));
    }
}

/// Autopilot is about to start a turn in `ws`: the cap that stops it, if one is reached, said
/// in plain words.
pub fn autopilot_cap(st: &AppState, ws: &Workspace) -> Option<String> {
    let caps = Caps::load(st, std::slice::from_ref(&ws.id));
    caps.stop_workspace(ws).map(|s| caps.text(&s))
}

// ---------- runaway agents ----------

/// Is anyone there for this turn? Not when it is a queue task's turn or autopilot's.
fn unattended(st: &AppState, session_id: &str, workspace_id: &str) -> bool {
    let task = st.queue.lock().unwrap().tasks.iter().any(|t| t.session_id.as_deref() == Some(session_id) && t.state == State::Running);
    task || st.db().setting(&crate::autopilot_key(workspace_id)).ok().flatten().is_some()
}

/// The files a turn changed: its start checkpoint against the worktree now.
fn turn_snap(dir: &Path, checkpoint: &str, ok: bool) -> Option<Snap> {
    let start = format!("{checkpoint}^{{tree}}");
    let end = crate::git::worktree_tree(dir).ok()?;
    let (files, capped) = crate::git::changed_blobs(dir, &start, &end, runaway::SNAP_FILES).ok()?;
    let files = files.into_iter().map(|(path, before, after)| runaway::FileChange { path, before, after }).collect();
    Some(Snap { ok, files, capped, label: String::new() })
}

/// "Turn ending 3:10 AM", how the evidence names a turn.
fn turn_label(at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(at)
        .map(|t| format!("Turn ending {}", t.with_timezone(&chrono::Local).format("%-I:%M %p")))
        .unwrap_or_else(|_| "A turn".into())
}

/// After a turn nobody was there for: keep what files it changed, and look for an agent going in
/// circles. Found: the chat says what was seen, autopilot stops, and the returned halt makes a
/// queue task need you. Never stops a turn; this runs after it ended.
#[allow(clippy::too_many_arguments)]
pub fn watch_turn(app: &AppHandle, st: &AppState, session_id: &str, workspace_id: &str, dir: &str, checkpoint: Option<&str>, turn_id: Option<i64>, ok: bool) -> Option<Halt> {
    if !unattended(st, session_id, workspace_id) {
        return None;
    }
    if let (Some(cp), Some(id)) = (checkpoint, turn_id)
        && let Some(snap) = turn_snap(Path::new(dir), cp, ok) {
            let _ = st.db().set_turn_snap(id, &snap);
        }
    let cfg = Budgets::from_json(st.db().setting(budget::SETTING).ok().flatten().as_deref()).runaway;
    if !cfg.on {
        return None;
    }
    let (events, _) = st.db().events_tail(session_id, 4000).unwrap_or_default();
    let snaps: Vec<Snap> = st.db().turn_snaps(workspace_id, runaway::SNAP_TURNS as i64).unwrap_or_default()
        .into_iter().map(|(mut s, at)| { s.label = turn_label(&at); s }).collect();
    let found = runaway::check(&events, &snaps, &cfg)?;
    let note = runaway::note(&found);
    let ev = json!({"kind": "stop", "reason": "loop", "text": note, "evidence": found.evidence});
    let _ = st.db().append_event(session_id, &ev);
    crate::emit_event(app, session_id, &ev);
    let halt = Halt { note, evidence: found.evidence };
    let key = crate::autopilot_key(workspace_id);
    if st.db().setting(&key).ok().flatten().is_some() {
        let _ = st.db().delete_setting(&key);
        let _ = app.emit("autopilot", json!({"workspace_id": workspace_id, "state": "loop", "detail": halt.note}));
        autopilot_ended(app, workspace_id, "loop", Some(halt.clone()));
    }
    Some(halt)
}

/// After a good turn: the checks, a draft PR, autopilot, in that order. The first that needs
/// you ends the task there.
fn run_steps(app: &AppHandle, id: &str) {
    let st = app.state::<AppState>();
    let Some(task) = st.queue.lock().unwrap().get(id).cloned() else { return };
    let finish = |state: State, note: &str| {
        update(app, |q| q.finish(id, state, note, now()));
    };
    let Some(ws) = task.workspace_id.as_deref().and_then(|w| st.db().workspace(w).ok()).filter(|w| w.state == "active") else {
        return finish(State::NeedsYou, "Its workspace was archived, so the next steps didn't run.");
    };
    let mut said: Vec<String> = vec![];
    if task.then.checks {
        update(app, |q| q.set_step(id, "checks"));
        match crate::start_checks(app, &st, &ws.id, None, task.session_id.clone()) {
            // Already running: "Run after every agent turn" started them. Wait for that run.
            Ok(()) => {}
            Err(err) if err.contains("already running") => {}
            Err(_) => said.push("No checks are set up for this repo, so none ran.".into()),
        }
        let until = Instant::now() + CHECKS_MAX;
        while st.checks.progress(&ws.id).is_some() && Instant::now() < until {
            std::thread::sleep(Duration::from_secs(2));
        }
        let failing: Vec<String> = st.checks.results(&ws.id).into_iter().filter(|r| r.status != crate::checks::Status::Passed).map(|r| r.name).collect();
        if !failing.is_empty() {
            return finish(State::NeedsYou, &format!("{} {} in Checks.", failing.join(" and "), if failing.len() == 1 { "fails" } else { "fail" }));
        }
    }
    if task.then.draft_pr || task.then.autopilot {
        update(app, |q| q.set_step(id, "pr"));
        match open_draft_pr(&st, &ws, &task) {
            Ok(Some(_)) => said.push("Opened a draft PR.".into()),
            Ok(None) => return finish(State::Done, "Nothing changed, so there's no PR."),
            Err(err) => return finish(State::NeedsYou, &format!("Couldn't open the PR. {err}")),
        }
    }
    if task.then.autopilot {
        return match crate::autopilot_on(&st, &ws.id) {
            Ok(()) => {
                update(app, |q| {
                    q.set_step(id, "autopilot");
                    if let Some(t) = q.tasks.iter_mut().find(|t| t.id == id) {
                        t.note = "Autopilot is getting the PR green.".into();
                    }
                });
                let _ = app.emit("autopilot", json!({"workspace_id": ws.id, "state": "on"}));
            }
            Err(err) => finish(State::NeedsYou, &format!("Autopilot couldn't start. {err}")),
        };
    }
    said.insert(0, "Finished.".into());
    finish(State::Done, &said.join(" "));
}

/// Commit what the agent left, push, and open a draft PR. None when the branch has nothing new.
fn open_draft_pr(st: &AppState, ws: &Workspace, task: &queue::Task) -> Res<Option<String>> {
    // The title the first message gave it, read now (`start_turn` set it).
    let ws = st.db().workspace(&ws.id).map_err(e)?;
    if let Some(pr) = crate::cached_pr(st, &ws).filter(|p| p["state"] == "OPEN") {
        return Ok(pr["url"].as_str().map(String::from));
    }
    let dir = Path::new(&ws.path);
    let title = if ws.title.trim().is_empty() { task.title() } else { ws.title.clone() };
    crate::git::git(dir, &["add", "-A"]).map_err(e)?;
    if crate::git::git(dir, &["diff", "--cached", "--quiet"]).is_err() {
        crate::git::git(dir, &["commit", "-q", "-m", &title]).map_err(e)?;
    }
    let ahead = [ws.target_branch.clone(), format!("origin/{}", ws.target_branch)].iter()
        .find_map(|base| crate::git::git(dir, &["rev-list", "--count", &format!("{base}..HEAD")]).ok())
        .and_then(|n| n.trim().parse::<u32>().ok());
    if ahead == Some(0) {
        return Ok(None);
    }
    crate::git::git(dir, &["push", "-u", "origin", &ws.branch]).map_err(e)?;
    let url = crate::gh(&ws.path, &["pr", "create", "--draft", "--head", &ws.branch, "--base", &ws.target_branch, "--title", &title, "--body", &task.prompt])?.trim().to_string();
    crate::pr_poke(st, Some(&ws.repo_id), true);
    Ok(Some(url))
}

/// Add a task (the Queue view, ⌘K or `nekotrail queue add`).
pub fn add(app: &AppHandle, task: NewTask) -> Res<queue::Task> {
    task.validate()?;
    let st = app.state::<AppState>();
    st.db().repo(&task.repo_id).map_err(|_| "That repo isn't in nekotrail. Add it first.".to_string())?;
    let t = update(app, |q| q.add(task, new_id(), now()).clone());
    poke(app);
    Ok(t)
}

/// Everything the Queue view shows.
pub fn view(st: &AppState) -> Value {
    let asking = asking(st);
    let caps = Caps::load(st, &resuming(st));
    let gate = |t: &queue::Task| caps.stop(t).map(|s| caps.text(&s));
    // The cap on every repo, for the banner. Repo and task caps show on their tasks.
    let cap = caps.budgets.day_stop(&caps.today, "").map(|s| caps.text(&s));
    let q = st.queue.lock().unwrap();
    let now = now();
    json!({
        "tasks": q.tasks, "paused": q.paused, "max_total": q.max_total, "max_per_repo": q.max_per_repo,
        "limits": q.limits.iter().filter(|(_, l)| l.until > now).map(|(a, l)| (a.clone(), json!({"until": l.until, "reported": l.reported, "text": l.text}))).collect::<serde_json::Map<String, Value>>(),
        "waits": q.waits_with(&asking, now, &gate), "asking": asking, "now": now, "cap": cap,
    })
}

// ---------- commands ----------

#[tauri::command]
pub async fn queue_view(app: AppHandle) -> Res<Value> {
    Ok(view(&app.state::<AppState>()))
}

#[tauri::command]
pub async fn queue_add(app: AppHandle, task: NewTask) -> Res<queue::Task> {
    tauri::async_runtime::spawn_blocking(move || add(&app, task)).await.map_err(e)?
}

#[tauri::command]
pub async fn queue_remove(app: AppHandle, id: String) -> Res<()> {
    update(&app, |q| q.remove(&id)).map(|_| ())
}

/// Drag and drop: `id` goes right before `before` (last when null).
#[tauri::command]
pub async fn queue_move(app: AppHandle, id: String, before: Option<String>) -> Res<()> {
    update(&app, |q| q.move_before(&id, before.as_deref()))?;
    poke(&app);
    Ok(())
}

#[tauri::command]
pub async fn queue_retry(app: AppHandle, id: String) -> Res<()> {
    update(&app, |q| q.retry(&id))?;
    poke(&app);
    Ok(())
}

/// Start a task now, yourself: one that waits for you (Ask before changes) or one in line.
#[tauri::command]
pub async fn queue_start_now(app: AppHandle, id: String) -> Res<()> {
    let agent = update(&app, |q| q.start_now(&id, now()))?;
    std::thread::spawn(move || launch(&app, &id, &agent));
    Ok(())
}

#[tauri::command]
pub async fn queue_set_paused(app: AppHandle, paused: bool) -> Res<()> {
    update(&app, |q| q.paused = paused);
    poke(&app);
    Ok(())
}

#[tauri::command]
pub async fn queue_set_limits(app: AppHandle, max_total: u32, max_per_repo: u32) -> Res<()> {
    update(&app, |q| {
        q.max_total = max_total.clamp(1, queue::MAX_LIMIT);
        q.max_per_repo = max_per_repo.clamp(1, queue::MAX_LIMIT);
    });
    poke(&app);
    Ok(())
}

#[tauri::command]
pub async fn queue_clear_finished(app: AppHandle) -> Res<usize> {
    Ok(update(&app, |q| q.clear_finished()))
}

/// What ended since you last saw the summary: {done, needs_you, failed, running, queued, text}.
#[tauri::command]
pub async fn queue_summary(app: AppHandle) -> Res<Value> {
    let s = app.state::<AppState>().queue.lock().unwrap().summary();
    let mut v = serde_json::to_value(&s).map_err(e)?;
    v["text"] = json!(s.text());
    Ok(v)
}

/// You saw it: the same tasks don't show up in the next one.
#[tauri::command]
pub async fn queue_summary_seen(app: AppHandle) -> Res<()> {
    update_in(&app.state::<AppState>(), |q| q.summary_seen = now());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three turns on a real worktree: change a file, put it back, change it again. The snapshots
    /// nekotrail keeps after each turn are what the back-and-forth sign reads.
    #[test]
    fn snapshots_of_real_turns_catch_a_file_going_back_and_forth() {
        let repo = crate::git::tests::init_repo();
        let r = repo.path();
        let prefix = crate::git::checkpoint_prefix("ws-loop");
        let mut snaps = vec![];
        for (i, text) in ["two\n", "one\n", "two\n"].iter().enumerate() {
            let cp = crate::git::checkpoint(r, &prefix).unwrap();
            std::fs::write(r.join("a.txt"), text).unwrap();
            let mut s = turn_snap(r, &cp, true).unwrap();
            s.label = format!("Turn {}", i + 1);
            snaps.push(s);
        }
        let f = runaway::back_and_forth(&snaps, 2).unwrap();
        assert_eq!(f.text, "a.txt went back to an earlier version 2 times");
        assert_eq!(f.evidence[1], "Turn 2: put it back to an earlier version");
        // A turn that changed nothing has an empty snapshot.
        let cp = crate::git::checkpoint(r, &prefix).unwrap();
        assert!(turn_snap(r, &cp, true).unwrap().files.is_empty());
        crate::git::prune_checkpoints(r, &prefix, 0).unwrap();
    }
}
