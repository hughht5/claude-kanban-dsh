#!/bin/sh
# Builds kb for the macOS app bundle (card #117). `cargo tauri build` runs this as its
# beforeBuildCommand when src-tauri/tauri.macos.bundle.json is passed with --config (install.sh
# and CI do that; a plain `cargo test` never does). The bundle takes kb as an external binary,
# so Tauri needs, before it compiles the app:
#
#   src-tauri/binaries/kb-<target triple>        for each cargo build it makes, and
#   src-tauri/binaries/kb-universal-apple-darwin for a universal bundle (the two joined).
#
# Tauri passes TAURI_ENV_TARGET_TRIPLE (aarch64-apple-darwin, x86_64-apple-darwin or
# universal-apple-darwin) and TAURI_ENV_DEBUG=true for a debug build. Without them this builds
# for this Mac (rustc's host) in release mode.
#
# kb gets the build's channel from KANBAN_CHANNEL at compile time (crates/store/src/channel.rs),
# so the test app's kb talks to the test app. It is built into a target folder per channel,
# target/live/kb or target/test/kb, never into the app's own target folder: tauri-build copies
# the sidecar into <app target>/<triple>/release/kb, and a kb built there would be overwritten
# by, or overwrite, the other channel's.
#
# Any failure stops the build with a message.

set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd -P)

die() {
    printf 'mac-kb.sh: %s\n' "$*" >&2
    exit 1
}

case ":$PATH:" in
    *":$HOME/.cargo/bin:"*) ;;
    *)
        if [ -d "$HOME/.cargo/bin" ]; then
            PATH="$HOME/.cargo/bin:$PATH"
            export PATH
        fi
        ;;
esac
command -v cargo >/dev/null 2>&1 || die "cargo isn't on PATH (install Rust with rustup)"

HOST=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')
TRIPLE=${TAURI_ENV_TARGET_TRIPLE:-$HOST}
[ -n "$TRIPLE" ] || die "no target triple: TAURI_ENV_TARGET_TRIPLE is unset and rustc -vV gave no host"

if [ "${TAURI_ENV_DEBUG:-}" = "true" ]; then
    PROFILE=debug
    RELEASE=""
else
    PROFILE=release
    RELEASE="--release"
fi

if [ "${KANBAN_CHANNEL:-}" = "test" ]; then
    CHANNEL=test
else
    CHANNEL=live
fi
KB_TARGET="$ROOT/target/$CHANNEL/kb"
OUT="$ROOT/src-tauri/binaries"
mkdir -p "$OUT" || die "can't make $OUT"

# Builds kb for one triple and copies it to src-tauri/binaries/kb-<triple>.
build_one() {
    triple=$1
    printf 'mac-kb.sh: building kb (%s channel, %s) for %s into %s\n' "$CHANNEL" "$PROFILE" "$triple" "$KB_TARGET"
    # $RELEASE is empty or one word, so it is left unquoted on purpose.
    # shellcheck disable=SC2086
    (cd "$ROOT" && CARGO_TARGET_DIR="$KB_TARGET" cargo build -p kb --target "$triple" $RELEASE) ||
        die "cargo build -p kb --target $triple failed"
    built="$KB_TARGET/$triple/$PROFILE/kb"
    [ -f "$built" ] || die "cargo finished, but $built is missing"
    cp "$built" "$OUT/kb-$triple.tmp" || die "can't copy $built to $OUT"
    chmod 755 "$OUT/kb-$triple.tmp"
    mv -f "$OUT/kb-$triple.tmp" "$OUT/kb-$triple" || die "can't write $OUT/kb-$triple"
}

case "$TRIPLE" in
    universal-apple-darwin)
        command -v lipo >/dev/null 2>&1 || die "lipo is missing (install the Xcode Command Line Tools: xcode-select --install)"
        # Tauri compiles the app once per architecture and wants each one's kb, then the
        # joined kb for the universal bundle.
        build_one aarch64-apple-darwin
        build_one x86_64-apple-darwin
        rm -f "$OUT/kb-universal-apple-darwin.tmp"
        lipo -create -output "$OUT/kb-universal-apple-darwin.tmp" \
            "$OUT/kb-aarch64-apple-darwin" "$OUT/kb-x86_64-apple-darwin" ||
            die "lipo couldn't join the two kb builds"
        chmod 755 "$OUT/kb-universal-apple-darwin.tmp"
        mv -f "$OUT/kb-universal-apple-darwin.tmp" "$OUT/kb-universal-apple-darwin" ||
            die "can't write $OUT/kb-universal-apple-darwin"
        printf 'mac-kb.sh: kb-universal-apple-darwin has %s\n' "$(lipo -archs "$OUT/kb-universal-apple-darwin")"
        ;;
    *-apple-darwin)
        build_one "$TRIPLE"
        ;;
    *)
        die "this script builds kb for macOS only, not for $TRIPLE (Windows builds kb with cargo build -p kb)"
        ;;
esac

printf 'mac-kb.sh: done; kb for %s is in %s\n' "$TRIPLE" "$OUT"
