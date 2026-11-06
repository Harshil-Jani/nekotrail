//! Real terminals (login shells in a PTY) for each workspace.

use anyhow::Result;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// Recent output, replayed when the terminal view is rebuilt (e.g. after switching workspace).
    scrollback: Arc<Mutex<String>>,
    /// Which start of this terminal id this is, so a late exit of an old process can't close a new one.
    generation: u64,
    /// Started to run a command (run, setup or a named action) rather than as an interactive shell.
    scripted: bool,
}

const SCROLLBACK: usize = 256 * 1024;
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
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let mut cmd = CommandBuilder::new(shell);
        match command {
            Some(c) => { cmd.args(["-lc", c]); }
            None => { cmd.arg("-l"); }
        }
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let scrollback = Arc::new(Mutex::new(String::new()));
        let sb = scrollback.clone();
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
        self.0.lock().unwrap().insert(id.to_string(), Pty { master: pair.master, writer, child, scrollback, generation, scripted: command.is_some() });
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

    pub fn write(&self, id: &str, data: &str) -> Result<()> {
        if let Some(p) = self.0.lock().unwrap().get_mut(id) {
            p.writer.write_all(data.as_bytes())?;
            p.writer.flush()?;
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

    pub fn is_open(&self, id: &str) -> bool {
        self.0.lock().unwrap().contains_key(id)
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
}
