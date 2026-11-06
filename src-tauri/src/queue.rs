//! The task queue: tasks you line up so agents keep working while you're away. Each task gets its
//! own new workspace. nekotrail starts up to `max_total` at a time (and `max_per_repo` in one
//! repo), and starts the next one when a task finishes or needs you.
//!
//! This file is the queue itself and every rule about it, with no Tauri in it, so the scheduling
//! is tested on its own: which task starts next, usage limits and their reset times, the fallback
//! agent, retry, a restart, and the morning summary. `queuerun.rs` makes workspaces and starts
//! the turns these rules ask for.
//!
//! Usage limits: when an agent says it is out of usage, its queued tasks wait until the reset time
//! it gave (or run on the task's fallback agent). With no time given, nekotrail waits 30 minutes,
//! then doubles the wait each time it hits the limit again, up to 5 hours. Once the wait is over,
//! only one task tries that agent until a turn of it ends well, so a still-limited agent costs one
//! turn, never three.

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, TimeZone};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// Where the queue is kept (the settings table), as one JSON value.
pub const SETTING: &str = "queue";
/// How many tasks run at once, overall and per repo, until you change it.
pub const DEFAULT_LIMIT: u32 = 3;
/// The most either limit can be set to.
pub const MAX_LIMIT: u32 = 16;
/// The longest task prompt.
pub const MAX_PROMPT: usize = 100_000;
/// After the time an agent gave, wait this much longer: its clock and ours may differ a little.
const RESET_GRACE: i64 = 60;
/// No reset time given: wait this long the first time, doubling up to `BACKOFF_MAX`.
const BACKOFF_FIRST: i64 = 30 * 60;
const BACKOFF_MAX: i64 = 5 * 3600;

pub const MODES: [&str; 4] = ["plan", "ask", "acceptEdits", "bypassPermissions"];

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// In line to start.
    #[default]
    Queued,
    /// Set to Ask before changes: it needs you there, so it waits for you to start it.
    Waiting,
    /// Its workspace is being made.
    Starting,
    /// The agent is working (or setup is running first).
    Running,
    /// The turn ended well and the "then" steps run: checks, a draft PR, autopilot.
    Then,
    Done,
    NeedsYou,
    Failed,
}

impl State {
    /// Holds a slot: counted against the limits.
    pub fn busy(self) -> bool {
        matches!(self, State::Starting | State::Running)
    }
    pub fn finished(self) -> bool {
        matches!(self, State::Done | State::NeedsYou | State::Failed)
    }
}

/// What happens after the agent's turn ends well.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Then {
    #[serde(default)]
    pub checks: bool,
    #[serde(default)]
    pub draft_pr: bool,
    /// Autopilot needs a PR, so it opens a draft PR first.
    #[serde(default)]
    pub autopilot: bool,
}

impl Then {
    pub fn any(&self) -> bool {
        self.checks || self.draft_pr || self.autopilot
    }
}

/// A task as you add it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct NewTask {
    pub prompt: String,
    pub repo_id: String,
    #[serde(default)]
    pub base: Option<String>,
    pub agent: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    pub mode: String,
    /// The agent to use instead while `agent` is out of usage.
    #[serde(default)]
    pub fallback: Option<String>,
    #[serde(default)]
    pub then: Then,
}

impl NewTask {
    pub fn validate(&self) -> Result<(), String> {
        let prompt = self.prompt.trim();
        if prompt.is_empty() {
            return Err("Write the task first.".into());
        }
        if prompt.chars().count() > MAX_PROMPT {
            return Err(format!("That task is too long (over {MAX_PROMPT} characters)."));
        }
        if self.repo_id.trim().is_empty() {
            return Err("Pick a repo for the task.".into());
        }
        let known = |a: &str| crate::agent::ALL.iter().any(|x| x.id() == a);
        if !known(&self.agent) {
            return Err(format!("There's no agent called \"{}\".", self.agent));
        }
        if let Some(f) = &self.fallback {
            if !known(f) {
                return Err(format!("There's no agent called \"{f}\"."));
            }
            if f == &self.agent {
                return Err("The fallback agent has to be a different agent.".into());
            }
        }
        if !MODES.contains(&self.mode.as_str()) {
            return Err(format!("\"{}\" isn't a permission mode.", self.mode));
        }
        if let Some(b) = &self.base
            && !crate::safe::branch_name(b) {
                return Err(format!("\"{b}\" isn't a valid branch name."));
            }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Task {
    pub id: String,
    pub prompt: String,
    pub repo_id: String,
    #[serde(default)]
    pub base: Option<String>,
    pub agent: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    pub mode: String,
    #[serde(default)]
    pub fallback: Option<String>,
    #[serde(default)]
    pub then: Then,
    #[serde(default)]
    pub state: State,
    /// What happened, in plain words: why it needs you, why it failed, what it did.
    #[serde(default)]
    pub note: String,
    /// The part running now: "setup", "checks", "pr" or "autopilot". Empty otherwise.
    #[serde(default)]
    pub step: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// The agent that ran it last: the fallback, when the first one was out of usage.
    #[serde(default)]
    pub ran_with: Option<String>,
    /// Pick it up again in the workspace it already has (after a limit, a restart or a retry).
    #[serde(default)]
    pub resume: bool,
    /// How many times it was retried.
    #[serde(default)]
    pub attempts: u32,
    /// Epoch seconds.
    pub added_at: i64,
    #[serde(default)]
    pub started_at: Option<i64>,
    #[serde(default)]
    pub ended_at: Option<i64>,
}

impl Task {
    /// The first line of the prompt, short: how the task is listed.
    pub fn title(&self) -> String {
        let line = self.prompt.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
        if line.chars().count() > 80 { format!("{}...", line.chars().take(77).collect::<String>()) } else { line.to_string() }
    }
}

/// An agent that said it is out of usage.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Limit {
    /// Epoch seconds when its tasks may start again.
    pub until: i64,
    /// The agent said when. False: nekotrail picked the wait (no time in its message).
    pub reported: bool,
    /// Limit reports with no time in a row, for the doubling wait.
    #[serde(default)]
    pub strikes: u32,
    /// What the agent said, short.
    #[serde(default)]
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Queue {
    /// In order: the first queued task starts first.
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub paused: bool,
    #[serde(default = "default_limit")]
    pub max_total: u32,
    #[serde(default = "default_limit")]
    pub max_per_repo: u32,
    /// Agent id -> its usage limit, until a turn of it ends well again.
    #[serde(default)]
    pub limits: HashMap<String, Limit>,
    /// Tasks that ended after this (epoch seconds) are in the next summary.
    #[serde(default)]
    pub summary_seen: i64,
}

fn default_limit() -> u32 {
    DEFAULT_LIMIT
}

impl Default for Queue {
    fn default() -> Self {
        Self { tasks: vec![], paused: false, max_total: DEFAULT_LIMIT, max_per_repo: DEFAULT_LIMIT, limits: HashMap::new(), summary_seen: 0 }
    }
}

/// Why a queued task hasn't started.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Wait {
    /// You paused the queue.
    Paused,
    /// Its agent (and fallback, if any) is out of usage until `until`.
    Limit { agent: String, until: i64, reported: bool },
    /// Its agent's limit is over and one task is checking that it really is.
    Probe { agent: String },
    /// `max_total` tasks are running.
    Slots,
    /// `max_per_repo` tasks are running in its repo.
    RepoSlots,
}

/// How a turn of any chat ended, for limits and the queue.
#[derive(Debug, Clone, Default)]
pub struct TurnEnd<'a> {
    pub session_id: &'a str,
    pub agent: &'a str,
    pub ok: bool,
    pub stopped: bool,
    /// Why you are needed: plan, question, login, limit, install, error, protected.
    pub needs: Option<&'a str>,
    /// The agent's words when it hit its limit.
    pub limit_text: Option<&'a str>,
    /// The reset time in those words, if any (see `parse_reset`).
    pub reset: Option<i64>,
}

