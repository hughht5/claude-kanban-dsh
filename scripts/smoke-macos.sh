#!/bin/sh
# The macOS smoke test (card #117): runs the installed smoke build ("Claude Kanban DSH Smoke", the
# test channel under its own name and identifier) under a throwaway HOME, with a fake claude
# (scripts/smoke/fake-claude.sh), and checks what a unit test can't.
#
#   sh scripts/smoke-macos.sh "<path to Claude Kanban DSH Smoke.app>" [--part launch1|all] [--prompts-check]
#   sh scripts/smoke-macos.sh "<path to Claude Kanban DSH Smoke.app>" --part lock [--kb <path to kb>]
#
#   --part launch1   launch 1 only (the binary started directly, then again and killed)
#   --part all       launch 1, then launch 2 (open -g, launchd's bare PATH, Apple Event quit)
#   --part lock      the test-app lock kill test: install.sh --lock-only killed, then taken again
#   --prompts-check  in launch 2, a dry-run card moved to Planning, then Human test and back: the fake
#                    claude gets must not name a C:\ path (#114's {prompts} fix)
#   --kb <path>      the kb for the lock test (default: the bundle's)
#
# It prints one line per check: "PASS <check>", "FAIL <check>: <why>" or "SKIP <check>: <why>",
# and exits 1 if any check failed. Every wait has a time limit. At the end it stops whatever
# it started and removes the throwaway HOME (SMOKE_KEEP=1 keeps it). It uploads nothing: the
# app's macos.log and api.log, update/restart.log and the apps' output are copied into
# $SMOKE_LOG_DIR (default: a folder beside the throwaway HOME) for CI to upload. api.json and
# the session settings files are never copied: they hold the app's token.
#
# It refuses to start while a smoke app runs. The smoke identifier never meets Hugh's live or
# test app, so it can run on his Mac too. Launch 2 needs macOS 13 or later (open --env).

set -u

# This test's own kb and app calls must not look like a card session's.
unset KANBAN_CARD KANBAN_SESSION KANBAN_API KANBAN_LOCK_OWNER CLAUDECODE

ROOT=$(cd "$(dirname "$0")/.." && pwd -P)

APP=""
PART=all
PROMPTS=0
KB_OPT=""
while [ $# -gt 0 ]; do
    case "$1" in
        --part)
            [ $# -ge 2 ] || { echo "smoke-macos.sh: --part needs launch1, all or lock" >&2; exit 2; }
            PART=$2
            shift 2
            ;;
        --prompts-check) PROMPTS=1; shift ;;
        --kb)
            [ $# -ge 2 ] || { echo "smoke-macos.sh: --kb needs a path" >&2; exit 2; }
            KB_OPT=$2
            shift 2
            ;;
        -h | --help) sed -n '2,25p' "$0"; exit 0 ;;
        -*) echo "smoke-macos.sh: unknown option $1" >&2; exit 2 ;;
        *)
            [ -z "$APP" ] || { echo "smoke-macos.sh: one app path only" >&2; exit 2; }
            APP=$1
            shift
            ;;
    esac
done
case "$PART" in
    launch1 | all | lock) ;;
    *) echo "smoke-macos.sh: --part is launch1, all or lock, not $PART" >&2; exit 2 ;;
esac

FAILS=0
pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s: %s\n' "$1" "$2"; FAILS=$((FAILS + 1)); }
skip() { printf 'SKIP %s: %s\n' "$1" "$2"; }
# Notes go to stderr, so a note inside $(...) is never taken as a value.
note() { printf '     %s\n' "$*" >&2; }
stop_now() { printf 'FAIL %s: %s\n' "$1" "$2"; exit 1; }

[ "$(uname -s)" = "Darwin" ] || stop_now preflight "this is for macOS only"
[ -n "$APP" ] || stop_now preflight "pass the path of the installed Claude Kanban DSH Smoke.app"
[ -d "$APP" ] || stop_now preflight "no app at $APP"
APP=$(cd "$APP" && pwd -P)
PLIST="$APP/Contents/Info.plist"
BIN_NAME=$(plutil -extract CFBundleExecutable raw -o - "$PLIST" 2>/dev/null) || stop_now preflight "no CFBundleExecutable in $PLIST"
IDENT=$(plutil -extract CFBundleIdentifier raw -o - "$PLIST" 2>/dev/null) || stop_now preflight "no CFBundleIdentifier in $PLIST"
BIN="$APP/Contents/MacOS/$BIN_NAME"
KB="$APP/Contents/MacOS/kb"
[ -x "$BIN" ] || stop_now preflight "no executable at $BIN"
[ -x "$KB" ] || stop_now preflight "no kb at $KB"
case "$IDENT" in
    *.smoke) ;;
    *) stop_now preflight "$IDENT isn't the smoke build (run this only on Claude Kanban DSH Smoke.app, never on the live or test app)" ;;
