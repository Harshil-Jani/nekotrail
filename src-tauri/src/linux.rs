//! Linux stand-ins for the macOS-only pieces: "Open in" through PATH and xdg-open, keeping the
//! computer awake through systemd-inhibit, and a clean environment for apps started from an AppImage.
//!
//! The module builds on every platform so `cargo test` on a Mac covers its logic too; only Linux
//! calls it.
#![cfg_attr(target_os = "macos", allow(dead_code))]

use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// How an app takes the folder to open.
#[derive(Clone, Copy)]
enum Folder {
    /// `code /path`
    Arg,
    /// `ghostty --working-directory=/path`
    Joined(&'static str),
    /// `konsole --workdir /path`
    Flag(&'static str),
}

/// Apps a workspace can open in on Linux: (id, name, commands to look for on PATH, folder style).
/// Ids match the macOS list where it is the same app, so the "Open in" setting carries over.
const OPEN_IN: &[(&str, &str, &[&str], Folder)] = &[
    ("cursor", "Cursor", &["cursor"], Folder::Arg),
    ("vscode", "Visual Studio Code", &["code"], Folder::Arg),
    ("zed", "Zed", &["zed", "zeditor"], Folder::Arg),
    ("windsurf", "Windsurf", &["windsurf"], Folder::Arg),
    ("intellij", "IntelliJ IDEA", &["idea", "intellij-idea-ultimate", "intellij-idea-community"], Folder::Arg),
    ("webstorm", "WebStorm", &["webstorm"], Folder::Arg),
    ("sublime", "Sublime Text", &["subl"], Folder::Arg),
    ("terminal", "Terminal", &["gnome-terminal", "kgx"], Folder::Joined("--working-directory=")),
    ("konsole", "Konsole", &["konsole"], Folder::Flag("--workdir")),
    ("ghostty", "Ghostty", &["ghostty"], Folder::Joined("--working-directory=")),
];

/// The first of `names` that is an executable file on `path` (a PATH-style list).
fn find_on(path: &std::ffi::OsStr, names: &[&str]) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    names.iter().find_map(|n| {
        std::env::split_paths(path)
            .map(|d| d.join(n))
            .find(|p| p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false))
    })
}

fn which(names: &[&str]) -> Option<PathBuf> {
    find_on(&std::env::var_os("PATH")?, names)
}

/// Which of those apps are installed, plus the file manager (id "finder", like on macOS).
pub fn open_in_targets() -> Vec<Value> {
    let mut v: Vec<Value> = OPEN_IN.iter().filter(|(_, _, bins, _)| which(bins).is_some()).map(|(k, n, _, _)| json!({"id": k, "name": n})).collect();
    v.push(json!({"id": "finder", "name": "Files"}));
    v
}

/// The command that opens `dir` with `app`, or an error the user can act on.
fn open_command(dir: &Path, app: &str) -> Result<Command, String> {
    if app == "finder" {
        let mut c = Command::new("xdg-open");
        c.arg(dir);
        return Ok(c);
    }
    let (_, name, bins, folder) = OPEN_IN.iter().find(|(k, ..)| *k == app).ok_or(format!("Unknown app: {app}. Pick another one in Settings."))?;
    let bin = which(bins).ok_or(format!("{name} isn't on your PATH. Pick another app in Settings."))?;
    let mut c = Command::new(bin);
    match folder {
        Folder::Arg => {
            c.arg(dir);
        }
        Folder::Joined(flag) => {
            let mut a = OsString::from(flag);
            a.push(dir);
            c.arg(a);
        }
        Folder::Flag(flag) => {
            c.arg(flag).arg(dir);
        }
    }
    Ok(c)
}

