#!/bin/sh
# Builds Claude Kanban DSH and installs it for this Mac user (card #117): the macOS twin of
# scripts/install.ps1. No sudo; it touches only your own folders.
#
#   sh scripts/install.sh                         # live: stage an update ("Update ready")
#   sh scripts/install.sh --channel test          # the test copy, installed and started now
#   sh scripts/install.sh --channel test --fresh-copy
#   sh scripts/install.sh --channel smoke --no-start --install-dir /tmp/apps   # CI's smoke build
#
# Options:
#   --channel live|test|smoke  live (the default), the test copy, or the smoke-test build (the
#                              test channel under its own name and identifier, for
#                              scripts/smoke-macos.sh; never started here)
#   --stage | --now            stage the update for the app's Restart now (live's default), or
#                              quit, install and start now (test's and smoke's default)
#   --fresh-copy               test only: replace the test board with a new copy of the live one
#   --universal                one app for Apple silicon and Intel (both Rust targets needed)
#   --dmg                      also make a .dmg of the built app
#   --no-start                 install but don't start it
#   --install-dir <dir>        where the .app goes (default ~/Applications)
#   --label <text>             the test-app lock's label (default: from KANBAN_CARD or "hugh")
#   --lock-only                test only: take the test-app lock and hold it until killed; no
#                              build (the smoke test's lock kill test)
#   --kb <path>                the kb to take the lock with when --lock-only
#
# Two modes, as on Windows:
#   --stage builds, then leaves <Name>-<stamp>.app.zip and update.json (version, commit,
#   built_at, installer, install_dir, app_exe, channel) in the data folder's update/ folder.
#   The running app shows "Update ready"; Restart now there swaps it in. A first install
#   can't be staged.
#   --now builds, quits the channel's running app (SIGTERM: its sessions are suspended),
#   puts the new .app in place and starts it in the background (open -g).
#
# Never run --channel live --now from a card session: it quits every running card session,
# probably your own. The script refuses it when KANBAN_CARD or KANBAN_SESSION is set (CI=true
# excepted).
#
# Data folders: live ~/.claude-kanban-dsh, test ~/.claude-kanban-dsh-test. Apps: <install dir>/Claude
# Kanban DSH.app and Claude Kanban DSH Test.app. Live links ~/.local/bin/kb to its kb; the test kb stays
# off PATH, so `kb` is always the live one.
#
# Signing: with the "Claude Kanban DSH Local" identity (scripts/mac-signing-cert.sh) when it is in
# the keychain, else ad hoc (then macOS asks for folder permissions again after each update).
# This script signs, not Tauri's bundler: the bundler can't pass --timestamp=none, so an offline
# build would fail, and it turns on the hardened runtime, which a self-signed app doesn't need.
#
# Plain POSIX sh: macOS /bin/sh, BSD tools, no GNU-only flags.

set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
TAURI_DIR="$ROOT/src-tauri"
SIGN_ID="Claude Kanban DSH Local"

die() {
    printf 'install.sh: %s\n' "$*" >&2
    exit 1
}
say() {
    printf '%s\n' "$*"
}
warn() {
    printf 'install.sh: warning: %s\n' "$*" >&2
}

# --- Options ---------------------------------------------------------------------------------

CHANNEL=live
MODE=""
FRESH=0
UNIVERSAL=0
DMG=0
START=1
INSTALL_DIR="$HOME/Applications"
LABEL=""
LOCK_ONLY=0
KB_GIVEN=""

need_value() {
    [ "$2" -ge 2 ] || die "$1 needs a value"
}
while [ $# -gt 0 ]; do
    case "$1" in
        --channel) need_value "$1" $#; CHANNEL=$2; shift 2 ;;
        --channel=*) CHANNEL=${1#*=}; shift ;;
        --stage)
            [ "$MODE" != "now" ] || die "pass --stage or --now, not both"
            MODE=stage; shift ;;
        --now)
            [ "$MODE" != "stage" ] || die "pass --stage or --now, not both"
            MODE=now; shift ;;
        --fresh-copy) FRESH=1; shift ;;
        --universal) UNIVERSAL=1; shift ;;
        --dmg) DMG=1; shift ;;
        --no-start) START=0; shift ;;
        --install-dir) need_value "$1" $#; INSTALL_DIR=$2; shift 2 ;;
        --install-dir=*) INSTALL_DIR=${1#*=}; shift ;;
        --label) need_value "$1" $#; LABEL=$2; shift 2 ;;
        --label=*) LABEL=${1#*=}; shift ;;
        --lock-only) LOCK_ONLY=1; shift ;;
        --kb) need_value "$1" $#; KB_GIVEN=$2; shift 2 ;;
        --kb=*) KB_GIVEN=${1#*=}; shift ;;
        -h | --help) sed -n '2,47p' "$0"; exit 0 ;;
        *) die "unknown option $1 (see sh scripts/install.sh --help)" ;;
    esac
