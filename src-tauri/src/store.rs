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
    let add = |table: &str, col: &str, def: &str| -> Result<()> {
        let n: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name=?1"),
            [col],
            |r| r.get(0),
        )?;
        if n == 0 {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {def};"))?;
        }
        Ok(())
    };
    add("workspaces", "merged", "INTEGER NOT NULL DEFAULT 0")?;
    // A name the user typed; the star name is used when empty.
    add("workspaces", "title", "TEXT NOT NULL DEFAULT ''")?;
    // backlog | in_progress | in_review | done; empty means "derive from PR state".
    add("workspaces", "status", "TEXT NOT NULL DEFAULT ''")?;
    // Repo preferences as JSON: prompts, files to copy into new workspaces, env vars.
    // Bake-offs: two workspaces given the same task by different agents share a pair id.
    add("workspaces", "pair_id", "TEXT NOT NULL DEFAULT ''")?;
    // Sidebar sections the user makes ("Today", "Waiting on review"...); empty means none.
    add("workspaces", "section_id", "TEXT NOT NULL DEFAULT ''")?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS sections (id TEXT PRIMARY KEY, name TEXT NOT NULL, sort INTEGER NOT NULL DEFAULT 0);")?;
    add("repos", "prefs", "TEXT NOT NULL DEFAULT '{}'")?;
    add("sessions", "effort", "TEXT")?;
    add("sessions", "hidden", "INTEGER NOT NULL DEFAULT 0")?;
    // Extra folders the agent may read and edit (/add-dir), as a JSON list.
    add("sessions", "extra_dirs", "TEXT NOT NULL DEFAULT '[]'")?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
    // Spend per finished turn, so totals never have to scan every chat message.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS turn_costs (id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL, agent TEXT NOT NULL, cost REAL NOT NULL, created_at TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS turn_costs_at ON turn_costs(created_at);",
    )?;
    // Once: tag history imported before events carried an "imported" flag. Imports were written in bulk
    // with one timestamp per chat, while live events each get their own, so shared timestamps mark them.
    let done: i64 = conn.query_row("SELECT COUNT(*) FROM settings WHERE key='migrated_imported_flag'", [], |r| r.get(0))?;
    if done == 0 {
        conn.execute_batch(
            "UPDATE messages SET event = json_set(event, '$.imported', json('true'))
             WHERE (session_id, created_at) IN (SELECT session_id, created_at FROM messages GROUP BY 1, 2 HAVING COUNT(*) > 1);
             DELETE FROM messages WHERE json_extract(event, '$.kind') = 'usage';
             INSERT INTO settings (key, value) VALUES ('migrated_imported_flag', '1');",
        )?;
    }
    let done: i64 = conn.query_row("SELECT COUNT(*) FROM settings WHERE key='migrated_turn_costs'", [], |r| r.get(0))?;
    if done == 0 {
        conn.execute_batch(
            "INSERT INTO turn_costs (session_id, workspace_id, agent, cost, created_at)
             SELECT m.session_id, s.workspace_id, s.agent, json_extract(m.event, '$.cost_usd'), m.created_at
             FROM messages m JOIN sessions s ON s.id = m.session_id
             WHERE json_extract(m.event, '$.kind') = 'done' AND json_extract(m.event, '$.cost_usd') IS NOT NULL
               AND json_extract(m.event, '$.imported') IS NULL;
             INSERT INTO settings (key, value) VALUES ('migrated_turn_costs', '1');",
        )?;
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
    #[serde(default)]
    pub prefs: RepoPrefs,
}

/// Per-repo preferences. Every field is optional so old rows and partial JSON still load.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RepoPrefs {
    /// Added to every agent's instructions in this repo.
    pub general_prompt: String,
    /// Used by "Review" (the agent reviews its own diff).
    pub review_prompt: String,
    /// Used by "Create PR" (the agent writes the title and description).
    pub pr_prompt: String,
    /// Git-ignored files copied from the main checkout into each new workspace, as globs.
    pub copy_files: Vec<String>,
    /// Extra environment variables for agents, scripts and terminals in this repo.
    pub env: Vec<(String, String)>,
    /// How PRs merge: squash (default), merge or rebase.
    pub merge_method: String,
    /// Named commands besides Run, e.g. ("test", "bun test"). Run from the Run menu, ⌘K or /run name.
    pub actions: Vec<(String, String)>,
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
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub pair_id: String,
    #[serde(default)]
    pub section_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Section {
    pub id: String,
    pub name: String,
    pub sort: i64,
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
    #[serde(default)]
    pub effort: Option<String>,
}