esac

# --- Helpers -----------------------------------------------------------------------------------

ere_escape() {
    printf '%s' "$1" | sed 's/[][\.*^$+?(){}|]/\\&/g'
}

# Runs "$@" every half second until it succeeds or $1 seconds pass.
wait_for() {
    wf_limit=$1
    shift
    wf_end=$(($(date +%s) + wf_limit))
    while :; do
        if "$@"; then return 0; fi
        [ "$(date +%s)" -lt "$wf_end" ] || return 1
        sleep 0.5
    done
}

# A process that is gone, or a zombie nobody has reaped yet.
dead() {
    dd_stat=$(ps -o stat= -p "$1" 2>/dev/null) || return 0
    case "$dd_stat" in
        '' | Z*) return 0 ;;
        *) return 1 ;;
    esac
}
alive() { ! dead "$1"; }

file_mode() { stat -f %Lp "$1" 2>/dev/null; }

# --- The throwaway HOME ------------------------------------------------------------------------

T=$(mktemp -d "${TMPDIR:-/tmp}/kanban-smoke.XXXXXX") || stop_now preflight "mktemp failed"
T=$(cd "$T" && pwd -P)
case "$T" in
    *' '*) stop_now preflight "the temporary folder has a space in its path ($T)" ;;
esac
DATA="$T/.claude-kanban-dsh-test"
LOGS=${SMOKE_LOG_DIR:-"$T-logs"}
mkdir -p "$LOGS"
T_ERE=$(ere_escape "$T")
STARTED=""
TEXTEDIT_OPENED=0

kbt() { HOME="$T" "$KB" "$@"; }

fakes_left() { pgrep -f "$T_ERE/(smoke-bin|\\.local/bin)/claude" 2>/dev/null; }
no_fakes() { [ -z "$(fakes_left)" ]; }

# True when none of the given .sleep files names a running `sleep 600`.
sleeps_gone() {
    for sg_file in "$@"; do
        sg_pid=$(cat "$sg_file" 2>/dev/null) || continue
        [ -n "$sg_pid" ] || continue
        if ps -o command= -p "$sg_pid" 2>/dev/null | grep -q 'sleep 600'; then
            return 1
        fi
    done
    return 0
}

collect_logs() {
    for cl in macos.log api.log; do
        [ ! -f "$DATA/$cl" ] || cp "$DATA/$cl" "$LOGS/$cl" 2>/dev/null || true
    done
    [ ! -f "$DATA/update/restart.log" ] || cp "$DATA/update/restart.log" "$LOGS/restart.log" 2>/dev/null || true
}

# Only a pid that is still one of ours (the smoke app or install.sh), never a reused one.
ours() {
    case "$(ps -o command= -p "$1" 2>/dev/null)" in
        "$BIN"* | *scripts/install.sh*) return 0 ;;
        *) return 1 ;;
    esac
}

