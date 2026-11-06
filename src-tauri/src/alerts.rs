//! Away alerts: short messages to your phone or team chat when something happens that matters
//! while you're away from the Mac. Off until you add a destination (Settings, Notifications).
//!
//! - Destinations: ntfy, Slack and Discord webhooks, a generic JSON webhook, Pushover. Each picks
//!   its events and whether to include the agent's last line.
//! - What a message says: the workspace title, its repo, what happened and a `nekotrail://` link
//!   that opens it on the Mac. Never code, file contents, prompts, commands or diffs. The agent's
//!   last line only when you turn it on for that destination, with secrets hidden as in an export.
//! - Secrets (webhook URLs, topics, tokens, user keys) live in the OS keychain (`SecretStore`),
//!   never in the database or a log. The database keeps only the host, for the Settings list.
//! - Bursts are gathered into one message (`Outbox`), each destination gets at most one message a
//!   minute and 20 an hour, and quiet hours hold messages until they end.
//! - Sending happens on its own thread with a 10 second timeout. A failure is kept as the
//!   destination's last error for Settings and never reaches a turn.
//!
//! Everything but the glue at the bottom is pure and tested with a fake network and a fake
//! keychain, so tests never send anything anywhere.

use crate::{e, redact, AppState, Res};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const SETTING: &str = "alerts";
const STATUS_SETTING: &str = "alerts.status";
const MORNING_SETTING: &str = "alerts.morning";
const KEYCHAIN_SERVICE: &str = "dev.nekotrail.app.alerts";

/// A burst is gathered until this many seconds pass with nothing new...
pub const GATHER: i64 = 20;
/// ...but never held longer than this after its first alert.
pub const HOLD_MAX: i64 = 60;
/// The shortest time between two messages to one destination.
pub const MIN_GAP: i64 = 60;
/// The most messages one destination gets in an hour. More are held and go out together.
pub const PER_HOUR: usize = 20;
/// The most alerts one destination holds (quiet hours, a failing service). Older ones are counted.
pub const MAX_PENDING: usize = 50;
/// How much of the agent's last line goes out.
pub const LAST_LINE_MAX: usize = 160;
/// Lines a gathered message lists before "and N more".
const DIGEST_LINES: usize = 6;
const TIMEOUT: Duration = Duration::from_secs(10);

/// Lock a mutex even if a thread panicked holding it: alerts must never take the app down.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------- settings ----------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// A plan to approve, a question, a tool call to allow, a sign-in, an error.
    NeedsYou,
    TurnFinished,
    ChecksFailed,
    ChecksGreen,
    PrMerged,
    QueueDrained,
    CapReached,
    Runaway,
    MorningSummary,
}

impl Event {
    pub const ALL: [Event; 9] = [
        Event::NeedsYou, Event::TurnFinished, Event::ChecksFailed, Event::ChecksGreen, Event::PrMerged,
        Event::QueueDrained, Event::CapReached, Event::Runaway, Event::MorningSummary,
    ];

    /// What a new destination gets. Turn finished and green checks are the noisy ones.
    pub fn default_on(self) -> bool {
        !matches!(self, Event::TurnFinished | Event::ChecksGreen)
    }

    /// Something stopped and waits for you: high priority where the service has one.
    pub fn urgent(self) -> bool {
        matches!(self, Event::NeedsYou | Event::ChecksFailed | Event::Runaway | Event::CapReached)
    }

    pub fn key(self) -> &'static str {
        match self {
            Event::NeedsYou => "needs_you",
            Event::TurnFinished => "turn_finished",
            Event::ChecksFailed => "checks_failed",
            Event::ChecksGreen => "checks_green",
            Event::PrMerged => "pr_merged",
            Event::QueueDrained => "queue_drained",
            Event::CapReached => "cap_reached",
            Event::Runaway => "runaway",
            Event::MorningSummary => "morning_summary",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    Ntfy,
    Slack,
    Discord,
    Webhook,
    Pushover,
}

impl Service {
    pub fn label(self) -> &'static str {
        match self {
            Service::Ntfy => "ntfy",
            Service::Slack => "Slack",
            Service::Discord => "Discord",
            Service::Webhook => "Webhook",
            Service::Pushover => "Pushover",
        }
    }
}

fn yes() -> bool {
    true
}

/// One place alerts go. Its secret is in the keychain under `id`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Destination {
    pub id: String,
    pub service: Service,
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub on: bool,
    #[serde(default)]
    pub events: Vec<Event>,
    /// Add the agent's last line (secrets hidden). Off by default.
    #[serde(default)]
    pub last_line: bool,
    /// Set by nekotrail, never by the window: a secret is saved for it, and the host it goes to.
    #[serde(default)]
    pub saved: bool,
    #[serde(default)]
    pub hint: String,
}

