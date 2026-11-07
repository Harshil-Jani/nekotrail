//! Multi-repo tasks: one task that changes 2 to 5 repos at once (an API, its web app, shared types).
//!
//! nekotrail makes a workspace in each repo, all on the same branch name, and links them with a
//! group id: the id of the main workspace, which the one chat runs in. The agent's working folder
//! is the main workspace; the others are passed as extra folders (`TurnSpec::dirs`). Only agents
//! that can take extra folders are offered (`Agent::extra_dirs`), checked 2026-11-06:
//!
//! | Agent    | How                                        | Checked with                       |
//! |----------|--------------------------------------------|------------------------------------|
//! | Claude   | `--add-dir <dir>`                          | `claude --help`, 2.1.319           |
//! | Codex    | `codex exec --add-dir <dir>`               | `codex exec --help`, 0.164.0       |
//! | Gemini   | `--include-directories <dir>`              | `gemini --help`, 0.65.0            |
//! | Copilot  | `copilot --acp --add-dir <dir>`            | `copilot --help`, 1.0.103          |
//! | the rest | none found: not offered                    | see below                          |
//!
//! ACP has `additionalDirectories` on `session/new`, `load` and `resume` for agents that advertise
//! `sessionCapabilities.additionalDirectories` (agentclientprotocol.com, session setup). nekotrail
//! sends it whenever an agent advertises it, but OpenCode 1.17.20, Copilot 1.0.103 and Grok 1.0.70
//! don't, so none is offered on that alone. Grok's `--add-dir` exists on `grok` but not on
//! `grok agent stdio`, which is what nekotrail runs. Kimi documents `--add-dir` and Qwen Code
//! `--include-directories` for their terminal UIs; neither is installed here to confirm the ACP
//! mode reads them, so they stay off until someone does.
//!
//! Pull requests: one per repo, then each body gets a line linking all of them
//! ("Part of a change across: org/api#12, org/web#34"). "Merge all" merges in the order the user
//! picked, only when every PR is green, and stops at the first one that fails.

use serde::Serialize;
use serde_json::Value;

pub const MIN_REPOS: usize = 2;
pub const MAX_REPOS: usize = 5;

/// The repos picked for a new multi-repo task: 2 to 5, none twice.
pub fn check_repos(ids: &[String]) -> Result<(), String> {
    if ids.len() < MIN_REPOS {
        return Err("Pick at least two repos for a multi-repo task.".into());
    }
    if ids.len() > MAX_REPOS {
        return Err(format!("A multi-repo task can span up to {MAX_REPOS} repos."));
    }
    for (i, id) in ids.iter().enumerate() {
        if ids[..i].contains(id) {
            return Err("Each repo can only be in a task once.".into());
        }
    }
    Ok(())
}

/// The folders the agent gets besides its own: the other active workspaces of the task. Only the
/// main workspace (whose id is the group id) has any.
pub fn linked_dirs(ws_id: &str, group_id: &str, members: &[(String, String, bool)]) -> Vec<String> {
    if group_id.is_empty() || ws_id != group_id {
        return vec![];
    }
    members.iter().filter(|(id, _, active)| id != ws_id && *active).map(|(_, path, _)| path.clone()).collect()
}

/// The order to merge in: the saved order for the workspaces still in the task, then any that
/// aren't in it yet, in their own order.
pub fn merge_order(members: &[String], saved: &[String]) -> Vec<String> {
    let mut out: Vec<String> = saved.iter().filter(|id| members.contains(id)).cloned().collect();
    out.dedup();
    for m in members {
        if !out.contains(m) {
            out.push(m.clone());
        }
    }
    out
}

/// One PR of the task, as GitHub names it across repos: `org/api#12`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PrRef {
    pub workspace_id: String,
    /// `owner/name`.
    pub repo: String,
    pub number: u64,
    pub url: String,
}

impl PrRef {
    pub fn short(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }
}

/// `owner/name` and the number from a PR link (`https://github.com/org/api/pull/12`, GitHub
/// Enterprise hosts too).
pub fn parse_pr_url(url: &str) -> Option<(String, u64)> {
    let rest = url.trim().split_once("://").map(|(_, r)| r)?;
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    // host / owner / name / pull / number
    if parts.len() < 5 || parts[3] != "pull" {
        return None;
    }
    let n = parts[4].split(['#', '?']).next()?.parse().ok()?;
    (!parts[1].is_empty() && !parts[2].is_empty()).then(|| (format!("{}/{}", parts[1], parts[2]), n))
}