cleanup() {
    collect_logs
    cp_live=""
    for cp_pid in $STARTED; do
        if ours "$cp_pid"; then
            kill -TERM "$cp_pid" 2>/dev/null || true
            cp_live="$cp_live $cp_pid"
        fi
    done
    [ -z "$cp_live" ] || sleep 2
    for cp_pid in $cp_live; do
        ! ours "$cp_pid" || kill -KILL "$cp_pid" 2>/dev/null || true
    done
    # Anything that still runs from the throwaway HOME: the apps, fake claudes, their sleeps.
    for cp_pid in $(pgrep -f "^$(ere_escape "$BIN")( |\$)" 2>/dev/null) $(fakes_left); do
        kill -KILL "$cp_pid" 2>/dev/null || true
    done
    for cl in "$T"/fake/*.sleep; do
        [ -f "$cl" ] || continue
        cp_pid=$(cat "$cl" 2>/dev/null)
        if [ -n "$cp_pid" ] && ps -o command= -p "$cp_pid" 2>/dev/null | grep -q 'sleep 600'; then
            kill -KILL "$cp_pid" 2>/dev/null || true
        fi
    done
    [ "$TEXTEDIT_OPENED" -eq 0 ] || pkill -x TextEdit 2>/dev/null || true
    if [ "${SMOKE_KEEP:-0}" = "1" ]; then
        echo "smoke-macos.sh: kept the throwaway HOME at $T"
    else
        rm -rf "$T"
    fi
    echo "smoke-macos.sh: logs in $LOGS"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

# --- Preflight: nothing of the smoke app may run ------------------------------------------------

preflight() {
    if pgrep -f "^$(ere_escape "$BIN")( |\$)" >/dev/null 2>&1; then
        stop_now preflight "a $BIN_NAME process runs already ($(pgrep -f "^$(ere_escape "$BIN")( |\$)" | tr '\n' ' ')); quit it first"
    fi
    # tauri-plugin-single-instance 2.5.5 listens on /tmp/<identifier, . and - as _>[_<semver>]_si.sock.
    # It is checked with lsof, not by connecting: a connection makes a running app show its window.
    pf_id=$(printf '%s' "$IDENT" | tr '.-' '__')
    for pf_sock in /tmp/"$pf_id"_*si.sock; do
        [ -S "$pf_sock" ] || continue
        if lsof -U 2>/dev/null | grep -F "$pf_sock" >/dev/null; then
            stop_now preflight "a running app holds $pf_sock; quit it first"
        fi
    done
}

setup_home() {
    mkdir -p "$T/smoke-bin" "$T/.local/bin" "$T/fake" "$T/work"
    cp "$ROOT/scripts/smoke/fake-claude.sh" "$T/smoke-bin/claude" || stop_now preflight "can't copy the fake claude"
    chmod 755 "$T/smoke-bin/claude"
    # The login shell adds the fake claude's folder; zsh reads .zprofile, bash .bash_profile.
    for sh_file in .zprofile .bash_profile .profile; do
        printf '%s\n' 'export PATH="$HOME/smoke-bin:$PATH"' >"$T/$sh_file"
    done
    touch "$T/focus.txt"
}

# --- Focus: TextEdit must stay in front -----------------------------------------------------------

front_name() {
    fn_asn=$(lsappinfo front 2>/dev/null) || return 1
    [ -n "$fn_asn" ] || return 1
    lsappinfo info -only name "$fn_asn" 2>/dev/null | sed -n 's/^"[^"]*"="\(.*\)"$/\1/p' | head -n 1
}
front_is() { [ "$(front_name 2>/dev/null)" = "$1" ]; }
FOCUS_READY=0
focus_prepare() {
    FOCUS_READY=0
    open -a TextEdit "$T/focus.txt" 2>/dev/null || true
    TEXTEDIT_OPENED=1
    if wait_for 20 front_is TextEdit; then
        FOCUS_READY=1
    else
        fail "focus-$1" "TextEdit never came to the front before the launch (front: $(front_name 2>/dev/null || true))"
    fi
}
focus_check() {
    [ "$FOCUS_READY" -eq 1 ] || return 0
    if front_is TextEdit; then
        pass "focus-kept-$1"
    else
        fail "focus-kept-$1" "the front app became '$(front_name 2>/dev/null || true)'"
    fi
}

# --- The app -----------------------------------------------------------------------------------

api_pid() { sed -n 's/.*"pid" *: *\([0-9][0-9]*\).*/\1/p' "$DATA/api.json" 2>/dev/null | head -n 1; }
api_fresh() {
    af_pid=$(api_pid)
    [ -n "$af_pid" ] && [ "$af_pid" != "$1" ] && alive "$af_pid"
}

# A new dry-run card in the throwaway HOME's work folder; prints its number.
new_card() {
    nc_out=$(kbt create --title "$1" --summary "Smoke test card (dry run)." 2>&1) || {
        note "kb create: $nc_out"
        return 1
    }
    nc_n=$(printf '%s\n' "$nc_out" | sed -n 's/^  "number": *\([0-9][0-9]*\).*/\1/p' | head -n 1)
    [ -n "$nc_n" ] || { note "kb create gave no number"; return 1; }
    nc_up=$(kbt update "$nc_n" --dryRun true --worktreePath "$T/work" 2>&1) || {
        note "kb update: $nc_up"
        return 1
    }
    printf '%s' "$nc_n"
}

session_state() {
    kbt sessions "$1" 2>/dev/null | sed -n 's/^    "state": "\([a-z_-]*\)".*/\1/p' | head -n 1
}
# Ready: the app has had the session's first statusLine report (or SessionStart hook).
session_ready() {
    sr_state=$(session_state "$1")
    [ -n "$sr_state" ] && [ "$sr_state" != "starting" ]
}
fake_id() { cat "$T/fake/card-$1.id" 2>/dev/null; }
fake_file() {
    ff_id=$(fake_id "$1") || return 1
    [ -n "$ff_id" ] || return 1
    printf '%s' "$T/fake/$ff_id.$2"
}
fake_has() {
    fh_f=$(fake_file "$1" "$2") || return 1
    [ -f "$fh_f" ]
}
env_value() {
    sed -n "s/^$2=//p" "$(fake_file "$1" env)" 2>/dev/null | head -n 1
}
card_status() {
    sqlite3 -readonly "$DATA/board.db" \
        "SELECT (SELECT status FROM session_events e WHERE e.session_id = s.id ORDER BY seq DESC LIMIT 1)
           FROM sessions s JOIN cards c ON c.id = s.card_id
          WHERE c.number = $1 ORDER BY s.started_at DESC LIMIT 1;" 2>&1
}

# Starts a card's session; true when the app took it.
start_session() {
    ss_out=$(kbt start "$1" 2>&1) || { note "kb start $1: $ss_out"; return 1; }
    case "$ss_out" in
        *queued*) note "kb start $1: $ss_out"; return 1 ;;
    esac
    return 0
}