done

case "$CHANNEL" in
    live | test | smoke) ;;
    *) die "--channel is live, test or smoke, not $CHANNEL" ;;
esac
if [ -z "$MODE" ]; then
    if [ "$CHANNEL" = "live" ]; then MODE=stage; else MODE=now; fi
fi

# --- 2. Rules ----------------------------------------------------------------------------------

if [ "$CHANNEL" = "live" ] && [ "$MODE" = "now" ] && [ "${CI:-}" != "true" ]; then
    if [ -n "${KANBAN_CARD:-}" ] || [ -n "${KANBAN_SESSION:-}" ]; then
        die "refused: --channel live --now from a card session would quit every running card session, probably this one. Stage it instead (no --now) and let Hugh click Restart now."
    fi
fi
[ "$FRESH" -eq 0 ] || [ "$CHANNEL" = "test" ] || die "--fresh-copy is for --channel test only; the live board is never replaced"
[ "$FRESH" -eq 0 ] || [ "$MODE" = "now" ] || die "--fresh-copy needs the test app stopped: use it with --now"
[ "$LOCK_ONLY" -eq 0 ] || [ "$CHANNEL" = "test" ] || die "--lock-only is for --channel test only"
[ "$CHANNEL" != "smoke" ] || [ "$MODE" = "now" ] || die "the smoke build isn't staged: it shares the test channel's data folder"

# --- 1. Checks ---------------------------------------------------------------------------------

[ "$(uname -s)" = "Darwin" ] || die "this is for macOS; on Windows run scripts\\install.ps1"

case ":$PATH:" in
    *":$HOME/.cargo/bin:"*) ;;
    *)
        if [ -d "$HOME/.cargo/bin" ]; then
            PATH="$HOME/.cargo/bin:$PATH"
            export PATH
        fi
        ;;
esac

if [ "$LOCK_ONLY" -eq 0 ]; then
    xcode-select -p >/dev/null 2>&1 ||
        die "the Xcode Command Line Tools are missing: run xcode-select --install, then this again"
    command -v cargo >/dev/null 2>&1 ||
        die "cargo is missing: install Rust from https://rustup.rs, then open a new Terminal"
    cargo tauri --version >/dev/null 2>&1 ||
        die "cargo tauri is missing: cargo install tauri-cli --version 2.12.4 --locked"
    if [ "$UNIVERSAL" -eq 1 ]; then
        INSTALLED_TARGETS=$(rustup target list --installed 2>/dev/null || true)
        for t in aarch64-apple-darwin x86_64-apple-darwin; do
            printf '%s\n' "$INSTALLED_TARGETS" | grep -qx "$t" ||
                die "--universal needs the Rust target $t: rustup target add aarch64-apple-darwin x86_64-apple-darwin"
        done
    fi
    FREE_KB=$(df -Pk "$ROOT" | awk 'NR == 2 { print $4 }')
    case "$FREE_KB" in
        '' | *[!0-9]*) warn "couldn't read the free space on the repo's disk; going on" ;;
        *)
            if [ "$FREE_KB" -lt 10485760 ]; then
                # A CI runner's disk is what it is: say so and try.
                if [ "${CI:-}" = "true" ]; then
                    warn "only $((FREE_KB / 1048576)) GB free on the repo's disk (a build wants about 10 GB); going on under CI"
                else
                    die "a build needs about 10 GB free on the repo's disk; there are $((FREE_KB / 1048576)) GB"
                fi
            fi
            ;;
    esac
fi

# --- 3. Names from the Tauri configs -----------------------------------------------------------

# A top-level string from a config: plutil reads JSON; sed is the fallback for old macOS.
conf_get() {
    v=$(plutil -extract "$1" raw -o - "$2" 2>/dev/null) ||
        v=$(sed -n "s/^  \"$1\": *\"\\([^\"]*\\)\".*/\\1/p" "$2" | head -n 1)
    printf '%s' "$v"
}

