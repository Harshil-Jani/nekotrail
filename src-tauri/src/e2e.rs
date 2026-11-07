//! The end-to-end test door. Debug builds only, and only when the app was started with
//! `NEKOTRAIL_E2E=1` (scripts/e2e.sh does that, against a throwaway home folder). The app then
//! makes no window, and the `nekotrail` socket also answers `{"e2e": "<op>", ...}` lines by
//! calling the same `#[tauri::command]` functions the window calls, so a test drives real user
//! journeys through real code: workspaces, chats, turns with an agent CLI, terminals, the queue.
//!
//! Every event the app emits to the window is recorded too, so a test can check what the window
//! would have seen (`seen`). Release builds don't contain this module.

use crate::{AppState, Res};
use serde_json::{json, Value};
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Listener, Manager};

/// Events the window listens to that the journeys check.
const RECORDED: [&str; 9] = ["running", "queue", "agent", "pty", "pty-exit", "workspaces-changed", "needs", "task-queue", "window-ready"];

/// Every recorded event, oldest first: `{"n": index, "event": name, "payload": ...}`.
static SEEN: LazyLock<Mutex<Vec<Value>>> = LazyLock::new(Default::default);

/// Record the window's events from now on.
pub fn start(app: &AppHandle) {
    for name in RECORDED {
        app.listen_any(name, move |ev| {
            let payload: Value = serde_json::from_str(ev.payload()).unwrap_or(Value::Null);
            let mut seen = SEEN.lock().unwrap_or_else(|p| p.into_inner());
            let n = seen.len();
            seen.push(json!({"n": n, "event": name, "payload": payload}));
        });
    }
}

fn str_arg<'a>(req: &'a Value, key: &str) -> Res<&'a str> {
    req[key].as_str().ok_or_else(|| format!("e2e: `{key}` is missing"))
}

fn to_value<T: serde::Serialize>(v: Res<T>) -> Res<Value> {
    v.and_then(|v| serde_json::to_value(v).map_err(crate::e))
}

/// Answer one `{"e2e": op, ...}` line.
pub fn handle(app: &AppHandle, line: &str) -> Res<Value> {
    use tauri::async_runtime::block_on;
    let req: Value = serde_json::from_str(line).map_err(|e| format!("e2e: not JSON: {e}"))?;
    let op = str_arg(&req, "e2e")?;
    let st = || app.state::<AppState>();
    let s = |k: &str| str_arg(&req, k).map(str::to_string);
    match op {
        "ping" => Ok(json!({"pid": std::process::id(), "home": st().home})),
        "add_repo" => to_value(block_on(crate::add_repo(st(), s("path")?))),
        "create_workspace" => to_value(block_on(crate::create_workspace(app.clone(), st(), s("repo_id")?, None, None, None))),
        "workspace" => to_value(st().db().workspace(&s("id")?).map_err(crate::e)),
        "new_session" => to_value(block_on(crate::new_session(st(), s("workspace_id")?, s("agent")?))),
        "send" => to_value(block_on(crate::send(app.clone(), st(), s("session_id")?, s("text")?))),
        "stop" => to_value(block_on(crate::stop(st(), s("session_id")?))),
        "queued" => to_value(block_on(crate::queued(st(), s("session_id")?))),
        "events_tail" => block_on(crate::session_events_tail(st(), s("session_id")?, req["limit"].as_i64().unwrap_or(50))),
        "events" => to_value(block_on(crate::session_events(st(), s("session_id")?))),
        "snapshot" => to_value(block_on(crate::snapshot(st(), false))),
        "queue_view" => block_on(crate::queuerun::queue_view(app.clone())),
        "pty_open" => to_value(block_on(crate::pty_open(app.clone(), st(), s("id")?, s("cwd")?, 80, 24))),
        "pty_write" => to_value(block_on(crate::pty_write(st(), s("id")?, s("data")?))),
        "pty_close" => to_value(block_on(crate::pty_close(st(), s("id")?))),
        "changes" => to_value(block_on(crate::changes(app.clone(), s("id")?, req["uncommitted"].as_bool()))),
        "file_diff" => to_value(block_on(crate::file_diff(app.clone(), s("id")?, s("file")?, req["uncommitted"].as_bool()))),
        "inbox" => to_value(Ok(crate::remote_take(st()))),
        "archive" => to_value(block_on(crate::archive_workspace(st(), s("id")?, req["force"].as_bool().unwrap_or(false)))),
        "seen" => {
            let since = req["since"].as_u64().unwrap_or(0) as usize;
            let seen = SEEN.lock().unwrap_or_else(|p| p.into_inner());
            Ok(Value::Array(seen.iter().skip(since).cloned().collect()))
        }
        // Long soaks: forget what was recorded so far (indices stay valid), so the recorder's own
        // growth doesn't read as the app's memory growing.
        "seen_clear" => {
            let mut seen = SEEN.lock().unwrap_or_else(|p| p.into_inner());
            let n = seen.len();
            for (i, e) in seen.iter_mut().enumerate() {
                *e = json!({"n": i, "event": "cleared"});
            }
            Ok(json!({"cleared": n}))
        }
        other => Err(format!("e2e: no op called {other}")),
    }
}