quit_app() {
    kill -TERM "$1" 2>/dev/null || true
    wait_for 60 dead "$1"
}

# --- Launch 1: the binary run directly ---------------------------------------------------------

launch1() {
    cp "$T/smoke-bin/claude" "$T/.local/bin/claude"
    chmod 755 "$T/.local/bin/claude"

    # kb with the app stopped: its error names the test app, so this kb is a test-channel build.
    l1_out=$(kbt list 2>&1)
    case "$l1_out" in
        *"(test)"*) pass "kb-names-test-app" ;;
        *) fail "kb-names-test-app" "kb list with the app stopped said: $l1_out" ;;
    esac

    focus_prepare launch1
    HOME="$T" "$BIN" --background >"$LOGS/app-launch1.log" 2>&1 &
    APP_PID=$!
    STARTED="$STARTED $APP_PID"

    if wait_for 30 test -f "$DATA/api.json"; then
        pass "api-json"
    else
        fail "api-json" "no $DATA/api.json within 30 s"
        return 1
    fi
    l1_mode=$(file_mode "$DATA/api.json")
    if [ "$l1_mode" = "600" ]; then pass "api-json-0600"; else fail "api-json-0600" "mode $l1_mode"; fi
    l1_mode=$(file_mode "$DATA")
    if [ "$l1_mode" = "700" ]; then pass "data-folder-0700"; else fail "data-folder-0700" "mode $l1_mode"; fi
    if wait_for 10 api_fresh 0 && [ "$(api_pid)" = "$APP_PID" ]; then
        pass "api-json-pid"
    else
        fail "api-json-pid" "api.json names pid '$(api_pid)', the app is $APP_PID"
    fi

    # The board's window exists by now; give it 5 s to take focus if it were going to.
    sleep 5
    focus_check launch1

    if l1_out=$(kbt list 2>&1); then pass "kb-list"; else fail "kb-list" "$l1_out"; fi

    HOME="$T" "$BIN" --background >>"$LOGS/app-launch1.log" 2>&1 &
    l1_second=$!
    STARTED="$STARTED $l1_second"
    if wait_for 10 dead "$l1_second" && alive "$APP_PID"; then
        pass "second-launch-exits"
    else
        fail "second-launch-exits" "the second copy is $(alive "$l1_second" && echo still running || echo gone), the first is $(alive "$APP_PID" && echo running || echo gone)"
    fi

    # A dry-run card: created, moved, read back.
    if l1_card=$(new_card "Smoke card"); then
        if l1_out=$(kbt move "$l1_card" Blocked 2>&1) &&
            kbt card "$l1_card" 2>/dev/null | grep -q '^  "column": "Blocked"' &&
            kbt card "$l1_card" 2>/dev/null | grep -q '"dryRun": true'; then
            pass "card-create-move-read"
        else
            fail "card-create-move-read" "card #$l1_card: $l1_out"
        fi
        if l1_out=$(kbt notify "$l1_card" "Smoke test notification" 2>&1); then
            pass "kb-notify"
        else
            fail "kb-notify" "$l1_out"
        fi
    else
        fail "card-create-move-read" "kb create failed"
    fi

    # Four sessions on a PTY each.
    L1_CARDS=""
    for l1_i in 1 2 3 4; do
        if l1_c=$(new_card "Smoke session $l1_i") && start_session "$l1_c"; then
            L1_CARDS="$L1_CARDS $l1_c"
        else
            fail "session-start-$l1_i" "couldn't start a session"
        fi
    done
    l1_ready=0
    l1_tty=0
    l1_pipe=0
    l1_lang=0
    l1_nofile=0
    l1_why=""
    SLEEPS=""
    for l1_c in $L1_CARDS; do
        if wait_for 30 session_ready "$l1_c"; then
            l1_ready=$((l1_ready + 1))
        else
            l1_why="$l1_why #$l1_c not ready (state '$(session_state "$l1_c")');"
        fi
        if ! wait_for 10 fake_has "$l1_c" env; then
            l1_why="$l1_why #$l1_c: the fake claude wrote no env file;"
            continue
        fi
        case "$(env_value "$l1_c" TTY)" in
            /dev/tty*) l1_tty=$((l1_tty + 1)) ;;
            *) l1_why="$l1_why #$l1_c tty '$(env_value "$l1_c" TTY)';" ;;
        esac
        case "$(env_value "$l1_c" YES_HEAD)" in
            *[Bb]roken\ pipe*) l1_why="$l1_why #$l1_c: yes | head said '$(env_value "$l1_c" YES_HEAD)';" ;;
            *) l1_pipe=$((l1_pipe + 1)) ;;
        esac
        if [ -n "$(env_value "$l1_c" LANG)" ]; then l1_lang=$((l1_lang + 1)); else l1_why="$l1_why #$l1_c LANG unset;"; fi
        l1_n=$(env_value "$l1_c" ULIMIT_N)
        case "$l1_n" in
            unlimited) l1_nofile=$((l1_nofile + 1)) ;;
            '' | *[!0-9]*) l1_why="$l1_why #$l1_c ulimit -n '$l1_n';" ;;
            *)
                if [ "$l1_n" -ge 10240 ]; then l1_nofile=$((l1_nofile + 1)); else l1_why="$l1_why #$l1_c ulimit -n $l1_n;"; fi
                ;;
        esac
        if wait_for 10 fake_has "$l1_c" sleep; then
            SLEEPS="$SLEEPS $(fake_file "$l1_c" sleep)"
        fi
    done
    l1_total=4
    report4() {
        if [ "$2" -eq "$l1_total" ]; then pass "$1 ($2 of $l1_total)"; else fail "$1" "$2 of $l1_total;$l1_why"; fi
    }
    report4 "sessions-ready" "$l1_ready"
    report4 "sessions-on-a-tty" "$l1_tty"
    report4 "sessions-no-broken-pipe" "$l1_pipe"
    report4 "sessions-lang-set" "$l1_lang"
    report4 "sessions-open-files-10240" "$l1_nofile"

    # The token is in the session files' hook URLs: each must be private.
    l1_bad=""
    l1_seen=0
    for l1_f in $(find "$DATA/sessions" -name settings.json -o -name mcp.json 2>/dev/null); do
        l1_seen=$((l1_seen + 1))
        l1_mode=$(file_mode "$l1_f")
        [ "$l1_mode" = "600" ] || l1_bad="$l1_bad ${l1_f#"$DATA"/}=$l1_mode"
    done
    if [ "$l1_seen" -eq 0 ]; then
        fail "session-files-0600" "no settings.json or mcp.json under $DATA/sessions"
    elif [ -z "$l1_bad" ]; then
        pass "session-files-0600 ($l1_seen files)"
    else
        fail "session-files-0600" "$l1_bad"
    fi

    # Account 2 isn't supported on a Mac: its card must refuse to start.
    if l1_c=$(new_card "Smoke account 2"); then
        if l1_out=$(kbt update "$l1_c" --accountPin account2 2>&1); then
            l1_out=$(kbt start "$l1_c" 2>&1)
            case "$l1_out" in
                *"Account 2 isn't supported on a Mac in this version"*) pass "account2-refused" ;;
                *) fail "account2-refused" "kb start said: $l1_out" ;;
            esac
        else
            skip "account2-refused" "kb can't set a card's account here: $l1_out"
        fi
    fi

    # The process tracker records every 5 s: let it see the handed-off sleeps.
    sleep 6
    if quit_app "$APP_PID"; then
        pass "quit-on-sigterm"
    else
        fail "quit-on-sigterm" "still running 60 s after SIGTERM"
        kill -KILL "$APP_PID" 2>/dev/null || true
        wait_for 10 dead "$APP_PID" || true
    fi
    l1_why=""
    l1_ok=0
    for l1_c in $L1_CARDS; do
        l1_s=$(card_status "$l1_c")
        if [ "$l1_s" = "suspended" ]; then l1_ok=$((l1_ok + 1)); else l1_why="$l1_why #$l1_c '$l1_s';"; fi
    done
    report4 "sessions-suspended" "$l1_ok"
    if wait_for 10 no_fakes; then
        pass "no-fake-claude-left"
    else
        fail "no-fake-claude-left" "still running: $(fakes_left | tr '\n' ' ')"
    fi
    # $SLEEPS holds paths without spaces (the throwaway HOME has none).
    # shellcheck disable=SC2086
    if [ -z "$SLEEPS" ]; then
        fail "reaper-after-quit" "no fake claude recorded a handed-off sleep"
    elif wait_for 10 sleeps_gone $SLEEPS; then
        pass "reaper-after-quit"
    else
        fail "reaper-after-quit" "a handed-off sleep 600 still runs after the app quit"
    fi

    # Launch 1b: started again, a session, then kill -9.
    l1_old=$(api_pid)
    HOME="$T" "$BIN" --background >>"$LOGS/app-launch1.log" 2>&1 &
    APP_PID=$!
    STARTED="$STARTED $APP_PID"
    if ! wait_for 30 api_fresh "$l1_old"; then
        fail "kill9-hangup" "the app didn't start again (no fresh api.json in 30 s)"
        return 1
    fi
    if l1_c=$(new_card "Smoke kill -9") && start_session "$l1_c" && wait_for 30 session_ready "$l1_c" &&
        wait_for 10 fake_has "$l1_c" sleep; then
        l1_sleep=$(fake_file "$l1_c" sleep)
        l1_hup=$(fake_file "$l1_c" hup)
        sleep 6
        kill -KILL "$APP_PID" 2>/dev/null || true
        if wait_for 10 test -f "$l1_hup"; then
            pass "kill9-hangup"
        else
            fail "kill9-hangup" "the fake claude got no SIGHUP within 10 s of kill -9"
        fi
        if wait_for 10 sleeps_gone "$l1_sleep"; then
            pass "kill9-reaper"
        else
            fail "kill9-reaper" "the handed-off sleep 600 still runs 10 s after kill -9"
        fi
    else
        fail "kill9-hangup" "no ready session to kill under (card '${l1_c:-}')"
        kill -KILL "$APP_PID" 2>/dev/null || true
    fi
    wait_for 10 dead "$APP_PID" || true
    return 0
}