/// Marks where nekotrail's line starts in a PR body, so making the PRs again replaces it.
pub const LINKS_MARK: &str = "<!-- nekotrail: linked PRs -->";

/// The line every PR body of the task ends with.
pub fn links_line(refs: &[PrRef]) -> String {
    let list: Vec<String> = refs.iter().map(PrRef::short).collect();
    format!("Part of a change across: {}", list.join(", "))
}

/// `body` with the task's links at the end, replacing an earlier version of them.
pub fn body_with_links(body: &str, refs: &[PrRef]) -> String {
    let own = match body.find(LINKS_MARK) {
        Some(i) => &body[..i],
        None => body,
    };
    let own = own.trim_end();
    let tail = format!("{LINKS_MARK}\n{}", links_line(refs));
    if own.is_empty() { tail } else { format!("{own}\n\n{tail}") }
}

/// Why a PR can't be merged yet, from `gh pr view --json state,isDraft,mergeable,
/// statusCheckRollup,reviewDecision`. None: it's green (open, ready, no conflicts, every check
/// passed, no changes requested). An already merged PR is "merged", which Merge all skips.
pub fn blocker(pr: &Value) -> Option<String> {
    match pr["state"].as_str() {
        Some("OPEN") => {}
        Some("MERGED") => return Some("merged".into()),
        Some("CLOSED") => return Some("closed".into()),
        _ => return Some("no PR".into()),
    }
    if pr["isDraft"] == true {
        return Some("draft".into());
    }
    if pr["mergeable"] == "CONFLICTING" {
        return Some("conflicts".into());
    }
    let result = |c: &Value| c["conclusion"].as_str().or(c["state"].as_str()).or(c["status"].as_str()).unwrap_or("").to_uppercase();
    let checks = pr["statusCheckRollup"].as_array().cloned().unwrap_or_default();
    let failing = checks.iter().filter(|c| ["FAILURE", "ERROR", "TIMED_OUT", "ACTION_REQUIRED", "STARTUP_FAILURE", "CANCELLED"].contains(&result(c).as_str())).count();
    if failing > 0 {
        return Some(format!("{failing} failing {}", if failing == 1 { "check" } else { "checks" }));
    }
    let running = checks.iter().filter(|c| ["", "PENDING", "QUEUED", "IN_PROGRESS", "EXPECTED", "WAITING"].contains(&result(c).as_str())).count();
    if running > 0 {
        return Some(format!("{running} {} running", if running == 1 { "check" } else { "checks" }));
    }
    if pr["reviewDecision"] == "CHANGES_REQUESTED" {
        return Some("changes requested".into());
    }
    None
}

/// Runs `gh`. The app runs the real one; tests hand in a fake.
pub trait Gh {
    fn run(&self, dir: &str, args: &[&str]) -> Result<String, String>;
}

/// The `gh` at `bin` (normally just "gh", found on PATH).
pub struct GhCli<'a>(pub &'a str);

