//! System notifications: the banner macOS (or the desktop) shows when an agent is done or needs you.
//!
//! macOS draws a banner's icon from the app that sends it. Only an installed `NekoTrail.app` has an
//! icon macOS can find under our bundle identifier; a bare debug binary would borrow another app's
//! (Terminal, Finder) or get the blank placeholder. So on macOS a banner is sent only from inside a
//! `.app`, and never during an end-to-end run (`NEKOTRAIL_E2E`). Everything else the window asks for
//! still happens (sounds and Dock bounces are the window's, and the window checks `quiet`).
//!
//! The banner reads: title = the task, subtitle = `repo · branch`, body = what happened, with the
//! mascot on the right (`contentImage`, bundled from `notice/`).
//!
//! To check the icon: `bun tauri build --debug --bundles app`, open the `.app` it writes under
//! `<target>/debug/bundle/macos/`, then Settings, Notifications, Send a test notification. The
//! banner's icon is the app icon macOS registered for `dev.nekotrail.app`; if an older build with
//! the same identifier is registered elsewhere, macOS may cache its icon until that one is removed
//! (`lsregister -dump | grep dev.nekotrail.app` lists them). From `bun tauri dev` the test button
//! says it sends nothing, by design.

use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Manager};

/// Whether a banner goes out, and why not when it doesn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    Send,
    /// An end-to-end run: no banners, sounds or Dock bounces, ever.
    Test,
    /// macOS, not running from a `.app`: the banner would wear a generic icon.
    Unbundled,
}

/// Decides for a process: `e2e` from the test door's switch, `exe` the running binary.
pub fn gate(e2e: bool, exe: &Path, macos: bool) -> Gate {
    if e2e {
        Gate::Test
    } else if macos && !in_app_bundle(exe) {
        Gate::Unbundled
    } else {
        Gate::Send
    }
}

/// `…/NekoTrail.app/Contents/MacOS/nekotrail`.
fn in_app_bundle(exe: &Path) -> bool {
    let mut up = exe.ancestors().skip(1);
    let (Some(macos), Some(contents), Some(app)) = (up.next(), up.next(), up.next()) else { return false };
    macos.file_name().is_some_and(|n| n == "MacOS")
        && contents.file_name().is_some_and(|n| n == "Contents")
        && app.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"))
}

fn current_gate() -> Gate {
    let exe = std::env::current_exe().unwrap_or_default();
    gate(crate::e2e_mode(), &exe, cfg!(target_os = "macos"))
}

/// Emoji and pictographs: the banner has the app icon and the mascot, the text stays plain.
fn is_pictograph(c: char) -> bool {
    matches!(c as u32,
        0x231A..=0x23FF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE00..=0xFE0F
        | 0x1F000..=0x1FAFF | 0x200D | 0x20E3 | 0xE0020..=0xE007F)
}

/// One plain line: no emoji, no line breaks, collapsed spaces, at most `max` characters.
pub fn plain(s: &str, max: usize) -> String {
    let flat: String = s.chars().filter(|c| !is_pictograph(*c)).map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
    let words: Vec<&str> = flat.split(' ').filter(|w| !w.is_empty()).collect();
    let line = words.join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let cut: String = line.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub title: String,
    pub subtitle: String,
    pub body: String,
}

impl Banner {
    pub fn new(title: &str, subtitle: &str, body: &str) -> Self {
        let title = plain(title, 80);
        Banner { title: if title.is_empty() { "NekoTrail".into() } else { title }, subtitle: plain(subtitle, 80), body: plain(body, 180) }
    }
}

/// What the window learns once at startup.
#[derive(Serialize)]
pub struct Mode {
    /// Banners go out from this process.
    pub system: bool,
    /// Why not, when they don't: "test" or "unbundled".
    pub reason: Option<Gate>,
    /// An end-to-end run: the window plays no sound and bounces nothing either.
    pub quiet: bool,
}

#[tauri::command]
pub fn notice_mode() -> Mode {
    let g = current_gate();
    Mode { system: g == Gate::Send, reason: (g != Gate::Send).then_some(g), quiet: g == Gate::Test }
}

