//! Running a short command (git, gh, a version check, the login shell) with a deadline.
//!
//! `Command::output()` waits as long as the process takes. A git that asks for a password on a
//! terminal it doesn't have, a `gh` on a dead network, or a shell profile that waits on something
//! never ends, and whatever waited on it (the turn's end, the PR poller, the app's start) waited
//! with it. `output` stops the command and everything it started at the deadline.
//!
//! It also writes stdin from its own thread while reading stdout. Writing all of stdin first
//! deadlocks once the command's output fills the pipe before it has read all its input
//! (`git hash-object --stdin-paths` with a few thousand paths).

use std::io::{Read, Write};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

/// Local git work (status, diff, a checkpoint of a big tree).
pub const GIT: Duration = Duration::from_secs(120);
/// git over the network (push, fetch, pull, clone).
pub const GIT_NETWORK: Duration = Duration::from_secs(300);
/// One `gh` call.
pub const GH: Duration = Duration::from_secs(60);
/// `<agent> --version` and similar checks.
pub const PROBE: Duration = Duration::from_secs(10);
/// The login shell that hands over your PATH and env (in the background, see loginenv.rs).
pub const LOGIN_SHELL: Duration = Duration::from_secs(10);

/// No password or account prompts: there is no terminal to answer them, so they would only wait.
pub fn no_prompts(cmd: &mut Command) -> &mut Command {
    cmd.env("GIT_TERMINAL_PROMPT", "0").env("GCM_INTERACTIVE", "never").env("GH_PROMPT_DISABLED", "1")
}

/// The deadline for `git <args>`: longer for the commands that talk to a remote.
pub fn git_limit(args: &[&str]) -> Duration {
    let network = ["push", "fetch", "pull", "clone", "ls-remote", "submodule"];
    if args.iter().any(|a| network.contains(a)) { GIT_NETWORK } else { GIT }
}

/// `cmd`'s output, like `Command::output`, but stopped (with everything it started) after
/// `limit`, which is then an error saying so. `stdin`, if any, is fed while the output is read.
pub fn output(cmd: &mut Command, stdin: Option<&[u8]>, limit: Duration) -> std::io::Result<Output> {
    // Your login shell's PATH and env once it has answered (loginenv.rs); the caller's own win.
    crate::loginenv::apply(cmd);
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn()?;
    let feeder = match (stdin, child.stdin.take()) {
        (Some(data), Some(mut pipe)) => {
            let data = data.to_vec();
            Some(std::thread::spawn(move || {
                let _ = pipe.write_all(&data);
            }))
        }
        _ => None,
    };
    let drain = |r: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        if let Some(mut r) = r {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = r.read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        rx
    };
    let out = drain(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let deadline = Instant::now() + limit;
    let status: ExitStatus = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if Instant::now() >= deadline {
            stop(&mut child);
            let what = format!("{:?}", cmd.get_program()).trim_matches('"').to_string();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("{what} took longer than {} s and was stopped", limit.as_secs()),
            ));
        }
        std::thread::sleep(Duration::from_millis(10).min(limit / 10 + Duration::from_millis(1)));
    };
    if let Some(f) = feeder {
        let _ = f.join();
    }
    // Something the command left running can keep the pipes open; don't wait on it for long.
    let take = |rx: std::sync::mpsc::Receiver<Vec<u8>>| rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    Ok(Output { status, stdout: take(out), stderr: take(err) })
}

/// TERM to the command's group, KILL a second later if it is still there.
fn stop(child: &mut std::process::Child) {
    let pid = child.id();
    let group = |sig: &str| {
        let _ = Command::new("kill").args([sig, "--", &format!("-{pid}")]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    };
    group("-TERM");
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    group("-KILL");
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_that_never_ends_is_stopped_at_its_deadline() {
        let t = Instant::now();
        let err = output(Command::new("sh").args(["-c", "sleep 30 & sleep 30"]), None, Duration::from_millis(300)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(err.to_string().contains("took longer than"), "{err}");
        assert!(t.elapsed() < Duration::from_secs(5), "took {:?}", t.elapsed());
    }

    #[test]
    fn output_and_status_come_back_like_command_output() {
        let o = output(Command::new("sh").args(["-c", "echo out; echo err >&2; exit 3"]), None, PROBE).unwrap();
        assert_eq!(o.status.code(), Some(3));
        assert_eq!(String::from_utf8_lossy(&o.stdout), "out\n");
        assert_eq!(String::from_utf8_lossy(&o.stderr), "err\n");
    }

    #[test]
    fn big_stdin_and_big_output_together_do_not_deadlock() {
        // cat echoes as it reads: writing 4 MB before reading would fill the output pipe first.
        let data = vec![b'x'; 4 << 20];
        let o = output(&mut Command::new("cat"), Some(&data), Duration::from_secs(20)).unwrap();
        assert_eq!(o.stdout.len(), data.len());
    }

    #[test]
    fn network_git_gets_more_time() {
        assert_eq!(git_limit(&["push", "-u", "origin", "b"]), GIT_NETWORK);
        assert_eq!(git_limit(&["status", "--porcelain"]), GIT);
    }
}
