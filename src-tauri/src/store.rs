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
    // Stacked PRs: the workspace whose branch this one was started from; empty means none.
    add("workspaces", "parent_id", "TEXT NOT NULL DEFAULT ''")?;
    // Where the title came from: auto (first prompt), pr, user; empty means no title (see title.rs).
    add("workspaces", "title_source", "TEXT NOT NULL DEFAULT ''")?;
    // Review workspaces: the PR number being reviewed, 0 for every other workspace.
    add("workspaces", "review_pr", "INTEGER NOT NULL DEFAULT 0")?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS sections (id TEXT PRIMARY KEY, name TEXT NOT NULL, sort INTEGER NOT NULL DEFAULT 0);")?;
    add("repos", "prefs", "TEXT NOT NULL DEFAULT '{}'")?;
    add("sessions", "effort", "TEXT")?;
    add("sessions", "hidden", "INTEGER NOT NULL DEFAULT 0")?;
    // Extra folders the agent may read and edit (/add-dir), as a JSON list.
    add("sessions", "extra_dirs", "TEXT NOT NULL DEFAULT '[]'")?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
    // Draft review comments on the diff, kept per workspace until they are sent to the agent.
    // `code` is the commented diff lines, each with its +, - or space marker, so the app can tell
    // when the agent changed them (outdated). Empty for comments that have no lines to check.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS review_comments (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL,
            file TEXT NOT NULL, side TEXT NOT NULL, line INTEGER NOT NULL, end_line INTEGER NOT NULL,
            code TEXT NOT NULL DEFAULT '', text TEXT NOT NULL, created_at TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS review_comments_by_ws ON review_comments(workspace_id);",
    )?;
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
    let done: i64 = conn.query_row("SELECT COUNT(*) FROM settings WHERE key='migrated_title_source'", [], |r| r.get(0))?;
    if done == 0 {
        migrate_title_source(conn)?;
        conn.execute("INSERT INTO settings (key, value) VALUES ('migrated_title_source', '1')", [])?;
    }
    // Each active workspace's block of 10 ports (see ports.rs). A row lives only while its
    // workspace is active; `base` is unique so two workspaces can never hold the same block.
    conn.execute_batch("CREATE TABLE IF NOT EXISTS port_blocks (workspace_id TEXT PRIMARY KEY, base INTEGER NOT NULL UNIQUE);")?;
    // Sessions brought in from the CLIs' own folders (see terminal.rs), so importing again only adds
    // new ones, even after a chat's agent session id changes.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS terminal_imports (agent TEXT NOT NULL, source_id TEXT NOT NULL, session_id TEXT NOT NULL,
            file TEXT NOT NULL, imported_at TEXT NOT NULL, PRIMARY KEY (agent, source_id));",
    )?;
    // Global search: new messages are indexed as they arrive, older ones in the background.
    crate::search::ensure(conn)?;
    Ok(())
}

