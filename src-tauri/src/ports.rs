//! Ports for each workspace, so five workspaces can run the same app without fighting over 3000.
//!
//! Every active workspace holds a block of 10 ports starting at `4100 + 10 * slot`. The block is
//! saved in the database, so it stays the same across restarts, and given back when the workspace
//! is archived. A block with anything already listening on one of its ports is skipped.
//!
//! Anything running in a workspace (terminals, setup and run scripts, agent turns) gets
//! `NEKOTRAIL_PORT` (the first port) and `NEKOTRAIL_PORT_1` to `_9`. It also gets `PORT`, unless
//! you set your own: one exported in your shell or added in Repo settings is never replaced.
//! A repo's `nekotrail.json` can name ports, `"ports": {"web": "PORT", "api": "API_PORT"}`: web
//! gets the first port as `PORT`, api the second as `API_PORT`, in the order written.
//!
//! The Ports popover asks `lsof` what listens (your own processes only), and "Free this port"
//! only ever stops a process that this workspace's own terminal, script or agent started: a
//! descendant of one of its processes, checked against `ps` right before each signal.

use crate::store::Store;
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// First port of the first block.
pub const FIRST: u16 = 4100;
/// Ports per workspace.
pub const SIZE: u16 = 10;
/// First port of the last block. Its block ends at 49149, below the range macOS hands out to
/// programs that ask for any free port (49152 and up), so those never land in a block.
pub const LAST: u16 = 49140;

/// The first free block: not held by another workspace, and nothing listening on any of its ports.
pub fn pick(taken: &[u16], in_use: impl Fn(u16) -> bool) -> Option<u16> {
    (FIRST..=LAST).step_by(SIZE as usize).find(|&b| !taken.contains(&b) && !(0..SIZE).any(|i| in_use(b + i)))
}

/// Something listens on this port on this Mac: it answers on 127.0.0.1 or ::1, or the port
/// can't be bound.
pub fn in_use(port: u16) -> bool {
    let local = [SocketAddr::from((Ipv4Addr::LOCALHOST, port)), SocketAddr::from((Ipv6Addr::LOCALHOST, port))];
    if local.iter().any(|a| TcpStream::connect_timeout(a, Duration::from_millis(150)).is_ok()) {
        return true;
    }
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_err()
}

/// The workspace's block, handing one out the first time. Only active workspaces hold one: an
/// archived workspace gets None and keeps nothing.
pub fn ensure(db: &Store, workspace_id: &str, in_use: impl Fn(u16) -> bool) -> Result<Option<u16>> {
    if db.workspace(workspace_id)?.state != "active" {
        return Ok(None);
    }
    if let Some(b) = db.port_block(workspace_id)? {
        return Ok(Some(b));
    }
    let mut taken: Vec<u16> = db.port_blocks()?.into_iter().map(|(_, b)| b).collect();
    // `claim` can only lose a race to another handle on the same database; try the next block then.
    for _ in 0..8 {
        let base = pick(&taken, &in_use).ok_or_else(|| anyhow!("Every port block is taken. Archive a workspace to free one."))?;
        if db.claim_port_block(workspace_id, base)? {
            return Ok(Some(base));
        }
        taken.push(base);
    }
    Err(anyhow!("Couldn't hand out ports for this workspace. Try again."))
}

/// Where `PORT` points: the port the repo named `PORT`, or the first one. The Preview tab
/// opens this one.
pub fn main_port(base: u16, names: &[(String, String)]) -> u16 {
    base + names.iter().position(|(_, v)| v == "PORT").unwrap_or(0) as u16
}

/// The variables for a workspace whose block starts at `base`, with the repo's port names.
/// `user_has(var)` is true for a variable you set yourself (exported in your shell, or in Repo
/// settings): those are left alone, so `PORT` and the named ones never replace yours.
pub fn env(base: u16, names: &[(String, String)], user_has: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    let mut out = vec![("NEKOTRAIL_PORT".to_string(), base.to_string())];
    for i in 1..SIZE {
        out.push((format!("NEKOTRAIL_PORT_{i}"), (base + i).to_string()));
    }
    for (i, (_, var)) in names.iter().take(SIZE as usize).enumerate() {
        if !user_has(var) {
            out.push((var.clone(), (base + i as u16).to_string()));
        }
    }
    if !names.iter().any(|(_, v)| v == "PORT") && !user_has("PORT") {
        out.push(("PORT".into(), base.to_string()));
    }
    out
}

// ---------- who listens ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listener {
    pub pid: u32,
    pub command: String,
    pub port: u16,
}

