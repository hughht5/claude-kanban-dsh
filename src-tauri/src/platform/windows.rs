//! The Windows side of the platform layer (card #114). Each part was moved here from the file
//! named above it, as it was: Windows behaves exactly as before the move.

use crate::place::Rect;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, RunEvent, WebviewWindow, Window};
use tauri_winrt_notification::Toast;

// --- Start-up and run events (main.rs): nothing to do on Windows ---------------------------

pub fn at_start() {}

pub fn on_run_event(_app: &AppHandle, _event: &RunEvent) {}

/// The app's builder before the plugins are added (card #117 sets macOS launch options here).
pub fn builder<R: tauri::Runtime>(b: tauri::Builder<R>) -> tauri::Builder<R> {
    b
}

/// The tray icon as tray.rs builds it.
pub fn tray_style<R: tauri::Runtime>(b: tauri::tray::TrayIconBuilder<R>) -> tauri::tray::TrayIconBuilder<R> {
    b
}

/// The count of cards needing Hugh is in the tooltip only.
pub fn tray_count<R: tauri::Runtime>(_tray: &tauri::tray::TrayIcon<R>, _n: usize) {}

/// A left click on the tray icon shows the window.
pub const TRAY_LEFT_CLICK_SHOWS: bool = true;

/// Account 2 runs on Windows.
pub const SECOND_ACCOUNT: bool = true;

/// Dev stacks run in WSL, so a `/` checkout path is a WSL one.
pub const DEV_STACKS: bool = true;

/// The profile folder is the user's own already, so a plain write.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

// --- Card sessions end with the app (from sessions.rs) -------------------------------------

use ::windows::Win32::Foundation::{CloseHandle, HANDLE};
use ::windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use ::windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SET_QUOTA, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE};

/// Opens the process now, so a reused pid can't fool it, and returns a wait for its exit.
pub fn exit_waiter(pid: u32) -> Option<impl FnOnce() + Send> {
    let h = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.ok()?;
    let raw = h.0 as isize;
    Some(move || unsafe {
        let h = HANDLE(raw as *mut core::ffi::c_void);
        WaitForSingleObject(h, INFINITE);
        let _ = CloseHandle(h);
    })
}

/// The Job Object that ends every card session when the app's process ends. Held for the
/// app's whole life and never closed, so Windows closes it, and kills the job's processes,
/// only when the app's process ends.
pub struct Job(isize);

impl Job {
    pub fn new() -> Result<Job, String> {
        unsafe {
            let h = CreateJobObjectW(None, None).map_err(|e| format!("job object: {e}"))?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|e| format!("job object limits: {e}"))?;
            Ok(Job(h.0 as isize))
        }
    }

    pub fn assign(&self, pid: u32) -> Result<(), String> {
        unsafe {
            let p = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid).map_err(|e| format!("open claude's process: {e}"))?;
            let r = AssignProcessToJobObject(HANDLE(self.0 as *mut core::ffi::c_void), p);
            let _ = CloseHandle(p);
            r.map_err(|e| format!("put claude in the job object: {e}"))
        }
    }
}

// --- Helper processes without a console window (sessions.rs, slash.rs, stack.rs) -----------

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `command.no_window()`: the process starts without a console window of its own.
pub trait NoWindow {
    fn no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn no_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(CREATE_NO_WINDOW)
    }
}

// --- Where Claude Code and Chrome are (sessions.rs, launch.rs) -----------------------------

/// The places `claude.exe` may be: the native install in `~\.local\bin`, `~\bin`, and the
/// desktop app's copies. Each is asked its version; the newest is used.
pub fn claude_candidates(home: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![home.join(".local").join("bin").join("claude.exe"), home.join("bin").join("claude.exe")];
    if let Some(appdata) = std::env::var_os("APPDATA") {
        if let Ok(dirs) = std::fs::read_dir(PathBuf::from(appdata).join("Claude").join("claude-code")) {
            candidates.extend(dirs.flatten().map(|d| d.path().join("claude.exe")));
        }
    }
    candidates
}

/// The folder used as home when `USERPROFILE` isn't set.
pub fn no_home() -> PathBuf {
    PathBuf::from("C:\\")
}

pub fn chrome_paths() -> Vec<PathBuf> {
    let mut places = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            places.push(PathBuf::from(base).join("Google").join("Chrome").join("Application").join("chrome.exe"));
        }
    }
    places
}

// --- Toasts (from notify.rs) ---------------------------------------------------------------
//
// The notification plugin is registered, but its desktop side drops the toast's click, so the
// toasts are shown here with `tauri-winrt-notification` (the library the plugin uses).