/// Once, when `title_source` arrives. Titles already there stay as they are: one that matches what
/// v0.10 to v0.12 derived from the first message is `auto` (so a PR title can replace it), anything
/// else is `user`. Untitled workspaces with a first prompt in their history get a title from it.
fn migrate_title_source(conn: &Connection) -> Result<()> {
    let rows: Vec<(String, String)> = {
        let mut st = conn.prepare("SELECT id, title FROM workspaces")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    for (id, title) in rows {
        let prompts = user_prompts(conn, &id, 5)?;
        let (title, source) = if title.is_empty() {
            match prompts.iter().map(|p| crate::title::task_title(p)).find(|t| !t.is_empty()) {
                Some(t) => (t, crate::title::AUTO),
                None => continue,
            }
        } else if prompts.first().is_some_and(|p| crate::title::legacy_task_title(p) == title) {
            (title, crate::title::AUTO)
        } else {
            (title, crate::title::USER)
        };
        conn.execute("UPDATE workspaces SET title=?2, title_source=?3 WHERE id=?1", params![id, title, source])?;
    }
    Ok(())
}

/// The first `limit` prompts the user sent in a workspace, oldest first, across its chats.
fn user_prompts(conn: &Connection, workspace_id: &str, limit: i64) -> Result<Vec<String>> {
    let mut st = conn.prepare(
        "SELECT json_extract(m.event, '$.text') FROM messages m JOIN sessions s ON s.id = m.session_id
         WHERE s.workspace_id = ?1 AND json_extract(m.event, '$.kind') = 'user' ORDER BY m.id LIMIT ?2",
    )?;
    let rows = st.query_map(params![workspace_id, limit], |r| r.get::<_, Option<String>>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().flatten().collect())
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
    /// Paths agents should leave alone, as gitignore-style globs (`.env*`, `migrations/**`).
    /// A repo's `nekotrail.json` can add more but never remove these (see `guard`).
    pub protected: Vec<String>,
    /// Run the Checks tab's checks after every agent turn that ends well.
    pub checks_after_turn: bool,
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
    /// Stacked PRs: the workspace whose branch this one started from (and targets), or "".
    #[serde(default)]
    pub parent_id: String,
    /// Where `title` came from: "auto", "pr" or "user"; "" while there is no title.
    #[serde(default)]
    pub title_source: String,
    /// Set on a review workspace: the number of the PR it reviews (someone else's). 0 for every other
    /// workspace. A review workspace is read-only: nothing is pushed from it (see `review`).
    #[serde(default)]
    pub review_pr: i64,
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

/// A draft review comment on lines `line..=end_line` of one changed file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewComment {
    pub id: String,
    pub file: String,
    /// "new" for added or unchanged lines, "old" when every commented line was removed.
    pub side: String,
    pub line: i64,
    pub end_line: i64,
    #[serde(default)]
    pub code: String,
    pub text: String,
}

/// (agent, finished turns, error turns, total turn time in ms, merged workspaces)
pub type AgentScore = (String, i64, i64, i64, i64);

pub struct Store {
    pub(crate) conn: Connection,
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
        parent_id: r.get(16)?,
        title_source: r.get(17)?,
        review_pr: r.get(18)?,
    })
}
const WS_COLS: &str = "id, repo_id, name, branch, path, target_branch, state, pinned, unread, notes, imported_from, created_at, title, status, pair_id, section_id, parent_id, title_source, review_pr";

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

