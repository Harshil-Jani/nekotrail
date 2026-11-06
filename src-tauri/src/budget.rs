//! Spend caps, so a night of queued tasks and autopilot can't run up a bill you didn't expect.
//!
//! Two caps, each set for all repos and, if you like, for one repo: a daily cap (resets at local
//! midnight) and a per-task cap. A task is one workspace: everything its agents spent there. Each
//! cap can count API-equivalent USD (agents that report cost), turns (every agent), or tokens
//! (new input and output, for agents that report them); any limit you set counts.
//!
//! What a cap stops: turns nobody is there for. The queue starts no new task and continues none,
//! and autopilot starts no new turn. A running turn always finishes (cost is only known when it
//! ends, so the total can go a little over). Messages you send yourself always go through.
//!
//! Also here: the runaway thresholds (see `runaway.rs`), saved with the caps.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Where the caps are kept (the settings table), as one JSON value.
pub const SETTING: &str = "budgets";

/// Limits for one scope. None: no limit.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Cap {
    #[serde(default)]
    pub usd: Option<f64>,
    #[serde(default)]
    pub turns: Option<u64>,
    #[serde(default)]
    pub tokens: Option<u64>,
}

impl Cap {
    pub fn is_empty(&self) -> bool {
        self.usd.is_none() && self.turns.is_none() && self.tokens.is_none()
    }
}

/// A daily cap and a per-task cap.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Caps {
    #[serde(default)]
    pub day: Cap,
    #[serde(default)]
    pub task: Cap,
}

/// When to stop an agent that is going in circles (see `runaway.rs`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Runaway {
    #[serde(default = "yes")]
    pub on: bool,
    /// The same command failed this many times in a row.
    #[serde(default = "default_fails")]
    pub same_failure: u32,
    /// A file went back to an earlier version this many times.
    #[serde(default = "default_flips")]
    pub back_and_forth: u32,
    /// This many turns in a row changed no file.
    #[serde(default = "default_idle")]
    pub no_change: u32,
}

fn yes() -> bool {
    true
}
fn default_fails() -> u32 {
    4
}
fn default_flips() -> u32 {
    2
}
fn default_idle() -> u32 {
    3
}

