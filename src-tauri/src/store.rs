//! Local SQLite store for repos, workspaces, chat sessions and their messages.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS repos (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    root_path TEXT NOT NULL UNIQUE,
    default_branch TEXT NOT NULL,
    remote_url TEXT,
    setup_script TEXT,
    run_script TEXT,
    archive_script TEXT,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL REFERENCES repos(id),
    name TEXT NOT NULL,
    branch TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    target_branch TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'active',
    pinned INTEGER NOT NULL DEFAULT 0,
    unread INTEGER NOT NULL DEFAULT 0,
    notes TEXT NOT NULL DEFAULT '',
    imported_from TEXT,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    agent TEXT NOT NULL,
    agent_session_id TEXT,
    title TEXT NOT NULL DEFAULT 'New chat',
    model TEXT,
    permission_mode TEXT NOT NULL DEFAULT 'acceptEdits',
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    event TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_by_session ON messages(session_id, id);
"#;

/// Columns added after v0.1. Each is idempotent.
fn migrate(conn: &Connection) -> Result<()> {
    let has = |col: &str| -> rusqlite::Result<bool> {
        conn.query_row("SELECT COUNT(*) FROM pragma_table_info('workspaces') WHERE name=?1", [col], |r| r.get::<_, i64>(0))
            .map(|n| n > 0)
    };
    if !has("merged")? {
        conn.execute_batch("ALTER TABLE workspaces ADD COLUMN merged INTEGER NOT NULL DEFAULT 0;")?;
    }
    Ok(())
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Repo {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub default_branch: String,
    pub remote_url: Option<String>,
    pub setup_script: Option<String>,
    pub run_script: Option<String>,
    pub archive_script: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub id: String,
    pub repo_id: String,
    pub name: String,
    pub branch: String,
    pub path: String,
    pub target_branch: String,
    pub state: String,
    pub pinned: bool,
    pub unread: bool,
    pub notes: String,
    pub imported_from: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub id: String,
    pub workspace_id: String,
    pub agent: String,
    pub agent_session_id: Option<String>,
    pub title: String,
    pub model: Option<String>,
    pub permission_mode: String,
}

pub struct Store {
    conn: Connection,
}

fn repo_from(r: &Row) -> rusqlite::Result<Repo> {
    Ok(Repo {
        id: r.get(0)?,
        name: r.get(1)?,
        root_path: r.get(2)?,
        default_branch: r.get(3)?,
        remote_url: r.get(4)?,
        setup_script: r.get(5)?,
        run_script: r.get(6)?,
        archive_script: r.get(7)?,
    })
}
const REPO_COLS: &str =
    "id, name, root_path, default_branch, remote_url, setup_script, run_script, archive_script";

fn workspace_from(r: &Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: r.get(0)?,
        repo_id: r.get(1)?,
        name: r.get(2)?,
        branch: r.get(3)?,
        path: r.get(4)?,
        target_branch: r.get(5)?,
        state: r.get(6)?,
        pinned: r.get(7)?,
        unread: r.get(8)?,
        notes: r.get(9)?,
        imported_from: r.get(10)?,
        created_at: r.get(11)?,
    })
}
const WS_COLS: &str = "id, repo_id, name, branch, path, target_branch, state, pinned, unread, notes, imported_from, created_at";

