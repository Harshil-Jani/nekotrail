//! Your login shell's PATH and env, without the window waiting for your shell profile.
//!
//! GUI apps on macOS start without your shell's PATH and env (CLAUDE_CONFIG_DIR, API keys...).
//! Linux desktop launchers have the same gap (nvm, ~/.local/bin and exports from .bashrc or
//! .zshrc). They come from `$SHELL -ilc env`, which takes 1 to 2 s on a busy profile and forever
//! on one that waits for something. The app used to run it before the window existed.
//!
//! Now:
//! 1. At launch, the env saved by the last good run is applied at once (`start`, before any
//!    thread exists). Nothing waits on the shell.
//! 2. The login shell runs on its own thread, with the usual deadline (`proc::LOGIN_SHELL`).
//! 3. Once it answers, its env goes on every agent, terminal and command started from then on
//!    (`apply`, `apply_pty`), and is saved for the next launch. An agent turn that starts while
//!    it is still running waits for it (`wait`), so it gets your API keys; the window never does.
//!
//! What is saved leaves out anything that looks like a secret (`secret_like`): those are only
//! ever held in memory, as before. Saved with owner-only permissions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

type Env = Arc<Vec<(String, String)>>;

#[derive(Default)]
enum Phase {
    /// `start` never ran (unit tests, tools): nothing to wait for, nothing to add.
    #[default]
    Off,
    /// The login shell is running.
    Loading,
    /// It answered (Some) or failed or hit its deadline (None).
    Ready(Option<Env>),
}

static STATE: Mutex<Phase> = Mutex::new(Phase::Off);
static CHANGED: Condvar = Condvar::new();

/// The file the last good login env is saved in.
pub fn cache_path(data: &Path) -> PathBuf {
    data.join("login-env.json")
}

/// Keys never taken from the login shell: they describe that shell, not your setup.
fn skipped(k: &str) -> bool {
    k.is_empty() || k.contains(char::is_whitespace) || k.contains('=') || matches!(k, "PWD" | "OLDPWD" | "SHLVL" | "_")
}

/// A key whose value is probably a credential. Those are used, never written to disk.
pub fn secret_like(k: &str) -> bool {
    let k = k.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL", "AUTH", "COOKIE", "PRIVATE", "SESSION"].iter().any(|w| k.contains(w))
}

