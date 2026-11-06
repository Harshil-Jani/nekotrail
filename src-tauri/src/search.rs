//! Global search over every chat: an SQLite FTS5 index of user prompts, agent replies, the shell
//! commands agents ran and the files they touched. Each row's rowid is its message id, so a hit
//! leads straight back to the message.
//!
//! New events are indexed as they are stored (`Store::append_event`). Messages stored before the
//! index existed are indexed once in the background, in small batches (`backfill_step`), so
//! opening the app never waits for it. Deleting a message deletes its row (a trigger).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

/// Bump when what gets indexed changes: the index is dropped and built again in the background.
const VERSION: &str = "1";
/// Agent replies longer than this are cut; nobody searches for words 32 KB into one reply.
const MAX_BODY: usize = 32 * 1024;
/// Hits shown per workspace before "more in this workspace".
pub const PER_WORKSPACE: usize = 6;

/// What a row is, for filters and icons.
pub const USER: &str = "user";
pub const AGENT: &str = "agent";
pub const COMMAND: &str = "command";
pub const FILE: &str = "file";


/// Create the index if it is missing or out of date. Runs when the store opens; cheap either way.
/// Messages already stored are left for `backfill_step` (rows up to `search_backfill_end`).
pub fn ensure(conn: &Connection) -> Result<()> {
    let have: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key='search_index_version'", [], |r| r.get(0))
        .optional()?;
    if have.as_deref() == Some(VERSION) {
        return Ok(());
    }
    let end: i64 = conn.query_row("SELECT COALESCE(MAX(id), 0) FROM messages", [], |r| r.get(0))?;
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS search_fts_gone;
         DROP TABLE IF EXISTS search_fts;
         CREATE VIRTUAL TABLE search_fts USING fts5(body, kind UNINDEXED, tokenize = 'unicode61 remove_diacritics 2', prefix = '2 3');
         CREATE TRIGGER search_fts_gone AFTER DELETE ON messages BEGIN DELETE FROM search_fts WHERE rowid = old.id; END;",
    )?;
    let set = |k: &str, v: &str| conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![k, v]);
    set("search_backfill_end", &end.to_string())?;
    set("search_backfill_at", "0")?;
    set("search_index_version", VERSION)?;
    Ok(())
}

/// Control characters out (they are our snippet markers), long text cut at a char boundary.
fn clean(s: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_control() && c != '\n' && c != '\t' { ' ' } else { c }).collect();
    if out.len() > MAX_BODY {
        let mut end = MAX_BODY;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
    }
    out
}

/// The searchable text of one chat event and what it is, or None for events nobody searches
/// (tool output, thinking, usage, done...). Every agent's shell tool is named "Bash" in the chat.
pub fn doc(ev: &Value) -> Option<(&'static str, String)> {
    let s = |k: &str| ev[k].as_str().map(str::trim).filter(|t| !t.is_empty());
    let (kind, text) = match ev["kind"].as_str()? {
        "user" => (USER, s("text")?),
        "text" => (AGENT, s("text")?),
        "tool" => match ev["name"].as_str()? {
            "Bash" => (COMMAND, s("input")?),
            "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" | "Delete" | "Move" => (FILE, s("input")?),
            _ => return None,
        },
        _ => return None,
    };
    Some((kind, clean(text)))
}

/// Index one stored message (no-op for events `doc` skips).
pub fn index(conn: &Connection, id: i64, ev: &Value) -> Result<()> {
    if let Some((kind, body)) = doc(ev) {
        conn.execute("INSERT INTO search_fts (rowid, body, kind) VALUES (?1, ?2, ?3)", params![id, body, kind])?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Default)]
pub struct Progress {
    /// Older messages indexed so far and in all. Both 0 when there was nothing to backfill.
    pub done: i64,
    pub total: i64,
    pub finished: bool,
}

fn setting_i64(conn: &Connection, key: &str) -> Result<i64> {
    let v: Option<String> = conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0)).optional()?;
    Ok(v.and_then(|v| v.parse().ok()).unwrap_or(0))
}

