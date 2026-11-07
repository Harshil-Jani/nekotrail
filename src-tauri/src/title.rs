//! Workspace titles: a short name for the task, so 20 workspaces stay readable. The star name
//! stays as the workspace's id and shows as its subtitle.
//!
//! A title comes from one of four places, recorded as `title_source`:
//! - `auto`: derived here from the first prompt, locally (no model call);
//! - `issue`: the Linear issue the workspace was started from ("ENG-123 Fix the login loop"),
//!   which the first prompt never replaces;
//! - `pr`: the PR's title, which replaces an `auto` or `issue` one and follows later renames of the PR;
//! - `user`: typed by hand, never overwritten by either of the others.
//!
//! An empty source means no title yet (the star name is shown).

/// Longest derived title, in characters, the trailing "…" included.
pub const MAX: usize = 48;

pub const AUTO: &str = "auto";
pub const ISSUE: &str = "issue";
pub const PR: &str = "pr";
pub const USER: &str = "user";

/// May a title from `incoming` replace one whose source is `current` ("" for none)?
pub fn may_replace(current: &str, incoming: &str) -> bool {
    match incoming {
        USER => true,
        PR => current != USER,
        ISSUE => current.is_empty() || current == AUTO,
        AUTO => current.is_empty(),
        _ => false,
    }
}

/// A workspace started from an issue: its identifier, then its title, cut like any other title.
/// The identifier comes first so it survives the cut.
pub fn issue_title(identifier: &str, title: &str) -> String {
    let t = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let id = identifier.trim();
    match (id.is_empty(), t.is_empty()) {
        (true, _) => shorten(&t),
        (false, true) => id.to_string(),
        (false, false) => shorten(&format!("{id} {t}")),
    }
}

/// A short title from a prompt: its first line or sentence that says something, without markdown,
/// `@file` refs, URLs or a leading slash command, in sentence case, cut at a word near 48 chars.
/// Empty when nothing is left (a bare `/mcp`, a lone URL).
pub fn task_title(text: &str) -> String {
    let mut fenced = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.is_empty() {
            continue;
        }
        let (clean, removed) = clean_line(line);
        let sentence = first_sentence(&clean);
        let t = tidy(&drop_lead_in(sentence), removed);
        // A line that is only a screenshot's name or a timestamp names nothing: try the next one.
        if t.chars().any(char::is_alphanumeric) && !is_noise(&t) {
            return shorten(&sentence_case(&t));
        }
    }
    String::new()
}

/// Markdown, refs, URLs and a slash command out of one line; words kept. Also says whether a
/// ref or URL was taken out of the middle, which can leave a dangling "in" or "see".
fn clean_line(line: &str) -> (String, bool) {
    let mut s = line;
    // Block markers: headings, quotes, list bullets, numbered items, task boxes.
    loop {
        let t = s.trim_start();
        let next = t
            .strip_prefix('#')
            .map(|r| r.trim_start_matches('#'))
            .filter(|r| r.is_empty() || r.starts_with(' '))
            .or_else(|| t.strip_prefix('>'))
            .or_else(|| ["- ", "* ", "+ "].iter().find_map(|b| t.strip_prefix(b)))
            .or_else(|| {
                let digits = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
                (digits > 0 && digits <= 3).then(|| t[digits..].strip_prefix(". ").or_else(|| t[digits..].strip_prefix(") "))).flatten()
            })
            .or_else(|| ["[ ] ", "[x] ", "[X] "].iter().find_map(|b| t.strip_prefix(b)));
        match next {
            Some(r) if r.len() < t.len() => s = r,
            _ => break,
        }
    }
    let s = links_to_text(s.trim());
    let mut words: Vec<String> = vec![];
    let mut removed = false;
    let all: Vec<&str> = s.split_whitespace().collect();
    let mut i = 0;
    while i < all.len() {
        let w = all[i];
        i += 1;
        // A leading /command (but not a path like /v1/tickets).
        if i == 1 && is_slash_command(w) {
            continue;
        }
        let bare = w.trim_matches(|c: char| "([{<\"'`".contains(c) || ")]}>\"'`,;:".contains(c));
        // An attached file whose name has spaces ("@.context/attachments/Screenshot 2026-11-05 at
        // 5.42.16 PM.png") goes on to the word that ends in its extension.
        if bare.starts_with('@') && !has_extension(bare)
            && let Some(end) = all[i..].iter().take(8).position(|x| is_media_file(x.trim_end_matches(|c: char| ",;:)".contains(c))))
        {
            i += end + 1;
            removed = true;
            continue;
        }
        if is_url(bare) || bare.starts_with('@') || is_media_file(bare) {
            removed = true;
            // Keep the punctuation that ended the sentence, so "See @a.ts. Then" still splits.
            if let Some(end) = w.chars().last().filter(|c| ".?!".contains(*c))
                && let Some(prev) = words.last_mut()
            {
                prev.push(end);
            }
            continue;
        }
        words.push(w.to_string());
    }
    // Emphasis and code marks, never the words inside them. Single `_` stays (snake_case).
    (words.join(" ").replace("**", "").replace("__", "").replace("~~", "").replace(['`', '*'], ""), removed)
}

