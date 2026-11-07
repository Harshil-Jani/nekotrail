//! Linear issues as a place to start a workspace from (New workspace, Linear; `nekotrail new
//! --linear ENG-123`).
//!
//! - The personal API key lives in the OS keychain (`KeyStore`), never in the database, a log or
//!   an export. The database keeps only who it belongs to ("Ada Lovelace, Acme") for Settings.
//! - Issues come from Linear's GraphQL API. A workspace takes the issue's own branch name
//!   (`branchName`), is titled after it (`title::ISSUE`) and its first message carries the
//!   identifier, title, description and link.
//! - The workspace remembers its issue (`Link`), so the PR nekotrail opens says "Fixes ENG-123"
//!   (Linear's GitHub integration links it from that), and the first turn can move the issue to
//!   the team's "In Progress" state (Settings, on by default). nekotrail never comments on Linear.
//! - Every failure is a plain sentence. Requests run off the async workers with a 10 second
//!   timeout, so a slow or missing network never holds up the app.
//!
//! Everything but the glue at the bottom is pure and tested with a fake API and a fake keychain,
//! so tests never reach Linear.

use crate::{e, redact, AppState, Res};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub const ENDPOINT: &str = "https://api.linear.app/graphql";
const KEYCHAIN_SERVICE: &str = "dev.nekotrail.app.linear";
const KEYCHAIN_ACCOUNT: &str = "api-key";
/// Who the saved key belongs to, as JSON `{name, org}`. Not a secret.
const ACCOUNT_SETTING: &str = "linear.account";
/// "off" stops the first turn from moving the issue to In Progress.
const MOVE_SETTING: &str = "linear.move_started";
/// The last thing that went wrong in the background (moving an issue), for Settings.
const ERROR_SETTING: &str = "linear.last_error";
const TIMEOUT: Duration = Duration::from_secs(10);
/// Issues listed at once. Search narrows it down.
const PAGE: u32 = 50;
/// The longest description put in the first message. Longer ones are cut with a note.
const MAX_DESCRIPTION: usize = 8000;

pub const NOT_CONNECTED: &str = "Connect Linear first: Settings, Integrations.";
const BAD_KEY: &str = "Linear didn't accept this key. Make a new personal API key in Linear (Settings, Security & access) and connect again.";
const RATE_LIMITED: &str = "Linear is limiting requests right now. Try again in a minute.";
const NO_NETWORK: &str = "Couldn't reach Linear. Check your network and try again.";
const SLOW: &str = "Linear didn't answer in 10 seconds. Try again in a moment.";
const UNREADABLE: &str = "Linear sent an answer NekoTrail couldn't read. Try again, or update NekoTrail.";

// ---------- what Linear sends ----------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    pub org: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    /// Linear's id (a UUID). Used for updates.
    pub id: String,
    /// "ENG-123".
    pub identifier: String,
    pub title: String,
    /// Markdown, "" when there is none.
    pub description: String,
    pub url: String,
    /// Linear's suggested branch name, e.g. "ada/eng-123-fix-the-login-loop". "" if missing.
    pub branch: String,
    pub state: String,
    /// triage, backlog, unstarted, started, completed or canceled.
    pub state_type: String,
    pub team: Team,
    pub assignee: Option<String>,
    pub labels: Vec<String>,
    /// 0 none, 1 urgent, 2 high, 3 medium, 4 low (Linear's numbers).
    pub priority: u8,
    pub updated_at: String,
}

/// A row of the New workspace list: the issue and the first message it would start with.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    #[serde(flatten)]
    pub issue: Issue,
    pub seed: String,
}

const ISSUE_FIELDS: &str = "id identifier title description url branchName priority updatedAt \
    state { name type } team { id key name } assignee { name } labels(first: 5) { nodes { name } }";

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

/// Who a key belongs to, from `viewer { name displayName organization { name } }`.
pub fn parse_account(data: &Value) -> Result<Account, String> {
    let v = &data["viewer"];
    if !v.is_object() {
        return Err(UNREADABLE.into());
    }
    let name = [&v["name"], &v["displayName"]].iter().map(|x| s(x)).find(|x| !x.trim().is_empty()).unwrap_or_default();
    Ok(Account { name, org: s(&v["organization"]["name"]) })
}

pub fn parse_team(v: &Value) -> Option<Team> {
    let t = Team { id: s(&v["id"]), key: s(&v["key"]), name: s(&v["name"]) };
    (!t.id.is_empty()).then_some(t)
}

pub fn parse_teams(data: &Value) -> Vec<Team> {
    let mut teams: Vec<Team> = data["teams"]["nodes"].as_array().map(|a| a.iter().filter_map(parse_team).collect()).unwrap_or_default();
    teams.sort_by_key(|t| t.name.to_lowercase());
    teams
}

