//! Checks: a workspace's test, lint and typecheck commands, run in the background and read into
//! a list of failures (see `testparse`). The commands are the repo's named actions whose names
//! say they are checks (`test`, `lint`, `typecheck`, ...) plus the `checks` in its
//! `nekotrail.json`. With none set, `suggest` reads the repo for sensible ones the user can add.
//!
//! The last result of each check is kept per workspace on disk, so the sidebar dot and the
//! "passed before, fails now" comparison survive a restart.

use crate::testparse::{self, Ctx, Failure, Parsed};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// A check that runs longer than this is stopped and reported as an error.
pub const TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Output kept per check for "Show output" (the end of it, where the summary is).
const KEEP_OUTPUT: usize = 200 * 1024;
/// Output read for parsing. Beyond this only the end is kept.
const READ_OUTPUT: usize = 4 * 1024 * 1024;

// ---------- which commands are checks ----------

const CHECK_WORDS: &[&str] = &[
    "test", "tests", "lint", "typecheck", "type-check", "types", "tsc", "check", "clippy", "vet", "e2e", "spec",
    "pytest", "jest", "vitest", "eslint", "ruff", "mypy", "fmt-check", "format-check",
];

/// A named action counts as a check when its name is a check word, or starts or ends with one
/// (`test:unit`, `lint-rust`, `unit-test`).
pub fn is_check_name(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    CHECK_WORDS.iter().any(|w| {
        n == *w
            || [':', '-', '_', '.'].iter().any(|s| n.starts_with(&format!("{w}{s}")) || n.ends_with(&format!("{s}{w}")))
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Defined {
    pub name: String,
    pub command: String,
    /// "repo" for the repo's `nekotrail.json`, "app" for an action saved in repo settings.
    pub source: &'static str,
}

/// The checks to show: the repo file's first, then actions saved in the app that look like
/// checks and aren't already named in the file.
pub fn defined(file: &[(String, String)], actions: &[(String, String)]) -> Vec<Defined> {
    let mut out: Vec<Defined> = file.iter().map(|(n, c)| Defined { name: n.clone(), command: c.clone(), source: "repo" }).collect();
    for (n, c) in actions {
        let (n, c) = (n.trim(), c.trim());
        if !n.is_empty() && !c.is_empty() && is_check_name(n) && !out.iter().any(|d| d.name == n) {
            out.push(Defined { name: n.to_string(), command: c.to_string(), source: "app" });
        }
    }
    out
}

// ---------- suggestions ----------

/// Checks that fit what's in the folder: package.json scripts (with the package manager its lock
/// file names), cargo, go, pytest, ruff, mypy, and make targets. Also looks one folder down for a
/// Cargo.toml or go.mod (a Tauri app's `src-tauri`). A name already taken gets the tool's name
/// after it (`test-rust`).
pub fn suggest(dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let mut add = |name: &str, lang: &str, cmd: String| {
        // Same command already in, or the Makefile already has a target by this name.
        if out.iter().any(|(n, c)| *c == cmd || (lang != "make" && n == name && c.starts_with("make "))) {
            return;
        }
        let name = if out.iter().any(|(n, _)| n == name) { format!("{name}-{lang}") } else { name.to_string() };
        if !out.iter().any(|(n, _)| *n == name) {
            out.push((name, cmd));
        }
    };
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    let has = |f: &str| dir.join(f).exists();

    // make: the team's own entry points come first.
    let makefile = read(&dir.join("Makefile"));
    for t in ["test", "lint", "typecheck", "check"] {
        if makefile.lines().any(|l| l.split(':').next().is_some_and(|x| x.trim() == t) && l.contains(':') && !l.starts_with('\t')) {
            add(if t == "check" { "check" } else { t }, "make", format!("make {t}"));
        }
    }

    // JavaScript and TypeScript.
    if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&read(&dir.join("package.json"))) {
        let (run, x) = if has("bun.lock") || has("bun.lockb") {
            ("bun run", "bunx")
        } else if has("pnpm-lock.yaml") {
            ("pnpm", "pnpm exec")
        } else if has("yarn.lock") {
            ("yarn", "yarn")
        } else {
            ("npm run", "npx")
        };
        let scripts = pkg["scripts"].as_object().cloned().unwrap_or_default();
        let script = |n: &str| scripts.get(n).and_then(|v| v.as_str()).filter(|s| !s.contains("no test specified"));
        let dep = |n: &str| pkg["dependencies"].get(n).is_some() || pkg["devDependencies"].get(n).is_some();
        if script("test").is_some() {
            add("test", "js", if run == "npm run" { "npm test".into() } else { format!("{run} test") });
        } else if run == "bun run" && (has("tests") || has("test") || has("__tests__")) {
            add("test", "js", "bun test".into());
        } else if dep("vitest") {
            add("test", "js", format!("{x} vitest run"));
        }
        if script("lint").is_some() {
            add("lint", "js", format!("{run} lint"));
        } else if dep("eslint") && ["eslint.config.js", "eslint.config.mjs", "eslint.config.cjs", "eslint.config.ts", ".eslintrc", ".eslintrc.js", ".eslintrc.cjs", ".eslintrc.json", ".eslintrc.yml"].iter().any(|f| has(f)) {
            add("lint", "js", format!("{x} eslint ."));
        }
        if let Some(n) = ["typecheck", "type-check", "check-types", "types", "tsc"].into_iter().find(|n| script(n).is_some()) {
            add("typecheck", "js", format!("{run} {n}"));
        } else if has("tsconfig.json") && dep("typescript") {
            add("typecheck", "js", format!("{x} tsc --noEmit"));
        }
    }

    // Rust and Go, here or one folder down.
    let mut sub_dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    sub_dirs.retain(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| !n.starts_with('.') && n != "node_modules" && n != "target" && n != "vendor"));
    sub_dirs.sort();
    let find = |marker: &str| -> Option<String> {
        if dir.join(marker).exists() {
            return Some(String::new());
        }
        sub_dirs.iter().find(|d| d.join(marker).exists()).and_then(|d| d.file_name()?.to_str().map(|n| format!("cd {n} && ")))
    };
    if let Some(cd) = find("Cargo.toml") {
        add("test", "rust", format!("{cd}cargo test"));
        add("lint", "rust", format!("{cd}cargo clippy --all-targets -- -D warnings"));
    }
    if let Some(cd) = find("go.mod") {
        add("test", "go", format!("{cd}go test ./..."));
        add("lint", "go", format!("{cd}go vet ./..."));
    }

    // Python.
    let pyproject = read(&dir.join("pyproject.toml"));
    let py = if has("uv.lock") { "uv run " } else if has("poetry.lock") { "poetry run " } else { "" };
    let pytest = has("pytest.ini")
        || has("conftest.py")
        || pyproject.contains("[tool.pytest")
        || pyproject.contains("pytest")
        || read(&dir.join("setup.cfg")).contains("[tool:pytest]")
        || read(&dir.join("tox.ini")).contains("[pytest]");
    if pytest {
        add("test", "python", format!("{py}pytest"));
    }
    if pyproject.contains("[tool.ruff") || has("ruff.toml") || has(".ruff.toml") {
        add("lint", "python", format!("{py}ruff check ."));
    }
    if pyproject.contains("[tool.mypy") || has("mypy.ini") {
        add("typecheck", "python", format!("{py}mypy ."));
    }
    out
}

// ---------- results ----------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Passed,
    Failed,
    /// It couldn't start, or ran past the time limit.
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckResult {
    pub name: String,
    pub command: String,
    pub status: Status,
    /// Tests that passed, when the tool says.
    pub passed: Option<u32>,
    pub failed: u32,
    pub failures: Vec<Failure>,
    /// Formats that were read, e.g. ["cargo test"]. Empty when nothing was recognised.
    pub formats: Vec<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    /// The end of what it printed.
    pub output: String,
    pub finished_at: String,
    /// It passed last time (same command) and doesn't now.
    #[serde(default)]
    pub regressed: bool,
}

/// What running one command gave.
#[derive(Debug, Default)]
pub struct Exec {
    pub code: Option<i32>,
    pub output: String,
    pub stopped: bool,
    pub timed_out: bool,
    pub duration: Duration,
    pub started: Option<SystemTime>,
    pub spawn_error: Option<String>,
}

/// Turn a finished command and what was parsed from it into a result. `prev` is this check's
/// last result, for the "passed before" mark.
pub fn outcome(name: &str, command: &str, ex: &Exec, parsed: Parsed, prev: Option<&CheckResult>, timeout: Duration) -> CheckResult {
    let Parsed { formats, passed, failed, mut failures } = parsed;
    let status = if ex.spawn_error.is_some() || ex.timed_out {
        Status::Error
    } else if ex.code == Some(0) {
        Status::Passed
    } else {
        Status::Failed
    };
    if status == Status::Passed {
        // A tool that exits 0 found nothing to fail on; anything matched was noise or warnings.
        failures.clear();
    } else if failures.is_empty() {
        let title = if let Some(err) = &ex.spawn_error {
            format!("Couldn't start: {err}")
        } else if ex.timed_out {
            format!("Ran longer than {} minutes, so it was stopped", timeout.as_secs() / 60)
        } else {
            match ex.code {
                Some(c) => format!("Exited with code {c}"),
                None => "Ended by a signal".to_string(),
            }
        };
        let tail: Vec<&str> = ex.output.lines().filter(|l| !l.trim().is_empty()).collect();
        let message = tail[tail.len().saturating_sub(20)..].join("\n");
        failures.push(Failure { title, message, ..Default::default() });
    }
    let failed = if status == Status::Passed { 0 } else { failed.unwrap_or(0).max(failures.len() as u32) };
    let regressed = status != Status::Passed && prev.is_some_and(|p| p.status == Status::Passed && p.command == command);
    CheckResult {
        name: name.to_string(),
        command: command.to_string(),
        status,
        passed,
        failed,
        failures,
        formats: formats.into_iter().map(String::from).collect(),
        exit_code: ex.code,
        duration_ms: ex.duration.as_millis() as u64,
        output: tail_of(&ex.output, KEEP_OUTPUT),
        finished_at: crate::store::now(),
        regressed,
    }
}

/// The checks that passed last time and fail now.
pub fn regressions(results: &[CheckResult]) -> Vec<String> {
    results.iter().filter(|r| r.regressed).map(|r| r.name.clone()).collect()
}

/// Red if any check fails, green if they all pass, None before the first run.
pub fn summary(results: &[CheckResult]) -> Option<&'static str> {
    if results.iter().any(|r| r.status != Status::Passed) {
        Some("failed")
    } else if results.is_empty() {
        None
    } else {
        Some("passed")
    }
}