# --- Launch 2: open -g, launchd's bare PATH ------------------------------------------------------

launch2() {
    l2_major=$(sw_vers -productVersion 2>/dev/null | cut -d. -f1)
    case "$l2_major" in
        '' | *[!0-9]*) l2_major=0 ;;
    esac
    if [ "$l2_major" -lt 13 ]; then
        skip "launch2" "open --env needs macOS 13 or later; this is $(sw_vers -productVersion 2>/dev/null)"
        return 0
    fi
    # Only the login shell's PATH can find the fake claude now.
    rm -f "$T/.local/bin/claude"

    focus_prepare launch2
    l2_old=$(api_pid)
    if ! l2_out=$(open -g --env "HOME=$T" -a "$APP" --args --background 2>&1); then
        fail "launch2-open" "$l2_out"
        return 1
    fi
    if ! wait_for 30 api_fresh "${l2_old:-0}"; then
        fail "launch2-open" "no fresh api.json within 30 s of open -g"
        return 1
    fi
    APP_PID=$(api_pid)
    STARTED="$STARTED $APP_PID"
    pass "launch2-open"

    l2_shell=$(tail -n 1 "$DATA/macos.log" 2>/dev/null | sed -n 's/.*login shell: \([^;]*\);.*/\1/p')
    if [ -n "$l2_shell" ]; then
        pass "macos-log-names-shell ($l2_shell)"
    else
        fail "macos-log-names-shell" "macos.log has no 'login shell:' line"
    fi

    sleep 5
    focus_check launch2

    if l2_c=$(new_card "Smoke login PATH") && start_session "$l2_c" && wait_for 30 session_ready "$l2_c"; then
        case "$(env_value "$l2_c" PATH)" in
            *"$T/smoke-bin"*) pass "login-shell-path" ;;
            *) fail "login-shell-path" "the session's PATH lacks $T/smoke-bin: $(env_value "$l2_c" PATH)" ;;
        esac
    else
        fail "login-shell-path" "no ready session: the app found no claude through the login shell's PATH (shell '$l2_shell')"
        l2_c=""
    fi

    if [ "$PROMPTS" -eq 1 ]; then
        prompts_check
    fi

    sleep 6
    l2_out=$(osascript -e 'with timeout of 20 seconds' -e "tell application id \"$IDENT\" to quit" -e 'end timeout' 2>&1)
    l2_rc=$?
    if [ "$l2_rc" -ne 0 ]; then
        case "$l2_out" in
            *-1743*) skip "apple-event-quit" "Automation consent: $l2_out" ;;
            *) fail "apple-event-quit" "osascript: $l2_out" ;;
        esac
        kill -TERM "$APP_PID" 2>/dev/null || true
    fi
    if wait_for 60 dead "$APP_PID"; then
        [ "$l2_rc" -ne 0 ] || pass "apple-event-quit"
    else
        fail "apple-event-quit" "still running 60 s after the quit"
        kill -KILL "$APP_PID" 2>/dev/null || true
    fi
    if [ -n "$l2_c" ]; then
        l2_s=$(card_status "$l2_c")
        if [ "$l2_s" = "suspended" ]; then
            pass "launch2-session-suspended"
        else
            fail "launch2-session-suspended" "card #$l2_c's session is '$l2_s'"
        fi
    fi
    return 0
}

