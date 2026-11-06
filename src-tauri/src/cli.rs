//! The `nekotrail` terminal command. It is this same binary, run as `nekotrail --cli <args>` by a
//! small script that Settings, General installs. It parses the arguments, starts the app if it
//! isn't running, and asks it over the Unix socket in `remote`.

use crate::remote::{self, QueueTask, Request, Task};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const HELP: &str = "nekotrail: run coding agents in parallel, from your terminal.

Usage:
  nekotrail                      Open nekotrail
  nekotrail .                    Add this repo if it's new, and show it
  nekotrail <path>               The same, for the repo at <path>
  nekotrail new \"<task>\"         Make a workspace in this repo and send it the task
      --agent <name>             Agent to use (claude, codex, gemini, ...). Default: claude
      --base <branch>            Branch to start from. Default: the repo's default branch
      --plan                     The agent plans first and waits for your OK before editing
      --linear <ENG-123>         Start from a Linear issue: its branch, title and text (connect
                                 Linear in Settings, Integrations first). A task is then optional
  nekotrail ls [--all] [--json]  Workspaces in this repo (or every repo): state, agent, PR
  nekotrail open <name>          Show a workspace, by name, branch or title
  nekotrail status [--json]      What needs you
  nekotrail queue add \"<task>\"   Line up a task. nekotrail runs a few at a time, each in its own workspace
      --agent <name>             Agent to use. Default: claude
      --fallback <name>          Agent to use while the first is out of usage
      --repo <path>              Repo to work in. Default: the one you're in
      --base <branch>            Branch to start from
      --mode <plan|ask|edits|auto>  Default: your default mode for that agent. ask waits for you to start it
      --checks                   Then run the repo's checks
      --pr                       Then open a draft PR
      --autopilot                Then open a draft PR and let autopilot get it green
  nekotrail queue [--json]       The queue, in order
  nekotrail --help | --version

nekotrail starts if it isn't running. For a folder named new, ls, open or status, write ./new.
";

/// What the command line asked for, before any path is resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Help,
    Version,
    Show,
    Add { path: String },
    New { task: Task },
    Ls { all: bool, json: bool },
    Open { query: String },
    Status { json: bool },
    QueueAdd { repo: Option<String>, task: QueueTask },
    QueueLs { json: bool },
}

/// Parse the arguments after `nekotrail`. Pure, so every form is tested.
pub fn parse(args: &[String]) -> Result<Action, String> {
    let Some(first) = args.first() else { return Ok(Action::Show) };
    let rest = &args[1..];
    let no_more = |what: &str, allowed: &[&str]| -> Result<Vec<String>, String> {
        let mut flags = Vec::new();
        for a in rest {
            if allowed.contains(&a.as_str()) {
                flags.push(a.clone());
            } else {
                return Err(format!("nekotrail {what} doesn't take \"{a}\". Run nekotrail --help."));
            }
        }
        Ok(flags)
    };
    match first.as_str() {
        "-h" | "--help" | "help" => Ok(Action::Help),
        "-V" | "--version" | "version" => Ok(Action::Version),
        "new" => parse_new(rest),
        "ls" | "list" => {
            let f = no_more("ls", &["--all", "-a", "--json"])?;
            Ok(Action::Ls { all: f.iter().any(|x| x == "--all" || x == "-a"), json: f.iter().any(|x| x == "--json") })
        }
        "status" => {
            let f = no_more("status", &["--json"])?;
            Ok(Action::Status { json: !f.is_empty() })
        }
        "queue" => match rest.first().map(String::as_str) {
            Some("add") => parse_queue_add(&rest[1..]),
            None | Some("ls" | "list" | "--json") => {
                let flags = if rest.first().is_some_and(|r| r != "--json") { &rest[1..] } else { rest };
                match flags.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                    [] => Ok(Action::QueueLs { json: false }),
                    ["--json"] => Ok(Action::QueueLs { json: true }),
                    _ => Err("nekotrail queue takes add or --json. Run nekotrail --help.".into()),
                }
            }
            Some(other) => Err(format!("nekotrail queue doesn't know \"{other}\". Try nekotrail queue add \"<task>\".")),
        },
        "open" => {
            let words: Vec<&str> = rest.iter().map(String::as_str).filter(|w| *w != "--").collect();
            if words.is_empty() {
                return Err("Say which workspace, for example: nekotrail open mochi".into());
            }
            Ok(Action::Open { query: words.join(" ") })
        }
        f if f.starts_with('-') => Err(format!("Unknown option \"{f}\". Run nekotrail --help.")),
        path => {
            if !rest.is_empty() {
                return Err("Give one folder at a time, or quote a path with spaces. Run nekotrail --help.".into());
            }
            Ok(Action::Add { path: path.to_string() })
        }
    }
}