fn tail_of(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    let start = s[start..].find('\n').map(|i| start + i + 1).unwrap_or(start);
    format!("(earlier output cut)\n{}", &s[start..])
}

// ---------- parsing in context ----------

/// The `module` line of the folder's go.mod.
pub fn go_module(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("go.mod")).ok()?;
    text.lines().find_map(|l| l.trim().strip_prefix("module ").map(|m| m.trim().trim_matches('"').to_string()))
}

/// `cd src-tauri && cargo test` runs in `src-tauri`, so its paths are relative to that.
fn command_dir(cmd: &str) -> Option<&str> {
    let rest = cmd.trim().strip_prefix("cd ")?;
    let (dir, _) = rest.split_once("&&")?;
    let dir = dir.trim();
    (!dir.is_empty() && !dir.contains(char::is_whitespace) && !dir.starts_with('/') && !dir.contains("..")).then_some(dir)
}

/// Read a finished command: a JUnit report it wrote wins, else its output. Files come out
/// relative to the workspace, with a few lines of code around each one.
pub fn read_results(root: &Path, command: &str, ex: &Exec) -> Parsed {
    let sub = command_dir(command);
    let run_dir = sub.map(|s| root.join(s)).unwrap_or_else(|| root.to_path_buf());
    let run_str = run_dir.to_string_lossy().to_string();
    let module = go_module(&run_dir);
    let ctx = Ctx { root: &run_str, go_module: module.as_deref() };
    let since = ex.started.map(|t| t - Duration::from_secs(1));
    let mut parsed = since
        .and_then(|t| {
            let mut all: Option<Parsed> = None;
            for xml in junit_reports(&run_dir, t) {
                if let Some(p) = testparse::parse_junit(&xml, &ctx) {
                    let a = all.get_or_insert_with(Parsed::default);
                    a.formats = p.formats;
                    a.passed = Some(a.passed.unwrap_or(0) + p.passed.unwrap_or(0));
                    a.failed = Some(a.failed.unwrap_or(0) + p.failed.unwrap_or(0));
                    a.failures.extend(p.failures);
                }
            }
            all
        })
        .unwrap_or_else(|| testparse::parse(&ex.output, &ctx));
    // Tools print the physical path (getcwd), which differs when the folder sits behind a symlink.
    if let Ok(real) = run_dir.canonicalize()
        && real != run_dir {
            let real = real.to_string_lossy().to_string();
            for f in parsed.failures.iter_mut() {
                if let Some(file) = &f.file
                    && file.starts_with('/') {
                        f.file = Some(testparse::rel(file, &real));
                    }
            }
        }
    for f in parsed.failures.iter_mut() {
        if let (Some(s), Some(file)) = (sub, &f.file)
            && !Path::new(file).is_absolute() && !root.join(file).exists() && run_dir.join(file).exists() {
                f.file = Some(format!("{s}/{file}"));
            }
    }
    for f in parsed.failures.iter_mut().take(50) {
        if let (Some(file), Some(line)) = (&f.file, f.line) {
            f.snippet = snippet(root, file, line);
        }
    }
    parsed
}

