//! Hand off to another agent, mid-task. A chat's agent hit its usage limit, went in circles, or you
//! just want a second pair of hands: a new chat in the same workspace, with another agent, gets a
//! brief built from what already happened, so nobody explains the task again.
//!
//! The brief is plain code over the chat's events, the workspace's changes and its last checks: no
//! model is asked to summarise, so the same chat always gives the same brief. It reads, in order:
//! why the hand-off happens, the original task, what you said since, what the agent did (its last
//! messages, the files changed against the target branch, the last checks, the commands still
//! failing, its last steps), what is left (its todo list or plan), and what to do now. It stays
//! under `MAX_CHARS`: when it doesn't fit, the least useful parts shrink first (old steps, then
//! long messages, then long file lists), always in the same order. Anything that looks like a
//! secret is hidden with `redact`, and paths in your home folder show as `~`.

use crate::git::FileChange;
use crate::queue::agent_name;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// About how long a brief may be, in characters. Long enough to carry the task, short enough to
/// read before sending and to leave the new agent's context for the work.
pub const MAX_CHARS: usize = 6000;

/// Why the hand-off happens, from the end of the chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Reason {
    /// The agent said it is out of usage.
    Limit,
    /// nekotrail stopped it going in circles (`runaway`).
    Loop,
    /// A spend cap stopped it (`budget`).
    Budget,
    /// You chose to.
    Asked,
}

/// One repo's changes, for the brief. A multi-repo task has one per repo, this chat's first.
#[derive(Debug)]
pub struct RepoChanges {
    pub name: String,
    /// Where the new agent finds it. None for the chat's own folder.
    pub folder: Option<String>,
    pub target: String,
    /// The files changed against the target branch, or why they couldn't be read.
    pub files: Result<Vec<FileChange>, String>,
}

/// The last run of one check (`checks::CheckResult`, the parts the brief uses).
#[derive(Debug, Clone)]
pub struct CheckLine {
    pub name: String,
    pub command: String,
    /// passed, failed or error.
    pub status: String,
    pub passed: Option<u32>,
    pub failed: u32,
    /// Names of the first failing tests.
    pub failures: Vec<String>,
}

impl CheckLine {
    pub fn from_result(r: &crate::checks::CheckResult) -> CheckLine {
        let status = serde_json::to_value(r.status).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| "error".into());
        CheckLine { name: r.name.clone(), command: r.command.clone(), status, passed: r.passed, failed: r.failed, failures: r.failures.iter().map(|f| f.title.clone()).collect() }
    }
}

pub struct Input<'a> {
    /// The agent handing off (its id).
    pub from: &'a str,
    pub events: &'a [Value],
    pub repos: &'a [RepoChanges],
    pub checks: &'a [CheckLine],
    /// Your home folder, shown as `~`.
    pub home: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Brief {
    pub text: String,
    pub reason: Reason,
    /// The original task, as the chat first had it (kept on the new chat, so a second hand-off
    /// still starts from it).
    pub task: String,
    /// How many things that look like secrets are hidden in `text`.
    pub hidden: usize,
    /// Something was shortened or left out to fit.
    pub cut: bool,
}

// ---------- reading the chat ----------

#[derive(Debug, Clone, PartialEq)]
struct Step {
    name: String,
    target: String,
    failed: bool,
    /// The first line of what it printed, for a failure.
    error: String,
}

#[derive(Debug, Default)]
struct Transcript {
    task: String,
    later: Vec<String>,
    answers: Vec<String>,
    steps: Vec<Step>,
    /// (done, text) from the agent's latest todo list.
    todos: Vec<(bool, String)>,
    plan: Option<String>,
    reason: Option<Reason>,
    stop: String,
}

fn s(ev: &Value, k: &str) -> String {
    ev[k].as_str().unwrap_or("").to_string()
}