/// The toast's app id: the installed app's own (its Start menu shortcut carries it), or
/// PowerShell's for a build run from `target`, the same rule the notification plugin uses.
fn app_id(app: &AppHandle) -> String {
    let dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.display().to_string())).unwrap_or_default();
    let sep = std::path::MAIN_SEPARATOR;
    if dir.ends_with(&format!("{sep}target{sep}debug")) || dir.ends_with(&format!("{sep}target{sep}release")) {
        Toast::POWERSHELL_APP_ID.to_string()
    } else {
        app.config().identifier.clone()
    }
}

/// Shows a toast. Clicking it brings the window forward and opens that card's panel (the page
/// handles `open-card`).
pub fn toast(app: &AppHandle, card_id: &str, head: &str, body: &str) {
    let id = app_id(app);
    let (a, card, head, body) = (app.clone(), card_id.to_string(), head.to_string(), body.to_string());
    std::thread::spawn(move || {
        let clicked = a.clone();
        let result = Toast::new(&id)
            .title(&head)
            .text1(&body)
            .on_activated(move |_| {
                crate::tray::show_main(&clicked);
                // No card (card #104's small-change notices): a click shows the board only.
                if !card.is_empty() {
                    let _ = clicked.emit("open-card", card.clone());
                }
                Ok(())
            })
            .show();
        if let Err(e) = result {
            eprintln!("toast: {e}");
        }
    });
}

// --- The board window (from place.rs and tray.rs) ------------------------------------------

/// The screen's work area, in physical pixels.
pub fn work_area(_w: &WebviewWindow) -> Rect {
    use ::windows::Win32::Foundation::RECT;
    use ::windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut r = RECT::default();
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut RECT as *mut core::ffi::c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    if ok.is_ok() && r.right > r.left {
        Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
    } else {
        Rect { x: 0, y: 0, w: 1440, h: 900 }
    }
}

