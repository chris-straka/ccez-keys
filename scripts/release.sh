#!/usr/bin/env bash
# A release on GitHub (public): the signed APK and its SHA-256. The keyboard
# finds it there and installs it itself (android/.../Updater.kt).
#   scripts/release.sh
set -euo pipefail
cd "$(dirname "$0")/.."
[ -z "$(git status --porcelain)" ] || { echo "commit first" >&2; exit 1; }
NO_INSTALL=1 scripts/build.sh
CODE="$(git rev-list --count HEAD)"
TAG="v0.1.$CODE"
APK="android/app/build/outputs/apk/release/app-release.apk"
OUT="$(mktemp -d)"
cp "$APK" "$OUT/ccez-keys-$TAG.apk"
(cd "$OUT" && shasum -a 256 "ccez-keys-$TAG.apk" > "ccez-keys-$TAG.apk.sha256")
if gh release view "$TAG" >/dev/null 2>&1; then echo "$TAG is released: commit something new first" >&2; exit 1; fi
# A tag left by a run that stopped before its release is used again.
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null || git tag "$TAG"
git push -q origin "$TAG"
gh release create "$TAG" "$OUT/ccez-keys-$TAG.apk" "$OUT/ccez-keys-$TAG.apk.sha256" --title "ccez-keys $TAG" --notes "$(git log -1 --format=%s)" --latest
echo "released $TAG"