impl Destination {
    pub fn new(id: String, service: Service) -> Self {
        Self {
            id, service, name: service.label().to_string(), on: true,
            events: Event::ALL.into_iter().filter(|e| e.default_on()).collect(),
            last_line: false, saved: false, hint: String::new(),
        }
    }
    pub fn wants(&self, ev: Event) -> bool {
        self.on && self.saved && self.events.contains(&ev)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Quiet {
    #[serde(default)]
    pub on: bool,
    /// "22:00", local time.
    pub from: String,
    pub to: String,
}

impl Default for Quiet {
    fn default() -> Self {
        Self { on: false, from: "22:00".into(), to: "07:00".into() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub destinations: Vec<Destination>,
    #[serde(default)]
    pub quiet: Quiet,
    /// When the morning summary goes out, local time.
    #[serde(default = "morning_default")]
    pub morning: String,
}

fn morning_default() -> String {
    "08:00".into()
}

impl Default for Config {
    fn default() -> Self {
        Self { destinations: vec![], quiet: Quiet::default(), morning: morning_default() }
    }
}

impl Config {
    pub fn from_json(s: Option<&str>) -> Self {
        s.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    pub fn wants(&self, ev: Event) -> bool {
        self.destinations.iter().any(|d| d.wants(ev))
    }
    pub fn wants_last_line(&self, ev: Event) -> bool {
        self.destinations.iter().any(|d| d.wants(ev) && d.last_line)
    }

    /// What the window sent, checked, with the fields only nekotrail sets taken from `self`.
    pub fn merge_from_window(&self, mut next: Config) -> Result<Config, String> {
        for t in [&next.quiet.from, &next.quiet.to, &next.morning] {
            minutes(t).ok_or_else(|| format!("\"{t}\" isn't a time. Use 24-hour time like 07:30."))?;
        }
        next.destinations.retain(|d| self.destinations.iter().any(|o| o.id == d.id));
        for d in &mut next.destinations {
            let old = self.destinations.iter().find(|o| o.id == d.id).expect("kept above");
            d.service = old.service;
            d.saved = old.saved;
            d.hint = old.hint.clone();
            d.name = d.name.trim().chars().take(40).collect();
            if d.name.is_empty() {
                d.name = d.service.label().to_string();
            }
            let mut seen = vec![];
            d.events.retain(|e| if seen.contains(e) { false } else { seen.push(*e); true });
        }
        Ok(next)
    }
}

/// "07:30" to minutes after midnight.
pub fn minutes(t: &str) -> Option<u32> {
    let (h, m) = t.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Inside quiet hours at `now` (minutes after midnight)? From 22:00 to 07:00 wraps midnight.
/// The same time for both means no quiet hours.
pub fn is_quiet(q: &Quiet, now: u32) -> bool {
    let (Some(from), Some(to)) = (minutes(&q.from), minutes(&q.to)) else { return false };
    if !q.on || from == to {
        return false;
    }
    if from < to { now >= from && now < to } else { now >= from || now < to }
}

// ---------- secrets ----------

/// What a destination needs to send, kept in the keychain as JSON. Which fields count depends on
/// the service: ntfy (topic URL, optional token), Slack and Discord (webhook URL), webhook (URL,
/// optional bearer token), Pushover (app token and user key).
#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct Secret {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub user: String,
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(hidden)")
    }
}

impl Secret {
    fn trimmed(&self) -> Secret {
        Secret { url: self.url.trim().into(), token: self.token.trim().into(), user: self.user.trim().into() }
    }
    fn parts(&self) -> [&str; 3] {
        [&self.url, &self.token, &self.user]
    }
}

/// Checks a secret before it is saved. Returns what Settings may show: the host, never the path.
pub fn check_secret(service: Service, s: &Secret) -> Result<String, String> {
    let url = |what: &str| -> Result<reqwest::Url, String> {
        let u = reqwest::Url::parse(&s.url).map_err(|_| format!("Paste the full {what}, starting with https://."))?;
        if !matches!(u.scheme(), "https" | "http") || u.host_str().is_none() {
            return Err(format!("Paste the full {what}, starting with https://."));
        }
        if !u.username().is_empty() || u.password().is_some() {
            return Err("Put the password in the token field, not in the address.".into());
        }
        Ok(u)
    };
    let token_ok = |t: &str| t.chars().all(|c| c.is_ascii_graphic());
    if !token_ok(&s.token) || !token_ok(&s.user) {
        return Err("Tokens and keys can't contain spaces.".into());
    }
    let host = match service {
        Service::Ntfy => {
            let u = url("topic URL")?;
            ntfy_split(&s.url).ok_or("Add the topic at the end of the URL, like https://ntfy.sh/my-topic.")?;
            u.host_str().unwrap_or("").to_string()
        }
        Service::Slack => {
            let u = url("webhook URL")?;
            if u.scheme() != "https" || u.host_str() != Some("hooks.slack.com") {
                return Err("A Slack incoming webhook starts with https://hooks.slack.com/.".into());
            }
            "hooks.slack.com".into()
        }
        Service::Discord => {
            let u = url("webhook URL")?;
            let host = u.host_str().unwrap_or("");
            if u.scheme() != "https" || !["discord.com", "discordapp.com", "ptb.discord.com", "canary.discord.com"].contains(&host) || !u.path().starts_with("/api/webhooks/") {
                return Err("A Discord webhook starts with https://discord.com/api/webhooks/.".into());
            }
            host.to_string()
        }
        Service::Webhook => url("webhook URL")?.host_str().unwrap_or("").to_string(),
        Service::Pushover => {
            let key = |k: &str| k.len() == 30 && k.chars().all(|c| c.is_ascii_alphanumeric());
            if !key(&s.token) {
                return Err("The Pushover app token is 30 letters and digits.".into());
            }
            if !key(&s.user) {
                return Err("The Pushover user key is 30 letters and digits.".into());
            }
            "api.pushover.net".into()
        }
    };
    Ok(host)
}

/// "https://ntfy.sh/my-topic" to ("https://ntfy.sh", "my-topic"). ntfy publishes JSON to its root.
fn ntfy_split(url: &str) -> Option<(String, String)> {
    let url = url.trim().trim_end_matches('/');
    let (base, topic) = url.rsplit_once('/')?;
    let valid = !topic.is_empty() && topic.len() <= 64 && topic.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    (valid && base.contains("://") && !base.ends_with(':') && !base.ends_with('/')).then(|| (base.to_string(), topic.to_string()))
}

/// Where secrets live. The app uses the OS keychain; tests use `MemSecrets`.
pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<Secret>, String>;
    fn set(&self, id: &str, secret: &Secret) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
}

/// macOS Keychain or the Linux Secret Service, through the keyring crate.
pub struct Keychain;

impl Keychain {
    fn entry(id: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, id).map_err(|err| keychain_error(&err))
    }
}

fn keychain_error(err: &keyring::Error) -> String {
    match err {
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
            if cfg!(target_os = "linux") {
                "Couldn't reach the keyring. Unlock GNOME Keyring or KWallet (any Secret Service) and try again.".into()
            } else {
                "Couldn't reach the keychain. Unlock it and try again.".into()
            }
        }
        _ => "The keychain couldn't keep this secret.".into(),
    }
}

impl SecretStore for Keychain {
    fn get(&self, id: &str) -> Result<Option<Secret>, String> {
        match Self::entry(id)?.get_password() {
            Ok(s) => Ok(serde_json::from_str(&s).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(keychain_error(&err)),
        }
    }
    fn set(&self, id: &str, secret: &Secret) -> Result<(), String> {
        let json = serde_json::to_string(secret).map_err(e)?;
        Self::entry(id)?.set_password(&json).map_err(|err| keychain_error(&err))
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        match Self::entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(keychain_error(&err)),
        }
    }
}

/// A keychain in memory, for tests.
#[derive(Default)]
pub struct MemSecrets(pub Mutex<HashMap<String, Secret>>);

impl SecretStore for MemSecrets {
    fn get(&self, id: &str) -> Result<Option<Secret>, String> {
        Ok(lock(&self.0).get(id).cloned())
    }
    fn set(&self, id: &str, secret: &Secret) -> Result<(), String> {
        lock(&self.0).insert(id.into(), secret.clone());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        lock(&self.0).remove(id);
        Ok(())
    }
}

/// Saves a destination's secret and marks it saved, after checking it.
pub fn save_secret(cfg: &mut Config, store: &dyn SecretStore, id: &str, secret: &Secret) -> Result<(), String> {
    let d = cfg.destinations.iter_mut().find(|d| d.id == id).ok_or("That destination is gone.")?;
    let secret = secret.trimmed();
    let hint = check_secret(d.service, &secret)?;
    store.set(id, &secret)?;
    d.saved = true;
    d.hint = hint;
    Ok(())
}

// ---------- messages ----------

/// One thing that happened. Holds no code, prompt or command; `last_line` is already masked and
/// only goes out to destinations that asked for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub event: Event,
    pub workspace_id: Option<String>,
    /// The workspace's title, or "" for alerts about the whole queue.
    pub workspace: String,
    pub repo: String,
    /// What happened, as a short sentence: "has a plan for you to approve", "8 done, 1 failed".
    pub what: String,
    pub last_line: Option<String>,
    pub at: i64,
}

pub fn link(workspace_id: &str) -> String {
    let enc: String = workspace_id.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    format!("nekotrail://open?workspace={enc}")
}

