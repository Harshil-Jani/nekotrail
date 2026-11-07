//! Repo avatars: the picture of the GitHub user or organisation that owns a repo, worked out from the
//! repo's remote URL (no `gh` call) and kept on disk, so the sidebar shows it on every launch and offline.
//! The sidebar falls back to the repo's first letter only when there is no GitHub remote, or the picture
//! was never fetched and the network is down.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A cached picture older than this is fetched again (the old one is still used if that fails).
const FRESH: Duration = Duration::from_secs(7 * 24 * 3600);
/// Avatars are small; anything bigger than this is not an avatar.
const MAX_BYTES: usize = 512 * 1024;

/// The owner in a GitHub remote: `git@github.com:owner/repo.git`, `https://github.com/owner/repo`,
/// `ssh://git@github.com:22/owner/repo.git`, `https://token@github.com/owner/repo`. None for other hosts.
pub fn github_owner(remote: &str) -> Option<String> {
    let r = remote.trim();
    let (host, path) = if let Some(rest) = r.split_once("://").map(|(_, rest)| rest) {
        let (auth_host, path) = rest.split_once('/')?;
        (auth_host.rsplit('@').next()?.split(':').next()?, path)
    } else {
        // scp-like: user@host:owner/repo
        let (user_host, path) = r.split_once(':')?;
        (user_host.rsplit('@').next()?, path)
    };
    if !host.eq_ignore_ascii_case("github.com") && !host.eq_ignore_ascii_case("www.github.com") {
        return None;
    }
    let owner = path.trim_start_matches('/').split('/').next()?;
    // GitHub logins: 1 to 39 letters, digits or hyphens.
    let ok = !owner.is_empty() && owner.len() <= 39 && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then(|| owner.to_string())
}

fn cache_file(dir: &Path, owner: &str) -> PathBuf {
    dir.join(format!("{}.img", owner.to_ascii_lowercase()))
}

/// The cached picture as a data URL, and whether it is still fresh.
pub fn cached(dir: &Path, owner: &str) -> Option<(String, bool)> {
    let file = cache_file(dir, owner);
    let bytes = std::fs::read(&file).ok().filter(|b| !b.is_empty())?;
    let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok());
    Some((data_url(&bytes), age.is_some_and(|a| a < FRESH)))
}

/// Downloads the owner's avatar into the cache. Errors are returned as text and never shown as such:
/// the caller falls back to the cached copy or the letter.
pub async fn fetch(dir: &Path, owner: &str) -> Result<String, String> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(concat!("nekotrail/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| err.without_url().to_string())?;
    // github.com/<owner>.png redirects to avatars.githubusercontent.com, for users and organisations alike.
    let resp = client.get(format!("https://github.com/{owner}.png?size=64")).send().await.map_err(|err| err.without_url().to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let bytes = resp.bytes().await.map_err(|err| err.without_url().to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES || mime(&bytes).is_none() {
        return Err("not an image".into());
    }
    std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let file = cache_file(dir, owner);
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &file)).map_err(|err| err.to_string())?;
    Ok(data_url(&bytes))
}

/// The avatar for `owner`: a fresh cached copy, else a new download, else a stale copy, else None.
pub async fn get(dir: &Path, owner: &str) -> Option<String> {
    let have = cached(dir, owner);
    if let Some((url, true)) = &have {
        return Some(url.clone());
    }
    match fetch(dir, owner).await {
        Ok(url) => Some(url),
        Err(_) => have.map(|(url, _)| url),
    }
}

fn mime(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG") {
        Some("image/png")
    } else if b.starts_with(b"\xFF\xD8\xFF") {
        Some("image/jpeg")
    } else if b.starts_with(b"GIF8") {
        Some("image/gif")
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// A `data:` URL, which the web view's image policy already allows (no file access needed).
pub fn data_url(bytes: &[u8]) -> String {
    format!("data:{};base64,{}", mime(bytes).unwrap_or("image/png"), base64(bytes))
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_from_remotes() {
        assert_eq!(github_owner("git@github.com:Harshil-Jani/nekotrail.git").as_deref(), Some("Harshil-Jani"));
        assert_eq!(github_owner("https://github.com/conductor-oss/conductor").as_deref(), Some("conductor-oss"));
        assert_eq!(github_owner("https://github.com/acme/api.git\n").as_deref(), Some("acme"));
        assert_eq!(github_owner("ssh://git@github.com:22/acme/api.git").as_deref(), Some("acme"));
        assert_eq!(github_owner("https://x-access-token:abc@github.com/acme/api").as_deref(), Some("acme"));
        assert_eq!(github_owner("https://www.GitHub.com/acme/api").as_deref(), Some("acme"));
        // Other hosts and broken URLs have no GitHub avatar.
        assert_eq!(github_owner("git@gitlab.com:acme/api.git"), None);
        assert_eq!(github_owner("https://github.example.com/acme/api"), None);
        assert_eq!(github_owner("https://notgithub.com/acme/api"), None);
        assert_eq!(github_owner("/Users/me/code/api"), None);
        assert_eq!(github_owner(""), None);
        assert_eq!(github_owner("https://github.com/"), None);
        assert_eq!(github_owner("https://github.com/../etc/passwd"), None);
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn cache_round_trip() {
        let dir = std::env::temp_dir().join(format!("nekotrail-avatar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(cached(&dir, "acme").is_none());
        std::fs::create_dir_all(&dir).unwrap();
        let png = b"\x89PNG\r\n\x1a\nrest";
        std::fs::write(cache_file(&dir, "Acme"), png).unwrap();
        let (url, fresh) = cached(&dir, "acme").unwrap();
        assert!(fresh);
        assert!(url.starts_with("data:image/png;base64,"), "{url}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