impl Gh for GhCli<'_> {
    fn run(&self, dir: &str, args: &[&str]) -> Result<String, String> {
        let out = std::process::Command::new(self.0).args(args).current_dir(dir).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(if err.is_empty() { format!("gh exited with {}", out.status) } else { err });
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

/// One repo of the task, as `gh` needs it.
#[derive(Debug, Clone)]
pub struct Member {
    pub workspace_id: String,
    /// The repo's name in nekotrail, for messages.
    pub name: String,
    /// Where `gh` runs: the workspace folder.
    pub dir: String,
    pub branch: String,
    pub base: String,
    /// `--squash`, `--merge` or `--rebase`, from the repo's settings.
    pub merge_flag: &'static str,
}

/// Open (or reuse) one PR per repo, then give every body the line that links them all.
/// Branches must be pushed already. `bodies` has one body per member. An error names what was
/// opened before it, so pressing again finishes the job (open PRs are reused, never doubled).
pub fn create_all(gh: &dyn Gh, members: &[Member], title: &str, bodies: &[String], draft: bool) -> Result<Vec<PrRef>, String> {
    let mut refs: Vec<PrRef> = vec![];
    let mut texts: Vec<String> = vec![];
    for (i, m) in members.iter().enumerate() {
        let made = (|| -> Result<(PrRef, String), String> {
            let open = gh.run(&m.dir, &["pr", "list", "--head", &m.branch, "--state", "open", "--limit", "1", "--json", "number,url,body"])?;
            let open: Value = serde_json::from_str(&open).map_err(|e| format!("gh answered something unexpected: {e}"))?;
            if let Some(pr) = open.get(0) {
                let url = pr["url"].as_str().unwrap_or("").to_string();
                let (repo, number) = parse_pr_url(&url).ok_or_else(|| format!("gh gave a PR link NekoTrail can't read: {url}"))?;
                return Ok((PrRef { workspace_id: m.workspace_id.clone(), repo, number, url }, pr["body"].as_str().unwrap_or("").to_string()));
            }
            let body = bodies.get(i).cloned().unwrap_or_default();
            let mut args = vec!["pr", "create", "--head", &m.branch, "--base", &m.base, "--title", title, "--body", &body];
            if draft {
                args.push("--draft");
            }
            let out = gh.run(&m.dir, &args)?;
            let url = out.lines().rev().map(str::trim).find(|l| l.contains("/pull/")).unwrap_or("").to_string();
            let (repo, number) = parse_pr_url(&url).ok_or_else(|| format!("gh didn't say which PR it opened: {}", out.trim()))?;
            Ok((PrRef { workspace_id: m.workspace_id.clone(), repo, number, url }, body))
        })();
        match made {
            Ok((r, body)) => {
                refs.push(r);
                texts.push(body);
            }
            Err(err) => {
                let done: Vec<String> = refs.iter().map(PrRef::short).collect();
                let before = if done.is_empty() { String::new() } else { format!("Opened {}. ", done.join(", ")) };
                return Err(format!("{before}Couldn't open the PR in {}: {err}. Press Create PRs again to finish; open PRs are kept.", m.name));
            }
        }
    }
    for ((m, r), body) in members.iter().zip(&refs).zip(&texts) {
        let linked = body_with_links(body, &refs);
        if linked != *body {
            let n = r.number.to_string();
            gh.run(&m.dir, &["pr", "edit", &n, "--body", &linked])
                .map_err(|err| format!("Opened {}, but couldn't add the links to {}: {err}", refs.iter().map(PrRef::short).collect::<Vec<_>>().join(", "), r.short()))?;
        }
    }
    Ok(refs)
}

/// What Merge all did with one repo's PR.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
    pub workspace_id: String,
    pub name: String,
    /// "merged", "already" (merged before), "failed" or "skipped" (after a failure).
    pub outcome: &'static str,
    pub detail: String,
}