/// What a service gets, before its own format.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub title: String,
    pub body: String,
    pub link: Option<String>,
    pub urgent: bool,
    /// The event when the message is about one kind of thing; None for a mixed burst.
    pub event: Option<Event>,
    pub items: Vec<Alert>,
    pub last_line: bool,
}

fn line_of(a: &Alert) -> String {
    match (a.workspace.is_empty(), a.repo.is_empty()) {
        (true, _) => a.what.clone(),
        (false, true) => format!("{} {}", a.workspace, a.what),
        (false, false) => format!("{} ({}) {}", a.workspace, a.repo, a.what),
    }
}

fn title_of(a: &Alert) -> String {
    match a.event {
        Event::QueueDrained => "The queue is done".into(),
        Event::MorningSummary => "Overnight in nekotrail".into(),
        Event::CapReached if a.workspace.is_empty() => "A spend cap stopped the queue".into(),
        _ if a.workspace.is_empty() => a.what.clone(),
        _ => format!("{} {}", a.workspace, a.what),
    }
}

/// One alert becomes its own message; a burst becomes one message that lists them.
pub fn compose(items: &[Alert], last_line: bool) -> Message {
    let urgent = items.iter().any(|a| a.event.urgent());
    let first = items.first().cloned().unwrap_or(Alert {
        event: Event::TurnFinished, workspace_id: None, workspace: String::new(), repo: String::new(), what: String::new(), last_line: None, at: 0,
    });
    let one_event = items.iter().all(|a| a.event == first.event).then_some(first.event);
    let one_workspace = items.iter().all(|a| a.workspace_id == first.workspace_id).then(|| first.workspace_id.clone()).flatten();
    if items.len() <= 1 {
        let mut body = vec![];
        let where_ = match (first.workspace.is_empty(), first.repo.is_empty()) {
            (false, false) => format!("{} in {}", first.workspace, first.repo),
            (false, true) => first.workspace.clone(),
            _ => String::new(),
        };
        match first.event {
            Event::QueueDrained | Event::MorningSummary => body.push(first.what.clone()),
            Event::CapReached if first.workspace.is_empty() => body.push(first.what.clone()),
            _ => body.push(where_),
        }
        if last_line && let Some(l) = first.last_line.as_ref().filter(|l| !l.is_empty()) {
            body.push(format!("Last line: {l}"));
        }
        return Message {
            title: title_of(&first), body: body.into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n"),
            link: first.workspace_id.as_deref().map(link), urgent, event: Some(first.event), items: items.to_vec(), last_line,
        };
    }
    let n = items.len();
    let title = match one_event {
        Some(Event::NeedsYou) => format!("{n} workspaces need you"),
        Some(Event::TurnFinished) => format!("{n} turns finished"),
        Some(Event::ChecksFailed) => format!("Checks failed on {n} PRs"),
        Some(Event::ChecksGreen) => format!("Checks are green on {n} PRs"),
        Some(Event::PrMerged) => format!("{n} PRs merged"),
        _ => format!("{n} updates from nekotrail"),
    };
    let mut lines: Vec<String> = items.iter().take(DIGEST_LINES).map(|a| format!("- {}", line_of(a))).collect();
    if n > DIGEST_LINES {
        lines.push(format!("and {} more", n - DIGEST_LINES));
    }
    Message { title, body: lines.join("\n"), link: one_workspace.as_deref().map(link), urgent, event: one_event, items: items.to_vec(), last_line }
}

/// One HTTP request, built but not sent.
#[derive(Clone, PartialEq)]
pub struct Req {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl std::fmt::Debug for Req {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Req(hidden)")
    }
}

/// Slack's three reserved characters.
fn slack_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn form(pairs: &[(&str, String)]) -> Vec<u8> {
    let enc = |s: &str| -> String {
        s.bytes().map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        }).collect()
    };
    pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&").into_bytes()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max - 1).collect::<String>()) }
}

const JSON: (&str, &str) = ("Content-Type", "application/json");

/// The request a service gets for a message.
pub fn request(service: Service, s: &Secret, m: &Message) -> Result<Req, String> {
    let text = |sep: &str| -> String {
        let mut t = m.body.clone();
        if let Some(l) = &m.link {
            t.push_str(&format!("{sep}Open on your Mac: {l}"));
        }
        t
    };
    let bearer = |h: &mut Vec<(String, String)>| {
        if !s.token.is_empty() {
            h.push(("Authorization".into(), format!("Bearer {}", s.token)));
        }
    };
    let mut headers = vec![(JSON.0.to_string(), JSON.1.to_string())];
    let (url, body) = match service {
        Service::Ntfy => {
            let (base, topic) = ntfy_split(&s.url).ok_or("The ntfy topic URL is incomplete.")?;
            bearer(&mut headers);
            let mut b = json!({"topic": topic, "title": clip(&m.title, 200), "message": clip(&m.body, 3000), "priority": if m.urgent { 4 } else { 3 }});
            if let Some(l) = &m.link {
                b["click"] = json!(l);
            }
            (base, b)
        }
        Service::Slack => (s.url.clone(), json!({"text": clip(&format!("*{}*\n{}", slack_escape(&m.title), slack_escape(&text("\n"))), 3000)})),
        Service::Discord => (s.url.clone(), json!({
            "username": "nekotrail", "content": clip(&format!("**{}**\n{}", m.title, text("\n")), 2000),
            // A workspace called "@everyone" pings nobody.
            "allowed_mentions": {"parse": []},
        })),
        Service::Webhook => {
            bearer(&mut headers);
            let items: Vec<Value> = m.items.iter().map(|a| {
                let mut v = json!({"event": a.event.key(), "workspace": a.workspace, "workspace_id": a.workspace_id, "repo": a.repo, "text": a.what, "at": a.at});
                if let Some(id) = &a.workspace_id {
                    v["link"] = json!(link(id));
                }
                if m.last_line && let Some(l) = &a.last_line {
                    v["last_line"] = json!(l);
                }
                v
            }).collect();
            (s.url.clone(), json!({
                "source": "nekotrail", "event": m.event.map(|e| e.key()).unwrap_or("several"),
                "title": m.title, "message": m.body, "link": m.link, "urgent": m.urgent, "items": items,
            }))
        }
        Service::Pushover => {
            let mut pairs = vec![("token", s.token.clone()), ("user", s.user.clone()), ("title", clip(&m.title, 250)), ("message", clip(&m.body, 1024)), ("priority", if m.urgent { "1" } else { "0" }.to_string())];
            if let Some(l) = &m.link {
                pairs.push(("url", l.clone()));
                pairs.push(("url_title", "Open in nekotrail".into()));
            }
            return Ok(Req { url: "https://api.pushover.net/1/messages.json".into(), headers: vec![("Content-Type".into(), "application/x-www-form-urlencoded".into())], body: form(&pairs) });
        }
    };
    Ok(Req { url, headers, body: serde_json::to_vec(&body).map_err(e)? })
}

/// The message "Send a test" sends.
pub fn test_message() -> Message {
    Message {
        title: "nekotrail test".into(),
        body: "Alerts from nekotrail reach you here. Real ones say which workspace and what happened, never your code.".into(),
        link: None, urgent: false, event: None, items: vec![], last_line: false,
    }
}

