//! Opening links and logins outside the app, and checking whether a dev stack is up.

use kanban_store::{looks_secret, profile_for, Profile};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn checked(url: &str, schemes: &[&str]) -> Result<(), String> {
    if looks_secret(url) {
        return Err("that link looks like it holds a token, so it isn't opened".into());
    }
    let lower = url.to_ascii_lowercase();
    if schemes.iter().any(|s| lower.starts_with(s)) {
        Ok(())
    } else {
        Err(format!("only {} links are opened", schemes.join(" and ")))
    }
}

pub fn open_external(url: &str) -> Result<(), String> {
    checked(url, &["https://", "http://", "claude://"])?;
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

fn chrome() -> Option<PathBuf> {
    let mut places = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            places.push(PathBuf::from(base).join("Google").join("Chrome").join("Application").join("chrome.exe"));
        }
    }
    places.into_iter().find(|p| p.exists())
}

/// Opens a login in the Chrome profile its label (or its link) maps to in `profiles`, the
/// `launch.profiles` setting (Settings, Browser profiles); anything else goes to the default browser.
pub fn open_login(url: &str, browser: &str, profiles: &[Profile]) -> Result<(), String> {
    checked(url, &["https://", "http://"])?;
    let profile = profile_for(profiles, browser, url);
    match (profile, chrome()) {
        (Some(profile), Some(exe)) => Command::new(exe)
            .arg(format!("--profile-directory={profile}"))
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("couldn't start Chrome: {e}")),
        (Some(_), None) => Err("Chrome isn't installed where expected".into()),
        (None, _) => open_external(url),
    }
}

/// For a login on a local dev stack, whether the stack answers the login link itself with a
/// success or a redirect. Only the status line is read; the body is never read or kept.
/// Links that aren't on 127.0.0.1 count as up, since there's nothing local to check.
pub fn stack_up(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://127.0.0.1:") else { return true };
    let port: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let path = login_path(&rest[port.len()..]);
    let Ok(port) = port.parse::<u16>() else { return true };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(800)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_millis(1500)));
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut head = [0u8; 16];
    let n = s.read(&mut head).unwrap_or(0);
    answered(&String::from_utf8_lossy(&head[..n]))
}

/// The path (and query) to ask for: what follows the port, without a #fragment; `/` if none.
fn login_path(after_port: &str) -> String {
    let p = after_port.split('#').next().unwrap_or("");
    if p.starts_with('/') { p.to_string() } else { "/".to_string() }
}

/// A status line with a 2xx or 3xx code.
fn answered(status_line: &str) -> bool {
    let code = status_line.split(' ').nth(1).unwrap_or("");
    status_line.starts_with("HTTP/1.") && (code.starts_with('2') || code.starts_with('3'))
}

#[cfg(test)]
mod stack_up_tests {
    use super::*;

    #[test]
    fn asks_for_the_login_link_and_takes_success_or_redirect() {
        assert_eq!(login_path("/login.html?as=admin&next=/x#top"), "/login.html?as=admin&next=/x");
        assert_eq!(login_path(""), "/");
        assert!(answered("HTTP/1.1 200 OK") && answered("HTTP/1.1 302 Found") && answered("HTTP/1.0 204 "));
        assert!(!answered("HTTP/1.1 404 Not") && !answered("HTTP/1.1 500 Err") && !answered(""));
    }
}