/// One issue node. None without an id or identifier (nothing to start from).
pub fn parse_issue(v: &Value) -> Option<Issue> {
    let i = Issue {
        id: s(&v["id"]),
        identifier: s(&v["identifier"]),
        title: s(&v["title"]),
        description: s(&v["description"]),
        url: s(&v["url"]),
        branch: s(&v["branchName"]),
        state: s(&v["state"]["name"]),
        state_type: s(&v["state"]["type"]),
        team: parse_team(&v["team"]).unwrap_or_default(),
        assignee: v["assignee"]["name"].as_str().map(str::to_string),
        labels: v["labels"]["nodes"].as_array().map(|a| a.iter().filter_map(|l| l["name"].as_str().map(str::to_string)).collect()).unwrap_or_default(),
        priority: v["priority"].as_u64().map(|p| p.min(4) as u8).unwrap_or(0),
        updated_at: s(&v["updatedAt"]),
    };
    (!i.id.is_empty() && !i.identifier.is_empty()).then_some(i)
}

pub fn parse_issues(data: &Value) -> Vec<Issue> {
    data["issues"]["nodes"].as_array().map(|a| a.iter().filter_map(parse_issue).collect()).unwrap_or_default()
}

/// The answer to an HTTP request, as data or a sentence for a person. `key` is hidden from any
/// text Linear echoes back.
pub fn read_answer(status: u16, body: &str, key: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let errors = v["errors"].as_array().cloned().unwrap_or_default();
    let code = |c: &str| errors.iter().any(|x| x["extensions"]["code"].as_str().is_some_and(|y| y.eq_ignore_ascii_case(c)));
    if status == 401 || status == 403 || code("AUTHENTICATION_ERROR") || code("FORBIDDEN") {
        return Err(BAD_KEY.into());
    }
    if status == 429 || code("RATELIMITED") {
        return Err(RATE_LIMITED.into());
    }
    if status >= 500 {
        return Err(format!("Linear isn't answering right now (HTTP {status}). Try again in a minute."));
    }
    if let Some(first) = errors.first() {
        let said = first["extensions"]["userPresentableMessage"].as_str().or(first["message"].as_str()).unwrap_or("something went wrong");
        return Err(format!("Linear said: {}", clean(said, key)));
    }
    if !(200..300).contains(&status) {
        return Err(format!("Linear answered HTTP {status}. Try again in a minute."));
    }
    match v.get("data") {
        Some(d) if d.is_object() => Ok(d.clone()),
        _ => Err(UNREADABLE.into()),
    }
}

/// Short, one line, without the key or anything else that looks like a secret.
fn clean(text: &str, key: &str) -> String {
    let mut t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if key.len() >= 8 {
        t = t.replace(key, "[hidden]");
    }
    let t = redact::mask(&t);
    if t.chars().count() <= 160 { t } else { format!("{}…", t.chars().take(159).collect::<String>()) }
}

// ---------- keys and identifiers ----------

/// A pasted key, tidied: no spaces around it, no "Bearer ". Errors say what a key looks like.
pub fn check_key(raw: &str) -> Result<String, String> {
    let k = raw.trim();
    let k = k.strip_prefix("Bearer ").or_else(|| k.strip_prefix("bearer ")).unwrap_or(k).trim();
    if k.is_empty() {
        return Err("Paste your Linear personal API key.".into());
    }
    if !k.starts_with("lin_api_") || k.len() < 20 || k.len() > 200 || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("That doesn't look like a Linear personal API key. It starts with lin_api_.".into());
    }
    Ok(k.to_string())
}

/// "ENG-123" (any case, or a linear.app issue link) as Linear's identifier, upper case.
pub fn issue_ref(raw: &str) -> Option<String> {
    let t = raw.trim();
    let t = match t.find("/issue/") {
        Some(i) if t.contains("linear.app/") => t[i + 7..].split(['/', '?', '#']).next().unwrap_or(""),
        _ => t,
    };
    let (team, num) = t.split_once('-')?;
    let ok_team = (1..=10).contains(&team.len()) && team.starts_with(|c: char| c.is_ascii_alphabetic()) && team.chars().all(|c| c.is_ascii_alphanumeric());
    let ok_num = (1..=7).contains(&num.len()) && num.chars().all(|c| c.is_ascii_digit()) && !num.starts_with('0');
    (ok_team && ok_num).then(|| format!("{}-{num}", team.to_ascii_uppercase()))
}

// ---------- what a workspace gets ----------

/// The branch for an issue: Linear's own name when git can use it, else `<prefix>/eng-123-<title>`.
pub fn branch_for(issue: &Issue, prefix: &str) -> String {
    let b = issue.branch.trim();
    if b.len() <= 200 && crate::safe::branch_name(b) && b.is_ascii() {
        return b.to_string();
    }
    let words: Vec<String> = issue
        .title
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(6)
        .map(str::to_string)
        .collect();
    let mut slug = format!("{}-{}", issue.identifier.to_lowercase(), words.join("-"));
    slug.truncate(60);
    format!("{prefix}/{}", slug.trim_end_matches('-'))
}

/// `branch`, or `branch-2`, `branch-3`... whichever `taken` says is free.
pub fn free_branch(branch: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(branch) {
        return branch.to_string();
    }
    (2..1000).map(|n| format!("{branch}-{n}")).find(|b| !taken(b)).unwrap_or_else(|| format!("{branch}-{}", rand::random::<u16>()))
}

