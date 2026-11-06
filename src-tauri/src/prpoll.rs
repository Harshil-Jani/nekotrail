//! One PR poller for every workspace. Per repo it asks GitHub for all active branches in a single
//! `gh api graphql` call, instead of one `gh pr list` per workspace. The PR bar, the sidebar, the
//! board, autopilot, stacked PRs and the overlap radar all read the same cache.
//!
//! This file holds the parts that need no app: building the query, turning GitHub's answer into the
//! same shape `gh pr list --json` gives, sorting out what went wrong, and deciding when each repo
//! is due next. The loop that ties it to the database and the window lives in `lib.rs`.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// While checks run or autopilot is on for a repo.
pub const HOT: Duration = Duration::from_secs(20);
/// When nothing is moving.
pub const IDLE: Duration = Duration::from_secs(300);
/// Longest wait after repeated failures.
pub const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);
/// Branches per GraphQL call. Each one is an alias with up to 100 checks, well inside GitHub's limits.
pub const MAX_PER_QUERY: usize = 40;
/// Below this many GraphQL points left, stop polling fast until the hour resets.
pub const LOW_RATE: u64 = 100;

/// What went wrong for a repo. Only `NoGh`, `NotLoggedIn` and `Other` are worth showing.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// `gh` isn't installed (or not on PATH).
    NoGh,
    /// `gh` is there but not signed in.
    NotLoggedIn,
    /// No GitHub remote, or GitHub doesn't know the repo.
    NotGithub,
    /// GitHub said slow down. `reset_in` is how long until the limit resets, when it said.
    RateLimited { reset_in: Option<Duration> },
    Other(String),
}

impl Problem {
    /// Short code for the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Problem::NoGh => "no_gh",
            Problem::NotLoggedIn => "not_logged_in",
            Problem::NotGithub => "not_github",
            Problem::RateLimited { .. } => "rate_limited",
            Problem::Other(_) => "error",
        }
    }

    /// Words for the PR bar, for the problems it shows.
    pub fn message(&self) -> String {
        match self {
            Problem::NoGh => "GitHub CLI (gh) isn't installed".into(),
            Problem::NotLoggedIn => "gh isn't logged in".into(),
            Problem::NotGithub => "This repo has no GitHub remote".into(),
            Problem::RateLimited { .. } => "GitHub asked us to slow down".into(),
            Problem::Other(s) => s.clone(),
        }
    }

    /// Problems the user can't fix by waiting: no point retrying them on every focus.
    pub fn is_sticky(&self) -> bool {
        matches!(self, Problem::NoGh | Problem::NotGithub)
    }
}

/// The GraphQL query for `n` branches: aliases `b0..bn`, each the newest PR from that head branch
/// (open, merged or closed), plus the rate limit. Branch names travel as variables, never inside
/// the query text, so no name can break or change the query.
pub fn build_query(n: usize) -> String {
    let vars: Vec<String> = (0..n).map(|i| format!(", $b{i}: String!")).collect();
    let fields: Vec<String> = (0..n)
        .map(|i| format!("    b{i}: pullRequests(headRefName: $b{i}, first: 1, orderBy: {{field: CREATED_AT, direction: DESC}}) {{ nodes {{ ...pr }} }}"))
        .collect();
    format!(
        "query($owner: String!, $name: String!{}) {{\n  rateLimit {{ remaining resetAt }}\n  repository(owner: $owner, name: $name) {{\n{}\n  }}\n}}\n{}",
        vars.join(""),
        fields.join("\n"),
        PR_FRAGMENT
    )
}

const PR_FRAGMENT: &str = "fragment pr on PullRequest {
  number url state title isDraft reviewDecision mergeable headRefOid baseRefName additions deletions
  author { login }
  comments { totalCount }
  latestReviews(first: 20) { nodes { author { login } state } }
  reviewRequests(first: 20) { nodes { requestedReviewer { __typename ... on User { login } ... on Team { name } ... on Bot { login } ... on Mannequin { login } } } }
  commits(last: 1) { nodes { commit { statusCheckRollup { contexts(first: 100) { nodes { __typename ... on CheckRun { name status conclusion detailsUrl } ... on StatusContext { context state targetUrl } } } } } } }
}";

/// Arguments for `gh` for one chunk of branches. `{owner}` and `{repo}` are filled in by gh from the
/// repo folder it runs in, so it picks the same GitHub repo `gh pr list` would (forks included).
/// Branches go with `-f` (raw strings): `-F` would read `@name` as a file or `123` as a number.
pub fn gh_args(branches: &[String], hostname: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = vec!["api".into(), "graphql".into()];
    if let Some(h) = hostname {
        a.extend(["--hostname".into(), h.into()]);
    }
    a.extend(["-f".into(), format!("query={}", build_query(branches.len()))]);
    a.extend(["-F".into(), "owner={owner}".into(), "-F".into(), "name={repo}".into()]);
    for (i, b) in branches.iter().enumerate() {
        a.extend(["-f".into(), format!("b{i}={b}")]);
    }
    a
}