fn parse_new(args: &[String]) -> Result<Action, String> {
    let mut task = Task::default();
    let mut words: Vec<String> = Vec::new();
    let mut it = args.iter();
    let mut flags_done = false;
    while let Some(a) = it.next() {
        if flags_done {
            words.push(a.clone());
            continue;
        }
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            let example = match name { "--agent" => "codex", "--linear" => "ENG-123", _ => "main" };
            inline.clone().or_else(|| it.next().cloned()).filter(|v| !v.is_empty()).ok_or_else(|| format!("{name} needs a value, for example {name} {example}."))
        };
        match flag {
            "--" => flags_done = true,
            "--agent" | "-a" => task.agent = Some(value("--agent")?),
            "--base" | "-b" => task.base = Some(value("--base")?),
            "--plan" | "-p" => task.plan = true,
            "--linear" | "-l" => {
                let v = value("--linear")?;
                task.linear = Some(crate::linear::issue_ref(&v).ok_or_else(|| format!("\"{v}\" isn't a Linear issue. Use its identifier, for example --linear ENG-123."))?);
            }
            f if f.starts_with('-') && f.len() > 1 => return Err(format!("nekotrail new doesn't know \"{f}\". Run nekotrail --help.")),
            _ => words.push(a.clone()),
        }
    }
    task.prompt = words.join(" ");
    task.validate(100_000)?;
    Ok(Action::New { task })
}

/// `edits` for `acceptEdits`, and the like. None for a mode nekotrail doesn't have.
pub fn mode_name(m: &str) -> Option<&'static str> {
    match m.to_ascii_lowercase().as_str() {
        "plan" => Some("plan"),
        "ask" | "ask-before-changes" => Some("ask"),
        "edits" | "accept-edits" | "acceptedits" | "auto-accept" => Some("acceptEdits"),
        "auto" | "full-auto" | "bypasspermissions" | "yolo" => Some("bypassPermissions"),
        _ => None,
    }
}

fn parse_queue_add(args: &[String]) -> Result<Action, String> {
    let mut task = QueueTask::default();
    let mut repo = None;
    let mut words: Vec<String> = Vec::new();
    let mut it = args.iter();
    let mut flags_done = false;
    while let Some(a) = it.next() {
        if flags_done {
            words.push(a.clone());
            continue;
        }
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        let mut value = |name: &str, example: &str| -> Result<String, String> {
            inline.clone().or_else(|| it.next().cloned()).filter(|v| !v.is_empty()).ok_or_else(|| format!("{name} needs a value, for example {name} {example}."))
        };
        match flag {
            "--" => flags_done = true,
            "--agent" | "-a" => task.agent = Some(value("--agent", "codex")?),
            "--fallback" => task.fallback = Some(value("--fallback", "codex")?),
            "--base" | "-b" => task.base = Some(value("--base", "main")?),
            "--repo" | "-r" => repo = Some(value("--repo", ".")?),
            "--mode" | "-m" => {
                let m = value("--mode", "auto")?;
                task.mode = Some(mode_name(&m).ok_or_else(|| format!("\"{m}\" isn't a mode. Use plan, ask, edits or auto."))?.to_string());
            }
            "--plan" | "-p" => task.mode = Some("plan".into()),
            "--checks" => task.checks = true,
            "--pr" | "--draft-pr" => task.pr = true,
            "--autopilot" => task.autopilot = true,
            f if f.starts_with('-') && f.len() > 1 => return Err(format!("nekotrail queue add doesn't know \"{f}\". Run nekotrail --help.")),
            _ => words.push(a.clone()),
        }
    }
    task.prompt = words.join(" ");
    task.validate()?;
    Ok(Action::QueueAdd { repo, task })
}

/// `~`, `.` and relative paths, made absolute against `cwd`.
pub fn absolute(path: &str, cwd: &Path, home: Option<&Path>) -> PathBuf {
    let p = match (path.strip_prefix('~'), home) {
        (Some(rest), Some(h)) if rest.is_empty() || rest.starts_with('/') => h.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(path),
    };
    if p.is_absolute() { p } else { cwd.join(p) }
}

/// The request for an action, with paths resolved. None for actions answered here.
pub fn request(action: &Action, cwd: &Path, home: Option<&Path>) -> Option<Request> {
    let here = Some(cwd.to_string_lossy().to_string());
    Some(match action {
        Action::Help | Action::Version => return None,
        Action::Show => Request::Show,
        Action::Add { path } => Request::Add { path: absolute(path, cwd, home).to_string_lossy().to_string() },
        Action::New { task } => Request::New { cwd: cwd.to_string_lossy().to_string(), task: task.clone() },
        Action::Ls { all, .. } => Request::Ls { cwd: here, all: *all },
        Action::Open { query } => Request::Open { cwd: here, query: query.clone() },
        Action::Status { .. } => Request::Status,
        Action::QueueAdd { repo, task } => Request::QueueAdd {
            cwd: cwd.to_string_lossy().to_string(),
            repo: repo.as_deref().map(|r| absolute(r, cwd, home).to_string_lossy().to_string()),
            task: task.clone(),
        },
        Action::QueueLs { .. } => Request::QueueLs,
    })
}

