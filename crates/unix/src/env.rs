//! The environment an app started from Finder or launchd lacks (card #117 item 3, at_start):
//! the open-files limit, the login shell's PATH and LANG. Called first thing in main, before
//! any thread starts, because it sets environment variables.

use crate::{sys, text};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// What `import_login_env` did, for one line in the app's log.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    /// The shell the PATH was read from (or would have been).
    pub shell: String,
    /// The PATH the app now has.
    pub path: String,
    /// The LANG the app set, when it had to set one.
    pub lang: Option<String>,
    /// The open-files soft limit (before, after).
    pub nofile: (u64, u64),
    /// Why the login PATH wasn't used, when it wasn't.
    pub note: Option<String>,
}

impl Imported {
    /// The log line.
    pub fn log_line(&self) -> String {
        let mut s = format!(
            "environment: shell {}, open files {} -> {}, PATH {}",
            self.shell, self.nofile.0, self.nofile.1, self.path
        );
        if let Some(l) = &self.lang {
            s.push_str(&format!(", LANG set to {l}"));
        }
        if let Some(n) = &self.note {
            s.push_str(&format!(" ({n})"));
        }
        s
    }
}

const LOGIN_LIMIT: Duration = Duration::from_secs(5);

/// Run `<shell> -l -c <script>` in its own process group and read its output, for at most 5 s.
/// Err says why nothing usable came back.
fn run_login_shell(shell: &str) -> Result<String, String> {
    let mut child = Command::new(shell)
        .arg("-l")
        .arg("-c")
        .arg(text::login_shell_script())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("could not run the login shell {shell}: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("no output pipe")?;
    // Read on a thread in chunks: something an rc file started in the background may keep the
    // pipe open after the shell has exited, so the end of the pipe can't be waited for.
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut out = Vec::new();
    let started = Instant::now();
    let finished = loop {
        while let Ok(chunk) = rx.try_recv() {
            out.extend(chunk);
        }
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) if started.elapsed() < LOGIN_LIMIT => std::thread::sleep(Duration::from_millis(20)),
            _ => break false,
        }
    };
    if !finished {
        // SAFETY: the group is the shell's own (process_group(0) made the shell its leader).
        unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) };
        let _ = child.wait();
        return Err(format!("the login shell {shell} did not finish within 5 s, so its PATH was not used"));
    }
    // The rest of the output, until the pipe ends or briefly goes quiet.
    while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) {
        out.extend(chunk);
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn apple_locale() -> Option<String> {
    let out = Command::new("/usr/bin/defaults").args(["read", "-g", "AppleLocale"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !s.is_empty()).then_some(s)
}

/// Raise the open-files limit, take PATH (and LANG) from the user's login shell, and set LANG
/// when no locale variable is set. KANBAN_NO_LOGIN_SHELL=1 skips the shell.
pub fn import_login_env() -> Imported {
    let nofile = sys::raise_nofile();
    let shell = text::choose_shell(var("SHELL").as_deref(), sys::user_shell().as_deref());
    let (login, note) = if var("KANBAN_NO_LOGIN_SHELL").as_deref() == Some("1") {
        (None, Some("KANBAN_NO_LOGIN_SHELL=1, so the login shell was not run".to_string()))
    } else {
        match run_login_shell(&shell) {
            Ok(out) => {
                let (path, lang) = text::parse_login_output(&out);
                let note = path.is_none().then(|| format!("the login shell {shell} printed no PATH"));
                (Some((path, lang)), note)
            }
            Err(e) => (None, Some(e)),
        }
    };
    let (login_path, login_lang) = login.unwrap_or((None, None));
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let path = text::merge_path(login_path.as_deref(), var("PATH").as_deref(), &home, &|p: &Path| p.is_dir());
    std::env::set_var("PATH", &path);
    let mut lang = None;
    if text::needs_lang(var("LANG").as_deref(), var("LC_ALL").as_deref(), var("LC_CTYPE").as_deref()) {
        let v = login_lang
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| text::locale_for(apple_locale().as_deref(), &|v: &str| Path::new("/usr/share/locale").join(v).is_dir()));
        std::env::set_var("LANG", &v);
        lang = Some(v);
    }
    Imported { shell, path, lang, nofile, note }
}
