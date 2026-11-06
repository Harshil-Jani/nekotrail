//! Hides things that look like secrets (tokens, keys, passwords, emails) in a short text before it
//! leaves the Mac. Twin of `src/redact.ts`, which the session export uses: the same rules in the
//! same order, so a line hidden in an export is hidden in an away alert too. The Rust regex crate
//! has no look-behind, so the rules that need one match the character before instead and only
//! hide the captured value.

use regex::{Captures, Regex};
use std::sync::LazyLock;

/// One rule: `group` is the part that gets hidden (0 for the whole match).
struct Detector {
    kind: &'static str,
    re: Regex,
    group: usize,
    ok: fn(&str, &Captures) -> bool,
}

fn always(_: &str, _: &Captures) -> bool {
    true
}

fn not_placeholder(v: &str, _: &Captures) -> bool {
    !is_placeholder(v)
}

/// Values that are clearly not a real secret: variables, templates, placeholders, numbers, booleans.
pub fn is_placeholder(v: &str) -> bool {
    static START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(\$|%|<|\{\{|\$\{|\[redacted)").unwrap());
    static CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(process\.env|os\.environ|env\(|getenv|secrets\.)").unwrap());
    static FILL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(x{3,}|\*{3,}|\.{3,}|•+|-+)$").unwrap());
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(changeme|change_me|your[_-]|example|placeholder|dummy|sample|null|none|nil|true|false|undefined|redacted|todo)").unwrap());
    static NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\d+(\.\d+)?[a-z]{0,2}$").unwrap());
    let s = v.trim();
    let s = s.strip_prefix(['"', '\'']).unwrap_or(s);
    let s = s.strip_suffix(['"', '\'']).unwrap_or(s);
    s.chars().count() < 4 || START.is_match(s) || CODE.is_match(s) || FILL.is_match(s) || WORD.is_match(s) || NUM.is_match(s)
}

static SECRET_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(SECRET|TOKEN|PASSW(?:OR)?D|(?:^|_)PWD(?:_|$)|API_?KEY|PRIVATE_?KEY|ACCESS_?KEY|CREDENTIALS?|(?:^|_)AUTH(?:_|$)|(?:^|_)DSN(?:_|$)|DATABASE_URL|CONN(?:ECTION)?_?STRING|WEBHOOK_URL|SIGNING_KEY|CLIENT_KEY)").unwrap()
});

fn email_ok(v: &str, _: &Captures) -> bool {
    let Some(at) = v.rfind('@') else { return false };
    let (local, domain) = (&v[..at], v[at + 1..].to_lowercase());
    let tld = domain.rsplit('.').next().unwrap_or("");
    const FILE_TLDS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "js", "mjs", "cjs", "ts", "tsx", "jsx", "css", "scss", "json", "md", "htm", "html", "lock", "yml", "yaml", "toml", "rs", "go", "py", "rb", "sh"];
    if FILE_TLDS.contains(&tld) {
        return false;
    }
    static EXAMPLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|\.)example\.(com|org|net)$|\.(example|test|invalid|localhost)$").unwrap());
    if EXAMPLE.is_match(&domain) {
        return false;
    }
    let l = local.to_lowercase();
    !(l.contains("noreply") || l.contains("no-reply") || domain == "users.noreply.github.com" || local == "git")
}

/// Specific formats first: when two rules match the same text, the earlier one names it.
static DETECTORS: LazyLock<Vec<Detector>> = LazyLock::new(|| {
    let d = |kind, re: &str, group, ok: fn(&str, &Captures) -> bool| Detector { kind, re: Regex::new(re).unwrap(), group, ok };
    vec![
        d("Private key", r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----", 0, always),
        d("GitHub token", r"\b(?:gh[pousr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{22,255})\b", 0, always),
        d("GitLab token", r"\bglpat-[A-Za-z0-9_-]{20,}\b", 0, always),
        d("Anthropic key", r"\bsk-ant-[A-Za-z0-9_-]{20,}", 0, always),
        // OpenAI style. Needs a digit, so a word like "sk-learn-is-a-library" stays.
        d("API key", r"(?:^|[^A-Za-z0-9_-])(sk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_-]{20,})", 1, |v, _| {
            v.chars().any(|c| c.is_ascii_digit()) && v[3..].chars().any(|c| c.is_ascii_alphabetic())
        }),
        d("Stripe key", r"\b(?:sk|rk|pk|whsec)_(?:live|test)_[A-Za-z0-9]{16,}\b|\bwhsec_[A-Za-z0-9]{24,}\b", 0, always),
        d("Slack token", r"\bxox[abposr]-[A-Za-z0-9-]{10,}\b", 0, always),
        d("Linear key", r"\blin_(?:api|oauth)_[A-Za-z0-9]{32,}\b", 0, always),
        d("AWS access key", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", 0, always),
        d("Google API key", r"\bAIza[0-9A-Za-z_-]{35}\b", 0, always),
        d("npm token", r"\bnpm_[A-Za-z0-9]{36}\b", 0, always),
        d("Hugging Face token", r"\bhf_[A-Za-z0-9]{30,}\b", 0, always),
        d("JWT", r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}", 0, always),
        d("Bearer token", r"\b[Bb]earer\s+([A-Za-z0-9._~+/-]{16,}=*)", 1, not_placeholder),
        d("Password in a URL", r#"\b[a-z][a-z0-9+.-]*://[^\s:/@'"`]+:([^\s@/'"`]+)@"#, 1, not_placeholder),
        // KEY=value with a secret-sounding upper-case key, on its own line or inline in a command.
        d("Secret value", r#"(?:^|[^A-Za-z0-9_])([A-Z][A-Z0-9_]*)[ \t]*=[ \t]*("[^"\n]*"|'[^'\n]*'|[^\s"'`;&|]+)"#, 2, |v, c| {
            SECRET_KEY.is_match(&c[1]) && !is_placeholder(v)
        }),
        // "apiKey": "...", password: '...' in JSON, YAML and code.
        d("Secret value", r#"(?i)["']?([A-Za-z0-9_-]*(?:api[_-]?key|secret|token|passw(?:or)?d)[A-Za-z0-9_-]*)["']?[ \t]*[:=][ \t]*["']([^"'\s]{6,})["']"#, 2, not_placeholder),
        d("Email address", r"(?:^|[^A-Za-z0-9._%+-])([A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,24})\b", 1, email_ok),
    ]
});

/// One place in a text that looks like a secret (byte offsets).
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: &'static str,
}

/// The secret-looking places in `text`, in order, without overlaps (the more specific rule wins).
pub fn scan(text: &str) -> Vec<Span> {
    let mut found: Vec<(usize, Span)> = vec![];
    for (rank, d) in DETECTORS.iter().enumerate() {
        for c in d.re.captures_iter(text) {
            let Some(m) = c.get(d.group) else { continue };
            let (mut start, mut end) = (m.start(), m.end());
            // Quotes around a value stay; only what's inside is hidden.
            let v = m.as_str();
            if v.len() >= 2 && (v.starts_with('"') && v.ends_with('"') || v.starts_with('\'') && v.ends_with('\'')) {
                start += 1;
                end -= 1;
            }
            let value = &text[start..end];
            if value.is_empty() || !(d.ok)(value, &c) {
                continue;
            }
            found.push((rank, Span { start, end, kind: d.kind }));
        }
    }
    found.sort_by_key(|(rank, s)| (*rank, s.start));
    let mut kept: Vec<Span> = vec![];
    for (_, f) in found {
        if kept.iter().any(|k| f.start < k.end && k.start < f.end) {
            continue;
        }
        kept.push(f);
    }
    kept.sort_by_key(|s| s.start);
    kept
}

/// `text` with every secret replaced by `[hidden: kind]`, as in an export.
pub fn mask(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for s in scan(text) {
        out.push_str(&text[at..s.start]);
        out.push_str(&format!("[hidden: {}]", s.kind));
        at = s.end;
    }
    out.push_str(&text[at..]);
    out
}

/// Replaces the home folder at the start of a path with `~`, only as a whole folder.
pub fn shorten_home(text: &str, home: &str) -> String {
    let h = home.trim_end_matches('/');
    if h.len() < 2 {
        return text.to_string();
    }
    let re = Regex::new(&format!(r"{}([^A-Za-z0-9._-]|$)", regex::escape(h))).unwrap();
    re.replace_all(text, "~$1").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Made-up values in the shape of real ones (the same as tests/redact.test.ts). None is live.
    fn kinds(text: &str) -> Vec<(&'static str, String)> {
        scan(text).into_iter().map(|s| (s.kind, text[s.start..s.end].to_string())).collect()
    }

    #[test]
    fn names_each_format() {
        let gh = format!("ghp_{}", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8");
        let openai = format!("sk-proj-{}", "Ab12Cd34Ef56Gh78Ij90Kl12");
        let anthropic = format!("sk-ant-api03-{}", "Zx9Yw8Vu7Ts6Rq5Po4Nm3");
        let slack = format!("xoxb-{}", "123456789012-abcdefABCDEF");
        let aws = format!("AKIA{}", "IOSFODNN7EXAMPL1");
        assert_eq!(kinds(&format!("export GH={gh}"))[0], ("GitHub token", gh.clone()));
        assert_eq!(kinds(&format!("key {openai}")), [("API key", openai)]);
        assert_eq!(kinds(&format!("key {anthropic}")), [("Anthropic key", anthropic)]);
        assert_eq!(kinds(&format!("slack {slack}")), [("Slack token", slack)]);
        let linear = format!("lin_api_{}", "Ab12Cd34Ef56Gh78Ij90Kl12Mn34Op56Qr78St90");
        assert_eq!(kinds(&format!("Authorization: {linear}")), [("Linear key", linear.clone())]);
        assert_eq!(mask(&format!("key={linear}.")), "key=[hidden: Linear key].");
        assert_eq!(kinds(&format!("aws configure set aws_access_key_id {aws}")), [("AWS access key", aws)]);
    }

    #[test]
    fn bearer_urls_and_key_values() {
        assert_eq!(kinds(r#"curl -H "Authorization: Bearer abcdEFGH1234ijklMNOP5678" https://api.x.io"#), [("Bearer token", "abcdEFGH1234ijklMNOP5678".into())]);
        assert_eq!(kinds("DATABASE=postgres://app:s3cretPassw0rd@db.internal:5432/app"), [("Password in a URL", "s3cretPassw0rd".into())]);
        assert_eq!(kinds("OPENAI_API_KEY=abc123def456 npm run dev"), [("Secret value", "abc123def456".into())]);
        assert_eq!(kinds(r#"{"apiKey": "k-29fj39fj2", "name": "x"}"#), [("Secret value", "k-29fj39fj2".into())]);
        assert_eq!(kinds("Author: Sam Lee <sam.lee@pawprint.io>"), [("Email address", "sam.lee@pawprint.io".into())]);
    }

    #[test]
    fn leaves_ordinary_engineering_text_alone() {
        for line in [
            "commit 9fceb02d0ae598e95dc970b74767f19372d61af8",
            "id 123e4567-e89b-12d3-a456-426614174000",
            "pip install scikit-learn; see risk-assessment-for-the-quarter",
            "GITHUB_TOKEN=$GITHUB_TOKEN gh pr create",
            "TOKEN_TTL=3600 SESSION_TIMEOUT=30s",
            "AUTHOR=sam",
            r#"password: "changeme""#,
            "git@github.com:pawprint/api.git",
            "icon@2x.png and logo@3x.webp",
            "noreply@github.com, you@example.com",
            "Bearer token",
            "All 42 tests pass. I opened PR #12.",
        ] {
            assert_eq!(scan(line), vec![], "{line}");
        }
    }

    #[test]
    fn mask_keeps_the_rest_and_the_quotes() {
        let gh = format!("ghp_{}", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8");
        assert_eq!(mask(&format!("push with {gh} to sam@pawprint.io")), "push with [hidden: GitHub token] to [hidden: Email address]");
        assert_eq!(mask(r#"DB_PASSWORD="hunter2hunter2""#), r#"DB_PASSWORD="[hidden: Secret value]""#);
        assert_eq!(mask("Done, café ✓"), "Done, café ✓");
    }

    #[test]
    fn home_becomes_a_tilde_only_as_a_whole_folder() {
        assert_eq!(shorten_home("cd /Users/sam/code && ls /Users/sam", "/Users/sam"), "cd ~/code && ls ~");
        assert_eq!(shorten_home("/Users/samantha/code", "/Users/sam"), "/Users/samantha/code");
        assert_eq!(shorten_home("/home/sam/x", "/home/sam/"), "~/x");
    }
}