// ---------- output ----------

fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}...", s.chars().take(n - 3).collect::<String>()) }
}

/// A plain table for `ls`. Includes the repo column when listing more than one repo.
pub fn format_ls(data: &Value) -> String {
    let rows = data["workspaces"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        return match data["repo"].as_str() {
            Some(r) => format!("No workspaces in {r} yet. Start one: nekotrail new \"<task>\"\n"),
            None => "No workspaces yet. Start one: nekotrail new \"<task>\"\n".into(),
        };
    }
    let with_repo = data["repo"].is_null();
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    let mut table: Vec<Vec<String>> = vec![];
    let mut head = vec![];
    if with_repo {
        head.push("REPO".to_string());
    }
    head.extend(["NAME", "STATE", "AGENT", "PR", "TITLE"].map(String::from));
    table.push(head);
    for r in &rows {
        let mut line = vec![];
        if with_repo {
            line.push(s(&r["repo"]));
        }
        let pr = r["pr"].as_str().map(String::from).unwrap_or_else(|| "-".into());
        line.extend([s(&r["name"]), s(&r["state"]), r["agent"].as_str().unwrap_or("-").to_string(), pr, cut(&s(&r["title"]), 50)]);
        table.push(line);
    }
    let cols = table[0].len();
    let widths: Vec<usize> = (0..cols).map(|c| table.iter().map(|l| l[c].chars().count()).max().unwrap_or(0)).collect();
    let mut out = String::new();
    for line in table {
        let cells: Vec<String> = line.iter().enumerate().map(|(i, c)| if i + 1 == cols { c.clone() } else { format!("{c:<w$}", w = widths[i]) }).collect();
        out.push_str(cells.join("  ").trim_end());
        out.push('\n');
    }
    out
}

