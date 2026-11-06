//! Pure rules and script text for the macOS side. Nothing here calls the system, so every rule
//! is tested on every system.

use std::path::{Path, PathBuf};

/// The marker printed before the login shell's PATH.
pub const PATH_START: &str = "__KANBAN_DSH_PATH_START__";
/// The marker printed after the login shell's PATH.
pub const PATH_END: &str = "__KANBAN_DSH_PATH_END__";
/// The marker printed before the login shell's LANG.
pub const LANG_START: &str = "__KANBAN_DSH_LANG_START__";
/// The marker printed after the login shell's LANG.
pub const LANG_END: &str = "__KANBAN_DSH_LANG_END__";

/// The `-c` script run in the user's login shell (`<shell> -l -c <script>`). It prints a
/// newline, then PATH and LANG each between their markers. `printenv` reads the exported
/// values, so the PATH is colon-joined in every shell (fish's own `$PATH` is a list), and the
/// script uses only `printf`, `;` and single quotes, so sh, bash, zsh, fish and tcsh all run it.
pub fn login_shell_script() -> String {
    format!(
        "printf '\\n%s' '{PATH_START}'; printenv PATH; printf '%s\\n%s' '{PATH_END}' '{LANG_START}'; printenv LANG; printf '%s\\n' '{LANG_END}'"
    )
}

fn between<'a>(out: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = out.find(start)? + start.len();
    let to = out[from..].find(end)? + from;
    let v = out[from..to].trim();
    (!v.is_empty()).then_some(v)
}

/// (PATH, LANG) from the login shell's output, each `None` when missing or empty. Text that
/// rc files print before or after the markers is ignored.
pub fn parse_login_output(out: &str) -> (Option<String>, Option<String>) {
    (
        between(out, PATH_START, PATH_END).map(str::to_string),
        between(out, LANG_START, LANG_END).map(str::to_string),
    )
}

fn absolute(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| s.starts_with('/'))
}

/// The shell to read the login PATH from: `$SHELL` if it is an absolute path, else the user
/// record's shell, else `/bin/zsh` (macOS's default since 10.15).
pub fn choose_shell(env_shell: Option<&str>, pw_shell: Option<&str>) -> String {
    absolute(env_shell).or(absolute(pw_shell)).unwrap_or("/bin/zsh").to_string()
}

/// The folders added after the login and current PATH when they exist and are missing.
fn fallback_dirs(home: &Path) -> Vec<String> {
    let home = home.to_string_lossy();
    let home = home.trim_end_matches('/');
    vec![
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        format!("{home}/.local/bin"),
        format!("{home}/.cargo/bin"),
        "/usr/bin".to_string(),
        "/bin".to_string(),
        "/usr/sbin".to_string(),
        "/sbin".to_string(),
    ]
}

/// The app's PATH: the login shell's entries first, then the current entries not already
/// there, then the fallback folders that exist and are still missing. No empty entries, no
/// duplicates, order kept.
pub fn merge_path(login: Option<&str>, current: Option<&str>, home: &Path, exists: &dyn Fn(&Path) -> bool) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut add = |e: &str| {
        if !e.is_empty() && !out.iter().any(|o| o == e) {
            out.push(e.to_string());
        }
    };
    for e in login.unwrap_or("").split(':').chain(current.unwrap_or("").split(':')) {
        add(e);
    }
    for d in fallback_dirs(home) {
        if exists(Path::new(&d)) {
            add(&d);
        }
    }
    out.join(":")
}

/// True when LANG, LC_ALL and LC_CTYPE are all unset or empty, so the app must set LANG.
pub fn needs_lang(lang: Option<&str>, lc_all: Option<&str>, lc_ctype: Option<&str>) -> bool {
    [lang, lc_all, lc_ctype].iter().all(|v| v.map_or(true, |s| s.trim().is_empty()))
}

/// LANG from macOS's AppleLocale (`defaults read -g AppleLocale`): the value cut at '@', plus
/// ".UTF-8", used only if `exists(value)` (the caller checks /usr/share/locale/<value>); else
/// "en_US.UTF-8".
pub fn locale_for(apple_locale: Option<&str>, exists: &dyn Fn(&str) -> bool) -> String {
    const DEFAULT: &str = "en_US.UTF-8";
    let Some(raw) = apple_locale else { return DEFAULT.to_string() };
    let base = raw.trim().split('@').next().unwrap_or("").trim();
    if base.is_empty() {
        return DEFAULT.to_string();
    }
    let v = format!("{base}.UTF-8");
    if exists(&v) {
        v
    } else {
        DEFAULT.to_string()
    }
}