/// The first message: identifier and title, the link, the description, and the PR line.
pub fn seed_text(issue: &Issue) -> String {
    let mut out = format!("Linear issue {}: {}", issue.identifier, issue.title.trim());
    if !issue.url.is_empty() {
        out.push_str(&format!("\n{}", issue.url));
    }
    let d = issue.description.trim();
    if !d.is_empty() {
        out.push_str("\n\n");
        if d.chars().count() > MAX_DESCRIPTION {
            out.push_str(d.chars().take(MAX_DESCRIPTION).collect::<String>().trim_end());
            out.push_str("\n\n(The description is cut here. The rest is on the issue in Linear.)");
        } else {
            out.push_str(d);
        }
    }
    out.push_str(&format!("\n\nWhen you open a PR, put \"{}\" in its description.", fixes_line(&issue.identifier)));
    out
}

pub fn fixes_line(identifier: &str) -> String {
    format!("Fixes {identifier}")
}

/// A PR description with the line Linear's GitHub integration looks for, unless the
/// identifier is already in it.
pub fn with_fixes_line(body: &str, identifier: &str) -> String {
    let id = identifier.to_ascii_uppercase();
    let has = body.to_ascii_uppercase().match_indices(&id).any(|(i, _)| {
        let up = body.as_bytes();
        let before = i.checked_sub(1).map(|j| up[j]);
        let after = up.get(i + id.len()).copied();
        !before.is_some_and(|c| c.is_ascii_alphanumeric()) && !after.is_some_and(|c| c.is_ascii_alphanumeric())
    });
    if has || id.is_empty() {
        return body.to_string();
    }
    let b = body.trim_end();
    if b.is_empty() { fixes_line(identifier) } else { format!("{b}\n\n{}", fixes_line(identifier)) }
}

/// Only an issue that hasn't started moves. One already started, done or canceled stays put.
pub fn should_move(state_type: &str) -> bool {
    matches!(state_type, "triage" | "backlog" | "unstarted")
}

/// The team's In Progress state: a started-type state called "In Progress", else the first
/// started-type state in the team's order.
pub fn started_state(states: &Value) -> Option<String> {
    let nodes = states["nodes"].as_array()?;
    let started: Vec<&Value> = nodes.iter().filter(|n| n["type"] == "started" && n["id"].is_string()).collect();
    started
        .iter()
        .find(|n| n["name"].as_str().is_some_and(|x| x.trim().eq_ignore_ascii_case("in progress")))
        .or_else(|| started.iter().min_by(|a, b| a["position"].as_f64().unwrap_or(f64::MAX).total_cmp(&b["position"].as_f64().unwrap_or(f64::MAX))))
        .map(|n| s(&n["id"]))
}

// ---------- queries ----------

/// What the New workspace list asks for. Empty fields mean "any".
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Filter {
    /// Everyone's issues, not only the ones assigned to you.
    pub anyone: bool,
    pub team: Option<String>,
    /// open (the default: not done or canceled), backlog, unstarted, started or all.
    pub state: Option<String>,
    pub query: Option<String>,
}

/// Linear's `IssueFilter` for a list. A query that is an identifier finds that issue, whoever
/// it is assigned to and whatever its state.
pub fn issue_filter(f: &Filter) -> Value {
    let q = f.query.as_deref().map(str::trim).unwrap_or("");
    if let Some(id) = issue_ref(q) {
        let (team, num) = id.split_once('-').unwrap_or_default();
        return json!({"and": [{"team": {"key": {"eqIgnoreCase": team}}}, {"number": {"eq": num.parse::<u64>().unwrap_or(0)}}]});
    }
    let mut and: Vec<Value> = vec![];
    if !f.anyone {
        and.push(json!({"assignee": {"isMe": {"eq": true}}}));
    }
    if let Some(t) = f.team.as_deref().filter(|t| !t.is_empty()) {
        and.push(json!({"team": {"id": {"eq": t}}}));
    }
    match f.state.as_deref().unwrap_or("open") {
        "all" => {}
        t @ ("triage" | "backlog" | "unstarted" | "started" | "completed" | "canceled") => and.push(json!({"state": {"type": {"eq": t}}})),
        _ => and.push(json!({"state": {"type": {"nin": ["completed", "canceled"]}}})),
    }
    if !q.is_empty() {
        and.push(json!({"title": {"containsIgnoreCase": q.chars().take(200).collect::<String>()}}));
    }
    json!({"and": and})
}

pub fn issues_query() -> String {
    format!("query Issues($filter: IssueFilter, $first: Int!) {{ issues(filter: $filter, first: $first, orderBy: updatedAt) {{ nodes {{ {ISSUE_FIELDS} }} }} }}")
}

pub fn issue_query() -> String {
    format!("query Issue($id: String!) {{ issue(id: $id) {{ {ISSUE_FIELDS} }} }}")
}

const VIEWER_QUERY: &str = "query { viewer { name displayName organization { name } } }";
const TEAMS_QUERY: &str = "query { teams(first: 100) { nodes { id key name } } }";
const MOVE_INFO_QUERY: &str = "query Move($id: String!) { issue(id: $id) { state { type } team { states { nodes { id name type position } } } } }";
const MOVE_MUTATION: &str = "mutation Move($id: String!, $state: String!) { issueUpdate(id: $id, input: { stateId: $state }) { success } }";

