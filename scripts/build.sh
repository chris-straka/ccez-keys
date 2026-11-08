#!/usr/bin/env bash
# Builds the release APK, signed with the key in ~/.config/ccez-keys (never
# regenerate it: a new key means uninstalling the keyboard and its words),
# and installs it on the phone when one is connected.
#   scripts/build.sh            build + install
#   NO_INSTALL=1 scripts/build.sh
set -euo pipefail
cd "$(dirname "$0")/.."
KEYDIR="$HOME/.config/ccez-keys"
export CCEZ_KEYS_KEYSTORE="$KEYDIR/release.jks"
export CCEZ_KEYS_KEY_PASSWORD="$(cat "$KEYDIR/password")"
export CCEZ_KEYS_VERSION_CODE="$(git rev-list --count HEAD)"
export CCEZ_KEYS_VERSION_NAME="0.1.$CCEZ_KEYS_VERSION_CODE"
# The phone only (an emulator build: CCEZ_KEYS_ABIS=arm64-v8a,x86_64).
export CCEZ_KEYS_ABIS="${CCEZ_KEYS_ABIS:-arm64-v8a}"
(cd android && ./gradlew --console=plain -q assembleRelease)
APK=android/app/build/outputs/apk/release/app-release.apk
echo "built $APK ($CCEZ_KEYS_VERSION_NAME)"
[ "${NO_INSTALL:-}" = 1 ] && exit 0
SERIAL="${KEYS_SERIAL:-$(adb devices | awk 'NR>1 && $2=="device" {print $1; exit}')}"
[ -z "$SERIAL" ] && { echo "no phone connected; not installed"; exit 0; }
adb -s "$SERIAL" install -r "$APK" >/dev/null && echo "installed on $SERIAL"
