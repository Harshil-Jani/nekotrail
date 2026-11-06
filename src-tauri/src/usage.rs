//! What each finished turn used, read from the agent's own end-of-turn report, and the Usage
//! view's numbers built from those turns: per day, repo, agent and model, with tokens, the
//! API-equivalent cost, turns and time. Pure, so the reading and the sums are tested on their own.
//!
//! Costs are what the CLIs report at API prices. On a subscription (Claude Pro or Max, ChatGPT
//! Plus or Pro) you pay a flat fee, so these are what the same work would cost on the API, not
//! what you are billed. Agents that report no cost (Codex, Gemini today) count turns and, when
//! they report them, tokens. Nothing here is estimated: a number the agent didn't report stays
//! unknown and is never filled in.

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// One finished turn, as the agent reported it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TurnFacts {
    /// API-equivalent USD. None: the agent doesn't report cost.
    pub cost: Option<f64>,
    /// Input tokens not read from the prompt cache (cache writes count here).
    pub input: Option<u64>,
    /// Input tokens read from the prompt cache.
    pub cached: Option<u64>,
    pub output: Option<u64>,
    /// The model that ran it, as the agent named it; empty when neither it nor the chat said.
    pub model: String,
    pub duration_ms: u64,
}

impl TurnFacts {
    /// Tokens that count toward a token cap: new input and output. Cache reads are left out:
    /// they repeat the same context every request and would swamp everything else.
    pub fn tokens(&self) -> Option<u64> {
        match (self.input, self.output) {
            (None, None) => None,
            (i, o) => Some(i.unwrap_or(0) + o.unwrap_or(0)),
        }
    }
}

fn n(v: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|k| v[*k].as_u64())
}

/// Read a `done` event. `session_model` is the chat's model setting, used when the agent didn't
/// name one; `measured_ms` is how long nekotrail saw the turn run, used when the agent didn't say.
pub fn facts(agent: &str, done: &Value, session_model: Option<&str>, measured_ms: u64) -> TurnFacts {
    let u = &done["usage"];
    let (input, cached, output) = match agent {
        // Claude: input_tokens leaves out both cache writes and cache reads.
        "claude" => {
            let base = n(u, &["input_tokens"]);
            let write = n(u, &["cache_creation_input_tokens"]);
            let input = match (base, write) {
                (None, None) => None,
                (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
            };
            (input, n(u, &["cache_read_input_tokens"]), n(u, &["output_tokens"]))
        }
        // Codex (OpenAI): cached_input_tokens is part of input_tokens.
        "codex" => {
            let all = n(u, &["input_tokens"]);
            let cached = n(u, &["cached_input_tokens"]);
            (all.map(|a| a.saturating_sub(cached.unwrap_or(0))), cached, n(u, &["output_tokens"]))
        }
        // Gemini's stats, and any other agent that reports a usage object.
        _ => {
            let all = n(u, &["input_tokens", "prompt_tokens", "input"]);
            let cached = n(u, &["cached", "cached_tokens", "cached_input_tokens"]);
            let input = match (all, cached) {
                (Some(a), Some(c)) if c <= a && agent == "gemini" => Some(a - c),
                (a, _) => a,
            };
            (input, cached, n(u, &["output_tokens", "candidates_tokens", "output"]))
        }
    };
    let model = done["model"].as_str().filter(|m| !m.is_empty()).or(session_model.filter(|m| !m.is_empty())).unwrap_or("").to_string();
    TurnFacts {
        cost: done["cost_usd"].as_f64().filter(|c| c.is_finite() && *c >= 0.0),
        input,
        cached,
        output,
        model,
        duration_ms: done["duration_ms"].as_u64().unwrap_or(measured_ms),
    }
}

/// A recorded turn, as the store hands it back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    /// RFC 3339, UTC.
    pub at: String,
    pub repo_id: String,
    pub agent: String,
    pub facts: TurnFacts,
}

/// One line of the Usage view: one day, repo, agent and model.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Line {
    /// YYYY-MM-DD in local time.
    pub day: String,
    pub repo_id: String,
    pub agent: String,
    pub model: String,
    pub turns: u64,
    /// Turns whose agent reported a cost. `cost` sums only those.
    pub priced_turns: u64,
    pub cost: f64,
    /// Turns whose agent reported tokens. The token sums cover only those.
    pub token_turns: u64,
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub duration_ms: u64,
}

/// The local day (YYYY-MM-DD) a UTC timestamp falls on.
pub fn local_day(at: &str, offset: FixedOffset) -> Option<String> {
    DateTime::parse_from_rfc3339(at).ok().map(|t| t.with_timezone(&offset).format("%Y-%m-%d").to_string())
}