// ---------- the network and the keychain ----------

/// Sends one GraphQL request. The app uses `Http`; tests answer from a script.
pub trait Api: Send + Sync {
    fn call(&self, key: &str, query: &str, vars: Value) -> Result<Value, String>;
}

/// reqwest, no redirects (they could carry the key elsewhere), 10 s timeout. Call it off the
/// async workers.
pub struct Http;

impl Api for Http {
    fn call(&self, key: &str, query: &str, vars: Value) -> Result<Value, String> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let body = json!({"query": query, "variables": vars}).to_string();
        let key = key.to_string();
        tauri::async_runtime::block_on(async move {
            let client = reqwest::Client::builder()
                .timeout(TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("nekotrail/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|_| NO_NETWORK.to_string())?;
            let resp = client
                .post(ENDPOINT)
                .header("Authorization", &key)
                .header("Content-Type", "application/json")
                .body(body)
                .send()
                .await
                .map_err(|err| if err.is_timeout() { SLOW.to_string() } else { NO_NETWORK.to_string() })?;
            let status = resp.status().as_u16();
            let text = resp.text().await.map_err(|err| if err.is_timeout() { SLOW.to_string() } else { NO_NETWORK.to_string() })?;
            read_answer(status, &text, &key)
        })
    }
}

/// Where the key lives. The app uses the OS keychain; tests use `MemKey`.
pub trait KeyStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, key: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

/// macOS Keychain or the Linux Secret Service, as away alerts use.
pub struct Keychain;

impl Keychain {
    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).map_err(|err| crate::alerts::keychain_error(&err))
    }
}

impl KeyStore for Keychain {
    fn get(&self) -> Result<Option<String>, String> {
        match Self::entry()?.get_password() {
            Ok(k) => Ok(Some(k).filter(|k| !k.is_empty())),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(crate::alerts::keychain_error(&err)),
        }
    }
    fn set(&self, key: &str) -> Result<(), String> {
        Self::entry()?.set_password(key).map_err(|err| crate::alerts::keychain_error(&err))
    }
    fn delete(&self) -> Result<(), String> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(crate::alerts::keychain_error(&err)),
        }
    }
}

/// A keychain in memory, for tests.
#[derive(Default)]
pub struct MemKey(pub Mutex<Option<String>>);

impl KeyStore for MemKey {
    fn get(&self) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }
    fn set(&self, key: &str) -> Result<(), String> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(key.to_string());
        Ok(())
    }
    fn delete(&self) -> Result<(), String> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = None;
        Ok(())
    }
}

fn key(store: &dyn KeyStore) -> Result<String, String> {
    store.get()?.ok_or_else(|| NOT_CONNECTED.to_string())
}

// ---------- the calls, over any API and keychain ----------

/// Checks a pasted key with Linear, then keeps it. Says whose it is.
pub fn connect(raw: &str, store: &dyn KeyStore, api: &dyn Api) -> Result<Account, String> {
    let k = check_key(raw)?;
    let account = parse_account(&api.call(&k, VIEWER_QUERY, json!({}))?)?;
    store.set(&k)?;
    Ok(account)
}

/// Asks Linear who the saved key belongs to.
pub fn whoami(store: &dyn KeyStore, api: &dyn Api) -> Result<Account, String> {
    parse_account(&api.call(&key(store)?, VIEWER_QUERY, json!({}))?)
}

pub fn teams(store: &dyn KeyStore, api: &dyn Api) -> Result<Vec<Team>, String> {
    Ok(parse_teams(&api.call(&key(store)?, TEAMS_QUERY, json!({}))?))
}

pub fn issues(f: &Filter, store: &dyn KeyStore, api: &dyn Api) -> Result<Vec<Row>, String> {
    let data = api.call(&key(store)?, &issues_query(), json!({"filter": issue_filter(f), "first": PAGE}))?;
    Ok(parse_issues(&data).into_iter().map(|issue| Row { seed: seed_text(&issue), issue }).collect())
}

/// One issue, by identifier ("ENG-123") or id.
pub fn issue(id: &str, store: &dyn KeyStore, api: &dyn Api) -> Result<Issue, String> {
    let data = api.call(&key(store)?, &issue_query(), json!({"id": id}))?;
    parse_issue(&data["issue"]).ok_or_else(|| format!("There's no Linear issue {id}, or this key can't see it."))
}

/// Moves an issue that hasn't started to its team's In Progress state. Ok(false): nothing to do.
pub fn move_to_started(issue_id: &str, store: &dyn KeyStore, api: &dyn Api) -> Result<bool, String> {
    let k = key(store)?;
    let data = api.call(&k, MOVE_INFO_QUERY, json!({"id": issue_id}))?;
    let i = &data["issue"];
    if !should_move(i["state"]["type"].as_str().unwrap_or("")) {
        return Ok(false);
    }
    let Some(state) = started_state(&i["team"]["states"]) else { return Ok(false) };
    let done = api.call(&k, MOVE_MUTATION, json!({"id": issue_id, "state": state}))?;
    if done["issueUpdate"]["success"] == true { Ok(true) } else { Err("Linear didn't move the issue to In Progress.".into()) }
}

