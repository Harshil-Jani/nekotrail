//! Imports your own repos, active workspaces and Claude/Codex chats from a local
//! Conductor install, so moving over loses nothing.
//!
//! It only reads the database (opened read-only) and never moves files: imported
//! workspaces keep their existing worktree folders, and chats resume through the
//! agent's own session id (`claude --resume`, `codex exec resume`).

use crate::store::{new_id, Repo, Session, Store, Workspace};
use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub fn default_db_path() -> Option<PathBuf> {
    let p = dirs::data_dir()?.join("com.conductor.app").join("conductor.db");
    p.exists().then_some(p)
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Report {
    pub repos: usize,
    pub workspaces: usize,
    pub sessions: usize,
    /// Workspaces skipped because their folder no longer exists or was already imported.
    pub skipped: usize,
    /// Chats whose message history was copied in.
    pub chats_with_history: usize,
}

struct SrcWorkspace {
    id: String,
    repo_id: String,
    name: String,
    branch: String,
    path: String,
    target: String,
    pinned: bool,
    notes: String,
}

/// Convert one Conductor chat into nekotrail events. Conductor stores what you typed as
/// plain text and the agent's side as the raw CLI stream, line by line.
pub fn history_events(src: &Connection, conductor_session: &str, agent: &str) -> Result<Vec<serde_json::Value>> {
    let mut st = src.prepare(
        "SELECT role, COALESCE(content,'') FROM session_messages
         WHERE session_id = ?1 AND cancelled_at IS NULL ORDER BY created_at, rowid",
    )?;
    let rows = st.query_map([conductor_session], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (role, content) = row?;
        if role == "user" {
            if !content.trim().is_empty() {
                out.push(serde_json::json!({"kind": "user", "text": content}));
            }
        } else {
            out.extend(crate::agent::parse(agent, &content).into_iter().filter(|e| e["kind"] != "session" && e["kind"] != "usage"));
        }
    }
    // Marked so spend totals don't count Conductor's history as if it happened today.
    for ev in out.iter_mut() {
        ev["imported"] = serde_json::json!(true);
    }
    Ok(out)
}

/// Fill in chat history for imported chats that have none yet. Matches by the agent's
/// session id, or by workspace folder and chat title when there is none. Returns chats filled.
pub fn backfill_history(store: &Store, db: &Path) -> Result<usize> {
    let src = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut filled = 0;
    for ws in store.workspaces(true)?.into_iter().filter(|w| w.imported_from.as_deref() == Some("conductor")) {
        for s in store.sessions(&ws.id)? {
            if store.has_events(&s.id)? {
                continue;
            }
            let found: Option<String> = match &s.agent_session_id {
                Some(sid) => src
                    .query_row("SELECT id FROM sessions WHERE claude_session_id = ?1 LIMIT 1", [sid], |r| r.get(0))
                    .ok(),
                None => src
                    .query_row(
                        "SELECT s.id FROM sessions s JOIN workspaces w ON w.id = s.workspace_id
                         WHERE w.workspace_path = ?1 AND COALESCE(s.title,'Imported chat') = ?2 LIMIT 1",
                        [&ws.path, &s.title],
                        |r| r.get(0),
                    )
                    .ok(),
            };
            let Some(cid) = found else { continue };
            let events = history_events(&src, &cid, &s.agent)?;
            if !events.is_empty() {
                store.append_events(&s.id, &events, &crate::store::now())?;
                filled += 1;
            }
        }
    }
    Ok(filled)
}

/// Custom prompts and copied-file globs from a Conductor repo. Missing columns (older Conductor
/// versions) just mean empty preferences.
fn conductor_prefs(src: &Connection, repo_id: &str) -> crate::store::RepoPrefs {
    let get = |col: &str| -> String {
        src.query_row(&format!("SELECT COALESCE({col}, '') FROM repos WHERE id = ?1"), [repo_id], |r| r.get(0))
            .unwrap_or_default()
    };
    let globs = get("file_include_globs");
    crate::store::RepoPrefs {
        general_prompt: get("custom_prompt_general"),
        review_prompt: get("custom_prompt_code_review"),
        pr_prompt: get("custom_prompt_create_pr"),
        copy_files: serde_json::from_str::<Vec<String>>(&globs)
            .unwrap_or_else(|_| globs.split([',', '\n']).map(|g| g.trim().to_string()).filter(|g| !g.is_empty()).collect()),
        env: vec![],
        merge_method: String::new(),
    }
}

pub fn import(store: &Store, db: &Path) -> Result<Report> {
    let src = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut report = Report::default();

    // Repos: reuse ours when the same folder is already added.
    let mut repo_map = std::collections::HashMap::new();
    let mut st = src.prepare(
        "SELECT id, name, root_path, default_branch, remote_url, setup_script, run_script, archive_script FROM repos",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            Repo {
                id: new_id(),
                name: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                root_path: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                default_branch: r.get::<_, Option<String>>(3)?.unwrap_or_else(|| "main".into()),
                remote_url: r.get(4)?,
                setup_script: r.get(5)?,
                run_script: r.get(6)?,
                archive_script: r.get(7)?,
                prefs: Default::default(),
            },
        ))
    })?;
    for row in rows {
        let (src_id, repo) = row?;
        if repo.root_path.is_empty() || !Path::new(&repo.root_path).exists() {
            continue;
        }
        let id = match store.repo_by_path(&repo.root_path)? {
            Some(existing) => existing.id,
            None => {
                let mut repo = repo.clone();
                repo.prefs = conductor_prefs(&src, &src_id);
                store.insert_repo(&repo)?;
                report.repos += 1;
                repo.id.clone()
            }
        };
        repo_map.insert(src_id, id);
    }

    // Active workspaces only; archived ones have no worktree left to open.
    let mut st = src.prepare(
        "SELECT id, repository_id, COALESCE(NULLIF(workspace_name,''), directory_name), branch, workspace_path,
                COALESCE(intended_target_branch, initialization_parent_branch), pinned_at IS NOT NULL, COALESCE(notes,'')
         FROM workspaces WHERE state = 'ready'",
    )?;
    let wss: Vec<SrcWorkspace> = st
        .query_map([], |r| {
            Ok(SrcWorkspace {
                id: r.get(0)?,
                repo_id: r.get(1)?,
                name: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                branch: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                path: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                target: r.get::<_, Option<String>>(5)?.unwrap_or_else(|| "main".into()),
                pinned: r.get(6)?,
                notes: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    for w in wss {
        let Some(repo_id) = repo_map.get(&w.repo_id) else { report.skipped += 1; continue };
        if w.path.is_empty() || !Path::new(&w.path).exists() || store.workspace_by_path(&w.path)?.is_some() {
            report.skipped += 1;
            continue;
        }
        let ws = Workspace {
            id: new_id(),
            repo_id: repo_id.clone(),
            name: w.name,
            branch: w.branch,
            path: w.path,
            target_branch: w.target,
            state: "active".into(),
            pinned: w.pinned,
            unread: false,
            notes: w.notes,
            imported_from: Some("conductor".into()),
            created_at: crate::store::now(),
            title: String::new(),
            status: String::new(),
            pair_id: String::new(),
            section_id: String::new(),
        };
        store.insert_workspace(&ws)?;
        report.workspaces += 1;

        let mut st = src.prepare(
            "SELECT COALESCE(agent_type,'claude'), claude_session_id, COALESCE(title,'Imported chat')
             FROM sessions WHERE workspace_id = ?1 AND COALESCE(is_hidden,0) = 0 ORDER BY created_at",
        )?;
        let sessions = st.query_map([&w.id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?))
        })?;
        for s in sessions {
            let (agent, sid, title) = s?;
            store.insert_session(&Session {
                id: new_id(),
                workspace_id: ws.id.clone(),
                agent: if agent == "codex" { "codex".into() } else { "claude".into() },
                agent_session_id: sid,
                title,
                model: None,
                permission_mode: "acceptEdits".into(),
                effort: None,
            })?;
            report.sessions += 1;
        }
    }
    report.chats_with_history = backfill_history(store, db)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal copy of the columns the importer reads.
    fn fixture(dir: &Path, repo_root: &Path, ws_path: &Path) -> PathBuf {
        let db = dir.join("conductor.db");
        let c = Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE repos (id TEXT, name TEXT, root_path TEXT, default_branch TEXT, remote_url TEXT,
                                 setup_script TEXT, run_script TEXT, archive_script TEXT);
             CREATE TABLE workspaces (id TEXT, repository_id TEXT, workspace_name TEXT, directory_name TEXT,
                                      branch TEXT, workspace_path TEXT, intended_target_branch TEXT,
                                      initialization_parent_branch TEXT, pinned_at TEXT, notes TEXT, state TEXT);
             CREATE TABLE sessions (id TEXT, workspace_id TEXT, agent_type TEXT, claude_session_id TEXT,
                                    title TEXT, is_hidden INTEGER, created_at TEXT);
             CREATE TABLE session_messages (session_id TEXT, role TEXT, content TEXT, created_at TEXT, cancelled_at TEXT);",
        )
        .unwrap();
        c.execute(
            "INSERT INTO repos VALUES ('r1','demo',?1,'main',NULL,'bun install',NULL,NULL)",
            [repo_root.to_str().unwrap()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO workspaces VALUES ('w1','r1','','tunis','me/fix',?1,'main','main','2026-01-01','todo','ready'),
                                           ('w2','r1','','gone','me/old','/nope','main','main',NULL,'','ready'),
                                           ('w3','r1','','arch','me/arch',?1,'main','main',NULL,'','archived')",
            [ws_path.to_str().unwrap()],
        )
        .unwrap();
        c.execute_batch(
            "INSERT INTO sessions VALUES ('s1','w1','claude','sid-1','Fix bug',0,'1'),
                                         ('s2','w1','codex','th-2','Try codex',0,'2'),
                                         ('s3','w1','claude',NULL,'hidden',1,'3');",
        )
        .unwrap();
        c.execute(
            "INSERT INTO session_messages VALUES ('s1','user','Fix the bug','1',NULL), ('s1','assistant',?1,'2',NULL),
                                                 ('s1','user','ignored, cancelled','3','4')",
            [r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Fixed."}]}}"#],
        )
        .unwrap();
        db
    }

    #[test]
    fn imports_active_workspaces_and_chats_once() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&ws).unwrap();
        let db = fixture(tmp.path(), &repo, &ws);

        let store = Store::in_memory().unwrap();
        let r = import(&store, &db).unwrap();
        assert_eq!(r, Report { repos: 1, workspaces: 1, sessions: 2, skipped: 1, chats_with_history: 1 });

        let w = &store.workspaces(false).unwrap()[0];
        assert_eq!((w.name.as_str(), w.branch.as_str(), w.pinned, w.notes.as_str()), ("tunis", "me/fix", true, "todo"));
        let s = store.sessions(&w.id).unwrap();
        assert_eq!(s[0].agent_session_id.as_deref(), Some("sid-1"));
        let ev = store.events(&s[0].id).unwrap();
        assert_eq!(ev.len(), 2, "{ev:?}");
        assert_eq!((ev[0]["kind"].as_str(), ev[1]["text"].as_str()), (Some("user"), Some("Fixed.")));
        assert_eq!(s[1].agent, "codex");
        assert_eq!(store.repos().unwrap()[0].setup_script.as_deref(), Some("bun install"));

        // Running it again adds nothing.
        let again = import(&store, &db).unwrap();
        assert_eq!(again, Report { repos: 0, workspaces: 0, sessions: 0, skipped: 2, chats_with_history: 0 });
        assert_eq!(store.events(&s[0].id).unwrap().len(), 2, "history is not copied twice");
    }
}