/// (agent, finished turns, error turns, total turn time in ms, merged workspaces)
pub type AgentScore = (String, i64, i64, i64, i64);

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
        prefs: serde_json::from_str(&r.get::<_, String>(8)?).unwrap_or_default(),
    })
}
const REPO_COLS: &str =
    "id, name, root_path, default_branch, remote_url, setup_script, run_script, archive_script, prefs";

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
        title: r.get(12)?,
        status: r.get(13)?,
        pair_id: r.get(14)?,
        section_id: r.get(15)?,
    })
}
const WS_COLS: &str = "id, repo_id, name, branch, path, target_branch, state, pinned, unread, notes, imported_from, created_at, title, status, pair_id, section_id";

fn session_from(r: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        agent: r.get(2)?,
        agent_session_id: r.get(3)?,
        title: r.get(4)?,
        model: r.get(5)?,
        permission_mode: r.get(6)?,
        effort: r.get(7)?,
    })
}
const SESSION_COLS: &str =
    "id, workspace_id, agent, agent_session_id, title, model, permission_mode, effort";

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
            &format!("INSERT INTO repos ({REPO_COLS}, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"),
            params![r.id, r.name, r.root_path, r.default_branch, r.remote_url, r.setup_script, r.run_script, r.archive_script, serde_json::to_string(&r.prefs)?, now()],
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

    pub fn set_repo_prefs(&self, id: &str, prefs: &RepoPrefs) -> Result<()> {
        self.conn.execute("UPDATE repos SET prefs=?2 WHERE id=?1", params![id, serde_json::to_string(prefs)?])?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM settings WHERE key=?1", [key])?;
        Ok(())
    }

    pub fn settings_with_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let mut st = self.conn.prepare("SELECT key, value FROM settings WHERE substr(key, 1, length(?1)) = ?1")?;
        let rows = st.query_map([prefix], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
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
            &format!("INSERT INTO workspaces ({WS_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)"),
            params![w.id, w.repo_id, w.name, w.branch, w.path, w.target_branch, w.state, w.pinned, w.unread, w.notes, w.imported_from, w.created_at, w.title, w.status, w.pair_id, w.section_id],
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

    pub fn set_path(&self, id: &str, path: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET path=?2 WHERE id=?1", params![id, path])?;
        Ok(())
    }

    pub fn set_title(&self, id: &str, title: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET title=?2 WHERE id=?1", params![id, title])?;
        Ok(())
    }

    pub fn set_status(&self, id: &str, status: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET status=?2 WHERE id=?1", params![id, status])?;
        Ok(())
    }

    pub fn sections(&self) -> Result<Vec<Section>> {
        let mut st = self.conn.prepare("SELECT id, name, sort FROM sections ORDER BY sort, name")?;
        let rows = st.query_map([], |r| Ok(Section { id: r.get(0)?, name: r.get(1)?, sort: r.get(2)? }))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn create_section(&self, name: &str) -> Result<Section> {
        let sort: i64 = self.conn.query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM sections", [], |r| r.get(0))?;
        let sec = Section { id: new_id(), name: name.to_string(), sort };
        self.conn.execute("INSERT INTO sections (id, name, sort) VALUES (?1, ?2, ?3)", params![sec.id, sec.name, sec.sort])?;
        Ok(sec)
    }

    pub fn rename_section(&self, id: &str, name: &str) -> Result<()> {
        self.conn.execute("UPDATE sections SET name=?2 WHERE id=?1", params![id, name])?;
        Ok(())
    }

    /// Delete a section; its workspaces go back to their project groups.
    pub fn delete_section(&self, id: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET section_id='' WHERE section_id=?1", [id])?;
        self.conn.execute("DELETE FROM sections WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn set_section(&self, workspace_id: &str, section_id: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET section_id=?2 WHERE id=?1", params![workspace_id, section_id])?;
        Ok(())
    }

    pub fn set_pair(&self, ids: &[String], pair: &str) -> Result<()> {
        for id in ids {
            self.conn.execute("UPDATE workspaces SET pair_id=?2 WHERE id=?1", params![id, pair])?;
        }
        Ok(())
    }

    /// Spend per day (YYYY-MM-DD, UTC) and agent over the last `days` days.
    pub fn daily_costs(&self, days: i64) -> Result<Vec<(String, String, f64)>> {
        let mut st = self.conn.prepare(
            "SELECT substr(created_at, 1, 10) AS day, agent, SUM(cost) FROM turn_costs WHERE created_at >= ?1 GROUP BY day, agent ORDER BY day",
        )?;
        let since = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
        let rows = st.query_map([since], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<f64>>(2)?.unwrap_or(0.0))))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Per agent: finished turns, turns that ended in an error, total turn time (ms), workspaces whose
    /// PR merged. Only turns run in nekotrail count (imported history is skipped).
    pub fn agent_scorecard(&self) -> Result<Vec<AgentScore>> {
        let mut st = self.conn.prepare(
            "SELECT s.agent,
                    COUNT(*),
                    SUM(CASE WHEN json_extract(m.event, '$.is_error') = 1 AND json_extract(m.event, '$.stopped') IS NULL THEN 1 ELSE 0 END),
                    COALESCE(SUM(json_extract(m.event, '$.duration_ms')), 0),
                    (SELECT COUNT(DISTINCT w.id) FROM workspaces w JOIN sessions s2 ON s2.workspace_id = w.id WHERE w.merged = 1 AND s2.agent = s.agent)
             FROM messages m JOIN sessions s ON s.id = m.session_id
             WHERE json_extract(m.event, '$.kind') = 'done' AND json_extract(m.event, '$.imported') IS NULL
             GROUP BY s.agent",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// The last `limit` events of a chat, plus how many there are in all (big chats open fast).
    pub fn events_tail(&self, session_id: &str, limit: i64) -> Result<(Vec<serde_json::Value>, i64)> {
        let total: i64 = self.conn.query_row("SELECT COUNT(*) FROM messages WHERE session_id=?1", [session_id], |r| r.get(0))?;
        let mut st = self.conn.prepare(
            "SELECT event FROM (SELECT id, event FROM messages WHERE session_id=?1 ORDER BY id DESC LIMIT ?2) ORDER BY id",
        )?;
        let rows = st
            .query_map(params![session_id, limit], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok().and_then(|t| serde_json::from_str(&t).ok()))
            .collect();
        Ok((rows, total))
    }

    pub fn set_branch(&self, id: &str, branch: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET branch=?2 WHERE id=?1", params![id, branch])?;
        Ok(())
    }

    pub fn set_session_hidden(&self, id: &str, hidden: bool) -> Result<()> {
        self.conn.execute("UPDATE sessions SET hidden=?2 WHERE id=?1", params![id, hidden])?;
        Ok(())
    }

    pub fn extra_dirs(&self, id: &str) -> Result<Vec<String>> {
        let raw: String = self.conn.query_row("SELECT extra_dirs FROM sessions WHERE id=?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    pub fn set_extra_dirs(&self, id: &str, dirs: &[String]) -> Result<()> {
        self.conn.execute("UPDATE sessions SET extra_dirs=?2 WHERE id=?1", params![id, serde_json::to_string(dirs)?])?;
        Ok(())
    }

    pub fn set_session_effort(&self, id: &str, effort: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE sessions SET effort=?2 WHERE id=?1", params![id, effort])?;
        Ok(())
    }

    /// Record what a finished turn cost (from the CLI's own report).
    pub fn record_cost(&self, session_id: &str, cost: f64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO turn_costs (session_id, workspace_id, agent, cost, created_at)
             SELECT id, workspace_id, agent, ?2, ?3 FROM sessions WHERE id=?1",
            params![session_id, cost, now()],
        )?;
        Ok(())
    }

    /// Total agent cost (USD) per workspace.
    pub fn workspace_costs(&self) -> Result<Vec<(String, f64)>> {
        let mut st = self.conn.prepare("SELECT workspace_id, SUM(cost) FROM turn_costs GROUP BY workspace_id")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get::<_, Option<f64>>(1)?.unwrap_or(0.0))))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn set_target_branch(&self, id: &str, branch: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET target_branch=?2 WHERE id=?1", params![id, branch])?;
        Ok(())
    }

    // ---- sessions ----

    pub fn insert_session(&self, s: &Session) -> Result<()> {
        self.conn.execute(
            &format!("INSERT INTO sessions ({SESSION_COLS}, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"),
            params![s.id, s.workspace_id, s.agent, s.agent_session_id, s.title, s.model, s.permission_mode, s.effort, now()],
        )?;
        Ok(())
    }

    pub fn sessions(&self, workspace_id: &str) -> Result<Vec<Session>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {SESSION_COLS} FROM sessions WHERE workspace_id=?1 AND hidden=0 ORDER BY created_at"
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

    /// Append many events in one transaction (history import).
    pub fn append_events(&self, session_id: &str, events: &[serde_json::Value], at: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut st = tx.prepare("INSERT INTO messages (session_id, event, created_at) VALUES (?1,?2,?3)")?;
            for ev in events {
                st.execute(params![session_id, ev.to_string(), at])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn has_events(&self, session_id: &str) -> Result<bool> {
        Ok(self.conn.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE session_id=?1)", [session_id], |r| r.get(0))?)
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
            prefs: RepoPrefs { general_prompt: "Use bun.".into(), copy_files: vec![".env*".into()], ..Default::default() },
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
            title: String::new(),
            status: String::new(),
            pair_id: String::new(),
            section_id: String::new(),
        };
        s.insert_workspace(&w).unwrap();
        let sec = s.create_section("Today").unwrap();
        s.set_section(&w.id, &sec.id).unwrap();
        assert_eq!(s.workspace(&w.id).unwrap().section_id, sec.id);
        s.delete_section(&sec.id).unwrap();
        assert_eq!(s.workspace(&w.id).unwrap().section_id, "");
        assert!(s.sections().unwrap().is_empty());
        s.set_pair(std::slice::from_ref(&w.id), "p1").unwrap();
        assert_eq!(s.workspace(&w.id).unwrap().pair_id, "p1");
        s.set_title(&w.id, "Gateway fix").unwrap();
        s.set_status(&w.id, "in_review").unwrap();
        let back = s.workspace(&w.id).unwrap();
        assert_eq!((back.title.as_str(), back.status.as_str()), ("Gateway fix", "in_review"));
        s.set_setting("caffeinate", "off").unwrap();
        s.set_setting("caffeinate", "on").unwrap();
        assert_eq!(s.setting("caffeinate").unwrap().as_deref(), Some("on"));
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
            effort: Some("high".into()),
        };
        s.insert_session(&sess).unwrap();
        s.record_cost(&sess.id, 0.5).unwrap();
        s.record_cost(&sess.id, 0.25).unwrap();
        // Imported history is never recorded as spend.
        s.append_events(&sess.id, &[serde_json::json!({"kind": "done", "cost_usd": 99.0, "imported": true})], &now()).unwrap();
        assert_eq!(s.workspace_costs().unwrap(), vec![(w.id.clone(), 0.75)]);
        let daily = s.daily_costs(7).unwrap();
        assert_eq!(daily.len(), 1);
        assert_eq!((daily[0].1.as_str(), daily[0].2), ("claude", 0.75));
        s.set_agent_session_id(&sess.id, "abc").unwrap();
        assert_eq!(s.session(&sess.id).unwrap().agent_session_id.as_deref(), Some("abc"));

        s.append_event(&sess.id, &serde_json::json!({"kind": "text", "text": "hi"})).unwrap();
        assert_eq!(s.events(&sess.id).unwrap().last().unwrap()["text"], "hi");
        s.append_event(&sess.id, &serde_json::json!({"kind": "done", "is_error": true, "duration_ms": 1000})).unwrap();
        let card = s.agent_scorecard().unwrap();
        assert_eq!((card[0].0.as_str(), card[0].1, card[0].2, card[0].3), ("claude", 1, 1, 1000));
        let (tail, total) = s.events_tail(&sess.id, 1).unwrap();
        assert_eq!((tail.len(), tail[0]["is_error"].as_bool(), total), (1, Some(true), 3));
    }
}