/// `env -0` output as (key, value) pairs, in order, the shell-only keys left out.
pub fn parse(out: &[u8]) -> Vec<(String, String)> {
    out.split(|b| *b == 0)
        .filter_map(|kv| {
            let kv = String::from_utf8_lossy(kv);
            let (k, v) = kv.split_once('=')?;
            (!skipped(k)).then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// The saved env, if there is one and it reads.
pub fn load_cache(path: &Path) -> Vec<(String, String)> {
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    let map: HashMap<String, String> = serde_json::from_str(&raw).unwrap_or_default();
    map.into_iter().filter(|(k, _)| !skipped(k) && !secret_like(k)).collect()
}

/// Save `env` for the next launch, secrets left out, readable by you only. Written to a temp
/// file and renamed, so a crash mid-write never leaves half a file.
pub fn save_cache(path: &Path, env: &[(String, String)]) -> std::io::Result<()> {
    let keep: HashMap<&str, &str> = env.iter().filter(|(k, _)| !secret_like(k)).map(|(k, v)| (k.as_str(), v.as_str())).collect();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    {
        use std::io::Write;
        let mut f = opts.open(&tmp)?;
        f.write_all(serde_json::to_string(&keep).unwrap_or_default().as_bytes())?;
    }
    std::fs::rename(&tmp, path)
}

/// At launch, before any other thread exists: apply the saved env, then refresh it from the
/// login shell in the background. `path_override` (e2e runs) replaces PATH in both.
pub fn start(cache: PathBuf, path_override: Option<String>) {
    for (k, v) in load_cache(&cache) {
        // SAFETY: called first thing in `run`, before any other thread reads the environment.
        unsafe { std::env::set_var(k, v) };
    }
    *STATE.lock().unwrap_or_else(|p| p.into_inner()) = Phase::Loading;
    std::thread::spawn(move || {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| crate::pty::FALLBACK_SHELL.into());
        // With a deadline: a profile that waits on something never answers. Past it, everything
        // keeps the env it has (the saved one, or what the app was started with).
        let fresh = crate::proc::output(Command::new(shell).args(["-ilc", "env -0"]), None, crate::proc::LOGIN_SHELL)
            .ok()
            .filter(|o| o.status.success())
            .map(|o| parse(&o.stdout))
            .filter(|env| env.iter().any(|(k, _)| k == "PATH"));
        let fresh = fresh.map(|mut env| {
            if let Some(p) = &path_override {
                env.retain(|(k, _)| k != "PATH");
                env.push(("PATH".into(), p.clone()));
            }
            let _ = save_cache(&cache, &env);
            Arc::new(env)
        });
        set_ready(fresh);
    });
}

fn set_ready(env: Option<Env>) {
    *STATE.lock().unwrap_or_else(|p| p.into_inner()) = Phase::Ready(env);
    CHANGED.notify_all();
}

/// The login shell's env once it has answered.
fn fresh() -> Option<Env> {
    match &*STATE.lock().unwrap_or_else(|p| p.into_inner()) {
        Phase::Ready(env) => env.clone(),
        _ => None,
    }
}

/// Wait up to `limit` for the login shell, if it is still running. For agent turns: they need
/// your API keys, which are never saved. Returns at once when it already answered (or never ran).
pub fn wait(limit: Duration) {
    let st = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let _ = CHANGED.wait_timeout_while(st, limit, |p| matches!(p, Phase::Loading));
}

/// Give `cmd` the login shell's env, where it has answered. What the caller set on `cmd`
/// itself (a workspace's PORT, GIT_TERMINAL_PROMPT, an agent's own switches) wins.
pub fn apply(cmd: &mut Command) -> &mut Command {
    if let Some(env) = fresh() {
        add(cmd, &env);
    }
    cmd
}

/// `env` on `cmd`, except the keys `cmd` already sets or removes itself.
fn add(cmd: &mut Command, env: &[(String, String)]) {
    let own: Vec<std::ffi::OsString> = cmd.get_envs().map(|(k, _)| k.to_os_string()).collect();
    cmd.envs(env.iter().filter(|(k, _)| !own.iter().any(|o| o == k.as_str())).map(|(k, v)| (k.as_str(), v.as_str())));
}

/// `apply` for a terminal, before its own env is set (which then wins).
pub fn apply_pty(cmd: &mut portable_pty::CommandBuilder) {
    if let Some(env) = fresh() {
        for (k, v) in env.iter() {
            cmd.env(k, v);
        }
    }
}

/// `std::env::var_os`, from the login shell's env when it has answered.
pub fn var_os(key: &str) -> Option<std::ffi::OsString> {
    fresh().and_then(|env| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.into())).or_else(|| std::env::var_os(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keeps_your_vars_and_drops_the_shells_own() {
        let out = b"PATH=/opt/bin:/usr/bin\0PWD=/x\0SHLVL=2\0MULTI=a\nb\0EQ=a=b\0_=/usr/bin/env\0";
        let env = parse(out);
        assert_eq!(env, vec![("PATH".into(), "/opt/bin:/usr/bin".into()), ("MULTI".into(), "a\nb".into()), ("EQ".into(), "a=b".into())]);
    }

    #[test]
    fn the_saved_env_has_no_secrets_and_only_you_can_read_it() {
        let d = tempfile::tempdir().unwrap();
        let path = cache_path(&d.path().join("data"));
        let env: Vec<(String, String)> = [
            ("PATH", "/opt/homebrew/bin:/usr/bin"),
            ("CLAUDE_CONFIG_DIR", "/Users/me/.claude-work"),
            ("ANTHROPIC_API_KEY", "sk-ant-x"),
            ("GH_TOKEN", "ghp_x"),
            ("OPENAI_API_KEY", "sk-x"),
            ("AWS_SECRET_ACCESS_KEY", "x"),
            ("SSH_AUTH_SOCK", "/tmp/agent"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        save_cache(&path, &env).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        for secret in ["sk-ant-x", "ghp_x", "sk-x", "AWS_SECRET"] {
            assert!(!raw.contains(secret), "{secret} was written to disk: {raw}");
        }
        let mut back = load_cache(&path);
        back.sort();
        assert_eq!(back, vec![("CLAUDE_CONFIG_DIR".into(), "/Users/me/.claude-work".into()), ("PATH".into(), "/opt/homebrew/bin:/usr/bin".into())]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(load_cache(&d.path().join("nothing.json")).is_empty(), "no file reads as no env");
        std::fs::write(&path, "{not json").unwrap();
        assert!(load_cache(&path).is_empty(), "a broken file reads as no env");
    }

    #[test]
    fn the_fresh_env_is_added_but_what_the_caller_set_wins() {
        // `add`, not `apply`: the global state stays Off, so other tests' commands are untouched.
        let env = vec![("PATH".into(), "/fresh/bin".into()), ("NEKO_FRESH".into(), "1".into()), ("NEKO_GONE".into(), "x".into())];
        let mut c = Command::new("true");
        c.env("PATH", "/mine").env_remove("NEKO_GONE");
        add(&mut c, &env);
        let envs: HashMap<String, Option<String>> =
            c.get_envs().map(|(k, v)| (k.to_string_lossy().into(), v.map(|v| v.to_string_lossy().into()))).collect();
        assert_eq!(envs["PATH"].as_deref(), Some("/mine"));
        assert_eq!(envs["NEKO_FRESH"].as_deref(), Some("1"));
        assert_eq!(envs["NEKO_GONE"], None, "a key the caller removed stays removed");
    }

    #[test]
    fn nothing_waits_when_the_login_shell_never_ran() {
        let t = std::time::Instant::now();
        wait(Duration::from_secs(5));
        assert!(t.elapsed() < Duration::from_secs(1));
        assert!(fresh().is_none());
    }
}