// ---------- which workspace came from which issue ----------

/// Kept in the settings table under `linear.link:<workspace id>`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub issue_id: String,
    pub identifier: String,
    pub url: String,
    /// The first turn already tried to move it (moved or not), so later turns leave it alone.
    #[serde(default)]
    pub started: bool,
}

fn link_key(workspace_id: &str) -> String {
    format!("linear.link:{workspace_id}")
}

pub fn link(st: &AppState, workspace_id: &str) -> Option<Link> {
    st.db().setting(&link_key(workspace_id)).ok().flatten().and_then(|s| serde_json::from_str(&s).ok())
}

pub fn save_link(st: &AppState, workspace_id: &str, l: &Link) -> Res<()> {
    st.db().set_setting(&link_key(workspace_id), &serde_json::to_string(l).map_err(e)?).map_err(e)
}

fn move_on(st: &AppState) -> bool {
    st.db().setting(MOVE_SETTING).ok().flatten().as_deref() != Some("off")
}

/// The first turn in a workspace made from an issue: move the issue to In Progress, once, on
/// its own thread. A failure is kept for Settings and never touches the turn.
pub fn on_turn(app: &AppHandle, st: &AppState, workspace_id: &str) {
    let Some(mut l) = link(st, workspace_id) else { return };
    if l.started {
        return;
    }
    l.started = true;
    let _ = save_link(st, workspace_id, &l);
    if !move_on(st) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let st = app.state::<AppState>();
        match move_to_started(&l.issue_id, &Keychain, &Http) {
            Ok(_) => {
                let _ = st.db().delete_setting(ERROR_SETTING);
            }
            Err(err) => {
                let _ = st.db().set_setting(ERROR_SETTING, &format!("Couldn't move {} to In Progress. {err}", l.identifier));
            }
        }
    });
}

// ---------- commands ----------

fn status(st: &AppState) -> Value {
    let account: Option<Account> = st.db().setting(ACCOUNT_SETTING).ok().flatten().and_then(|s| serde_json::from_str(&s).ok());
    let last_error = st.db().setting(ERROR_SETTING).ok().flatten().filter(|s| !s.is_empty());
    json!({"connected": account.is_some(), "account": account, "move_started": move_on(st), "last_error": last_error})
}

async fn off_main<T: Send + 'static>(app: AppHandle, f: impl FnOnce(&AppState) -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>())).await.map_err(e)?
}

/// Connected or not, whose key, the In Progress setting and the last background error. Never
/// reads the keychain, so opening Settings never asks to unlock it.
#[tauri::command]
pub async fn linear_status(app: AppHandle) -> Res<Value> {
    Ok(status(&app.state::<AppState>()))
}

/// Checks the key with Linear, keeps it in the keychain and says whose it is.
#[tauri::command]
pub async fn linear_connect(app: AppHandle, key: String) -> Res<Value> {
    off_main(app, move |st| {
        let account = connect(&key, &Keychain, &Http)?;
        st.db().set_setting(ACCOUNT_SETTING, &serde_json::to_string(&account).map_err(e)?).map_err(e)?;
        let _ = st.db().delete_setting(ERROR_SETTING);
        Ok(status(st))
    })
    .await
}

/// Asks Linear who the saved key belongs to (the Test button).
#[tauri::command]
pub async fn linear_test(app: AppHandle) -> Res<Value> {
    off_main(app, move |st| {
        let account = whoami(&Keychain, &Http)?;
        st.db().set_setting(ACCOUNT_SETTING, &serde_json::to_string(&account).map_err(e)?).map_err(e)?;
        let _ = st.db().delete_setting(ERROR_SETTING);
        Ok(status(st))
    })
    .await
}

/// Deletes the key from the keychain. Workspaces made from issues keep their "Fixes" line.
#[tauri::command]
pub async fn linear_disconnect(app: AppHandle) -> Res<Value> {
    off_main(app, move |st| {
        Keychain.delete()?;
        let _ = st.db().delete_setting(ACCOUNT_SETTING);
        let _ = st.db().delete_setting(ERROR_SETTING);
        Ok(status(st))
    })
    .await
}

#[tauri::command]
pub async fn linear_set_move(app: AppHandle, on: bool) -> Res<Value> {
    let st = app.state::<AppState>();
    st.db().set_setting(MOVE_SETTING, if on { "on" } else { "off" }).map_err(e)?;
    Ok(status(&st))
}

#[tauri::command]
pub async fn linear_teams(app: AppHandle) -> Res<Vec<Team>> {
    off_main(app, |_| teams(&Keychain, &Http)).await
}