/// What the runner does after `turn_ended`.
#[derive(Debug, PartialEq)]
pub enum After {
    Nothing,
    /// Run this task's "then" steps.
    Steps(String),
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Summary {
    pub done: u32,
    pub needs_you: u32,
    pub failed: u32,
    /// Still to go, for the line under it.
    pub running: u32,
    pub queued: u32,
}

impl Summary {
    pub fn is_empty(&self) -> bool {
        self.done + self.needs_you + self.failed == 0
    }
    /// "8 done, 2 need you, 1 failed".
    pub fn text(&self) -> String {
        let mut parts = vec![];
        if self.done > 0 {
            parts.push(format!("{} done", self.done));
        }
        if self.needs_you > 0 {
            parts.push(format!("{} {} you", self.needs_you, if self.needs_you == 1 { "needs" } else { "need" }));
        }
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        parts.join(", ")
    }
}

/// Plain names for notes. The frontend has its own (agents.ts).
pub fn agent_name(id: &str) -> &'static str {
    match id {
        "codex" => "Codex",
        "gemini" => "Gemini",
        "opencode" => "OpenCode",
        "cursor" => "Cursor",
        "copilot" => "Copilot",
        "goose" => "Goose",
        "qwen" => "Qwen",
        "kimi" => "Kimi",
        "auggie" => "Auggie",
        "grok" => "Grok",
        "kiro" => "Kiro",
        "droid" => "Droid",
        "vibe" => "Vibe",
        "cline" => "Cline",
        "kilo" => "Kilo",
        _ => "Claude",
    }
}

/// Can this agent run in this mode with nobody there? "Ask before changes" stops for you on every
/// edit, and ACP agents stop to ask before commands unless they're on Full auto.
pub fn unattended(agent: &str, mode: &str) -> bool {
    match mode {
        "ask" => false,
        "bypassPermissions" => true,
        _ => !crate::agent::get(agent).interactive(mode),
    }
}

/// How long to wait after the `strikes`-th limit in a row with no reset time.
fn backoff(strikes: u32) -> i64 {
    let n = strikes.clamp(1, 10) - 1;
    (BACKOFF_FIRST << n).min(BACKOFF_MAX)
}