/// Read `lsof -F pcn` output: `p<pid>`, `c<command>`, then `n<address>:<port>` per socket.
/// One entry per process and port (a server on both 127.0.0.1 and ::1 is one).
pub fn parse_lsof(text: &str) -> Vec<Listener> {
    let mut out: Vec<Listener> = vec![];
    let (mut pid, mut command) = (0u32, String::new());
    for line in text.lines() {
        let (tag, rest) = line.split_at(line.len().min(1));
        match tag {
            "p" => {
                pid = rest.parse().unwrap_or(0);
                command.clear();
            }
            "c" => command = rest.to_string(),
            "n" => {
                // "*:3000", "127.0.0.1:3000", "[::1]:5173". Established sockets ("a->b") are skipped.
                if rest.contains("->") {
                    continue;
                }
                let Some(port) = rest.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()) else { continue };
                if pid > 1 && !out.iter().any(|l| l.pid == pid && l.port == port) {
                    out.push(Listener { pid, command: command.clone(), port });
                }
            }
            _ => {}
        }
    }
    out.sort_by_key(|l| (l.port, l.pid));
    out
}

fn uid() -> &'static str {
    static UID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    UID.get_or_init(|| {
        Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or_else(|| "-1".into())
    })
}

/// Every TCP port your own processes listen on.
pub fn listeners() -> Vec<Listener> {
    // -a: all conditions at once; -u: only your processes; -nP: numbers, no name lookups (fast).
    let out = Command::new("lsof").args(["-nP", "-a", "-iTCP", "-sTCP:LISTEN", "-u", uid(), "-F", "pcn"]).output();
    // lsof exits 1 when nothing matches; the output is still right.
    out.map(|o| parse_lsof(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

/// The working folder of each of these processes (yours only), from `lsof -d cwd`.
pub fn cwds(pids: &[u32]) -> HashMap<u32, String> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let list = pids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let Ok(o) = Command::new("lsof").args(["-a", "-d", "cwd", "-u", uid(), "-p", &list, "-F", "n"]).output() else { return HashMap::new() };
    let mut out = HashMap::new();
    let mut pid = 0u32;
    for line in String::from_utf8_lossy(&o.stdout).lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().unwrap_or(0);
        } else if let Some(n) = line.strip_prefix('n')
            && pid > 1 {
                out.insert(pid, n.to_string());
            }
    }
    out
}

/// `ps -o pid=,ppid=` output as child to parent.
pub fn parse_ps(text: &str) -> HashMap<u32, u32> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
        })
        .collect()
}