/// `[text](url)` and `![alt](url)` become their text.
fn links_to_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|i| open + i) else { break };
        let Some(end) = rest[close..].find(')').map(|i| close + i) else { break };
        let before = &rest[..open];
        match before.strip_suffix('!') {
            // An image says nothing about the task.
            Some(b) => out.push_str(b),
            None => {
                out.push_str(before);
                out.push_str(&rest[open + 1..close]);
            }
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn is_slash_command(w: &str) -> bool {
    w.strip_prefix('/').is_some_and(|c| c.chars().next().is_some_and(|f| f.is_ascii_alphabetic()) && c.chars().all(|ch| ch.is_ascii_alphanumeric() || "-_:".contains(ch)))
}

/// A pasted image or recording by name ("image.png", "Screen Recording.mov"): never a task.
fn is_media_file(w: &str) -> bool {
    let l = w.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".heic", ".tiff", ".bmp", ".mov", ".mp4", ".webm", ".pdf"].iter().any(|x| l.ends_with(x) && l.len() > x.len())
}

/// The last path segment has a short extension (`a.ts`, `notes.md`), so a ref ends here.
fn has_extension(w: &str) -> bool {
    let name = w.rsplit('/').next().unwrap_or(w);
    name.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// A title that says nothing: only a date, a time and the words a screenshot's name is made of
/// ("2026-11-05 at 5.42.16 PM", "Screenshot", "image"). Titles like these are replaced.
pub fn is_noise(t: &str) -> bool {
    const FILLER: [&str; 12] = ["at", "am", "pm", "screenshot", "screen", "shot", "recording", "image", "img", "photo", "pasted", "untitled"];
    !t.split_whitespace().any(|w| {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        w.chars().filter(|c| c.is_alphabetic()).count() >= 2 && !FILLER.contains(&w.as_str()) && !is_media_file(&w)
    })
}

fn is_url(w: &str) -> bool {
    let l = w.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("www.") || l.starts_with("file://")
}

/// Up to the first `.`, `?` or `!` that ends a sentence (followed by a space or the end), so file
/// names like `App.tsx` and versions like `1.2` stay whole. CJK full stops count too.
fn first_sentence(s: &str) -> &str {
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if "。？！".contains(c) {
            return &s[..i];
        }
        if ".?!".contains(c) && it.peek().is_none_or(|(_, n)| n.is_whitespace()) {
            return &s[..i];
        }
    }
    s
}

/// Politeness that says nothing about the task: "Hey, can you please fix X" is "fix X".
fn drop_lead_in(s: &str) -> String {
    const LEADS: [&str; 12] = ["hey", "hi", "hello", "ok", "okay", "so", "please", "can you", "could you", "would you", "i want you to", "i need you to"];
    let mut s = s.trim();
    loop {
        let hit = LEADS.iter().find(|l| {
            s.get(..l.len()).is_some_and(|p| p.eq_ignore_ascii_case(l)) && s[l.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric())
        });
        match hit {
            // Never strip it down to nothing ("Please." stays a title rather than nothing).
            Some(l) if s[l.len()..].trim_start_matches([',', '!', ' ']).chars().any(char::is_alphanumeric) => {
                s = s[l.len()..].trim_start_matches([',', '!', ' ']);
            }
            _ => break,
        }
    }
    s.to_string()
}

/// Collapse spaces and drop stray punctuation at the ends. With `dangling`, also the little words
/// a removed ref or a cut leaves behind ("Fix the bug in @src/a.ts" is "Fix the bug").
fn tidy(s: &str, dangling: bool) -> String {
    const DANGLING: [&str; 16] = ["in", "at", "on", "of", "from", "to", "for", "with", "and", "or", "the", "a", "an", "see", "per", "via"];
    let edge = |c: char| matches!(c, ',' | ';' | ':' | '.' | '!' | '?' | '-' | '\u{2013}' | '\u{2014}' | '|' | '&' | '=' | '>' | '<');
    let mut words: Vec<&str> = s.split_whitespace().collect();
    loop {
        let before = words.len();
        while words.first().is_some_and(|w| w.chars().all(edge)) {
            words.remove(0);
        }
        while words.last().is_some_and(|w| w.chars().all(edge)) {
            words.pop();
        }
        if dangling && words.len() > 2 && words.last().is_some_and(|w| DANGLING.contains(&w.trim_end_matches(edge).to_lowercase().as_str())) {
            words.pop();
        }
        if words.len() == before {
            break;
        }
    }
    let joined = words.join(" ");
    joined.trim_start_matches(edge).trim_end_matches(edge).to_string()
}