/// The start of today in local time, as a UTC RFC 3339 string the store's timestamps compare with.
pub fn local_midnight(now: DateTime<FixedOffset>) -> String {
    day_start(now.date_naive(), *now.offset())
}

/// The start of `day` at `offset`, as a UTC RFC 3339 string.
pub fn day_start(day: NaiveDate, offset: FixedOffset) -> String {
    let midnight = day.and_hms_opt(0, 0, 0).unwrap_or_default();
    match midnight.and_local_timezone(offset).single() {
        Some(t) => t.with_timezone(&Utc).to_rfc3339(),
        None => midnight.and_utc().to_rfc3339(),
    }
}

/// Group turns by local day, repo, agent and model. Newest day first, then repo, agent, model.
pub fn aggregate(rows: &[Row], offset: FixedOffset) -> Vec<Line> {
    let mut m: BTreeMap<(String, String, String, String), Line> = BTreeMap::new();
    for r in rows {
        let Some(day) = local_day(&r.at, offset) else { continue };
        let key = (day.clone(), r.repo_id.clone(), r.agent.clone(), r.facts.model.clone());
        let l = m.entry(key).or_insert_with(|| Line { day, repo_id: r.repo_id.clone(), agent: r.agent.clone(), model: r.facts.model.clone(), ..Default::default() });
        l.turns += 1;
        if let Some(c) = r.facts.cost {
            l.priced_turns += 1;
            l.cost += c;
        }
        if r.facts.input.is_some() || r.facts.output.is_some() || r.facts.cached.is_some() {
            l.token_turns += 1;
            l.input += r.facts.input.unwrap_or(0);
            l.cached += r.facts.cached.unwrap_or(0);
            l.output += r.facts.output.unwrap_or(0);
        }
        l.duration_ms += r.facts.duration_ms;
    }
    let mut v: Vec<Line> = m.into_values().collect();
    v.sort_by(|a, b| b.day.cmp(&a.day).then_with(|| a.repo_id.cmp(&b.repo_id)).then_with(|| a.agent.cmp(&b.agent)).then_with(|| a.model.cmp(&b.model)));
    v
}

