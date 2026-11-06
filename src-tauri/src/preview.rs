//! The Preview tab: is the workspace's dev server up, will it show inside a frame, and a
//! snapshot of the web view for Annotate.
//!
//! Only local addresses are ever touched here (localhost, 127.0.0.1, [::1]). The UI enforces the
//! same rule, and a `frame-src` CSP in index.html stops the embedded page from navigating anywhere else.

use serde::Serialize;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub struct LocalUrl {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
}

/// Parse `http(s)://localhost|127.0.0.1|[::1][:port][/path]`. Anything else (other hosts, user info,
/// odd schemes) is refused, so a crafted address can't make us talk to the network.
pub fn parse_local(url: &str) -> Option<LocalUrl> {
    let url = url.trim();
    let (https, rest) = if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else {
        return None;
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    if authority.contains('@') || authority.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let close = v6.find(']')?;
        let host = &v6[..close];
        let after = &v6[close + 1..];
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p),
            None if after.is_empty() => None,
            None => return None,
        };
        (format!("[{host}]"), port)
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (authority.to_string(), None),
        }
    };
    let host = host.to_ascii_lowercase();
    if !matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]") {
        return None;
    }
    let port = match port {
        Some(p) => p.parse::<u16>().ok().filter(|&p| p > 0)?,
        None => if https { 443 } else { 80 },
    };
    let path = if path.is_empty() { "/".to_string() } else { path.to_string() };
    if path.chars().any(|c| c.is_control() || c == ' ') {
        return None;
    }
    Some(LocalUrl { https, host, port, path })
}

/// Why a page refuses to be shown inside another app, from its response headers, if it does.
pub fn frame_blocked(head: &str) -> Option<String> {
    for line in head.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else { continue };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        if name == "x-frame-options" && (value.contains("deny") || value.contains("sameorigin")) {
            return Some("x-frame-options".into());
        }
        if name == "content-security-policy" {
            for directive in value.split(';') {
                let mut parts = directive.split_whitespace();
                if parts.next() == Some("frame-ancestors") && !parts.any(|s| s == "*") {
                    return Some("frame-ancestors".into());
                }
            }
        }
    }
    None
}

#[derive(Serialize, Debug, Default)]
pub struct Probe {
    /// Something answers on that port.
    pub up: bool,
    /// HTTP status of the page, when we could read it (not for https).
    pub status: Option<u16>,
    /// Set when the page forbids being framed ("x-frame-options" or "frame-ancestors").
    pub blocked: Option<String>,
}

/// Knock on the dev server: connect, and for plain http read the response head.
pub fn probe(url: &str) -> Result<Probe, String> {
    let u = parse_local(url).ok_or("Only localhost and 127.0.0.1 addresses can be previewed.")?;
    let host = u.host.trim_start_matches('[').trim_end_matches(']');
    let addrs: Vec<_> = (host, u.port).to_socket_addrs().map_err(|e| e.to_string())?.collect();
    // "localhost" is ::1 and 127.0.0.1; dev servers often listen on only one of them.
    let Some(mut stream) = addrs.iter().find_map(|a| TcpStream::connect_timeout(a, Duration::from_millis(800)).ok()) else {
        return Ok(Probe::default());
    };
    if u.https {
        return Ok(Probe { up: true, ..Default::default() });
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}:{}\r\nAccept: text/html\r\nUser-Agent: nekotrail-preview\r\nConnection: close\r\n\r\n",
        u.path, u.host, u.port
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return Ok(Probe { up: true, ..Default::default() });
    }
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while buf.len() < 64 * 1024 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let head = text.split("\r\n\r\n").next().unwrap_or("");
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok());
    Ok(Probe { up: true, status, blocked: frame_blocked(head) })
}