fn read(events: &[Value]) -> Transcript {
    let mut t = Transcript::default();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    // A chat that is itself a hand-off: its first message is the brief, and the task came with it.
    let mut skip_brief = false;
    for ev in events {
        match ev["kind"].as_str().unwrap_or("") {
            "handoff" if ev["dir"] == "from" => {
                if t.task.is_empty() {
                    t.task = s(ev, "task");
                }
                skip_brief = true;
            }
            "user" => {
                let text = s(ev, "text");
                if skip_brief {
                    skip_brief = false;
                } else if t.task.is_empty() {
                    t.task = text;
                } else if !text.trim().is_empty() {
                    t.later.push(text);
                }
                // Why the agent stopped is read from after your last message.
                t.reason = None;
                t.stop.clear();
            }
            "text" | "error" => match ev["fix"].as_str() {
                Some("limit") => {
                    t.reason = Some(Reason::Limit);
                    t.stop = s(ev, "text");
                }
                Some(_) => {}
                None if ev["kind"] == "text" => {
                    let text = s(ev, "text");
                    if !text.trim().is_empty() {
                        t.answers.push(text);
                    }
                }
                None => {}
            },
            "stop" => {
                t.reason = Some(if ev["reason"] == "budget" { Reason::Budget } else { Reason::Loop });
                t.stop = s(ev, "text");
            }
            "tool" => {
                let (name, input) = (s(ev, "name"), s(ev, "input"));
                match name.as_str() {
                    "TodoWrite" => {
                        if let Some(list) = todos(&input) {
                            t.todos = list;
                        }
                    }
                    "ExitPlanMode" => t.plan = Some(input),
                    _ => {
                        by_id.insert(s(ev, "id"), t.steps.len());
                        t.steps.push(Step { name, target: one_line(&input, 160), failed: false, error: String::new() });
                    }
                }
            }
            "tool_result" => {
                if let Some(&i) = by_id.get(&s(ev, "id")) {
                    let failed = ev["is_error"].as_bool().unwrap_or(false);
                    t.steps[i].failed = failed;
                    t.steps[i].error = if failed { first_line(&s(ev, "output")) } else { String::new() };
                }
            }
            _ => {}
        }
    }
    t
}

/// A todo list (TodoWrite's JSON) as (done, text).
fn todos(input: &str) -> Option<Vec<(bool, String)>> {
    let list: Vec<Value> = serde_json::from_str(input).ok()?;
    let out: Vec<(bool, String)> = list
        .iter()
        .map(|x| (x["status"] == "completed", x["content"].as_str().unwrap_or("").trim().to_string()))
        .filter(|(_, c)| !c.is_empty())
        .collect();
    (!out.is_empty()).then_some(out)
}

/// Commands whose latest run failed, each once, oldest first.
fn still_failing(steps: &[Step]) -> Vec<&Step> {
    let mut last: Vec<&Step> = vec![];
    for st in steps.iter().filter(|x| x.name == "Bash") {
        last.retain(|x| x.target != st.target);
        last.push(st);
    }
    last.into_iter().filter(|x| x.failed).collect()
}

// ---------- text helpers ----------

/// At most `n` characters, with "…" when cut. Never splits a character.
fn clip(text: &str, n: usize) -> (String, bool) {
    let text = text.trim();
    if text.chars().count() <= n {
        return (text.to_string(), false);
    }
    let mut out: String = text.chars().take(n.saturating_sub(1)).collect();
    out = out.trim_end().to_string();
    out.push('…');
    (out, true)
}

fn one_line(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.trim().lines().collect();
    let first = lines.first().copied().unwrap_or("");
    let (mut out, cut) = clip(first, n);
    if lines.len() > 1 && !cut {
        out.push_str(" …");
    }
    out
}

fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| clip(l, 160).0).unwrap_or_default()
}