/// `status`: one line per workspace that needs you, then how many agents are working.
pub fn format_status(data: &Value) -> String {
    let mut out = String::new();
    let needs = data["needs"].as_array().cloned().unwrap_or_default();
    if needs.is_empty() {
        out.push_str("Nothing needs you.\n");
    }
    for r in &needs {
        let why: Vec<&str> = r["needs"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let name = r["name"].as_str().unwrap_or("?");
        let title = r["title"].as_str().filter(|t| !t.is_empty()).map(|t| format!(" ({})", cut(t, 50))).unwrap_or_default();
        let repo = r["repo"].as_str().map(|r| format!("{r}/")).unwrap_or_default();
        out.push_str(&format!("{repo}{name}{title}: {}\n", why.join(", ")));
    }
    match data["working"].as_u64().unwrap_or(0) {
        0 => {}
        1 => out.push_str("1 agent is working.\n"),
        n => out.push_str(&format!("{n} agents are working.\n")),
    }
    out
}

/// What `queue add` says back.
pub fn format_queued(data: &Value) -> String {
    let s = |k: &str| data[k].as_str().unwrap_or("").to_string();
    let agent = crate::queue::agent_name(&s("agent"));
    if data["state"] == "waiting" {
        return format!("Added to the queue in {} for {agent}. It waits for you to start it in nekotrail, because it asks before changes.\n", s("repo"));
    }
    let when = match data["ahead"].as_u64().unwrap_or(0) {
        0 => "It starts as soon as a slot is free.".to_string(),
        1 => "1 task is ahead of it.".to_string(),
        n => format!("{n} tasks are ahead of it."),
    };
    let paused = if data["paused"] == true { " The queue is paused: resume it in nekotrail's Queue view." } else { "" };
    format!("Queued in {} for {agent}. {when}{paused}\n", s("repo"))
}

/// `queue`: one line per task, in order.
pub fn format_queue(data: &Value) -> String {
    let rows = data["tasks"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        return "The queue is empty. Add a task: nekotrail queue add \"<task>\"\n".into();
    }
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    let state = |r: &Value| match (r["state"].as_str().unwrap_or(""), r["step"].as_str().unwrap_or("")) {
        ("queued", _) => "queued",
        ("waiting", _) => "waits for you",
        ("starting", _) | ("running", "") => "running",
        ("running", _) => "setup",
        ("then", "checks") => "checks",
        ("then", "pr") => "opening PR",
        ("then", _) => "autopilot",
        ("done", _) => "done",
        ("needs_you", _) => "needs you",
        _ => "failed",
    };
    let mut table: Vec<[String; 4]> = vec![["STATE".into(), "REPO".into(), "AGENT".into(), "TASK".into()]];
    for r in &rows {
        table.push([state(r).into(), s(&r["repo"]), crate::queue::agent_name(&s(&r["agent"])).into(), cut(&s(&r["title"]), 60)]);
    }
    let w: Vec<usize> = (0..3).map(|c| table.iter().map(|l| l[c].chars().count()).max().unwrap_or(0)).collect();
    let mut out = String::new();
    for l in table {
        out.push_str(&format!("{:<a$}  {:<b$}  {:<c$}  {}\n", l[0], l[1], l[2], l[3], a = w[0], b = w[1], c = w[2]));
    }
    if data["paused"] == true {
        out.push_str("The queue is paused. Resume it in nekotrail's Queue view.\n");
    }
    out
}

fn format_done(action: &Action, data: &Value) -> String {
    let s = |k: &str| data[k].as_str().unwrap_or("").to_string();
    match action {
        Action::Add { .. } if data["added"].as_bool() == Some(true) => format!("Added {} to nekotrail.\n", s("repo")),
        Action::Add { .. } => format!("Showing {} in nekotrail.\n", s("repo")),
        Action::New { .. } if data["issue"].is_string() => format!("Started {} for {} in {} with {} (branch {}).\nIts folder: {}\n", s("workspace"), s("issue"), s("repo"), s("agent"), s("branch"), s("path")),
        Action::New { .. } => format!("Started {} in {} with {} (branch {}).\nIts folder: {}\n", s("workspace"), s("repo"), s("agent"), s("branch"), s("path")),
        Action::Open { .. } => format!("Showing {} in nekotrail.\n", s("workspace")),
        Action::Ls { json: true, .. } | Action::Status { json: true } | Action::QueueLs { json: true } => format!("{}\n", serde_json::to_string_pretty(data).unwrap_or_default()),
        Action::QueueAdd { .. } => format_queued(data),
        Action::QueueLs { .. } => format_queue(data),
        Action::Ls { .. } => format_ls(data),
        Action::Status { .. } => format_status(data),
        _ => String::new(),
    }
}

// ---------- talking to the app ----------

#[cfg(unix)]
fn connect(sock: &Path) -> std::io::Result<std::os::unix::net::UnixStream> {
    std::os::unix::net::UnixStream::connect(sock)
}

/// Start the app (the bundle this command lives in, else the installed one). `background` keeps
/// it behind your terminal, for commands that only read.
#[cfg(target_os = "macos")]
fn launch(background: bool) -> Result<(), String> {
    let exe = std::env::current_exe().ok();
    let bundle = exe.as_deref().and_then(|e| e.ancestors().find(|p| p.extension().is_some_and(|x| x == "app")).map(Path::to_path_buf));
    let mut cmd = std::process::Command::new("open");
    if background {
        cmd.arg("-g");
    }
    match bundle {
        Some(b) => cmd.arg(b),
        None => cmd.args(["-b", "dev.nekotrail.app"]),
    };
    let ok = cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
    if ok { Ok(()) } else { Err("Couldn't start nekotrail. Is it in your Applications folder?".into()) }
}

/// Start the app: this same binary, without `--cli`, detached from the terminal.
#[cfg(not(target_os = "macos"))]
fn launch(_background: bool) -> Result<(), String> {
    use std::process::Stdio;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|_| ()).map_err(|e| format!("Couldn't start nekotrail: {e}"))
}

#[cfg(unix)]
fn ask(req: &Request, background: bool) -> Result<Value, String> {
    use std::io::ErrorKind::{ConnectionRefused, NotFound};
    let data = remote::data_dir().ok_or("Can't find your Application Support folder.")?;
    let sock = remote::socket_path(&data);
    let conn = match connect(&sock) {
        Ok(c) => c,
        Err(err) if matches!(err.kind(), NotFound | ConnectionRefused) => {
            eprintln!("Starting nekotrail...");
            launch(background)?;
            let until = Instant::now() + Duration::from_secs(40);
            loop {
                std::thread::sleep(Duration::from_millis(250));
                if let Ok(c) = connect(&sock) {
                    break c;
                }
                if Instant::now() > until {
                    return Err("nekotrail started but didn't answer. Open it once from Applications, then try again.".into());
                }
            }
        }
        Err(err) => return Err(format!("Couldn't reach nekotrail: {err}")),
    };
    // Making a workspace runs git (and maybe a fetch), so give it time.
    conn.set_read_timeout(Some(Duration::from_secs(180))).map_err(|e| e.to_string())?;
    (&conn).write_all(remote::encode(req).as_bytes()).map_err(|e| format!("Couldn't reach nekotrail: {e}"))?;
    let mut line = String::new();
    BufReader::new(&conn).read_line(&mut line).map_err(|e| format!("nekotrail didn't answer: {e}"))?;
    remote::parse_answer(&line)
}