/// Every process's parent, right now.
pub fn parents() -> HashMap<u32, u32> {
    Command::new("ps").args(["-axo", "pid=,ppid="]).output().map(|o| parse_ps(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

/// `pid` is one of `roots` or descends from one. Roots are the processes a workspace started
/// (its terminals and agent turns); pid 1 and below never count, so an orphan never matches.
pub fn in_tree(pid: u32, roots: &[u32], parents: &HashMap<u32, u32>) -> bool {
    let mut cur = pid;
    for _ in 0..512 {
        if cur <= 1 {
            return false;
        }
        if roots.contains(&cur) {
            return true;
        }
        match parents.get(&cur) {
            Some(&p) if p != cur => cur = p,
            _ => return false,
        }
    }
    false
}

/// `path` is the workspace folder or inside it.
pub fn inside(path: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches('/');
    !folder.is_empty() && (path == folder || path.strip_prefix(folder).is_some_and(|r| r.starts_with('/')))
}

/// Stop the processes listening on `port` that are in this workspace's tree: TERM, then KILL
/// for any still listening after 3 s. `roots` is read again before each signal, and the tree
/// is checked again too, so a process that ended (and whose pid came back as something else)
/// is never signalled. Returns how many it stopped.
pub fn free(port: u16, roots: impl Fn() -> Vec<u32>) -> Result<usize> {
    let me = std::process::id();
    let owned = |ls: &[Listener]| -> Vec<u32> {
        let (r, p) = (roots(), parents());
        let mut pids: Vec<u32> = ls.iter().filter(|l| l.port == port && l.pid != me && in_tree(l.pid, &r, &p)).map(|l| l.pid).collect();
        pids.dedup();
        pids
    };
    let now = listeners();
    let first = owned(&now);
    if first.is_empty() {
        return Err(anyhow!(if now.iter().any(|l| l.port == port) {
            "This workspace didn't start what is listening on that port, so NekoTrail won't stop it. Quit it where it runs."
        } else {
            "Nothing is listening on that port any more."
        }));
    }
    for pid in &first {
        let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
    }
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        if owned(&listeners()).is_empty() {
            return Ok(first.len());
        }
    }
    for pid in owned(&listeners()) {
        let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
    }
    Ok(first.len())
}

// ---------- spotting a server that ignored PORT ----------

/// The port a "port already taken" error names, from a dev server's output. `Some(None)` when the
/// error doesn't say which port (Python's "[Errno 48] Address already in use"). None when the text
/// has no such error. "Port 3000 is in use, trying 3001 instead" is not an error: the server moved.
pub fn in_use_error(text: &str) -> Option<Option<u16>> {
    let clean = strip_ansi(text);
    let line = clean.lines().find(|l| {
        let l = l.to_ascii_lowercase();
        l.contains("eaddrinuse") || l.contains("address already in use") || l.contains("port is already in use") || l.contains("port is already allocated")
    })?;
    Some(port_in(line))
}

/// The last port-looking number in a line: after a ':' (`:::3000`, `127.0.0.1:3000`) or the word
/// "port" (`port 3000`, `"127.0.0.1" port 3000`).
fn port_in(line: &str) -> Option<u16> {
    let b = line.as_bytes();
    let lower = line.to_ascii_lowercase();
    let mut found = None;
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_digit()) {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let before = lower[..start].trim_end_matches([' ', '"', '\'']);
            let after_ok = i == b.len() || !b[i].is_ascii_alphanumeric() && b[i] != b'.';
            let marked = before.ends_with(':') || before.ends_with("port");
            if marked && after_ok
                && let Ok(p) = line[start..i].parse::<u16>()
                    && p > 0 {
                        found = Some(p);
                    }
        } else {
            i += 1;
        }
    }
    found
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// What the Run script really starts: `npm run dev`, `bun dev`, `pnpm dev` or `yarn dev` are looked
/// up in the workspace's package.json, so the fix can name the actual dev server.
pub fn resolve_run(run: &str, dir: &Path) -> String {
    let words: Vec<&str> = run.split_whitespace().collect();
    let name = match words.as_slice() {
        [pm, "run", n, ..] if matches!(*pm, "npm" | "pnpm" | "yarn" | "bun") => Some(*n),
        ["npm", "start", ..] => Some("start"),
        [pm, n, ..] if matches!(*pm, "pnpm" | "yarn" | "bun") && !matches!(*n, "run" | "x" | "install" | "add" | "exec") && !n.contains('.') => Some(*n),
        _ => None,
    };
    let script = name.and_then(|n| {
        let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        v["scripts"][n].as_str().map(str::to_string)
    });
    match script {
        Some(s) => format!("{run} ({s})"),
        None => run.to_string(),
    }
}

/// One plain sentence on how to make this dev server use the workspace's port.
pub fn fix_hint(run: &str, var: &str) -> String {
    let c = run.to_ascii_lowercase();
    let words: Vec<&str> = c.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':'))).collect();
    let has = |w: &str| words.contains(&w);
    let start = |cmd: String| format!("In the Run script, start it with `{cmd}`.");
    if has("next") {
        start(format!("next dev -p ${var}"))
    } else if has("nuxt") || has("nuxi") {
        start(format!("nuxt dev --port ${var}"))
    } else if has("astro") {
        start(format!("astro dev --port ${var}"))
    } else if has("vite") || has("svelte-kit") || has("remix") {
        start(format!("vite --port ${var} --strictPort"))
    } else if has("ng") {
        start(format!("ng serve --port ${var}"))
    } else if has("rails") {
        start(format!("rails server -p ${var}"))
    } else if has("manage.py") {
        start(format!("python manage.py runserver ${var}"))
    } else if has("uvicorn") || has("flask") || has("webpack") || has("webpack-dev-server") || has("hugo") {
        format!("In the Run script, add `--port ${var}` to the command that starts it.")
    } else {
        format!(
            "Make your dev server listen on ${var} (most take `--port ${var}`). If it reads its own variable, name it in nekotrail.json: \"ports\": {{\"web\": \"ITS_VARIABLE\"}}."
        )
    }
}

// ---------- the Ports popover ----------

/// A workspace as the report sees it.
pub struct WsInfo {
    pub id: String,
    /// Its name as the sidebar shows it, for "belongs to <name>".
    pub label: String,
    pub base: u16,
    pub names: Vec<(String, String)>,
    /// The port variables nekotrail exports for it (`env`), so the rows show exactly those.
    pub env: Vec<(String, String)>,
    /// Its Run script, resolved through package.json, for the fix.
    pub run: String,
    /// The Run script stopped on "address already in use": Some(port), or Some(None) when it
    /// didn't say which port.
    pub failed: Option<Option<u16>>,
}