impl Default for Runaway {
    fn default() -> Self {
        Self { on: true, same_failure: default_fails(), back_and_forth: default_flips(), no_change: default_idle() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Budgets {
    /// For every repo.
    #[serde(default)]
    pub all: Caps,
    /// Repo id -> caps for that repo, on top of `all`.
    #[serde(default)]
    pub repos: HashMap<String, Caps>,
    #[serde(default)]
    pub runaway: Runaway,
}

/// The highest a cap can be set to, so a typo can't mean "no cap".
const MAX_USD: f64 = 100_000.0;
const MAX_TURNS: u64 = 100_000;
const MAX_TOKENS: u64 = 100_000_000_000;

impl Budgets {
    pub fn from_json(text: Option<&str>) -> Budgets {
        text.and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Check what the Usage view sends. Empty repo entries are dropped.
    pub fn validate(mut self) -> Result<Budgets, String> {
        let check = |c: &Cap| -> Result<(), String> {
            if let Some(u) = c.usd
                && (!u.is_finite() || u <= 0.0 || u > MAX_USD) {
                    return Err(format!("A dollar cap has to be more than $0 and at most ${MAX_USD:.0}."));
                }
            if c.turns.is_some_and(|t| t == 0 || t > MAX_TURNS) {
                return Err(format!("A turn cap has to be between 1 and {MAX_TURNS}."));
            }
            if c.tokens.is_some_and(|t| t == 0 || t > MAX_TOKENS) {
                return Err("A token cap has to be more than 0.".into());
            }
            Ok(())
        };
        for caps in std::iter::once(&self.all).chain(self.repos.values()) {
            check(&caps.day)?;
            check(&caps.task)?;
        }
        self.repos.retain(|_, c| !(c.day.is_empty() && c.task.is_empty()));
        let r = &mut self.runaway;
        r.same_failure = r.same_failure.clamp(2, 50);
        r.back_and_forth = r.back_and_forth.clamp(1, 50);
        r.no_change = r.no_change.clamp(2, 50);
        Ok(self)
    }

    pub fn any(&self) -> bool {
        !(self.all.day.is_empty() && self.all.task.is_empty()) || !self.repos.is_empty()
    }
}

/// What was used in some scope.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Spent {
    /// API-equivalent USD, from the turns that reported a cost.
    pub usd: f64,
    pub turns: u64,
    /// New input and output tokens, from the turns that reported tokens.
    pub tokens: u64,
}

impl Spent {
    pub fn add(&mut self, cost: Option<f64>, tokens: Option<u64>) {
        self.turns += 1;
        self.usd += cost.unwrap_or(0.0);
        self.tokens += tokens.unwrap_or(0);
    }
}

/// Today's spend, overall and per repo. Built once per look, from the store.
#[derive(Clone, Debug, Default)]
pub struct Today {
    pub all: Spent,
    pub repos: HashMap<String, Spent>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Day,
    Task,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    Usd,
    Turns,
    Tokens,
}

/// A cap that is reached.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Stop {
    pub scope: Scope,
    pub measure: Measure,
    /// The repo whose own cap it is; None for the cap on every repo.
    pub repo_id: Option<String>,
    pub cap: f64,
    pub used: f64,
}

fn money(v: f64) -> String {
    format!("${v:.2}")
}

fn count(v: f64) -> String {
    let n = v.round() as u64;
    // 1234567 -> 1,234,567
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl Stop {
    /// "$20.40 of $20.00", "40 of 40 turns", "2,000,000 of 2,000,000 tokens".
    pub fn amount(&self) -> String {
        match self.measure {
            Measure::Usd => format!("{} of {}", money(self.used), money(self.cap)),
            Measure::Turns => format!("{} of {} turns", count(self.used), count(self.cap)),
            Measure::Tokens => format!("{} of {} tokens", count(self.used), count(self.cap)),
        }
    }

    /// One plain line. `repo_name` names the repo when it is that repo's own cap.
    pub fn text(&self, repo_name: Option<&str>) -> String {
        let usd = if self.measure == Measure::Usd { " (API-equivalent)" } else { "" };
        match (self.scope, repo_name.filter(|_| self.repo_id.is_some())) {
            (Scope::Day, None) => format!("Today's cap is reached: {}{usd}. Nothing new starts until midnight.", self.amount()),
            (Scope::Day, Some(r)) => format!("Today's cap for {r} is reached: {}{usd}. Nothing new starts there until midnight.", self.amount()),
            (Scope::Task, _) => format!("This task reached its cap: {}{usd}.", self.amount()),
        }
    }
}

fn over(cap: &Cap, used: &Spent, scope: Scope, repo: Option<&str>) -> Option<Stop> {
    let stop = |measure, cap: f64, used: f64| Stop { scope, measure, repo_id: repo.map(String::from), cap, used };
    if let Some(c) = cap.usd
        && used.usd >= c {
            return Some(stop(Measure::Usd, c, used.usd));
        }
    if let Some(c) = cap.turns
        && used.turns >= c {
            return Some(stop(Measure::Turns, c as f64, used.turns as f64));
        }
    if let Some(c) = cap.tokens
        && used.tokens >= c {
            return Some(stop(Measure::Tokens, c as f64, used.tokens as f64));
        }
    None
}

impl Budgets {
    /// The daily cap that stops new turns in `repo_id`, if one is reached.
    pub fn day_stop(&self, today: &Today, repo_id: &str) -> Option<Stop> {
        over(&self.all.day, &today.all, Scope::Day, None).or_else(|| {
            let caps = self.repos.get(repo_id)?;
            over(&caps.day, today.repos.get(repo_id).unwrap_or(&Spent::default()), Scope::Day, Some(repo_id))
        })
    }

    /// The per-task cap a task (a workspace that spent `task`) has reached, if any.
    pub fn task_stop(&self, repo_id: &str, task: &Spent) -> Option<Stop> {
        over(&self.all.task, task, Scope::Task, None)
            .or_else(|| self.repos.get(repo_id).and_then(|c| over(&c.task, task, Scope::Task, Some(repo_id))))
    }

    /// Can an unattended turn start for this task now? `task` is None for a task with no
    /// workspace yet (it has spent nothing).
    pub fn check(&self, today: &Today, repo_id: &str, task: Option<&Spent>) -> Option<Stop> {
        self.day_stop(today, repo_id).or_else(|| task.and_then(|t| self.task_stop(repo_id, t)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spent(usd: f64, turns: u64, tokens: u64) -> Spent {
        Spent { usd, turns, tokens }
    }

    #[test]
    fn day_cap_counts_every_repo_and_a_repo_cap_only_its_own() {
        let mut b = Budgets::default();
        let mut today = Today { all: spent(19.5, 30, 0), ..Default::default() };
        today.repos.insert("r1".into(), spent(15.0, 20, 0));
        today.repos.insert("r2".into(), spent(4.5, 10, 0));
        assert_eq!(b.check(&today, "r1", None), None, "no caps: nothing stops");

        b.all.day.usd = Some(20.0);
        assert_eq!(b.check(&today, "r1", None), None);
        today.all.usd = 20.0;
        let s = b.check(&today, "r2", None).unwrap();
        assert_eq!((s.scope, s.measure, s.repo_id.as_deref()), (Scope::Day, Measure::Usd, None));
        assert_eq!(s.text(None), "Today's cap is reached: $20.00 of $20.00 (API-equivalent). Nothing new starts until midnight.");

        // A repo cap stops that repo only.
        b.all.day.usd = None;
        b.repos.insert("r1".into(), Caps { day: Cap { turns: Some(20), ..Default::default() }, ..Default::default() });
        let s = b.check(&today, "r1", None).unwrap();
        assert_eq!(s.text(Some("pawprint-api")), "Today's cap for pawprint-api is reached: 20 of 20 turns. Nothing new starts there until midnight.");
        assert_eq!(b.check(&today, "r2", None), None);
        // A repo that ran nothing today is under its cap.
        b.repos.insert("r3".into(), Caps { day: Cap { turns: Some(1), ..Default::default() }, ..Default::default() });
        assert_eq!(b.check(&today, "r3", None), None);
    }

    #[test]
    fn task_cap_counts_the_tasks_workspace_and_tokens_cover_unpriced_agents() {
        let mut b = Budgets::default();
        b.all.task = Cap { usd: Some(5.0), turns: None, tokens: Some(2_000_000) };
        let today = Today::default();
        assert_eq!(b.check(&today, "r1", None), None, "a task with no workspace yet spent nothing");
        assert_eq!(b.check(&today, "r1", Some(&spent(4.99, 3, 10))), None);
        let s = b.check(&today, "r1", Some(&spent(5.12, 4, 10))).unwrap();
        assert_eq!(s.text(None), "This task reached its cap: $5.12 of $5.00 (API-equivalent).");
        // Codex reports tokens, not cost: the token limit stops it.
        let s = b.check(&today, "r1", Some(&spent(0.0, 9, 2_000_100))).unwrap();
        assert_eq!((s.measure, s.text(None)), (Measure::Tokens, "This task reached its cap: 2,000,100 of 2,000,000 tokens.".to_string()));
        // A repo's own task cap applies on top of the one for every repo.
        b.repos.insert("r2".into(), Caps { task: Cap { turns: Some(3), ..Default::default() }, ..Default::default() });
        assert_eq!(b.check(&today, "r2", Some(&spent(0.1, 3, 0))).map(|s| s.measure), Some(Measure::Turns));
        assert_eq!(b.check(&today, "r1", Some(&spent(0.1, 3, 0))), None);
        // The day cap is checked first: it says more about what happens next.
        b.all.day.turns = Some(1);
        let today = Today { all: spent(0.0, 1, 0), ..Default::default() };
        assert_eq!(b.check(&today, "r1", Some(&spent(9.0, 1, 0))).map(|s| s.scope), Some(Scope::Day));
    }

    #[test]
    fn caps_are_checked_and_survive_a_round_trip() {
        let mut b = Budgets::default();
        b.all.day.usd = Some(0.0);
        assert!(b.clone().validate().is_err());
        b.all.day.usd = Some(f64::NAN);
        assert!(b.clone().validate().is_err());
        b.all.day.usd = Some(25.0);
        b.all.task.turns = Some(0);
        assert!(b.clone().validate().is_err());
        b.all.task.turns = Some(12);
        b.repos.insert("empty".into(), Caps::default());
        b.runaway.same_failure = 1;
        let v = b.validate().unwrap();
        assert!(!v.repos.contains_key("empty"));
        assert_eq!(v.runaway.same_failure, 2);
        assert_eq!(Budgets::from_json(Some(&v.to_json())), v);
        // Nothing saved yet, or something unreadable: no caps, runaway checks on.
        let d = Budgets::from_json(Some("not json"));
        assert!(!d.any());
        assert_eq!(d.runaway, Runaway { on: true, same_failure: 4, back_and_forth: 2, no_change: 3 });
    }
}