/// The database holds repo environment variables (often API keys) and every chat, so only you may
/// read it: the folder 0700, the database and its WAL files 0600 (SQLite gives new WAL files the
/// database's mode).
fn private_files(db: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = db.parent() {
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    for suffix in ["", "-wal", "-shm"] {
        let mut p = db.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::set_permissions(Path::new(&p), std::fs::Permissions::from_mode(0o600));
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        private_files(path);
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
            &format!("INSERT INTO workspaces ({WS_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)"),
            params![w.id, w.repo_id, w.name, w.branch, w.path, w.target_branch, w.state, w.pinned, w.unread, w.notes, w.imported_from, w.created_at, w.title, w.status, w.pair_id, w.section_id, w.parent_id, w.title_source, w.review_pr],
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
        // An archived workspace gives its ports back. Restoring it hands out a block again.
        if state != "active" {
            self.release_port_block(id)?;
        }
        Ok(())
    }

    // ---- port blocks (see ports.rs) ----

    pub fn port_block(&self, workspace_id: &str) -> Result<Option<u16>> {
        Ok(self.conn.query_row("SELECT base FROM port_blocks WHERE workspace_id=?1", [workspace_id], |r| r.get(0)).optional()?)
    }

    /// Every block held by an active workspace, as (workspace id, first port).
    pub fn port_blocks(&self) -> Result<Vec<(String, u16)>> {
        let mut st = self.conn.prepare(
            "SELECT p.workspace_id, p.base FROM port_blocks p JOIN workspaces w ON w.id = p.workspace_id AND w.state = 'active' ORDER BY p.base",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Give `base` to the workspace. False when another active workspace already holds it.
    /// Rows left by workspaces that are no longer active are cleared first.
    pub fn claim_port_block(&self, workspace_id: &str, base: u16) -> Result<bool> {
        self.conn.execute(
            "DELETE FROM port_blocks WHERE workspace_id NOT IN (SELECT id FROM workspaces WHERE state = 'active')",
            [],
        )?;
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO port_blocks (workspace_id, base) VALUES (?1, ?2)",
            params![workspace_id, base],
        )?;
        Ok(n == 1)
    }

    pub fn release_port_block(&self, workspace_id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM port_blocks WHERE workspace_id=?1", [workspace_id])?;
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

    /// Sets the title if `source` may replace the current one (see `title::may_replace`).
    /// An empty title clears it. Returns whether anything changed.
    pub fn set_title(&self, id: &str, title: &str, source: &str) -> Result<bool> {
        let (cur_title, cur_source): (String, String) =
            self.conn.query_row("SELECT title, title_source FROM workspaces WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        // A title from before `title_source` (none should be left) counts as typed.
        let cur_source = if cur_source.is_empty() && !cur_title.is_empty() { crate::title::USER } else { cur_source.as_str() };
        if !crate::title::may_replace(cur_source, source) {
            return Ok(false);
        }
        let (title, source) = if title.is_empty() { ("", "") } else { (title, source) };
        if title == cur_title && source == cur_source {
            return Ok(false);
        }
        self.conn.execute("UPDATE workspaces SET title=?2, title_source=?3 WHERE id=?1", params![id, title, source])?;
        Ok(true)
    }

    /// The title the first prompts give a workspace, or "" when none says anything.
    pub fn prompt_title(&self, id: &str) -> Result<String> {
        Ok(user_prompts(&self.conn, id, 5)?.iter().map(|p| crate::title::task_title(p)).find(|t| !t.is_empty()).unwrap_or_default())
    }

    /// Untitled workspaces with prompts in their history (just imported ones) get a title from them.
    pub fn backfill_titles(&self) -> Result<usize> {
        let ids: Vec<String> = {
            let mut st = self.conn.prepare("SELECT id FROM workspaces WHERE title=''")?;
            st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        let mut n = 0;
        for id in ids {
            let t = self.prompt_title(&id)?;
            if !t.is_empty() && self.set_title(&id, &t, crate::title::AUTO)? {
                n += 1;
            }
        }
        Ok(n)
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

    /// Off the stack: the parent merged and this workspace now targets `branch` directly.
    pub fn unstack(&self, id: &str, branch: &str) -> Result<()> {
        self.conn.execute("UPDATE workspaces SET target_branch=?2, parent_id='' WHERE id=?1", params![id, branch])?;
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
        crate::search::index(&self.conn, self.conn.last_insert_rowid(), event)?;
        Ok(())
    }

    /// Append many events in one transaction (history import).
    pub fn append_events(&self, session_id: &str, events: &[serde_json::Value], at: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut st = tx.prepare("INSERT INTO messages (session_id, event, created_at) VALUES (?1,?2,?3)")?;
            for ev in events {
                st.execute(params![session_id, ev.to_string(), at])?;
                crate::search::index(&tx, tx.last_insert_rowid(), ev)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ---- terminal import (see terminal.rs) ----

    /// (agent, CLI session id) of every chat nekotrail has: imported from a terminal, or run here.
    pub fn terminal_known(&self) -> Result<std::collections::HashSet<(String, String)>> {
        let mut st = self.conn.prepare(
            "SELECT agent, source_id FROM terminal_imports
             UNION SELECT agent, agent_session_id FROM sessions WHERE agent_session_id IS NOT NULL",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// One imported chat in one transaction: the chat, its events (each with its own time, and
    /// indexed for search), and the record that it came in. False when it was already imported.
    pub fn import_terminal_chat(&self, s: &Session, agent: &str, source_id: &str, file: &str, created_at: &str, events: &[(serde_json::Value, String)]) -> Result<bool> {
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute(
            "INSERT OR IGNORE INTO terminal_imports (agent, source_id, session_id, file, imported_at) VALUES (?1,?2,?3,?4,?5)",
            params![agent, source_id, s.id, file, now()],
        )?;
        if n == 0 {
            return Ok(false);
        }
        tx.execute(
            &format!("INSERT INTO sessions ({SESSION_COLS}, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"),
            params![s.id, s.workspace_id, s.agent, s.agent_session_id, s.title, s.model, s.permission_mode, s.effort, created_at],
        )?;
        {
            let mut st = tx.prepare("INSERT INTO messages (session_id, event, created_at) VALUES (?1,?2,?3)")?;
            for (ev, at) in events {
                st.execute(params![s.id, ev.to_string(), if at.is_empty() { created_at } else { at.as_str() }])?;
                crate::search::index(&tx, tx.last_insert_rowid(), ev)?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    // ---- review comments ----

    pub fn review_comments(&self, workspace_id: &str) -> Result<Vec<ReviewComment>> {
        let mut st = self.conn.prepare(
            "SELECT id, file, side, line, end_line, code, text FROM review_comments WHERE workspace_id=?1 ORDER BY created_at, rowid",
        )?;
        let rows = st
            .query_map([workspace_id], |r| {
                Ok(ReviewComment { id: r.get(0)?, file: r.get(1)?, side: r.get(2)?, line: r.get(3)?, end_line: r.get(4)?, code: r.get(5)?, text: r.get(6)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Add comments in one go (one from the diff, or a batch of findings). An id already stored is
    /// left as it is, so adding the same batch twice keeps one copy.
    pub fn add_review_comments(&self, workspace_id: &str, comments: &[ReviewComment]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let at = now();
        for c in comments {
            anyhow::ensure!(c.side == "old" || c.side == "new", "unknown side {}", c.side);
            anyhow::ensure!(!c.text.trim().is_empty(), "a review comment needs some text");
            let (a, b) = (c.line.min(c.end_line), c.line.max(c.end_line));
            tx.execute(
                "INSERT OR IGNORE INTO review_comments (id, workspace_id, file, side, line, end_line, code, text, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![c.id, workspace_id, c.file, c.side, a, b, c.code, c.text.trim(), at],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Change one draft comment's text (the user edits a finding before it is posted).
    pub fn update_review_comment(&self, workspace_id: &str, id: &str, text: &str) -> Result<()> {
        anyhow::ensure!(!text.trim().is_empty(), "a review comment needs some text");
        let n = self.conn.execute("UPDATE review_comments SET text=?3 WHERE workspace_id=?1 AND id=?2", params![workspace_id, id, text.trim()])?;
        anyhow::ensure!(n == 1, "that comment is gone");
        Ok(())
    }

    /// Remove these comments from one workspace (deleted, cleared as outdated, or sent).
    pub fn remove_review_comments(&self, workspace_id: &str, ids: &[String]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for id in ids {
            tx.execute("DELETE FROM review_comments WHERE workspace_id=?1 AND id=?2", params![workspace_id, id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn has_events(&self, session_id: &str) -> Result<bool> {
        Ok(self.conn.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE session_id=?1)", [session_id], |r| r.get(0))?)
    }

    // ---- global search (see search.rs) ----

    pub fn search(&self, query: &str, filter: &crate::search::Filter) -> Result<Vec<crate::search::Group>> {
        crate::search::search(&self.conn, query, filter)
    }

    pub fn search_progress(&self) -> Result<crate::search::Progress> {
        crate::search::progress(&self.conn)
    }

    /// Index the next `batch` messages stored before the index existed; true once all are done.
    pub fn search_backfill_step(&self, batch: i64) -> Result<bool> {
        Ok(crate::search::backfill_step(&self.conn, batch)?.1)
    }

    pub fn message_place(&self, message_id: i64) -> Result<crate::search::Place> {
        crate::search::place(&self.conn, message_id)
    }

    /// Whether the user has sent anything in this chat yet.
    pub fn has_user_message(&self, session_id: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id=?1 AND json_extract(event, '$.kind')='user'",
            [session_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
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
    fn database_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("data");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let db = dir.join("nekotrail.db");
        let s = Store::open(&db).unwrap();
        s.set_setting("k", "v").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&db), 0o600);
        let wal = dir.join("nekotrail.db-wal");
        if wal.exists() {
            assert_eq!(mode(&wal), 0o600);
        }
    }

    #[test]
    fn review_comments_are_kept_per_workspace_until_removed() {
        let d = tempfile::tempdir().unwrap();
        let db = d.path().join("nekotrail.db");
        let c = |id: &str, line: i64, end: i64, text: &str| ReviewComment {
            id: id.into(), file: "src/app.ts".into(), side: "new".into(), line, end_line: end,
            code: "+let a = 1;\n+let b = 2;".into(), text: text.into(),
        };
        {
            let s = Store::open(&db).unwrap();
            s.add_review_comments("w1", &[c("a", 12, 13, "  Use const.  "), c("b", 40, 40, "Name this better.")]).unwrap();
            s.add_review_comments("w2", &[c("z", 1, 1, "Other workspace.")]).unwrap();
            // The same id twice keeps the first copy.
            s.add_review_comments("w1", &[c("a", 1, 1, "Duplicate")]).unwrap();
            // A range given backwards is stored start first; empty notes and unknown sides are refused.
            s.add_review_comments("w1", &[c("c", 9, 7, "Backwards range")]).unwrap();
            assert!(s.add_review_comments("w1", &[c("d", 1, 1, "   ")]).is_err());
            assert!(s.add_review_comments("w1", &[ReviewComment { side: "left".into(), ..c("e", 1, 1, "x") }]).is_err());
        }
        // Drafts survive a restart.
        let s = Store::open(&db).unwrap();
        let got = s.review_comments("w1").unwrap();
        assert_eq!(got.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c"]);
        assert_eq!(got[0], c("a", 12, 13, "Use const."));
        assert_eq!((got[2].line, got[2].end_line), (7, 9));
        // Removing only touches the named comments in that workspace.
        s.remove_review_comments("w1", &["a".into(), "z".into()]).unwrap();
        assert_eq!(s.review_comments("w1").unwrap().len(), 2);
        assert_eq!(s.review_comments("w2").unwrap().len(), 1);
    }

    #[test]
    fn stacked_workspaces_remember_their_parent_until_unstacked() {
        let s = Store::in_memory().unwrap();
        let r = sample_repo();
        s.insert_repo(&r).unwrap();
        let ws = |name: &str, target: &str, parent: &str| Workspace {
            id: new_id(), repo_id: r.id.clone(), name: name.into(), branch: format!("me/{name}"), path: format!("/tmp/ws/{name}"),
            target_branch: target.into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: now(), title: String::new(), status: String::new(), pair_id: String::new(),
            section_id: String::new(), parent_id: parent.into(), title_source: String::new(), review_pr: 0,
        };
        let parent = ws("mittens", "main", "");
        let child = ws("socks", "me/mittens", &parent.id);
        s.insert_workspace(&parent).unwrap();
        s.insert_workspace(&child).unwrap();
        assert_eq!(s.workspace(&child.id).unwrap().parent_id, parent.id);
        s.unstack(&child.id, "main").unwrap();
        let off = s.workspace(&child.id).unwrap();
        assert_eq!((off.parent_id.as_str(), off.target_branch.as_str()), ("", "main"));
        assert_eq!(s.workspace(&parent.id).unwrap(), parent);
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
            section_id: String::new(), parent_id: String::new(), title_source: String::new(), review_pr: 0,
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
        s.set_title(&w.id, "Gateway fix", "user").unwrap();
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
        // "Rules for this task" go with the first user message only.
        assert!(!s.has_user_message(&sess.id).unwrap());
        s.append_event(&sess.id, &serde_json::json!({"kind": "user", "text": "go"})).unwrap();
        assert!(s.has_user_message(&sess.id).unwrap());
    }

    /// A workspace with one chat whose user prompts are `prompts`.
    fn titled_ws(s: &Store, r: &Repo, name: &str, title: &str, prompts: &[&str]) -> String {
        let w = Workspace {
            id: new_id(), repo_id: r.id.clone(), name: name.into(), branch: format!("me/{name}"), path: format!("/tmp/ws/{name}"),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: now(), title: title.into(), status: String::new(), pair_id: String::new(),
            section_id: String::new(), parent_id: String::new(), title_source: String::new(), review_pr: 0,
        };
        s.insert_workspace(&w).unwrap();
        let sess = Session {
            id: new_id(), workspace_id: w.id.clone(), agent: "claude".into(), agent_session_id: None,
            title: "New chat".into(), model: None, permission_mode: "acceptEdits".into(), effort: None,
        };
        s.insert_session(&sess).unwrap();
        for p in prompts {
            s.append_event(&sess.id, &serde_json::json!({"kind": "user", "text": p})).unwrap();
            s.append_event(&sess.id, &serde_json::json!({"kind": "text", "text": "On it."})).unwrap();
        }
        w.id
    }

    #[test]
    fn title_sources_follow_the_rules() {
        use crate::title::{AUTO, PR, USER};
        let s = Store::in_memory().unwrap();
        let r = sample_repo();
        s.insert_repo(&r).unwrap();
        let id = titled_ws(&s, &r, "mochi", "", &[]);
        let get = |s: &Store| { let w = s.workspace(&id).unwrap(); (w.title, w.title_source) };
        // First prompt: auto fills the empty slot; a later prompt doesn't replace it.
        assert!(s.set_title(&id, "Fix the 502", AUTO).unwrap());
        assert!(!s.set_title(&id, "Add a test", AUTO).unwrap());
        assert_eq!(get(&s), ("Fix the 502".into(), AUTO.into()));
        // The PR title replaces it, and follows a PR rename.
        assert!(s.set_title(&id, "Outlive the ALB idle timeout", PR).unwrap());
        assert!(!s.set_title(&id, "Outlive the ALB idle timeout", PR).unwrap(), "same title is not a change");
        assert!(s.set_title(&id, "Fix 502s after deploy", PR).unwrap());
        assert!(!s.set_title(&id, "Something else", AUTO).unwrap());
        assert_eq!(get(&s), ("Fix 502s after deploy".into(), PR.into()));
        // A typed title is never overwritten by either.
        assert!(s.set_title(&id, "Gateway", USER).unwrap());
        assert!(!s.set_title(&id, "Fix 502s after deploy", PR).unwrap());
        assert!(!s.set_title(&id, "Fix the 502", AUTO).unwrap());
        assert_eq!(get(&s), ("Gateway".into(), USER.into()));
        // Clearing it by hand makes it automatic again.
        assert!(s.set_title(&id, "", USER).unwrap());
        assert_eq!(get(&s), (String::new(), String::new()));
        assert!(s.set_title(&id, "Fix the 502", AUTO).unwrap());
    }

    #[test]
    fn title_migration_and_backfill() {
        let s = Store::in_memory().unwrap();
        let r = sample_repo();
        s.insert_repo(&r).unwrap();
        // As a v0.12 database: titles but no sources.
        let legacy = titled_ws(&s, &r, "mochi", "Fix the 502 after deploy", &["Fix the 502 after deploy. Then add a test."]);
        let typed = titled_ws(&s, &r, "comet", "Gateway work", &["Fix the 502 after deploy."]);
        let untitled = titled_ws(&s, &r, "dash", "", &["/mcp", "## add **rate limiting** to @src/api.ts"]);
        let empty = titled_ws(&s, &r, "pip", "", &[]);
        s.conn.execute_batch("UPDATE workspaces SET title_source=''; DELETE FROM settings WHERE key='migrated_title_source';").unwrap();
        migrate(&s.conn).unwrap();
        let get = |id: &str| { let w = s.workspace(id).unwrap(); (w.title, w.title_source) };
        assert_eq!(get(&legacy), ("Fix the 502 after deploy".into(), "auto".into()));
        assert_eq!(get(&typed), ("Gateway work".into(), "user".into()));
        // Skips the prompt that says nothing (a bare /mcp) and takes the next.
        assert_eq!(get(&untitled), ("Add rate limiting".into(), "auto".into()));
        assert_eq!(get(&empty), (String::new(), String::new()));
        // Once only: a title cleared afterwards is not filled again by the migration.
        s.set_title(&untitled, "", "user").unwrap();
        migrate(&s.conn).unwrap();
        assert_eq!(get(&untitled), (String::new(), String::new()));
        // An import's backfill fills untitled workspaces that have prompts, and nothing else.
        assert_eq!(s.backfill_titles().unwrap(), 1);
        assert_eq!(get(&untitled).0, "Add rate limiting");
        assert_eq!(get(&typed).0, "Gateway work");
        assert_eq!(s.prompt_title(&empty).unwrap(), "");
    }
}
