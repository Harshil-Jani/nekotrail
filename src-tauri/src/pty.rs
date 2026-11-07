//! Real terminals (login shells in a PTY) for each workspace.

use anyhow::Result;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    /// Input for the terminal. Its own thread writes it, so typing or pasting never waits under
    /// the terminals' lock: a paste bigger than the PTY's input buffer into a program that isn't
    /// reading (a dev server, `sleep`) used to block there and freeze every terminal, then the app.
    input: std::sync::mpsc::Sender<Vec<u8>>,
    child: Box<dyn Child + Send + Sync>,
    /// Recent output, replayed when the terminal view is rebuilt (e.g. after switching workspace).
    scrollback: Arc<Mutex<String>>,
    /// Which start of this terminal id this is, so a late exit of an old process can't close a new one.
    generation: u64,
    /// Started to run a command (run, setup or a named action) rather than as an interactive shell.
    scripted: bool,
}

const SCROLLBACK: usize = 256 * 1024;

/// The shell when $SHELL is unset: zsh is the macOS default, bash the usual Linux one.
pub const FALLBACK_SHELL: &str = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/bash" };
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Default, Clone)]
pub struct Ptys(Arc<Mutex<HashMap<String, Pty>>>);

impl Ptys {
    /// Start a login shell in `cwd`, or run `command` in one (`$SHELL -lc command`).
    /// `on_data` gets output chunks; `on_exit(generation)` fires once when it ends. Returns the generation.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        &self,
        id: &str,
        cwd: &str,
        env: &[(String, String)],
        command: Option<&str>,
        cols: u16,
        rows: u16,
        on_data: impl Fn(String) + Send + 'static,
        on_exit: impl FnOnce(u64) + Send + 'static,
    ) -> Result<u64> {
        self.close(id);
        let generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| FALLBACK_SHELL.into());
        let mut cmd = CommandBuilder::new(shell);
        match command {
            Some(c) => { cmd.args(["-lc", c]); }
            None => { cmd.arg("-l"); }
        }
        cmd.cwd(cwd);
        crate::loginenv::apply_pty(&mut cmd);
        cmd.env("TERM", "xterm-256color");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let (input, typed) = std::sync::mpsc::channel::<Vec<u8>>();
        // Ends when the terminal is forgotten (its sender dropped) or its process is gone.
        std::thread::spawn(move || {
            for bytes in typed {
                if writer.write_all(&bytes).and_then(|_| writer.flush()).is_err() {
                    break;
                }
            }
        });
        let scrollback = Arc::new(Mutex::new(String::new()));
        let sb = scrollback.clone();
        // In the map before its output is read, so an exit that comes at once still finds it and
        // removes it (before, a terminal that ended first stayed listed as running).
        self.0.lock().unwrap().insert(id.to_string(), Pty { master: pair.master, input, child, scrollback, generation, scripted: command.is_some() });
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut pending: Vec<u8> = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&buf[..n]);
                        // Only emit complete UTF-8; keep a split multibyte char for the next read.
                        let valid = match std::str::from_utf8(&pending) {
                            Ok(_) => pending.len(),
                            Err(e) if e.error_len().is_none() => e.valid_up_to(),
                            Err(_) => pending.len(),
                        };
                        let chunk: Vec<u8> = pending.drain(..valid).collect();
                        let text = String::from_utf8_lossy(&chunk).into_owned();
                        {
                            let mut b = sb.lock().unwrap();
                            b.push_str(&text);
                            if b.len() > SCROLLBACK {
                                let mut cut = b.len() - SCROLLBACK;
                                while !b.is_char_boundary(cut) {
                                    cut += 1;
                                }
                                b.drain(..cut);
                            }
                        }
                        on_data(text);
                    }
                }
            }
            on_exit(generation);
        });
        Ok(generation)
    }

    /// Output so far, if this terminal is running.
    pub fn scrollback(&self, id: &str) -> Option<String> {
        self.0.lock().unwrap().get(id).map(|p| p.scrollback.lock().unwrap().clone())
    }

    /// Forget a terminal whose process ended, unless it has been restarted since.
    pub fn close_if(&self, id: &str, generation: u64) {
        let mut map = self.0.lock().unwrap();
        if map.get(id).is_some_and(|p| p.generation == generation) {
            map.remove(id);
        }
    }

    /// Type `data` into the terminal. Returns at once: the terminal's input thread writes it.
    pub fn write(&self, id: &str, data: &str) -> Result<()> {
        let input = self.0.lock().unwrap().get(id).map(|p| p.input.clone());
        if let Some(input) = input {
            input.send(data.as_bytes().to_vec()).map_err(|_| anyhow::anyhow!("This terminal has ended."))?;
        }
        Ok(())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        if let Some(p) = self.0.lock().unwrap().get(id) {
            p.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        }
        Ok(())
    }

    pub fn close(&self, id: &str) {
        if let Some(mut p) = self.0.lock().unwrap().remove(id) {
            let _ = p.child.kill();
        }
    }

    /// Stop every terminal whose id starts with `prefix`. A workspace's terminals are all named
    /// `<workspace id>:<name>` (`:term`, the extra tabs `:term-<time>`, `:run`, `:setup`, actions),
    /// so `"<workspace id>:"` closes them all. Returns how many were closed.
    pub fn close_prefix(&self, prefix: &str) -> usize {
        let ids: Vec<String> = self.0.lock().unwrap().keys().filter(|k| k.starts_with(prefix)).cloned().collect();
        for id in &ids {
            self.close(id);
        }
        ids.len()
    }

    pub fn is_open(&self, id: &str) -> bool {
        self.0.lock().unwrap().contains_key(id)
    }

    /// (terminal id, pid of its shell or script) for every running terminal. What they start
    /// descends from these pids, which is how the Ports popover knows a server is a workspace's.
    pub fn pids(&self) -> Vec<(String, u32)> {
        self.0.lock().unwrap().iter().filter_map(|(id, p)| Some((id.clone(), p.child.process_id()?))).collect()
    }

    /// Terminals doing work that a restart would stop: every running script, and every
    /// interactive shell with a command in the foreground. A shell at its prompt is not counted,
    /// even if it keeps background helpers (prompt themes often run one).
    pub fn busy(&self) -> usize {
        let map = self.0.lock().unwrap();
        map.values()
            .filter(|p| {
                p.scripted
                    || match (p.master.process_group_leader(), p.child.process_id()) {
                        (Some(fg), Some(shell)) => fg as u32 != shell,
                        _ => false,
                    }
            })
            .count()
    }

    /// Stop every terminal (used right before the app relaunches).
    pub fn close_all(&self) {
        let ids: Vec<String> = self.0.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.close(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn runs_commands_in_cwd() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("marker-file"), "").unwrap();
        let ptys = Ptys::default();
        let (tx, rx) = mpsc::channel::<String>();
        ptys.open("t", d.path().to_str().unwrap(), &[("NEKO".into(), "1".into())], None, 80, 24, move |s| { let _ = tx.send(s); }, |_| {}).unwrap();
        ptys.write("t", "ls; exit\n").unwrap();
        let mut out = String::new();
        while let Ok(s) = rx.recv_timeout(Duration::from_secs(10)) {
            out.push_str(&s);
            if out.contains("marker-file") { break; }
        }
        assert!(out.contains("marker-file"), "got: {out}");
        ptys.close("t");
        assert!(!ptys.is_open("t"));
    }

    #[test]
    fn a_big_paste_into_a_busy_terminal_freezes_nothing() {
        // `sleep` never reads its input, so the PTY's input queue fills. Writing 240 KB used to block under the terminals' lock: every terminal, then the app.
        let d = tempfile::tempdir().unwrap();
        let cwd = d.path().to_str().unwrap();
        let ptys = Ptys::default();
        ptys.open("busy", cwd, &[], Some("sleep 30"), 80, 24, |_| {}, |_| {}).unwrap();
        let (tx, rx) = mpsc::channel::<String>();
        ptys.open("other", cwd, &[], None, 80, 24, move |s| { let _ = tx.send(s); }, |_| {}).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let p2 = ptys.clone();
        std::thread::spawn(move || {
            // Whole lines: the tty keeps complete lines until they are read, and blocks the writer
            // once its input queue is full (characters past one line's limit are only dropped).
            p2.write("busy", &format!("{}\n", "x".repeat(79)).repeat(3000)).unwrap();
            let _ = done_tx.send(());
        });
        assert!(done_rx.recv_timeout(Duration::from_secs(5)).is_ok(), "the paste blocked");
        // The other terminal and the map still answer.
        ptys.write("other", "echo still-$((40+2))\n").unwrap();
        let mut out = String::new();
        while let Ok(s) = rx.recv_timeout(Duration::from_secs(10)) {
            out.push_str(&s);
            if out.contains("still-42") { break; }
        }
        assert!(out.contains("still-42"), "got: {out}");
        assert!(ptys.is_open("busy") && ptys.scrollback("busy").is_some());
        ptys.close("busy");
        ptys.close("other");
    }

    #[test]
    fn a_terminal_that_ends_at_once_is_not_left_listed() {
        let d = tempfile::tempdir().unwrap();
        let ptys = Ptys::default();
        let (tx, rx) = mpsc::channel::<u64>();
        let p2 = ptys.clone();
        ptys.open("quick", d.path().to_str().unwrap(), &[], Some("true"), 80, 24, |_| {}, move |g| { p2.close_if("quick", g); let _ = tx.send(g); }).unwrap();
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(!ptys.is_open("quick"));
    }

    #[test]
    fn command_runs_keep_scrollback_and_old_exits_cannot_close_new_runs() {
        let d = tempfile::tempdir().unwrap();
        let cwd = d.path().to_str().unwrap();
        let ptys = Ptys::default();
        let (tx, rx) = mpsc::channel::<u64>();
        let tx2 = tx.clone();
        let first = ptys.open("run", cwd, &[("NEKO_VAR".into(), "meow".into())], Some("echo $NEKO_VAR; sleep 30"), 80, 24, |_| {}, move |g| { let _ = tx.send(g); }).unwrap();
        // Wait for output to land in the scrollback.
        for _ in 0..100 {
            if ptys.scrollback("run").unwrap_or_default().contains("meow") { break; }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(ptys.scrollback("run").unwrap().contains("meow"));

        // Restart: the first process is killed and its exit arrives late.
        let second = ptys.open("run", cwd, &[], Some("sleep 30"), 80, 24, |_| {}, move |g| { let _ = tx2.send(g); }).unwrap();
        assert_ne!(first, second);
        let ended = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(ended, first);
        ptys.close_if("run", ended);
        assert!(ptys.is_open("run"), "the old exit must not close the new run");
        ptys.close("run");
    }

    #[test]
    fn busy_counts_scripts_and_shells_running_a_command_but_not_idle_shells() {
        let d = tempfile::tempdir().unwrap();
        let cwd = d.path().to_str().unwrap();
        let ptys = Ptys::default();
        let (tx, rx) = mpsc::channel::<String>();
        ptys.open("sh", cwd, &[], None, 80, 24, move |s| { let _ = tx.send(s); }, |_| {}).unwrap();
        // Let the login shell settle at its prompt.
        let settle = |want: usize| {
            for _ in 0..100 {
                if ptys.busy() == want { return true; }
                std::thread::sleep(Duration::from_millis(100));
            }
            false
        };
        assert!(settle(0), "an idle shell must not count as busy");
        ptys.write("sh", "sleep 30\n").unwrap();
        assert!(settle(1), "a shell running sleep must count as busy");
        ptys.open("run", cwd, &[], Some("sleep 30"), 80, 24, |_| {}, |_| {}).unwrap();
        assert!(settle(2), "a running script counts as busy");
        ptys.close_all();
        assert_eq!(ptys.busy(), 0);
        assert!(!ptys.is_open("sh") && !ptys.is_open("run"));
        drop(rx);
    }

    #[test]
    fn close_prefix_closes_every_tab_of_one_workspace_and_no_other() {
        let d = tempfile::tempdir().unwrap();
        let cwd = d.path().to_str().unwrap();
        let ptys = Ptys::default();
        for id in ["w1:term", "w1:term-mh2k9x", "w1:run", "w10:term"] {
            ptys.open(id, cwd, &[], Some("sleep 30"), 80, 24, |_| {}, |_| {}).unwrap();
        }
        let pids: HashMap<String, u32> = ptys.pids().into_iter().collect();
        assert_eq!(ptys.close_prefix("w1:"), 3);
        assert!(!ptys.is_open("w1:term") && !ptys.is_open("w1:term-mh2k9x") && !ptys.is_open("w1:run"));
        assert!(ptys.is_open("w10:term"), "another workspace whose id starts the same keeps its terminal");
        // The processes are gone, not just forgotten.
        let alive = |pid: u32| unsafe { libc::kill(pid as i32, 0) } == 0;
        for _ in 0..100 {
            if !alive(pids["w1:term-mh2k9x"]) { break; }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!alive(pids["w1:term-mh2k9x"]), "the extra tab's process still runs");
        ptys.close_all();
    }
}