# --- #114's {prompts} fix: no Windows path in a Mac session's first message -------------------

prompts_check() {
    if ! pc_c=$(new_card "Smoke prompts"); then
        fail "prompts-no-windows-path" "kb create failed"
        return 0
    fi
    pc_out=$(kbt move "$pc_c" Planning 2>&1) || { fail "prompts-no-windows-path" "kb move: $pc_out"; return 0; }
    # Planning starts the card's session by itself; start it here if it didn't.
    if ! wait_for 10 fake_has "$pc_c" env; then
        start_session "$pc_c" || true
    fi
    pc_has_planning() {
        pc_in=$(fake_file "$pc_c" in) || return 1
        grep -qi 'planning' "$pc_in" 2>/dev/null
    }
    if ! wait_for 60 pc_has_planning; then
        if fake_has "$pc_c" first; then
            note "no line naming planning reached the fake claude in 60 s; checking what did"
        else
            fail "prompts-no-windows-path" "no message reached the fake claude within 60 s"
            return 0
        fi
    fi
    pc_first=$(fake_file "$pc_c" first)
    pc_in=$(fake_file "$pc_c" in)
    if grep -F 'C:\' "$pc_first" "$pc_in" >/dev/null 2>&1; then
        fail "prompts-no-windows-path" "the message names a C:\\ path: $(grep -F 'C:\' "$pc_in" | head -n 1)"
    elif grep -F '\planning.md' "$pc_first" "$pc_in" >/dev/null 2>&1; then
        fail "prompts-no-windows-path" "a backslash before planning.md: $(grep -F '\planning.md' "$pc_in" | head -n 1)"
    else
        pass "prompts-no-windows-path"
        pc_line=$(grep -o '[^ ]*planning\.md' "$pc_in" 2>/dev/null | head -n 1)
        [ -z "$pc_line" ] || note "the prompt path it got: $pc_line"
    fi
    back_check
    kbt stop "$pc_c" >/dev/null 2>&1 || true
}