impl Queue {
    pub fn from_json(text: Option<&str>) -> Queue {
        text.and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn get(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Result<&mut Task, String> {
        self.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| "That task isn't in the queue anymore.".to_string())
    }

    /// nekotrail just started. Turns that were running died with the old window: put them back in
    /// line to pick up where they were. Steps that were half done need a look.
    pub fn after_restart(&mut self) {
        for t in &mut self.tasks {
            match t.state {
                State::Starting | State::Running => {
                    t.state = State::Queued;
                    t.resume = t.workspace_id.is_some();
                    t.step.clear();
                    t.note = "nekotrail closed while this ran. It picks up where it was.".into();
                }
                State::Then if t.step != "autopilot" => {
                    let what = match t.step.as_str() { "checks" => "the checks", _ => "opening the PR" };
                    t.state = State::NeedsYou;
                    t.note = format!("nekotrail closed before {what} finished.");
                    t.step.clear();
                }
                _ => {}
            }
        }
    }

    /// Add a task at the end of the line. Tasks that need you there (Ask before changes) wait for
    /// you to start them.
    pub fn add(&mut self, new: NewTask, id: String, now: i64) -> &Task {
        let state = if unattended(&new.agent, &new.mode) { State::Queued } else { State::Waiting };
        self.tasks.push(Task {
            id, prompt: new.prompt.trim().to_string(), repo_id: new.repo_id, base: new.base.filter(|b| !b.trim().is_empty()),
            agent: new.agent, model: new.model.filter(|m| !m.trim().is_empty()), effort: new.effort.filter(|e| !e.trim().is_empty()),
            mode: new.mode, fallback: new.fallback, then: new.then, state, added_at: now, ..Default::default()
        });
        self.tasks.last().unwrap()
    }

    pub fn remove(&mut self, id: &str) -> Result<Task, String> {
        let i = self.tasks.iter().position(|t| t.id == id).ok_or("That task isn't in the queue anymore.")?;
        if self.tasks[i].state.busy() {
            return Err("This task is running. Stop its agent in the workspace first.".into());
        }
        Ok(self.tasks.remove(i))
    }

    /// Drag and drop: put `id` right before `before`, or last when `before` is None.
    pub fn move_before(&mut self, id: &str, before: Option<&str>) -> Result<(), String> {
        if before == Some(id) {
            return Ok(());
        }
        let i = self.tasks.iter().position(|t| t.id == id).ok_or("That task isn't in the queue anymore.")?;
        let t = self.tasks.remove(i);
        let at = match before {
            Some(b) => self.tasks.iter().position(|x| x.id == b).unwrap_or(self.tasks.len()),
            None => self.tasks.len(),
        };
        self.tasks.insert(at, t);
        Ok(())
    }

    /// Put a task that failed or needs you back in line. It continues in its workspace if it has one.
    pub fn retry(&mut self, id: &str) -> Result<(), String> {
        let t = self.get_mut(id)?;
        if !matches!(t.state, State::Failed | State::NeedsYou) {
            return Err("Only a task that failed or needs you can be retried.".into());
        }
        t.state = if unattended(&t.agent, &t.mode) { State::Queued } else { State::Waiting };
        t.attempts += 1;
        t.resume = t.workspace_id.is_some();
        t.note.clear();
        t.step.clear();
        t.ended_at = None;
        Ok(())
    }

    /// You started a waiting task (or one in line) yourself: it starts now, whatever the limits say.
    pub fn start_now(&mut self, id: &str, now: i64) -> Result<String, String> {
        let t = self.get_mut(id)?;
        if !matches!(t.state, State::Waiting | State::Queued) {
            return Err("This task has already started.".into());
        }
        let agent = t.agent.clone();
        Self::mark_starting(t, &agent, now);
        Ok(agent)
    }

    fn mark_starting(t: &mut Task, agent: &str, now: i64) {
        t.state = State::Starting;
        t.ran_with = Some(agent.to_string());
        t.started_at = Some(now);
        t.ended_at = None;
        t.step.clear();
        if agent != t.agent {
            t.note = format!("{} is out of usage, so {} runs it.", agent_name(&t.agent), agent_name(agent));
        } else {
            t.note.clear();
        }
    }

    /// Tasks holding a slot. A task whose agent is waiting for your OK on a command doesn't: it
    /// needs you, so the next one starts.
    fn busy<'a>(&'a self, asking: &'a HashSet<String>) -> impl Iterator<Item = &'a Task> + 'a {
        self.tasks.iter().filter(move |t| t.state.busy() && !asking.contains(&t.id))
    }

    /// Agent id -> tasks running on it right now (for the one-try-after-a-limit rule).
    fn in_flight(&self) -> HashMap<String, u32> {
        let mut m = HashMap::new();
        for t in self.tasks.iter().filter(|t| t.state.busy()) {
            *m.entry(t.ran_with.clone().unwrap_or_else(|| t.agent.clone())).or_insert(0) += 1;
        }
        m
    }

    /// Out of usage right now: the time it may run again.
    pub fn limited(&self, agent: &str, now: i64) -> Option<&Limit> {
        self.limits.get(agent).filter(|l| l.until > now)
    }

    /// Free to start a task on: no limit, or its wait is over and no other task is trying it.
    fn agent_free(&self, agent: &str, in_flight: &HashMap<String, u32>, now: i64) -> bool {
        match self.limits.get(agent) {
            None => true,
            Some(l) if l.until > now => false,
            Some(_) => in_flight.get(agent).copied().unwrap_or(0) == 0,
        }
    }

    /// The agent a task runs on now: its own, else its fallback. None: both are out of usage.
    fn pick_agent(&self, t: &Task, in_flight: &HashMap<String, u32>, now: i64) -> Option<String> {
        if self.agent_free(&t.agent, in_flight, now) {
            return Some(t.agent.clone());
        }
        t.fallback.as_ref()
            .filter(|f| **f != t.agent && unattended(f, &t.mode) && self.agent_free(f, in_flight, now))
            .cloned()
    }

    /// The tasks to start now and the agent for each, in queue order, within the limits. Pure:
    /// `claim` marks them as starting.
    pub fn plan(&self, asking: &HashSet<String>, now: i64) -> Vec<(String, String)> {
        if self.paused {
            return vec![];
        }
        let max_total = self.max_total.clamp(1, MAX_LIMIT);
        let max_repo = self.max_per_repo.clamp(1, MAX_LIMIT);
        let mut total = self.busy(asking).count() as u32;
        let mut per_repo: HashMap<String, u32> = HashMap::new();
        for t in self.busy(asking) {
            *per_repo.entry(t.repo_id.clone()).or_insert(0) += 1;
        }
        let mut in_flight = self.in_flight();
        let mut out = vec![];
        for t in self.tasks.iter().filter(|t| t.state == State::Queued) {
            if total >= max_total {
                break;
            }
            if per_repo.get(&t.repo_id).copied().unwrap_or(0) >= max_repo {
                continue;
            }
            let Some(agent) = self.pick_agent(t, &in_flight, now) else { continue };
            total += 1;
            *per_repo.entry(t.repo_id.clone()).or_insert(0) += 1;
            *in_flight.entry(agent.clone()).or_insert(0) += 1;
            out.push((t.id.clone(), agent));
        }
        out
    }

    /// Plan and mark what starts, in one step (so two ticks at once can't start a task twice).
    pub fn claim(&mut self, asking: &HashSet<String>, now: i64) -> Vec<(String, String)> {
        let starts = self.plan(asking, now);
        for (id, agent) in &starts {
            if let Ok(t) = self.get_mut(id) {
                Self::mark_starting(t, agent, now);
            }
        }
        starts
    }

    /// Why each queued task hasn't started, for the Queue view.
    pub fn waits(&self, asking: &HashSet<String>, now: i64) -> HashMap<String, Wait> {
        let starting: HashSet<String> = self.plan(asking, now).into_iter().map(|(id, _)| id).collect();
        let in_flight = self.in_flight();
        let total = self.busy(asking).count() as u32;
        let mut out = HashMap::new();
        for t in self.tasks.iter().filter(|t| t.state == State::Queued && !starting.contains(&t.id)) {
            let wait = if self.paused {
                Wait::Paused
            } else if self.pick_agent(t, &in_flight, now).is_none() {
                match self.limited(&t.agent, now) {
                    Some(l) => Wait::Limit { agent: t.agent.clone(), until: l.until, reported: l.reported },
                    None => Wait::Probe { agent: t.agent.clone() },
                }
            } else if total >= self.max_total.clamp(1, MAX_LIMIT) {
                Wait::Slots
            } else {
                Wait::RepoSlots
            };
            out.insert(t.id.clone(), wait);
        }
        out
    }