/// A code span that survives backticks inside it.
fn code(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let ticks = "`".repeat(longest + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') { " " } else { "" };
    format!("{ticks}{pad}{text}{pad}{ticks}")
}

fn quote(text: &str) -> String {
    text.lines().map(|l| if l.trim().is_empty() { ">".to_string() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n")
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// "3 more files".
fn more(n: usize, one: &str, many: &str) -> String {
    format!("{n} more {}", if n == 1 { one } else { many })
}

fn verb(name: &str) -> &str {
    match name {
        "Bash" => "Ran",
        "Read" => "Read",
        "Write" => "Wrote",
        "Edit" | "MultiEdit" | "NotebookEdit" => "Edited",
        "Grep" => "Searched for",
        "Glob" => "Listed",
        "WebFetch" => "Fetched",
        "WebSearch" => "Searched the web for",
        "Task" | "Agent" => "Asked a subagent to",
        other => other,
    }
}

// ---------- the brief ----------

/// How much of each part goes in. The first level that fits wins; later levels keep less.
#[derive(Debug, Clone, Copy)]
struct Caps {
    task: usize,
    later: usize,
    later_len: usize,
    answers: usize,
    answer_len: usize,
    files: usize,
    failed: usize,
    steps: usize,
    todos: usize,
    plan: usize,
}

const LEVELS: [Caps; 5] = [
    Caps { task: 2000, later: 3, later_len: 400, answers: 2, answer_len: 900, files: 25, failed: 5, steps: 12, todos: 15, plan: 1500 },
    Caps { task: 2000, later: 3, later_len: 300, answers: 2, answer_len: 600, files: 20, failed: 5, steps: 6, todos: 12, plan: 1000 },
    Caps { task: 1500, later: 2, later_len: 250, answers: 1, answer_len: 500, files: 12, failed: 3, steps: 0, todos: 10, plan: 700 },
    Caps { task: 1000, later: 1, later_len: 200, answers: 1, answer_len: 350, files: 8, failed: 2, steps: 0, todos: 8, plan: 500 },
    Caps { task: 700, later: 0, later_len: 0, answers: 1, answer_len: 250, files: 5, failed: 2, steps: 0, todos: 6, plan: 300 },
];

/// Hides secrets and shortens home paths in one piece of text.
fn clean(text: &str, home: Option<&str>) -> String {
    let t = match home {
        Some(h) => crate::redact::shorten_home(text, h),
        None => text.to_string(),
    };
    crate::redact::mask(&t)
}

/// The brief for handing this chat to another agent.
pub fn build(input: &Input) -> Brief {
    let t = read(input.events);
    let reason = t.reason.unwrap_or(Reason::Asked);
    let mut out = None;
    for (i, caps) in LEVELS.iter().enumerate() {
        let (text, cut) = render(input, &t, reason, caps);
        if text.chars().count() <= MAX_CHARS || i == LEVELS.len() - 1 {
            out = Some((text, cut || i > 0));
            break;
        }
    }
    let (mut text, mut cut) = out.unwrap_or_default();
    // Many repos with long names could still be over: cut the end, keeping the instruction.
    if text.chars().count() > MAX_CHARS {
        let tail = your_turn(input, &t, reason);
        let room = MAX_CHARS.saturating_sub(tail.chars().count() + 4);
        let head: String = text.chars().take(room).collect();
        text = format!("{}…\n\n{tail}", head.trim_end());
        cut = true;
    }
    let hidden = text.matches("[hidden: ").count();
    Brief { text, reason, task: clean(t.task.trim(), input.home), hidden, cut }
}

fn intro(from: &str, reason: Reason, stop: &str, repos: usize) -> String {
    let why = match reason {
        Reason::Limit => format!("{from} ran out of usage partway through this task, so you're taking over."),
        Reason::Loop if !stop.trim().is_empty() => format!("nekotrail stopped {from} because it was going in circles ({}), so you're taking over.", one_line(stop.strip_suffix('.').filter(|x| !x.ends_with('.')).unwrap_or(stop), 200)),
        Reason::Loop => format!("nekotrail stopped {from} because it was going in circles, so you're taking over."),
        Reason::Budget => format!("{from} stopped at a spend cap partway through this task, so you're taking over."),
        Reason::Asked => format!("You're taking over this task from {from}, partway through."),
    };
    let place = if repos > 1 {
        format!("You work in the same folders and on the same branch: everything {from} changed is already there. This task spans {repos} repos, listed below.")
    } else {
        format!("You work in the same folder and on the same branch: everything {from} changed is already there.")
    };
    format!("{why} {place} Don't start over.")
}

fn your_turn(input: &Input, t: &Transcript, _reason: Reason) -> String {
    let failing = input.checks.iter().any(|c| c.status != "passed") || !still_failing(&t.steps).is_empty();
    let mut lines = vec![
        "## Your turn".to_string(),
        "1. Start with `git status` and `git diff` to see the work as it is now. Don't redo what's done.".to_string(),
    ];
    lines.push(if failing {
        "2. Carry on with what's left, and get the failing checks and commands above passing.".to_string()
    } else {
        "2. Carry on with what's left.".to_string()
    });
    lines.push("3. When you finish, say what you changed and how you checked it.".to_string());
    lines.join("\n")
}

fn render(input: &Input, t: &Transcript, reason: Reason, c: &Caps) -> (String, bool) {
    let home = input.home;
    let from = agent_name(input.from);
    let cut = std::cell::Cell::new(false);
    let take = |text: &str, n: usize| {
        let (s, was) = clip(&clean(text, home), n);
        if was {
            cut.set(true);
        }
        s
    };
    let mut parts: Vec<String> = vec![intro(from, reason, &clean(&t.stop, home), input.repos.len())];

    // The task.
    parts.push(if t.task.trim().is_empty() {
        format!("## The task\n{from}'s chat has no message from the user yet. Ask what the task is before you change anything.")
    } else {
        format!("## The task\n{}", quote(&take(&t.task, c.task)))
    });

    // What you said since.
    if c.later > 0 && !t.later.is_empty() {
        let skip = t.later.len().saturating_sub(c.later);
        let mut s = String::from("## What the user said since");
        if skip > 0 {
            s.push_str(&format!("\n({} left out.)", plural(skip, "earlier message", "earlier messages")));
        }
        for m in &t.later[skip..] {
            s.push_str(&format!("\n- {}", take(m, c.later_len).replace('\n', " ")));
        }
        parts.push(s);
    } else if !t.later.is_empty() {
        cut.set(true);
    }

    // What the agent did.
    let mut did = vec![format!("## What {from} did")];
    let answers = &t.answers[t.answers.len().saturating_sub(c.answers)..];
    if t.answers.len() > answers.len() {
        cut.set(true);
    }
    if !answers.is_empty() {
        let head = if answers.len() == 1 { format!("{from}'s last message:") } else { format!("{from}'s last {} messages:", answers.len()) };
        let quoted: Vec<String> = answers.iter().map(|a| quote(&take(a, c.answer_len))).collect();
        did.push(format!("{head}\n{}", quoted.join("\n\n")));
    } else {
        did.push(format!("{from} hasn't written anything yet."));
    }
    // Files changed, per repo.
    let many = input.repos.len() > 1;
    for (i, r) in input.repos.iter().enumerate() {
        let target = code(&r.target);
        let head = match (many, &r.folder) {
            (false, _) => format!("Files changed against {target}:"),
            (true, None) => format!("In {} (this folder), files changed against {target}:", r.name),
            (true, Some(f)) => format!("In {} ({}), files changed against {target}:", r.name, clean(f, home)),
        };
        match &r.files {
            Err(why) => did.push(format!("{} couldn't be read ({}).", if many { format!("{}'s changes", r.name) } else { "The changes".into() }, one_line(why, 160))),
            Ok(files) if files.is_empty() => did.push(if many { format!("In {}{}: no files changed yet against {target}.", r.name, if i == 0 { " (this folder)" } else { "" }) } else { format!("No files changed yet against {target}.") }),
            Ok(files) => {
                let mut lines = vec![head];
                let (add, del) = files.iter().fold((0u64, 0u64), |(a, d), f| (a + f.additions as u64, d + f.deletions as u64));
                for f in files.iter().take(c.files) {
                    let what = match f.status.as_str() {
                        "A" => " (new)",
                        "D" => " (deleted)",
                        "R" => " (renamed)",
                        _ => "",
                    };
                    lines.push(format!("- {} +{} -{}{what}", code(&clean(&f.path, home)), f.additions, f.deletions));
                }
                if files.len() > c.files {
                    cut.set(true);
                    lines.push(format!("- and {}", more(files.len() - c.files, "file", "files")));
                }
                lines.push(format!("{} in all, +{add} -{del}.", plural(files.len(), "file", "files")));
                did.push(lines.join("\n"));
            }
        }
    }
    // Checks.
    if !input.checks.is_empty() {
        let line = input
            .checks
            .iter()
            .map(|k| {
                let what = match k.status.as_str() {
                    "passed" => match k.passed {
                        Some(n) => format!("passed ({n})"),
                        None => "passed".into(),
                    },
                    "failed" if k.failed > 0 => {
                        let names: Vec<String> = k.failures.iter().take(3).map(|f| clean(&one_line(f, 80), home)).collect();
                        if names.is_empty() { format!("failed ({} failing)", k.failed) } else { format!("failed ({} failing: {})", k.failed, names.join(", ")) }
                    }
                    "failed" => "failed".into(),
                    _ => "couldn't run".into(),
                };
                format!("{} {} {what}", k.name, code(&clean(&one_line(&k.command, 100), home)))
            })
            .collect::<Vec<_>>()
            .join("; ");
        did.push(format!("Last checks: {line}."));
    }
    // Commands still failing.
    let failing = still_failing(&t.steps);
    if !failing.is_empty() && c.failed > 0 {
        let skip = failing.len().saturating_sub(c.failed);
        if skip > 0 {
            cut.set(true);
        }
        let mut lines = vec!["Commands that still fail (their last run failed):".to_string()];
        for f in &failing[skip..] {
            let err = if f.error.is_empty() { String::new() } else { format!(": {}", take(&f.error, 160)) };
            lines.push(format!("- {}{err}", code(&take(&f.target, 160))));
        }
        did.push(lines.join("\n"));
    }
    // The last steps, with repeats folded.
    if c.steps > 0 && !t.steps.is_empty() {
        let mut folded: Vec<(&Step, usize)> = vec![];
        for st in &t.steps {
            match folded.last_mut() {
                Some((last, n)) if last.name == st.name && last.target == st.target => {
                    *last = st;
                    *n += 1;
                }
                _ => folded.push((st, 1)),
            }
        }
        let skip = folded.len().saturating_sub(c.steps);
        if skip > 0 {
            cut.set(true);
        }
        let mut lines = vec![if skip > 0 { format!("Its last {} steps (of {}):", folded.len() - skip, t.steps.len()) } else { "Its steps:".to_string() }];
        for (st, n) in &folded[skip..] {
            let times = if *n > 1 { format!(" ({n} times)") } else { String::new() };
            let mark = if st.failed { "Failed: " } else { "" };
            lines.push(format!("- {mark}{} {}{times}", verb(&st.name), code(&take(&st.target, 160))));
        }
        did.push(lines.join("\n"));
    } else if !t.steps.is_empty() {
        cut.set(true);
    }
    parts.push(format!("{}\n{}", did[0], did[1..].join("\n\n")));

    // What's left.
    let mut left = vec!["## What's left".to_string()];
    let open: Vec<&(bool, String)> = t.todos.iter().filter(|(done, _)| !done).collect();
    if !t.todos.is_empty() {
        let done = t.todos.len() - open.len();
        if open.is_empty() {
            left.push(format!("Everything on {from}'s todo list is done ({}). Check the work against the task and finish anything it still needs.", plural(done, "item", "items")));
        } else {
            left.push(format!("From {from}'s todo list ({done} of {} done), still to do:", t.todos.len()));
            for (_, item) in open.iter().take(c.todos) {
                left.push(format!("- [ ] {}", take(item, 200).replace('\n', " ")));
            }
            if open.len() > c.todos {
                cut.set(true);
                left.push(format!("- and {}", more(open.len() - c.todos, "item", "items")));
            }
        }
    } else if let Some(plan) = t.plan.as_deref().filter(|p| !p.trim().is_empty()) {
        left.push(format!("{from}'s plan (check which steps are done):"));
        left.push(quote(&take(plan, c.plan)));
    } else {
        left.push(format!("{from} left no todo list or plan. Work out what's left from the task and the changes."));
    }
    parts.push(left.join("\n"));

    parts.push(your_turn(input, t, reason));
    (parts.join("\n\n"), cut.get())
}

// ---------- the new chat ----------

/// Can "Ask before changes" be promised for this agent? Claude asks itself; ACP agents when they
/// have a way to (`AcpAgent::asks`). Codex and Gemini have none.
pub fn asks(agent: &str) -> bool {
    agent == "claude" || std::iter::once(&crate::acp::OPENCODE).chain(crate::acp::MORE.iter().copied()).any(|a| a.id == agent && a.asks())
}

/// The new chat's mode: the same as the chat it continues, so a hand-off never gives an agent more
/// room than you gave the last one. "Ask before changes" on an agent that can't ask becomes Plan.
pub fn mode_for(mode: &str, target: &str) -> &'static str {
    match mode {
        "ask" if asks(target) => "ask",
        "ask" | "plan" => "plan",
        "default" => "default",
        "bypassPermissions" => "bypassPermissions",
        _ => "acceptEdits",
    }
}

/// The line the old chat keeps ("Handed off to Codex") and the one the new chat starts with
/// ("Continued from Claude"), each pointing at the other chat.
pub fn marker_to(agent: &str, session_id: &str) -> Value {
    serde_json::json!({"kind": "handoff", "dir": "to", "agent": agent, "session_id": session_id})
}

pub fn marker_from(agent: &str, session_id: &str, task: &str, reason: Reason) -> Value {
    serde_json::json!({"kind": "handoff", "dir": "from", "agent": agent, "session_id": session_id, "task": task, "reason": reason})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(path: &str, status: &str, a: u32, d: u32) -> FileChange {
        FileChange { path: path.into(), status: status.into(), additions: a, deletions: d }
    }
    fn repo(files: Vec<FileChange>) -> Vec<RepoChanges> {
        vec![RepoChanges { name: "api".into(), folder: None, target: "main".into(), files: Ok(files) }]
    }
    fn user(t: &str) -> Value {
        json!({"kind": "user", "text": t})
    }
    fn say(t: &str) -> Value {
        json!({"kind": "text", "text": t})
    }
    fn tool(id: &str, name: &str, input: &str) -> Value {
        json!({"kind": "tool", "id": id, "name": name, "input": input})
    }
    fn result(id: &str, output: &str, err: bool) -> Value {
        json!({"kind": "tool_result", "id": id, "output": output, "is_error": err})
    }
    fn brief(events: &[Value], repos: &[RepoChanges], checks: &[CheckLine]) -> Brief {
        build(&Input { from: "claude", events, repos, checks, home: Some("/Users/sam") })
    }
    fn pos(text: &str, needle: &str) -> usize {
        text.find(needle).unwrap_or_else(|| panic!("{needle:?} isn't in:\n{text}"))
    }

    fn chat() -> Vec<Value> {
        vec![
            user("The load balancer returns 502 on /v1/tickets after a deploy. Find out why and fix it."),
            tool("t1", "Bash", "kubectl logs deploy/api | grep 502"),
            result("t1", "proxy error: connection reset by peer", false),
            tool("t2", "Read", "/Users/sam/code/api/internal/server/http.go"),
            result("t2", "srv := ...", false),
            say("The server closes idle connections after 30s, the balancer keeps them 60s."),
            tool("t3", "Edit", "internal/server/http.go"),
            result("t3", "", false),
            tool("t4", "TodoWrite", r#"[{"content":"Raise IdleTimeout to 75s","status":"completed"},{"content":"Make it configurable for the NLB","status":"in_progress"},{"content":"Test without sleeping 61s","status":"pending"}]"#),
            tool("t5", "Bash", "go test ./internal/server/..."),
            result("t5", "\n--- FAIL: TestIdleTimeout (0.00s)\nFAIL", true),
            say("Raised IdleTimeout to 75s. The NLB test still fails; next I make it configurable."),
            json!({"kind": "done", "is_error": false}),
            json!({"kind": "text", "text": "You've hit your limit · resets 3pm", "fix": "limit", "agent": "claude"}),
        ]
    }

    #[test]
    fn a_brief_has_every_part_in_order() {
        let checks = vec![CheckLine { name: "test".into(), command: "go test ./...".into(), status: "failed".into(), passed: Some(41), failed: 2, failures: vec!["TestIdleTimeout/behind_nlb".into(), "TestSLA".into()] }];
        let b = brief(&chat(), &repo(vec![file("internal/server/http.go", "M", 3, 1), file("internal/server/config.go", "A", 20, 0)]), &checks);
        assert_eq!(b.reason, Reason::Limit);
        assert!(b.text.starts_with("Claude ran out of usage partway through this task"), "{}", b.text);
        let order = ["## The task", "502 on /v1/tickets", "## What Claude did", "Claude's last 2 messages", "Raised IdleTimeout to 75s", "Files changed against `main`", "`internal/server/config.go` +20 -0 (new)", "2 files in all, +23 -1.", "Last checks: test `go test ./...` failed (2 failing: TestIdleTimeout/behind_nlb, TestSLA).", "Commands that still fail", "`go test ./internal/server/...`: --- FAIL: TestIdleTimeout (0.00s)", "Its steps:", "- Edited `internal/server/http.go`", "## What's left", "(1 of 3 done)", "- [ ] Make it configurable for the NLB", "- [ ] Test without sleeping 61s", "## Your turn", "`git status` and `git diff`", "get the failing checks and commands above passing"];
        let mut at = 0;
        for want in order {
            let p = pos(&b.text, want);
            assert!(p >= at, "{want:?} is out of order in:\n{}", b.text);
            at = p;
        }
        // Done items, the limit message and the todo tool itself stay out.
        assert!(!b.text.contains("- [ ] Raise IdleTimeout"));
        assert!(!b.text.contains("hit your limit"));
        assert!(!b.text.contains("TodoWrite"));
        // Home paths show as ~.
        assert!(b.text.contains("`~/code/api/internal/server/http.go`"), "{}", b.text);
        assert!(!b.text.contains("/Users/sam"));
        assert_eq!(b.task, "The load balancer returns 502 on /v1/tickets after a deploy. Find out why and fix it.");
        assert!(!b.cut);
        assert!(b.text.chars().count() <= MAX_CHARS);
    }

    #[test]
    fn the_same_chat_always_gives_the_same_brief() {
        let r = repo(vec![file("a.go", "M", 1, 1)]);
        assert_eq!(brief(&chat(), &r, &[]), brief(&chat(), &r, &[]));
    }

    #[test]
    fn an_empty_chat_still_gives_a_usable_brief() {
        let b = brief(&[], &repo(vec![]), &[]);
        assert_eq!(b.reason, Reason::Asked);
        assert!(b.text.starts_with("You're taking over this task from Claude, partway through."));
        assert!(b.text.contains("no message from the user yet. Ask what the task is"));
        assert!(b.text.contains("Claude hasn't written anything yet."));
        assert!(b.text.contains("No files changed yet against `main`."));
        assert!(b.text.contains("left no todo list or plan"));
        assert!(b.text.ends_with("3. When you finish, say what you changed and how you checked it."));
        assert!(b.text.contains("2. Carry on with what's left.\n"), "nothing is failing: {}", b.text);
        assert_eq!((b.hidden, b.cut, b.task.as_str()), (0, false, ""));
    }

    #[test]
    fn a_long_chat_is_cut_to_fit_keeping_the_task_and_the_instruction() {
        let mut ev = vec![user(&format!("Build the export. {}", "Every column matters. ".repeat(150)))];
        for i in 0..400 {
            ev.push(tool(&format!("t{i}"), "Bash", &format!("cargo test export_{i} -- --nocapture {}", "x".repeat(100))));
            ev.push(result(&format!("t{i}"), "error[E0308]: mismatched types", i % 3 == 0));
            ev.push(say(&format!("Step {i}: {}", "I looked at the writer and the reader again. ".repeat(40))));
            ev.push(user(&format!("Also handle case {i}. {}", "Keep it short. ".repeat(30))));
        }
        ev.push(tool("plan", "TodoWrite", &serde_json::to_string(&(0..40).map(|i| json!({"content": format!("Item {i} {}", "y".repeat(150)), "status": "pending"})).collect::<Vec<_>>()).unwrap()));
        let files: Vec<FileChange> = (0..120).map(|i| file(&format!("src/export/part_{i:03}_{}.rs", "z".repeat(40)), "M", i, 1)).collect();
        let b = brief(&ev, &repo(files), &[]);
        let n = b.text.chars().count();
        assert!(n <= MAX_CHARS, "{n} characters");
        assert!(b.cut);
        assert!(b.text.contains("## The task\n> Build the export."));
        assert!(b.text.contains("## Your turn"));
        assert!(b.text.contains("- and 112 more files"), "the file list says how many it left out:\n{}", b.text);
        assert!(b.text.contains("120 files in all"));
        assert!(b.text.contains("- and 32 more items"));
        // The newest message is the one kept.
        assert!(b.text.contains("Step 399:"));
        assert!(!b.text.contains("Step 398:"));
        assert!(b.text.contains("Also handle case 399") || !b.text.contains("Also handle case"), "the latest user message, or none");
    }

    #[test]
    fn shrinks_old_steps_before_the_last_message() {
        let mut ev = vec![user("Fix the export.")];
        for i in 0..30 {
            ev.push(tool(&format!("t{i}"), "Read", &format!("src/file_{i}.rs {}", "q".repeat(140))));
        }
        ev.push(say(&format!("Done with the reader. {}", "Details. ".repeat(80))));
        let b = brief(&ev, &repo(vec![]), &[]);
        assert!(b.text.contains("Its last 12 steps (of 30):"), "{}", b.text);
        assert!(b.text.contains("Done with the reader."));
        assert!(b.cut, "steps were left out");
    }

    #[test]
    fn secrets_are_hidden_everywhere() {
        // Made up, and put together at run time so no file holds anything shaped like a real key.
        let token = ["ghp", "Zq81Lm02Xc93Vb47Nn56Aa19Ss28Dd37Ff46"].join("_");
        let aws = ["Qm7Rt2Vx9", "Kp4Wz8Ny3", "Hb6Jc1Ld5", "Fg0Sa"].concat();
        let ev = vec![
            user(&format!("Use the token {token} to call the API.")),
            tool("t1", "Bash", &format!("curl -H \"Authorization: Bearer {token}\" https://api.github.com/user")),
            result("t1", &format!("{}={aws}", ["AWS", "SECRET", "ACCESS", "KEY"].join("_")), true),
            say(&format!("I used {token}.")),
        ];
        let b = brief(&ev, &repo(vec![]), &[]);
        assert!(!b.text.contains(&token), "{}", b.text);
        assert!(!b.text.contains(&aws), "{}", b.text);
        assert!(!b.task.contains(&token));
        assert!(b.hidden >= 3, "{} hidden in {}", b.hidden, b.text);
        assert!(b.text.contains("[hidden: "));
    }

    #[test]
    fn a_multi_repo_task_lists_each_repo_and_its_folder() {
        let repos = vec![
            RepoChanges { name: "pawprint-api".into(), folder: None, target: "main".into(), files: Ok(vec![file("api/orders.go", "M", 30, 2)]) },
            RepoChanges { name: "pawprint-web".into(), folder: Some("/Users/sam/code/ws/pawprint-web/vega".into()), target: "main".into(), files: Ok(vec![file("src/Orders.tsx", "A", 40, 0)]) },
            RepoChanges { name: "pawprint-types".into(), folder: Some("/Users/sam/code/ws/pawprint-types/vega".into()), target: "develop".into(), files: Ok(vec![]) },
            RepoChanges { name: "pawprint-docs".into(), folder: Some("/Users/sam/code/ws/pawprint-docs/vega".into()), target: "main".into(), files: Err("not a git repository".into()) },
        ];
        let b = brief(&[user("Export orders as CSV.")], &repos, &[]);
        assert!(b.text.contains("This task spans 4 repos"), "{}", b.text);
        assert!(b.text.contains("You work in the same folders"));
        let a = pos(&b.text, "In pawprint-api (this folder), files changed against `main`:\n- `api/orders.go` +30 -2");
        let w = pos(&b.text, "In pawprint-web (~/code/ws/pawprint-web/vega), files changed against `main`:\n- `src/Orders.tsx` +40 -0 (new)");
        let t = pos(&b.text, "In pawprint-types: no files changed yet against `develop`.");
        let d = pos(&b.text, "pawprint-docs's changes couldn't be read (not a git repository).");
        assert!(a < w && w < t && t < d, "repos keep their order");
    }

    #[test]
    fn why_comes_from_after_the_last_message() {
        let limit = json!({"kind": "text", "text": "limit", "fix": "limit"});
        assert_eq!(brief(&[user("a"), limit.clone()], &[], &[]).reason, Reason::Limit);
        assert_eq!(brief(&[user("a"), limit, user("try again")], &[], &[]).reason, Reason::Asked);
        let lp = json!({"kind": "stop", "reason": "loop", "text": "The same command failed 4 times in a row: go test ./..."});
        let b = brief(&[user("a"), lp], &[], &[]);
        assert_eq!(b.reason, Reason::Loop);
        assert!(b.text.starts_with("nekotrail stopped Claude because it was going in circles (The same command failed 4 times in a row: go test ./...), so you're taking over."), "{}", b.text);
        assert_eq!(brief(&[user("a"), json!({"kind": "stop", "reason": "budget", "text": "cap"})], &[], &[]).reason, Reason::Budget);
        // An install or login problem isn't a reason to hand off.
        assert_eq!(brief(&[user("a"), json!({"kind": "error", "text": "x", "fix": "login"})], &[], &[]).reason, Reason::Asked);
    }

    #[test]
    fn a_command_that_passed_later_is_not_failing() {
        let ev = vec![
            user("go"),
            tool("1", "Bash", "make test"),
            result("1", "FAIL", true),
            tool("2", "Bash", "make lint"),
            result("2", "lint: 3 problems", true),
            tool("3", "Bash", "make test"),
            result("3", "ok", false),
        ];
        let b = brief(&ev, &repo(vec![]), &[]);
        assert!(b.text.contains("Commands that still fail (their last run failed):\n- `make lint`: lint: 3 problems"), "{}", b.text);
        assert!(!b.text.contains("- `make test`: FAIL"));
        assert!(b.text.contains("- Failed: Ran `make test`"), "the step list still shows it failed once");
    }

    #[test]
    fn a_second_hand_off_keeps_the_original_task() {
        let first = brief(&chat(), &repo(vec![]), &[]);
        let ev = vec![
            marker_from("claude", "s1", &first.task, first.reason),
            user(&first.text),
            say("Made the timeout configurable."),
            tool("t9", "Bash", "go test ./..."),
            result("t9", "ok", false),
        ];
        let b = build(&Input { from: "codex", events: &ev, repos: &repo(vec![]), checks: &[], home: None });
        assert_eq!(b.task, first.task);
        assert!(b.text.starts_with("You're taking over this task from Codex"));
        assert!(b.text.contains("## The task\n> The load balancer returns 502"));
        assert!(!b.text.contains("What the user said since"), "the old brief is not a message from the user");
        assert!(!b.text.contains("Claude ran out of usage"), "the old brief isn't quoted back");
    }

    #[test]
    fn later_messages_and_plans() {
        let ev = vec![
            user("Add CSV export."),
            user("Use semicolons."),
            user("And a header row."),
            tool("p", "ExitPlanMode", "## Plan\n1. Writer\n2. Endpoint"),
        ];
        let b = brief(&ev, &repo(vec![]), &[]);
        assert!(b.text.contains("## What the user said since\n- Use semicolons.\n- And a header row."), "{}", b.text);
        assert!(b.text.contains("Claude's plan (check which steps are done):\n> ## Plan\n> 1. Writer"));
        let done = vec![user("x"), tool("t", "TodoWrite", r#"[{"content":"a","status":"completed"}]"#)];
        assert!(brief(&done, &repo(vec![]), &[]).text.contains("Everything on Claude's todo list is done (1 item)."));
    }

    #[test]
    fn modes_never_loosen() {
        assert_eq!(mode_for("ask", "claude"), "ask");
        assert_eq!(mode_for("ask", "opencode"), "ask");
        assert_eq!(mode_for("ask", "copilot"), "ask");
        assert_eq!(mode_for("ask", "codex"), "plan", "Codex can't ask, so it only reads");
        assert_eq!(mode_for("ask", "gemini"), "plan");
        assert_eq!(mode_for("plan", "codex"), "plan");
        assert_eq!(mode_for("acceptEdits", "gemini"), "acceptEdits");
        assert_eq!(mode_for("bypassPermissions", "codex"), "bypassPermissions");
        assert_eq!(mode_for("default", "codex"), "default");
        assert_eq!(mode_for("anything else", "codex"), "acceptEdits");
    }

    #[test]
    fn code_spans_and_clipping() {
        assert_eq!(code("a`b"), "``a`b``");
        assert_eq!(code("`x`"), "`` `x` ``");
        assert_eq!(clip("héllo wörld", 6), ("héllo…".to_string(), true));
        assert_eq!(clip("short", 10), ("short".to_string(), false));
        assert_eq!(one_line("first\nsecond", 50), "first …");
    }
}