/// Run the command. Returns the exit code: 0 done, 1 nekotrail said no, 2 bad arguments.
#[cfg(unix)]
pub fn main(args: Vec<String>) -> i32 {
    let action = match parse(&args) {
        Ok(a) => a,
        Err(err) => {
            eprintln!("{err}");
            return 2;
        }
    };
    match action {
        Action::Help => {
            print!("{HELP}");
            return 0;
        }
        Action::Version => {
            println!("nekotrail {}", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        _ => {}
    }
    let Ok(cwd) = std::env::current_dir() else {
        eprintln!("This folder no longer exists.");
        return 1;
    };
    let Some(req) = request(&action, &cwd, dirs::home_dir().as_deref()) else { return 0 };
    // Reading, or lining up work for later: nekotrail stays behind your terminal.
    let background = matches!(action, Action::Ls { .. } | Action::Status { .. } | Action::QueueAdd { .. } | Action::QueueLs { .. });
    match ask(&req, background) {
        Ok(data) => {
            print!("{}", format_done(&action, &data));
            0
        }
        Err(err) => {
            eprintln!("{err}");
            1
        }
    }
}

#[cfg(not(unix))]
pub fn main(_: Vec<String>) -> i32 {
    eprintln!("The nekotrail command works on macOS only.");
    1
}

// ---------- installing ----------

/// `'...'` for sh, with `'` written as `'\''`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The script on your PATH. It runs whichever nekotrail app wrote it.
pub fn script_text(exe: &Path) -> String {
    format!(
        "#!/bin/sh\n# The nekotrail command. nekotrail writes this file (Settings, General); run nekotrail --help.\nexec {} --cli \"$@\"\n",
        sh_quote(&exe.to_string_lossy())
    )
}

/// `<app data>/bin/nekotrail`, the file the PATH link points at.
pub fn script_path(data: &Path) -> PathBuf {
    data.join("bin").join("nekotrail")
}

#[cfg(unix)]
fn write_script(path: &Path, exe: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let text = script_text(exe);
    if std::fs::read_to_string(path).ok().as_deref() != Some(text.as_str()) {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text)?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&tmp, path)?;
    }
    Ok(())
}

/// Where the link can go, in order: /usr/local/bin, then ~/.local/bin.
pub fn link_dirs() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("/usr/local/bin")];
    if let Some(h) = dirs::home_dir() {
        v.push(h.join(".local").join("bin"));
    }
    v
}

#[derive(Debug, PartialEq)]
pub enum LinkError {
    /// Something else is already called `nekotrail` here.
    Taken(PathBuf),
    /// None of the folders could be written.
    NotWritable,
}

/// A link we made earlier (it points at a nekotrail script), or a dangling one.
fn ours(link: &Path, script: &Path) -> bool {
    match std::fs::read_link(link) {
        Ok(to) => to == script || to.ends_with("dev.nekotrail.app/bin/nekotrail") || !link.exists(),
        Err(_) => false,
    }
}

/// Link `script` into the first folder that takes it. Never replaces a file that isn't ours.
#[cfg(unix)]
pub fn link_into(script: &Path, dirs: &[PathBuf]) -> Result<PathBuf, LinkError> {
    for dir in dirs {
        // ~/.local/bin may not exist yet; /usr/local/bin is never created (that needs an admin).
        if !dir.is_dir() && (dir.starts_with("/usr") || std::fs::create_dir_all(dir).is_err()) {
            continue;
        }
        let link = dir.join("nekotrail");
        if link.symlink_metadata().is_ok() {
            if !ours(&link, script) {
                return Err(LinkError::Taken(link));
            }
            if std::fs::read_link(&link).is_ok_and(|to| to == script) {
                return Ok(link);
            }
            if std::fs::remove_file(&link).is_err() {
                continue;
            }
        }
        if std::os::unix::fs::symlink(script, &link).is_ok() {
            return Ok(link);
        }
    }
    Err(LinkError::NotWritable)
}

/// The link already on your PATH folders, if there is one.
pub fn installed_link(script: &Path, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().map(|d| d.join("nekotrail")).find(|l| std::fs::read_link(l).is_ok_and(|to| to == script))
}

fn on_path(dir: &Path) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == dir))
}