fn session_from(r: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        agent: r.get(2)?,
        agent_session_id: r.get(3)?,
        title: r.get(4)?,
        model: r.get(5)?,
        permission_mode: r.get(6)?,
    })
}
const SESSION_COLS: &str =
    "id, workspace_id, agent, agent_session_id, title, model, permission_mode";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    // ---- repos ----

    pub fn insert_repo(&self, r: &Repo) -> Result<()> {
        self.conn.execute(
            &format!("INSERT INTO repos ({REPO_COLS}, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"),
            params![r.id, r.name, r.root_path, r.default_branch, r.remote_url, r.setup_script, r.run_script, r.archive_script, now()],
        )?;
        Ok(())
    }

    pub fn update_repo_scripts(&self, id: &str, setup: Option<&str>, run: Option<&str>, archive: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET setup_script=?2, run_script=?3, archive_script=?4 WHERE id=?1",
            params![id, setup, run, archive],
        )?;
        Ok(())
    }

    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut st = self.conn.prepare(&format!("SELECT {REPO_COLS} FROM repos ORDER BY name"))?;
        let rows = st.query_map([], repo_from)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn repo(&self, id: &str) -> Result<Repo> {
        Ok(self.conn.query_row(&format!("SELECT {REPO_COLS} FROM repos WHERE id=?1"), [id], repo_from)?)
    }

    pub fn repo_by_path(&self, root: &str) -> Result<Option<Repo>> {
        Ok(self
            .conn
            .query_row(&format!("SELECT {REPO_COLS} FROM repos WHERE root_path=?1"), [root], repo_from)
            .optional()?)
    }

    // ---- workspaces ----

    pub fn insert_workspace(&self, w: &Workspace) -> Result<()> {
        self.conn.execute(
            &format!("INSERT INTO workspaces ({WS_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"),
            params![w.id, w.repo_id, w.name, w.branch, w.path, w.target_branch, w.state, w.pinned, w.unread, w.notes, w.imported_from, w.created_at],
        )?;
        Ok(())
    }

    pub fn workspaces(&self, include_archived: bool) -> Result<Vec<Workspace>> {
        let filter = if include_archived { "" } else { "WHERE state='active'" };
        let mut st = self.conn.prepare(&format!(
            "SELECT {WS_COLS} FROM workspaces {filter} ORDER BY pinned DESC, created_at DESC"
        ))?;
        let rows = st.query_map([], workspace_from)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn workspace(&self, id: &str) -> Result<Workspace> {
        Ok(self.conn.query_row(&format!("SELECT {WS_COLS} FROM workspaces WHERE id=?1"), [id], workspace_from)?)
    }

    pub fn workspace_by_path(&self, path: &str) -> Result<Option<Workspace>> {
        Ok(self
            .conn
            .query_row(&format!("SELECT {WS_COLS} FROM workspaces WHERE path=?1"), [path], workspace_from)
            .optional()?)
    }

    pub fn set_workspace_state(&self, id: &str, state: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET state=?2 WHERE id=?1", params![id, state])?;
        Ok(())
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET pinned=?2 WHERE id=?1", params![id, pinned])?;
        Ok(())
    }

    pub fn set_unread(&self, id: &str, unread: bool) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET unread=?2 WHERE id=?1", params![id, unread])?;
        Ok(())
    }

    pub fn set_notes(&self, id: &str, notes: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET notes=?2 WHERE id=?1", params![id, notes])?;
        Ok(())
    }

    /// Mark a workspace's PR as merged; its star shines from now on.
    pub fn set_merged(&self, id: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET merged=1 WHERE id=?1", [id])?;
        Ok(())
    }

    /// Workspace names whose PR has merged (archived ones included).
    pub fn merged_names(&self) -> Result<Vec<String>> {
        let mut st = self.conn.prepare("SELECT name FROM workspaces WHERE merged=1")?;
        let rows = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn set_target_branch(&self, id: &str, branch: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET target_branch=?2 WHERE id=?1", params![id, branch])?;
        Ok(())
    }

    // ---- sessions ----

    pub fn insert_session(&self, s: &Session) -> Result<()> {
        self.conn.execute(
            &format!("INSERT INTO sessions ({SESSION_COLS}, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)"),
            params![s.id, s.workspace_id, s.agent, s.agent_session_id, s.title, s.model, s.permission_mode, now()],
        )?;
        Ok(())
    }

    pub fn sessions(&self, workspace_id: &str) -> Result<Vec<Session>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {SESSION_COLS} FROM sessions WHERE workspace_id=?1 ORDER BY created_at"
        ))?;
        let rows = st.query_map([workspace_id], session_from)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn session(&self, id: &str) -> Result<Session> {
        Ok(self.conn.query_row(&format!("SELECT {SESSION_COLS} FROM sessions WHERE id=?1"), [id], session_from)?)
    }

    pub fn set_agent_session_id(&self, id: &str, agent_session_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET agent_session_id=?2 WHERE id=?1",
            params![id, agent_session_id],
        )?;
        Ok(())
    }

    pub fn update_session_settings(&self, id: &str, title: &str, model: Option<&str>, permission_mode: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET title=?2, model=?3, permission_mode=?4 WHERE id=?1",
            params![id, title, model, permission_mode],
        )?;
        Ok(())
    }

    // ---- messages ----

    pub fn append_event(&self, session_id: &str, event: &serde_json::Value) -> Result<()> {
        self.conn.execute(
            "INSERT INTO messages (session_id, event, created_at) VALUES (?1,?2,?3)",
            params![session_id, event.to_string(), now()],
        )?;
        Ok(())
    }

    pub fn events(&self, session_id: &str) -> Result<Vec<serde_json::Value>> {
        let mut st = self.conn.prepare("SELECT event FROM messages WHERE session_id=?1 ORDER BY id")?;
        let rows = st
            .query_map([session_id], |r| r.get::<_, String>(0))?
            .filter_map(|s| s.ok().and_then(|s| serde_json::from_str(&s).ok()))
            .collect();
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample_repo() -> Repo {
        Repo {
            id: new_id(),
            name: "demo".into(),
            root_path: "/tmp/demo".into(),
            default_branch: "main".into(),
            remote_url: None,
            setup_script: Some("bun install".into()),
            run_script: None,
            archive_script: None,
        }
    }

    #[test]
    fn repo_workspace_session_round_trip() {
        let s = Store::in_memory().unwrap();
        let r = sample_repo();
        s.insert_repo(&r).unwrap();
        assert_eq!(s.repos().unwrap(), vec![r.clone()]);

        let w = Workspace {
            id: new_id(),
            repo_id: r.id.clone(),
            name: "mittens".into(),
            branch: "me/mittens".into(),
            path: "/tmp/ws/mittens".into(),
            target_branch: "main".into(),
            state: "active".into(),
            pinned: false,
            unread: false,
            notes: String::new(),
            imported_from: None,
            created_at: now(),
        };
        s.insert_workspace(&w).unwrap();
        s.set_pinned(&w.id, true).unwrap();
        assert!(s.workspace(&w.id).unwrap().pinned);

        s.set_merged(&w.id).unwrap();
        assert_eq!(s.merged_names().unwrap(), vec!["mittens".to_string()]);

        s.set_workspace_state(&w.id, "archived").unwrap();
        assert!(s.workspaces(false).unwrap().is_empty());
        assert_eq!(s.workspaces(true).unwrap().len(), 1);

        let sess = Session {
            id: new_id(),
            workspace_id: w.id.clone(),
            agent: "claude".into(),
            agent_session_id: None,
            title: "New chat".into(),
            model: None,
            permission_mode: "acceptEdits".into(),
        };
        s.insert_session(&sess).unwrap();
        s.set_agent_session_id(&sess.id, "abc").unwrap();
        assert_eq!(s.session(&sess.id).unwrap().agent_session_id.as_deref(), Some("abc"));

        s.append_event(&sess.id, &serde_json::json!({"kind": "text", "text": "hi"})).unwrap();
        assert_eq!(s.events(&sess.id).unwrap()[0]["text"], "hi");
    }
}
