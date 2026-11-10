#!/usr/bin/env bash
# Builds dist/ccez-keys.app (the Mac preview app and its Swift translation
# helper) and signs it with the owner's stable self-signed certificate (the
# one ccez-clip uses, ~/.config/ccez-clip), so Accessibility stays granted
# across updates.
#   scripts/mac-bundle.sh [version]     version defaults to 0.1.<commit count>
set -euo pipefail
cd "$(dirname "$0")/.."
DIR="${CCEZ_SIGNING:-$HOME/.config/ccez-clip}"
KC="$DIR/signing.keychain-db"
[ -f "$KC" ] || { echo "no signing keychain (ccez-clip's scripts/make-cert.sh makes it)"; exit 1; }
VERSION="${1:-0.1.$(git rev-list --count HEAD)}"
SHA1="$(cat "$DIR/signing-cert.sha1")"

cargo build --release -p ccez-keys-desktop
APP=dist/ccez-keys.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
# The bundle keeps its executable name (Accessibility knows the app by it).
cp target/release/ccez-keys-desktop "$APP/Contents/MacOS/ccez-keys-mac"
swiftc -O -o "$APP/Contents/MacOS/ccez-keys-translate" desktop/swift/translate.swift
sed "s/@VERSION@/$VERSION/g" desktop/Info.plist > "$APP/Contents/Info.plist"

security unlock-keychain -p "$(cat "$DIR/keychain-password")" "$KC"
sign() { codesign --force --timestamp=none --keychain "$KC" --sign "$SHA1" "$@"; }
sign --identifier uk.ccez.keys.mac.translate "$APP/Contents/MacOS/ccez-keys-translate"
sign "$APP"
codesign --verify --deep --strict "$APP"
echo "$APP $VERSION"
