#!/usr/bin/env bash
# Gate: preview-languages (goal.md beside this file). A setting chooses
# which of the owner's learning languages the strip previews; its default
# keeps today's previews; it survives a restart; the repo's own checks
# stay green; the core's public API stays as the launcher builds it.
#
# Through fleet, from the repo (an Android SDK with the NDK, cargo-ndk):
#   fleet run --snapshot --needs adb --cores 6 -- bash gates/preview-languages/gate.sh
# Every check prints PASS or FAIL (details indented under it); a check that
# can't run is a FAIL. The last line is GATE PASS (exit 0) or GATE FAIL
# (exit 1). POSIX ERE only (no \b or \w: macOS lacks them).
set -u
cd "$(git rev-parse --show-toplevel)" || exit 2
export LC_ALL=C

G=gates/preview-languages
TESTS=core/tests/preview_languages.rs
STRIP=android/app/src/main/kotlin/uk/ccez/keys/learn/Strip.kt
UI=android/app/src/main/kotlin/uk/ccez/keys/ui/KeyboardUi.kt
OUT="${FLEET_ARTIFACTS:-$PWD/gate-out}/preview-languages"
rm -rf "$OUT"
mkdir -p "$OUT"
# Fleet gives each job a build folder of its own (CARGO_TARGET_DIR, warm
# from earlier jobs). Without one, this run's own, removed at exit.
if [ -z "${CARGO_TARGET_DIR:-}" ]; then
  export CARGO_TARGET_DIR="$PWD/target/gate-preview-$$"
  trap 'rm -rf "$CARGO_TARGET_DIR"' EXIT
fi
FAILS=0
pass() { echo "PASS  $1"; }
fail() {
  echo "FAIL  $1"
  FAILS=$((FAILS + 1))
}
detail() { sed 's/^/      /'; }

# --- 1. The core's public API and settings, as at main -----------------------
sed 's/^[[:space:]]*//' core/src/*.rs >"$OUT/src-trimmed.txt"
gone=""
while IFS= read -r l; do
  case "$l" in '' | '#'*) continue ;; esac
  grep -qxF -- "$l" "$OUT/src-trimmed.txt" || gone="$gone$l"$'\n'
done <"$G/api-base.txt"
if [ -z "$gone" ]; then
  pass "every public core function from main is unchanged ($G/api-base.txt)"
else
  fail "public core functions changed or gone (the launcher builds this core):"
  printf '%s' "$gone" | head -8 | detail
fi
gone=""
while IFS= read -r k; do
  case "$k" in '' | '#'*) continue ;; esac
  grep -qF -- "key: \"$k\"" core/src/settings.rs || gone="$gone$k "
done <"$G/keys-base.txt"
if [ -z "$gone" ]; then
  pass "every setting from main is still there"
else
  fail "settings gone: $gone"
fi

# --- 2. The core's tests: the new ones by name, and the whole suite ----------
if [ -f "$TESTS" ]; then
  (cargo test --release -p ccez-keys-core --no-fail-fast 2>&1) >"$OUT/core-tests.log"
  core_rc=$?
else
  echo "no $TESTS" >"$OUT/core-tests.log"
  core_rc=99
fi
for t in \
  by_default_every_learning_language_previews_as_today \
  by_default_the_owners_own_lists_preview_as_today \
  hidden_languages_leave_the_previews_and_the_order_stays \
  the_owners_case_a_few_of_many \
  typing_a_learning_language_still_previews_in_the_default \
  hiding_every_language_leaves_no_previews \
  typing_languages_and_the_learning_list_stay_as_they_are \
  the_setting_survives_a_restart \
  an_older_settings_file_without_it_previews_every_language \
  it_is_in_the_schema_takes_language_codes_only; do
  if grep -qxF "test $t ... ok" "$OUT/core-tests.log"; then
    pass "core test $t"
  else
    fail "core test $t"
    grep -E "^test $t |^---- $t |panicked at|assertion .*failed|^  left:|^ right:" "$OUT/core-tests.log" | head -6 | detail
  fi
done
if [ "$core_rc" -eq 0 ] && grep -q '^test result: ok' "$OUT/core-tests.log" && ! grep -q '^test result: FAILED' "$OUT/core-tests.log"; then
  pass "every core test passes (cargo test --release -p ccez-keys-core)"
else
  fail "core tests fail or don't build (cargo test exited $core_rc)"
  grep -E '^error(\[E[0-9]+\])?:|^test .* FAILED$|^failures:$|^    [a-z_]+$' "$OUT/core-tests.log" | head -12 | detail
fi

# --- 3. The Android app builds and its unit tests pass ------------------------
SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$([ "$(uname)" = Darwin ] && echo "$HOME/Library/Android/sdk" || echo "$HOME/Android/Sdk")}}"
export ANDROID_HOME="$SDK"
if [ -d "$SDK" ]; then
  echo "sdk.dir=$SDK" >android/local.properties
  # The phone's processor only (the bindings are read from its library).
  (cd android && CCEZ_KEYS_ABIS=arm64-v8a ./gradlew --console=plain --no-daemon :app:testDebugUnitTest 2>&1) >"$OUT/android-tests.log"
  gradle_rc=$?
else
  echo "no Android SDK at $SDK" >"$OUT/android-tests.log"
  gradle_rc=99
fi
results=android/app/build/test-results/testDebugUnitTest
ran=$(cat "$results"/*.xml 2>/dev/null | grep -oE '<testsuite [^>]*' | grep -oE ' tests="[0-9]+"' | grep -oE '[0-9]+' | awk '{s+=$1} END {print s+0}')
bad=$(cat "$results"/*.xml 2>/dev/null | grep -oE '<testsuite [^>]*' | grep -oE ' (failures|errors)="[0-9]+"' | grep -oE '[0-9]+' | awk '{s+=$1} END {print s+0}')
if [ "$gradle_rc" -eq 0 ] && [ "${ran:-0}" -gt 0 ] && [ "${bad:-0}" -eq 0 ]; then
  pass "Android app builds and its unit tests pass ($ran tests)"
else
  fail "Android build or unit tests fail (gradle exited $gradle_rc)"
  grep -E '^e: |error:|FAILED|What went wrong|> ' "$OUT/android-tests.log" | head -12 | detail
fi

# --- 4. The strip and the settings panel use the setting ----------------------
if grep -qF 'stripLanguages(' "$STRIP" && ! grep -qF 'languages.learning' "$STRIP"; then
  pass "the strip takes its languages from the core's stripLanguages ($STRIP)"
else
  fail "the strip doesn't take its languages from the core's stripLanguages ($STRIP)"
  grep -nF -e 'languages.learning' -e 'stripLanguages' "$STRIP" | head -4 | detail
fi
if grep -qF '"learning.strip_hidden"' "$UI" && grep -qF 'Heading("Preview")' "$UI"; then
  pass "settings has the Preview checklist ($UI)"
else
  fail "settings has no Preview checklist writing learning.strip_hidden ($UI)"
fi

if [ "$FAILS" -eq 0 ]; then
  echo "GATE PASS"
  exit 0
fi
echo "GATE FAIL"
exit 1