/// The Unix rule for a `file:` URL: "file:///Users/x.png" gives "/Users/x.png" and
/// "file://localhost/Users/x" gives "/Users/x" (the scheme and host in any case); anything else
/// comes back unchanged. The caller refuses a result that starts with "//".
pub fn file_url_path(s: &str) -> &str {
    let prefix = "file://";
    if !s.get(..prefix.len()).is_some_and(|p| p.eq_ignore_ascii_case(prefix)) {
        return s;
    }
    let rest = &s[prefix.len()..];
    if rest.starts_with('/') {
        return rest;
    }
    let host = "localhost/";
    if rest.get(..host.len()).is_some_and(|h| h.eq_ignore_ascii_case(host)) {
        return &rest[host.len() - 1..];
    }
    s
}

/// The places to look for `claude`, in order, with no duplicates: the installer's and the old
/// local install's copies, Homebrew's two folders, then each desktop-app folder D (D/claude and
/// D/claude.app/Contents/MacOS/claude), then each PATH entry.
pub fn claude_candidates(home: &Path, path_var: Option<&str>, desktop: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut add = |p: PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    add(home.join(".local/bin/claude"));
    add(home.join(".claude/local/claude"));
    add(PathBuf::from("/opt/homebrew/bin/claude"));
    add(PathBuf::from("/usr/local/bin/claude"));
    for d in desktop {
        add(d.join("claude"));
        add(d.join("claude.app/Contents/MacOS/claude"));
    }
    for e in path_var.unwrap_or("").split(':').filter(|e| !e.is_empty()) {
        add(Path::new(e).join("claude"));
    }
    out
}

/// The open-files limit to set: the hard limit, at most 10240 (macOS refuses more than
/// OPEN_MAX for the soft limit), and never lower than the soft limit already is.
pub fn nofile_target(soft: u64, hard: u64) -> u64 {
    hard.min(10240).max(soft)
}

/// `s` as one sh word: in single quotes, a ' inside written as '\''.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn q(p: &Path) -> String {
    sh_quote(&p.to_string_lossy())
}

/// The update helper (POSIX sh), run in its own session by the app before it quits for
/// Restart now. It waits up to 5 minutes for the app (`pid`) to exit, unpacks the staged zip
/// with ditto beside it, moves the old bundle to <zip folder>/previous.app and the new one in
/// (putting the old one back if that fails), opens the app again, and writes every step to
/// `log` and the outcome to `result` ({"ok":bool,"error":string|null,"at":"<UTC RFC 3339>"}).
pub fn update_helper_script(pid: u32, zip: &Path, install_dir: &Path, app_bundle: &Path, log: &Path, result: &Path) -> String {
    let zip_dir = zip.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let name = app_bundle.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "app.app".to_string());
    let mut s = String::new();
    s.push_str("#!/bin/sh\n# Claude Kanban DSH update helper (card #117). Written by the app; safe to delete.\n");
    s.push_str("PATH=\"$PATH:/usr/bin:/bin\"; export PATH\n");
    s.push_str(&format!("PID={pid}\n"));
    s.push_str(&format!("ZIP={}\n", q(zip)));
    s.push_str(&format!("ZIPDIR={}\n", q(&zip_dir)));
    s.push_str(&format!("DIR={}\n", q(install_dir)));
    s.push_str(&format!("APP={}\n", q(app_bundle)));
    s.push_str(&format!("NAME={}\n", sh_quote(&name)));
    s.push_str(&format!("LOG={}\n", q(log)));
    s.push_str(&format!("RESULT={}\n", q(result)));
    s.push_str(
        r#"PREV="$ZIPDIR/previous.app"
TMP=""
now() { date -u +%Y-%m-%dT%H:%M:%SZ; }
log() { printf '%s %s\n' "$(now)" "$1" >> "$LOG" 2>/dev/null; }
json_str() { printf '%s' "$1" | tr '\n\r\t' '   ' | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'; }
result() {
  if [ -n "$2" ]; then err="\"$(json_str "$2")\""; else err=null; fi
  printf '{"ok":%s,"error":%s,"at":"%s"}\n' "$1" "$err" "$(now)" > "$RESULT.tmp" && mv -f "$RESULT.tmp" "$RESULT"
}
finish_fail() {
  log "FAILED: $1"
  result false "$1"
  if [ -n "$TMP" ]; then rm -rf "$TMP"; fi
  if [ -e "$APP" ]; then
    log "opening the app that is there: $APP"
    open "$APP" >> "$LOG" 2>&1 || log "open failed"
  fi
  exit 1
}
log "update helper started: install folder $DIR, app $APP, update $ZIP"
log "waiting for the app (pid $PID) to exit"
n=0
while kill -0 "$PID" 2>/dev/null; do
  n=$((n + 1))
  if [ "$n" -ge 300 ]; then
    log "FAILED: the app (pid $PID) did not exit within 5 minutes"
    result false "the app (pid $PID) did not exit within 5 minutes"
    exit 1
  fi
  sleep 1
done
log "the app has exited"
if ! TMP=$(mktemp -d "$ZIPDIR/unpack.XXXXXX" 2>&1); then
  e="$TMP"; TMP=""
  finish_fail "could not make a temporary folder beside the update: $e"
fi
log "unpacking into $TMP"
if ! err=$(ditto -x -k "$ZIP" "$TMP" 2>&1); then
  finish_fail "ditto could not unpack the update: $err"
fi
NEW="$TMP/$NAME"
if [ ! -d "$NEW" ]; then
  finish_fail "the update has no $NAME"
fi
rm -rf "$PREV"
moved=0
if [ -e "$APP" ]; then
  if ! err=$(mv "$APP" "$PREV" 2>&1); then
    finish_fail "could not move the old app aside: $err"
  fi
  moved=1
  log "the old app is at $PREV"
fi
mkdir -p "$DIR" 2>/dev/null
if ! err=$(mv "$NEW" "$APP" 2>&1); then
  log "could not move the new app in: $err"
  if [ "$moved" = 1 ]; then
    if e2=$(mv "$PREV" "$APP" 2>&1); then log "the old app is back"; else log "could not put the old app back: $e2"; fi
  fi
  finish_fail "could not move the new app in: $err"
fi
log "the new app is in place"
rm -rf "$TMP"
result true ""
log "opening $APP"
open "$APP" >> "$LOG" 2>&1 || log "open failed"
exit 0
"#,
    );
    s
}