#[tauri::command]
pub async fn linear_issues(app: AppHandle, filter: Filter) -> Res<Vec<Row>> {
    off_main(app, move |_| issues(&filter, &Keychain, &Http)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers from a script and records what was asked. Nothing leaves the test.
    #[derive(Default)]
    struct FakeApi {
        answers: Mutex<Vec<Result<Value, String>>>,
        asked: Mutex<Vec<(String, String, Value)>>,
    }
    impl FakeApi {
        fn with(answers: Vec<Result<Value, String>>) -> Self {
            FakeApi { answers: Mutex::new(answers), ..Default::default() }
        }
        fn asked(&self) -> Vec<(String, String, Value)> {
            self.asked.lock().unwrap().clone()
        }
    }
    impl Api for FakeApi {
        fn call(&self, key: &str, query: &str, vars: Value) -> Result<Value, String> {
            self.asked.lock().unwrap().push((key.into(), query.into(), vars));
            let mut a = self.answers.lock().unwrap();
            if a.is_empty() { Err("no answer scripted".into()) } else { a.remove(0) }
        }
    }

    // Made up, in the shape of a real key. Not live.
    const KEY: &str = concat!("lin_", "api_", "Ab12Cd34Ef56Gh78Ij90Kl12Mn34Op56Qr78St90");

    fn node() -> Value {
        json!({
            "id": "9f0c-uuid", "identifier": "ENG-123", "title": "Fix the login loop on Safari",
            "description": "Users on **Safari 17** get sent back to `/login`.\n\n- [ ] Repro\n- [ ] Fix",
            "url": "https://linear.app/acme/issue/ENG-123/fix-the-login-loop-on-safari",
            "branchName": "ada/eng-123-fix-the-login-loop-on-safari", "priority": 2, "updatedAt": "2026-11-06T10:00:00.000Z",
            "state": {"name": "Todo", "type": "unstarted"}, "team": {"id": "t1", "key": "ENG", "name": "Engineering"},
            "assignee": {"name": "Ada Lovelace"}, "labels": {"nodes": [{"name": "Bug"}, {"name": "Auth"}]}
        })
    }

    #[test]
    fn parses_issues_teams_and_the_viewer() {
        let i = parse_issue(&node()).unwrap();
        assert_eq!(i.identifier, "ENG-123");
        assert_eq!(i.branch, "ada/eng-123-fix-the-login-loop-on-safari");
        assert_eq!((i.state.as_str(), i.state_type.as_str()), ("Todo", "unstarted"));
        assert_eq!(i.team, Team { id: "t1".into(), key: "ENG".into(), name: "Engineering".into() });
        assert_eq!(i.assignee.as_deref(), Some("Ada Lovelace"));
        assert_eq!(i.labels, ["Bug", "Auth"]);
        assert_eq!(i.priority, 2);
        // Nulls Linear sends for an unassigned issue with no description.
        let bare = parse_issue(&json!({"id": "x", "identifier": "OPS-1", "title": "T", "description": null, "assignee": null, "labels": {"nodes": []}, "priority": 9})).unwrap();
        assert_eq!((bare.description.as_str(), bare.assignee.clone(), bare.priority), ("", None, 4));
        assert!(parse_issue(&json!({"identifier": "ENG-1"})).is_none());
        let list = parse_issues(&json!({"issues": {"nodes": [node(), {"title": "no id"}, node()]}}));
        assert_eq!(list.len(), 2);
        assert!(parse_issues(&json!({})).is_empty());
        let teams = parse_teams(&json!({"teams": {"nodes": [{"id": "b", "key": "WEB", "name": "web"}, {"id": "a", "key": "ENG", "name": "Engineering"}, {"key": "X"}]}}));
        assert_eq!(teams.iter().map(|t| t.key.as_str()).collect::<Vec<_>>(), ["ENG", "WEB"]);
        assert_eq!(parse_account(&json!({"viewer": {"name": "Ada Lovelace", "organization": {"name": "Acme"}}})).unwrap(), Account { name: "Ada Lovelace".into(), org: "Acme".into() });
        assert_eq!(parse_account(&json!({"viewer": {"name": "", "displayName": "ada", "organization": {"name": "Acme"}}})).unwrap().name, "ada");
        assert!(parse_account(&json!({})).is_err());
    }

    #[test]
    fn answers_become_data_or_plain_sentences() {
        assert_eq!(read_answer(200, r#"{"data":{"viewer":{"name":"A"}}}"#, KEY).unwrap()["viewer"]["name"], "A");
        assert_eq!(read_answer(401, "", KEY).unwrap_err(), BAD_KEY);
        let auth = r#"{"errors":[{"message":"Authentication required, not authenticated","extensions":{"code":"AUTHENTICATION_ERROR"}}]}"#;
        assert_eq!(read_answer(400, auth, KEY).unwrap_err(), BAD_KEY);
        let limited = r#"{"errors":[{"message":"Rate limit exceeded","extensions":{"code":"RATELIMITED"}}]}"#;
        assert_eq!(read_answer(400, limited, KEY).unwrap_err(), RATE_LIMITED);
        assert_eq!(read_answer(429, "", KEY).unwrap_err(), RATE_LIMITED);
        assert!(read_answer(503, "<html>", KEY).unwrap_err().contains("HTTP 503"));
        assert_eq!(read_answer(200, "not json", KEY).unwrap_err(), UNREADABLE);
        // Linear's own words, without the key if it echoes it, and on one short line.
        let echoed = format!(r#"{{"errors":[{{"message":"Bad header {KEY}\n now","extensions":{{"userPresentableMessage":"Entity not found: {KEY}"}}}}]}}"#);
        let said = read_answer(200, &echoed, KEY).unwrap_err();
        assert!(said.starts_with("Linear said: Entity not found") && !said.contains(KEY), "{said}");
    }

    #[test]
    fn keys_and_identifiers() {
        assert_eq!(check_key(&format!("  {KEY}\n")).unwrap(), KEY);
        assert_eq!(check_key(&format!("Bearer {KEY}")).unwrap(), KEY);
        assert!(check_key("").unwrap_err().contains("Paste"));
        assert!(check_key("ghp_abcdefghijklmnopqrstuvwxyz").unwrap_err().contains("lin_api_"));
        assert!(check_key("lin_api_short").is_err());
        assert!(check_key(&format!("{KEY} extra")).is_err());
        assert_eq!(issue_ref("eng-123").as_deref(), Some("ENG-123"));
        assert_eq!(issue_ref(" OPS2-7 ").as_deref(), Some("OPS2-7"));
        assert_eq!(issue_ref("https://linear.app/acme/issue/ENG-42/fix-it?x=1").as_deref(), Some("ENG-42"));
        for bad in ["", "ENG", "ENG-", "-12", "ENG-012", "1NG-2", "ENG-12a", "fix the login", "ENG-123 ENG-4", "--base"] {
            assert_eq!(issue_ref(bad), None, "{bad}");
        }
    }

    #[test]
    fn branch_names() {
        let i = parse_issue(&node()).unwrap();
        assert_eq!(branch_for(&i, "harshil"), "ada/eng-123-fix-the-login-loop-on-safari");
        // A name git can't take safely (or none at all) falls back to one made here.
        for b in ["", "--upload-pack=x", "a..b", "has space", "ünïcode/branch"] {
            let odd = Issue { branch: b.into(), ..i.clone() };
            assert_eq!(branch_for(&odd, "harshil"), "harshil/eng-123-fix-the-login-loop-on-safari", "{b}");
        }
        let taken = ["ada/eng-1", "ada/eng-1-2"];
        assert_eq!(free_branch("ada/eng-1", |b| taken.contains(&b)), "ada/eng-1-3");
        assert_eq!(free_branch("ada/eng-2", |b| taken.contains(&b)), "ada/eng-2");
    }

    #[test]
    fn the_first_message_carries_the_issue() {
        let i = parse_issue(&node()).unwrap();
        let seed = seed_text(&i);
        assert_eq!(seed, "Linear issue ENG-123: Fix the login loop on Safari\n\
            https://linear.app/acme/issue/ENG-123/fix-the-login-loop-on-safari\n\n\
            Users on **Safari 17** get sent back to `/login`.\n\n- [ ] Repro\n- [ ] Fix\n\n\
            When you open a PR, put \"Fixes ENG-123\" in its description.");
        let bare = Issue { description: "  ".into(), url: String::new(), ..i.clone() };
        assert_eq!(seed_text(&bare), "Linear issue ENG-123: Fix the login loop on Safari\n\nWhen you open a PR, put \"Fixes ENG-123\" in its description.");
        let long = Issue { description: "word ".repeat(3000), ..i };
        let s = seed_text(&long);
        assert!(s.contains("The rest is on the issue in Linear") && s.chars().count() < MAX_DESCRIPTION + 400);
    }

    #[test]
    fn pr_descriptions_get_the_fixes_line_once() {
        assert_eq!(with_fixes_line("Adds a retry.", "ENG-123"), "Adds a retry.\n\nFixes ENG-123");
        assert_eq!(with_fixes_line("", "ENG-123"), "Fixes ENG-123");
        assert_eq!(with_fixes_line("Adds a retry.\n\n", "ENG-123"), "Adds a retry.\n\nFixes ENG-123");
        // Already named, in any case or form: left as it is.
        assert_eq!(with_fixes_line("Closes eng-123", "ENG-123"), "Closes eng-123");
        assert_eq!(with_fixes_line("See https://linear.app/acme/issue/ENG-123/x", "ENG-123"), "See https://linear.app/acme/issue/ENG-123/x");
        // ENG-1234 and XENG-123 are other issues.
        assert_eq!(with_fixes_line("Part of ENG-1234 and XENG-123", "ENG-123"), "Part of ENG-1234 and XENG-123\n\nFixes ENG-123");
    }

    #[test]
    fn filters() {
        let mine = issue_filter(&Filter::default());
        assert_eq!(mine, json!({"and": [{"assignee": {"isMe": {"eq": true}}}, {"state": {"type": {"nin": ["completed", "canceled"]}}}]}));
        let f = Filter { anyone: true, team: Some("t1".into()), state: Some("started".into()), query: Some(" login ".into()) };
        assert_eq!(issue_filter(&f), json!({"and": [{"team": {"id": {"eq": "t1"}}}, {"state": {"type": {"eq": "started"}}}, {"title": {"containsIgnoreCase": "login"}}]}));
        assert_eq!(issue_filter(&Filter { state: Some("all".into()), anyone: true, ..Default::default() }), json!({"and": []}));
        // An unknown state is the default, not a filter Linear would reject.
        assert_eq!(issue_filter(&Filter { state: Some("weird".into()), ..Default::default() }), mine);
        // An identifier finds that issue, whoever has it.
        assert_eq!(issue_filter(&Filter { query: Some("eng-42".into()), ..Default::default() }), json!({"and": [{"team": {"key": {"eqIgnoreCase": "ENG"}}}, {"number": {"eq": 42}}]}));
        let q: Filter = serde_json::from_value(json!({"team": "t1"})).unwrap();
        assert!(!q.anyone && q.state.is_none());
    }

    #[test]
    fn the_in_progress_state() {
        let states = json!({"nodes": [
            {"id": "s1", "name": "Todo", "type": "unstarted", "position": 1},
            {"id": "s3", "name": "In Review", "type": "started", "position": 3},
            {"id": "s2", "name": "In progress", "type": "started", "position": 2},
            {"id": "s4", "name": "Done", "type": "completed", "position": 4}
        ]});
        assert_eq!(started_state(&states).as_deref(), Some("s2"));
        let renamed = json!({"nodes": [{"id": "s3", "name": "Review", "type": "started", "position": 3}, {"id": "s2", "name": "Doing", "type": "started", "position": 2}]});
        assert_eq!(started_state(&renamed).as_deref(), Some("s2"));
        assert_eq!(started_state(&json!({"nodes": [{"id": "s1", "type": "unstarted"}]})), None);
        assert!(should_move("backlog") && should_move("unstarted") && should_move("triage"));
        assert!(!should_move("started") && !should_move("completed") && !should_move("canceled") && !should_move(""));
    }

    #[test]
    fn connecting_checks_the_key_before_keeping_it() {
        let store = MemKey::default();
        let api = FakeApi::with(vec![Err(BAD_KEY.into())]);
        assert_eq!(connect(KEY, &store, &api).unwrap_err(), BAD_KEY);
        assert_eq!(store.get().unwrap(), None);
        // A malformed key never reaches Linear.
        assert!(connect("hello", &store, &api).is_err());
        assert_eq!(api.asked().len(), 1);
        let api = FakeApi::with(vec![Ok(json!({"viewer": {"name": "Ada Lovelace", "organization": {"name": "Acme"}}}))]);
        let who = connect(&format!(" {KEY} "), &store, &api).unwrap();
        assert_eq!((who.name.as_str(), who.org.as_str()), ("Ada Lovelace", "Acme"));
        assert_eq!(store.get().unwrap().as_deref(), Some(KEY));
        assert_eq!(api.asked()[0].0, KEY);
    }

    #[test]
    fn calls_need_a_key_and_send_it() {
        let store = MemKey::default();
        let api = FakeApi::default();
        assert_eq!(issues(&Filter::default(), &store, &api).unwrap_err(), NOT_CONNECTED);
        assert_eq!(teams(&store, &api).unwrap_err(), NOT_CONNECTED);
        assert!(api.asked().is_empty());
        store.set(KEY).unwrap();
        let api = FakeApi::with(vec![Ok(json!({"issues": {"nodes": [node()]}})), Ok(json!({"issue": null})), Ok(json!({"issue": node()}))]);
        let rows = issues(&Filter::default(), &store, &api).unwrap();
        assert_eq!(rows[0].seed, seed_text(&rows[0].issue));
        let (key, query, vars) = &api.asked()[0];
        assert_eq!(key, KEY);
        assert!(query.contains("branchName") && vars["first"] == PAGE);
        assert!(issue("ENG-9", &store, &api).unwrap_err().contains("no Linear issue ENG-9"));
        assert_eq!(issue("ENG-123", &store, &api).unwrap().identifier, "ENG-123");
        // A row as the window sees it: the issue's fields and its seed side by side.
        let v = serde_json::to_value(&rows[0]).unwrap();
        assert_eq!((v["identifier"].as_str(), v["seed"].is_string()), (Some("ENG-123"), true));
    }

    #[test]
    fn moving_only_touches_an_issue_that_has_not_started() {
        let store = MemKey::default();
        store.set(KEY).unwrap();
        let states = json!({"nodes": [{"id": "s2", "name": "In Progress", "type": "started", "position": 2}]});
        let api = FakeApi::with(vec![
            Ok(json!({"issue": {"state": {"type": "unstarted"}, "team": {"states": states}}})),
            Ok(json!({"issueUpdate": {"success": true}})),
        ]);
        assert_eq!(move_to_started("uuid", &store, &api), Ok(true));
        let asked = api.asked();
        assert!(asked[1].1.starts_with("mutation") && asked[1].2 == json!({"id": "uuid", "state": "s2"}));
        // Already started (or done): one look, no change, and never a comment.
        let api = FakeApi::with(vec![Ok(json!({"issue": {"state": {"type": "started"}, "team": {"states": states}}}))]);
        assert_eq!(move_to_started("uuid", &store, &api), Ok(false));
        assert_eq!(api.asked().len(), 1);
        assert!(!issues_query().contains("comment") && !MOVE_MUTATION.contains("comment"));
        let api = FakeApi::with(vec![Err(RATE_LIMITED.into())]);
        assert_eq!(move_to_started("uuid", &store, &api).unwrap_err(), RATE_LIMITED);
    }
}
