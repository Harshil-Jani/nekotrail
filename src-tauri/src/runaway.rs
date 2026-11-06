//! Runaway protection: notice an agent going in circles while nobody is watching, so the queue or
//! autopilot stops giving it turns and the task waits for you, with what was seen.
//!
//! Three signs, each with a threshold you can change (see `budget::Runaway`):
//! - the same command failed N times in a row (from the chat's tool calls, within a turn or
//!   across the last few turns);
//! - a file went back to an earlier version again and again, between turns (A, B, A, B);
//! - K turns in a row changed no file.
//!
//! Files are compared between turns (a snapshot when each watched turn ends), so edits that go
//! back and forth inside one turn are not seen. Pure, so each sign is tested with fixtures.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// How many recent turns of the chat the command check reads.
pub const COMMAND_TURNS: usize = 3;
/// How many recent snapshots the file checks read.
pub const SNAP_TURNS: usize = 8;
/// The most changed files a snapshot keeps.
pub const SNAP_FILES: usize = 200;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    SameFailure,
    BackAndForth,
    NoChange,
}

/// What was seen, in plain words, with the evidence lines the chat and the queue show.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Finding {
    pub kind: Kind,
    pub text: String,
    pub evidence: Vec<String>,
}

/// One file a turn changed: its blob before and after ("" when it didn't exist).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct FileChange {
    pub path: String,
    pub before: String,
    pub after: String,
}

/// The files one watched turn changed, kept with the turn.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Snap {
    /// The turn ended without needing you (not stopped, no limit, no error).
    pub ok: bool,
    pub files: Vec<FileChange>,
    /// More files changed than a snapshot keeps.
    #[serde(default)]
    pub capped: bool,
    /// How the evidence names this turn ("Turn at 3:10 AM"). Set when read back, not saved.
    #[serde(skip)]
    pub label: String,
}

fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() > n { format!("{}...", s.chars().take(n - 3).collect::<String>()) } else { s.to_string() }
}

/// The last thing a failed command printed, which is usually the reason.
fn reason(output: &str) -> String {
    let line = output.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("no output");
    clip(line, 160)
}