/// Settings, General: "Install command line tool".
#[cfg(unix)]
pub fn install() -> Result<Value, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let data = remote::data_dir().ok_or("Can't find your Application Support folder.")?;
    let script = script_path(&data);
    write_script(&script, &exe).map_err(|e| format!("Couldn't write {}: {e}", script.display()))?;
    match link_into(&script, &link_dirs()) {
        Ok(link) => {
            let dir = link.parent().unwrap_or(Path::new("/"));
            let hint = (!on_path(dir)).then(|| format!("{} isn't on your PATH yet. Add this line to ~/.zshrc, then open a new terminal: export PATH=\"{}:$PATH\"", dir.display(), dir.display()));
            Ok(json!({"path": link, "hint": hint}))
        }
        Err(LinkError::Taken(p)) => Err(format!("{} already exists and isn't nekotrail's. Remove it, then try again.", p.display())),
        Err(LinkError::NotWritable) => Err(format!(
            "nekotrail can't write to /usr/local/bin or ~/.local/bin. Run this in Terminal instead: sudo ln -sf {} /usr/local/bin/nekotrail",
            sh_quote(&script.to_string_lossy())
        )),
    }
}

/// Where the command is installed, if it is.
pub fn status() -> Value {
    let link = remote::data_dir().and_then(|d| installed_link(&script_path(&d), &link_dirs()));
    json!({"path": link})
}