BASE_CONF="$TAURI_DIR/tauri.conf.json"
case "$CHANNEL" in
    live) CONF="$BASE_CONF" ;;
    test) CONF="$TAURI_DIR/tauri.test.conf.json" ;;
    smoke) CONF="$TAURI_DIR/tauri.smoke.conf.json" ;;
esac
PRODUCT=$(conf_get productName "$CONF")
BINARY=$(conf_get mainBinaryName "$CONF")
IDENTIFIER=$(conf_get identifier "$CONF")
VERSION=$(conf_get version "$BASE_CONF")
# The live config has no mainBinaryName: the bundle's executable is the cargo binary's name.
[ -n "$BINARY" ] || BINARY=claude-kanban-dsh
[ -n "$PRODUCT" ] || die "no productName in $CONF"
[ -n "$IDENTIFIER" ] || die "no identifier in $CONF"
[ -n "$VERSION" ] || VERSION=0.0.0

APP="$INSTALL_DIR/$PRODUCT.app"
APP_BIN="$APP/Contents/MacOS/$BINARY"
if [ "$CHANNEL" = "live" ]; then
    DATA="$HOME/.claude-kanban-dsh"
    NAME="Claude Kanban DSH"
else
    # The smoke build is the test channel: its data folder is the test one (it runs only under
    # a throwaway HOME in scripts/smoke-macos.sh).
    DATA="$HOME/.claude-kanban-dsh-test"
    if [ "$CHANNEL" = "test" ]; then NAME="Claude Kanban DSH (test)"; else NAME="Claude Kanban DSH (smoke)"; fi
fi
UPDATE_DIR="$DATA/update"
LOCK_DIR="$HOME/.claude-kanban-dsh-shared/testapp"

if [ "$MODE" = "stage" ] && [ ! -d "$APP" ]; then
    die "$NAME isn't installed at $APP yet, so there is nothing to update. Run this with --now for the first install."
fi

# --- 4. The build's identity -------------------------------------------------------------------

COMMIT=$(git -C "$ROOT" rev-parse --short=12 HEAD 2>/dev/null || true)
[ -n "$COMMIT" ] || COMMIT=unknown
if [ -n "$(git -C "$ROOT" status --porcelain 2>/dev/null || true)" ]; then
    COMMIT="$COMMIT-dirty"
fi
BUILT_AT=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# --- 7. The test-app lock (functions; taken after the build) -----------------------------------

# Card #115's lock, held through descriptor 9 of this shell: kb install-lock waits by the
# rules, takes flock on it, writes the lock file and exits. The lock ends when this shell ends,
# even if it is killed. Children that outlive a step must not keep descriptor 9 (9>&-).
LOCK_HELD=0
KB_LOCK=""
take_lock() {
    mkdir -p "$LOCK_DIR" || die "can't make $LOCK_DIR"
    chmod 700 "$HOME/.claude-kanban-dsh-shared" "$LOCK_DIR" 2>/dev/null || true
    # Read and write: kb install-lock --phase reads the lock file through the descriptor.
    exec 9<>"$LOCK_DIR/install.lock"
    set -- install-lock --fd 9 --lock-dir "$LOCK_DIR" --commit "$COMMIT" --worktree "$ROOT"
    [ -z "$LABEL" ] || set -- "$@" --label "$LABEL"
    case "${KANBAN_CARD:-}" in
        '' | *[!0-9#]*) ;;
        *) set -- "$@" --card "$KANBAN_CARD" ;;
    esac
    say "Taking the test-app lock ($LOCK_DIR/install.lock)..."
    rc=0
    "$KB_LOCK" "$@" || rc=$?
    case "$rc" in
        0) LOCK_HELD=1 ;;
        2) die "kb install-lock refused its arguments, or this kb isn't a macOS build (exit 2)" ;;
        3) die "gave up waiting for the test-app lock: another install still holds it" ;;
        *) die "kb install-lock failed (exit $rc)" ;;
    esac
}
lock_phase() {
    [ "$LOCK_HELD" -eq 1 ] || return 0
    "$KB_LOCK" install-lock --fd 9 --lock-dir "$LOCK_DIR" --phase "$1" ||
        warn "couldn't set the lock's phase to $1"
}
release_lock() {
    [ "$LOCK_HELD" -eq 1 ] || return 0
    exec 9>&-
    LOCK_HELD=0
}

