//! Reads the output of test, lint and typecheck commands into a list of failures with a file and
//! line where the tool gives one. Pure functions over text, so every format has a fixture test.
//!
//! Formats: JUnit XML, cargo test, rustc and clippy diagnostics, go test (and go build/vet
//! errors), pytest, jest, vitest, bun test, tsc and eslint (stylish). Several can match one
//! output (a `check` script that runs tsc then eslint); their results add up.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// One failing test or error.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Failure {
    /// The test's name, or the error itself for compilers and linters.
    pub title: String,
    /// Relative to the workspace when it is inside it.
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
    /// A few numbered lines of the file around `line`, filled in after parsing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Parsed {
    /// The formats that recognised something, e.g. ["cargo test"].
    pub formats: Vec<&'static str>,
    pub passed: Option<u32>,
    /// What the tools reported as failed (falls back to the number of failures found).
    pub failed: Option<u32>,
    pub failures: Vec<Failure>,
}

/// What a parser needs to know about where the command ran.
#[derive(Debug, Default, Clone, Copy)]
pub struct Ctx<'a> {
    /// The workspace folder: absolute paths inside it become relative.
    pub root: &'a str,
    /// The `module` line of go.mod, to turn a Go package into a folder.
    pub go_module: Option<&'a str>,
}

#[derive(Default)]
struct Part {
    passed: Option<u32>,
    failed: Option<u32>,
    failures: Vec<Failure>,
}

type ParseFn = fn(&[&str], &Ctx) -> Option<Part>;

const MAX_FAILURES: usize = 200;
const MAX_MESSAGE: usize = 2000;

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