/// Lines `line - 3` to `line + 3` of a file inside `root`, numbered, the failing one marked `>`.
pub fn snippet(root: &Path, file: &str, line: u32) -> Option<String> {
    if line == 0 || Path::new(file).is_absolute() || file.split('/').any(|p| p == "..") {
        return None;
    }
    let path = root.join(file);
    if std::fs::metadata(&path).ok()?.len() > 2 * 1024 * 1024 {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let at = line as usize;
    if at > lines.len() {
        return None;
    }
    let (from, to) = (at.saturating_sub(3).max(1), (at + 3).min(lines.len()));
    let width = to.to_string().len();
    Some(
        (from..=to)
            .map(|n| format!("{} {n:>width$} | {}", if n == at { '>' } else { ' ' }, lines[n - 1]))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// JUnit XML files under `dir` written since `since` (a few folders deep, skipping dependencies).
fn junit_reports(dir: &Path, since: SystemTime) -> Vec<String> {
    const SKIP: &[&str] = &[".git", "node_modules", ".venv", "venv", "__pycache__", ".next", ".nuxt", "vendor", ".nekotrail", ".cache"];
    let mut out = vec![];
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > 20_000 || out.len() >= 20 {
                return out;
            }
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                // target/ is huge; only nextest writes reports there.
                let skip = SKIP.contains(&name.as_str()) || (name == "target" && !p.join("nextest").exists());
                if depth < 4 && !skip {
                    stack.push((if name == "target" { p.join("nextest") } else { p }, depth + 1));
                }
                continue;
            }
            if !name.ends_with(".xml") || meta.len() > 20 * 1024 * 1024 || meta.modified().map(|m| m < since).unwrap_or(true) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&p)
                && text.contains("<testcase") {
                    out.push(text);
                }
        }
    }
    out
}