/// After an update or a move, point the installed script at this copy of the app. Only a real
/// app bundle does this, so running a dev build never takes over your command.
#[cfg(unix)]
pub fn refresh_script() {
    let (Ok(exe), Some(data)) = (std::env::current_exe(), remote::data_dir()) else { return };
    let script = script_path(&data);
    if script.exists() && exe.to_string_lossy().contains(".app/Contents/MacOS/") {
        let _ = write_script(&script, &exe);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Action, String> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn parses_every_form() {
        assert_eq!(p(&[]), Ok(Action::Show));
        assert_eq!(p(&["--help"]), Ok(Action::Help));
        assert_eq!(p(&["-h"]), Ok(Action::Help));
        assert_eq!(p(&["--version"]), Ok(Action::Version));
        assert_eq!(p(&["."]), Ok(Action::Add { path: ".".into() }));
        assert_eq!(p(&["~/code/app"]), Ok(Action::Add { path: "~/code/app".into() }));
        assert_eq!(p(&["./ls"]), Ok(Action::Add { path: "./ls".into() }));
        assert_eq!(p(&["ls"]), Ok(Action::Ls { all: false, json: false }));
        assert_eq!(p(&["ls", "--all", "--json"]), Ok(Action::Ls { all: true, json: true }));
        assert_eq!(p(&["status"]), Ok(Action::Status { json: false }));
        assert_eq!(p(&["status", "--json"]), Ok(Action::Status { json: true }));
        assert_eq!(p(&["open", "mochi"]), Ok(Action::Open { query: "mochi".into() }));
        assert_eq!(p(&["open", "fix", "the", "login"]), Ok(Action::Open { query: "fix the login".into() }));
    }

    #[test]
    fn parses_new() {
        let new = |prompt: &str, agent: Option<&str>, base: Option<&str>, plan: bool| {
            Ok(Action::New { task: Task { prompt: prompt.into(), agent: agent.map(Into::into), base: base.map(Into::into), plan, linear: None } })
        };
        assert_eq!(p(&["new", "fix the login bug"]), new("fix the login bug", None, None, false));
        assert_eq!(p(&["new", "fix", "the", "bug"]), new("fix the bug", None, None, false));
        assert_eq!(p(&["new", "fix it", "--agent", "codex", "--base", "main", "--plan"]), new("fix it", Some("codex"), Some("main"), true));
        assert_eq!(p(&["new", "--agent=gemini", "-b", "dev", "fix it"]), new("fix it", Some("gemini"), Some("dev"), false));
        // After --, a task may start with a dash.
        assert_eq!(p(&["new", "--", "--weird", "task"]), new("--weird task", None, None, false));
    }

    #[test]
    fn parses_new_from_linear() {
        let linear = |args: &[&str]| match p(args) { Ok(Action::New { task }) => (task.linear, task.prompt, task.agent), other => panic!("{other:?}") };
        assert_eq!(linear(&["new", "--linear", "ENG-123"]), (Some("ENG-123".into()), String::new(), None));
        assert_eq!(linear(&["new", "--linear=eng-7", "--agent", "codex", "add", "tests", "too"]), (Some("ENG-7".into()), "add tests too".into(), Some("codex".into())));
        assert_eq!(linear(&["new", "-l", "https://linear.app/acme/issue/OPS-9/fix"]).0.as_deref(), Some("OPS-9"));
        assert!(p(&["new", "--linear", "fix-the-bug"]).unwrap_err().contains("isn't a Linear issue"));
        assert!(p(&["new", "--linear"]).unwrap_err().contains("--linear ENG-123"));
        // Without an issue, a task is still required.
        assert!(p(&["new"]).unwrap_err().contains("Write the task"));
        // An old app that doesn't know the field still reads the request; a new one keeps it.
        let task = Task { linear: Some("ENG-1".into()), ..Default::default() };
        let req = crate::remote::Request::New { cwd: "/r".into(), task };
        assert_eq!(crate::remote::decode(&crate::remote::encode(&req)).unwrap(), req);
        assert!(!crate::remote::encode(&crate::remote::Request::New { cwd: "/r".into(), task: Task { prompt: "x".into(), ..Default::default() } }).contains("linear"));
    }

    #[test]
    fn parses_queue() {
        let add = |args: &[&str]| match p(args) { Ok(Action::QueueAdd { repo, task }) => (repo, task), other => panic!("{other:?}") };
        let (repo, t) = add(&["queue", "add", "fix", "the", "bug"]);
        assert_eq!((repo, t.prompt.as_str(), t.agent, t.mode), (None, "fix the bug", None, None));
        let (repo, t) = add(&["queue", "add", "fix it", "--agent", "codex", "--fallback=claude", "--repo", "../api", "--base", "dev", "--mode", "auto", "--checks", "--pr", "--autopilot"]);
        assert_eq!(repo.as_deref(), Some("../api"));
        assert_eq!((t.agent.as_deref(), t.fallback.as_deref(), t.base.as_deref(), t.mode.as_deref()), (Some("codex"), Some("claude"), Some("dev"), Some("bypassPermissions")));
        assert!(t.checks && t.pr && t.autopilot);
        assert_eq!(add(&["queue", "add", "x", "-m", "edits"]).1.mode.as_deref(), Some("acceptEdits"));
        assert_eq!(add(&["queue", "add", "x", "--plan"]).1.mode.as_deref(), Some("plan"));
        assert_eq!(add(&["queue", "add", "--", "--weird"]).1.prompt, "--weird");
        assert_eq!(p(&["queue"]), Ok(Action::QueueLs { json: false }));
        assert_eq!(p(&["queue", "ls", "--json"]), Ok(Action::QueueLs { json: true }));
        assert_eq!(p(&["queue", "--json"]), Ok(Action::QueueLs { json: true }));
        assert!(p(&["queue", "add"]).unwrap_err().contains("nekotrail queue add"));
        assert!(p(&["queue", "add", "x", "--mode", "yolo2"]).unwrap_err().contains("plan, ask, edits or auto"));
        assert!(p(&["queue", "add", "x", "--agent", "codex", "--fallback", "codex"]).unwrap_err().contains("different agent"));
        assert!(p(&["queue", "add", "x", "--force"]).unwrap_err().contains("--force"));
        assert!(p(&["queue", "rm"]).is_err());
        let cwd = Path::new("/work/app");
        let req = request(&Action::QueueAdd { repo: Some("../api".into()), task: QueueTask { prompt: "x".into(), ..Default::default() } }, cwd, None).unwrap();
        assert!(matches!(req, Request::QueueAdd { cwd, repo: Some(r), .. } if cwd == "/work/app" && r == "/work/app/../api"));
    }

    #[test]
    fn formats_the_queue() {
        assert!(format_queue(&json!({"tasks": [], "paused": false})).contains("queue is empty"));
        let out = format_queue(&json!({"paused": true, "tasks": [
            {"title": "Fix the login bug", "state": "running", "step": "", "agent": "codex", "repo": "api"},
            {"title": "Add dark mode", "state": "waiting", "step": "", "agent": "claude", "repo": "web"},
            {"title": "Bump deps", "state": "then", "step": "autopilot", "agent": "claude", "repo": "web"},
        ]}));
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("STATE") && lines[1].starts_with("running ") && lines[1].contains("Codex"));
        assert!(lines[2].starts_with("waits for you") && lines[3].starts_with("autopilot"));
        assert!(out.ends_with("Queue view.\n"));
        assert_eq!(format_queued(&json!({"repo": "api", "agent": "codex", "state": "queued", "ahead": 2, "paused": false})), "Queued in api for Codex. 2 tasks are ahead of it.\n");
        assert!(format_queued(&json!({"repo": "api", "agent": "claude", "state": "queued", "ahead": 0, "paused": true})).contains("paused"));
        assert!(format_queued(&json!({"repo": "api", "agent": "claude", "state": "waiting", "ahead": 0})).contains("waits for you to start it"));
    }

    #[test]
    fn bad_arguments_say_what_to_do() {
        assert!(p(&["new"]).unwrap_err().contains("nekotrail new \""));
        assert!(p(&["new", "x", "--agent"]).unwrap_err().contains("--agent needs a value"));
        assert!(p(&["new", "x", "--agent", "skynet"]).unwrap_err().contains("claude"));
        assert!(p(&["new", "x", "--base", "-evil"]).is_err());
        assert!(p(&["new", "x", "--force"]).unwrap_err().contains("--force"));
        assert!(p(&["ls", "extra"]).is_err());
        assert!(p(&["status", "--all"]).is_err());
        assert!(p(&["open"]).is_err());
        assert!(p(&["--frobnicate"]).is_err());
        assert!(p(&["a", "b"]).is_err());
    }

    #[test]
    fn paths_become_absolute() {
        let cwd = Path::new("/work/app");
        let home = Path::new("/Users/me");
        assert_eq!(absolute(".", cwd, Some(home)), PathBuf::from("/work/app/."));
        assert_eq!(absolute("../other", cwd, Some(home)), PathBuf::from("/work/app/../other"));
        assert_eq!(absolute("/abs", cwd, Some(home)), PathBuf::from("/abs"));
        assert_eq!(absolute("~", cwd, Some(home)), PathBuf::from("/Users/me/"));
        assert_eq!(absolute("~/code", cwd, Some(home)), PathBuf::from("/Users/me/code"));
        assert_eq!(absolute("~bob", cwd, Some(home)), PathBuf::from("/work/app/~bob"));
        let req = request(&Action::New { task: Task { prompt: "x".into(), ..Default::default() } }, cwd, None).unwrap();
        assert!(matches!(req, Request::New { cwd, .. } if cwd == "/work/app"));
        assert_eq!(request(&Action::Help, cwd, None), None);
    }

    #[test]
    fn formats_ls_and_status() {
        let data = json!({"repo": "app", "workspaces": [
            {"name": "mochi", "state": "working", "agent": "codex", "pr": "#3 open, checks failing", "title": "Fix the login bug"},
            {"name": "luna", "state": "idle", "agent": null, "pr": null, "title": ""},
        ]});
        let out = format_ls(&data);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("NAME") && !lines[0].contains("REPO"));
        assert!(lines[1].contains("mochi") && lines[1].contains("codex") && lines[1].contains("#3 open"));
        assert!(lines[2].starts_with("luna ") && lines[2].contains(" - "));
        assert!(format_ls(&json!({"repo": null, "workspaces": [{"repo": "app", "name": "x", "state": "idle"}]})).starts_with("REPO"));
        assert!(format_ls(&json!({"repo": "app", "workspaces": []})).contains("No workspaces in app"));
        let st = format_status(&json!({"working": 2, "needs": [{"repo": "app", "name": "mochi", "title": "Fix it", "needs": ["waiting for your OK", "PR checks are failing"]}]}));
        assert_eq!(st, "app/mochi (Fix it): waiting for your OK, PR checks are failing\n2 agents are working.\n");
        assert_eq!(format_status(&json!({"working": 0, "needs": []})), "Nothing needs you.\n");
    }

    #[test]
    fn script_runs_this_app_in_cli_mode() {
        let s = script_text(Path::new("/Applications/neko's trail.app/Contents/MacOS/nekotrail"));
        assert!(s.starts_with("#!/bin/sh\n"));
        assert!(s.contains(r#"exec '/Applications/neko'\''s trail.app/Contents/MacOS/nekotrail' --cli "$@""#));
    }

    #[cfg(unix)]
    #[test]
    fn links_into_the_first_writable_folder_and_never_clobbers() {
        let t = tempfile::tempdir().unwrap();
        let script = t.path().join("dev.nekotrail.app/bin/nekotrail");
        write_script(&script, Path::new("/x/nekotrail")).unwrap();
        let a = t.path().join("a");
        let b = t.path().join("b");
        // `a` doesn't exist yet and gets made (like ~/.local/bin).
        assert_eq!(link_into(&script, &[a.clone(), b.clone()]), Ok(a.join("nekotrail")));
        assert_eq!(installed_link(&script, &[b.clone(), a.clone()]), Some(a.join("nekotrail")));
        // Installing again is fine.
        assert_eq!(link_into(&script, std::slice::from_ref(&a)), Ok(a.join("nekotrail")));
        // A file that isn't ours stops the install.
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("nekotrail"), "someone else's").unwrap();
        assert_eq!(link_into(&script, &[b.clone(), a.clone()]), Err(LinkError::Taken(b.join("nekotrail"))));
        assert_eq!(std::fs::read_to_string(b.join("nekotrail")).unwrap(), "someone else's");
        // A dangling link of ours is replaced.
        let c = t.path().join("c");
        std::fs::create_dir_all(&c).unwrap();
        std::os::unix::fs::symlink(t.path().join("gone"), c.join("nekotrail")).unwrap();
        assert_eq!(link_into(&script, std::slice::from_ref(&c)), Ok(c.join("nekotrail")));
        // A folder that can't be written is skipped.
        use std::os::unix::fs::PermissionsExt;
        let ro = t.path().join("ro");
        std::fs::create_dir_all(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        let d = t.path().join("d");
        assert_eq!(link_into(&script, &[ro.clone(), d.clone()]), Ok(d.join("nekotrail")));
        assert_eq!(link_into(&script, std::slice::from_ref(&ro)), Err(LinkError::NotWritable));
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