re!(ANSI, r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07|\r");

/// Colour codes and carriage returns out, so the patterns below see plain lines.
pub fn strip_ansi(s: &str) -> String {
    ANSI.replace_all(s, "").into_owned()
}

/// Every format that recognises something in `output`, added up.
pub fn parse(output: &str, ctx: &Ctx) -> Parsed {
    let text = strip_ansi(output);
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Parsed::default();
    let parsers: [(&'static str, ParseFn); 10] = [
        ("cargo test", cargo_test),
        ("rustc", rustc),
        ("go test", go_test),
        ("pytest", pytest),
        ("jest", jest),
        ("vitest", vitest),
        ("bun test", bun_test),
        ("tsc", tsc),
        ("eslint", eslint),
        ("JUnit XML", junit_inline),
    ];
    for (name, f) in parsers {
        if let Some(p) = f(&lines, ctx) {
            add(&mut out, name, p, ctx);
        }
    }
    finish(out)
}

/// A JUnit XML report (a file the command wrote). None when it has no test cases.
pub fn parse_junit(xml: &str, ctx: &Ctx) -> Option<Parsed> {
    let p = junit(xml)?;
    let mut out = Parsed::default();
    add(&mut out, "JUnit XML", p, ctx);
    Some(finish(out))
}

fn add(out: &mut Parsed, name: &'static str, p: Part, ctx: &Ctx) {
    out.formats.push(name);
    if let Some(n) = p.passed {
        out.passed = Some(out.passed.unwrap_or(0) + n);
    }
    let failed = p.failed.unwrap_or(p.failures.len() as u32);
    out.failed = Some(out.failed.unwrap_or(0) + failed);
    for mut f in p.failures {
        f.file = f.file.map(|x| rel(&x, ctx.root));
        f.message = clip(f.message.trim_end());
        f.title = f.title.trim().to_string();
        out.failures.push(f);
    }
}

fn finish(mut out: Parsed) -> Parsed {
    let mut seen = std::collections::HashSet::new();
    out.failures.retain(|f| seen.insert((f.file.clone(), f.line, f.title.clone())));
    out.failures.truncate(MAX_FAILURES);
    out
}

fn clip(s: &str) -> String {
    if s.len() <= MAX_MESSAGE {
        return s.to_string();
    }
    let mut end = MAX_MESSAGE;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// A path as the user would write it: relative to the workspace when inside it, no leading `./`.
pub fn rel(path: &str, root: &str) -> String {
    let p = path.trim().trim_start_matches("file://");
    let root = root.trim_end_matches('/');
    let p = if !root.is_empty() && p.starts_with(root) && p[root.len()..].starts_with('/') { &p[root.len() + 1..] } else { p };
    p.trim_start_matches("./").to_string()
}

fn num(s: &str) -> Option<u32> {
    s.parse().ok()
}

/// `(\d+) word` anywhere in a summary line.
fn count(line: &str, words: &[&str]) -> Option<u32> {
    let mut total = None;
    for w in words {
        let re = Regex::new(&format!(r"(\d+) {w}\b")).ok()?;
        for c in re.captures_iter(line) {
            total = Some(total.unwrap_or(0) + num(&c[1]).unwrap_or(0));
        }
    }
    total
}

// ---------- cargo test ----------

re!(CARGO_RESULT, r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed;");
re!(CARGO_FAILED, r"^test (\S+) \.\.\. FAILED$");
re!(CARGO_SECTION, r"^---- (\S+) stdout ----$");
re!(CARGO_PANIC_NEW, r"panicked at (.+?):(\d+):\d+:$");
re!(CARGO_PANIC_OLD, r"panicked at '(.*)', (.+?):(\d+):\d+$");

fn cargo_test(lines: &[&str], _: &Ctx) -> Option<Part> {
    let mut part = Part::default();
    let mut seen = false;
    for l in lines {
        if let Some(c) = CARGO_RESULT.captures(l) {
            seen = true;
            part.passed = Some(part.passed.unwrap_or(0) + num(&c[1]).unwrap_or(0));
            part.failed = Some(part.failed.unwrap_or(0) + num(&c[2]).unwrap_or(0));
        }
    }
    let failed: Vec<&str> = lines.iter().filter_map(|l| CARGO_FAILED.captures(l).map(|c| c.get(1).unwrap().as_str())).collect();
    if !seen && failed.is_empty() {
        return None;
    }
    for name in failed {
        let mut f = Failure { title: name.to_string(), ..Default::default() };
        if let Some(start) = lines.iter().position(|l| CARGO_SECTION.captures(l).is_some_and(|c| &c[1] == name)) {
            let body: Vec<&str> = lines[start + 1..].iter().take_while(|l| !l.starts_with("---- ") && **l != "failures:").copied().collect();
            let mut msg = vec![];
            let mut after_panic = false;
            for l in &body {
                if let Some(c) = CARGO_PANIC_NEW.captures(l) {
                    f.file = Some(c[1].to_string());
                    f.line = num(&c[2]);
                    after_panic = true;
                    continue;
                }
                if let Some(c) = CARGO_PANIC_OLD.captures(l) {
                    msg.push(c[1].to_string());
                    f.file = Some(c[2].to_string());
                    f.line = num(&c[3]);
                    continue;
                }
                if l.starts_with("note: run with `RUST_BACKTRACE") {
                    break;
                }
                if after_panic || f.file.is_none() {
                    msg.push(l.to_string());
                }
            }
            f.message = msg.join("\n").trim().to_string();
        }
        part.failures.push(f);
    }
    Some(part)
}

// ---------- rustc and clippy diagnostics ----------

re!(RUSTC_HEAD, r"^error(?:\[(E\d+)\])?: (.+)$");
re!(RUSTC_AT, r"^\s*--> (.+?):(\d+):\d+$");

fn rustc(lines: &[&str], _: &Ctx) -> Option<Part> {
    let mut part = Part::default();
    for (i, l) in lines.iter().enumerate() {
        let Some(c) = RUSTC_HEAD.captures(l) else { continue };
        let title = c[2].to_string();
        if title.starts_with("could not compile") || title.starts_with("aborting due to") || title.starts_with("test failed") {
            continue;
        }
        let Some((j, at)) = lines.iter().enumerate().skip(i + 1).take(3).find_map(|(j, x)| RUSTC_AT.captures(x).map(|a| (j, a))) else { continue };
        let notes: Vec<String> = lines[j + 1..]
            .iter()
            .take_while(|x| !x.trim().is_empty())
            .filter_map(|x| x.trim_start().strip_prefix("= ").map(String::from))
            .collect();
        let code = c.get(1).map(|m| format!(" [{}]", m.as_str())).unwrap_or_default();
        part.failures.push(Failure {
            title: title.clone(),
            file: Some(at[1].to_string()),
            line: num(&at[2]),
            message: std::iter::once(format!("{title}{code}")).chain(notes).collect::<Vec<_>>().join("\n"),
            snippet: None,
        });
    }
    (!part.failures.is_empty()).then_some(part)
}

// ---------- go test, go build, go vet ----------

re!(GO_FAIL, r"^(\s*)--- FAIL: (\S+)");
re!(GO_PASS, r"^\s*--- PASS: ");
re!(GO_LOG, r"^\s+([\w.\-]+\.go):(\d+): ?(.*)$");
re!(GO_PKG, r"^(?:FAIL|ok)\s+(\S+)(?:\s|$)");
re!(GO_BUILD, r"^([\w./\-]+\.go):(\d+):(?:\d+:)? (.+)$");
re!(GO_FRAME, r"^\t(/\S+\.go):(\d+)");

fn go_test(lines: &[&str], ctx: &Ctx) -> Option<Part> {
    let mut part = Part::default();
    let mut passed = 0u32;
    let mut seen = false;
    // Failures waiting for their package line (`FAIL\tpkg`), which comes after them.
    let mut waiting: Vec<usize> = vec![];
    let mut cur: Option<(usize, usize)> = None; // (failure index, indent of its --- FAIL line)
    for l in lines {
        if let Some(c) = GO_FAIL.captures(l) {
            seen = true;
            part.failures.push(Failure { title: c[2].to_string(), ..Default::default() });
            let i = part.failures.len() - 1;
            waiting.push(i);
            cur = Some((i, c[1].len()));
            continue;
        }
        if GO_PASS.is_match(l) {
            seen = true;
            passed += 1;
            cur = None;
            continue;
        }
        if let Some(c) = GO_PKG.captures(l) {
            seen = true;
            let dir = go_dir(&c[1], ctx.go_module);
            for i in waiting.drain(..) {
                let f = &mut part.failures[i];
                if let (Some(file), Some(d)) = (&f.file, &dir)
                    && !file.contains('/') && !d.is_empty() {
                        f.file = Some(format!("{d}/{file}"));
                    }
            }
            cur = None;
            continue;
        }
        if l.starts_with("# ") || l.starts_with("=== ") || *l == "FAIL" || *l == "PASS" {
            cur = None;
            continue;
        }
        if let Some(c) = GO_BUILD.captures(l) {
            seen = true;
            part.failures.push(Failure { title: c[3].to_string(), file: Some(c[1].to_string()), line: num(&c[2]), message: c[3].to_string(), snippet: None });
            cur = None;
            continue;
        }
        let Some((i, indent)) = cur else { continue };
        let f = &mut part.failures[i];
        if let Some(rest) = l.strip_prefix("panic: ") {
            f.message.push_str(&format!("panic: {rest}\n"));
            continue;
        }
        if let Some(c) = GO_FRAME.captures(l) {
            if f.file.is_none() && !ctx.root.is_empty() && c[1].starts_with(ctx.root) {
                f.file = Some(c[1].to_string());
                f.line = num(&c[2]);
            }
            continue;
        }
        let lead = l.len() - l.trim_start().len();
        if lead <= indent {
            continue;
        }
        if let Some(c) = GO_LOG.captures(l) {
            if f.file.is_none() {
                f.file = Some(c[1].to_string());
                f.line = num(&c[2]);
            }
            f.message.push_str(&c[3]);
            f.message.push('\n');
        } else if !f.message.is_empty() {
            f.message.push_str(l.trim());
            f.message.push('\n');
        }
    }
    if !seen {
        return None;
    }
    // A parent test that only failed because a subtest did says nothing of its own.
    let names: Vec<String> = part.failures.iter().map(|f| f.title.clone()).collect();
    part.failures.retain(|f| !(f.message.trim().is_empty() && names.iter().any(|n| n.starts_with(&format!("{}/", f.title)))));
    part.passed = (passed > 0).then_some(passed);
    Some(part)
}

/// `pawprint/internal/tickets` with module `pawprint` is the folder `internal/tickets`.
fn go_dir(pkg: &str, module: Option<&str>) -> Option<String> {
    let m = module?;
    if pkg == m {
        return Some(String::new());
    }
    pkg.strip_prefix(m).and_then(|r| r.strip_prefix('/')).map(String::from)
}

// ---------- pytest ----------

re!(PY_SUMMARY, r"^=+ (.*\b(?:passed|failed|errors?|skipped)\b.*) in [\d.]+s(?: \([^)]*\))? =+$");
re!(PY_SHORT, r"^(FAILED|ERROR) (\S+)(?: - (.*))?$");
re!(PY_HEADER, r"^_{3,} (.+?) _{3,}$");
re!(PY_LOC, r"^([\w./\-]+\.py):(\d+): (.*)$");

fn pytest(lines: &[&str], _: &Ctx) -> Option<Part> {
    let mut part = Part::default();
    let mut seen = false;
    for l in lines {
        if let Some(c) = PY_SUMMARY.captures(l) {
            seen = true;
            part.passed = count(&c[1], &["passed"]);
            part.failed = count(&c[1], &["failed", "error", "errors"]);
        }
    }
    // Sections under FAILURES / ERRORS, by their header.
    let mut sections: Vec<(String, Vec<&str>)> = vec![];
    let mut on = false;
    for l in lines {
        if l.starts_with('=') {
            on = l.contains(" FAILURES ") || l.contains(" ERRORS ");
            continue;
        }
        if !on {
            continue;
        }
        if let Some(c) = PY_HEADER.captures(l) {
            sections.push((c[1].to_string(), vec![]));
        } else if let Some(s) = sections.last_mut() {
            s.1.push(l);
        }
    }
    let from_section = |body: &[&str]| -> (Option<String>, Option<u32>, String) {
        let msg: Vec<&str> = body.iter().filter_map(|l| l.strip_prefix('E').filter(|r| r.starts_with(' ')).map(str::trim)).collect();
        let loc = body.iter().rev().find_map(|l| PY_LOC.captures(l));
        (loc.as_ref().map(|c| c[1].to_string()), loc.as_ref().and_then(|c| num(&c[2])), msg.join("\n"))
    };
    let short: Vec<_> = lines.iter().filter_map(|l| PY_SHORT.captures(l)).collect();
    if !short.is_empty() {
        seen = true;
        for c in short {
            let id = c[2].to_string();
            let (file, test) = id.split_once("::").map(|(f, t)| (f.to_string(), t.to_string())).unwrap_or((id.clone(), String::new()));
            let header = test.replace("::", ".");
            let sec = sections.iter().find(|(h, _)| !header.is_empty() && *h == header).or_else(|| sections.iter().find(|(h, _)| test.is_empty() && h.contains(&file)));
            let mut f = Failure { title: if test.is_empty() { id.clone() } else { test.clone() }, file: Some(file.clone()), message: c.get(3).map(|m| m.as_str().to_string()).unwrap_or_default(), ..Default::default() };
            if let Some((_, body)) = sec {
                // The traceback's last line: where it was raised, which may be outside the test file.
                let (lf, line, msg) = from_section(body);
                if lf.is_some() {
                    f.file = lf;
                    f.line = line;
                }
                if !msg.is_empty() {
                    f.message = msg;
                }
            }
            part.failures.push(f);
        }
    } else if !sections.is_empty() {
        seen = true;
        for (h, body) in &sections {
            let (file, line, message) = from_section(body);
            part.failures.push(Failure { title: h.clone(), file, line, message, snippet: None });
        }
    }
    seen.then_some(part)
}

// ---------- jest ----------

re!(JEST_SUMMARY, r"^Tests:\s+(.+)$");
re!(JEST_FILE, r"^\s*FAIL\s+(\S+)(?:\s+\([^)]*\))?\s*$");
re!(JEST_BULLET, r"^\s+● (.+)$");
re!(STACK_AT, r"^\s+at (?:.*? \()?([^()\s]+?):(\d+):\d+\)?$");
re!(CODE_FRAME, r"^\s*>?\s*\d+ \|");

fn jest(lines: &[&str], _: &Ctx) -> Option<Part> {
    let mut part = Part::default();
    let mut seen = false;
    for l in lines {
        if let Some(c) = JEST_SUMMARY.captures(l) {
            seen = true;
            part.passed = count(&c[1], &["passed"]);
            part.failed = count(&c[1], &["failed"]);
        }
    }
    let mut file: Option<String> = None;
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if let Some(c) = JEST_FILE.captures(l) {
            file = Some(c[1].to_string());
        }
        let Some(c) = JEST_BULLET.captures(l) else { i += 1; continue };
        let title = c[1].to_string();
        i += 1;
        if title.starts_with("Console") {
            continue;
        }
        seen = true;
        let mut msg = vec![];
        let mut loc: Option<(String, u32)> = None;
        let mut in_frame = false;
        while i < lines.len() {
            let x = lines[i];
            if JEST_BULLET.is_match(x) || JEST_FILE.is_match(x) || x.starts_with("Test Suites:") || x.starts_with(" PASS ") {
                break;
            }
            if let Some(a) = STACK_AT.captures(x) {
                if loc.is_none() && !a[1].contains("node_modules") && !a[1].starts_with("node:") {
                    loc = Some((a[1].to_string(), num(&a[2]).unwrap_or(0)));
                }
                in_frame = true;
            } else if CODE_FRAME.is_match(x) || x.trim_start().starts_with('|') {
                in_frame = true;
            } else if !in_frame && !x.trim().is_empty() {
                msg.push(x.trim().to_string());
            }
            i += 1;
        }
        let (f, line) = match loc {
            Some((f, n)) => (Some(f), Some(n)),
            None => (file.clone(), None),
        };
        part.failures.push(Failure { title, file: f, line, message: msg.join("\n"), snippet: None });
    }
    seen.then_some(part)
}

// ---------- vitest ----------

re!(VITEST_SUMMARY, r"^\s*Tests\s+(.+?)\s*\(\d+\)\s*$");
re!(VITEST_FAIL, r"^\s*FAIL\s+(\S+) > (.+?)\s*$");
re!(VITEST_AT, r"❯ (\S+?):(\d+):\d+");

fn vitest(lines: &[&str], _: &Ctx) -> Option<Part> {
    let mut part = Part::default();
    let mut seen = false;
    for l in lines {
        if let Some(c) = VITEST_SUMMARY.captures(l) {
            seen = true;
            part.passed = count(&c[1], &["passed"]);
            part.failed = count(&c[1], &["failed"]);
        }
    }
    let mut i = 0;
    while i < lines.len() {
        let Some(c) = VITEST_FAIL.captures(lines[i]) else { i += 1; continue };
        seen = true;
        let mut f = Failure { title: c[2].to_string(), file: Some(c[1].to_string()), ..Default::default() };
        i += 1;
        let mut msg = vec![];
        let mut para_done = false;
        while i < lines.len() {
            let x = lines[i];
            if VITEST_FAIL.is_match(x) || x.trim_start().starts_with('⎯') {
                break;
            }
            if let Some(a) = VITEST_AT.captures(x) {
                if f.line.is_none() {
                    f.file = Some(a[1].to_string());
                    f.line = num(&a[2]);
                }
                para_done = true;
            } else if x.trim().is_empty() {
                para_done = para_done || !msg.is_empty();
            } else if !para_done {
                msg.push(x.trim().to_string());
            }
            i += 1;
        }
        f.message = msg.join("\n");
        part.failures.push(f);
    }
    seen.then_some(part)
}

// ---------- bun test ----------

re!(BUN_PASS, r"^\s*(\d+) pass\s*$");
re!(BUN_FAIL, r"^\s*(\d+) fail\s*$");
re!(BUN_FAILED, r"^\(fail\) (.+?)(?: \[[\d.]+m?s\])?$");
re!(BUN_FILE, r"^(\S+\.(?:[cm]?[jt]sx?)):$");

fn bun_test(lines: &[&str], _: &Ctx) -> Option<Part> {
    let mut part = Part::default();
    let mut seen = false;
    for l in lines {
        if let Some(c) = BUN_PASS.captures(l) {
            seen = true;
            part.passed = num(&c[1]);
        }
        if let Some(c) = BUN_FAIL.captures(l) {
            seen = true;
            part.failed = num(&c[1]);
        }
    }
    let mut file: Option<String> = None;
    let mut msg: Vec<String> = vec![];
    let mut loc: Option<(String, u32)> = None;
    let mut in_error = false;
    for l in lines {
        if let Some(c) = BUN_FILE.captures(l) {
            file = Some(c[1].to_string());
            (msg, loc, in_error) = (vec![], None, false);
        } else if l.starts_with("(pass)") || l.starts_with("(skip)") || l.starts_with("(todo)") {
            (msg, loc, in_error) = (vec![], None, false);
        } else if let Some(c) = BUN_FAILED.captures(l) {
            seen = true;
            let (f, line) = match loc.take() {
                Some((f, n)) => (Some(f), Some(n)),
                None => (file.clone(), None),
            };
            part.failures.push(Failure { title: c[1].to_string(), file: f, line, message: msg.join("\n"), snippet: None });
            (msg, in_error) = (vec![], false);
        } else if let Some(rest) = l.strip_prefix("error: ") {
            in_error = true;
            msg = vec![rest.to_string()];
        } else if let Some(a) = STACK_AT.captures(l) {
            if loc.is_none() && !a[1].contains("node_modules") {
                loc = Some((a[1].to_string(), num(&a[2]).unwrap_or(0)));
            }
            in_error = false;
        } else if in_error && !l.trim().is_empty() {
            msg.push(l.trim().to_string());
        }
    }
    seen.then_some(part)
}

// ---------- tsc ----------

re!(TSC_PLAIN, r"^(.+?)\((\d+),\d+\): error (TS\d+): (.+)$");
re!(TSC_PRETTY, r"^(.+?):(\d+):\d+ - error (TS\d+): (.+)$");
re!(TSC_FOUND, r"^Found (\d+) errors?\b");

fn tsc(lines: &[&str], _: &Ctx) -> Option<Part> {
    let mut part = Part::default();
    let mut last_plain = false;
    for l in lines {
        if let Some(c) = TSC_PLAIN.captures(l).or_else(|| TSC_PRETTY.captures(l)) {
            last_plain = TSC_PLAIN.is_match(l);
            part.failures.push(Failure { title: c[4].to_string(), file: Some(c[1].trim().to_string()), line: num(&c[2]), message: format!("{} ({})", &c[4], &c[3]), snippet: None });
            continue;
        }
        if let Some(c) = TSC_FOUND.captures(l) {
            part.failed = num(&c[1]);
        }
        // Plain output continues a message on indented lines right under it.
        if last_plain && l.starts_with("  ") && !l.trim().is_empty() {
            if let Some(f) = part.failures.last_mut() {
                f.message.push('\n');
                f.message.push_str(l.trim());
            }
            continue;
        }
        last_plain = false;
    }
    (!part.failures.is_empty()).then_some(part)
}

// ---------- eslint (stylish) ----------

re!(ESLINT_PROBLEM, r"^\s+(\d+):(\d+)\s+(error|warning)\s+(.+?)(?:\s{2,}(\S+))?\s*$");
re!(ESLINT_SUMMARY, r"^✖ \d+ problems? \((\d+) errors?, \d+ warnings?\)");
re!(CODE_FILE, r"^\S.*\.(?:[cm]?[jt]sx?|vue|svelte|astro|json|md|mdx|html|css|scss|graphql|ya?ml)$");

fn eslint(lines: &[&str], _: &Ctx) -> Option<Part> {
    let mut part = Part::default();
    let mut file: Option<&str> = None;
    let mut seen = false;
    for l in lines {
        if CODE_FILE.is_match(l) {
            file = Some(l.trim());
            continue;
        }
        if let Some(c) = ESLINT_SUMMARY.captures(l) {
            seen = true;
            part.failed = num(&c[1]);
            continue;
        }
        let Some(f) = file else { continue };
        let Some(c) = ESLINT_PROBLEM.captures(l) else {
            if !l.trim().is_empty() {
                file = None;
            }
            continue;
        };
        seen = true;
        if &c[3] != "error" {
            continue;
        }
        let rule = c.get(5).map(|m| format!(" ({})", m.as_str())).unwrap_or_default();
        part.failures.push(Failure { title: c[4].to_string(), file: Some(f.to_string()), line: num(&c[1]), message: format!("{}{rule}", &c[4]), snippet: None });
    }
    seen.then_some(part)
}

// ---------- JUnit XML ----------

re!(ANY_LOC, r"([\w./\-]+\.[A-Za-z]{1,5}):(\d+)");

/// JUnit XML printed straight to the output (some reporters do that).
fn junit_inline(lines: &[&str], _: &Ctx) -> Option<Part> {
    let start = lines.iter().position(|l| l.trim_start().starts_with("<testsuite"))?;
    junit(&lines[start..].join("\n"))
}

fn junit(xml: &str) -> Option<Part> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    #[derive(Default)]
    struct Case {
        name: String,
        class: String,
        file: Option<String>,
        line: Option<u32>,
        failed: bool,
        skipped: bool,
        message: String,
        text: String,
    }
    let attrs = |e: &quick_xml::events::BytesStart| -> Vec<(String, String)> {
        e.attributes()
            .flatten()
            .map(|a| {
                let raw = a.value.to_string();
                let v = quick_xml::escape::unescape(&raw).map(|c| c.into_owned()).unwrap_or(raw);
                (a.key.as_ref().to_string(), v)
            })
            .collect()
    };
    let get = |a: &[(String, String)], k: &str| a.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());

    let mut r = Reader::from_str(xml);
    let mut part = Part::default();
    let (mut cases, mut passed) = (0u32, 0u32);
    let mut cur: Option<Case> = None;
    let mut in_fail = false;
    let finish = |c: Case, part: &mut Part, passed: &mut u32| {
        if c.skipped && !c.failed {
            return;
        }
        if !c.failed {
            *passed += 1;
            return;
        }
        let body = c.text.trim().to_string();
        let (mut file, mut line) = (c.file.clone(), c.line);
        if line.is_none()
            && let Some(m) = ANY_LOC.captures(&body).or_else(|| ANY_LOC.captures(&c.message)) {
                file = Some(m[1].to_string());
                line = num(&m[2]);
            }
        if file.is_none() && c.class.contains('/') {
            file = Some(c.class.clone());
        }
        let message = if body.is_empty() { c.message.clone() } else if c.message.is_empty() || body.contains(&c.message) { body } else { format!("{}\n{body}", c.message) };
        part.failures.push(Failure { title: c.name, file, line, message, snippet: None });
    };
    loop {
        let ev = r.read_event();
        let (start, empty) = match &ev {
            Ok(Event::Start(e)) => (Some(e), false),
            Ok(Event::Empty(e)) => (Some(e), true),
            _ => (None, false),
        };
        if let Some(e) = start {
            match e.local_name().as_ref() {
                "testcase" => {
                    let a = attrs(e);
                    cases += 1;
                    let c = Case {
                        name: get(&a, "name").unwrap_or_default(),
                        class: get(&a, "classname").unwrap_or_default(),
                        file: get(&a, "file"),
                        line: get(&a, "line").and_then(|x| num(&x)),
                        ..Default::default()
                    };
                    if let Some(old) = cur.replace(c) {
                        finish(old, &mut part, &mut passed);
                    }
                    // `<testcase .../>` has no end tag: it passed.
                    if empty && let Some(c) = cur.take() {
                        finish(c, &mut part, &mut passed);
                    }
                }
                "failure" | "error" => {
                    if let Some(c) = cur.as_mut() {
                        c.failed = true;
                        c.message = get(&attrs(e), "message").unwrap_or_default();
                        in_fail = !empty;
                    }
                }
                "skipped" => {
                    if let Some(c) = cur.as_mut() {
                        c.skipped = true;
                    }
                }
                _ => {}
            }
            continue;
        }
        match ev {
            Ok(Event::End(e)) if matches!(e.local_name().as_ref(), "failure" | "error") => in_fail = false,
            Ok(Event::End(e)) if e.local_name().as_ref() == "testcase" => {
                if let Some(c) = cur.take() {
                    finish(c, &mut part, &mut passed);
                }
            }
            Ok(Event::Text(t)) if in_fail => {
                if let Some(c) = cur.as_mut() {
                    c.text.push_str(&t);
                }
            }
            Ok(Event::CData(t)) if in_fail => {
                if let Some(c) = cur.as_mut() {
                    c.text.push_str(&t);
                }
            }
            Ok(Event::GeneralRef(g)) if in_fail => {
                if let Some(c) = cur.as_mut() {
                    let name = g.to_string();
                    let ch = g.resolve_char_ref().ok().flatten().map(String::from).unwrap_or_else(|| match name.as_str() {
                        "amp" => "&".into(),
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        other => format!("&{other};"),
                    });
                    c.text.push_str(&ch);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    if let Some(c) = cur.take() {
        finish(c, &mut part, &mut passed);
    }
    if cases == 0 {
        return None;
    }
    part.passed = Some(passed);
    Some(part)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/fixtures/checks/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }
    fn at(f: &Failure) -> (Option<&str>, Option<u32>) {
        (f.file.as_deref(), f.line)
    }

    #[test]
    fn cargo_test_counts_every_binary_and_finds_each_panic() {
        let p = parse(&fixture("cargo_test.txt"), &Ctx { root: "/code/demo", go_module: None });
        assert_eq!(p.formats, ["cargo test"]);
        assert_eq!((p.passed, p.failed), (Some(3), Some(2)));
        assert_eq!(p.failures.len(), 2);
        assert_eq!(p.failures[0].title, "math::subtracts");
        assert_eq!(at(&p.failures[0]), (Some("src/math.rs"), Some(14)));
        assert!(p.failures[0].message.starts_with("assertion `left == right` failed"));
        assert!(p.failures[0].message.contains("right: 2"));
        assert!(!p.failures[0].message.contains("RUST_BACKTRACE"));
        assert_eq!(at(&p.failures[1]), (Some("src/parse.rs"), Some(31)));
        assert_eq!(p.failures[1].message, "called `Option::unwrap()` on a `None` value");
    }

    #[test]
    fn rustc_and_clippy_errors_but_not_warnings() {
        let p = parse(&fixture("rustc_errors.txt"), &Ctx { root: "/code/demo", go_module: None });
        assert_eq!(p.formats, ["rustc"]);
        assert_eq!(p.failures.len(), 2);
        assert_eq!(p.failures[0].title, "this `if` has identical blocks");
        assert_eq!(at(&p.failures[0]), (Some("src/lib.rs"), Some(8)));
        assert!(p.failures[0].message.contains("help: for further information"));
        assert_eq!(p.failures[1].title, "mismatched types");
        // Absolute paths inside the workspace come out relative.
        assert_eq!(at(&p.failures[1]), (Some("src/main.rs"), Some(4)));
        assert!(p.failures[1].message.contains("[E0308]"));
    }

    #[test]
    fn go_test_subtests_logs_packages_and_build_errors() {
        let p = parse(&fixture("go_test.txt"), &Ctx { root: "/code/pawprint", go_module: Some("pawprint") });
        assert_eq!(p.formats, ["go test"]);
        let titles: Vec<&str> = p.failures.iter().map(|f| f.title.as_str()).collect();
        // TestSLA itself only failed through its subtest, so only the subtest is listed.
        assert_eq!(titles, ["TestSLA/friday_evening", "TestHolidays", "undefined: backoff"]);
        assert_eq!(at(&p.failures[0]), (Some("internal/tickets/sla_test.go"), Some(42)));
        assert_eq!(p.failures[0].message.trim(), "deadline = 2026-11-06 21:00, want 2026-11-09 12:00");
        assert_eq!(at(&p.failures[1]), (Some("internal/tickets/holidays_test.go"), Some(18)));
        assert_eq!(p.failures[1].message.trim(), "holiday list is empty\nexpected 3 entries");
        assert_eq!(at(&p.failures[2]), (Some("internal/billing/retry.go"), Some(27)));
        assert_eq!(p.passed, Some(1));
        // Without go.mod the file stays as go printed it.
        let p = parse(&fixture("go_test.txt"), &Ctx::default());
        assert_eq!(at(&p.failures[0]), (Some("sla_test.go"), Some(42)));
    }

    #[test]
    fn pytest_uses_the_summary_and_the_traceback_line() {
        let p = parse(&fixture("pytest.txt"), &Ctx { root: "/code/pawprint", go_module: None });
        assert_eq!(p.formats, ["pytest"]);
        assert_eq!((p.passed, p.failed), (Some(4), Some(2)));
        assert_eq!(p.failures[0].title, "test_friday_rollover");
        assert_eq!(at(&p.failures[0]), (Some("tests/test_sla.py"), Some(14)));
        // The full E lines, not the summary's cut-off message.
        assert!(p.failures[0].message.starts_with("AssertionError: assert datetime.datetime(2026, 11, 6, 21, 0) == datetime.datetime(2026, 11, 9, 12, 0)"));
        assert_eq!(p.failures[1].title, "TestRetry::test_gives_up_after_3");
        assert_eq!(at(&p.failures[1]), (Some("tests/test_billing.py"), Some(22)));
        assert_eq!(p.failures[1].message, "assert 5 == 3");
    }

    #[test]
    fn jest_bullets_with_message_and_first_own_frame() {
        let p = parse(&fixture("jest.txt"), &Ctx::default());
        assert_eq!(p.formats, ["jest"]);
        assert_eq!((p.passed, p.failed), (Some(7), Some(2)));
        assert_eq!(p.failures[0].title, "pricing › annual toggle › shows the yearly price");
        assert_eq!(at(&p.failures[0]), (Some("src/pricing/annual.test.ts"), Some(14)));
        assert_eq!(p.failures[0].message, "expect(received).toBe(expected) // Object.is equality\nExpected: \"$96/yr\"\nReceived: \"$120/yr\"");
        assert_eq!(at(&p.failures[1]), (Some("src/pricing/round.ts"), Some(3)));
        assert_eq!(p.failures[1].message, "TypeError: Cannot read properties of undefined (reading 'cents')");
    }

    #[test]
    fn vitest_failed_tests_section() {
        let p = parse(&fixture("vitest.txt"), &Ctx::default());
        assert_eq!(p.formats, ["vitest"]);
        assert_eq!((p.passed, p.failed), (Some(6), Some(1)));
        assert_eq!(p.failures.len(), 1);
        assert_eq!(p.failures[0].title, "pricing > annual toggle > shows the yearly price");
        assert_eq!(at(&p.failures[0]), (Some("src/pricing/annual.test.ts"), Some(14)));
        assert_eq!(p.failures[0].message, "AssertionError: expected '$120/yr' to be '$96/yr' // Object.is equality");
    }

    #[test]
    fn bun_test_error_comes_before_the_fail_line() {
        let p = parse(&fixture("bun_test.txt"), &Ctx { root: "/code/nekotrail", go_module: None });
        assert_eq!(p.formats, ["bun test"]);
        assert_eq!((p.passed, p.failed), (Some(2), Some(1)));
        assert_eq!(p.failures[0].title, "names > slugs keep digits");
        assert_eq!(at(&p.failures[0]), (Some("src/names.test.ts"), Some(3)));
        assert_eq!(p.failures[0].message, "expect(received).toBe(expected)\nExpected: \"fix-502\"\nReceived: \"fix502\"");
    }

    #[test]
    fn tsc_plain_and_pretty() {
        let p = parse(&fixture("tsc.txt"), &Ctx::default());
        assert_eq!(p.formats, ["tsc"]);
        assert_eq!(p.failures.len(), 2);
        assert_eq!(at(&p.failures[0]), (Some("src/api.ts"), Some(42)));
        assert_eq!(p.failures[0].title, "Type 'string' is not assignable to type 'number'.");
        assert!(p.failures[1].message.ends_with("Property 'command' is missing in type '{ id: string; }' but required in type 'Check'."));
        let p = parse(&fixture("tsc_pretty.txt"), &Ctx::default());
        assert_eq!(p.failures.len(), 1);
        assert_eq!(at(&p.failures[0]), (Some("src/x.ts"), Some(5)));
        assert_eq!(p.failures[0].message, "';' expected. (TS1005)");
        assert_eq!(p.failed, Some(1));
    }

    #[test]
    fn eslint_errors_per_file_warnings_left_out() {
        let p = parse(&fixture("eslint.txt"), &Ctx { root: "/code/pawprint-web", go_module: None });
        assert_eq!(p.formats, ["eslint"]);
        assert_eq!(p.failed, Some(3));
        assert_eq!(p.failures.len(), 3);
        assert_eq!(at(&p.failures[0]), (Some("src/pages/pricing.tsx"), Some(4)));
        assert_eq!(p.failures[0].message, "'useMemo' is defined but never used (@typescript-eslint/no-unused-vars)");
        assert_eq!(at(&p.failures[1]), (Some("src/pages/pricing.tsx"), Some(31)));
        assert_eq!(at(&p.failures[2]), (Some("src/app.tsx"), Some(7)));
        assert_eq!(p.failures[2].message, "Parsing error: Unexpected token");
    }

    #[test]
    fn junit_cases_failures_errors_and_skips() {
        let p = parse_junit(&fixture("junit.xml"), &Ctx::default()).unwrap();
        assert_eq!(p.formats, ["JUnit XML"]);
        assert_eq!((p.passed, p.failed), (Some(1), Some(2)));
        assert_eq!(p.failures[0].title, "pricing > shows the yearly price");
        assert_eq!(at(&p.failures[0]), (Some("src/pricing/annual.test.ts"), Some(14)));
        assert!(p.failures[0].message.starts_with("AssertionError: expected '$120/yr' to be '$96/yr' & more"));
        // No file attribute: the location comes from the error text.
        assert_eq!(at(&p.failures[1]), (Some("tests/test_billing.py"), Some(30)));
        assert!(p.failures[1].message.contains("ZeroDivisionError"));
        assert!(parse_junit("<testsuites></testsuites>", &Ctx::default()).is_none());
        // Printed to the output instead of a file works too.
        let p = parse(&format!("running...\n{}", fixture("junit.xml").lines().skip(1).collect::<Vec<_>>().join("\n")), &Ctx::default());
        assert!(p.formats.contains(&"JUnit XML"));
    }

    #[test]
    fn colour_codes_and_unknown_output() {
        let coloured = "\x1b[31msrc/a.ts(1,2): error TS2304: Cannot find name 'x'.\x1b[0m\r\n";
        let p = parse(coloured, &Ctx::default());
        assert_eq!(at(&p.failures[0]), (Some("src/a.ts"), Some(1)));
        let p = parse("Segmentation fault\nsomething went wrong", &Ctx::default());
        assert!(p.formats.is_empty() && p.failures.is_empty() && p.failed.is_none());
    }

    #[test]
    fn several_formats_add_up() {
        let out = format!("{}\n{}", fixture("tsc.txt"), fixture("eslint.txt"));
        let p = parse(&out, &Ctx { root: "/code/pawprint-web", go_module: None });
        assert_eq!(p.formats, ["tsc", "eslint"]);
        assert_eq!(p.failures.len(), 5);
        assert_eq!(p.failed, Some(5));
    }
}