/// Capitalise the first letter when the first word is plain lowercase letters; leave `iOS`,
/// `snake_case`, `App.tsx`, `/v1` and scripts without case (Japanese, Chinese) as they are.
fn sentence_case(s: &str) -> String {
    let first = s.split_whitespace().next().unwrap_or("");
    let plain = first.chars().all(char::is_lowercase) && first.chars().count() > 1;
    let mut cs = s.chars();
    match cs.next() {
        Some(c) if plain => c.to_uppercase().chain(cs).collect(),
        _ => s.to_string(),
    }
}

/// At most `MAX` chars: cut at the last space past a third of the way, else mid-word (CJK has no spaces).
fn shorten(s: &str) -> String {
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let cut: String = s.chars().take(MAX - 1).collect();
    let body = match cut.rfind(' ') {
        Some(i) if cut[..i].chars().count() > MAX / 3 => tidy(&cut[..i], true),
        _ => cut.trim_end().to_string(),
    };
    format!("{body}…")
}

/// What v0.10 to v0.12 called a workspace after its first message. Only used once, to tell
/// those titles (source `auto`) from ones the user typed when the column is added.
pub fn legacy_task_title(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let sentence = line.split_inclusive(['.', '?', '!']).next().unwrap_or(line).trim_end_matches(['.', '?', '!']).trim();
    if sentence.chars().count() <= 48 {
        return sentence.to_string();
    }
    let cut: String = sentence.chars().take(48).collect();
    match cut.rfind(' ') {
        Some(i) if i > 20 => format!("{}…", &cut[..i]),
        _ => format!("{cut}…"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_prompts() {
        assert_eq!(task_title("Fix the 502 after deploy. Then add a test."), "Fix the 502 after deploy");
        assert_eq!(task_title("\n  Add dark mode?\nmore"), "Add dark mode");
        assert_eq!(task_title("add dark mode to settings"), "Add dark mode to settings");
        // File names and versions are not sentence ends.
        assert_eq!(task_title("Fix the crash in App.tsx when zoom is 1.5"), "Fix the crash in App.tsx when zoom is 1.5");
        assert_eq!(task_title("Hey, can you please fix the login redirect?"), "Fix the login redirect");
        // Mixed case and identifiers keep their case.
        assert_eq!(task_title("iOS build is broken"), "iOS build is broken");
        assert_eq!(task_title("snake_case names in the API"), "snake_case names in the API");
        // Little words are only trimmed where a ref was removed or the text was cut.
        assert_eq!(task_title("Turn dark mode on"), "Turn dark mode on");
        assert_eq!(task_title("#482 is flaky"), "#482 is flaky");
    }

    #[test]
    fn markdown_is_stripped() {
        assert_eq!(task_title("## Make **retries** use `backoff`"), "Make retries use backoff");
        assert_eq!(task_title("- [ ] wire the [SLA timer](https://x.dev/sla) to the worker"), "Wire the SLA timer to the worker");
        assert_eq!(task_title("> 1. rename *foo* to bar_baz"), "Rename foo to bar_baz");
        assert_eq!(task_title("```\nlet x = 1;\n```\nExplain this snippet"), "Explain this snippet");
        assert_eq!(task_title("![screenshot](a.png) the header overlaps"), "The header overlaps");
    }

    #[test]
    fn refs_urls_and_commands_are_stripped() {
        assert_eq!(task_title("Fix the bug in @src/App.tsx"), "Fix the bug");
        assert_eq!(task_title("@src/api.ts @src/App.tsx make displayName use the PR title"), "Make displayName use the PR title");
        assert_eq!(task_title("Fix the crash from https://sentry.io/x/123"), "Fix the crash");
        assert_eq!(task_title("Port (https://example.com/spec) to Go"), "Port to Go");
        assert_eq!(task_title("/plan add rate limiting to /v1/tickets"), "Add rate limiting to /v1/tickets");
        // A path is not a command.
        assert_eq!(task_title("/v1/tickets returns 502"), "/v1/tickets returns 502");
        // Nothing left on the first line: the next line names the task.
        assert_eq!(task_title("https://linear.app/x/ISSUE-12\n\nMake exports faster"), "Make exports faster");
        assert_eq!(task_title("@notes.md. Then clean up the tests"), "Then clean up the tests");
    }

    #[test]
    fn screenshots_and_timestamps_are_not_titles() {
        // An attachment whose name has spaces is one ref, all of it.
        assert_eq!(task_title("@.context/attachments/Screenshot 2026-11-05 at 5.42.16 PM.png the sidebar logo is missing"), "The sidebar logo is missing");
        assert_eq!(task_title("@.context/attachments/Screenshot 2026-11-05 at 5.42.16 PM.png\nFix the header"), "Fix the header");
        // A pasted image by name.
        assert_eq!(task_title("image.png PR Issues"), "PR Issues");
        assert_eq!(task_title("image.png\n\nWhy is the PR bar empty?"), "Why is the PR bar empty");
        // Nothing but a timestamp: no title, so the branch or star shows instead.
        assert_eq!(task_title("2026-11-05 at 5.42.16 PM"), "");
        assert_eq!(task_title("Screenshot 2026-11-05 at 5.42.16 PM.png"), "");
        assert!(is_noise("2026-11-05 at 5.42.16…") && is_noise("image.png") && is_noise("Screenshot"));
        assert!(!is_noise("Fix 502s after deploy") && !is_noise("PR Issues") && !is_noise("ログインのバグを直して"));
        // Refs that end in a normal file keep working.
        assert_eq!(task_title("@src/App.tsx make the header calmer"), "Make the header calmer");
    }

    #[test]
    fn long_text_is_cut_at_a_word() {
        let t = task_title("The load balancer returns 502 on /v1/tickets right after a deploy and nobody knows why");
        assert!(t.ends_with('…') && t.chars().count() <= MAX, "{t}");
        assert_eq!(t, "The load balancer returns 502 on /v1/tickets…");
        // A dangling little word at the cut goes too.
        let t = task_title("Move the ticket SLA timer from the API process into the worker with the queue");
        assert_eq!(t, "Move the ticket SLA timer from the API process…");
        let t = task_title("Retry webhooks with backoff and jitter when the receiver times out or returns a 5xx");
        assert_eq!(t, "Retry webhooks with backoff and jitter when…");
        // One huge word is cut mid-word.
        let t = task_title(&"a".repeat(80));
        assert_eq!(t.chars().count(), MAX);
    }

    #[test]
    fn non_latin_text() {
        assert_eq!(task_title("ログインのバグを直して。テストも追加"), "ログインのバグを直して");
        let long = task_title(&"修复部署后的网关错误".repeat(10));
        assert!(long.ends_with('…') && long.chars().count() == MAX, "{long}");
        assert_eq!(task_title("исправь ошибку входа"), "Исправь ошибку входа");
        assert_eq!(task_title("Ajoute le mode sombre, s'il te plaît"), "Ajoute le mode sombre, s'il te plaît");
    }

    #[test]
    fn empty_and_nothing_left() {
        assert_eq!(task_title(""), "");
        assert_eq!(task_title("   \n\n  "), "");
        assert_eq!(task_title("/mcp"), "");
        assert_eq!(task_title("https://example.com"), "");
        assert_eq!(task_title("@src/App.tsx"), "");
        assert_eq!(task_title("```\nonly code\n```"), "");
        assert_eq!(task_title("Please."), "Please");
    }

    #[test]
    fn source_rules() {
        // Derived titles only fill an empty slot.
        assert!(may_replace("", AUTO));
        assert!(!may_replace(AUTO, AUTO));
        assert!(!may_replace(PR, AUTO));
        assert!(!may_replace(USER, AUTO));
        // A PR title replaces a derived one and follows PR renames, but never a typed one.
        assert!(may_replace("", PR));
        assert!(may_replace(AUTO, PR));
        assert!(may_replace(PR, PR));
        assert!(!may_replace(USER, PR));
        // The user always wins.
        assert!(may_replace(AUTO, USER) && may_replace(PR, USER) && may_replace(USER, USER) && may_replace("", USER));
        assert!(!may_replace("", "other"));
        // An issue's title beats the first prompt's, and gives way to the PR's and the user's.
        assert!(may_replace("", ISSUE) && may_replace(AUTO, ISSUE));
        assert!(!may_replace(ISSUE, AUTO) && !may_replace(PR, ISSUE) && !may_replace(USER, ISSUE));
        assert!(may_replace(ISSUE, PR) && may_replace(ISSUE, USER));
    }

    #[test]
    fn issue_titles_keep_the_identifier() {
        assert_eq!(issue_title("ENG-123", "Fix the  login loop"), "ENG-123 Fix the login loop");
        assert_eq!(issue_title("ENG-7", "  "), "ENG-7");
        assert_eq!(issue_title("", "Only a title"), "Only a title");
        let long = issue_title("ENG-123", "Make the checkout page remember the last shipping address across devices");
        assert!(long.starts_with("ENG-123 Make the checkout"), "{long}");
        assert!(long.ends_with('…') && long.chars().count() <= MAX, "{long}");
    }
}