/// Shows a banner. `mood` picks the mascot: "happy" (done) or "idle" (needs you).
/// Returns the gate: `send` when it went out, else why it was skipped.
#[tauri::command]
pub fn system_notify(app: AppHandle, title: String, subtitle: String, body: String, mood: Option<String>) -> Gate {
    let g = current_gate();
    if g != Gate::Send {
        return g;
    }
    let b = Banner::new(&title, &subtitle, &body);
    let image = format!("notice/cat-{}.png", if mood.as_deref() == Some("idle") { "idle" } else { "happy" });
    let image = app.path().resolve(image, tauri::path::BaseDirectory::Resource).ok().filter(|p| p.exists());
    show(&app, b, image);
    g
}

#[cfg(target_os = "macos")]
fn show(app: &AppHandle, b: Banner, image: Option<std::path::PathBuf>) {
    let id = app.config().identifier.clone();
    // Off the main thread, as the notification plugin does: delivery can take a moment.
    let _ = std::thread::Builder::new().name("banner".into()).spawn(move || {
        // Once per process; whoever comes first (this or the plugin) sets the same identifier.
        let _ = mac_notification_sys::set_application(&id);
        let image = image.map(|p| p.to_string_lossy().into_owned());
        let mut n = mac_notification_sys::Notification::new();
        n.title(&b.title).message(&b.body).asynchronous(true);
        if !b.subtitle.is_empty() {
            n.subtitle(&b.subtitle);
        }
        if let Some(path) = image.as_deref() {
            n.content_image(path);
        }
        if let Err(err) = n.send() {
            eprintln!("notification: {err}");
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn show(app: &AppHandle, b: Banner, image: Option<std::path::PathBuf>) {
    use tauri_plugin_notification::NotificationExt;
    // Linux and Windows have no subtitle line: it leads the body instead.
    let body = if b.subtitle.is_empty() { b.body } else { format!("{}\n{}", b.subtitle, b.body) };
    let mut n = app.notification().builder().title(b.title).body(body);
    if let Some(p) = image {
        n = n.icon(p.to_string_lossy().into_owned());
    }
    if let Err(err) = n.show() {
        eprintln!("notification: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_installed_app_sends_on_macos() {
        let app = Path::new("/Applications/NekoTrail.app/Contents/MacOS/nekotrail");
        let debug = Path::new("/Users/me/.cache/nekotrail-target/debug/nekotrail");
        assert_eq!(gate(false, app, true), Gate::Send);
        assert_eq!(gate(false, debug, true), Gate::Unbundled);
        // A bundle built by `tauri build --debug` is a real .app too.
        assert_eq!(gate(false, Path::new("/t/debug/bundle/macos/NekoTrail.app/Contents/MacOS/nekotrail"), true), Gate::Send);
        // Something merely named like a bundle is not one.
        assert_eq!(gate(false, Path::new("/x/NekoTrail.app/nekotrail"), true), Gate::Unbundled);
        assert_eq!(gate(false, Path::new("nekotrail"), true), Gate::Unbundled);
        // Linux and Windows: their banners don't depend on a bundle.
        assert_eq!(gate(false, debug, false), Gate::Send);
    }

    #[test]
    fn an_e2e_run_never_sends_a_banner() {
        let app = Path::new("/Applications/NekoTrail.app/Contents/MacOS/nekotrail");
        assert_eq!(gate(true, app, true), Gate::Test);
        assert_eq!(gate(true, app, false), Gate::Test);
        assert_eq!(gate(true, Path::new("/t/debug/nekotrail"), true), Gate::Test);
    }

    #[test]
    fn the_test_door_is_closed_in_this_process_so_banners_follow_the_binary() {
        // `cargo test` runs without NEKOTRAIL_E2E: the gate is the bundle rule alone.
        assert!(!crate::e2e_mode());
        assert!(!notice_mode().quiet);
    }

    #[test]
    fn banner_text_is_plain() {
        let b = Banner::new("\u{2705} Fix the login redirect", "web · purr/fix-login", "The agent\nfinished   its turn.");
        assert_eq!(b, Banner {
            title: "Fix the login redirect".into(),
            subtitle: "web · purr/fix-login".into(),
            body: "The agent finished its turn.".into(),
        });
        assert_eq!(Banner::new("\u{23F3}", "", "x").title, "NekoTrail");
        let long = "word ".repeat(80);
        let body = Banner::new("t", "", &long).body;
        assert_eq!(body.chars().count(), 180);
        assert!(body.ends_with('…'));
    }
}