/// The whole desktop across every screen.
pub fn virtual_screen(_w: &WebviewWindow) -> Rect {
    use ::windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
    unsafe {
        Rect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            w: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            h: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

/// Puts the window at `r` and shows it: behind everything and not activated with
/// `background`, else in front. Runs on the main thread.
pub fn place_window(w: &WebviewWindow, r: Rect, background: bool) {
    if let Ok(h) = w.hwnd() {
        use ::windows::Win32::Foundation::HWND;
        use ::windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_BOTTOM, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW};
        let hwnd = HWND(h.0 as *mut core::ffi::c_void);
        let (after, flags) = if background { (HWND_BOTTOM, SWP_SHOWWINDOW | SWP_NOACTIVATE) } else { (HWND_TOP, SWP_SHOWWINDOW) };
        unsafe {
            let _ = SetWindowPos(hwnd, after, r.x, r.y, r.w, r.h, flags);
        }
        if !background {
            bring_forward(hwnd);
        }
    }
}

/// Makes the window the foreground window. Windows refuses `SetForegroundWindow` to a process
/// that wasn't started by the foreground one, which is how the app comes back after Restart now
/// (a hidden helper starts it once the old copy has gone), so a refused call only flashes the
/// taskbar. Then it taps Alt first and asks again, as the window framework does when it creates
/// a focused window: Windows counts the tap as the user's own input. (Not `AttachThreadInput`:
/// sharing a hung app's input queue would hang the board's UI thread.)
fn bring_forward(hwnd: ::windows::Win32::Foundation::HWND) {
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VK_LMENU, VK_MENU,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};
    unsafe {
        if GetForegroundWindow() == hwnd || SetForegroundWindow(hwnd).as_bool() {
            return;
        }
        let scan = MapVirtualKeyW(u32::from(VK_MENU.0), MAPVK_VK_TO_VSC) as u16;
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_LMENU, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        };
        let taps = [key(KEYEVENTF_EXTENDEDKEY), key(KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP)];
        SendInput(&taps, std::mem::size_of::<INPUT>() as i32);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Hides the window after the framework's own hide(). The window is first shown with
/// SetWindowPos (place_window), which the window framework doesn't see, so it still thinks the
/// window is hidden and its hide() does nothing. Hide it directly too.
pub fn hide_window(window: &Window) {
    if let Ok(h) = window.hwnd() {
        use ::windows::Win32::Foundation::HWND;
        use ::windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
        unsafe {
            let _ = ShowWindow(HWND(h.0 as *mut core::ffi::c_void), SW_HIDE);
        }
    }
}

// --- The install script's quit signal (from tray.rs) ---------------------------------------

/// Waits on a named Windows event so `scripts\install.ps1` can ask for a clean quit, the same
/// way tray > Quit does. Its name has a `Test` suffix in the test build, so each channel's
/// install quits only its own app.
pub fn on_quit_signal(app: AppHandle, quit: fn(&AppHandle)) {
    wait_for_event(kanban_store::quit_event(), move || quit(&app));
}

/// Waits, on a thread of its own, for the named manual-reset event `name` to be set, then
/// calls `then`.
fn wait_for_event(name: &str, then: impl FnOnce() + Send + 'static) {
    use ::windows::core::HSTRING;
    use ::windows::Win32::System::Threading::{CreateEventW, ResetEvent};
    let Ok(event) = (unsafe { CreateEventW(None, true, false, &HSTRING::from(name)) }) else { return };
    // The event is manual-reset and named, so if the copy before this one is still shutting
    // down, it is still set from that copy's quit. Clear it, or this copy would quit at once.
    let _ = unsafe { ResetEvent(event) };
    let raw = event.0 as isize;
    std::thread::spawn(move || {
        let event = HANDLE(raw as _);
        unsafe { WaitForSingleObject(event, INFINITE) };
        then();
    });
}

// --- Restart now's helper (from update.rs) -------------------------------------------------

/// A PowerShell literal string: single quotes, with any inside doubled.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The helper's script: wait for the app (pid) to exit, run the installer silently into the
/// install folder, then start the app again whatever the installer did, so Hugh is never left
/// without the board. It starts in front, not with `--background`: Hugh clicked Restart now, so
/// he is looking at the board and expects it back. Each step is logged to `log`.
/// With `background` (a test app's Restart now, card #135) the app starts with `--background`,
/// behind whatever is in front; otherwise it comes back in front.
pub fn update_helper(pid: u32, installer: &Path, install_dir: &Path, app_exe: &Path, log: &Path, background: bool) -> String {
    let log = ps_quote(&log.display().to_string());
    let setup = ps_quote(&installer.display().to_string());
    // /D must come last and unquoted, even with spaces (NSIS).
    let d_arg = ps_quote(&format!("/D={}", install_dir.display()));
    let dir = ps_quote(&install_dir.display().to_string());
    let exe = ps_quote(&app_exe.display().to_string());
    let bg = if background { " -ArgumentList '--background'" } else { "" };
    // restart.log names the flag too, so a tester can see it there.
    let bg_note = if background { " + ' --background'" } else { "" };
    format!(
        "$ErrorActionPreference = 'Continue'\n\
         $log = {log}\n\
         function Note($t) {{ \"$(Get-Date -Format o) $t\" | Out-File -FilePath $log -Append -Encoding utf8 }}\n\
         Note 'Waiting for the app (pid {pid}) to exit'\n\
         try {{ Wait-Process -Id {pid} -Timeout 300 -ErrorAction Stop }} catch {{}}\n\
         if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ Note 'The app is still running after 5 minutes; not installing.'; exit 1 }}\n\
         Note ('Installing ' + {setup})\n\
         try {{\n\
           $p = Start-Process -FilePath {setup} -ArgumentList '/S', {d_arg} -Wait -PassThru\n\
           Note \"Installer exit code $($p.ExitCode)\"\n\
         }} catch {{ Note \"Installer failed to start: $_\" }}\n\
         Note ('Starting ' + {exe}{bg_note})\n\
         Start-Process -FilePath {exe} -WorkingDirectory {dir}{bg}\n\
         Note 'Done'\n"
    )
}

/// Starts PowerShell with no window, in its own process group and (when allowed) outside any
/// job this app is in, so it outlives the app.
pub fn spawn_detached(script: &str) -> Result<(), String> {
    use base64::Engine;
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    // -EncodedCommand takes the script as base64 of UTF-16LE, so no quoting can go wrong.
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let run = |flags: u32| {
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-EncodedCommand", &encoded])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(flags)
            .spawn()
    };
    run(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB)
        .or_else(|_| run(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP))
        .map(|_| ())
        .map_err(|e| format!("couldn't start the update helper: {e}"))
}

// --- file:// URLs (from session_cmds.rs) ---------------------------------------------------

/// The path in a `file:///C:/...` URL (`C:/...`); anything else as it is.
pub fn file_url_path(path: &str) -> &str {
    match path.get(..8) {
        Some(head) if head.eq_ignore_ascii_case("file:///") => &path[8..],
        _ => path,
    }
}

// --- The shared test-app lock files (from testapp.rs) ---------------------------------------

/// `share_all(&mut options)`: a file opened with every kind of sharing (a holder may have it
/// open for writing and delete-on-close).
pub fn share_all(o: &mut std::fs::OpenOptions) {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
    o.share_mode(7);
}

/// A lock file's writer is checked by its pid and start time (`process_start`). Off Windows
/// the file's presence is all the test app's lock check reads.
pub const PID_LOCKS: bool = true;

/// Windows holds install.lock by its open handle and deletes it on close, so its presence and
/// its writer's pid tell (testapp.rs): nothing more to check here (card #117's Mac uses flock).
pub fn file_lock_held(_path: &Path) -> Option<bool> {
    None
}

/// The start time of process `pid`, if it is running.
pub fn process_start(pid: u32) -> Option<chrono::DateTime<chrono::Utc>> {
    use ::windows::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
    use ::windows::Win32::System::Threading::{GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut code = 0u32;
        let alive = GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
        let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let got = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok();
        let _ = CloseHandle(h);
        if !alive || !got {
            return None;
        }
        // 100 ns ticks since 1601.
        let ticks = ((c.dwHighDateTime as i64) << 32) | c.dwLowDateTime as i64;
        let secs = ticks / 10_000_000 - 11_644_473_600;
        chrono::DateTime::from_timestamp(secs, ((ticks % 10_000_000) * 100) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn helper_quotes_paths() {
        let s = update_helper(
            42,
            Path::new(r"C:\Users\o'neil\.claude-kanban-dsh\update\Claude Kanban DSH_0.1.0_x64-setup.exe"),
            Path::new(r"C:\Users\o'neil\Programs\Claude Kanban DSH"),
            Path::new(r"C:\Users\o'neil\Programs\Claude Kanban DSH\claude-kanban-dsh.exe"),
            Path::new(r"C:\Users\o'neil\.claude-kanban-dsh\update\restart.log"),
            false,
        );
        assert!(s.contains("Wait-Process -Id 42"));
        assert!(s.contains(r"'/D=C:\Users\o''neil\Programs\Claude Kanban DSH'"));
        assert!(s.contains(r"-FilePath 'C:\Users\o''neil\Programs\Claude Kanban DSH\claude-kanban-dsh.exe'"));
        // Hugh clicked for the restart, so the app comes back in front, not in the background.
        assert!(!s.contains("--background"), "the restarted app must not start in the background");
        // The installer runs only after the wait, and the app starts after the installer.
        let (w, i, r) = (s.find("Wait-Process").unwrap(), s.find("'/S'").unwrap(), s.rfind("Start-Process -FilePath 'C:").unwrap());
        assert!(w < i && i < r);
    }

    /// The helper must be valid PowerShell: parse it (never run it) with PowerShell's own parser.
    #[test]
    fn helper_parses_as_powershell() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("helper.ps1");
        for background in [false, true] {
            let s = update_helper(7, Path::new(r"C:\a b\setup.exe"), Path::new(r"C:\a b\App"), Path::new(r"C:\a b\App\x.exe"), Path::new(r"C:\a b\r.log"), background);
            std::fs::write(&file, s).unwrap();
            let cmd = format!(
                "$e=$null;$t=$null;[void][System.Management.Automation.Language.Parser]::ParseFile('{}',[ref]$t,[ref]$e);$e.Count",
                file.display()
            );
            let Ok(out) = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &cmd]).output() else {
                return; // no PowerShell here
            };
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0", "parse errors in:\n{}", std::fs::read_to_string(&file).unwrap());
        }
    }

    /// A test app's Restart now starts the app with --background, after the installer (card #135).
    #[test]
    fn helper_starts_a_test_app_in_the_background() {
        let s = update_helper(7, Path::new(r"C:\a b\setup.exe"), Path::new(r"C:\a b\App"), Path::new(r"C:\a b\App\x.exe"), Path::new(r"C:\a b\r.log"), true);
        let start = s.rfind(r"Start-Process -FilePath 'C:\a b\App\x.exe'").expect("the app's start");
        let line = s[start..].lines().next().unwrap();
        assert!(line.ends_with("-ArgumentList '--background'"), "{line}");
        assert!(s.find("'/S'").unwrap() < start, "the installer runs first");
        assert_eq!(s.matches("--background").count(), 2, "the app's start and its log line");
        assert!(s.contains(r"Note ('Starting ' + 'C:\a b\App\x.exe' + ' --background')"), "restart.log names the flag");
    }

    #[test]
    fn file_urls_lose_their_scheme() {
        assert_eq!(file_url_path("file:///C:/tmp/a.png"), "C:/tmp/a.png");
        assert_eq!(file_url_path("FILE:///C:/tmp/a.png"), "C:/tmp/a.png");
        assert_eq!(file_url_path(r"C:\tmp\a.png"), r"C:\tmp\a.png");
        assert_eq!(file_url_path("file:"), "file:");
    }

    /// A process that runs for about `secs` seconds, with no window.
    fn sleeper(secs: u32) -> std::process::Child {
        Command::new("ping")
            .args(["-n", &(secs + 1).to_string(), "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .no_window()
            .spawn()
            .unwrap()
    }

    /// Whether process `pid` ends within `limit` (true if it is gone already).
    fn ends_within(pid: u32, limit: Duration) -> bool {
        let Some(wait) = exit_waiter(pid) else { return true };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            wait();
            let _ = tx.send(());
        });
        rx.recv_timeout(limit).is_ok()
    }

    #[test]
    fn exit_waiter_returns_when_the_process_exits() {
        let mut quick = sleeper(1);
        assert!(ends_within(quick.id(), Duration::from_secs(20)), "a 1 s process ended");
        let _ = quick.wait();
        let mut slow = sleeper(30);
        assert!(!ends_within(slow.id(), Duration::from_millis(500)), "a 30 s process is still running");
        let _ = slow.kill();
        let _ = slow.wait();
    }

    const OWNER: &str = "KANBAN_JOB_OWNER_TEST";

    /// The owner for `job_ends_its_processes_when_its_owner_exits`: run only as that test's child
    /// process. Puts a long-running process in a Job, prints its pid and exits.
    #[test]
    #[ignore = "run by job_ends_its_processes_when_its_owner_exits"]
    fn job_owner() {
        if std::env::var_os(OWNER).is_none() {
            return;
        }
        let job = Job::new().unwrap();
        let child = sleeper(60);
        job.assign(child.id()).unwrap();
        println!("SLEEPER_PID={}", child.id());
    }

    /// The Job ends the processes in it when the process that holds it exits, as the app's
    /// sessions end when the app does.
    #[test]
    fn job_ends_its_processes_when_its_owner_exits() {
        use std::io::BufRead;
        let mut owner = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "platform::windows::tests::job_owner", "--ignored", "--nocapture", "--test-threads=1"])
            .env(OWNER, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // Read only up to the pid, then wait for the owner itself: the sleeper inherits the
        // owner's stdout, so waiting for the pipe to close would wait for the sleeper too.
        let mut seen = String::new();
        let pid: u32 = std::io::BufReader::new(owner.stdout.take().unwrap())
            .lines()
            .map_while(Result::ok)
            .inspect(|l| seen.push_str(&format!("{l}\n")))
            // libtest prints the test's name on the same line, before the pid.
            .find_map(|l| l.split_once("SLEEPER_PID=").and_then(|(_, p)| p.split_whitespace().next()?.parse().ok()))
            .unwrap_or_else(|| panic!("the owner printed no pid:\n{seen}"));
        owner.wait().unwrap();
        let ended = ends_within(pid, Duration::from_secs(10));
        if !ended {
            let _ = Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).no_window().output();
        }
        assert!(ended, "the Job's process {pid} outlived its owner");
    }

    /// The quit signal fires when its named event is set. A name of its own, never the live or
    /// test app's (setting those would quit a running app).
    #[test]
    fn named_event_calls_back_when_set() {
        use ::windows::core::HSTRING;
        use ::windows::Win32::System::Threading::{CreateEventW, SetEvent};
        let name = format!(r"Local\ClaudeKanbanDshQuitUnitTest-{}", uuid::Uuid::new_v4().simple());
        assert!(!name.contains("ClaudeKanbanDshQuitTest") && name != kanban_store::quit_event());
        let (tx, rx) = mpsc::channel();
        wait_for_event(&name, move || {
            let _ = tx.send(());
        });
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "not before the event is set");
        unsafe {
            let h = CreateEventW(None, true, false, &HSTRING::from(name.as_str())).unwrap();
            SetEvent(h).unwrap();
            let _ = CloseHandle(h);
        }
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "the callback ran once the event was set");
    }
}

/// The command a card session runs to install its build into test app `channel` (card #147).
pub fn install_command(channel: &str) -> String {
    format!("powershell -ExecutionPolicy Bypass -File scripts/install.ps1 -Channel {channel}")
}