impl WsInfo {
    fn holds(&self, port: u16) -> bool {
        (self.base..self.base + SIZE).contains(&port)
    }
    /// The variable the fix tells you to use: PORT when nekotrail set it, else NEKOTRAIL_PORT.
    fn var(&self) -> &str {
        if self.env.iter().any(|(k, _)| k == "PORT") { "PORT" } else { "NEKOTRAIL_PORT" }
    }
    fn main(&self) -> u16 {
        main_port(self.base, &self.names)
    }
}

/// Who started a listening process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum How {
    /// A descendant of one of the workspace's terminals, scripts or agent turns.
    Tree,
    /// Running in the workspace's folder, but its parent is gone (nekotrail can't vouch for it).
    Folder,
}

#[derive(Debug, Serialize)]
pub struct Holder {
    pub pid: u32,
    pub command: String,
    /// The workspace that started it, if any.
    pub workspace_id: Option<String>,
    pub how: Option<How>,
    /// It belongs to this workspace's process tree, so "Free this port" may stop it.
    pub can_free: bool,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub port: u16,
    /// The variables that hold this port ("NEKOTRAIL_PORT", "PORT").
    pub vars: Vec<String>,
    /// Its name from nekotrail.json ("web"), if any.
    pub name: Option<String>,
    /// Outside this workspace's block: something it started listens where it shouldn't.
    pub outside: bool,
    pub holder: Option<Holder>,
}

#[derive(Debug, Serialize)]
pub struct Problem {
    pub port: Option<u16>,
    pub text: String,
    pub fix: String,
}

#[derive(Debug, Serialize)]
pub struct WsPorts {
    pub id: String,
    pub base: u16,
    /// The port the Preview tab opens.
    pub main: u16,
    pub rows: Vec<Row>,
    pub problems: Vec<Problem>,
}

/// Ports at or above this are the ones macOS hands out to "any free port" requests (agent CLIs,
/// language servers, debuggers). Those never mean a dev server ignored PORT.
const ANY_PORT: u16 = 49152;