fn norm(cmd: &str) -> String {
    cmd.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The same shell command failed `n` times in a row, the last time in the latest turn. Reads the
/// last `COMMAND_TURNS` turns of the chat (turns start at the user's message).
pub fn same_failure(events: &[Value], n: u32) -> Option<Finding> {
    let starts: Vec<usize> = events.iter().enumerate().filter(|(_, e)| e["kind"] == "user").map(|(i, _)| i).collect();
    let from = starts.len().checked_sub(COMMAND_TURNS).map(|k| starts[k]).unwrap_or(0);
    let latest = starts.last().copied().unwrap_or(0);
    // (command, failed, reason, in the latest turn), in the order they ran.
    let mut runs: Vec<(String, Option<bool>, String, bool)> = vec![];
    let mut by_id: HashMap<String, usize> = HashMap::new();
    for (i, ev) in events.iter().enumerate().skip(from) {
        match ev["kind"].as_str() {
            Some("tool") if ev["name"] == "Bash" => {
                let cmd = norm(ev["input"].as_str().unwrap_or(""));
                if cmd.is_empty() {
                    continue;
                }
                if let Some(id) = ev["id"].as_str() {
                    by_id.insert(id.to_string(), runs.len());
                }
                runs.push((cmd, None, String::new(), i >= latest));
            }
            Some("tool_result") => {
                if let Some(&k) = ev["id"].as_str().and_then(|id| by_id.get(id)) {
                    runs[k].1 = Some(ev["is_error"].as_bool().unwrap_or(false));
                    runs[k].2 = reason(ev["output"].as_str().unwrap_or(""));
                }
            }
            _ => {}
        }
    }
    let mut best: Option<(String, Vec<String>)> = None;
    let mut seen: Vec<&str> = vec![];
    for (cmd, _, _, _) in runs.iter().rev() {
        if seen.contains(&cmd.as_str()) {
            continue;
        }
        seen.push(cmd);
        // The trailing streak of failures of this command: a success (or a run with no result) ends it.
        let mine: Vec<&(String, Option<bool>, String, bool)> = runs.iter().filter(|r| &r.0 == cmd).collect();
        let streak: Vec<&&(String, Option<bool>, String, bool)> = mine.iter().rev().take_while(|r| r.1 == Some(true)).collect();
        if streak.len() < n as usize || !streak.first().is_some_and(|r| r.3) {
            continue;
        }
        if best.as_ref().is_none_or(|b| streak.len() > b.1.len()) {
            let evidence = streak.iter().rev().enumerate().map(|(k, r)| format!("Run {}: {}", k + 1, r.2)).collect();
            best = Some((cmd.clone(), evidence));
        }
    }
    best.map(|(cmd, evidence)| Finding {
        kind: Kind::SameFailure,
        text: format!("The same command failed {} times in a row: {}", evidence.len(), clip(&cmd, 120)),
        evidence,
    })
}

/// A file went back to a version it had before, `times` times or more, across the last
/// `SNAP_TURNS` snapshots (oldest first).
pub fn back_and_forth(snaps: &[Snap], times: u32) -> Option<Finding> {
    let recent = &snaps[snaps.len().saturating_sub(SNAP_TURNS)..];
    // Path -> (versions in order, times it went back, evidence).
    let mut files: Vec<(String, Vec<String>, u32, Vec<String>)> = vec![];
    for s in recent {
        for f in &s.files {
            let k = match files.iter().position(|x| x.0 == f.path) {
                Some(k) => k,
                None => {
                    files.push((f.path.clone(), vec![], 0, vec![]));
                    files.len() - 1
                }
            };
            let e = &mut files[k];
            if e.1.last() != Some(&f.before) {
                e.1.push(f.before.clone());
            }
            let back = e.1[..e.1.len() - 1].contains(&f.after);
            if back {
                e.2 += 1;
            }
            e.3.push(format!("{}: {}", s.label, if back { "put it back to an earlier version" } else { "changed it" }));
            e.1.push(f.after.clone());
        }
    }
    files.into_iter().filter(|f| f.2 >= times).max_by_key(|f| f.2).map(|(path, _, n, evidence)| Finding {
        kind: Kind::BackAndForth,
        text: format!("{path} went back to an earlier version {n} {}", if n == 1 { "time" } else { "times" }),
        evidence,
    })
}

/// The last `k` turns that ended normally changed no file.
pub fn no_change(snaps: &[Snap], k: u32) -> Option<Finding> {
    let ok: Vec<&Snap> = snaps.iter().filter(|s| s.ok).collect();
    let last = ok.get(ok.len().checked_sub(k as usize)?..)?;
    if last.len() < k as usize || last.iter().any(|s| !s.files.is_empty() || s.capped) {
        return None;
    }
    Some(Finding {
        kind: Kind::NoChange,
        text: format!("The last {k} turns changed no files"),
        evidence: last.iter().map(|s| format!("{}: no file changed", s.label)).collect(),
    })
}

/// Every check, the first sign found. `snaps` are oldest first.
pub fn check(events: &[Value], snaps: &[Snap], cfg: &crate::budget::Runaway) -> Option<Finding> {
    if !cfg.on {
        return None;
    }
    same_failure(events, cfg.same_failure)
        .or_else(|| back_and_forth(snaps, cfg.back_and_forth))
        .or_else(|| no_change(snaps, cfg.no_change))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(id: &str, cmd: &str, failed: bool, out: &str) -> [Value; 2] {
        [
            json!({"kind": "tool", "id": id, "name": "Bash", "input": cmd}),
            json!({"kind": "tool_result", "id": id, "output": out, "is_error": failed}),
        ]
    }

    fn user(t: &str) -> Value {
        json!({"kind": "user", "text": t})
    }

    #[test]
    fn same_failing_command_n_times_in_a_row() {
        let mut ev = vec![user("fix the build")];
        for i in 0..4 {
            ev.extend(run(&format!("t{i}"), "cargo  test", true, "running 3 tests\n\nerror: test failed, to rerun pass `--lib`\n"));
            ev.push(json!({"kind": "tool", "id": format!("e{i}"), "name": "Edit", "input": "src/lib.rs"}));
        }
        let f = same_failure(&ev, 4).unwrap();
        assert_eq!(f.kind, Kind::SameFailure);
        assert_eq!(f.text, "The same command failed 4 times in a row: cargo test");
        assert_eq!(f.evidence.len(), 4);
        assert_eq!(f.evidence[0], "Run 1: error: test failed, to rerun pass `--lib`");
        assert_eq!(same_failure(&ev, 5), None, "below the threshold");

        // It passed in the end: not a loop.
        let mut fixed = ev.clone();
        fixed.extend(run("ok", "cargo test", false, "test result: ok"));
        assert_eq!(same_failure(&fixed, 4), None);

        // A different command failing in between doesn't break the streak; a success of this one does.
        let mut mixed = vec![user("go")];
        mixed.extend(run("a", "npm test", true, "1 failed"));
        mixed.extend(run("b", "npm run lint", true, "2 problems"));
        mixed.extend(run("c", "npm test", true, "1 failed"));
        mixed.extend(run("d", "npm test", false, "ok"));
        mixed.extend(run("e", "npm test", true, "1 failed"));
        assert_eq!(same_failure(&mixed, 2), None, "npm test passed in between; lint failed once");
    }

    #[test]
    fn same_failure_reads_recent_turns_and_needs_the_latest() {
        // Two failures in each of two turns: four in a row across turns.
        let mut ev = vec![user("one")];
        ev.extend(run("a", "go test ./...", true, "FAIL"));
        ev.extend(run("b", "go test ./...", true, "FAIL"));
        ev.push(user("Autopilot: CI is failing"));
        ev.extend(run("c", "go test ./...", true, "FAIL"));
        ev.extend(run("d", "go test ./...", true, "FAIL"));
        assert!(same_failure(&ev, 4).is_some());
        // An old streak that the latest turn didn't add to is history, not a loop.
        ev.push(user("now the docs"));
        ev.extend(run("e", "make docs", false, "ok"));
        assert_eq!(same_failure(&ev, 4), None);
        // Turns older than the window are not read.
        let mut old = vec![user("1")];
        old.extend(run("a", "pytest", true, "E"));
        old.extend(run("b", "pytest", true, "E"));
        for t in 0..COMMAND_TURNS {
            old.push(user(&format!("t{t}")));
            old.extend(run(&format!("x{t}"), "pytest", true, "E"));
        }
        assert_eq!(same_failure(&old, 4).map(|f| f.evidence.len()), None);
        assert_eq!(same_failure(&old, 3).map(|f| f.evidence.len()), Some(3));
        // A command still running (no result yet) neither counts nor breaks anything.
        let mut open = vec![user("go")];
        for i in 0..3 {
            open.extend(run(&format!("f{i}"), "bun test", true, "fail"));
        }
        open.push(json!({"kind": "tool", "id": "z", "name": "Bash", "input": "bun test"}));
        assert_eq!(same_failure(&open, 3), None);
    }

    fn snap(label: &str, ok: bool, files: &[(&str, &str, &str)]) -> Snap {
        Snap {
            ok, capped: false, label: label.into(),
            files: files.iter().map(|(p, b, a)| FileChange { path: p.to_string(), before: b.to_string(), after: a.to_string() }).collect(),
        }
    }

    #[test]
    fn a_file_edited_back_and_forth() {
        let snaps = vec![
            snap("Turn 1", true, &[("src/a.rs", "A", "B"), ("src/b.rs", "x", "y")]),
            snap("Turn 2", true, &[("src/a.rs", "B", "A")]),
            snap("Turn 3", true, &[("src/a.rs", "A", "B"), ("src/b.rs", "y", "z")]),
        ];
        let f = back_and_forth(&snaps, 2).unwrap();
        assert_eq!(f.text, "src/a.rs went back to an earlier version 2 times");
        assert_eq!(f.evidence, ["Turn 1: changed it", "Turn 2: put it back to an earlier version", "Turn 3: put it back to an earlier version"]);
        assert_eq!(back_and_forth(&snaps[..2], 2), None, "one undo is fine");
        assert_eq!(back_and_forth(&snaps[..2], 1).map(|f| f.kind), Some(Kind::BackAndForth));
        // Steady progress on one file is not back and forth.
        let steady = vec![snap("1", true, &[("a", "A", "B")]), snap("2", true, &[("a", "B", "C")]), snap("3", true, &[("a", "C", "D")])];
        assert_eq!(back_and_forth(&steady, 1), None);
        // Deleted and made again with the same content counts too ("" is "no file").
        let gone = vec![snap("1", true, &[("tmp.txt", "", "Q")]), snap("2", true, &[("tmp.txt", "Q", "")]), snap("3", true, &[("tmp.txt", "", "Q")])];
        assert_eq!(back_and_forth(&gone, 2).map(|f| f.text), Some("tmp.txt went back to an earlier version 2 times".into()));
    }

    #[test]
    fn turns_that_change_nothing() {
        let snaps = vec![
            snap("Turn 1", true, &[("a", "A", "B")]),
            snap("Turn 2", true, &[]),
            snap("Turn 3", false, &[]),
            snap("Turn 4", true, &[]),
            snap("Turn 5", true, &[]),
        ];
        let f = no_change(&snaps, 3).unwrap();
        assert_eq!(f.text, "The last 3 turns changed no files");
        // A turn that hit its usage limit (not ok) isn't counted either way.
        assert_eq!(f.evidence, ["Turn 2: no file changed", "Turn 4: no file changed", "Turn 5: no file changed"]);
        assert_eq!(no_change(&snaps, 4), None);
        assert_eq!(no_change(&snaps[..1], 1), None);
        let mut big = snaps.clone();
        big.push(Snap { ok: true, capped: true, ..Default::default() });
        assert_eq!(no_change(&big, 1), None, "too many files to list still means files changed");
    }

    #[test]
    fn check_runs_every_sign_and_can_be_off() {
        let mut cfg = crate::budget::Runaway::default();
        let idle: Vec<Snap> = (1..=3).map(|i| snap(&format!("Turn {i}"), true, &[])).collect();
        assert_eq!(check(&[], &idle, &cfg).map(|f| f.kind), Some(Kind::NoChange));
        cfg.on = false;
        assert_eq!(check(&[], &idle, &cfg), None);
    }
}
