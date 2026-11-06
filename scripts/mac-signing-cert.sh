#!/bin/sh
# Makes the self-signed code-signing identity "Claude Kanban DSH Local" in your login keychain
# (card #117). Run it once, yourself, in Terminal on your Mac:
#
#   sh scripts/mac-signing-cert.sh
#
# Why: macOS remembers folder permissions (Documents, Desktop and so on) per signature. An
# ad-hoc signed app gets a new signature at every build, so macOS asks again after each update.
# scripts/install.sh signs with this identity when it finds it, so the permissions stay.
# It is not an Apple Developer ID: a downloaded copy still needs Open Anyway in System Settings.
#
# What it does:
#   1. makes a certificate for code signing, valid for 10 years (codesign refuses an expired
#      one, and a new one would reset the folder permissions again);
#   2. imports it and its key into the login keychain, with codesign allowed to use the key;
#   3. trusts it for code signing (macOS asks you to confirm);
#   4. lets codesign use the key without a dialog (asks for your Mac login once, here in
#      Terminal), so a build never stops on a keychain dialog in front of whatever you're doing.
#
# It refuses to run if the identity exists already. Nothing is stored except in the keychain:
# the key file and the short-lived bundle passphrase are made in a private temporary folder
# and removed at the end. The undo is printed at the end.

set -eu

NAME="Claude Kanban DSH Local"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

die() {
    printf 'mac-signing-cert.sh: %s\n' "$*" >&2
    exit 1
}

[ "$(uname -s)" = "Darwin" ] || die "this is for macOS only"
[ -f "$KEYCHAIN" ] || die "no login keychain at $KEYCHAIN"
command -v security >/dev/null 2>&1 || die "the security tool is missing"
command -v codesign >/dev/null 2>&1 || die "codesign is missing (install the Xcode Command Line Tools: xcode-select --install)"

# macOS's own openssl (LibreSSL) writes a .p12 that the keychain reads. A Homebrew OpenSSL 3
# earlier on PATH writes one it can't, unless told to use the old format.
if [ -x /usr/bin/openssl ]; then
    OPENSSL=/usr/bin/openssl
else
    OPENSSL=$(command -v openssl) || die "openssl is missing"
fi
P12_LEGACY=""
case "$("$OPENSSL" version 2>/dev/null)" in
    OpenSSL\ 3*) P12_LEGACY="-legacy" ;;
esac

if security find-identity -v -p codesigning 2>/dev/null | grep -F "\"$NAME\"" >/dev/null; then
    die "the identity \"$NAME\" exists already; nothing changed. To make a new one, remove it first (see the undo in this script's last lines)"
fi
if security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
    die "a certificate named \"$NAME\" is in the login keychain already (perhaps without its key); remove it in Keychain Access first"
fi

WORK=$(mktemp -d "${TMPDIR:-/tmp}/kanban-cert.XXXXXX") || die "can't make a temporary folder"
chmod 700 "$WORK"
cleanup() {
    rm -rf "$WORK"
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM
trap 'cleanup; exit 129' HUP

cat >"$WORK/cert.cnf" <<EOF
[ req ]
distinguished_name = dn
prompt = no
x509_extensions = ext

[ dn ]
CN = $NAME

[ ext ]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
subjectKeyIdentifier = hash
EOF

echo "1/4 Making the certificate \"$NAME\" (valid 10 years)..."
(umask 077 && "$OPENSSL" req -x509 -newkey rsa:2048 -nodes -sha256 -days 3650 \
    -config "$WORK/cert.cnf" -keyout "$WORK/key.pem" -out "$WORK/cert.pem" >/dev/null 2>&1) ||
    die "openssl couldn't make the certificate"

# A throwaway passphrase for the .p12 bundle only: never printed, never stored, gone with
# $WORK. openssl reads it from the environment. `security import` takes it only as an argument
# (-P), so for that one command it is visible to `ps` on this Mac for a moment; it protects
# nothing after the import, because the bundle is deleted at once.
P12_PHRASE=$("$OPENSSL" rand -hex 24) || die "openssl rand failed"
# $P12_LEGACY is empty or one word, so it is left unquoted on purpose.
# shellcheck disable=SC2086
(umask 077 && SK_P12_PHRASE="$P12_PHRASE" "$OPENSSL" pkcs12 -export $P12_LEGACY \
    -inkey "$WORK/key.pem" -in "$WORK/cert.pem" -name "$NAME" \
    -out "$WORK/id.p12" -passout env:SK_P12_PHRASE >/dev/null 2>&1) ||
    die "openssl couldn't make the .p12 bundle"
rm -f "$WORK/key.pem"

echo "2/4 Importing it into the login keychain (codesign may use the key)..."
security import "$WORK/id.p12" -k "$KEYCHAIN" -f pkcs12 -P "$P12_PHRASE" -T /usr/bin/codesign >/dev/null ||
    die "security import failed"
P12_PHRASE=""
rm -f "$WORK/id.p12"

echo "3/4 Trusting it for code signing. macOS asks you to confirm with Touch ID or your Mac login..."
security add-trusted-cert -p codeSign -k "$KEYCHAIN" "$WORK/cert.pem" ||
    die "the trust setting wasn't made (cancelled?). The identity is imported but not trusted: run the undo below, then this script again"

echo "4/4 Letting codesign use the key without a dialog."
echo "    The security tool now asks for your Mac login (what you type isn't shown)."
if ! security set-key-partition-list -S apple-tool:,apple:,codesign: -s -l "$NAME" "$KEYCHAIN" >/dev/null; then
    # Some macOS versions don't ask by themselves. Ask here, with echo off, and pass it on.
    # It then goes to `security` as an argument (-k), visible to `ps` on this Mac for a moment.
    echo "    It didn't ask. Type your Mac login here instead (not shown; passed to security as an argument):"
    stty -echo 2>/dev/null || true
    IFS= read -r MAC_LOGIN || MAC_LOGIN=""
    stty echo 2>/dev/null || true
    echo
    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -l "$NAME" -k "$MAC_LOGIN" "$KEYCHAIN" >/dev/null ||
        { MAC_LOGIN=""; die "set-key-partition-list failed: codesign may show a keychain dialog at the first build (click Always Allow)"; }
    MAC_LOGIN=""
fi

echo "Checking: signing a test file..."
cp /usr/bin/true "$WORK/signtest"
if codesign --force --timestamp=none -s "$NAME" "$WORK/signtest" >/dev/null 2>&1 &&
    codesign --verify "$WORK/signtest" >/dev/null 2>&1; then
    echo "Done. \"$NAME\" signs code; scripts/install.sh uses it from now on."
else
    echo "The identity is in the keychain, but a test signature failed. Run:"
    echo "  security find-identity -v -p codesigning"
    echo "It should list \"$NAME\". If not, run the undo below and this script again."
fi

cat <<EOF

To undo (in Terminal):
  security find-certificate -c "$NAME" -p "$KEYCHAIN" > /tmp/claude-kanban-dsh-local.pem
  security remove-trusted-cert /tmp/claude-kanban-dsh-local.pem
  security delete-identity -c "$NAME" "$KEYCHAIN"
  rm /tmp/claude-kanban-dsh-local.pem
After that, builds are signed ad hoc again, and macOS asks for folder permissions again after
each update.
EOF