/// The popover's data for each workspace in `ws` (or just `only`). `owner(pid)` says which
/// workspace started a process, and how.
pub fn report(ws: &[WsInfo], listeners: &[Listener], owner: impl Fn(u32) -> Option<(String, How)>, only: Option<&str>) -> Vec<WsPorts> {
    let owners: HashMap<u32, Option<(String, How)>> = listeners.iter().map(|l| (l.pid, owner(l.pid))).collect();
    let label = |id: &str| ws.iter().find(|w| w.id == id).map(|w| w.label.clone()).unwrap_or_else(|| "another workspace".into());
    let holder_for = |w: &WsInfo, l: &Listener| {
        let o = owners.get(&l.pid).cloned().flatten();
        Holder {
            pid: l.pid,
            command: l.command.clone(),
            can_free: o.as_ref().is_some_and(|(id, how)| id == &w.id && *how == How::Tree),
            workspace_id: o.as_ref().map(|(id, _)| id.clone()),
            how: o.map(|(_, how)| how),
        }
    };
    // Prefer the process this workspace started when several share a port (a parent and its worker).
    let at = |w: &WsInfo, port: u16| -> Option<&Listener> {
        let all: Vec<&Listener> = listeners.iter().filter(|l| l.port == port).collect();
        all.iter().find(|l| owners.get(&l.pid).cloned().flatten().is_some_and(|(id, _)| id == w.id)).or(all.first()).copied()
    };
    let mut out = vec![];
    for w in ws.iter().filter(|w| only.is_none_or(|o| o == w.id)) {
        let mut rows = vec![];
        for i in 0..SIZE {
            let port = w.base + i;
            let p = port.to_string();
            let vars: Vec<String> = w.env.iter().filter(|(_, v)| *v == p).map(|(k, _)| k.clone()).collect();
            let named = w.names.get(i as usize);
            let l = at(w, port);
            if i == 0 || named.is_some() || l.is_some() {
                rows.push(Row { port, vars, name: named.map(|(n, _)| n.clone()), outside: false, holder: l.map(|l| holder_for(w, l)) });
            }
        }
        let mut problems = vec![];
        let fix = fix_hint(&w.run, w.var());
        // Something this workspace started listens outside its block: it ignored PORT.
        for l in listeners.iter().filter(|l| !w.holds(l.port) && l.port < ANY_PORT) {
            let Some((id, _)) = owners.get(&l.pid).cloned().flatten() else { continue };
            if id != w.id || rows.iter().any(|r| r.port == l.port) {
                continue;
            }
            rows.push(Row { port: l.port, vars: vec![], name: None, outside: true, holder: Some(holder_for(w, l)) });
            let text = match ws.iter().find(|o| o.id != w.id && o.holds(l.port)) {
                Some(o) => format!(
                    "{} is listening on {}, a port that belongs to {}. It ignored this workspace's port, {}.",
                    l.command, l.port, o.label, w.main()
                ),
                None => format!(
                    "{} is listening on {}, not on this workspace's port {}. It ignored PORT, so the same app in another workspace will clash on {}.",
                    l.command, l.port, w.main(), l.port
                ),
            };
            problems.push(Problem { port: Some(l.port), text, fix: fix.clone() });
        }
        // The Run script stopped because its port was taken.
        if let Some(failed) = w.failed {
            let text = match failed {
                Some(p) if w.holds(p) => match listeners.iter().find(|l| l.port == p) {
                    Some(l) => format!("Your app couldn't start: {} (pid {}) already holds {p}, one of this workspace's ports.", l.command, l.pid),
                    None => format!("Your app couldn't start: {p} was taken when it tried. It's free now, so try Run again."),
                },
                Some(p) => {
                    let who = match listeners.iter().find(|l| l.port == p) {
                        Some(l) => match owners.get(&l.pid).cloned().flatten() {
                            Some((id, _)) if id == w.id => format!("{} from this workspace", l.command),
                            Some((id, _)) => format!("{} in {}", l.command, label(&id)),
                            None => format!("{} (pid {}), which NekoTrail didn't start", l.command, l.pid),
                        },
                        None => "another app".into(),
                    };
                    format!("Your app couldn't start: {p} is taken by {who}. It ignored this workspace's port, {}, and tried its default.", w.main())
                }
                None => format!("Your app couldn't start because its port was taken. This workspace's port is {}.", w.main()),
            };
            problems.push(Problem { port: failed, text, fix: fix.clone() });
        }
        rows.sort_by_key(|r| (r.outside, r.port));
        out.push(WsPorts { id: w.id.clone(), base: w.base, main: w.main(), rows, problems });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Repo, Workspace};

    fn repo() -> Repo {
        Repo {
            id: "r".into(), name: "demo".into(), root_path: "/code/demo".into(), default_branch: "main".into(),
            remote_url: None, setup_script: None, run_script: None, archive_script: None, prefs: Default::default(),
        }
    }

    fn ws(id: &str) -> Workspace {
        Workspace {
            id: id.into(), repo_id: "r".into(), name: id.into(), branch: format!("b/{id}"), path: format!("/w/{id}"),
            target_branch: "main".into(), state: "active".into(), pinned: false, unread: false, notes: String::new(),
            imported_from: None, created_at: String::new(), title: String::new(), status: String::new(), pair_id: String::new(),
            section_id: String::new(), parent_id: String::new(), title_source: String::new(), review_pr: 0, group_id: String::new(),
        }
    }

    fn names(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn hands_out_blocks_in_order_and_skips_ports_in_use() {
        let free = |_: u16| false;
        assert_eq!(pick(&[], free), Some(4100));
        assert_eq!(pick(&[4100, 4110], free), Some(4120));
        assert_eq!(pick(&[4110], free), Some(4100), "a gap left by an archived workspace is reused");
        // Something already on 4105 (inside the first block) or on 4110: both blocks are skipped.
        assert_eq!(pick(&[], |p| p == 4105 || p == 4110), Some(4120));
        // The last block stays below the ports macOS gives out for "any free port".
        let all: Vec<u16> = (FIRST..=LAST).step_by(SIZE as usize).collect();
        assert_eq!(*all.last().unwrap() + SIZE - 1, 49149);
        assert_eq!(pick(&all, free), None);
    }

    #[test]
    fn in_use_sees_a_real_listener() {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(in_use(port));
        drop(l);
    }

    #[test]
    fn blocks_persist_are_unique_and_are_released_on_archive() {
        let db = Store::in_memory().unwrap();
        db.insert_repo(&repo()).unwrap();
        for id in ["a", "b", "c"] {
            db.insert_workspace(&ws(id)).unwrap();
        }
        let free = |_: u16| false;
        let a = ensure(&db, "a", free).unwrap().unwrap();
        let b = ensure(&db, "b", free).unwrap().unwrap();
        assert_eq!((a, b), (4100, 4110));
        // Stable: asking again (or after a restart, same database) gives the same block, even if
        // something now listens on it (that's this workspace's own server).
        assert_eq!(ensure(&db, "a", |_| true).unwrap(), Some(4100));
        // Skips ports in use when handing a new block out.
        assert_eq!(ensure(&db, "c", |p| (4120..4130).contains(&p)).unwrap(), Some(4130));
        // The database refuses a second holder for a block.
        assert!(!db.claim_port_block("c", 4100).unwrap());

        // Archiving gives the block back; the next workspace gets it.
        db.set_workspace_state("b", "archived").unwrap();
        assert_eq!(db.port_block("b").unwrap(), None);
        assert_eq!(ensure(&db, "b", free).unwrap(), None, "an archived workspace holds no ports");
        db.insert_workspace(&ws("d")).unwrap();
        assert_eq!(ensure(&db, "d", free).unwrap(), Some(4110));
        // Restored, it gets a block again (the next free one).
        db.set_workspace_state("b", "active").unwrap();
        assert_eq!(ensure(&db, "b", free).unwrap(), Some(4120));
    }

    #[test]
    fn blocks_survive_reopening_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let db = Store::open(&path).unwrap();
            db.insert_repo(&repo()).unwrap();
            db.insert_workspace(&ws("a")).unwrap();
            db.insert_workspace(&ws("b")).unwrap();
            assert_eq!(ensure(&db, "a", |_| false).unwrap(), Some(4100));
            assert_eq!(ensure(&db, "b", |_| false).unwrap(), Some(4110));
        }
        let db = Store::open(&path).unwrap();
        assert_eq!(ensure(&db, "b", |_| false).unwrap(), Some(4110));
        assert_eq!(db.port_blocks().unwrap(), vec![("a".to_string(), 4100), ("b".to_string(), 4110)]);
    }

    #[test]
    fn env_sets_port_unless_you_set_it_and_follows_the_repo_names() {
        let get = |env: &[(String, String)], k: &str| env.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
        // Nothing named, nothing of yours: PORT is the first port.
        let e = env(4110, &[], |_| false);
        assert_eq!(get(&e, "NEKOTRAIL_PORT").as_deref(), Some("4110"));
        assert_eq!(get(&e, "NEKOTRAIL_PORT_1").as_deref(), Some("4111"));
        assert_eq!(get(&e, "NEKOTRAIL_PORT_9").as_deref(), Some("4119"));
        assert_eq!(get(&e, "NEKOTRAIL_PORT_10"), None);
        assert_eq!(get(&e, "PORT").as_deref(), Some("4110"));
        // You exported PORT (shell or Repo settings): left alone.
        let e = env(4110, &[], |k| k == "PORT");
        assert_eq!(get(&e, "PORT"), None);
        assert_eq!(get(&e, "NEKOTRAIL_PORT").as_deref(), Some("4110"));
        // The repo names its ports in order; PORT goes where the repo put it.
        let n = names(&[("api", "API_PORT"), ("web", "PORT")]);
        let e = env(4110, &n, |_| false);
        assert_eq!(get(&e, "API_PORT").as_deref(), Some("4110"));
        assert_eq!(get(&e, "PORT").as_deref(), Some("4111"));
        assert_eq!(e.iter().filter(|(k, _)| k == "PORT").count(), 1);
        assert_eq!(main_port(4110, &n), 4111);
        // Even a variable the repo names never replaces one you set.
        let e = env(4110, &n, |k| k == "PORT" || k == "API_PORT");
        assert_eq!((get(&e, "PORT"), get(&e, "API_PORT")), (None, None));
        assert_eq!(main_port(4110, &[]), 4110);
    }

    #[test]
    fn reads_lsof_and_ps() {
        let lsof = "p501\ncnode\nf23\nn*:3000\nf24\nn[::1]:3000\np777\ncvite\nf30\nn127.0.0.1:4110\nf31\nn127.0.0.1:4110->127.0.0.1:55000\np1\nclaunchd\nn*:22\n";
        assert_eq!(
            parse_lsof(lsof),
            vec![
                Listener { pid: 501, command: "node".into(), port: 3000 },
                Listener { pid: 777, command: "vite".into(), port: 4110 },
            ]
        );
        let ps = "  1     0\n 400     1\n 450   400\n 501   450\n 900     1\nbad line\n";
        let parents = parse_ps(ps);
        assert_eq!(parents.get(&501), Some(&450));
        assert_eq!(parents.len(), 5);
    }

    #[test]
    fn only_the_workspaces_own_process_tree_counts() {
        // 400 is the workspace's terminal; 450 its shell's child; 501 the server that child started.
        let parents = parse_ps("1 0\n400 1\n450 400\n501 450\n900 1\n901 900\n");
        assert!(in_tree(501, &[400], &parents));
        assert!(in_tree(400, &[400], &parents), "the root itself counts");
        assert!(!in_tree(901, &[400], &parents), "another tree");
        assert!(!in_tree(900, &[400], &parents));
        assert!(!in_tree(501, &[], &parents), "no roots, nothing is ours");
        assert!(!in_tree(501, &[1], &parents), "launchd adopts orphans; it is never a root");
        assert!(!in_tree(12345, &[400], &parents), "unknown pid");
        // A loop in a broken table ends.
        assert!(!in_tree(7, &[400], &parse_ps("7 8\n8 7\n")));
        assert!(inside("/w/a/apps/web", "/w/a"));
        assert!(inside("/w/a", "/w/a/"));
        assert!(!inside("/w/ab", "/w/a"));
        assert!(!inside("/w/a", ""));
    }

    #[test]
    fn real_process_tree_and_free_refuses_strangers() {
        // A child of this test process is in its tree; this process isn't in the child's.
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let p = parents();
        let me = std::process::id();
        assert!(in_tree(child.id(), &[me], &p));
        assert!(!in_tree(me, &[child.id()], &p));

        // A listener this test owns, freed for a workspace whose tree doesn't include it.
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let err = free(port, || vec![child.id()]).unwrap_err().to_string();
        assert!(err.contains("didn't start"), "{err}");
        assert!(in_use(port), "the stranger keeps running");
        // nekotrail itself is never a target, even when its pid is passed as a root.
        let err = free(port, || vec![me]).unwrap_err().to_string();
        assert!(err.contains("didn't start"), "{err}");
        drop(l);
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn free_stops_a_server_in_the_tree() {
        // A shell (the "terminal") that starts a listening Python server as its child.
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let script = format!("python3 -c 'import socket,time; s=socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind((\"127.0.0.1\", {port})); s.listen(); time.sleep(60)' & wait");
        let Ok(mut shell) = Command::new("/bin/sh").args(["-c", &script]).spawn() else { return };
        let root = shell.id();
        let mut up = false;
        for _ in 0..50 {
            if listeners().iter().any(|l| l.port == port) {
                up = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !up {
            // No python3 on this machine: nothing to check.
            let _ = shell.kill();
            return;
        }
        assert_eq!(free(port, || vec![root]).unwrap(), 1);
        assert!(!listeners().iter().any(|l| l.port == port));
        let _ = shell.wait();
    }

    #[test]
    fn spots_port_taken_errors() {
        assert_eq!(in_use_error("Error: listen EADDRINUSE: address already in use :::3000\n"), Some(Some(3000)));
        assert_eq!(in_use_error("\x1b[31mError: listen EADDRINUSE: address already in use 127.0.0.1:5173\x1b[0m"), Some(Some(5173)));
        assert_eq!(in_use_error("Address already in use - bind(2) for \"127.0.0.1\" port 3000 (Errno::EADDRINUSE)"), Some(Some(3000)));
        assert_eq!(in_use_error("listen tcp :8080: bind: address already in use"), Some(Some(8080)));
        assert_eq!(in_use_error("OSError: [Errno 48] Address already in use"), Some(None));
        assert_eq!(in_use_error("Error: That port is already in use."), Some(None));
        // A server that moved to another port did start: not an error.
        assert_eq!(in_use_error("Port 5173 is in use, trying another one..."), None);
        assert_eq!(in_use_error("  ready in 300 ms\n  Local: http://localhost:4110/"), None);
    }

    #[test]
    fn hints_name_the_real_dev_server() {
        assert!(fix_hint("bun dev (vite)", "PORT").contains("vite --port $PORT --strictPort"));
        assert!(fix_hint("npm run dev (next dev)", "PORT").contains("next dev -p $PORT"));
        assert!(fix_hint("bin/rails server", "NEKOTRAIL_PORT").contains("rails server -p $NEKOTRAIL_PORT"));
        assert!(fix_hint("python manage.py runserver", "PORT").contains("runserver $PORT"));
        assert!(fix_hint("make dev", "PORT").contains("nekotrail.json"));
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("package.json"), r#"{"scripts":{"dev":"vite","start":"next start"}}"#).unwrap();
        assert_eq!(resolve_run("bun dev", d.path()), "bun dev (vite)");
        assert_eq!(resolve_run("npm run dev -- --host", d.path()), "npm run dev -- --host (vite)");
        assert_eq!(resolve_run("npm start", d.path()), "npm start (next start)");
        assert_eq!(resolve_run("bun run.ts", d.path()), "bun run.ts");
        assert_eq!(resolve_run("bun install", d.path()), "bun install");
    }

    fn info(id: &str, base: u16) -> WsInfo {
        WsInfo { id: id.into(), label: format!("Task {id}"), base, names: vec![], env: env(base, &[], |_| false), run: "bun dev (vite)".into(), failed: None }
    }

    #[test]
    fn report_lists_ports_and_who_holds_them() {
        let mut a = info("a", 4100);
        a.names = names(&[("web", "PORT"), ("api", "API_PORT")]);
        a.env = env(4100, &a.names, |_| false);
        let ls = vec![
            Listener { pid: 10, command: "vite".into(), port: 4100 },
            Listener { pid: 11, command: "node".into(), port: 4105 },
            Listener { pid: 12, command: "postgres".into(), port: 5432 },
        ];
        let owner = |pid: u32| match pid {
            10 => Some(("a".to_string(), How::Tree)),
            11 => Some(("a".to_string(), How::Folder)),
            _ => None,
        };
        let r = report(&[a, info("b", 4110)], &ls, owner, Some("a"));
        assert_eq!(r.len(), 1);
        let a = &r[0];
        assert_eq!((a.base, a.main), (4100, 4100));
        let ports: Vec<u16> = a.rows.iter().map(|r| r.port).collect();
        assert_eq!(ports, [4100, 4101, 4105], "named ports, the first port, and anything listening in the block");
        assert_eq!(a.rows[0].vars, ["NEKOTRAIL_PORT", "PORT"]);
        assert_eq!(a.rows[0].name.as_deref(), Some("web"));
        assert_eq!(a.rows[1].vars, ["NEKOTRAIL_PORT_1", "API_PORT"]);
        assert!(a.rows[1].holder.is_none());
        let h = a.rows[0].holder.as_ref().unwrap();
        assert!(h.can_free && h.how == Some(How::Tree));
        let h = a.rows[2].holder.as_ref().unwrap();
        assert!(!h.can_free, "started in the folder but not in the tree: never freed");
        assert!(a.problems.is_empty(), "postgres on 5432 isn't this workspace's");
    }

    #[test]
    fn report_explains_a_server_that_ignored_port() {
        let a = info("a", 4100);
        let b = info("b", 4110);
        // a's vite ignored PORT and took 3000; b's server landed on 4111 (a's? no: b's own block).
        let ls = vec![
            Listener { pid: 20, command: "node".into(), port: 3000 },
            Listener { pid: 21, command: "node".into(), port: 4115 },
            Listener { pid: 22, command: "claude".into(), port: 52000 },
        ];
        let owner = |pid: u32| match pid {
            20 | 22 => Some(("a".to_string(), How::Tree)),
            21 => Some(("b".to_string(), How::Tree)),
            _ => None,
        };
        let r = report(&[a, b], &ls, owner, None);
        let a = &r[0];
        assert_eq!(a.problems.len(), 1, "an agent's own high port is not a dev server");
        assert!(a.problems[0].text.contains("listening on 3000, not on this workspace's port 4100"), "{}", a.problems[0].text);
        assert!(a.problems[0].fix.contains("vite --port $PORT"));
        let outside = a.rows.iter().find(|r| r.outside).unwrap();
        assert_eq!(outside.port, 3000);
        assert!(r[1].problems.is_empty());

        // a's server took a port in b's block.
        let ls = vec![Listener { pid: 20, command: "node".into(), port: 4112 }];
        let r = report(&[info("a", 4100), info("b", 4110)], &ls, owner, Some("a"));
        assert!(r[0].problems[0].text.contains("belongs to Task b"), "{}", r[0].problems[0].text);

        // b's Run script failed on 3000, which a's server holds.
        let mut b = info("b", 4110);
        b.failed = Some(Some(3000));
        let ls = vec![Listener { pid: 20, command: "node".into(), port: 3000 }];
        let r = report(&[info("a", 4100), b], &ls, owner, Some("b"));
        let p = &r[0].problems[0];
        assert_eq!(p.port, Some(3000));
        assert!(p.text.contains("3000 is taken by node in Task a"), "{}", p.text);
        assert!(p.text.contains("ignored this workspace's port, 4110"), "{}", p.text);
        // You set your own PORT: the fix points at NEKOTRAIL_PORT, which is always ours.
        let mut c = info("c", 4120);
        c.env = env(4120, &[], |k| k == "PORT");
        c.failed = Some(None);
        let r = report(&[c], &[], owner, None);
        assert!(r[0].problems[0].fix.contains("$NEKOTRAIL_PORT"));
        assert!(r[0].problems[0].text.contains("port is 4120"));
    }
}