# --- The app's own messages (build.rs's KANBAN_PROMPTS_DIR): back from Human test ------------
# Moving the card from Human test to In progress sends it "It came back from Human test ...
# (<prompts>/in-progress.md)", built from the folder build.rs compiles in. A Windows path or
# a backslash before in-progress.md there is the bug the router found in the final merge.
back_check() {
    bc_out=$(kbt move "$pc_c" "Human test" 2>&1) || { fail "app-messages-no-windows-path" "kb move to Human test: $bc_out"; return 0; }
    bc_out=$(kbt move "$pc_c" "In progress" 2>&1) || { fail "app-messages-no-windows-path" "kb move to In progress: $bc_out"; return 0; }
    bc_has_back() {
        bc_in=$(fake_file "$pc_c" in) || return 1
        grep -q 'came back from Human test' "$bc_in" 2>/dev/null
    }
    if ! wait_for 60 bc_has_back; then
        fail "app-messages-no-windows-path" "the back-from-Human-test message didn't reach the fake claude within 60 s"
        return 0
    fi
    bc_in=$(fake_file "$pc_c" in)
    bc_line=$(grep -o '[^ (]*in-progress\.md' "$bc_in" 2>/dev/null | tail -n 1)
    if grep -F 'C:\' "$bc_in" >/dev/null 2>&1; then
        fail "app-messages-no-windows-path" "a C:\\ path: $(grep -F 'C:\' "$bc_in" | tail -n 1 | cut -c1-200)"
    elif grep -F '\in-progress.md' "$bc_in" >/dev/null 2>&1; then
        fail "app-messages-no-windows-path" "a backslash before in-progress.md: $bc_line"
    else
        case "$bc_line" in
            /*/prompts/in-progress.md) pass "app-messages-no-windows-path"; note "the path it got: $bc_line" ;;
            *) fail "app-messages-no-windows-path" "no /…/prompts/in-progress.md path in the message (got: ${bc_line:-none})" ;;
        esac
    fi
}

# --- The test-app lock kill test ----------------------------------------------------------------

lock_test() {
    lt_kb=${KB_OPT:-$KB}
    [ -x "$lt_kb" ] || { fail "lock-take" "no kb at $lt_kb"; return 0; }
    lt_log1="$LOGS/lock1.log"
    lt_log2="$LOGS/lock2.log"
    HOME="$T" sh "$ROOT/scripts/install.sh" --channel test --lock-only --kb "$lt_kb" >"$lt_log1" 2>&1 &
    lt_p1=$!
    STARTED="$STARTED $lt_p1"
    if wait_for 60 grep -qx locked "$lt_log1"; then
        pass "lock-take"
    else
        fail "lock-take" "no 'locked' within 60 s: $(tail -n 3 "$lt_log1" | tr '\n' ' ')"
        kill -TERM "$lt_p1" 2>/dev/null || true
        return 0
    fi
    # Killed mid-run: the lock belongs to that shell's open file, so it must end with it.
    kill -KILL "$lt_p1" 2>/dev/null || true
    wait_for 5 dead "$lt_p1" || true
    HOME="$T" sh "$ROOT/scripts/install.sh" --channel test --lock-only --kb "$lt_kb" >"$lt_log2" 2>&1 &
    lt_p2=$!
    STARTED="$STARTED $lt_p2"
    if wait_for 15 grep -qx locked "$lt_log2"; then
        pass "lock-after-kill"
    else
        fail "lock-after-kill" "the second run didn't take the lock within 15 s: $(tail -n 3 "$lt_log2" | tr '\n' ' ')"
    fi
    kill -TERM "$lt_p2" 2>/dev/null || true
    wait_for 5 dead "$lt_p2" || true
}

# --- Run -----------------------------------------------------------------------------------------

echo "smoke-macos.sh: $APP ($IDENT, $(sw_vers -productVersion 2>/dev/null), $(uname -m)); throwaway HOME $T"
setup_home
case "$PART" in
    lock)
        lock_test
        ;;
    launch1)
        preflight
        launch1 || true
        ;;
    all)
        preflight
        launch1 || true
        launch2 || true
        ;;
esac

if [ "$FAILS" -eq 0 ]; then
    echo "smoke-macos.sh: all checks passed"
    exit 0
fi
echo "smoke-macos.sh: $FAILS check(s) failed"
exit 1