if [ "$LOCK_ONLY" -eq 1 ]; then
    if [ -n "$KB_GIVEN" ]; then
        KB_LOCK=$KB_GIVEN
    elif [ -x "$APP/Contents/MacOS/kb" ]; then
        KB_LOCK="$APP/Contents/MacOS/kb"
    else
        KB_LOCK="$TAURI_DIR/binaries/kb-$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')"
    fi
    [ -x "$KB_LOCK" ] || die "no kb to take the lock with at $KB_LOCK (pass --kb <path>)"
    take_lock
    say "Holding the lock until this script is stopped (--lock-only)."
    # The sleeps don't inherit descriptor 9, so killing this shell frees the lock at once.
    while :; do
        sleep 1 9>&-
    done
fi

# --- 5. Build ----------------------------------------------------------------------------------

if [ "$CHANNEL" = "live" ]; then
    TARGET_BASE="$ROOT/target"
else
    TARGET_BASE="$ROOT/target/$CHANNEL"
fi
if [ "$UNIVERSAL" -eq 1 ]; then
    TRIPLE=universal-apple-darwin
    OUT_DIR="$TARGET_BASE/universal-apple-darwin/release"
    ARCH_NAME=universal
else
    TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
    OUT_DIR="$TARGET_BASE/release"
    case "$TRIPLE" in
        aarch64-*) ARCH_NAME=aarch64 ;;
        x86_64-*) ARCH_NAME=x64 ;;
        *) ARCH_NAME=$TRIPLE ;;
    esac
fi

set -- --bundles app
[ "$UNIVERSAL" -eq 0 ] || set -- "$@" --target universal-apple-darwin
[ "$CHANNEL" != "test" ] || set -- "$@" --features mac-devtools
set -- "$@" --config "$TAURI_DIR/tauri.macos.bundle.json"
case "$CHANNEL" in
    test) set -- "$@" --config "$TAURI_DIR/tauri.test.conf.json" --config "$TAURI_DIR/tauri.test.macos.conf.json" ;;
    smoke) set -- "$@" --config "$TAURI_DIR/tauri.smoke.conf.json" ;;
esac

say "Building kb and the app for the $CHANNEL channel ($TRIPLE; $VERSION, $COMMIT)..."
# In a subshell, so every variable set for the build ends with it.
(
    cd "$ROOT"
    export KANBAN_GIT_COMMIT="$COMMIT"
    export KANBAN_BUILT_AT="$BUILT_AT"
    # The before-build step (mac-kb.sh) runs in Tauri's frontend folder: make that the repo.
    export TAURI_FRONTEND_PATH="$ROOT"
    # The bundler must not sign: this script does (see the top).
    # (Without APPLE_CERTIFICATE the bundler ignores the rest of that pair.)
    unset APPLE_SIGNING_IDENTITY APPLE_CERTIFICATE
    if [ "$CHANNEL" = "live" ]; then
        # A live build is never made with KANBAN_CHANNEL set, nor into another target folder.
        unset KANBAN_CHANNEL CARGO_TARGET_DIR
    else
        export KANBAN_CHANNEL=test
        export CARGO_TARGET_DIR="$TARGET_BASE"
    fi
    cargo tauri build "$@"
) || die "cargo tauri build failed"

BUILT_APP="$OUT_DIR/bundle/macos/$PRODUCT.app"
[ -d "$BUILT_APP" ] || die "the build finished, but $BUILT_APP is missing"
[ -x "$BUILT_APP/Contents/MacOS/$BINARY" ] || die "$BUILT_APP has no Contents/MacOS/$BINARY"
[ -x "$BUILT_APP/Contents/MacOS/kb" ] || die "$BUILT_APP has no Contents/MacOS/kb (externalBin)"

# --- Signing -----------------------------------------------------------------------------------

if security find-identity -v -p codesigning 2>/dev/null | grep -F "\"$SIGN_ID\"" >/dev/null; then
    SIGN_WITH=$SIGN_ID
    say "Signing with \"$SIGN_ID\"..."
else
    SIGN_WITH=-
    warn "no \"$SIGN_ID\" identity (scripts/mac-signing-cert.sh makes it): signing ad hoc, so macOS asks for folder permissions again after each update"
fi
# Inside out: kb first, then the app (its executable and the bundle's seal).
codesign --force --timestamp=none -s "$SIGN_WITH" --identifier "$IDENTIFIER.kb" "$BUILT_APP/Contents/MacOS/kb" ||
    die "codesign failed on kb"
codesign --force --timestamp=none -s "$SIGN_WITH" "$BUILT_APP" ||
    die "codesign failed on the app"