pub fn open_in(dir: &Path, app: &str) -> Result<(), String> {
    let mut c = open_command(dir, app)?;
    c.current_dir(dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    scrub_appimage_env(&mut c);
    let mut child = c.spawn().map_err(|e| e.to_string())?;
    // Reap it when it exits, so a long-lived terminal or editor never lingers as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// An AppImage points GTK, GIO and XDG paths at its own bundled libraries. Apps we start (an editor,
/// a terminal, the file manager) must not inherit those, or they load mismatched modules. Drops every
/// path entry under `$APPDIR` from the child's environment; does nothing outside an AppImage.
pub fn scrub_appimage_env(c: &mut Command) {
    let Some(appdir) = std::env::var_os("APPDIR").filter(|_| std::env::var_os("APPIMAGE").is_some()) else { return };
    for (k, v) in appimage_env_fixes(&appdir, std::env::vars_os()) {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
}

/// For each variable that mentions `appdir`: the value without those entries, or None to unset it.
fn appimage_env_fixes(appdir: &std::ffi::OsStr, vars: impl Iterator<Item = (OsString, OsString)>) -> Vec<(OsString, Option<OsString>)> {
    let appdir = appdir.to_string_lossy().into_owned();
    if appdir.is_empty() {
        return vec![];
    }
    vars.filter_map(|(k, v)| {
        let s = v.to_string_lossy();
        if !s.contains(&appdir) {
            return None;
        }
        let kept: Vec<&str> = s.split(':').filter(|p| !p.is_empty() && !p.contains(&appdir)).collect();
        Some((k, (!kept.is_empty()).then(|| OsString::from(kept.join(":")))))
    })
    .collect()
}

/// Keep the computer from idling to sleep while `pid` runs, like `caffeinate -i -w` on macOS.
/// Uses systemd-inhibit (logind's idle lock); on systems without it this quietly does nothing.
pub fn keep_awake_while(pid: u32) {
    let Some(inhibit) = which(&["systemd-inhibit"]) else { return };
    let Some(tail) = which(&["tail"]) else { return };
    let mut c = Command::new(inhibit);
    c.args(["--what=idle", "--who=nekotrail", "--why=An agent is working", "--mode=block"])
        .arg(tail)
        // GNU tail exits when that process does, which releases the lock.
        .args(["--pid", &pid.to_string(), "-f", "/dev/null"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Ok(mut child) = c.spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn finds_only_executables_in_path_order() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let mk = |d: &Path, n: &str, mode: u32| {
            let p = d.join(n);
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
            p
        };
        mk(a.path(), "zed", 0o644); // not executable: skipped
        let zb = mk(b.path(), "zed", 0o755);
        let path = std::env::join_paths([a.path(), b.path()]).unwrap();
        assert_eq!(find_on(&path, &["zed", "zeditor"]), Some(zb));
        let za = mk(a.path(), "zeditor", 0o755);
        // The first name wins over a later one, wherever it sits on PATH.
        assert_eq!(find_on(&path, &["zeditor", "zed"]), Some(za));
        assert_eq!(find_on(&path, &["code"]), None);
    }

    #[test]
    fn appimage_paths_are_dropped_and_other_entries_kept() {
        let vars = [
            ("XDG_DATA_DIRS", "/tmp/.mount_nekoX/usr/share:/usr/local/share:/usr/share"),
            ("GTK_PATH", "/tmp/.mount_nekoX/usr/lib/gtk-3.0"),
            ("HOME", "/home/me"),
            ("PATH", "/tmp/.mount_nekoX/usr/bin:/usr/bin"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let mut fixes = appimage_env_fixes("/tmp/.mount_nekoX".as_ref(), vars.into_iter());
        fixes.sort();
        assert_eq!(
            fixes,
            vec![
                ("GTK_PATH".into(), None),
                ("PATH".into(), Some("/usr/bin".into())),
                ("XDG_DATA_DIRS".into(), Some("/usr/local/share:/usr/share".into())),
            ]
        );
        assert!(appimage_env_fixes("".as_ref(), [("A".into(), "x".into())].into_iter()).is_empty());
    }

    #[test]
    fn open_commands_pass_the_folder_the_way_each_app_wants() {
        let dir = Path::new("/w/mochi");
        let args = |c: &Command| c.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let files = open_command(dir, "finder").unwrap();
        assert_eq!(files.get_program(), "xdg-open");
        assert_eq!(args(&files), ["/w/mochi"]);
        assert!(open_command(dir, "xcode").unwrap_err().contains("Unknown app"));
        // Folder styles, checked without needing the apps installed.
        let style = |id: &str| OPEN_IN.iter().find(|(k, ..)| *k == id).unwrap().3;
        assert!(matches!(style("vscode"), Folder::Arg));
        assert!(matches!(style("terminal"), Folder::Joined("--working-directory=")));
        assert!(matches!(style("konsole"), Folder::Flag("--workdir")));
    }
}