/// PNG of everything the app's web view shows right now, at the screen's pixel density.
/// WKWebView's own snapshot includes cross-origin frames, which the page itself can't read.
#[cfg(target_os = "macos")]
pub async fn snapshot_png(webview: tauri::Webview) -> Result<Vec<u8>, String> {
    use block2::RcBlock;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use objc2_web_kit::WKWebView;
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    webview
        .with_webview(move |pw| {
            let wk = pw.inner() as *const WKWebView;
            let done = tx.clone();
            let handler = RcBlock::new(move |img: *mut NSImage, err: *mut NSError| {
                let out = if img.is_null() {
                    let why = unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string());
                    Err(why.unwrap_or_else(|| "The snapshot came back empty.".into()))
                } else {
                    let img = unsafe { &*img };
                    img.TIFFRepresentation()
                        .and_then(|tiff| NSBitmapImageRep::imageRepWithData(&tiff))
                        .and_then(|rep| unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) })
                        .map(|png| png.to_vec())
                        .ok_or_else(|| "Couldn't turn the snapshot into a PNG.".to_string())
                };
                let _ = done.send(out);
            });
            match unsafe { wk.as_ref() } {
                Some(wk) => unsafe { wk.takeSnapshotWithConfiguration_completionHandler(None, &handler) },
                None => { let _ = tx.send(Err("No web view to snapshot.".into())); }
            }
        })
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(10)))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|_| "The snapshot took too long.".to_string())?
}

#[cfg(not(target_os = "macos"))]
pub async fn snapshot_png(_webview: tauri::Webview) -> Result<Vec<u8>, String> {
    Err("Annotate needs macOS for now.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_addresses_parse() {
        let u = parse_local("http://localhost:3000/dashboard?x=1").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.path.as_str()), (false, "localhost", 3000, "/dashboard?x=1"));
        assert_eq!(parse_local("http://127.0.0.1:5173").unwrap().path, "/");
        assert_eq!(parse_local("https://LOCALHOST").unwrap().port, 443);
        assert_eq!(parse_local("http://[::1]:8080/").unwrap().host, "[::1]");
        for bad in [
            "http://example.com",
            "http://localhost.evil.com:3000",
            "http://localhost:3000@evil.com/",
            "http://evil.com#@localhost:3000",
            "http://127.0.0.2:3000",
            "http://0.0.0.0:3000",
            "ftp://localhost:21",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "http://localhost:99999",
            "http://localhost:0",
            "http://localhost:3000/a b",
            "localhost:3000",
        ] {
            assert!(parse_local(bad).is_none(), "{bad} should be refused");
        }
    }

    #[test]
    fn spots_pages_that_refuse_frames() {
        assert_eq!(frame_blocked("HTTP/1.1 200 OK\r\nX-Frame-Options: DENY\r\n").as_deref(), Some("x-frame-options"));
        assert_eq!(frame_blocked("HTTP/1.1 200 OK\r\nx-frame-options: SAMEORIGIN").as_deref(), Some("x-frame-options"));
        assert_eq!(frame_blocked("HTTP/1.1 200 OK\r\nContent-Security-Policy: default-src 'self'; frame-ancestors 'self'").as_deref(), Some("frame-ancestors"));
        assert_eq!(frame_blocked("HTTP/1.1 200 OK\r\nContent-Security-Policy: frame-ancestors *"), None);
        assert_eq!(frame_blocked("HTTP/1.1 200 OK\r\nContent-Security-Policy-Report-Only: frame-ancestors 'none'"), None);
        assert_eq!(frame_blocked("HTTP/1.1 200 OK\r\nContent-Type: text/html"), None);
    }

    #[test]
    fn probe_reads_a_real_local_server() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = [0u8; 1024];
            let _ = s.read(&mut req);
            s.write_all(b"HTTP/1.1 200 OK\r\nX-Frame-Options: DENY\r\nContent-Length: 2\r\n\r\nhi").unwrap();
        });
        let p = probe(&format!("http://127.0.0.1:{port}/")).unwrap();
        server.join().unwrap();
        assert!(p.up);
        assert_eq!(p.status, Some(200));
        assert_eq!(p.blocked.as_deref(), Some("x-frame-options"));

        // Nothing listening: not up, and no error.
        let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert!(!probe(&format!("http://127.0.0.1:{free}/")).unwrap().up);
        assert!(probe("http://example.com/").is_err());
    }
}