codesign --verify --deep --strict "$BUILT_APP" || die "the signature doesn't verify"
say "Built $BUILT_APP ($VERSION, $COMMIT, $BUILT_AT)"

# --- The .dmg (after signing, so it holds the signed app) --------------------------------------

if [ "$DMG" -eq 1 ]; then
    DMG_DIR="$OUT_DIR/bundle/dmg"
    DMG_PATH="$DMG_DIR/${PRODUCT}_${VERSION}_${ARCH_NAME}.dmg"
    STAGE_DIR="$DMG_DIR/stage"
    mkdir -p "$DMG_DIR"
    rm -rf "$STAGE_DIR" "$DMG_PATH"
    mkdir -p "$STAGE_DIR"
    ditto "$BUILT_APP" "$STAGE_DIR/$PRODUCT.app" || die "couldn't copy the app for the .dmg"
    ln -s /Applications "$STAGE_DIR/Applications"
    hdiutil create -volname "$PRODUCT" -srcfolder "$STAGE_DIR" -ov -format UDZO "$DMG_PATH" >/dev/null ||
        die "hdiutil couldn't make the .dmg"
    rm -rf "$STAGE_DIR"
    say "DMG: $DMG_PATH"
fi

# --- 6. Stage: leave the update for the app's "Update ready", and stop -------------------------

json_str() {
    printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' | awk 'BEGIN { ORS = "" } NR > 1 { print "\\n" } { print }'
}

if [ "$MODE" = "stage" ]; then
    mkdir -p "$UPDATE_DIR" || die "can't make $UPDATE_DIR"
    # A name of its own per build, so nothing lands on a zip the app's helper is unpacking.
    STAMP=$(date -u +%Y%m%d%H%M%S)
    ZIP="$UPDATE_DIR/$PRODUCT-$STAMP.app.zip"
    rm -f "$ZIP.tmp"
    ditto -c -k --sequesterRsrc --keepParent "$BUILT_APP" "$ZIP.tmp" || die "couldn't zip the app"
    mv -f "$ZIP.tmp" "$ZIP"
    JSON="$UPDATE_DIR/update.json"
    # Written beside it and moved into place, so the app never reads half a file.
    {
        printf '{\n'
        printf '  "version": "%s",\n' "$(json_str "$VERSION")"
        printf '  "commit": "%s",\n' "$(json_str "$COMMIT")"
        printf '  "built_at": "%s",\n' "$(json_str "$BUILT_AT")"
        printf '  "installer": "%s",\n' "$(json_str "$ZIP")"
        printf '  "install_dir": "%s",\n' "$(json_str "$INSTALL_DIR")"
        printf '  "app_exe": "%s",\n' "$(json_str "$APP")"
        printf '  "channel": "%s"' "$(json_str "$CHANNEL")"
        # A test app's Restart now comes back behind whatever is in front (card #135); never live.
        if [ "$CHANNEL" != "live" ]; then printf ',\n  "background": true'; fi
        printf '\n'
        printf '}\n'
    } >"$JSON.tmp" || die "couldn't write $JSON.tmp"
    mv -f "$JSON.tmp" "$JSON" || die "couldn't write $JSON"
    # Older staged zips are build copies only; the newest one is all the app needs.
    for f in "$UPDATE_DIR"/*.app.zip; do
        [ -e "$f" ] || continue
        [ "$f" = "$ZIP" ] || rm -f "$f"
    done
    say "Staged $ZIP"
    say "$NAME shows 'Update ready' along the top of the board within 30 seconds (or at its next start)."
    say "Restart now there installs it; nothing was quit or restarted."
    exit 0
fi

# --- 7. Take the test-app lock (test channel only) ---------------------------------------------

if [ "$CHANNEL" = "test" ]; then
    KB_LOCK="$BUILT_APP/Contents/MacOS/kb"
    take_lock
fi

# --- 8. Quit the channel's running app ---------------------------------------------------------

# A pid that is this channel's installed app: the one api.json names if its executable is
# ours, else the first process whose command line starts with our binary's full path (macOS
# cuts process names at 16 characters, so pgrep -x can't tell the channels apart).
ere_escape() {
    printf '%s' "$1" | sed 's/[][\.*^$+?(){}|]/\\&/g'
}
running_pid() {
    if [ -f "$DATA/api.json" ]; then
        pid=$(sed -n 's/.*"pid" *: *\([0-9][0-9]*\).*/\1/p' "$DATA/api.json" | head -n 1)
        if [ -n "$pid" ] && [ "$(ps -o comm= -p "$pid" 2>/dev/null || true)" = "$APP_BIN" ]; then
            printf '%s\n' "$pid"
            return 0
        fi
    fi
    pgrep -f "^$(ere_escape "$APP_BIN")( |\$)" 2>/dev/null | head -n 1
}