/// The helper's result file: (ok, error, at). `None` when it isn't the result's JSON.
pub fn restart_result(json: &str) -> Option<(bool, Option<String>, String)> {
    let v: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    let ok = v.get("ok")?.as_bool()?;
    let error = v.get("error").and_then(|e| e.as_str()).map(str::to_string);
    let at = v.get("at").and_then(|a| a.as_str()).unwrap_or("").to_string();
    Some((ok, error, at))
}

/// What Cmd+Q does next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuitStep {
    /// No session runs: quit now.
    Quit,
    /// Bring the window forward and ask the page.
    AskPage,
    /// The page shows the question, or has had less than 3 s to: wait.
    Wait,
    /// The page did not answer within 3 s: ask with a native alert.
    NativeAlert,
}

/// The Cmd+Q rule: `running` sessions, how long ago the page was asked (None: not yet), and
/// whether the page said it shows the question.
pub fn quit_step(running: usize, asked_ms_ago: Option<u64>, page_shown: bool) -> QuitStep {
    match (running, asked_ms_ago) {
        (0, _) => QuitStep::Quit,
        (_, None) => QuitStep::AskPage,
        (_, Some(_)) if page_shown => QuitStep::Wait,
        (_, Some(ms)) if ms < 3000 => QuitStep::Wait,
        _ => QuitStep::NativeAlert,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_output_between_markers() {
        let out = format!("Welcome!\nrc noise\n\n{PATH_START}/opt/homebrew/bin:/usr/bin\n{PATH_END}\n{LANG_START}en_GB.UTF-8\n{LANG_END}\nbye\n");
        assert_eq!(parse_login_output(&out), (Some("/opt/homebrew/bin:/usr/bin".into()), Some("en_GB.UTF-8".into())));
        let empty = format!("{PATH_START}\n{PATH_END}\n{LANG_START}\n{LANG_END}\n");
        assert_eq!(parse_login_output(&empty), (None, None));
        assert_eq!(parse_login_output("nothing here"), (None, None));
        assert_eq!(parse_login_output(&format!("{PATH_START}/bin")), (None, None), "no end marker");
        let s = login_shell_script();
        assert!(s.contains(PATH_START) && s.contains(PATH_END) && s.contains(LANG_START) && s.contains(LANG_END));
        assert!(!s.contains("echo"));
    }

    /// Where sh exists (Git Bash here, every Mac), the script really prints the markers.
    #[test]
    fn login_script_runs_in_sh() {
        let Ok(out) = std::process::Command::new("sh").arg("-c").arg(login_shell_script()).env("LANG", "en_GB.UTF-8").output() else { return };
        let (path, lang) = parse_login_output(&String::from_utf8_lossy(&out.stdout));
        assert!(path.is_some_and(|p| !p.is_empty()));
        assert_eq!(lang.as_deref(), Some("en_GB.UTF-8"));
    }

    #[test]
    fn shell_choice() {
        assert_eq!(choose_shell(Some("/bin/bash"), Some("/bin/zsh")), "/bin/bash");
        assert_eq!(choose_shell(Some(""), Some("/opt/homebrew/bin/fish")), "/opt/homebrew/bin/fish");
        assert_eq!(choose_shell(Some("bash"), None), "/bin/zsh", "a relative $SHELL is not used");
        assert_eq!(choose_shell(None, Some("")), "/bin/zsh");
        assert_eq!(choose_shell(None, None), "/bin/zsh");
    }

    #[test]
    fn path_merge() {
        let home = Path::new("/Users/h");
        let all = |_: &Path| true;
        let none = |_: &Path| false;
        assert_eq!(
            merge_path(Some("/a:/b::/a"), Some("/usr/bin:/b:/c:"), home, &none),
            "/a:/b:/usr/bin:/c"
        );
        assert_eq!(
            merge_path(Some("/a"), None, home, &all),
            "/a:/opt/homebrew/bin:/usr/local/bin:/Users/h/.local/bin:/Users/h/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
        let some = |p: &Path| p == Path::new("/usr/bin") || p == Path::new("/Users/h/.cargo/bin");
        assert_eq!(merge_path(None, Some("/usr/bin:/bin"), Path::new("/Users/h/"), &some), "/usr/bin:/bin:/Users/h/.cargo/bin");
        assert_eq!(merge_path(None, None, home, &none), "");
    }

    #[test]
    fn lang_needed() {
        assert!(needs_lang(None, None, None));
        assert!(needs_lang(Some(""), Some(" "), None));
        assert!(!needs_lang(Some("en_GB.UTF-8"), None, None));
        assert!(!needs_lang(None, Some("C"), None));
        assert!(!needs_lang(None, None, Some("UTF-8")));
    }

    #[test]
    fn locale_cases() {
        let on_disk = |v: &str| ["en_GB.UTF-8", "en_US.UTF-8", "fr_FR.UTF-8"].contains(&v);
        assert_eq!(locale_for(Some("en_GB"), &on_disk), "en_GB.UTF-8");
        assert_eq!(locale_for(Some("en_GB@rg=gbzzzz"), &on_disk), "en_GB.UTF-8");
        assert_eq!(locale_for(Some("fr_FR\n"), &on_disk), "fr_FR.UTF-8");
        assert_eq!(locale_for(Some("xx_YY"), &on_disk), "en_US.UTF-8");
        assert_eq!(locale_for(None, &on_disk), "en_US.UTF-8");
        assert_eq!(locale_for(Some("en"), &on_disk), "en_US.UTF-8");
        assert_eq!(locale_for(Some("en"), &|v: &str| v == "en.UTF-8"), "en.UTF-8");
        assert_eq!(locale_for(Some("@rg=gbzzzz"), &on_disk), "en_US.UTF-8");
    }

    #[test]
    fn file_urls() {
        assert_eq!(file_url_path("file:///Users/x.png"), "/Users/x.png");
        assert_eq!(file_url_path("FILE:///Users/x.png"), "/Users/x.png");
        assert_eq!(file_url_path("file://localhost/Users/x"), "/Users/x");
        assert_eq!(file_url_path("file://LocalHost/Users/x"), "/Users/x");
        assert_eq!(file_url_path("file://host/share/x"), "file://host/share/x");
        assert_eq!(file_url_path("file:////host/x"), "//host/x", "the caller refuses //");
        assert_eq!(file_url_path("/Users/x.png"), "/Users/x.png");
        assert_eq!(file_url_path("https://x/y"), "https://x/y");
        assert_eq!(file_url_path("file:"), "file:");
        assert_eq!(file_url_path("fïle:///x"), "fïle:///x");
    }

    #[test]
    fn claude_candidate_order() {
        let home = Path::new("/Users/h");
        let desk = [PathBuf::from("/Users/h/Library/Application Support/Claude/claude-code/2.1.0")];
        let got = claude_candidates(home, Some("/usr/local/bin:/opt/x/bin::/usr/bin"), &desk);
        let want: Vec<PathBuf> = [
            "/Users/h/.local/bin/claude",
            "/Users/h/.claude/local/claude",
            "/opt/homebrew/bin/claude",
            "/usr/local/bin/claude",
            "/Users/h/Library/Application Support/Claude/claude-code/2.1.0/claude",
            "/Users/h/Library/Application Support/Claude/claude-code/2.1.0/claude.app/Contents/MacOS/claude",
            "/opt/x/bin/claude",
            "/usr/bin/claude",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn nofile() {
        assert_eq!(nofile_target(256, u64::MAX), 10240);
        assert_eq!(nofile_target(256, 4096), 4096);
        assert_eq!(nofile_target(20000, u64::MAX), 20000, "never lower");
        assert_eq!(nofile_target(256, 100), 256);
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote(""), "''");
        assert_eq!(sh_quote("$HOME `x` \"y\""), "'$HOME `x` \"y\"'");
    }

    fn helper() -> String {
        update_helper_script(
            4321,
            Path::new("/Users/h/.claude-kanban-dsh/update/Claude Kanban DSH.zip"),
            Path::new("/Users/h/Applications"),
            Path::new("/Users/h/Applications/Claude Kanban DSH.app"),
            Path::new("/Users/h/.claude-kanban-dsh/update/restart.log"),
            Path::new("/Users/h/.claude-kanban-dsh/update/restart-result.json"),
        )
    }

    #[test]
    fn helper_script_text() {
        let s = helper();
        assert!(s.starts_with("#!/bin/sh\n"));
        assert!(s.contains("PATH=\"$PATH:/usr/bin:/bin\"; export PATH"));
        assert!(s.contains("PID=4321\n"));
        assert!(s.contains("APP='/Users/h/Applications/Claude Kanban DSH.app'\n"));
        assert!(s.contains("NAME='Claude Kanban DSH.app'\n"));
        assert!(s.contains("ZIPDIR='/Users/h/.claude-kanban-dsh/update'\n"));
        assert!(s.contains("ditto -x -k \"$ZIP\" \"$TMP\""));
        assert!(s.contains("err=$(mv \"$NEW\" \"$APP\" 2>&1)"));
        assert!(s.contains("open \"$APP\""));
        assert!(!s.contains("open -g"), "the app opens in front");
        assert!(!s.contains("/usr/bin/open") && !s.contains("/usr/bin/ditto"), "from PATH, so a test can stub them");
        let quoted = update_helper_script(1, Path::new("/tmp/it's/u.zip"), Path::new("/a"), Path::new("/a/x.app"), Path::new("/l"), Path::new("/r"));
        assert!(quoted.contains("ZIP='/tmp/it'\\''s/u.zip'\n"));
    }

    /// The helper parses as sh where sh exists (Git Bash here, every Mac); skipped otherwise.
    #[test]
    fn helper_script_passes_sh_n() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("helper.sh");
        std::fs::write(&f, helper()).unwrap();
        let Ok(out) = std::process::Command::new("sh").arg("-n").arg(&f).output() else { return };
        assert!(out.status.success(), "sh -n: {}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn restart_results() {
        assert_eq!(restart_result(r#"{"ok":true,"error":null,"at":"2026-11-06T10:00:00Z"}"#), Some((true, None, "2026-11-06T10:00:00Z".into())));
        assert_eq!(
            restart_result("{\"ok\":false,\"error\":\"mv: rename x: Permission denied\",\"at\":\"2026-11-06T10:00:00Z\"}\n"),
            Some((false, Some("mv: rename x: Permission denied".into()), "2026-11-06T10:00:00Z".into()))
        );
        assert_eq!(restart_result(r#"{"ok":true,"extra":1}"#), Some((true, None, String::new())));
        assert_eq!(restart_result(r#"{"error":"x"}"#), None);
        assert_eq!(restart_result("not json"), None);
    }

    #[test]
    fn quit_steps() {
        assert_eq!(quit_step(0, None, false), QuitStep::Quit);
        assert_eq!(quit_step(0, Some(5000), false), QuitStep::Quit);
        assert_eq!(quit_step(2, None, false), QuitStep::AskPage);
        assert_eq!(quit_step(2, Some(10_000), true), QuitStep::Wait);
        assert_eq!(quit_step(2, Some(2999), false), QuitStep::Wait);
        assert_eq!(quit_step(2, Some(3000), false), QuitStep::NativeAlert);
        assert_eq!(quit_step(1, Some(60_000), false), QuitStep::NativeAlert);
    }
}