/// Merge every PR of the task in `order`. Nothing merges unless every PR is green (or already
/// merged). Stops at the first merge that fails; the ones after it are skipped. Err: not all green.
pub fn merge_all(gh: &dyn Gh, order: &[Member]) -> Result<Vec<Step>, String> {
    let mut merged_before: Vec<bool> = vec![];
    let mut waiting: Vec<String> = vec![];
    for m in order {
        let view = gh.run(&m.dir, &["pr", "view", &m.branch, "--json", "number,state,isDraft,mergeable,statusCheckRollup,reviewDecision"]);
        let why = match view {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(pr) => blocker(&pr),
                Err(_) => Some("couldn't read its PR".to_string()),
            },
            Err(err) if err.contains("no pull requests found") => Some("no PR".to_string()),
            Err(err) => Some(err),
        };
        merged_before.push(why.as_deref() == Some("merged"));
        match why.as_deref() {
            None | Some("merged") => {}
            Some(w) => waiting.push(format!("{}: {w}", m.name)),
        }
    }
    if !waiting.is_empty() {
        return Err(format!("Not every PR is green yet, so nothing was merged. {}.", waiting.join("; ")));
    }
    let mut steps: Vec<Step> = vec![];
    let mut failed = false;
    for (m, before) in order.iter().zip(merged_before) {
        let step = |outcome, detail: String| Step { workspace_id: m.workspace_id.clone(), name: m.name.clone(), outcome, detail };
        if failed {
            steps.push(step("skipped", "Not merged because an earlier one failed.".into()));
        } else if before {
            steps.push(step("already", "Merged before.".into()));
        } else {
            match gh.run(&m.dir, &["pr", "merge", &m.branch, m.merge_flag]) {
                Ok(_) => steps.push(step("merged", String::new())),
                Err(err) => {
                    failed = true;
                    steps.push(step("failed", err));
                }
            }
        }
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A fake gh: answers by (folder, first two args), records every call.
    #[derive(Default)]
    struct FakeGh {
        answers: HashMap<(String, String), Result<String, String>>,
        calls: RefCell<Vec<(String, Vec<String>)>>,
    }

    impl FakeGh {
        fn on(mut self, dir: &str, cmd: &str, answer: Result<&str, &str>) -> Self {
            self.answers.insert((dir.into(), cmd.into()), answer.map(str::to_string).map_err(str::to_string));
            self
        }
        fn calls(&self, cmd: &str) -> Vec<(String, Vec<String>)> {
            self.calls.borrow().iter().filter(|(_, a)| a[..2].join(" ") == cmd).cloned().collect()
        }
    }

    impl Gh for FakeGh {
        fn run(&self, dir: &str, args: &[&str]) -> Result<String, String> {
            self.calls.borrow_mut().push((dir.into(), args.iter().map(|a| a.to_string()).collect()));
            let key = (dir.to_string(), args[..2].join(" "));
            self.answers.get(&key).cloned().unwrap_or_else(|| Err(format!("unexpected gh {}", args.join(" "))))
        }
    }

    fn member(name: &str) -> Member {
        Member { workspace_id: format!("ws-{name}"), name: name.into(), dir: format!("/w/{name}"), branch: "me/vega".into(), base: "main".into(), merge_flag: "--squash" }
    }

    const GREEN: &str = r#"{"number":1,"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","statusCheckRollup":[{"name":"ci","status":"COMPLETED","conclusion":"SUCCESS"}],"reviewDecision":"APPROVED"}"#;

    #[test]
    fn repo_count_and_duplicates() {
        let ids = |n: usize| (0..n).map(|i| format!("r{i}")).collect::<Vec<_>>();
        assert!(check_repos(&ids(1)).is_err());
        assert!(check_repos(&ids(2)).is_ok());
        assert!(check_repos(&ids(5)).is_ok());
        assert!(check_repos(&ids(6)).is_err());
        assert!(check_repos(&["a".into(), "b".into(), "a".into()]).is_err());
    }

    #[test]
    fn only_the_main_workspace_gets_the_other_folders() {
        let members = vec![
            ("main".to_string(), "/w/api".to_string(), true),
            ("web".to_string(), "/w/web".to_string(), true),
            ("old".to_string(), "/w/old".to_string(), false),
        ];
        assert_eq!(linked_dirs("main", "main", &members), vec!["/w/web".to_string()]);
        assert!(linked_dirs("web", "main", &members).is_empty());
        assert!(linked_dirs("main", "", &members).is_empty());
    }

    #[test]
    fn merge_order_keeps_the_saved_order_and_adds_new_repos() {
        let m = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(merge_order(&m(&["a", "b", "c"]), &m(&["c", "a"])), m(&["c", "a", "b"]));
        assert_eq!(merge_order(&m(&["a", "b"]), &m(&["gone", "b"])), m(&["b", "a"]));
        assert_eq!(merge_order(&m(&["a", "b"]), &[]), m(&["a", "b"]));
    }

    #[test]
    fn pr_links_read_github_and_enterprise_urls() {
        assert_eq!(parse_pr_url("https://github.com/org/api/pull/12"), Some(("org/api".into(), 12)));
        assert_eq!(parse_pr_url("https://git.corp.example/team/web/pull/34/files"), Some(("team/web".into(), 34)));
        assert_eq!(parse_pr_url("https://github.com/org/api/pull/7#issuecomment-1"), Some(("org/api".into(), 7)));
        assert_eq!(parse_pr_url("https://github.com/org/api/issues/12"), None);
        assert_eq!(parse_pr_url("not a link"), None);
    }

    #[test]
    fn bodies_get_one_links_line_that_is_replaced_not_stacked() {
        let refs = vec![
            PrRef { workspace_id: "a".into(), repo: "org/api".into(), number: 12, url: String::new() },
            PrRef { workspace_id: "b".into(), repo: "org/web".into(), number: 34, url: String::new() },
        ];
        let once = body_with_links("Adds the export endpoint.\n", &refs);
        assert_eq!(once, format!("Adds the export endpoint.\n\n{LINKS_MARK}\nPart of a change across: org/api#12, org/web#34"));
        assert_eq!(body_with_links(&once, &refs), once);
        assert_eq!(body_with_links("", &refs[..1]), format!("{LINKS_MARK}\nPart of a change across: org/api#12"));
    }

    #[test]
    fn create_all_opens_each_pr_then_cross_links_every_body() {
        let gh = FakeGh::default()
            .on("/w/api", "pr list", Ok("[]"))
            .on("/w/web", "pr list", Ok(r#"[{"number":34,"url":"https://github.com/org/web/pull/34","body":"Web side."}]"#))
            .on("/w/api", "pr create", Ok("Creating pull request...\nhttps://github.com/org/api/pull/12\n"))
            .on("/w/api", "pr edit", Ok(""))
            .on("/w/web", "pr edit", Ok(""));
        let refs = create_all(&gh, &[member("api"), member("web")], "Export orders", &["API side.".into(), "unused".into()], true).unwrap();
        assert_eq!(refs.iter().map(PrRef::short).collect::<Vec<_>>(), ["org/api#12", "org/web#34"]);
        // The open web PR is reused, so only api is created, as a draft.
        let created = gh.calls("pr create");
        assert_eq!(created.len(), 1);
        assert!(created[0].1.contains(&"--draft".to_string()));
        assert!(created[0].1.windows(2).any(|w| w == ["--base", "main"]));
        // Both bodies end with the same line, after their own text.
        let edits = gh.calls("pr edit");
        assert_eq!(edits.len(), 2);
        let body = |i: usize| edits[i].1.last().unwrap().clone();
        assert_eq!(edits[0].1[2], "12");
        assert!(body(0).starts_with("API side.\n\n"));
        assert!(body(1).starts_with("Web side.\n\n"));
        for i in 0..2 {
            assert!(body(i).ends_with("Part of a change across: org/api#12, org/web#34"), "{}", body(i));
        }
    }

    #[test]
    fn create_all_says_what_it_opened_before_a_failure() {
        let gh = FakeGh::default()
            .on("/w/api", "pr list", Ok("[]"))
            .on("/w/api", "pr create", Ok("https://github.com/org/api/pull/12"))
            .on("/w/web", "pr list", Ok("[]"))
            .on("/w/web", "pr create", Err("GraphQL: No commits between main and me/vega"));
        let err = create_all(&gh, &[member("api"), member("web")], "t", &[], false).unwrap_err();
        assert!(err.starts_with("Opened org/api#12. Couldn't open the PR in web: GraphQL: No commits"), "{err}");
        assert!(gh.calls("pr edit").is_empty(), "no links until every PR is open");
    }

    #[test]
    fn green_means_open_ready_passing_and_not_blocked() {
        assert_eq!(blocker(&serde_json::from_str(GREEN).unwrap()), None);
        let with = |k: &str, v: Value| { let mut p: Value = serde_json::from_str(GREEN).unwrap(); p[k] = v; blocker(&p) };
        assert_eq!(with("isDraft", json!(true)).as_deref(), Some("draft"));
        assert_eq!(with("mergeable", json!("CONFLICTING")).as_deref(), Some("conflicts"));
        assert_eq!(with("reviewDecision", json!("CHANGES_REQUESTED")).as_deref(), Some("changes requested"));
        assert_eq!(with("state", json!("MERGED")).as_deref(), Some("merged"));
        assert_eq!(with("statusCheckRollup", json!([{"conclusion": "FAILURE"}, {"conclusion": "SUCCESS"}])).as_deref(), Some("1 failing check"));
        assert_eq!(with("statusCheckRollup", json!([{"status": "IN_PROGRESS", "conclusion": null}, {"state": "PENDING"}])).as_deref(), Some("2 checks running"));
        // A repo with no checks at all is green.
        assert_eq!(with("statusCheckRollup", json!([])), None);
    }

    #[test]
    fn merge_all_merges_in_the_chosen_order() {
        let gh = FakeGh::default()
            .on("/w/api", "pr view", Ok(GREEN)).on("/w/web", "pr view", Ok(GREEN)).on("/w/types", "pr view", Ok(GREEN))
            .on("/w/api", "pr merge", Ok("")).on("/w/web", "pr merge", Ok("")).on("/w/types", "pr merge", Ok(""));
        let steps = merge_all(&gh, &[member("types"), member("api"), member("web")]).unwrap();
        assert!(steps.iter().all(|s| s.outcome == "merged"));
        let merged: Vec<String> = gh.calls("pr merge").into_iter().map(|(d, _)| d).collect();
        assert_eq!(merged, ["/w/types", "/w/api", "/w/web"]);
        assert_eq!(gh.calls("pr merge")[0].1, ["pr", "merge", "me/vega", "--squash"]);
    }

    #[test]
    fn merge_all_merges_nothing_unless_every_pr_is_green() {
        let red = r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","statusCheckRollup":[{"conclusion":"FAILURE"}]}"#;
        let gh = FakeGh::default()
            .on("/w/api", "pr view", Ok(GREEN))
            .on("/w/web", "pr view", Ok(red))
            .on("/w/types", "pr view", Err("no pull requests found for branch \"me/vega\""));
        let err = merge_all(&gh, &[member("api"), member("web"), member("types")]).unwrap_err();
        assert_eq!(err, "Not every PR is green yet, so nothing was merged. web: 1 failing check; types: no PR.");
        assert!(gh.calls("pr merge").is_empty());
    }

    #[test]
    fn merge_all_stops_at_the_first_failure_and_skips_the_rest() {
        let gh = FakeGh::default()
            .on("/w/types", "pr view", Ok(GREEN)).on("/w/api", "pr view", Ok(GREEN)).on("/w/web", "pr view", Ok(GREEN))
            .on("/w/types", "pr merge", Ok(""))
            .on("/w/api", "pr merge", Err("Pull request is not mergeable: the base branch policy prohibits the merge"))
            .on("/w/web", "pr merge", Ok(""));
        let steps = merge_all(&gh, &[member("types"), member("api"), member("web")]).unwrap();
        let outcomes: Vec<&str> = steps.iter().map(|s| s.outcome).collect();
        assert_eq!(outcomes, ["merged", "failed", "skipped"]);
        assert!(steps[1].detail.contains("base branch policy"));
        assert_eq!(gh.calls("pr merge").len(), 2, "web is never tried");
    }

    #[test]
    fn merge_all_again_after_a_failure_skips_the_merged_ones() {
        let merged = r#"{"state":"MERGED","isDraft":false}"#;
        let gh = FakeGh::default()
            .on("/w/types", "pr view", Ok(merged)).on("/w/api", "pr view", Ok(GREEN))
            .on("/w/api", "pr merge", Ok(""));
        let steps = merge_all(&gh, &[member("types"), member("api")]).unwrap();
        assert_eq!(steps.iter().map(|s| s.outcome).collect::<Vec<_>>(), ["already", "merged"]);
        assert_eq!(gh.calls("pr merge").len(), 1);
    }

    /// The real process path: `GhCli` runs a fake `gh` script and reports its stderr on failure.
    #[cfg(unix)]
    #[test]
    fn gh_cli_runs_the_binary_and_reports_its_error() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let gh = tmp.path().join("gh");
        std::fs::write(&gh, "#!/bin/sh\nif [ \"$2\" = merge ]; then echo 'merge blocked' >&2; exit 1; fi\necho \"$PWD $*\"\n").unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let cli = GhCli(gh.to_str().unwrap());
        let dir = tmp.path().to_str().unwrap();
        assert!(cli.run(dir, &["pr", "view", "me/vega"]).unwrap().ends_with("pr view me/vega\n"));
        assert_eq!(cli.run(dir, &["pr", "merge", "me/vega"]), Err("merge blocked".into()));
    }

    #[test]
    fn agents_offered_for_a_multi_repo_task() {
        let mut ids: Vec<&str> = crate::agent::ALL.iter().filter(|a| a.extra_dirs()).map(|a| a.id()).collect();
        ids.sort();
        assert_eq!(ids, ["claude", "codex", "copilot", "gemini"]);
    }
}