fn csv_field(s: &str) -> String {
    // A cell starting with = + - @ would run as a formula in a spreadsheet: quote it with a '.
    let s = if s.starts_with(['=', '+', '-', '@']) { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// The lines as CSV, one row per day, repo, agent and model. Unknown numbers are empty cells,
/// never zeros: a row with no priced turns has no cost.
pub fn to_csv(lines: &[Line], repo_name: impl Fn(&str) -> String, agent_name: impl Fn(&str) -> String) -> String {
    let mut out = String::from("day,repo,agent,model,turns,priced_turns,api_equivalent_usd,input_tokens,cached_input_tokens,output_tokens,minutes\n");
    for l in lines {
        let opt = |known: bool, v: String| if known { v } else { String::new() };
        let cells = [
            csv_field(&l.day),
            csv_field(&repo_name(&l.repo_id)),
            csv_field(&agent_name(&l.agent)),
            csv_field(if l.model.is_empty() { "default" } else { &l.model }),
            l.turns.to_string(),
            l.priced_turns.to_string(),
            opt(l.priced_turns > 0, format!("{:.4}", l.cost)),
            opt(l.token_turns > 0, l.input.to_string()),
            opt(l.token_turns > 0, l.cached.to_string()),
            opt(l.token_turns > 0, l.output.to_string()),
            format!("{:.1}", l.duration_ms as f64 / 60_000.0),
        ];
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_each_agents_report() {
        // Claude's result line, through the real parser.
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":2100,"total_cost_usd":0.0421,
            "usage":{"input_tokens":12,"cache_creation_input_tokens":3000,"cache_read_input_tokens":20000,"output_tokens":500},
            "modelUsage":{"claude-haiku-4-5":{"costUSD":0.001,"outputTokens":40,"contextWindow":200000},"claude-opus-4-5":{"costUSD":0.041,"outputTokens":460,"contextWindow":200000}}}"#;
        let done = crate::agent::parse_claude(&line.replace('\n', "")).pop().unwrap();
        let f = facts("claude", &done, Some("sonnet"), 9999);
        assert_eq!(f, TurnFacts { cost: Some(0.0421), input: Some(3012), cached: Some(20000), output: Some(500), model: "claude-opus-4-5".into(), duration_ms: 2100 });
        assert_eq!(f.tokens(), Some(3512));

        // Codex: cached is part of input, no cost, no duration (nekotrail's own clock is used).
        let done = crate::agent::parse_codex(r#"{"type":"turn.completed","usage":{"input_tokens":24763,"cached_input_tokens":24448,"output_tokens":122}}"#).pop().unwrap();
        let f = facts("codex", &done, Some("gpt-5-codex"), 61_000);
        assert_eq!(f, TurnFacts { cost: None, input: Some(315), cached: Some(24448), output: Some(122), model: "gpt-5-codex".into(), duration_ms: 61_000 });

        // Gemini's stats.
        let f = facts("gemini", &json!({"kind": "done", "duration_ms": 5, "usage": {"input_tokens": 900, "output_tokens": 80, "cached": 100}}), None, 0);
        assert_eq!((f.input, f.cached, f.output, f.cost, f.model.as_str()), (Some(800), Some(100), Some(80), None, ""));

        // An agent that reports nothing: every number stays unknown.
        let f = facts("goose", &json!({"kind": "done", "is_error": false}), None, 1200);
        assert_eq!(f, TurnFacts { duration_ms: 1200, ..Default::default() });
        assert_eq!(f.tokens(), None);

        // ACP agents that report cost per turn.
        let f = facts("opencode", &json!({"kind": "done", "cost_usd": 0.5, "duration_ms": 10}), Some("anthropic/claude-sonnet-4-5"), 0);
        assert_eq!((f.cost, f.model.as_str()), (Some(0.5), "anthropic/claude-sonnet-4-5"));
        // A broken number is not a cost.
        assert_eq!(facts("claude", &json!({"cost_usd": -1.0}), None, 0).cost, None);
    }

    fn row(at: &str, repo: &str, agent: &str, model: &str, cost: Option<f64>, tokens: Option<(u64, u64)>, ms: u64) -> Row {
        Row {
            at: at.into(), repo_id: repo.into(), agent: agent.into(),
            facts: TurnFacts { cost, input: tokens.map(|t| t.0), cached: None, output: tokens.map(|t| t.1), model: model.into(), duration_ms: ms },
        }
    }

    #[test]
    fn groups_by_local_day_repo_agent_and_model() {
        let ist = FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        let rows = vec![
            // 23:00 UTC on the 6th is 04:30 on the 7th in India.
            row("2026-11-06T23:00:00+00:00", "r1", "claude", "opus", Some(1.25), Some((100, 10)), 60_000),
            row("2026-11-07T02:00:00+00:00", "r1", "claude", "opus", Some(0.75), None, 30_000),
            row("2026-11-07T03:00:00+00:00", "r1", "codex", "", None, Some((500, 50)), 90_000),
            // 18:00 UTC on the 6th is 23:30 on the 6th.
            row("2026-11-06T18:00:00+00:00", "r2", "claude", "opus", Some(2.0), Some((1, 1)), 1000),
            row("not a time", "r2", "claude", "opus", Some(99.0), None, 0),
        ];
        let lines = aggregate(&rows, ist);
        assert_eq!(lines.len(), 3);
        let l = &lines[0];
        assert_eq!((l.day.as_str(), l.repo_id.as_str(), l.agent.as_str(), l.model.as_str()), ("2026-11-07", "r1", "claude", "opus"));
        assert_eq!((l.turns, l.priced_turns, l.cost, l.token_turns, l.input, l.output, l.duration_ms), (2, 2, 2.0, 1, 100, 10, 90_000));
        let c = &lines[1];
        assert_eq!((c.agent.as_str(), c.turns, c.priced_turns, c.cost, c.token_turns, c.input), ("codex", 1, 0, 0.0, 1, 500));
        assert_eq!((lines[2].day.as_str(), lines[2].repo_id.as_str(), lines[2].cost), ("2026-11-06", "r2", 2.0));

        let csv = to_csv(&lines, |r| if r == "r1" { "api, v2".into() } else { "=web".into() }, |a| a.to_string());
        let out: Vec<&str> = csv.lines().collect();
        assert_eq!(out[0], "day,repo,agent,model,turns,priced_turns,api_equivalent_usd,input_tokens,cached_input_tokens,output_tokens,minutes");
        assert_eq!(out[1], "2026-11-07,\"api, v2\",claude,opus,2,2,2.0000,100,0,10,1.5");
        // Codex reported no cost: an empty cell, not $0.
        assert_eq!(out[2], "2026-11-07,\"api, v2\",codex,default,1,0,,500,0,50,1.5");
        // A name that would run as a spreadsheet formula is quoted.
        assert_eq!(out[3], "2026-11-06,'=web,claude,opus,1,1,2.0000,1,0,1,0.0");
    }

    #[test]
    fn local_midnight_is_in_utc() {
        let now = DateTime::parse_from_rfc3339("2026-11-07T04:30:00+05:30").unwrap();
        assert_eq!(local_midnight(now), "2026-11-06T18:30:00+00:00");
        // A turn at 18:31 UTC on the 6th is today in India; one at 18:29 isn't.
        assert!("2026-11-06T18:31:00.000+00:00" >= local_midnight(now).as_str());
        assert!("2026-11-06T18:29:59+00:00" < local_midnight(now).as_str());
    }
}