PID=$(running_pid || true)
if [ -n "$PID" ]; then
    say "Quitting the running $NAME (pid $PID; its sessions are suspended)..."
    kill -TERM "$PID" 2>/dev/null || true
    # Stopping takes about two seconds per session, so allow a minute.
    i=0
    while kill -0 "$PID" 2>/dev/null && [ "$i" -lt 60 ]; do
        sleep 1
        i=$((i + 1))
    done
    if kill -0 "$PID" 2>/dev/null; then
        die "$NAME is still running after a minute: quit it from the menu bar, then run this again."
    fi
fi

# --- 9. Install --------------------------------------------------------------------------------

lock_phase installing
say "Installing into $APP..."
mkdir -p "$INSTALL_DIR" || die "can't make $INSTALL_DIR"
NEW="$APP.new"
OLD="$APP.old"
rm -rf "$NEW"
ditto "$BUILT_APP" "$NEW" 9>&- || die "couldn't copy the app to $NEW"
if [ -d "$APP" ]; then
    rm -rf "$OLD"
    mv "$APP" "$OLD" || die "couldn't move the old app aside ($APP)"
fi
if mv "$NEW" "$APP"; then
    rm -rf "$OLD"
else
    [ ! -d "$OLD" ] || mv "$OLD" "$APP" || true
    die "couldn't move the new app into place; the old one is back at $APP"
fi
[ -x "$APP_BIN" ] || die "installed, but $APP_BIN is missing"

# --- 10. kb on PATH (live) or the test board (test) --------------------------------------------

KB_INSTALLED="$APP/Contents/MacOS/kb"
case "$CHANNEL" in
    live)
        mkdir -p "$HOME/.local/bin"
        ln -sf "$KB_INSTALLED" "$HOME/.local/bin/kb" || die "couldn't link ~/.local/bin/kb"
        say "kb is linked at ~/.local/bin/kb."
        case ":$PATH:" in
            *":$HOME/.local/bin:"*) ;;
            *) warn "~/.local/bin isn't on your PATH, so new Terminals won't find kb. Add this line to ~/.zprofile: export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
        esac
        ;;
    test)
        say "The test kb is not put on PATH: run it as \"$KB_INSTALLED\"."
        TEST_BOARD="$DATA/board.db"
        LIVE_BOARD="$HOME/.claude-kanban-dsh/board.db"
        if [ "$FRESH" -eq 1 ] || [ ! -f "$TEST_BOARD" ]; then
            if [ -f "$LIVE_BOARD" ]; then
                lock_phase copying
                # The test kb reads the live board (its default --from) read-only, with VACUUM
                # INTO: safe while the live app runs. --fresh-copy moves the old one to backups.
                say "Copying the live board into $TEST_BOARD..."
                if [ "$FRESH" -eq 1 ]; then
                    "$KB_INSTALLED" copy-board --to "$TEST_BOARD" --replace 9>&- || die "kb copy-board failed"
                else
                    "$KB_INSTALLED" copy-board --to "$TEST_BOARD" 9>&- || die "kb copy-board failed"
                fi
            elif [ "$FRESH" -eq 1 ]; then
                say "There is no live board at $LIVE_BOARD to copy; the test board is kept as it is."
            else
                say "There is no live board at $LIVE_BOARD to copy; the test app starts with an empty board."
            fi
        else
            say "Keeping the test board at $TEST_BOARD (pass --fresh-copy for a new copy of the live one)."
        fi
        ;;
    smoke) ;;
esac

# --- 11. Start ---------------------------------------------------------------------------------

if [ "$CHANNEL" = "smoke" ]; then
    say "Installed $APP. The smoke app is started only by scripts/smoke-macos.sh, with a throwaway HOME."
elif [ "$START" -eq 1 ]; then
    lock_phase starting
    say "Starting $NAME in the background..."
    # -g: it opens behind whatever is in front, without taking focus.
    open -g -a "$APP" --args --background 9>&- || die "open couldn't start $APP"
else
    say "Installed $APP (not started: --no-start)."
fi

# --- 12. Done: free the lock -------------------------------------------------------------------

lock_phase started
release_lock
say "Done."