/// An error made safe to show and keep: no secret, no address, short.
pub fn clean_error(err: &str, s: &Secret) -> String {
    let mut out = err.to_string();
    for p in s.parts() {
        if p.len() >= 4 {
            out = out.replace(p, "[hidden]");
        }
    }
    clip(&redact::mask(&out), 200)
}

/// Sends one request. The app uses `Http`; tests record requests instead.
pub trait Transport: Send + Sync {
    fn post(&self, req: &Req) -> Result<(), String>;
}

/// Sends with reqwest: no redirects (a redirect could carry the request elsewhere), 10 s timeout.
pub struct Http;

impl Transport for Http {
    fn post(&self, req: &Req) -> Result<(), String> {
        // Same TLS setup the updater uses; installing twice is harmless.
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let req = req.clone();
        tauri::async_runtime::block_on(async move {
            let client = reqwest::Client::builder()
                .timeout(TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("nekotrail/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|err| err.without_url().to_string())?;
            let mut rb = client.post(&req.url);
            for (k, v) in &req.headers {
                rb = rb.header(k, v);
            }
            let resp = rb.body(req.body).send().await.map_err(|err| {
                if err.is_timeout() {
                    "No answer in 10 seconds.".to_string()
                } else if err.is_connect() {
                    "Couldn't connect. Check the address and your network.".to_string()
                } else {
                    err.without_url().to_string()
                }
            })?;
            let status = resp.status();
            if status.is_success() {
                return Ok(());
            }
            let said: String = resp.text().await.unwrap_or_default().chars().take(120).collect();
            let said = said.split_whitespace().collect::<Vec<_>>().join(" ");
            Err(if said.is_empty() { format!("The service answered {status}.") } else { format!("The service answered {status}: {said}") })
        })
    }
}

/// Builds and sends a burst to one destination. The error is already cleaned.
pub fn deliver(d: &Destination, items: &[Alert], store: &dyn SecretStore, net: &dyn Transport) -> Result<(), String> {
    send_message(d, &compose(items, d.last_line), store, net)
}

pub fn send_message(d: &Destination, m: &Message, store: &dyn SecretStore, net: &dyn Transport) -> Result<(), String> {
    let s = store.get(&d.id)?.ok_or("Its address or token is missing. Add it again.")?;
    let req = request(d.service, &s, m).map_err(|err| clean_error(&err, &s))?;
    net.post(&req).map_err(|err| clean_error(&err, &s))
}

// ---------- gathering and rate limits ----------

/// One destination's waiting alerts and when it last sent. Times are epoch seconds.
#[derive(Default, Debug)]
pub struct Outbox {
    pending: Vec<Alert>,
    first_at: i64,
    last_at: i64,
    sent: VecDeque<i64>,
    /// Alerts dropped because too many were waiting (still counted in "and N more").
    dropped: usize,
}

impl Outbox {
    pub fn push(&mut self, a: Alert, now: i64) {
        if self.pending.is_empty() {
            self.first_at = now;
        }
        self.last_at = now;
        // The same thing about the same workspace again: keep the newest.
        self.pending.retain(|p| !(p.event == a.event && p.workspace_id.is_some() && p.workspace_id == a.workspace_id));
        self.pending.push(a);
        if self.pending.len() > MAX_PENDING {
            self.pending.remove(0);
            self.dropped += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.pending.len() + self.dropped
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// The alerts to send now, if any: the burst has settled (or waited long enough), quiet hours
    /// are over, and the destination is under its rate limits.
    pub fn take_due(&mut self, now: i64, quiet: bool) -> Option<Vec<Alert>> {
        while self.sent.front().is_some_and(|t| now - t >= 3600) {
            self.sent.pop_front();
        }
        if self.pending.is_empty() || quiet {
            return None;
        }
        let settled = now - self.last_at >= GATHER || now - self.first_at >= HOLD_MAX;
        let gap = self.sent.back().is_none_or(|t| now - t >= MIN_GAP);
        if !settled || !gap || self.sent.len() >= PER_HOUR {
            return None;
        }
        self.sent.push_back(now);
        let mut out = std::mem::take(&mut self.pending);
        if self.dropped > 0 {
            out.insert(0, Alert {
                event: Event::NeedsYou, workspace_id: None, workspace: String::new(), repo: String::new(),
                what: format!("{} older updates were left out", self.dropped), last_line: None, at: now,
            });
            out[0].event = out.get(1).map(|a| a.event).unwrap_or(Event::NeedsYou);
            self.dropped = 0;
        }
        Some(out)
    }
}

/// The destinations and their outboxes.
#[derive(Default, Debug)]
pub struct Engine {
    pub config: Config,
    outboxes: HashMap<String, Outbox>,
}

impl Engine {
    pub fn new(config: Config) -> Self {
        Self { config, outboxes: HashMap::new() }
    }

    /// Queue an alert for every destination that wants it. Returns how many.
    pub fn fire(&mut self, a: &Alert, now: i64) -> usize {
        let mut n = 0;
        for d in self.config.destinations.iter().filter(|d| d.wants(a.event)) {
            let mut a = a.clone();
            if !d.last_line {
                a.last_line = None;
            }
            self.outboxes.entry(d.id.clone()).or_default().push(a, now);
            n += 1;
        }
        n
    }

    /// What to send now, per destination.
    pub fn due(&mut self, now: i64, quiet: bool) -> Vec<(Destination, Vec<Alert>)> {
        let ids: Vec<String> = self.config.destinations.iter().map(|d| d.id.clone()).collect();
        self.outboxes.retain(|id, _| ids.contains(id));
        let mut out = vec![];
        for d in &self.config.destinations {
            if let Some(ob) = self.outboxes.get_mut(&d.id)
                && let Some(items) = ob.take_due(now, quiet) {
                    out.push((d.clone(), items));
                }
        }
        out
    }

    pub fn held(&self, id: &str) -> usize {
        self.outboxes.get(id).map(Outbox::len).unwrap_or(0)
    }
}

// ---------- what each event says ----------

/// The sentence for an agent that stopped and needs you, by the reason the turn gave.
pub fn needs_text(reason: &str) -> &'static str {
    match reason {
        "plan" => "has a plan for you to approve",
        "question" => "has a question for you",
        "approval" => "wants your OK",
        "login" => "needs you to sign in",
        "limit" => "hit its usage limit",
        "install" => "can't find its agent CLI",
        "error" => "stopped with an error",
        "protected" => "changed a protected file",
        "checks" => "has a check that fails now",
        "gave_up" => "needs you: autopilot handed it back",
        _ => "needs you",
    }
}

/// "pass", "fail" or "wait" for a PR's checks; None with no checks.
fn checks_outcome(pr: &Value) -> Option<&'static str> {
    let checks = pr["statusCheckRollup"].as_array().filter(|c| !c.is_empty())?;
    let result = |c: &Value| c["conclusion"].as_str().filter(|s| !s.is_empty()).or(c["state"].as_str()).or(c["status"].as_str()).unwrap_or("").to_uppercase();
    let failing = checks.iter().any(|c| ["FAILURE", "ERROR", "TIMED_OUT", "STARTUP_FAILURE", "CANCELLED", "ACTION_REQUIRED"].contains(&result(c).as_str()));
    let pending = checks.iter().any(|c| ["", "PENDING", "QUEUED", "IN_PROGRESS", "EXPECTED", "WAITING", "REQUESTED"].contains(&result(c).as_str()));
    Some(if pending { "wait" } else if failing { "fail" } else { "pass" })
}

/// What changed between two looks at a workspace's PR that is worth an alert. Nothing on the
/// first look after nekotrail starts, so a restart doesn't repeat old news.
pub fn pr_alert(before: Option<&Value>, after: &Value) -> Option<(Event, String)> {
    let (b, a) = (before.map(|v| &v["pr"]).filter(|p| p.is_object())?, after["pr"].as_object().map(|_| &after["pr"])?);
    if b["number"] != a["number"] {
        return None;
    }
    let num = a["number"].as_u64().map(|n| format!("#{n}")).unwrap_or_else(|| "its PR".into());
    if a["state"] == "MERGED" && b["state"] != "MERGED" {
        return Some((Event::PrMerged, format!("merged {num}")));
    }
    if a["state"] != "OPEN" {
        return None;
    }
    let (was, now) = (checks_outcome(b), checks_outcome(a)?);
    let new_head = a["headRefOid"] != b["headRefOid"];
    if was == Some(now) && !new_head {
        return None;
    }
    match now {
        "fail" => Some((Event::ChecksFailed, format!("has failing checks on {num}"))),
        "pass" => Some((Event::ChecksGreen, format!("has green checks on {num}"))),
        _ => None,
    }
}

/// The agent's last line of text in a chat, short, with secrets hidden and home as `~`.
pub fn last_line_of(events: &[Value], home: &str) -> Option<String> {
    let text = events.iter().rev().filter(|ev| ev["kind"] == "text").find_map(|ev| {
        ev["text"].as_str()?.lines().rev().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("```")).map(str::to_string)
    })?;
    let plain = text.trim_start_matches(['#', '-', '*', '>', ' ']).replace(['`', '*'], "");
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    // Mask before shortening, so a cut can't leave half a token showing.
    let masked = redact::shorten_home(&redact::mask(&plain), home);
    (!masked.is_empty()).then(|| clip(&masked, LAST_LINE_MAX))
}

// ---------- the app ----------

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct Status {
    /// Epoch seconds.
    pub last_ok: Option<i64>,
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
}

struct Live {
    engine: Mutex<Engine>,
    status: Mutex<HashMap<String, Status>>,
    /// When the queue last started having work, while it has some.
    queue_busy_since: Mutex<Option<i64>>,
}

static LIVE: LazyLock<Live> = LazyLock::new(|| Live {
    engine: Mutex::new(Engine::default()),
    status: Mutex::new(HashMap::new()),
    queue_busy_since: Mutex::new(None),
});

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn local_minute() -> u32 {
    use chrono::Timelike;
    let t = chrono::Local::now();
    t.hour() * 60 + t.minute()
}

/// Load the settings and start the loop that sends what is due (every 5 seconds).
pub fn start(app: AppHandle) {
    {
        let st = app.state::<AppState>();
        let db = st.db();
        lock(&LIVE.engine).config = Config::from_json(db.setting(SETTING).ok().flatten().as_deref());
        *lock(&LIVE.status) = db.setting(STATUS_SETTING).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    }
    std::thread::spawn(move || loop {
        flush(&app);
        morning(&app);
        std::thread::sleep(Duration::from_secs(5));
    });
}

fn flush(app: &AppHandle) {
    let quiet = { let eng = lock(&LIVE.engine); is_quiet(&eng.config.quiet, local_minute()) };
    let due = lock(&LIVE.engine).due(now(), quiet);
    for (d, items) in due {
        let app = app.clone();
        std::thread::spawn(move || {
            let r = deliver(&d, &items, &Keychain, &Http);
            record(&app, &d.id, r);
        });
    }
}

fn record(app: &AppHandle, id: &str, r: Result<(), String>) {
    let snapshot = {
        let mut all = lock(&LIVE.status);
        let s = all.entry(id.to_string()).or_default();
        match r {
            Ok(()) => { s.last_ok = Some(now()); s.last_error = None; s.last_error_at = None; }
            Err(err) => { s.last_error = Some(err); s.last_error_at = Some(now()); }
        }
        serde_json::to_string(&*all).unwrap_or_default()
    };
    let _ = app.state::<AppState>().db().set_setting(STATUS_SETTING, &snapshot);
    let _ = app.emit("alerts-status", ());
}

/// Something happened. Cheap when no destination wants this event. Never blocks on the network.
pub fn fire(app: &AppHandle, event: Event, workspace_id: Option<&str>, session_id: Option<&str>, what: &str) {
    let (wants, wants_line) = { let eng = lock(&LIVE.engine); (eng.config.wants(event), eng.config.wants_last_line(event)) };
    if !wants {
        return;
    }
    let st = app.state::<AppState>();
    let (mut workspace, mut repo, mut line) = (String::new(), String::new(), None);
    if let Some(id) = workspace_id {
        let db = st.db();
        if let Ok(w) = db.workspace(id) {
            workspace = redact::mask(if w.title.trim().is_empty() { &w.name } else { w.title.trim() });
            workspace = clip(&workspace, 80);
            repo = db.repo(&w.repo_id).map(|r| r.name).unwrap_or_default();
            if wants_line {
                let session = session_id.map(str::to_string).or_else(|| db.sessions(id).ok().and_then(|s| s.last().map(|s| s.id.clone())));
                if let Some(sid) = session {
                    let (events, _) = db.events_tail(&sid, 40).unwrap_or_default();
                    let home = dirs::home_dir().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default();
                    line = last_line_of(&events, &home);
                }
            }
        }
    }
    let a = Alert { event, workspace_id: workspace_id.map(str::to_string), workspace, repo, what: what.to_string(), last_line: line, at: now() };
    lock(&LIVE.engine).fire(&a, now());
}

/// Called on each queue tick: says "the queue is done" once it runs out of work.
pub fn queue_tick(app: &AppHandle, busy: bool, summary_since: impl FnOnce(i64) -> String) {
    let since = {
        let mut b = lock(&LIVE.queue_busy_since);
        match (busy, *b) {
            (true, None) => { *b = Some(now()); return; }
            (false, Some(t)) => { *b = None; t }
            _ => return,
        }
    };
    let text = summary_since(since - 1);
    if !text.is_empty() {
        fire(app, Event::QueueDrained, None, None, &text);
    }
}

/// Once a day at the morning time: what the queue did since the last summary.
fn morning(app: &AppHandle) {
    let at = { let eng = lock(&LIVE.engine); if !eng.config.wants(Event::MorningSummary) { return } minutes(&eng.config.morning).unwrap_or(480) };
    if local_minute() < at {
        return;
    }
    let st = app.state::<AppState>();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let last = st.db().setting(MORNING_SETTING).ok().flatten().unwrap_or_default();
    let (day, since) = last.split_once('|').map(|(d, t)| (d.to_string(), t.parse().unwrap_or(0))).unwrap_or_default();
    if day == today {
        return;
    }
    let since = if since > 0 { since } else { now() - 24 * 3600 };
    let text = { let q = st.queue.lock().unwrap_or_else(|p| p.into_inner()); q.summary_since(since).text() };
    let _ = st.db().set_setting(MORNING_SETTING, &format!("{today}|{}", now()));
    if !text.is_empty() {
        fire(app, Event::MorningSummary, None, None, &text);
    }
}

// ---------- commands ----------

fn save_config(st: &AppState, cfg: Config) -> Value {
    let _ = st.db().set_setting(SETTING, &cfg.to_json());
    lock(&LIVE.engine).config = cfg;
    view()
}

fn view() -> Value {
    let eng = lock(&LIVE.engine);
    let status = lock(&LIVE.status);
    let held: HashMap<String, usize> = eng.config.destinations.iter().map(|d| (d.id.clone(), eng.held(&d.id))).collect();
    json!({"config": eng.config, "status": *status, "held": held, "quiet_now": is_quiet(&eng.config.quiet, local_minute())})
}

/// Destinations, quiet hours, the morning time, and each destination's last error.
#[tauri::command]
pub async fn alerts_view() -> Res<Value> {
    Ok(view())
}

#[tauri::command]
pub async fn alerts_add(app: AppHandle, service: Service) -> Res<Value> {
    let mut cfg = lock(&LIVE.engine).config.clone();
    if cfg.destinations.len() >= 10 {
        return Err("That's 10 destinations, the most nekotrail keeps.".into());
    }
    cfg.destinations.push(Destination::new(crate::store::new_id(), service));
    Ok(save_config(&app.state::<AppState>(), cfg))
}

#[tauri::command]
pub async fn alerts_save(app: AppHandle, config: Config) -> Res<Value> {
    let cfg = lock(&LIVE.engine).config.merge_from_window(config)?;
    Ok(save_config(&app.state::<AppState>(), cfg))
}

/// Saves a destination's address or tokens in the keychain. Runs off the async workers: the
/// keychain can take a moment (or ask to be unlocked).
#[tauri::command]
pub async fn alerts_set_secret(app: AppHandle, id: String, secret: Secret) -> Res<Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut cfg = lock(&LIVE.engine).config.clone();
        save_secret(&mut cfg, &Keychain, &id, &secret)?;
        lock(&LIVE.status).remove(&id);
        Ok(save_config(&app.state::<AppState>(), cfg))
    }).await.map_err(e)?
}