    /// The workspace and chat a starting task runs in.
    pub fn started(&mut self, id: &str, workspace_id: &str, session_id: &str) -> Result<(), String> {
        let t = self.get_mut(id)?;
        t.state = State::Running;
        t.workspace_id = Some(workspace_id.to_string());
        t.session_id = Some(session_id.to_string());
        t.resume = false;
        Ok(())
    }

    pub fn set_step(&mut self, id: &str, step: &str) {
        if let Ok(t) = self.get_mut(id) {
            t.step = step.to_string();
        }
    }

    /// End a task: Done, NeedsYou or Failed, with what to tell you.
    pub fn finish(&mut self, id: &str, state: State, note: &str, now: i64) {
        if let Ok(t) = self.get_mut(id) {
            t.state = state;
            t.note = note.to_string();
            t.step.clear();
            t.ended_at = Some(now);
        }
    }

    /// An agent said it is out of usage. `reset` is the time it gave, if any.
    pub fn record_limit(&mut self, agent: &str, text: &str, reset: Option<i64>, now: i64) {
        let strikes = self.limits.get(agent).map(|l| l.strikes).unwrap_or(0);
        let text: String = text.trim().chars().take(300).collect();
        let limit = match reset.filter(|r| *r > now) {
            Some(r) => Limit { until: r + RESET_GRACE, reported: true, strikes: 0, text },
            None => Limit { until: now + backoff(strikes + 1), reported: false, strikes: strikes + 1, text },
        };
        self.limits.insert(agent.to_string(), limit);
    }

    /// A turn of any chat ended. Usage limits are tracked for every chat (yours too), so the queue
    /// never starts a task on an agent that just said it is out. Returns what to do next for the
    /// queue task this chat belongs to, if it does.
    pub fn turn_ended(&mut self, e: &TurnEnd, now: i64) -> After {
        if e.needs == Some("limit") {
            self.record_limit(e.agent, e.limit_text.unwrap_or(""), e.reset, now);
        } else if e.ok && !e.stopped {
            self.limits.remove(e.agent);
        }
        let Some(t) = self.tasks.iter_mut().find(|t| t.session_id.as_deref() == Some(e.session_id) && t.state == State::Running) else {
            return After::Nothing;
        };
        let name = agent_name(e.agent);
        let error = || "The agent stopped with an error. Its chat says why.".to_string();
        let (state, note) = match e.needs {
            // Back in line in the same workspace: it continues when the limit resets, or on the fallback.
            Some("limit") => {
                t.state = State::Queued;
                t.resume = true;
                t.started_at = None;
                t.note = format!("{name} ran out of usage. It continues when the limit resets.");
                return After::Nothing;
            }
            _ if e.stopped => (State::NeedsYou, "You stopped it.".to_string()),
            Some("plan") => (State::NeedsYou, "The plan waits for your OK.".into()),
            Some("question") => (State::NeedsYou, "The agent asked you a question.".into()),
            Some("login") => (State::NeedsYou, format!("{name} needs you to sign in.")),
            Some("install") => (State::Failed, format!("{name} isn't installed.")),
            Some("protected") => (State::NeedsYou, "It changed a protected file.".into()),
            Some(_) => (State::Failed, error()),
            None if !e.ok => (State::Failed, error()),
            None if t.then.any() => {
                t.state = State::Then;
                t.note.clear();
                return After::Steps(t.id.clone());
            }
            None => (State::Done, "Finished.".into()),
        };
        t.state = state;
        t.note = note;
        t.step.clear();
        t.ended_at = Some(now);
        After::Nothing
    }

    /// Autopilot stopped following a task's PR: "ready", "gave_up", "off" (closed or merged) or
    /// "stopped" (you turned it off).
    pub fn autopilot_ended(&mut self, workspace_id: &str, how: &str, now: i64) -> bool {
        let Some(t) = self.tasks.iter().find(|t| t.workspace_id.as_deref() == Some(workspace_id) && t.state == State::Then && t.step == "autopilot") else {
            return false;
        };
        let id = t.id.clone();
        let (state, note) = match how {
            "ready" => (State::Done, "Autopilot got the PR green. It's ready for you to merge."),
            "gave_up" => (State::NeedsYou, "Autopilot tried 3 times and the PR still isn't green."),
            "stopped" => (State::Done, "You turned autopilot off."),
            _ => (State::Done, "The PR was closed or merged."),
        };
        self.finish(&id, state, note, now);
        true
    }

    /// Tasks that ended since you last saw the summary, and what is still going.
    pub fn summary(&self) -> Summary {
        let mut s = Summary::default();
        for t in &self.tasks {
            if t.state.finished() && t.ended_at.is_some_and(|e| e > self.summary_seen) {
                match t.state {
                    State::Done => s.done += 1,
                    State::NeedsYou => s.needs_you += 1,
                    _ => s.failed += 1,
                }
            }
            match t.state {
                State::Starting | State::Running | State::Then => s.running += 1,
                State::Queued | State::Waiting => s.queued += 1,
                _ => {}
            }
        }
        s
    }

    /// Remove every finished task.
    pub fn clear_finished(&mut self) -> usize {
        let before = self.tasks.len();
        self.tasks.retain(|t| !t.state.finished());
        before - self.tasks.len()
    }

    /// Workspaces made by queue tasks that are gone (archived): their tasks forget them, so a
    /// retry starts fresh.
    pub fn forget_workspace(&mut self, workspace_id: &str) {
        for t in &mut self.tasks {
            if t.workspace_id.as_deref() == Some(workspace_id) && !t.state.busy() {
                t.workspace_id = None;
                t.session_id = None;
                t.resume = false;
            }
        }
    }
}