// ---------- running ----------

/// Run `cmd` in a login shell in `cwd`, output and errors together, until it ends, `stop` is
/// set, or `timeout` passes. `on_pid` gets the process id (its process group, for Stop).
pub fn exec(cmd: &str, cwd: &Path, env: &[(String, String)], stop: &AtomicBool, timeout: Duration, on_pid: impl FnOnce(u32)) -> Exec {
    use std::os::unix::process::CommandExt;
    let t0 = Instant::now();
    let mut ex = Exec { started: Some(SystemTime::now()), ..Default::default() };
    let (mut reader, writer) = match std::io::pipe() {
        Ok(p) => p,
        Err(err) => {
            ex.spawn_error = Some(err.to_string());
            return ex;
        }
    };
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut c = Command::new(shell);
    crate::loginenv::apply(&mut c);
    c.args(["-lc", cmd]).current_dir(cwd).stdin(Stdio::null()).process_group(0);
    for (k, v) in env {
        c.env(k, v);
    }
    // Plain output, and no watch mode or prompts (jest, vitest and others check CI).
    c.env("CI", "1").env("NO_COLOR", "1").env("FORCE_COLOR", "0").env("TERM", "dumb");
    match writer.try_clone() {
        Ok(w2) => {
            c.stdout(writer).stderr(w2);
        }
        Err(err) => {
            ex.spawn_error = Some(err.to_string());
            return ex;
        }
    }
    let child = c.spawn();
    drop(c); // our copies of the pipe's write end, so reading ends when the command does
    let mut child = match child {
        Ok(ch) => ch,
        Err(err) => {
            ex.spawn_error = Some(err.to_string());
            return ex;
        }
    };
    let pid = child.id();
    on_pid(pid);
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    {
        let buf = buf.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut b = buf.lock().unwrap();
                        b.extend_from_slice(&chunk[..n]);
                        if b.len() > READ_OUTPUT + READ_OUTPUT / 2 {
                            let cut = b.len() - READ_OUTPUT;
                            b.drain(..cut);
                        }
                    }
                }
            }
            let _ = done_tx.send(());
        });
    }
    loop {
        match child.try_wait() {
            Ok(Some(s)) => {
                ex.code = s.code();
                break;
            }
            Ok(None) => {}
            Err(_) => break,
        }
        if stop.load(Ordering::Relaxed) || t0.elapsed() > timeout {
            ex.stopped = stop.load(Ordering::Relaxed);
            ex.timed_out = !ex.stopped;
            kill_group(pid, &mut child);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // A background process the command left behind can hold the pipe open; don't wait on it.
    let _ = done_rx.recv_timeout(Duration::from_secs(2));
    ex.output = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    ex.duration = t0.elapsed();
    ex
}

fn kill_group(pid: u32, child: &mut std::process::Child) {
    let signal = |sig: &str| {
        let _ = Command::new("kill").args([sig, "--", &format!("-{pid}")]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    };
    signal("-TERM");
    for _ in 0..30 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    signal("-KILL");
    let _ = child.wait();
}

// ---------- state ----------

struct Active {
    stop: Arc<AtomicBool>,
    /// The check running now, and the ones still to go.
    current: Option<String>,
    queued: Vec<String>,
}

#[derive(Default)]
struct Inner {
    /// Last result of each check, per workspace (loaded from disk on first use).
    results: HashMap<String, Vec<CheckResult>>,
    /// Workspace id to "passed" or "failed", for the sidebar.
    index: Option<HashMap<String, String>>,
    active: HashMap<String, Active>,
}

/// Every workspace's check results and runs.
#[derive(Clone)]
pub struct Checks {
    inner: Arc<Mutex<Inner>>,
    dir: PathBuf,
}

impl Checks {
    pub fn new(dir: PathBuf) -> Self {
        Self { inner: Default::default(), dir }
    }

    fn file(&self, ws: &str) -> PathBuf {
        self.dir.join(format!("{}.json", ws.replace(['/', '\\', '.'], "_")))
    }

    fn load<'a>(&self, inner: &'a mut Inner, ws: &str) -> &'a mut Vec<CheckResult> {
        inner.results.entry(ws.to_string()).or_insert_with(|| {
            std::fs::read_to_string(self.file(ws)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
        })
    }

    fn index<'a>(&self, inner: &'a mut Inner) -> &'a mut HashMap<String, String> {
        inner.index.get_or_insert_with(|| {
            std::fs::read_to_string(self.dir.join("index.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
        })
    }

    pub fn results(&self, ws: &str) -> Vec<CheckResult> {
        let mut inner = self.inner.lock().unwrap();
        self.load(&mut inner, ws).clone()
    }

    /// The last result of one check.
    pub fn result(&self, ws: &str, name: &str) -> Option<CheckResult> {
        self.results(ws).into_iter().find(|r| r.name == name)
    }

    /// Store a finished check, replacing its last result, and write it to disk.
    pub fn put(&self, ws: &str, r: CheckResult) {
        let mut inner = self.inner.lock().unwrap();
        let list = self.load(&mut inner, ws);
        match list.iter_mut().find(|x| x.name == r.name) {
            Some(x) => *x = r,
            None => list.push(r),
        }
        let list = list.clone();
        self.save(&mut inner, ws, &list);
    }

    /// Keep only the results of checks that still exist (a renamed or removed check drops out).
    pub fn retain(&self, ws: &str, names: &[String]) {
        let mut inner = self.inner.lock().unwrap();
        let list = self.load(&mut inner, ws);
        let before = list.len();
        list.retain(|r| names.contains(&r.name));
        if list.len() != before {
            let list = list.clone();
            self.save(&mut inner, ws, &list);
        }
    }

    fn save(&self, inner: &mut Inner, ws: &str, list: &[CheckResult]) {
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(self.file(ws), serde_json::to_string(list).unwrap_or_default());
        let status = summary(list);
        let index = self.index(inner);
        match status {
            Some(s) => index.insert(ws.to_string(), s.to_string()),
            None => index.remove(ws),
        };
        let _ = std::fs::write(self.dir.join("index.json"), serde_json::to_string(index).unwrap_or_default());
    }

    /// Forget a workspace's results (it was archived).
    pub fn forget(&self, ws: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.results.remove(ws);
        let _ = std::fs::remove_file(self.file(ws));
        let index = self.index(&mut inner);
        if index.remove(ws).is_some() {
            let _ = std::fs::write(self.dir.join("index.json"), serde_json::to_string(index).unwrap_or_default());
        }
    }

    /// Start a run of `names`. None when one is already going in this workspace.
    pub fn begin(&self, ws: &str, names: &[String]) -> Option<Arc<AtomicBool>> {
        let mut inner = self.inner.lock().unwrap();
        if inner.active.contains_key(ws) {
            return None;
        }
        let stop = Arc::new(AtomicBool::new(false));
        inner.active.insert(ws.to_string(), Active { stop: stop.clone(), current: None, queued: names.to_vec() });
        Some(stop)
    }

    /// `name` starts now.
    pub fn starting(&self, ws: &str, name: &str) {
        if let Some(a) = self.inner.lock().unwrap().active.get_mut(ws) {
            a.queued.retain(|n| n != name);
            a.current = Some(name.to_string());
        }
    }

    pub fn end(&self, ws: &str) {
        self.inner.lock().unwrap().active.remove(ws);
    }

    /// Stop the run: the check going now is ended and the rest don't start.
    pub fn stop(&self, ws: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        let Some(a) = inner.active.get(ws) else { return false };
        a.stop.store(true, Ordering::Relaxed);
        true
    }

    /// (running now, still to go) for a workspace with a run going.
    pub fn progress(&self, ws: &str) -> Option<(Option<String>, Vec<String>)> {
        self.inner.lock().unwrap().active.get(ws).map(|a| (a.current.clone(), a.queued.clone()))
    }

    /// Every workspace's dot: "running", "failed" or "passed".
    pub fn statuses(&self) -> HashMap<String, String> {
        let mut inner = self.inner.lock().unwrap();
        let mut out = self.index(&mut inner).clone();
        for ws in inner.active.keys() {
            out.insert(ws.clone(), "running".into());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_names() {
        for n in ["test", "Test", "lint", "typecheck", "test:unit", "lint-rust", "unit-test", "e2e", "check", "clippy", "tsc"] {
            assert!(is_check_name(n), "{n}");
        }
        for n in ["db:migrate", "deploy", "testify", "contest", "dev", "build", "checkout"] {
            assert!(!is_check_name(n), "{n}");
        }
    }

    #[test]
    fn repo_file_first_then_check_actions() {
        let file = vec![("test".to_string(), "bun test".to_string())];
        let actions = vec![("test".into(), "npm test".into()), ("lint".into(), "eslint .".into()), ("db:migrate".into(), "x".into()), ("e2e".into(), " ".into())];
        let d = defined(&file, &actions);
        assert_eq!(d, [
            Defined { name: "test".into(), command: "bun test".into(), source: "repo" },
            Defined { name: "lint".into(), command: "eslint .".into(), source: "app" },
        ]);
    }

    fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (f, body) in files {
            let p = d.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        d
    }

    #[test]
    fn suggests_per_package_manager() {
        let pkg = r#"{"scripts":{"test":"vitest run","lint":"eslint .","typecheck":"tsc -b"}}"#;
        let d = dir_with(&[("package.json", pkg), ("pnpm-lock.yaml", "")]);
        assert_eq!(suggest(d.path()), [("test".to_string(), "pnpm test".to_string()), ("lint".into(), "pnpm lint".into()), ("typecheck".into(), "pnpm typecheck".into())]);
        let d = dir_with(&[("package.json", pkg)]);
        assert_eq!(suggest(d.path())[0].1, "npm test");
        assert_eq!(suggest(d.path())[1].1, "npm run lint");
        // npm's placeholder test script isn't a test.
        let d = dir_with(&[("package.json", r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"},"devDependencies":{"typescript":"5"}}"#), ("tsconfig.json", "{}"), ("yarn.lock", "")]);
        assert_eq!(suggest(d.path()), [("typecheck".to_string(), "yarn tsc --noEmit".to_string())]);
    }

    #[test]
    fn suggests_bun_test_and_a_rust_crate_one_folder_down() {
        // Like nekotrail itself: bun with a tests folder, and the Rust side in src-tauri.
        let d = dir_with(&[("package.json", r#"{"devDependencies":{"typescript":"6"}}"#), ("bun.lock", ""), ("tests/a.test.ts", ""), ("tsconfig.json", "{}"), ("src-tauri/Cargo.toml", "")]);
        assert_eq!(suggest(d.path()), [
            ("test".to_string(), "bun test".to_string()),
            ("typecheck".into(), "bunx tsc --noEmit".into()),
            ("test-rust".into(), "cd src-tauri && cargo test".into()),
            ("lint".into(), "cd src-tauri && cargo clippy --all-targets -- -D warnings".into()),
        ]);
    }

    #[test]
    fn suggests_go_python_and_make() {
        let d = dir_with(&[("go.mod", "module pawprint\n"), ("Makefile", "test:\n\tgo test ./...\nlint: vet\n\tgolangci-lint run\n")]);
        assert_eq!(suggest(d.path()), [("test".to_string(), "make test".to_string()), ("lint".into(), "make lint".into())]);
        let d = dir_with(&[("go.mod", "module pawprint\n")]);
        assert_eq!(suggest(d.path()), [("test".to_string(), "go test ./...".to_string()), ("lint".into(), "go vet ./...".into())]);
        let d = dir_with(&[("pyproject.toml", "[tool.pytest.ini_options]\n[tool.ruff]\n[tool.mypy]\n"), ("uv.lock", "")]);
        assert_eq!(suggest(d.path()), [("test".to_string(), "uv run pytest".to_string()), ("lint".into(), "uv run ruff check .".into()), ("typecheck".into(), "uv run mypy .".into())]);
        assert!(suggest(dir_with(&[("README.md", "")]).path()).is_empty());
    }

    fn ex(code: Option<i32>, output: &str) -> Exec {
        Exec { code, output: output.into(), ..Default::default() }
    }
    fn passed(cmd: &str) -> CheckResult {
        outcome("test", cmd, &ex(Some(0), "ok"), Parsed::default(), None, TIMEOUT)
    }

    #[test]
    fn passed_before_fails_now_is_a_regression() {
        let before = passed("bun test");
        assert_eq!(before.status, Status::Passed);
        let parsed = testparse::parse("(fail) names > slugs [1ms]\n 0 pass\n 1 fail\n", &Ctx::default());
        let now = outcome("test", "bun test", &ex(Some(1), ""), parsed.clone(), Some(&before), TIMEOUT);
        assert_eq!((now.status, now.failed, now.regressed), (Status::Failed, 1, true));
        assert_eq!(regressions(std::slice::from_ref(&now)), ["test"]);
        // Already failing: not new.
        let again = outcome("test", "bun test", &ex(Some(1), ""), parsed.clone(), Some(&now), TIMEOUT);
        assert!(!again.regressed);
        // A different command isn't the same check.
        let other = outcome("test", "bun test --bail", &ex(Some(1), ""), parsed, Some(&before), TIMEOUT);
        assert!(!other.regressed);
        // Never ran before: nothing to compare with.
        assert!(!outcome("test", "bun test", &ex(Some(1), "boom"), Parsed::default(), None, TIMEOUT).regressed);
    }

    #[test]
    fn exit_code_decides_and_unread_failures_still_show() {
        // Nothing recognised: the end of the output is the failure.
        let r = outcome("lint", "x", &ex(Some(2), "line 1\n\nsomething broke\n"), Parsed::default(), None, TIMEOUT);
        assert_eq!(r.status, Status::Failed);
        assert_eq!(r.failures[0].title, "Exited with code 2");
        assert_eq!(r.failures[0].message, "line 1\nsomething broke");
        // Exit 0 wins over anything that looked like an error.
        let parsed = testparse::parse("src/a.ts(1,1): error TS1: x\n", &Ctx::default());
        let r = outcome("tsc", "x", &ex(Some(0), ""), parsed, None, TIMEOUT);
        assert_eq!((r.status, r.failed, r.failures.len()), (Status::Passed, 0, 0));
        let r = outcome("t", "x", &Exec { timed_out: true, ..Default::default() }, Parsed::default(), Some(&passed("x")), TIMEOUT);
        assert_eq!(r.status, Status::Error);
        assert!(r.failures[0].title.starts_with("Ran longer than 30 minutes"));
        assert!(r.regressed);
        assert_eq!(summary(&[passed("a"), r]), Some("failed"));
        assert_eq!(summary(&[passed("a")]), Some("passed"));
        assert_eq!(summary(&[]), None);
    }

    #[test]
    fn runs_a_command_and_reads_its_output() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.ts"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let stop = AtomicBool::new(false);
        let mut pid = 0;
        let cmd = "echo \"a.ts(3,1): error TS2322: bad\" >&2; echo $CI; exit 3";
        let e = exec(cmd, d.path(), &[], &stop, TIMEOUT, |p| pid = p);
        assert!(pid > 0);
        assert_eq!(e.code, Some(3));
        assert!(e.output.contains("error TS2322") && e.output.contains('1'), "{}", e.output);
        let parsed = read_results(d.path(), cmd, &e);
        assert_eq!(parsed.failures[0].file.as_deref(), Some("a.ts"));
        assert_eq!(parsed.failures[0].snippet.as_deref(), Some("  1 | one\n  2 | two\n> 3 | three\n  4 | four\n  5 | five"));
    }

    #[test]
    fn stop_ends_the_command_and_its_children() {
        let d = tempfile::tempdir().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            s2.store(true, Ordering::Relaxed);
        });
        let t = Instant::now();
        let e = exec("sleep 30 & sleep 30", d.path(), &[], &stop, TIMEOUT, |_| {});
        assert!(e.stopped && !e.timed_out);
        assert!(t.elapsed() < Duration::from_secs(8), "{:?}", t.elapsed());
        let e = exec("sleep 30", d.path(), &[], &AtomicBool::new(false), Duration::from_millis(300), |_| {});
        assert!(e.timed_out);
    }

    #[test]
    fn junit_report_written_by_the_command_wins() {
        let d = tempfile::tempdir().unwrap();
        let xml = r#"<testsuite><testcase name="adds" file="m.ts" line="2"><failure message="nope"/></testcase><testcase name="ok"/></testsuite>"#;
        std::fs::write(d.path().join("m.ts"), "a\nb\n").unwrap();
        let cmd = format!("mkdir -p reports && printf '%s' '{xml}' > reports/junit.xml; echo 'Tests: 9 failed'; exit 1");
        let e = exec(&cmd, d.path(), &[], &AtomicBool::new(false), TIMEOUT, |_| {});
        let p = read_results(d.path(), &cmd, &e);
        assert_eq!(p.formats, ["JUnit XML"]);
        assert_eq!((p.passed, p.failed), (Some(1), Some(1)));
        assert_eq!(p.failures[0].title, "adds");
        assert_eq!(p.failures[0].message, "nope");
    }

    #[test]
    fn paths_from_a_subfolder_command() {
        let d = dir_with(&[("src-tauri/src/lib.rs", "fn a() {}\n")]);
        let cmd = "cd src-tauri && false";
        let e = Exec { code: Some(1), output: "error: bad\n --> src/lib.rs:1:1\n".into(), ..Default::default() };
        let p = read_results(d.path(), cmd, &e);
        assert_eq!(p.failures[0].file.as_deref(), Some("src-tauri/src/lib.rs"));
        assert!(p.failures[0].snippet.is_some());
        assert_eq!(command_dir("cd ../x && y"), None);
    }

    #[test]
    fn paths_through_a_symlinked_folder() {
        let d = dir_with(&[("src/a.ts", "x\n")]);
        let link = tempfile::tempdir().unwrap();
        let via = link.path().join("ws");
        std::os::unix::fs::symlink(d.path(), &via).unwrap();
        let real = d.path().canonicalize().unwrap();
        let e = Exec { code: Some(1), output: format!("{}/src/a.ts(1,1): error TS1: bad\n", real.display()), ..Default::default() };
        let p = read_results(&via, "tsc", &e);
        assert_eq!(p.failures[0].file.as_deref(), Some("src/a.ts"));
        assert!(p.failures[0].snippet.is_some());
    }

    #[test]
    fn results_survive_a_restart() {
        let d = tempfile::tempdir().unwrap();
        let c = Checks::new(d.path().to_path_buf());
        assert!(c.statuses().is_empty());
        c.put("w1", passed("bun test"));
        let mut lint = passed("eslint .");
        lint.name = "lint".into();
        lint.status = Status::Failed;
        c.put("w1", lint);
        c.put("w2", passed("go test ./..."));
        assert!(c.begin("w2", &["test".into()]).is_some());
        assert!(c.begin("w2", &["test".into()]).is_none(), "one run at a time");
        let s = c.statuses();
        assert_eq!((s["w1"].as_str(), s["w2"].as_str()), ("failed", "running"));
        c.end("w2");
        let again = Checks::new(d.path().to_path_buf());
        let s = again.statuses();
        assert_eq!((s["w1"].as_str(), s["w2"].as_str()), ("failed", "passed"));
        assert_eq!(again.results("w1").len(), 2);
        again.retain("w1", &["test".into()]);
        assert_eq!(again.statuses()["w1"], "passed");
        again.forget("w1");
        assert!(!again.statuses().contains_key("w1"));
        assert!(again.results("w1").is_empty());
    }
}