#[tauri::command]
pub async fn alerts_remove(app: AppHandle, id: String) -> Res<Value> {
    tauri::async_runtime::spawn_blocking(move || {
        Keychain.delete(&id)?;
        let mut cfg = lock(&LIVE.engine).config.clone();
        cfg.destinations.retain(|d| d.id != id);
        lock(&LIVE.status).remove(&id);
        Ok(save_config(&app.state::<AppState>(), cfg))
    }).await.map_err(e)?
}

/// Sends the test message now, past quiet hours and the rate limit. The answer is the error, if any.
#[tauri::command]
pub async fn alerts_test(app: AppHandle, id: String) -> Res<Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let d = lock(&LIVE.engine).config.destinations.iter().find(|d| d.id == id).cloned().ok_or("That destination is gone.")?;
        let r = send_message(&d, &test_message(), &Keychain, &Http);
        record(&app, &id, r.clone());
        r.map(|()| view())
    }).await.map_err(e)?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records what would have been sent. Nothing leaves the test.
    #[derive(Default)]
    struct FakeNet {
        sent: Mutex<Vec<Req>>,
        fail: Option<String>,
    }
    impl Transport for FakeNet {
        fn post(&self, req: &Req) -> Result<(), String> {
            lock(&self.sent).push(req.clone());
            match &self.fail { Some(f) => Err(f.clone()), None => Ok(()) }
        }
    }

    fn body(r: &Req) -> Value {
        serde_json::from_slice(&r.body).unwrap()
    }

    fn alert(event: Event, ws: &str) -> Alert {
        Alert { event, workspace_id: Some(ws.into()), workspace: format!("Fix {ws}"), repo: "pawprint-web".into(), what: needs_text("plan").into(), last_line: Some("All 42 tests pass.".into()), at: 0 }
    }

    fn dest(service: Service, events: &[Event]) -> Destination {
        Destination { saved: true, events: events.to_vec(), ..Destination::new("d1".into(), service) }
    }

    const SLACK: &str = "https://hooks.slack.com/services/T000/B000/XXXXXXXXXXXXXXXX";
    const PUSH_TOKEN: &str = "azGDORePK8gMaC0QOYAMyEEuzJnyUi";
    const PUSH_USER: &str = "uQiRzpo4DXghDmr9QzzfQu27cmVRsG";

    #[test]
    fn a_new_destination_sends_no_content_and_skips_the_noisy_events() {
        let d = Destination::new("x".into(), Service::Ntfy);
        assert!(!d.last_line, "the last line is off by default");
        assert!(!d.saved, "nothing goes out before a secret is saved");
        assert!(!d.wants(Event::NeedsYou));
        assert!(!d.events.contains(&Event::TurnFinished) && !d.events.contains(&Event::ChecksGreen));
        let m = compose(&[alert(Event::NeedsYou, "w1")], d.last_line);
        assert!(!m.body.contains("42 tests"), "{}", m.body);
        assert_eq!(m.title, "Fix w1 has a plan for you to approve");
        assert_eq!(m.body, "Fix w1 in pawprint-web");
        assert_eq!(m.link.as_deref(), Some("nekotrail://open?workspace=w1"));
        assert!(m.urgent);
    }

    #[test]
    fn the_last_line_only_goes_out_when_asked_for() {
        let m = compose(&[alert(Event::TurnFinished, "w1")], true);
        assert!(m.body.ends_with("Last line: All 42 tests pass."), "{}", m.body);
        let mut eng = Engine::new(Config { destinations: vec![dest(Service::Webhook, &[Event::NeedsYou])], ..Default::default() });
        eng.fire(&alert(Event::NeedsYou, "w1"), 0);
        let items = eng.due(100, false).remove(0).1;
        assert_eq!(items[0].last_line, None, "dropped for a destination that didn't ask");
    }

    #[test]
    fn last_line_is_masked_and_short() {
        let gh = format!("ghp_{}", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8");
        let events = vec![
            json!({"kind": "text", "text": "Earlier answer"}),
            json!({"kind": "tool", "name": "Bash"}),
            json!({"kind": "text", "text": format!("Done.\n\n**Pushed** with `{gh}` from /Users/sam/code/app\n```")}),
        ];
        assert_eq!(last_line_of(&events, "/Users/sam").unwrap(), "Pushed with [hidden: GitHub token] from ~/code/app");
        let long = vec![json!({"kind": "text", "text": "word ".repeat(100)})];
        assert_eq!(last_line_of(&long, "/x").unwrap().chars().count(), LAST_LINE_MAX);
        assert_eq!(last_line_of(&[json!({"kind": "tool"})], "/x"), None);
    }

    #[test]
    fn payload_per_service() {
        let m = compose(&[alert(Event::NeedsYou, "w1")], false);
        // ntfy: JSON to the server root, topic in the body, the link as the click action.
        let s = Secret { url: "https://ntfy.sh/cat-alerts".into(), token: "tk_abcdef".into(), ..Default::default() };
        let r = request(Service::Ntfy, &s, &m).unwrap();
        assert_eq!(r.url, "https://ntfy.sh");
        assert!(r.headers.contains(&("Authorization".into(), "Bearer tk_abcdef".into())));
        let b = body(&r);
        assert_eq!((b["topic"].as_str(), b["priority"].as_u64(), b["click"].as_str()), (Some("cat-alerts"), Some(4), Some("nekotrail://open?workspace=w1")));
        // Slack: one text field, reserved characters escaped.
        let mut m2 = m.clone();
        m2.title = "A <b> & c".into();
        let r = request(Service::Slack, &Secret { url: SLACK.into(), ..Default::default() }, &m2).unwrap();
        assert_eq!(r.url, SLACK);
        let t = body(&r)["text"].as_str().unwrap().to_string();
        assert!(t.starts_with("*A &lt;b&gt; &amp; c*\n") && t.contains("Open on your Mac: nekotrail://open?workspace=w1"), "{t}");
        // Discord: mentions off.
        let r = request(Service::Discord, &Secret { url: "https://discord.com/api/webhooks/1/abc".into(), ..Default::default() }, &m).unwrap();
        assert_eq!(body(&r)["allowed_mentions"], json!({"parse": []}));
        assert!(body(&r)["content"].as_str().unwrap().starts_with("**Fix w1 has a plan"));
        // Generic webhook: structured, with each item, no last line unless asked.
        let r = request(Service::Webhook, &Secret { url: "https://example.org/hook".into(), ..Default::default() }, &m).unwrap();
        let b = body(&r);
        assert_eq!((b["source"].as_str(), b["event"].as_str()), (Some("nekotrail"), Some("needs_you")));
        assert_eq!(b["items"][0]["repo"], "pawprint-web");
        assert!(b["items"][0].get("last_line").is_none());
        assert!(!r.headers.iter().any(|(k, _)| k == "Authorization"));
        // Pushover: a form with token and user, high priority, the link as a supplementary URL.
        let r = request(Service::Pushover, &Secret { token: PUSH_TOKEN.into(), user: PUSH_USER.into(), ..Default::default() }, &m).unwrap();
        assert_eq!(r.url, "https://api.pushover.net/1/messages.json");
        let f = String::from_utf8(r.body).unwrap();
        assert!(f.contains(&format!("token={PUSH_TOKEN}")) && f.contains("priority=1") && f.contains("url=nekotrail%3A%2F%2Fopen%3Fworkspace%3Dw1"), "{f}");
    }

    #[test]
    fn secrets_are_checked_before_they_are_kept() {
        let ok = |svc, s: Secret| check_secret(svc, &s);
        assert_eq!(ok(Service::Ntfy, Secret { url: "https://ntfy.sh/cat-alerts".into(), ..Default::default() }).unwrap(), "ntfy.sh");
        assert!(ok(Service::Ntfy, Secret { url: "https://ntfy.sh/".into(), ..Default::default() }).is_err(), "no topic");
        assert!(ok(Service::Slack, Secret { url: "https://example.com/services/x".into(), ..Default::default() }).is_err());
        assert!(ok(Service::Discord, Secret { url: "https://discord.com/channels/1".into(), ..Default::default() }).is_err());
        assert!(ok(Service::Webhook, Secret { url: "file:///etc/passwd".into(), ..Default::default() }).is_err());
        assert!(ok(Service::Webhook, Secret { url: "https://me:pw@example.org/h".into(), ..Default::default() }).is_err());
        assert!(ok(Service::Pushover, Secret { token: "short".into(), user: PUSH_USER.into(), ..Default::default() }).is_err());
        assert_eq!(ok(Service::Webhook, Secret { url: "https://hooks.example.org/a/very/secret/path".into(), ..Default::default() }).unwrap(), "hooks.example.org", "only the host is shown");
    }

    #[test]
    fn keychain_abstraction_keeps_secrets_out_of_the_config() {
        let store = MemSecrets::default();
        let mut cfg = Config { destinations: vec![Destination::new("d1".into(), Service::Slack)], ..Default::default() };
        let err = save_secret(&mut cfg, &store, "d1", &Secret { url: "https://example.com/x".into(), ..Default::default() }).unwrap_err();
        assert!(err.contains("hooks.slack.com"));
        assert!(store.get("d1").unwrap().is_none(), "a bad secret is not kept");
        save_secret(&mut cfg, &store, "d1", &Secret { url: format!("  {SLACK} "), ..Default::default() }).unwrap();
        assert_eq!(store.get("d1").unwrap().unwrap().url, SLACK, "trimmed");
        assert!(cfg.destinations[0].saved);
        let json = cfg.to_json();
        assert!(!json.contains("XXXXXXXX") && !json.contains("services/T000"), "{json}");
        // The window can't mark a destination saved or change its host or service.
        let mut forged = cfg.clone();
        forged.destinations[0].saved = false;
        forged.destinations[0].hint = "evil.example".into();
        forged.destinations[0].service = Service::Webhook;
        forged.destinations.push(Destination { saved: true, ..Destination::new("new".into(), Service::Webhook) });
        let merged = cfg.merge_from_window(forged).unwrap();
        assert_eq!(merged.destinations.len(), 1);
        assert_eq!((merged.destinations[0].saved, merged.destinations[0].hint.as_str(), merged.destinations[0].service), (true, "hooks.slack.com", Service::Slack));
        let mut bad = cfg.clone();
        bad.quiet.from = "25:00".into();
        assert!(cfg.merge_from_window(bad).is_err());
        // The Debug form of a secret and of a request never shows it.
        assert_eq!(format!("{:?}", store.get("d1").unwrap().unwrap()), "Secret(hidden)");
    }

    #[test]
    fn deliver_uses_the_keychain_and_cleans_errors() {
        let store = MemSecrets::default();
        let d = dest(Service::Slack, &[Event::NeedsYou]);
        let net = FakeNet::default();
        assert!(deliver(&d, &[alert(Event::NeedsYou, "w1")], &store, &net).unwrap_err().contains("missing"));
        assert!(lock(&net.sent).is_empty());
        store.set("d1", &Secret { url: SLACK.into(), ..Default::default() }).unwrap();
        deliver(&d, &[alert(Event::NeedsYou, "w1")], &store, &net).unwrap();
        assert_eq!(lock(&net.sent).len(), 1);
        let failing = FakeNet { fail: Some(format!("error sending request for url ({SLACK})")), ..Default::default() };
        let err = deliver(&d, &[alert(Event::NeedsYou, "w1")], &store, &failing).unwrap_err();
        assert!(!err.contains("XXXXXXXX") && err.contains("[hidden]"), "{err}");
    }

    #[test]
    fn five_needs_you_in_a_minute_become_one_message() {
        let store = MemSecrets::default();
        store.set("d1", &Secret { url: SLACK.into(), ..Default::default() }).unwrap();
        let net = FakeNet::default();
        let mut eng = Engine::new(Config { destinations: vec![dest(Service::Slack, &[Event::NeedsYou])], ..Default::default() });
        let mut sends = 0;
        // One every 12 seconds, a check every 5 seconds, as the loop does.
        for t in 0..=180 {
            if t % 12 == 0 && t < 60 {
                eng.fire(&alert(Event::NeedsYou, &format!("w{t}")), t);
            }
            if t % 5 != 0 {
                continue;
            }
            for (d, items) in eng.due(t, false) {
                deliver(&d, &items, &store, &net).unwrap();
                sends += 1;
            }
        }
        assert_eq!(sends, 1);
        let text = body(&lock(&net.sent)[0])["text"].as_str().unwrap().to_string();
        assert!(text.starts_with("*5 workspaces need you*"), "{text}");
        assert_eq!(text.matches("\n- ").count(), 5);
        assert!(!text.contains("Open on your Mac"), "a mixed burst has no single link");
    }

    #[test]
    fn rate_limit_and_dedupe() {
        let mut eng = Engine::new(Config { destinations: vec![dest(Service::Ntfy, &[Event::NeedsYou, Event::PrMerged])], ..Default::default() });
        eng.fire(&alert(Event::NeedsYou, "w1"), 0);
        assert!(eng.due(10, false).is_empty(), "gathering");
        assert_eq!(eng.due(20, false)[0].1.len(), 1);
        // Within a minute of the last message: held, and the same workspace twice counts once.
        eng.fire(&alert(Event::NeedsYou, "w2"), 30);
        eng.fire(&alert(Event::NeedsYou, "w2"), 31);
        eng.fire(&alert(Event::PrMerged, "w2"), 32);
        assert!(eng.due(70, false).is_empty(), "60 s gap");
        assert_eq!(eng.due(80, false)[0].1.len(), 2);
        // No more than PER_HOUR in an hour.
        let mut t = 200;
        let mut n = 0;
        while t < 3600 {
            eng.fire(&alert(Event::NeedsYou, &format!("x{t}")), t);
            n += eng.due(t + GATHER, false).len();
            t += MIN_GAP + GATHER;
        }
        assert_eq!(n, PER_HOUR - 2);
        assert!(eng.held("d1") > 0);
    }

    #[test]
    fn quiet_hours_hold_messages_until_they_end() {
        let q = Quiet { on: true, from: "22:00".into(), to: "07:00".into() };
        assert!(is_quiet(&q, 23 * 60) && is_quiet(&q, 3 * 60) && !is_quiet(&q, 7 * 60) && !is_quiet(&q, 12 * 60));
        let day = Quiet { on: true, from: "09:00".into(), to: "17:30".into() };
        assert!(is_quiet(&day, 9 * 60) && !is_quiet(&day, 17 * 60 + 30));
        assert!(!is_quiet(&Quiet { on: false, ..q.clone() }, 23 * 60));
        assert!(!is_quiet(&Quiet { on: true, from: "08:00".into(), to: "08:00".into() }, 8 * 60));
        let mut eng = Engine::new(Config { destinations: vec![dest(Service::Ntfy, &[Event::NeedsYou, Event::QueueDrained])], ..Default::default() });
        for i in 0..3 {
            eng.fire(&alert(Event::NeedsYou, &format!("w{i}")), i * 600);
        }
        assert!(eng.due(4000, true).is_empty());
        let out = eng.due(4005, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1.len(), 3, "one message when quiet hours end");
    }

    #[test]
    fn too_many_held_alerts_are_counted_not_kept() {
        let mut ob = Outbox::default();
        for i in 0..(MAX_PENDING + 5) {
            ob.push(alert(Event::NeedsYou, &format!("w{i}")), 0);
        }
        let out = ob.take_due(100, false).unwrap();
        assert_eq!(out.len(), MAX_PENDING + 1);
        assert_eq!(out[0].what, "5 older updates were left out");
        assert!(compose(&out, false).body.ends_with(&format!("and {} more", MAX_PENDING + 1 - DIGEST_LINES)));
    }

    #[test]
    fn queue_and_summary_messages_name_no_workspace() {
        let a = Alert { event: Event::MorningSummary, workspace_id: None, workspace: String::new(), repo: String::new(), what: "8 done, 2 need you, 1 failed".into(), last_line: None, at: 0 };
        let m = compose(std::slice::from_ref(&a), true);
        assert_eq!((m.title.as_str(), m.body.as_str(), m.link.as_deref()), ("Overnight in nekotrail", "8 done, 2 need you, 1 failed", None));
        let m = compose(&[Alert { event: Event::QueueDrained, ..a }], false);
        assert_eq!(m.title, "The queue is done");
    }

    #[test]
    fn pr_changes_worth_an_alert() {
        let pr = |state: &str, sha: &str, checks: Value| json!({"workspace_id": "w1", "pr": {"number": 12, "state": state, "headRefOid": sha, "statusCheckRollup": checks}});
        let fail = json!([{"conclusion": "FAILURE"}, {"conclusion": "SUCCESS"}]);
        let pass = json!([{"conclusion": "SUCCESS"}, {"state": "SUCCESS"}]);
        let wait = json!([{"status": "IN_PROGRESS", "conclusion": null}]);
        assert_eq!(pr_alert(None, &pr("OPEN", "a", fail.clone())), None, "first look after a restart");
        assert_eq!(pr_alert(Some(&pr("OPEN", "a", wait.clone())), &pr("OPEN", "a", fail.clone())), Some((Event::ChecksFailed, "has failing checks on #12".into())));
        assert_eq!(pr_alert(Some(&pr("OPEN", "a", fail.clone())), &pr("OPEN", "a", fail.clone())), None, "no repeat");
        assert_eq!(pr_alert(Some(&pr("OPEN", "a", wait.clone())), &pr("OPEN", "a", pass.clone())).unwrap().0, Event::ChecksGreen);
        assert_eq!(pr_alert(Some(&pr("OPEN", "a", pass.clone())), &pr("OPEN", "b", pass.clone())).unwrap().0, Event::ChecksGreen, "a new push went green between looks");
        assert_eq!(pr_alert(Some(&pr("OPEN", "a", pass.clone())), &pr("OPEN", "b", wait.clone())), None);
        assert_eq!(pr_alert(Some(&pr("OPEN", "a", pass.clone())), &pr("MERGED", "a", pass.clone())), Some((Event::PrMerged, "merged #12".into())));
        assert_eq!(pr_alert(Some(&pr("MERGED", "a", pass.clone())), &pr("MERGED", "a", pass)), None);
        assert_eq!(pr_alert(Some(&json!({"workspace_id": "w1", "problem": {}})), &pr("MERGED", "a", wait)), None);
    }

    #[test]
    fn links_and_config_round_trip() {
        assert_eq!(link("a b/é"), "nekotrail://open?workspace=a%20b%2F%C3%A9");
        let mut cfg = Config::default();
        cfg.destinations.push(Destination::new("d1".into(), Service::Pushover));
        assert_eq!(Config::from_json(Some(&cfg.to_json())), cfg);
        assert_eq!(Config::from_json(Some("not json")), Config::default());
        assert_eq!(minutes("07:30"), Some(450));
        assert_eq!(minutes("7:5"), Some(425));
        assert_eq!(minutes("24:00"), None);
    }
}
