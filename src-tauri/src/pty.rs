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
}

#[derive(Default, Clone)]
pub struct Ptys(Arc<Mutex<HashMap<String, Pty>>>);

impl Ptys {
    /// Start a login shell in `cwd`. `on_data` gets output chunks; `on_exit` fires once when it ends.
    pub fn open(
        &self,
        id: &str,
        cwd: &str,
        cols: u16,
        rows: u16,
        on_data: impl Fn(String) + Send + 'static,
        on_exit: impl FnOnce() + Send + 'static,
    ) -> Result<()> {
        self.close(id);
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let mut cmd = CommandBuilder::new(shell);
        cmd.arg("-l");
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
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
                        on_data(String::from_utf8_lossy(&chunk).into_owned());
                    }
                }
            }
            on_exit();
        });
        self.0.lock().unwrap().insert(id.to_string(), Pty { master: pair.master, writer, child });
        Ok(())
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
        ptys.open("t", d.path().to_str().unwrap(), 80, 24, move |s| { let _ = tx.send(s); }, || {}).unwrap();
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
}