/// The GitHub host for a remote URL when it isn't github.com (GitHub Enterprise), so gh asks the
/// right server. None for github.com, for unknown shapes, and for hosts that clearly aren't GitHub.
pub fn enterprise_host(remote: &str) -> Option<String> {
    let rest = remote.split_once("://").map(|(_, r)| r).unwrap_or(remote);
    let rest = rest.rsplit_once('@').map(|(_, r)| r).unwrap_or(rest);
    let host = rest.split(['/', ':']).next()?.to_lowercase();
    if host.is_empty() || host == "github.com" || host == "www.github.com" {
        return None;
    }
    Some(host)
}

/// Remotes we know aren't GitHub: skip them without spawning gh at all.
pub fn clearly_not_github(remote: &str) -> bool {
    let r = remote.to_lowercase();
    ["gitlab.", "bitbucket.", "codeberg.org", "dev.azure.com", "visualstudio.com", "sr.ht", "gitea."].iter().any(|h| r.contains(h))
}

/// One GraphQL PR node in the shape `gh pr list --json` uses, which the UI and autopilot read.
pub fn normalize(node: &Value) -> Value {
    let rollup: Vec<Value> = node["commits"]["nodes"]
        .get(0)
        .and_then(|c| c["commit"]["statusCheckRollup"]["contexts"]["nodes"].as_array())
        .into_iter()
        .flatten()
        .map(|c| match c["__typename"].as_str() {
            Some("StatusContext") => json!({"__typename": "StatusContext", "context": c["context"], "state": c["state"], "targetUrl": c["targetUrl"]}),
            _ => json!({"__typename": "CheckRun", "name": c["name"], "status": c["status"], "conclusion": c["conclusion"], "detailsUrl": c["detailsUrl"]}),
        })
        .collect();
    let reviews: Vec<Value> = node["latestReviews"]["nodes"].as_array().into_iter().flatten()
        .map(|r| json!({"author": {"login": r["author"]["login"].as_str().unwrap_or("ghost")}, "state": r["state"]}))
        .collect();
    let requests: Vec<Value> = node["reviewRequests"]["nodes"].as_array().into_iter().flatten()
        .filter_map(|r| {
            let who = &r["requestedReviewer"];
            if let Some(l) = who["login"].as_str() { Some(json!({"login": l})) } else { who["name"].as_str().map(|n| json!({"name": n})) }
        })
        .collect();
    json!({
        "number": node["number"], "url": node["url"], "state": node["state"], "title": node["title"],
        "isDraft": node["isDraft"].as_bool().unwrap_or(false), "reviewDecision": node["reviewDecision"].as_str().unwrap_or(""),
        "mergeable": node["mergeable"], "headRefOid": node["headRefOid"], "baseRefName": node["baseRefName"],
        "additions": node["additions"].as_u64().unwrap_or(0), "deletions": node["deletions"].as_u64().unwrap_or(0),
        // A deleted account comes back as null; GitHub shows those as "ghost".
        "author": {"login": node["author"]["login"].as_str().unwrap_or("ghost")},
        "commentCount": node["comments"]["totalCount"].as_u64().unwrap_or(0),
        "latestReviews": reviews, "reviewRequests": requests, "statusCheckRollup": rollup,
    })
}

/// What one GraphQL answer said.
#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    /// Branch to its newest PR (normalized), or Null when the branch has no PR.
    pub prs: HashMap<String, Value>,
    /// Branches GitHub couldn't answer for this time (partial errors). Keep what we had for them.
    pub missing: Vec<String>,
    /// GraphQL points left this hour, and how long until it resets.
    pub remaining: Option<u64>,
    pub reset_in: Option<Duration>,
}