/// The message that continues a task in the chat it already has.
pub fn resume_prompt(t: &Task) -> String {
    format!("Pick this task up where you left off. Check what is already done in this workspace first, then finish it.\n\nThe task:\n{}", t.prompt)
}

/// The first message of a new chat in a workspace where another agent already started the task.
pub fn handover_prompt(t: &Task, from: &str) -> String {
    format!("{} started this task in this workspace and stopped partway. Check what is already done (git status and git diff), then finish it.\n\nThe task:\n{}", agent_name(from), t.prompt)
}

// ---------- reset times ----------

static EPOCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\|(\d{10})\b").unwrap());
static RELATIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:again|retry|resets?|wait)\s+(?:in|after)\s+((?:\d+(?:\.\d+)?\s*(?:days?|d|hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s)\b[\s,]*(?:and\s+)?)+)").unwrap()
});
static PART: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)?)\s*(days?|d|hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s)\b").unwrap());
static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:resets?|again|available)\s+(?:at\s+|on\s+)?(?:(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.?\s+(\d{1,2})(?:st|nd|rd|th)?,?\s+(?:at\s+)?)?(\d{1,2})(?::(\d{2}))?\s*(am|pm|a\.m\.|p\.m\.)?").unwrap()
});

/// When an agent's usage limit message says it resets, as epoch seconds. Understands Claude's
/// `...|1762400000`, "try again in 2 hours 13 minutes", "retry in 37s", and clock times like
/// "resets 3pm", "resets at 3:05 PM", "try again at 15:00" or "resets Nov 7, 3pm", read in local
/// time (`now`'s offset). A time zone in the message is not read. None when it says no time.
pub fn parse_reset(text: &str, now: DateTime<FixedOffset>) -> Option<i64> {
    let t = text.to_lowercase();
    if let Some(c) = EPOCH.captures(&t) {
        return c[1].parse().ok();
    }
    if let Some(c) = RELATIVE.captures(&t) {
        let mut secs = 0f64;
        for p in PART.captures_iter(&c[1]) {
            let n: f64 = p[1].parse().ok()?;
            let unit = match p[2].chars().next() { Some('d') => 86400.0, Some('h') => 3600.0, Some('m') => 60.0, _ => 1.0 };
            secs += n * unit;
        }
        if secs > 0.0 {
            return Some(now.timestamp() + secs.ceil() as i64);
        }
    }
    for c in CLOCK.captures_iter(&t) {
        let ampm = c.get(5).map(|m| m.as_str().starts_with('p'));
        let minute: u32 = match c.get(4) { Some(m) => m.as_str().parse().ok()?, None => 0 };
        // A bare number ("resets 5") is too unsure to wait on.
        if c.get(4).is_none() && ampm.is_none() {
            continue;
        }
        let mut hour: u32 = c[3].parse().ok()?;
        if let Some(pm) = ampm {
            if hour == 0 || hour > 12 {
                continue;
            }
            hour = hour % 12 + if pm { 12 } else { 0 };
        }
        if hour > 23 || minute > 59 {
            continue;
        }
        let tz = *now.offset();
        let at = |d: NaiveDate| d.and_hms_opt(hour, minute, 0).and_then(|n| tz.from_local_datetime(&n).single());
        let when = match (c.get(1), c.get(2)) {
            (Some(mon), Some(day)) => {
                let month = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|m| *m == mon.as_str())? as u32 + 1;
                let day: u32 = day.as_str().parse().ok()?;
                let this = at(NaiveDate::from_ymd_opt(now.year(), month, day)?)?;
                // "Jan 2" read on Dec 30 is next year.
                if this.timestamp() < now.timestamp() - 86400 { at(NaiveDate::from_ymd_opt(now.year() + 1, month, day)?)? } else { this }
            }
            _ => {
                let today = at(now.date_naive())?;
                if today.timestamp() <= now.timestamp() { at(now.date_naive().succ_opt()?)? } else { today }
            }
        };
        return Some(when.timestamp());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn new(prompt: &str, repo: &str, agent: &str) -> NewTask {
        NewTask { prompt: prompt.into(), repo_id: repo.into(), agent: agent.into(), mode: "acceptEdits".into(), ..Default::default() }
    }

    fn queue(tasks: &[(&str, &str, &str)]) -> Queue {
        let mut q = Queue::default();
        for (i, (id, repo, agent)) in tasks.iter().enumerate() {
            q.add(new(&format!("task {id}"), repo, agent), id.to_string(), NOW + i as i64);
        }
        q
    }

    fn ids(starts: &[(String, String)]) -> Vec<&str> {
        starts.iter().map(|(id, _)| id.as_str()).collect()
    }

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    /// The task's chat ended its turn the way the arguments say.
    #[allow(clippy::too_many_arguments)]
    fn end_turn(q: &mut Queue, id: &str, agent: &str, ok: bool, needs: Option<&str>, text: Option<&str>, reset: Option<i64>, now: i64) -> After {
        let sid = format!("s-{id}");
        q.started(id, &format!("w-{id}"), &sid).unwrap();
        q.turn_ended(&TurnEnd { session_id: &sid, agent, ok, stopped: false, needs, limit_text: text, reset }, now)
    }

    #[test]
    fn starts_up_to_the_global_limit_in_order() {
        let mut q = queue(&[("a", "r1", "claude"), ("b", "r2", "claude"), ("c", "r3", "claude"), ("d", "r4", "claude"), ("e", "r5", "claude")]);
        assert_eq!(q.max_total, 3, "three at a time until you change it");
        assert_eq!(ids(&q.claim(&none(), NOW)), ["a", "b", "c"]);
        // Nothing more until one ends.
        assert!(q.claim(&none(), NOW).is_empty());
        assert_eq!(q.waits(&none(), NOW)["d"], Wait::Slots);
        end_turn(&mut q, "b", "claude", true, None, None, None, NOW);
        assert_eq!(q.get("b").unwrap().state, State::Done);
        assert_eq!(ids(&q.claim(&none(), NOW)), ["d"]);
        q.max_total = 5;
        assert_eq!(ids(&q.claim(&none(), NOW)), ["e"]);
    }

    #[test]
    fn per_repo_limit_skips_to_another_repo() {
        let mut q = queue(&[("a", "r1", "claude"), ("b", "r1", "claude"), ("c", "r1", "claude"), ("d", "r2", "codex")]);
        q.max_per_repo = 2;
        q.max_total = 3;
        // c waits for a repo slot, so d (another repo) goes ahead of it.
        assert_eq!(ids(&q.claim(&none(), NOW)), ["a", "b", "d"]);
        q.max_total = 4;
        assert_eq!(q.waits(&none(), NOW)["c"], Wait::RepoSlots);
        end_turn(&mut q, "a", "claude", true, None, None, None, NOW);
        assert_eq!(ids(&q.claim(&none(), NOW)), ["c"]);
    }

    #[test]
    fn a_task_waiting_for_your_ok_frees_its_slot() {
        let mut q = queue(&[("a", "r1", "claude"), ("b", "r1", "claude")]);
        q.max_total = 1;
        assert_eq!(ids(&q.claim(&none(), NOW)), ["a"]);
        q.started("a", "w", "s").unwrap();
        assert!(q.claim(&none(), NOW).is_empty());
        let asking: HashSet<String> = ["a".to_string()].into();
        assert_eq!(ids(&q.claim(&asking, NOW)), ["b"]);
    }

    #[test]
    fn pause_stops_new_starts_and_ask_mode_waits_for_you() {
        let mut q = queue(&[("a", "r1", "claude")]);
        let mut ask = new("careful one", "r1", "claude");
        ask.mode = "ask".into();
        q.add(ask, "b".into(), NOW);
        assert_eq!(q.get("b").unwrap().state, State::Waiting, "Ask before changes needs you there");
        // ACP agents ask before commands unless on Full auto.
        let mut acp = new("x", "r1", "opencode");
        q.add(acp.clone(), "c".into(), NOW);
        assert_eq!(q.get("c").unwrap().state, State::Waiting);
        acp.mode = "bypassPermissions".into();
        q.add(acp, "d".into(), NOW);
        assert_eq!(q.get("d").unwrap().state, State::Queued);
        q.paused = true;
        assert!(q.claim(&none(), NOW).is_empty());
        assert_eq!(q.waits(&none(), NOW)["a"], Wait::Paused);
        q.paused = false;
        assert_eq!(ids(&q.claim(&none(), NOW)), ["a", "d"], "waiting tasks never start by themselves");
        assert_eq!(q.start_now("b", NOW).unwrap(), "claude");
        assert_eq!(q.get("b").unwrap().state, State::Starting);
        assert!(q.start_now("b", NOW).is_err());
    }

    #[test]
    fn a_limit_pauses_that_agent_until_the_reset_it_reported() {
        let mut q = queue(&[("a", "r1", "claude"), ("b", "r2", "claude"), ("c", "r3", "codex")]);
        q.max_total = 1;
        assert_eq!(ids(&q.claim(&none(), NOW)), ["a"]);
        let reset = NOW + 3600;
        let after = end_turn(&mut q, "a", "claude", false, Some("limit"), Some("Claude AI usage limit reached|1800003600"), Some(reset), NOW);
        assert_eq!(after, After::Nothing);
        let a = q.get("a").unwrap();
        assert_eq!(a.state, State::Queued, "back in line, in its own workspace");
        assert!(a.resume && a.workspace_id.is_some());
        assert!(a.note.contains("ran out of usage"));
        // Claude's tasks wait; Codex's goes ahead.
        assert_eq!(ids(&q.claim(&none(), NOW + 10)), ["c"]);
        match &q.waits(&none(), NOW + 10)["a"] {
            Wait::Limit { agent, until, reported } => assert!(agent == "claude" && *until == reset + RESET_GRACE && *reported),
            w => panic!("{w:?}"),
        }
        end_turn(&mut q, "c", "codex", true, None, None, None, NOW + 20);
        // Still before the reset: nothing starts, however often it looks.
        for t in [NOW + 30, NOW + 1800, reset] {
            assert!(q.claim(&none(), t).is_empty(), "never hammer before the reset");
        }
        // After the reset, one task tries Claude, not all of them at once.
        q.max_total = 3;
        assert_eq!(ids(&q.claim(&none(), reset + RESET_GRACE + 1)), ["a"]);
        assert!(q.claim(&none(), reset + RESET_GRACE + 2).is_empty());
        assert_eq!(q.waits(&none(), reset + RESET_GRACE + 2)["b"], Wait::Probe { agent: "claude".into() });
        // It worked: the limit is gone and the rest start.
        end_turn(&mut q, "a", "claude", true, None, None, None, reset + 100);
        assert!(!q.limits.contains_key("claude"));
        assert_eq!(ids(&q.claim(&none(), reset + 101)), ["b"]);
    }

    #[test]
    fn a_limit_with_no_time_backs_off_and_doubles() {
        let mut q = queue(&[("a", "r1", "codex")]);
        q.claim(&none(), NOW);
        end_turn(&mut q, "a", "codex", false, Some("limit"), Some("You've hit your usage limit."), None, NOW);
        assert_eq!(q.limits["codex"].until, NOW + 30 * 60);
        assert!(!q.limits["codex"].reported);
        assert!(q.claim(&none(), NOW + 29 * 60).is_empty());
        let t2 = NOW + 31 * 60;
        assert_eq!(ids(&q.claim(&none(), t2)), ["a"]);
        end_turn(&mut q, "a", "codex", false, Some("limit"), Some("still out"), None, t2);
        assert_eq!(q.limits["codex"].until, t2 + 60 * 60, "the second wait is twice as long");
        for s in 3..12 {
            q.record_limit("codex", "", None, NOW);
            assert!(q.limits["codex"].until - NOW <= 5 * 3600, "strike {s} waits at most 5 hours");
        }
        // A reset time in the past counts as no time.
        q.limits.clear();
        q.record_limit("codex", "", Some(NOW - 10), NOW);
        assert_eq!(q.limits["codex"].until, NOW + 30 * 60);
    }

    #[test]
    fn limits_from_your_own_chats_count_too() {
        let mut q = queue(&[("a", "r1", "claude")]);
        q.turn_ended(&TurnEnd { session_id: "not-a-task", agent: "claude", ok: false, needs: Some("limit"), reset: Some(NOW + 600), ..Default::default() }, NOW);
        assert!(q.claim(&none(), NOW).is_empty());
        // And a good turn of that agent anywhere lifts it.
        q.turn_ended(&TurnEnd { session_id: "not-a-task", agent: "claude", ok: true, ..Default::default() }, NOW + 5);
        assert_eq!(ids(&q.claim(&none(), NOW + 6)), ["a"]);
    }

    #[test]
    fn fallback_agent_runs_the_task_while_the_first_is_out() {
        let mut q = Queue::default();
        let mut t = new("fix it", "r1", "claude");
        t.fallback = Some("codex".into());
        q.add(t, "a".into(), NOW);
        q.add(new("no fallback", "r1", "claude"), "b".into(), NOW);
        q.record_limit("claude", "limit", Some(NOW + 3600), NOW);
        let starts = q.claim(&none(), NOW);
        assert_eq!(starts, [("a".to_string(), "codex".to_string())]);
        let a = q.get("a").unwrap();
        assert_eq!(a.ran_with.as_deref(), Some("codex"));
        assert!(a.note.contains("Codex runs it"));
        assert!(matches!(q.waits(&none(), NOW)["b"], Wait::Limit { .. }));
        // Both out: it waits.
        let mut q2 = Queue::default();
        let mut t = new("x", "r1", "claude");
        t.fallback = Some("codex".into());
        q2.add(t, "a".into(), NOW);
        q2.record_limit("claude", "", Some(NOW + 100), NOW);
        q2.record_limit("codex", "", Some(NOW + 100), NOW);
        assert!(q2.claim(&none(), NOW).is_empty());
        // A fallback that would stop to ask (OpenCode outside Full auto) isn't used unattended.
        let mut q3 = Queue::default();
        let mut t = new("x", "r1", "claude");
        t.fallback = Some("opencode".into());
        q3.add(t, "a".into(), NOW);
        q3.record_limit("claude", "", Some(NOW + 100), NOW);
        assert!(q3.claim(&none(), NOW).is_empty());
    }

    #[test]
    fn turn_endings_map_to_states() {
        let mut q = queue(&[("a", "r", "claude"), ("b", "r", "claude"), ("c", "r", "claude"), ("d", "r", "claude"), ("e", "r", "claude")]);
        q.max_total = 9;
        q.claim(&none(), NOW);
        end_turn(&mut q, "a", "claude", true, Some("plan"), None, None, NOW);
        assert_eq!((q.get("a").unwrap().state, q.get("a").unwrap().note.as_str()), (State::NeedsYou, "The plan waits for your OK."));
        end_turn(&mut q, "b", "claude", false, Some("error"), None, None, NOW);
        assert_eq!(q.get("b").unwrap().state, State::Failed);
        end_turn(&mut q, "c", "claude", false, Some("login"), None, None, NOW);
        assert_eq!(q.get("c").unwrap().note, "Claude needs you to sign in.");
        end_turn(&mut q, "d", "claude", true, Some("protected"), None, None, NOW);
        assert_eq!(q.get("d").unwrap().state, State::NeedsYou);
        q.started("e", "w", "se").unwrap();
        q.turn_ended(&TurnEnd { session_id: "se", agent: "claude", ok: false, stopped: true, ..Default::default() }, NOW);
        assert_eq!(q.get("e").unwrap().note, "You stopped it.");
        assert!(q.tasks.iter().all(|t| t.ended_at == Some(NOW)));
    }

    #[test]
    fn then_steps_run_after_a_good_turn_and_autopilot_ends_them() {
        let mut q = Queue::default();
        let mut t = new("x", "r", "claude");
        t.then = Then { checks: true, draft_pr: true, autopilot: true };
        q.add(t, "a".into(), NOW);
        q.claim(&none(), NOW);
        assert_eq!(end_turn(&mut q, "a", "claude", true, None, None, None, NOW), After::Steps("a".into()));
        assert_eq!(q.get("a").unwrap().state, State::Then);
        assert!(q.claim(&none(), NOW).is_empty() && q.plan(&none(), NOW).is_empty());
        q.set_step("a", "autopilot");
        assert!(!q.autopilot_ended("other", "ready", NOW));
        assert!(q.autopilot_ended("w-a", "gave_up", NOW + 5));
        assert_eq!(q.get("a").unwrap().state, State::NeedsYou);
        assert_eq!(q.get("a").unwrap().ended_at, Some(NOW + 5));
    }

    #[test]
    fn steps_do_not_hold_a_slot() {
        let mut q = queue(&[("a", "r", "claude"), ("b", "r", "claude")]);
        q.tasks[0].then.draft_pr = true;
        q.max_total = 1;
        q.claim(&none(), NOW);
        end_turn(&mut q, "a", "claude", true, None, None, None, NOW);
        assert_eq!(ids(&q.claim(&none(), NOW)), ["b"]);
    }

    #[test]
    fn retry_continues_in_its_workspace_and_reorder_moves_tasks() {
        let mut q = queue(&[("a", "r", "claude"), ("b", "r", "claude"), ("c", "r", "claude")]);
        q.claim(&none(), NOW);
        end_turn(&mut q, "a", "claude", false, Some("error"), None, None, NOW);
        assert!(q.retry("b").is_err(), "b is running");
        q.retry("a").unwrap();
        let a = q.get("a").unwrap();
        assert_eq!((a.state, a.attempts, a.resume, a.ended_at), (State::Queued, 1, true, None));
        assert!(a.note.is_empty());
        assert!(q.remove("b").is_err(), "a running task can't be removed");
        q.move_before("c", Some("a")).unwrap();
        assert_eq!(q.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["c", "a", "b"]);
        q.move_before("c", None).unwrap();
        assert_eq!(q.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        q.move_before("a", Some("a")).unwrap();
        assert!(q.move_before("zz", None).is_err());
        q.forget_workspace("w-a");
        assert!(!q.get("a").unwrap().resume && q.get("a").unwrap().workspace_id.is_none());
    }

    #[test]
    fn persists_and_recovers_after_a_restart() {
        let mut q = queue(&[("a", "r", "claude"), ("b", "r", "claude"), ("c", "r", "claude"), ("d", "r", "claude")]);
        q.max_per_repo = 2;
        q.claim(&none(), NOW);
        q.started("a", "wa", "sa").unwrap();
        // b is still making its workspace (no workspace yet), c is in its steps.
        q.tasks[2].state = State::Then;
        q.tasks[2].step = "pr".into();
        q.record_limit("codex", "out", Some(NOW + 50), NOW);
        let saved = q.to_json();
        // Through the real settings table.
        let store = crate::store::Store::in_memory().unwrap();
        store.set_setting(SETTING, &saved).unwrap();
        let mut back = Queue::from_json(store.setting(SETTING).unwrap().as_deref());
        assert_eq!(back, q, "everything comes back as it was");
        back.after_restart();
        let get = |id: &str| back.get(id).unwrap().clone();
        assert_eq!((get("a").state, get("a").resume), (State::Queued, true), "picks up in its workspace");
        assert_eq!((get("b").state, get("b").resume), (State::Queued, false), "starts fresh");
        assert_eq!(get("c").state, State::NeedsYou);
        assert!(get("c").note.contains("opening the PR"));
        assert_eq!(get("d").state, State::Queued);
        assert_eq!(back.limits["codex"].until, NOW + 50 + RESET_GRACE);
        // Old or broken data starts an empty queue rather than failing.
        assert_eq!(Queue::from_json(Some("not json")), Queue::default());
        assert_eq!(Queue::from_json(Some("{}")).max_total, DEFAULT_LIMIT);
    }

    #[test]
    fn morning_summary_counts_what_ended_since_you_looked() {
        let mut q = queue(&[("a", "r", "claude"), ("b", "r", "claude"), ("c", "r", "claude"), ("d", "r", "claude"), ("e", "r", "claude")]);
        q.max_total = 4;
        q.claim(&none(), NOW);
        end_turn(&mut q, "a", "claude", true, None, None, None, NOW + 1);
        end_turn(&mut q, "b", "claude", true, None, None, None, NOW + 2);
        end_turn(&mut q, "c", "claude", true, Some("question"), None, None, NOW + 3);
        end_turn(&mut q, "d", "claude", false, Some("error"), None, None, NOW + 4);
        let s = q.summary();
        assert_eq!((s.done, s.needs_you, s.failed, s.running, s.queued), (2, 1, 1, 0, 1));
        assert_eq!(s.text(), "2 done, 1 needs you, 1 failed");
        // Shown once: after you've seen it, the same tasks don't count again.
        q.summary_seen = NOW + 4;
        assert!(q.summary().is_empty());
        q.claim(&none(), NOW + 5);
        end_turn(&mut q, "e", "claude", true, Some("plan"), None, None, NOW + 6);
        assert_eq!(q.summary().text(), "1 needs you");
        let many = Summary { done: 8, needs_you: 2, failed: 1, ..Default::default() };
        assert_eq!(many.text(), "8 done, 2 need you, 1 failed");
        assert_eq!(q.clear_finished(), 5);
    }

    #[test]
    fn new_tasks_are_checked() {
        assert!(new("fix it", "r", "claude").validate().is_ok());
        assert!(new("  ", "r", "claude").validate().is_err());
        assert!(new("x", "", "claude").validate().is_err());
        assert!(new("x", "r", "skynet").validate().is_err());
        let mut t = new("x", "r", "claude");
        t.fallback = Some("claude".into());
        assert!(t.validate().unwrap_err().contains("different"));
        t.fallback = Some("codex".into());
        t.base = Some("-evil".into());
        assert!(t.validate().is_err());
        t.base = Some("main".into());
        t.mode = "yolo".into();
        assert!(t.validate().is_err());
        assert_eq!(Task { prompt: "\n  Fix the login bug\nmore".into(), ..Default::default() }.title(), "Fix the login bug");
    }

    fn at(rfc: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(rfc).unwrap()
    }

    #[test]
    fn reads_reset_times_from_limit_messages() {
        // 22:30 local, at UTC+5:30.
        let now = at("2026-11-06T22:30:00+05:30");
        let ts = |s: &str| at(s).timestamp();
        assert_eq!(parse_reset("Claude AI usage limit reached|1762400000", now), Some(1762400000));
        assert_eq!(parse_reset("You've hit your usage limit. Try again in 2 hours 13 minutes.", now), Some(now.timestamp() + 2 * 3600 + 13 * 60));
        assert_eq!(parse_reset("Quota exceeded. Please retry in 37.2s.", now), Some(now.timestamp() + 38));
        assert_eq!(parse_reset("rate limited, resets in 1h 5m", now), Some(now.timestamp() + 3900));
        assert_eq!(parse_reset("Try again in 1 day", now), Some(now.timestamp() + 86400));
        // Clock times are local and the next one to come.
        assert_eq!(parse_reset("5-hour limit reached ∙ resets 3am", now), Some(ts("2026-11-07T03:00:00+05:30")));
        assert_eq!(parse_reset("You've hit your limit · resets 11pm (Asia/Kolkata)", now), Some(ts("2026-11-06T23:00:00+05:30")));
        assert_eq!(parse_reset("Usage limit reached. Try again at 3:05 PM.", now), Some(ts("2026-11-07T15:05:00+05:30")));
        assert_eq!(parse_reset("limit will reset at 23:45", now), Some(ts("2026-11-06T23:45:00+05:30")));
        assert_eq!(parse_reset("Weekly limit reached, resets Nov 9, 4pm", now), Some(ts("2026-11-09T16:00:00+05:30")));
        assert_eq!(parse_reset("resets Jan 2 at 9am", now), Some(ts("2027-01-02T09:00:00+05:30")));
        assert_eq!(parse_reset("Resets 12am", now), Some(ts("2026-11-07T00:00:00+05:30")));
        // No time, or nothing sure enough to wait on.
        assert_eq!(parse_reset("Usage limit reached.", now), None);
        assert_eq!(parse_reset("You have exhausted your daily quota on this model.", now), None);
        assert_eq!(parse_reset("resets 5", now), None);
        assert_eq!(parse_reset("try again later", now), None);
        assert_eq!(parse_reset("resets 25:00", now), None);
    }
}
