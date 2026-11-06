//! macOS system calls: process listing and start times (libproc), waiting for an exit (waitid,
//! kqueue), the open-files limit, the user record, detached helpers and private files.

use crate::track::Proc;
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

pub use crate::lock::flock_nb;

fn bsd_info(pid: i32) -> Option<libc::proc_bsdinfo> {
    if pid <= 0 {
        return None;
    }
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: proc_bsdinfo is plain data; proc_pidinfo writes at most `size` bytes into it.
    unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let n = libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size);
        (n == size).then_some(info)
    }
}

fn start_us(info: &libc::proc_bsdinfo) -> u64 {
    info.pbi_start_tvsec.saturating_mul(1_000_000).saturating_add(info.pbi_start_tvusec)
}

/// A process's start time in microseconds since the epoch, or None when it isn't there (or
/// isn't readable).
pub fn process_start(pid: i32) -> Option<u64> {
    bsd_info(pid).map(|i| start_us(&i))
}

/// Every process of this user, now, with its session ID. Processes that end while the list is
/// read are left out.
pub fn all_procs() -> Vec<(Proc, i32)> {
    // SAFETY: a null buffer asks only for the count; then the buffer is valid for its size.
    let pids: Vec<libc::pid_t> = unsafe {
        let n = libc::proc_listallpids(std::ptr::null_mut(), 0);
        if n <= 0 {
            return Vec::new();
        }
        let mut buf: Vec<libc::pid_t> = vec![0; n as usize + 64];
        let bytes = (buf.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        let got = libc::proc_listallpids(buf.as_mut_ptr().cast(), bytes);
        if got <= 0 {
            return Vec::new();
        }
        buf.truncate((got as usize).min(buf.len()));
        buf
    };
    // SAFETY: getuid has no preconditions.
    let me = unsafe { libc::getuid() };
    let mut out = Vec::with_capacity(pids.len());
    for pid in pids.into_iter().filter(|p| *p > 0) {
        let Some(info) = bsd_info(pid) else { continue };
        if info.pbi_uid != me {
            continue;
        }
        // SAFETY: getsid only reads the pid.
        let sid = unsafe { libc::getsid(pid) };
        if sid < 0 {
            continue;
        }
        out.push((Proc { pid, start: start_us(&info) }, sid));
    }
    out
}

/// The process runs, and is the same process (same start time).
pub fn alive(p: &Proc) -> bool {
    process_start(p.pid) == Some(p.start)
}

/// Block until the child `pid` exits, without reaping it (WNOWAIT), so its exit status stays
/// for `Child::try_wait`. Returns at once if it isn't a child (ECHILD).
pub fn wait_exit(pid: i32) {
    loop {
        // SAFETY: siginfo_t is plain data that waitid fills.
        let r = unsafe {
            let mut info: libc::siginfo_t = std::mem::zeroed();
            libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT)
        };
        if r == 0 {
            return;
        }
        if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return;
        }
    }
}

/// Block until the process `pid` (any process, not only a child) exits: kqueue EVFILT_PROC
/// NOTE_EXIT. Returns at once if it has already gone (ESRCH). If kqueue fails otherwise, it
/// polls every half second.
pub fn watch_exit(pid: i32) {
    // SAFETY: kqueue and kevent get valid pointers to one event; the queue is closed at the end.
    unsafe {
        let kq = libc::kqueue();
        if kq >= 0 {
            let mut ev: libc::kevent = std::mem::zeroed();
            ev.ident = pid as libc::uintptr_t;
            ev.filter = libc::EVFILT_PROC;
            ev.flags = libc::EV_ADD | libc::EV_ONESHOT;
            ev.fflags = libc::NOTE_EXIT;
            let r = libc::kevent(kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null());
            if r < 0 {
                let e = io::Error::last_os_error().raw_os_error();
                libc::close(kq);
                if e == Some(libc::ESRCH) {
                    return;
                }
            } else {
                loop {
                    let mut out: libc::kevent = std::mem::zeroed();
                    let n = libc::kevent(kq, std::ptr::null(), 0, &mut out, 1, std::ptr::null());
                    if n > 0 {
                        libc::close(kq);
                        return;
                    }
                    if n < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                        break;
                    }
                }
                libc::close(kq);
            }
        }
        while libc::kill(pid, 0) == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}

/// Raise the open-files soft limit to `text::nofile_target` (launchd starts apps at 256).
/// Returns (before, after).
pub fn raise_nofile() -> (u64, u64) {
    // SAFETY: rlimit is plain data.
    unsafe {
        let mut rl: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rl) != 0 {
            return (0, 0);
        }
        let before = rl.rlim_cur as u64;
        let target = crate::text::nofile_target(before, rl.rlim_max as u64);
        if target > before {
            let new = libc::rlimit { rlim_cur: target as libc::rlim_t, rlim_max: rl.rlim_max };
            libc::setrlimit(libc::RLIMIT_NOFILE, &new);
        }
        let mut now: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut now) != 0 {
            return (before, before);
        }
        (before, now.rlim_cur as u64)
    }
}

/// The user record's login shell (getpwuid(getuid()).pw_shell).
pub fn user_shell() -> Option<String> {
    // SAFETY: getpwuid returns null or a record valid until the next call on this thread;
    // the shell is copied out at once. Called at start, before other threads.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() || (*pw).pw_shell.is_null() {
            return None;
        }
        let s = std::ffi::CStr::from_ptr((*pw).pw_shell).to_string_lossy().into_owned();
        (!s.trim().is_empty()).then_some(s)
    }
}

/// Run `/bin/sh -c script` in its own session (setsid), with no stdin, stdout or stderr, and
/// don't wait for it: it outlives the app.
pub fn spawn_detached_sh(script: &str) -> Result<(), String> {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(script).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe, so it may run between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn().map(drop).map_err(|e| format!("could not start /bin/sh: {e}"))
}

/// Create the folder (and its parents) and set it to 0700, also when it already exists.
pub fn private_dir(p: &Path) -> io::Result<()> {
    std::fs::create_dir_all(p)?;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))
}

/// Write the file as 0600: a temp file beside it, then a rename over it.
pub fn write_private(p: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = p.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let write = || -> io::Result<()> {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, p)
    };
    let r = write();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}