pub(crate) fn reset_in(reset_at: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Duration> {
    let t = chrono::DateTime::parse_from_rfc3339(reset_at).ok()?;
    (t.with_timezone(&chrono::Utc) - now).to_std().ok()
}

pub(crate) fn rate_limited_text(s: &str) -> bool {
    let s = s.to_lowercase();
    s.contains("rate limit") || s.contains("rate_limited") || s.contains("abuse detection")
}

/// Read gh's output for one chunk. gh exits non-zero on GraphQL errors but still prints the body,
/// so the body is read first: a partial answer is still an answer.
pub fn parse_response(stdout: &str, branches: &[String], now: chrono::DateTime<chrono::Utc>) -> Result<Parsed, Problem> {
    let body: Value = serde_json::from_str(stdout.trim()).map_err(|_| Problem::Other("GitHub sent something unexpected".into()))?;
    let errors = body["errors"].as_array().cloned().unwrap_or_default();
    let rate = &body["data"]["rateLimit"];
    let reset = rate["resetAt"].as_str().and_then(|r| reset_in(r, now));
    if errors.iter().any(|e| e["type"] == "RATE_LIMITED" || rate_limited_text(e["message"].as_str().unwrap_or(""))) {
        return Err(Problem::RateLimited { reset_in: reset });
    }
    let repo = &body["data"]["repository"];
    if !repo.is_object() {
        if errors.iter().any(|e| e["type"] == "NOT_FOUND") {
            return Err(Problem::NotGithub);
        }
        let msg = errors.first().and_then(|e| e["message"].as_str()).unwrap_or("GitHub didn't return the repo");
        return Err(Problem::Other(msg.to_string()));
    }
    let mut out = Parsed { remaining: rate["remaining"].as_u64(), reset_in: reset, ..Default::default() };
    for (i, b) in branches.iter().enumerate() {
        match repo[format!("b{i}")]["nodes"].as_array() {
            Some(nodes) => { out.prs.insert(b.clone(), nodes.first().map(normalize).unwrap_or(Value::Null)); }
            None => out.missing.push(b.clone()),
        }
    }
    Ok(out)
}

/// Sort out a failed gh run from its exit code and stderr (stdout had no usable body).
pub fn classify_failure(code: Option<i32>, stderr: &str) -> Problem {
    let s = stderr.to_lowercase();
    if rate_limited_text(&s) {
        Problem::RateLimited { reset_in: None }
    } else if code == Some(4) || s.contains("gh auth login") || s.contains("not logged") || s.contains("authentication") || s.contains("bad credentials") {
        Problem::NotLoggedIn
    } else if s.contains("none of the git remotes") || s.contains("not a git repository") || s.contains("could not resolve to a repository") || s.contains("no git remotes") {
        Problem::NotGithub
    } else {
        Problem::Other(stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("gh failed").trim().to_string())
    }
}

/// Run gh once for one chunk.
pub fn fetch_chunk(gh: &str, dir: &str, branches: &[String], hostname: Option<&str>) -> Result<Parsed, Problem> {
    let out = match Command::new(gh).args(gh_args(branches, hostname)).current_dir(dir).stdin(Stdio::null()).output() {
        Ok(o) => o,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(Problem::NoGh),
        Err(err) => return Err(Problem::Other(err.to_string())),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stdout.trim_start().starts_with('{') {
        match parse_response(&stdout, branches, chrono::Utc::now()) {
            // A body we couldn't place: let stderr explain it.
            Err(Problem::Other(_)) if !out.status.success() => {}
            r => return r,
        }
    }
    Err(classify_failure(out.status.code(), &stderr))
}

/// All branches of one repo, `MAX_PER_QUERY` per gh call. Branch names are de-duplicated first.
/// Fails as a whole only if the first chunk fails; later chunk failures become `missing`.
pub fn fetch_repo(gh: &str, dir: &str, branches: &[String], hostname: Option<&str>) -> Result<Parsed, Problem> {
    let mut uniq: Vec<String> = vec![];
    for b in branches {
        if !uniq.contains(b) { uniq.push(b.clone()); }
    }
    let mut all = Parsed::default();
    for (n, chunk) in uniq.chunks(MAX_PER_QUERY).enumerate() {
        match fetch_chunk(gh, dir, chunk, hostname) {
            Ok(p) => {
                all.prs.extend(p.prs);
                all.missing.extend(p.missing);
                all.remaining = p.remaining.or(all.remaining);
                all.reset_in = p.reset_in.or(all.reset_in);
            }
            Err(e) if n == 0 => return Err(e),
            Err(_) => all.missing.extend(chunk.iter().cloned()),
        }
    }
    Ok(all)
}

const PENDING: [&str; 6] = ["", "PENDING", "QUEUED", "IN_PROGRESS", "EXPECTED", "WAITING"];

/// An open PR that is still moving on its own: checks running, or GitHub still working out
/// whether it merges cleanly (right after a push).
pub fn is_hot(pr: &Value) -> bool {
    if pr["state"] != "OPEN" {
        return false;
    }
    let result = |c: &Value| c["conclusion"].as_str().filter(|s| !s.is_empty()).or(c["state"].as_str()).or(c["status"].as_str()).unwrap_or("").to_uppercase();
    pr["mergeable"] == "UNKNOWN" || pr["statusCheckRollup"].as_array().into_iter().flatten().any(|c| PENDING.contains(&result(c).as_str()))
}

/// How long until a repo's next poll. `hot`: checks running or autopilot on. `failures`: polls
/// in a row that failed (0 after a good one), doubling the wait up to `MAX_BACKOFF`.
pub fn next_delay(hot: bool, failures: u32, problem: Option<&Problem>, low_rate_reset: Option<Duration>) -> Duration {
    let base = if hot { HOT } else { IDLE };
    let backoff = |d: Duration| d.saturating_mul(1u32 << failures.min(10)).min(MAX_BACKOFF);
    match problem {
        None => match low_rate_reset {
            // Nearly out of points: idle pace, or wait for the reset if that is sooner than a fail.
            Some(r) => IDLE.max(r.min(MAX_BACKOFF)),
            None => base,
        },
        // Waiting fixes a rate limit: until it resets (plus a little), never sooner than the backoff.
        Some(Problem::RateLimited { reset_in }) => {
            let b = backoff(IDLE.max(base));
            reset_in.map(|r| (r + Duration::from_secs(5)).min(MAX_BACKOFF).max(b.min(MAX_BACKOFF))).unwrap_or(b)
        }
        Some(Problem::NoGh) | Some(Problem::NotGithub) => MAX_BACKOFF,
        Some(Problem::NotLoggedIn) => IDLE,
        Some(Problem::Other(_)) => backoff(base.max(Duration::from_secs(30))),
    }
}

/// When each repo is due, and why.
#[derive(Debug, Default, Clone)]
pub struct RepoClock {
    pub next: Option<Instant>,
    pub last_try: Option<Instant>,
    pub failures: u32,
    pub problem: Option<Problem>,
}

/// The poller's timetable. Pure: the loop and the tests drive it with their own clock.
#[derive(Debug)]
pub struct Schedule {
    pub repos: HashMap<String, RepoClock>,
    /// The window is on screen. While hidden, only repos with autopilot on are polled.
    pub visible: bool,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule { repos: HashMap::new(), visible: true }
    }
}

impl Schedule {
    /// Repos to poll now. `repos` is (repo id, autopilot on for one of its workspaces).
    pub fn due(&self, now: Instant, repos: &[(String, bool)]) -> Vec<String> {
        repos
            .iter()
            .filter(|(id, autopilot)| (self.visible || *autopilot) && self.repos.get(id).and_then(|c| c.next).is_none_or(|n| n <= now))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// How long the loop may sleep before something is due (a wake-up cuts it short).
    pub fn wait(&self, now: Instant, repos: &[(String, bool)]) -> Duration {
        repos
            .iter()
            .filter(|(_, autopilot)| self.visible || *autopilot)
            .map(|(id, _)| self.repos.get(id).and_then(|c| c.next).map_or(Duration::ZERO, |n| n.saturating_duration_since(now)))
            .min()
            .unwrap_or(IDLE)
            .min(IDLE)
    }

    /// A poll finished.
    pub fn record(&mut self, id: &str, now: Instant, hot: bool, result: Result<Option<Duration>, Problem>) {
        let c = self.repos.entry(id.to_string()).or_default();
        c.last_try = Some(now);
        match result {
            Ok(low_rate_reset) => {
                c.failures = 0;
                c.problem = None;
                c.next = Some(now + next_delay(hot, 0, None, low_rate_reset));
            }
            Err(p) => {
                c.next = Some(now + next_delay(hot, c.failures, Some(&p), None));
                c.failures = c.failures.saturating_add(1);
                c.problem = Some(p);
            }
        }
    }

    /// Ask for a poll now. Skipped if the last try was under `min_age` ago (so focus and switching
    /// workspaces can't hammer GitHub), or, unless `force`, if the problem won't fix itself.
    /// Returns whether the repo is now due.
    pub fn poke(&mut self, id: &str, now: Instant, min_age: Duration, force: bool) -> bool {
        let c = self.repos.entry(id.to_string()).or_default();
        if c.last_try.is_some_and(|t| now.saturating_duration_since(t) < min_age) {
            return false;
        }
        if !force && c.problem.as_ref().is_some_and(|p| p.is_sticky() || matches!(p, Problem::RateLimited { .. })) {
            return false;
        }
        c.next = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-11-06T13:30:00Z").unwrap().with_timezone(&chrono::Utc)
    }

    fn b(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    // Trimmed from a real answer for cli/cli (read-only query, 2026-11-06).
    const FIXTURE: &str = include_str!("../tests/fixtures/pr_graphql.json");

    #[test]
    fn query_has_one_alias_and_variable_per_branch() {
        let q = build_query(3);
        assert!(q.contains("$b0: String!, $b1: String!, $b2: String!"));
        assert!(q.contains("b2: pullRequests(headRefName: $b2, first: 1"));
        assert!(!q.contains("$b3"));
        assert!(q.contains("rateLimit { remaining resetAt }"));
        assert!(q.contains("fragment pr on PullRequest"));
        // Balanced braces, or GitHub rejects the whole thing.
        assert_eq!(q.matches('{').count(), q.matches('}').count());
    }

    #[test]
    fn branch_names_never_enter_the_query_text() {
        let evil = b(&["x\") { evil }", "@etc/passwd", "123"]);
        let a = gh_args(&evil, None);
        let q = a.iter().find(|s| s.starts_with("query=")).unwrap();
        assert!(!q.contains("evil") && !q.contains("passwd"));
        // Raw -f for every branch, so gh never reads "@..." as a file or "123" as a number.
        assert_eq!(a.iter().position(|s| s == "b1=@etc/passwd").map(|i| a[i - 1].as_str()), Some("-f"));
        assert_eq!(a.iter().position(|s| s == "b2=123").map(|i| a[i - 1].as_str()), Some("-f"));
        assert!(a.windows(2).any(|w| w[0] == "-F" && w[1] == "owner={owner}"));
        assert!(!a.contains(&"--hostname".to_string()));
        assert_eq!(&gh_args(&b(&["a"]), Some("github.acme.com"))[2..4], &["--hostname".to_string(), "github.acme.com".to_string()]);
    }

    #[test]
    fn enterprise_hosts_only_for_non_github_com() {
        assert_eq!(enterprise_host("git@github.com:me/x.git"), None);
        assert_eq!(enterprise_host("https://github.com/me/x"), None);
        assert_eq!(enterprise_host("ssh://git@github.acme.com/me/x.git"), Some("github.acme.com".into()));
        assert_eq!(enterprise_host("git@github.acme.com:me/x.git"), Some("github.acme.com".into()));
        assert!(clearly_not_github("git@gitlab.com:me/x.git"));
        assert!(!clearly_not_github("git@github.com:me/x.git"));
    }

    #[test]
    fn parses_a_real_answer_into_gh_pr_list_shape() {
        let p = parse_response(FIXTURE, &b(&["bagtoad/triage-ghmock-skill", "williammartin-multiaccount-draft-08-evaluation", "no-pr-here"]), now()).unwrap();
        assert_eq!(p.remaining, Some(4988));
        assert_eq!(p.reset_in, Some(Duration::from_secs(37 * 60 + 6)));
        assert!(p.missing.is_empty());
        let draft = &p.prs["bagtoad/triage-ghmock-skill"];
        assert_eq!(draft["number"], 14960);
        assert_eq!(draft["isDraft"], true);
        assert_eq!(draft["author"]["login"], "BagToad");
        assert_eq!(draft["baseRefName"], "bagtoad/triage-workflow-updates");
        assert_eq!(draft["latestReviews"][0]["state"], "COMMENTED");
        assert_eq!(draft["commentCount"], 0);
        assert_eq!(draft["statusCheckRollup"][0]["name"], "build (ubuntu-latest)");
        let failing = &p.prs["williammartin-multiaccount-draft-08-evaluation"];
        assert!(failing["statusCheckRollup"].as_array().unwrap().iter().any(|c| c["conclusion"] == "FAILURE"));
        assert!(failing["statusCheckRollup"].as_array().unwrap().iter().any(|c| c["__typename"] == "StatusContext" && c["context"] == "ci/legacy"));
        assert_eq!(failing["reviewRequests"], json!([{"login": "octocat"}, {"name": "core"}]));
        assert_eq!(p.prs["no-pr-here"], Value::Null);
    }

    #[test]
    fn partial_errors_keep_the_branches_that_answered() {
        let body = json!({
            "data": {"rateLimit": {"remaining": 4000, "resetAt": "2026-11-06T14:00:00Z"},
                     "repository": {"b0": {"nodes": []}, "b1": null}},
            "errors": [{"type": "INTERNAL", "path": ["repository", "b1"], "message": "Something went wrong"}],
        });
        let p = parse_response(&body.to_string(), &b(&["a", "bee"]), now()).unwrap();
        assert_eq!(p.prs.get("a"), Some(&Value::Null));
        assert_eq!(p.missing, vec!["bee".to_string()]);
    }

    #[test]
    fn errors_are_sorted_into_what_the_user_can_do() {
        let nf = json!({"data": {"rateLimit": {"remaining": 1, "resetAt": "2026-11-06T14:07:06Z"}, "repository": null},
                        "errors": [{"type": "NOT_FOUND", "path": ["repository"], "message": "Could not resolve to a Repository"}]});
        assert_eq!(parse_response(&nf.to_string(), &b(&["a"]), now()), Err(Problem::NotGithub));
        let rl = json!({"errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}],
                        "data": {"rateLimit": {"remaining": 0, "resetAt": "2026-11-06T13:40:00Z"}}});
        assert_eq!(parse_response(&rl.to_string(), &b(&["a"]), now()), Err(Problem::RateLimited { reset_in: Some(Duration::from_secs(600)) }));
        assert!(matches!(parse_response("not json", &b(&["a"]), now()), Err(Problem::Other(_))));

        assert_eq!(classify_failure(Some(4), "To get started with GitHub CLI, please run:  gh auth login"), Problem::NotLoggedIn);
        assert_eq!(classify_failure(Some(1), "Error parsing \"owner\" value: failed to run git: fatal: not a git repository"), Problem::NotGithub);
        assert_eq!(classify_failure(Some(1), "none of the git remotes configured for this repository point to a known GitHub host"), Problem::NotGithub);
        assert_eq!(classify_failure(Some(1), "HTTP 403: You have exceeded a secondary rate limit"), Problem::RateLimited { reset_in: None });
        assert_eq!(classify_failure(Some(1), "\nerror connecting to api.github.com\n"), Problem::Other("error connecting to api.github.com".into()));
    }

    #[test]
    fn hot_means_checks_running_or_mergeability_unknown() {
        let pr = |checks: Value, mergeable: &str, state: &str| json!({"state": state, "mergeable": mergeable, "statusCheckRollup": checks});
        assert!(is_hot(&pr(json!([{"status": "IN_PROGRESS", "conclusion": null}]), "MERGEABLE", "OPEN")));
        assert!(is_hot(&pr(json!([{"state": "PENDING"}]), "MERGEABLE", "OPEN")));
        assert!(is_hot(&pr(json!([]), "UNKNOWN", "OPEN")));
        assert!(!is_hot(&pr(json!([{"status": "COMPLETED", "conclusion": "FAILURE"}]), "MERGEABLE", "OPEN")));
        assert!(!is_hot(&pr(json!([{"status": "IN_PROGRESS"}]), "UNKNOWN", "MERGED")));
        assert!(!is_hot(&Value::Null));
    }

    #[test]
    fn cadence_is_20s_hot_5min_idle() {
        assert_eq!(next_delay(true, 0, None, None), HOT);
        assert_eq!(next_delay(false, 0, None, None), IDLE);
        // Nearly out of points: no fast polling, wait for the reset (capped).
        assert_eq!(next_delay(true, 0, None, Some(Duration::from_secs(60))), IDLE);
        assert_eq!(next_delay(true, 0, None, Some(Duration::from_secs(900))), Duration::from_secs(900));
    }

    #[test]
    fn back_off_doubles_and_caps() {
        let other = Problem::Other("boom".into());
        assert_eq!(next_delay(true, 0, Some(&other), None), Duration::from_secs(30));
        assert_eq!(next_delay(true, 1, Some(&other), None), Duration::from_secs(60));
        assert_eq!(next_delay(false, 1, Some(&other), None), Duration::from_secs(600));
        assert_eq!(next_delay(false, 9, Some(&other), None), MAX_BACKOFF);
        let rl = |s| Problem::RateLimited { reset_in: Some(Duration::from_secs(s)) };
        assert_eq!(next_delay(true, 0, Some(&rl(1200)), None), Duration::from_secs(1205));
        assert_eq!(next_delay(true, 0, Some(&rl(10)), None), IDLE);
        assert_eq!(next_delay(true, 2, Some(&Problem::RateLimited { reset_in: None }), None), Duration::from_secs(1200));
        assert_eq!(next_delay(true, 0, Some(&Problem::NoGh), None), MAX_BACKOFF);
        assert_eq!(next_delay(true, 0, Some(&Problem::NotLoggedIn), None), IDLE);
    }

    #[test]
    fn schedule_pauses_while_hidden_except_autopilot() {
        let t0 = Instant::now();
        let mut s = Schedule::default();
        let repos = vec![("r1".to_string(), false), ("r2".to_string(), true)];
        assert_eq!(s.due(t0, &repos), vec!["r1", "r2"]);
        s.record("r1", t0, false, Ok(None));
        s.record("r2", t0, true, Ok(None));
        assert!(s.due(t0 + Duration::from_secs(19), &repos).is_empty());
        assert_eq!(s.wait(t0, &repos), HOT);
        assert_eq!(s.due(t0 + HOT, &repos), vec!["r2"]);
        s.visible = false;
        assert_eq!(s.due(t0 + IDLE, &repos), vec!["r2"]);
        s.visible = true;
        assert_eq!(s.due(t0 + IDLE, &repos), vec!["r1", "r2"]);
    }

    #[test]
    fn pokes_are_debounced_and_skip_problems_waiting_cannot_fix() {
        let t0 = Instant::now();
        let mut s = Schedule::default();
        s.record("r", t0, false, Ok(None));
        assert!(!s.poke("r", t0 + Duration::from_secs(5), Duration::from_secs(15), false));
        assert!(s.poke("r", t0 + Duration::from_secs(16), Duration::from_secs(15), false));
        assert_eq!(s.due(t0 + Duration::from_secs(16), &[("r".into(), false)]), vec!["r"]);
        s.record("r", t0, false, Err(Problem::NotGithub));
        assert!(!s.poke("r", t0 + Duration::from_secs(60), Duration::from_secs(15), false));
        assert!(s.poke("r", t0 + Duration::from_secs(60), Duration::from_secs(2), true));
        s.record("r", t0, false, Err(Problem::RateLimited { reset_in: None }));
        assert!(!s.poke("r", t0 + Duration::from_secs(60), Duration::from_secs(15), false));
    }

    #[test]
    fn failures_count_up_then_reset_on_success() {
        let t0 = Instant::now();
        let mut s = Schedule::default();
        s.record("r", t0, false, Err(Problem::Other("x".into())));
        s.record("r", t0, false, Err(Problem::Other("x".into())));
        assert_eq!(s.repos["r"].failures, 2);
        assert_eq!(s.repos["r"].next, Some(t0 + Duration::from_secs(600)));
        s.record("r", t0, false, Ok(None));
        assert_eq!(s.repos["r"].failures, 0);
        assert_eq!(s.repos["r"].problem, None);
    }

    /// A fake `gh` that logs every run (folder and args) and answers like GitHub. Repos whose folder
    /// name has "hot" in it report a check still running; "nologin" acts like a signed-out gh.
    #[cfg(unix)]
    fn shim(dir: &std::path::Path) -> (String, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let log = dir.join("gh.log");
        let gh = dir.join("gh");
        let script = format!(r#"#!/bin/sh
echo "$(basename "$PWD") $1 $2" >> "{log}"
case "$PWD" in *nologin*) echo "To get started with GitHub CLI, please run:  gh auth login" >&2; exit 4;; esac
node='[]'
case "$PWD" in *hot*) node='[{{"number":7,"url":"u","state":"OPEN","title":"t","isDraft":false,"reviewDecision":null,"mergeable":"MERGEABLE","headRefOid":"abc","baseRefName":"main","additions":1,"deletions":0,"author":{{"login":"me"}},"comments":{{"totalCount":0}},"latestReviews":{{"nodes":[]}},"reviewRequests":{{"nodes":[]}},"commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{{"contexts":{{"nodes":[{{"__typename":"CheckRun","name":"ci","status":"IN_PROGRESS","conclusion":null}}]}}}}}}}}]}}}}]';; esac
out=""
for a in "$@"; do
  case "$a" in b[0-9]*=*) k="${{a%%=*}}"; out="$out${{out:+,}}\"$k\":{{\"nodes\":$node}}";; esac
done
echo "{{\"data\":{{\"rateLimit\":{{\"remaining\":4000,\"resetAt\":\"2030-01-01T00:00:00Z\"}},\"repository\":{{$out}}}}}}"
"#, log = log.display());
        std::fs::write(&gh, script).unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        (gh.to_string_lossy().to_string(), log)
    }

    #[cfg(unix)]
    fn runs(log: &std::path::Path) -> usize {
        std::fs::read_to_string(log).map(|s| s.lines().count()).unwrap_or(0)
    }

    /// Read-only check of the exact query against GitHub with your real gh. Not run by default:
    /// `NEKOTRAIL_GH_REPO=<folder in a GitHub repo> NEKOTRAIL_GH_BRANCH=<branch with a PR> cargo test real_gh -- --ignored`
    #[test]
    #[ignore]
    fn real_gh_read_only() {
        let dir = std::env::var("NEKOTRAIL_GH_REPO").expect("NEKOTRAIL_GH_REPO");
        let branch = std::env::var("NEKOTRAIL_GH_BRANCH").expect("NEKOTRAIL_GH_BRANCH");
        let p = fetch_repo("gh", &dir, &[branch.clone(), "nekotrail/surely-no-such-branch".into()], None).unwrap();
        assert!(p.missing.is_empty());
        assert!(p.prs[&branch]["number"].is_u64(), "{:?}", p.prs[&branch]);
        assert_eq!(p.prs["nekotrail/surely-no-such-branch"], Value::Null);
        assert!(p.remaining.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn one_gh_process_per_repo_through_a_fake_gh() {
        let tmp = tempfile::tempdir().unwrap();
        let (gh, log) = shim(tmp.path());
        let repo = tmp.path().join("hot-repo");
        std::fs::create_dir(&repo).unwrap();
        let branches: Vec<String> = (0..5).map(|i| format!("me/task-{i}")).collect();
        let p = fetch_repo(&gh, repo.to_str().unwrap(), &branches, None).unwrap();
        assert_eq!(runs(&log), 1);
        assert_eq!(p.prs.len(), 5);
        assert!(is_hot(&p.prs["me/task-0"]));
        // 45 branches (two of them twice) take two runs: 40, then 3.
        let mut many: Vec<String> = (0..43).map(|i| format!("b/{i}")).collect();
        many.extend(["b/1".to_string(), "b/2".to_string()]);
        let p = fetch_repo(&gh, repo.to_str().unwrap(), &many, None).unwrap();
        assert_eq!(runs(&log), 3);
        assert_eq!(p.prs.len(), 43);
        // Signed out, and no gh at all.
        let nologin = tmp.path().join("nologin");
        std::fs::create_dir(&nologin).unwrap();
        assert_eq!(fetch_repo(&gh, nologin.to_str().unwrap(), &branches, None), Err(Problem::NotLoggedIn));
        assert_eq!(fetch_repo("/no/such/gh", repo.to_str().unwrap(), &branches, None), Err(Problem::NoGh));
    }

    /// The before/after count for the roadmap: 20 workspaces in 4 repos, 10 minutes, window visible,
    /// opening another workspace every 2 minutes. "After" really spawns the fake gh, on a simulated
    /// clock driven by `Schedule`. "Before" counts the 0.10.1 timers: PR bar, one `gh pr list` on
    /// open and every 60 s; sidebar, one per repo at start and every 5 min; autopilot, one per
    /// autopilot workspace every 90 s plus one when turned on.
    #[cfg(unix)]
    #[test]
    fn gh_runs_in_ten_minutes_before_and_after() {
        let tmp = tempfile::tempdir().unwrap();
        let (gh, log) = shim(tmp.path());
        let simulate = |names: [&str; 4], autopilot_repo: Option<usize>| -> usize {
            let start = runs(&log);
            let dirs: Vec<std::path::PathBuf> = names.iter().map(|n| { let d = tmp.path().join(n); std::fs::create_dir_all(&d).unwrap(); d }).collect();
            let branches: Vec<String> = (0..5).map(|i| format!("me/task-{i}")).collect();
            let keys: Vec<(String, bool)> = (0..4).map(|i| (i.to_string(), autopilot_repo == Some(i))).collect();
            let mut s = Schedule::default();
            let t0 = Instant::now();
            for sec in 0..600u64 {
                let now = t0 + Duration::from_secs(sec);
                // You open a workspace in the next repo every 2 minutes (the PR bar asks for a look).
                if sec > 0 && sec % 120 == 0 {
                    s.poke(&((sec / 120) % 4).to_string(), now, Duration::from_secs(15), false);
                }
                for id in s.due(now, &keys) {
                    let i: usize = id.parse().unwrap();
                    let p = fetch_repo(&gh, dirs[i].to_str().unwrap(), &branches, None).unwrap();
                    let hot = autopilot_repo == Some(i) || p.prs.values().any(is_hot);
                    s.record(&id, now, hot, Ok(None));
                }
            }
            runs(&log) - start
        };
        let before = |autopilot: bool| {
            let bar = 600 / 60; // reopening on a switch lines up with the 60 s timer
            let sidebar = 4 * 2; // t=0 and t=300
            let auto = if autopilot { 1 + 600 / 90 } else { 0 };
            bar + sidebar + auto
        };
        let idle = simulate(["a1", "a2", "a3", "a4"], None);
        let busy = simulate(["b1", "b2", "hot-b3", "b4"], Some(1));
        println!("gh runs in 10 min, 20 workspaces in 4 repos. Idle: before {} after {idle}. CI running in one repo and autopilot in another: before {} after {busy}.", before(false), before(true));
        assert_eq!(idle, 12, "4 at start, 4 at 5 min, 4 for the switches");
        assert!(idle < before(false));
        // Two repos at 20 s is 30 runs each, plus the idle two. More runs than before, but every run
        // refreshes all workspaces of its repo, where before only the open one was ever current.
        assert!(busy <= 30 + 30 + 2 * 3 + 4, "{busy}");
    }
}