/// Where the backfill stands: messages up to `end` that existed before the index, `at` the last
/// one indexed. Counts are by message, from the rowid range, so this stays cheap.
pub fn progress(conn: &Connection) -> Result<Progress> {
    let (at, end) = (setting_i64(conn, "search_backfill_at")?, setting_i64(conn, "search_backfill_end")?);
    let count = |lo: i64, hi: i64| -> rusqlite::Result<i64> {
        conn.query_row("SELECT COUNT(*) FROM messages WHERE id > ?1 AND id <= ?2", params![lo, hi], |r| r.get(0))
    };
    let total = count(0, end)?;
    let done = count(0, at.min(end))?;
    Ok(Progress { done, total, finished: at >= end })
}

/// Index the next `batch` older messages in one transaction. Returns how many were read and
/// whether the backfill is finished. Live messages are indexed as they arrive, so they are never here.
pub fn backfill_step(conn: &Connection, batch: i64) -> Result<(i64, bool)> {
    let (at, end) = (setting_i64(conn, "search_backfill_at")?, setting_i64(conn, "search_backfill_end")?);
    if at >= end {
        return Ok((0, true));
    }
    let tx = conn.unchecked_transaction()?;
    let rows: Vec<(i64, String)> = {
        let mut st = tx.prepare_cached("SELECT id, event FROM messages WHERE id > ?1 AND id <= ?2 ORDER BY id LIMIT ?3")?;
        st.query_map(params![at, end, batch], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    {
        let mut ins = tx.prepare_cached("INSERT INTO search_fts (rowid, body, kind) VALUES (?1, ?2, ?3)")?;
        for (id, raw) in &rows {
            let Ok(ev) = serde_json::from_str::<Value>(raw) else { continue };
            if let Some((kind, body)) = doc(&ev) {
                ins.execute(params![id, body, kind])?;
            }
        }
    }
    let next = rows.last().map(|r| r.0).unwrap_or(end);
    let next = if (rows.len() as i64) < batch { end } else { next };
    tx.execute("UPDATE settings SET value=?1 WHERE key='search_backfill_at'", [next.to_string()])?;
    tx.commit()?;
    Ok((rows.len() as i64, next >= end))
}

/// Turn what the user typed into a safe FTS5 query: every word becomes a quoted phrase (quotes
/// doubled), so `NOT`, `OR`, `col:`, `*`, `(`, `^` and `-` are just text. Words are ANDed and the
/// last one matches as a prefix while you are still typing it. None when nothing is searchable.
pub fn fts_query(input: &str) -> Option<String> {
    let words: Vec<String> = input
        .split_whitespace()
        .map(|w| w.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .collect();
    let last = words.len().checked_sub(1)?;
    let prefix = !input.ends_with(char::is_whitespace);
    let q = words
        .iter()
        .enumerate()
        .map(|(i, w)| format!("\"{}\"{}", w.replace('"', "\"\""), if i == last && prefix { "*" } else { "" }))
        .collect::<Vec<_>>()
        .join(" ");
    Some(q)
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Filter {
    pub repo: Option<String>,
    pub agent: Option<String>,
    pub workspace: Option<String>,
    /// RFC 3339; only messages from then on.
    pub since: Option<String>,
    /// "user" (only my messages) or "command" (only commands).
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Hit {
    pub message_id: i64,
    pub session_id: String,
    pub kind: String,
    /// The matching text around the hit, matches wrapped in \u{2}...\u{3}. Control characters
    /// never reach the index (see `clean`), so the UI can split on them instead of trusting HTML.
    pub snippet: String,
    pub created_at: String,
    pub agent: String,
    pub chat: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Group {
    pub workspace_id: String,
    pub repo_id: String,
    pub repo: String,
    pub name: String,
    pub title: String,
    pub branch: String,
    pub state: String,
    pub hits: Vec<Hit>,
    /// Hits in this workspace beyond the ones listed.
    pub more: usize,
}

/// Search every chat. Groups come in order of their best hit; hits inside a group by relevance,
/// newer first on ties. User prompts weigh a little more, since that is usually what you remember.
pub fn search(conn: &Connection, input: &str, f: &Filter) -> Result<Vec<Group>> {
    let Some(q) = fts_query(input) else { return Ok(vec![]) };
    let limit = f.limit.unwrap_or(300).clamp(1, 2000);
    let per = if f.workspace.is_some() { usize::MAX } else { PER_WORKSPACE };
    let mut st = conn.prepare_cached(
        "SELECT f.rowid, f.kind, snippet(search_fts, 0, char(2), char(3), '…', 16), m.session_id, m.created_at,
                s.agent, s.title, w.id, w.repo_id, COALESCE(r.name, ''), w.name, w.title, w.branch, w.state
         FROM search_fts f
         JOIN messages m ON m.id = f.rowid
         JOIN sessions s ON s.id = m.session_id
         JOIN workspaces w ON w.id = s.workspace_id
         LEFT JOIN repos r ON r.id = w.repo_id
         WHERE search_fts MATCH ?1
           AND (?2 IS NULL OR w.repo_id = ?2)
           AND (?3 IS NULL OR s.agent = ?3)
           AND (?4 IS NULL OR w.id = ?4)
           AND (?5 IS NULL OR m.created_at >= ?5)
           AND (?6 IS NULL OR f.kind = ?6)
         ORDER BY bm25(search_fts) * (CASE f.kind WHEN 'user' THEN 1.25 ELSE 1.0 END), f.rowid DESC
         LIMIT ?7",
    )?;
    let mut groups: Vec<Group> = vec![];
    let mut rows = st.query(params![q, f.repo, f.agent, f.workspace, f.since, f.kind, limit])?;
    while let Some(r) = rows.next()? {
        let ws: String = r.get(7)?;
        let i = match groups.iter().position(|g| g.workspace_id == ws) {
            Some(i) => i,
            None => {
                groups.push(Group {
                    workspace_id: ws,
                    repo_id: r.get(8)?,
                    repo: r.get(9)?,
                    name: r.get(10)?,
                    title: r.get(11)?,
                    branch: r.get(12)?,
                    state: r.get(13)?,
                    hits: vec![],
                    more: 0,
                });
                groups.len() - 1
            }
        };
        let g = &mut groups[i];
        if g.hits.len() >= per {
            g.more += 1;
            continue;
        }
        g.hits.push(Hit {
            message_id: r.get(0)?,
            kind: r.get(1)?,
            snippet: r.get(2)?,
            session_id: r.get(3)?,
            created_at: r.get(4)?,
            agent: r.get(5)?,
            chat: r.get(6)?,
        });
    }
    Ok(groups)
}

/// Where a message sits: its chat, its workspace, how many events of the chat come before it
/// and how many there are in all, so the chat can load enough history to show it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Place {
    pub session_id: String,
    pub workspace_id: String,
    pub index: i64,
    pub total: i64,
}

pub fn place(conn: &Connection, message_id: i64) -> Result<Place> {
    let (session_id, workspace_id): (String, String) = conn.query_row(
        "SELECT m.session_id, s.workspace_id FROM messages m JOIN sessions s ON s.id = m.session_id WHERE m.id = ?1",
        [message_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (index, total): (i64, i64) = conn.query_row(
        "SELECT SUM(id < ?2), COUNT(*) FROM messages WHERE session_id = ?1",
        params![session_id, message_id],
        |r| Ok((r.get::<_, Option<i64>>(0)?.unwrap_or(0), r.get(1)?)),
    )?;
    Ok(Place { session_id, workspace_id, index, total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{new_id, now, Repo, RepoPrefs, Session, Store, Workspace};
    use serde_json::json;

    fn repo(s: &Store, name: &str) -> Repo {
        let r = Repo {
            id: new_id(), name: name.into(), root_path: format!("/tmp/{name}"), default_branch: "main".into(),
            remote_url: None, setup_script: None, run_script: None, archive_script: None, prefs: RepoPrefs::default(),
        };
        s.insert_repo(&r).unwrap();
        r
    }

    /// A workspace with one chat; returns (workspace id, session id).
    fn chat(s: &Store, r: &Repo, name: &str, agent: &str) -> (String, String) {
        let w = Workspace {
            id: new_id(), repo_id: r.id.clone(), name: name.into(), branch: format!("me/{name}"), path: format!("/tmp/ws/{name}"),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: now(), title: String::new(), status: String::new(), pair_id: String::new(),
            section_id: String::new(), parent_id: String::new(), title_source: String::new(),
        };
        s.insert_workspace(&w).unwrap();
        let sess = Session {
            id: new_id(), workspace_id: w.id.clone(), agent: agent.into(), agent_session_id: None,
            title: "New chat".into(), model: None, permission_mode: "acceptEdits".into(), effort: None,
        };
        s.insert_session(&sess).unwrap();
        (w.id, sess.id)
    }

    fn ids(groups: &[Group]) -> Vec<i64> {
        groups.iter().flat_map(|g| g.hits.iter().map(|h| h.message_id)).collect()
    }

    fn last_id(s: &Store) -> i64 {
        s.conn.query_row("SELECT MAX(id) FROM messages", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn indexes_prompts_replies_commands_and_files_as_they_arrive() {
        let s = Store::in_memory().unwrap();
        let r = repo(&s, "pawprint-api");
        let (w, sid) = chat(&s, &r, "mochi", "claude");
        s.append_event(&sid, &json!({"kind": "user", "text": "Why does the gateway return 502 after a deploy?"})).unwrap();
        s.append_event(&sid, &json!({"kind": "thinking", "text": "gateway thoughts"})).unwrap();
        s.append_event(&sid, &json!({"kind": "tool", "id": "t1", "name": "Bash", "input": "kubectl logs deploy/gateway | grep 502"})).unwrap();
        s.append_event(&sid, &json!({"kind": "tool_result", "id": "t1", "output": "gateway gateway gateway", "is_error": false})).unwrap();
        s.append_event(&sid, &json!({"kind": "tool", "id": "t2", "name": "Edit", "input": "internal/server/http_idle.go"})).unwrap();
        s.append_event(&sid, &json!({"kind": "text", "text": "The idle timeout was shorter than the load balancer's."})).unwrap();
        let all = search(&s.conn, "gateway", &Filter::default()).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].workspace_id, w);
        let kinds: Vec<&str> = all[0].hits.iter().map(|h| h.kind.as_str()).collect();
        // Thinking and tool output are not indexed.
        assert_eq!(kinds.len(), 2);
        assert!(kinds.contains(&USER) && kinds.contains(&COMMAND));
        // Paths match by their parts, including words joined by _ and the extension.
        assert_eq!(search(&s.conn, "http_idle", &Filter::default()).unwrap()[0].hits[0].kind, FILE);
        assert_eq!(search(&s.conn, "server/http_idle.go", &Filter::default()).unwrap()[0].hits[0].kind, FILE);
        // The last word is a prefix while typing.
        assert_eq!(ids(&search(&s.conn, "idle time", &Filter::default()).unwrap()).len(), 1);
        assert!(search(&s.conn, "idle time ", &Filter::default()).unwrap().is_empty(), "a finished word is whole");
        // Snippets mark the match.
        let snip = &search(&s.conn, "balancer", &Filter::default()).unwrap()[0].hits[0].snippet;
        assert!(snip.contains("\u{2}balancer\u{3}"), "{snip}");
        // History import (append_events) is indexed too.
        s.append_events(&sid, &[json!({"kind": "user", "text": "imported question about kittens"})], &now()).unwrap();
        assert_eq!(ids(&search(&s.conn, "kittens", &Filter::default()).unwrap()).len(), 1);
    }

    #[test]
    fn backfills_older_messages_in_batches_without_doubling_new_ones() {
        let s = Store::in_memory().unwrap();
        let r = repo(&s, "api");
        let (w, sid) = chat(&s, &r, "mochi", "claude");
        let all = Filter { workspace: Some(w), limit: Some(100), ..Default::default() };
        // As a database from before the index: messages but no search rows.
        for i in 0..25 {
            s.append_event(&sid, &json!({"kind": "user", "text": format!("old prompt number {i} about caching")})).unwrap();
        }
        s.conn.execute_batch("DELETE FROM settings WHERE key='search_index_version'").unwrap();
        ensure(&s.conn).unwrap();
        assert!(search(&s.conn, "caching", &Filter::default()).unwrap().is_empty());
        assert_eq!(progress(&s.conn).unwrap(), Progress { done: 0, total: 25, finished: false });
        // A message arriving mid-backfill is indexed right away, and only once.
        s.append_event(&sid, &json!({"kind": "user", "text": "new prompt about caching"})).unwrap();
        assert_eq!(ids(&search(&s.conn, "caching", &Filter::default()).unwrap()).len(), 1);
        assert_eq!(backfill_step(&s.conn, 10).unwrap(), (10, false));
        assert_eq!(progress(&s.conn).unwrap(), Progress { done: 10, total: 25, finished: false });
        assert_eq!(backfill_step(&s.conn, 10).unwrap(), (10, false));
        assert_eq!(backfill_step(&s.conn, 10).unwrap(), (5, true));
        assert_eq!(backfill_step(&s.conn, 10).unwrap(), (0, true));
        assert_eq!(progress(&s.conn).unwrap(), Progress { done: 25, total: 25, finished: true });
        let found = ids(&search(&s.conn, "caching", &all).unwrap());
        let mut uniq = found.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!((found.len(), uniq.len()), (26, 26));
        // Opening again changes nothing; a deleted message leaves the index.
        ensure(&s.conn).unwrap();
        assert!(progress(&s.conn).unwrap().finished);
        let newest = last_id(&s);
        s.conn.execute("DELETE FROM messages WHERE id=?1", [newest]).unwrap();
        assert!(!ids(&search(&s.conn, "caching", &all).unwrap()).contains(&newest));
        let n: i64 = s.conn.query_row("SELECT COUNT(*) FROM search_fts", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 25);
    }

    #[test]
    fn ranks_by_relevance_then_newest_and_groups_by_workspace() {
        let s = Store::in_memory().unwrap();
        let r = repo(&s, "api");
        let (wa, a) = chat(&s, &r, "mochi", "claude");
        let (wb, b) = chat(&s, &r, "comet", "codex");
        s.append_event(&a, &json!({"kind": "text", "text": "We could add a retry here, along with a lot of other unrelated words about logging, metrics, dashboards and alerts."})).unwrap();
        let weak = last_id(&s);
        s.append_event(&b, &json!({"kind": "text", "text": "retry with backoff, retry on 503"})).unwrap();
        let strong = last_id(&s);
        s.append_event(&a, &json!({"kind": "text", "text": "retry with backoff, retry on 503"})).unwrap();
        let strong_newer = last_id(&s);
        let g = search(&s.conn, "retry", &Filter::default()).unwrap();
        // Equal text: the newer one first, so its workspace leads; the long reply comes last.
        assert_eq!(g.iter().map(|g| g.workspace_id.as_str()).collect::<Vec<_>>(), vec![wa.as_str(), wb.as_str()]);
        assert_eq!(g[0].hits.iter().map(|h| h.message_id).collect::<Vec<_>>(), vec![strong_newer, weak]);
        assert_eq!(g[1].hits[0].message_id, strong);
        // Your own prompt beats an agent reply with the same text.
        s.append_event(&b, &json!({"kind": "user", "text": "flaky login test"})).unwrap();
        let mine = last_id(&s);
        s.append_event(&a, &json!({"kind": "text", "text": "flaky login test"})).unwrap();
        assert_eq!(ids(&search(&s.conn, "flaky", &Filter::default()).unwrap())[0], mine);
        // Many hits in one workspace: a few listed, the rest counted; the workspace filter lists all.
        for i in 0..10 {
            s.append_event(&a, &json!({"kind": "text", "text": format!("pagination step {i}")})).unwrap();
        }
        let g = search(&s.conn, "pagination", &Filter::default()).unwrap();
        assert_eq!((g[0].hits.len(), g[0].more), (PER_WORKSPACE, 10 - PER_WORKSPACE));
        let g = search(&s.conn, "pagination", &Filter { workspace: Some(wa.clone()), ..Default::default() }).unwrap();
        assert_eq!((g[0].hits.len(), g[0].more), (10, 0));
    }

    #[test]
    fn filters_by_repo_agent_date_and_kind() {
        let s = Store::in_memory().unwrap();
        let api = repo(&s, "api");
        let web = repo(&s, "web");
        let (_, a) = chat(&s, &api, "mochi", "claude");
        let (_, b) = chat(&s, &web, "comet", "codex");
        s.append_event(&a, &json!({"kind": "user", "text": "deploy the worker"})).unwrap();
        s.append_event(&a, &json!({"kind": "tool", "id": "1", "name": "Bash", "input": "fly deploy --app worker"})).unwrap();
        s.append_event(&b, &json!({"kind": "text", "text": "deploy preview is up"})).unwrap();
        let n = |f: Filter| ids(&search(&s.conn, "deploy", &f).unwrap()).len();
        assert_eq!(n(Filter::default()), 3);
        assert_eq!(n(Filter { repo: Some(web.id.clone()), ..Default::default() }), 1);
        assert_eq!(n(Filter { agent: Some("claude".into()), ..Default::default() }), 2);
        assert_eq!(n(Filter { kind: Some(USER.into()), ..Default::default() }), 1);
        assert_eq!(n(Filter { kind: Some(COMMAND.into()), ..Default::default() }), 1);
        // Back-date one message: a "last 7 days" filter drops it.
        s.conn.execute("UPDATE messages SET created_at='2020-01-01T00:00:00+00:00' WHERE json_extract(event, '$.kind')='text'", []).unwrap();
        let week = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        assert_eq!(n(Filter { since: Some(week), ..Default::default() }), 2);
    }

    #[test]
    fn query_syntax_is_always_plain_text() {
        assert_eq!(fts_query("fix 502").as_deref(), Some("\"fix\" \"502\"*"));
        assert_eq!(fts_query("fix 502 ").as_deref(), Some("\"fix\" \"502\""));
        assert_eq!(fts_query("say \"hi\"").as_deref(), Some("\"say\" \"\"\"hi\"\"\"*"));
        assert_eq!(fts_query("   ").as_deref(), None);
        assert_eq!(fts_query("-- ** ()").as_deref(), None);
        let s = Store::in_memory().unwrap();
        let r = repo(&s, "api");
        let (_, sid) = chat(&s, &r, "mochi", "claude");
        s.append_event(&sid, &json!({"kind": "user", "text": "NOT this OR that: body:x \"quoted\" a*b (paren) ^start NEAR(a b) -flag"})).unwrap();
        for q in ["NOT", "OR", "body:x", "\"", "\"quoted", "a*b", "(paren", "^start", "NEAR(a", "-flag", "AND OR NOT", "x\"y", "\u{2}", "'; DROP TABLE messages; --", "*", "col:"] {
            let got = search(&s.conn, q, &Filter::default());
            assert!(got.is_ok(), "{q:?}: {got:?}");
        }
        assert_eq!(ids(&search(&s.conn, "NOT", &Filter::default()).unwrap()).len(), 1, "NOT is a word, not an operator");
        assert_eq!(ids(&search(&s.conn, "body:x", &Filter::default()).unwrap()).len(), 1, "not a column filter");
        assert_eq!(ids(&search(&s.conn, "-flag", &Filter::default()).unwrap()).len(), 1, "not a negation");
        assert!(search(&s.conn, "'; DROP TABLE messages; --", &Filter::default()).is_ok());
        assert!(s.has_events(&sid).unwrap());
        // Control characters in a message can't fake snippet markers.
        s.append_event(&sid, &json!({"kind": "user", "text": "odd \u{2}marker\u{3} text"})).unwrap();
        let snip = &search(&s.conn, "odd", &Filter::default()).unwrap()[0].hits[0].snippet;
        assert_eq!(snip.matches('\u{2}').count(), 1, "{snip:?}");
    }

    #[test]
    fn archived_workspaces_are_searched_and_places_are_found() {
        let s = Store::in_memory().unwrap();
        let r = repo(&s, "api");
        let (w, sid) = chat(&s, &r, "mochi", "claude");
        for i in 0..5 {
            s.append_event(&sid, &json!({"kind": "text", "text": format!("filler {i}")})).unwrap();
        }
        s.append_event(&sid, &json!({"kind": "user", "text": "remember the walrus"})).unwrap();
        let target = last_id(&s);
        s.append_event(&sid, &json!({"kind": "text", "text": "after"})).unwrap();
        s.set_workspace_state(&w, "archived").unwrap();
        let g = search(&s.conn, "walrus", &Filter::default()).unwrap();
        assert_eq!((g[0].state.as_str(), g[0].repo.as_str(), g[0].name.as_str()), ("archived", "api", "mochi"));
        assert_eq!(place(&s.conn, target).unwrap(), Place { session_id: sid.clone(), workspace_id: w, index: 5, total: 7 });
        assert!(place(&s.conn, 999_999).is_err());
    }

    /// 20k messages (a heavy user's store): time the backfill and a search. Prints the numbers.
    #[test]
    fn backfill_20k_messages() {
        let s = Store::in_memory().unwrap();
        let r = repo(&s, "api");
        let words = ["gateway", "timeout", "retry", "cache", "deploy", "worker", "schema", "login", "billing", "export"];
        let mut sessions = vec![];
        for i in 0..40 {
            sessions.push(chat(&s, &r, &format!("ws{i}"), if i % 2 == 0 { "claude" } else { "codex" }).1);
        }
        let reply = "I looked at the handler and the tests. The fix is small: read the value from config, add a test for the edge case, and keep the old default so nothing else changes. ".repeat(3);
        let t = std::time::Instant::now();
        s.conn.execute_batch("DROP TRIGGER search_fts_gone; DROP TABLE search_fts; DELETE FROM settings WHERE key LIKE 'search_%';").unwrap();
        let tx = s.conn.unchecked_transaction().unwrap();
        for i in 0..20_000usize {
            let w = words[i % words.len()];
            let ev = match i % 4 {
                0 => json!({"kind": "user", "text": format!("Please fix the {w} bug number {i}")}),
                1 => json!({"kind": "tool", "id": i.to_string(), "name": "Bash", "input": format!("go test ./internal/{w}/... -run Test{i}")}),
                2 => json!({"kind": "tool_result", "id": (i - 1).to_string(), "output": reply, "is_error": false}),
                _ => json!({"kind": "text", "text": format!("{reply} ({w} {i})")}),
            };
            tx.execute("INSERT INTO messages (session_id, event, created_at) VALUES (?1, ?2, ?3)", params![sessions[i % 40], ev.to_string(), now()]).unwrap();
        }
        tx.commit().unwrap();
        eprintln!("seeded 20k messages in {:?}", t.elapsed());
        let t = std::time::Instant::now();
        ensure(&s.conn).unwrap();
        eprintln!("ensure (create index, mark backfill) {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let mut batches = 0;
        let mut worst = std::time::Duration::ZERO;
        loop {
            let b = std::time::Instant::now();
            let (_, done) = backfill_step(&s.conn, 500).unwrap();
            worst = worst.max(b.elapsed());
            batches += 1;
            if done {
                break;
            }
        }
        eprintln!("backfill 20k messages: {:?} in {batches} batches of 500, slowest batch {worst:?}", t.elapsed());
        let t = std::time::Instant::now();
        let g = search(&s.conn, "gateway", &Filter::default()).unwrap();
        eprintln!("search 'gateway': {} workspaces, {:?}", g.len(), t.elapsed());
        let t = std::time::Instant::now();
        let g2 = search(&s.conn, "go test ./internal/cache", &Filter { kind: Some(COMMAND.into()), ..Default::default() }).unwrap();
        eprintln!("search commands 'go test ./internal/cache': {} workspaces, {:?}", g2.len(), t.elapsed());
        assert!(!g.is_empty() && !g2.is_empty());
        assert!(progress(&s.conn).unwrap().finished);
    }

    /// Times the backfill on a real database. Point NEKOTRAIL_SEARCH_DB at a COPY (it is written to):
    /// `NEKOTRAIL_SEARCH_DB=/tmp/copy.db cargo test --lib real_store -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_store() {
        let Ok(path) = std::env::var("NEKOTRAIL_SEARCH_DB") else { return };
        let t = std::time::Instant::now();
        let s = Store::open(std::path::Path::new(&path)).unwrap();
        eprintln!("open (migrate, create index) {:?}", t.elapsed());
        let p = progress(&s.conn).unwrap();
        eprintln!("to backfill: {} messages", p.total);
        let t = std::time::Instant::now();
        let (mut batches, mut worst) = (0, std::time::Duration::ZERO);
        loop {
            let b = std::time::Instant::now();
            let (_, done) = backfill_step(&s.conn, 500).unwrap();
            worst = worst.max(b.elapsed());
            batches += 1;
            if done {
                break;
            }
        }
        eprintln!("backfill: {:?} in {batches} batches, slowest {worst:?}", t.elapsed());
        let rows: i64 = s.conn.query_row("SELECT COUNT(*) FROM search_fts", [], |r| r.get(0)).unwrap();
        eprintln!("indexed rows: {rows}");
        for q in ["test", "fix the", "src/components", "cargo clippy", "e"] {
            let t = std::time::Instant::now();
            let g = search(&s.conn, q, &Filter::default()).unwrap();
            eprintln!("search {q:?}: {} workspaces, {} hits shown, {:?}", g.len(), g.iter().map(|g| g.hits.len()).sum::<usize>(), t.elapsed());
        }
    }
}
