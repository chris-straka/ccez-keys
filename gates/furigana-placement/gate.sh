#!/usr/bin/env bash
# Gate: furigana-placement (goal.md beside this file). The review word's
# reading is split by kanji run, each part centred over its own kanji,
# kana with nothing over them, the whole reading over the whole word when
# the split fails or could go two ways; the repo's own checks stay green;
# the core's public API stays as the launcher builds it.
#
# Through fleet, from the repo (an Android SDK with the NDK, cargo-ndk):
#   fleet run --snapshot --needs adb --cores 6 -- bash gates/furigana-placement/gate.sh
# Every check prints PASS or FAIL (details indented under it); a check that
# can't run is a FAIL. The last line is GATE PASS (exit 0) or GATE FAIL
# (exit 1). POSIX ERE only (no \b or \w: macOS lacks them).
set -u
cd "$(git rev-parse --show-toplevel)" || exit 2
export LC_ALL=C

G=gates/furigana-placement
TESTS=core/tests/ruby.rs
KTEST=android/app/src/test/kotlin/uk/ccez/keys/RubyTest.kt
REVIEW=android/app/src/main/kotlin/uk/ccez/keys/learn/Review.kt
STRIPUI=android/app/src/main/kotlin/uk/ccez/keys/ui/StripUi.kt
RUBYUI=android/app/src/main/kotlin/uk/ccez/keys/ui/Ruby.kt
OUT="${FLEET_ARTIFACTS:-$PWD/gate-out}/furigana-placement"
rm -rf "$OUT"
mkdir -p "$OUT"
# Fleet gives each job a build folder of its own (CARGO_TARGET_DIR, warm
# from earlier jobs). Without one, this run's own, removed at exit.
if [ -z "${CARGO_TARGET_DIR:-}" ]; then
  export CARGO_TARGET_DIR="$PWD/target/gate-ruby-$$"
  trap 'rm -rf "$CARGO_TARGET_DIR"' EXIT
fi
FAILS=0
pass() { echo "PASS  $1"; }
fail() {
  echo "FAIL  $1"
  FAILS=$((FAILS + 1))
}
detail() { sed 's/^/      /'; }

# --- 1. The core's public API and settings, as at the base ---------------------
sed 's/^[[:space:]]*//' core/src/*.rs >"$OUT/src-trimmed.txt"
gone=""
while IFS= read -r l; do
  case "$l" in '' | '#'*) continue ;; esac
  grep -qxF -- "$l" "$OUT/src-trimmed.txt" || gone="$gone$l"$'\n'
done <"$G/api-base.txt"
if [ -z "$gone" ]; then
  pass "every public core function from the base is unchanged ($G/api-base.txt)"
else
  fail "public core functions changed or gone (the launcher builds this core):"
  printf '%s' "$gone" | head -8 | detail
fi
if grep -qF 'pub struct Ruby {' core/src/learn.rs && grep -qF 'pub rt: String,' core/src/learn.rs; then
  pass "the core's Ruby record is unchanged"
else
  fail "the core's Ruby record changed (core/src/learn.rs)"
fi

# --- 2. The core's tests: the split by name, and the whole suite ---------------
if [ -f "$TESTS" ]; then
  (cargo test --release -p ccez-keys-core --no-fail-fast 2>&1) >"$OUT/core-tests.log"
  core_rc=$?
else
  echo "no $TESTS" >"$OUT/core-tests.log"
  core_rc=99
fi
for t in \
  the_owners_word_okurigana_in_the_middle_and_at_the_end \
  okurigana_at_the_end \
  okurigana_in_the_middle \
  kana_before_the_kanji \
  one_kanji_run_takes_the_whole_reading \
  a_split_that_could_go_two_ways_falls_back_to_the_whole_word \
  a_reading_that_doesnt_fit_falls_back_to_the_whole_word \
  words_without_kanji_get_nothing_over_them \
  no_reading_gets_nothing_over_it \
  katakana_in_the_word_or_the_reading_match_hiragana \
  the_readings_of_a_split_spell_the_reading; do
  if grep -qxF "test $t ... ok" "$OUT/core-tests.log"; then
    pass "core test $t"
  else
    fail "core test $t"
    grep -E "^test $t |^---- $t |panicked at|assertion .*failed|^  left:|^ right:" "$OUT/core-tests.log" | head -6 | detail
  fi
done
# typing.rs's fast_enough_for_each_key times suggestions against a clock
# (4 ms each): on a busy fleet machine it can miss with nothing changed.
# When it is the only failure, it runs again alone, up to three times.
failed=$(sed -n '/^failures:$/,/^test result:/p' "$OUT/core-tests.log" | grep -E '^    [A-Za-z0-9_:]+$' | sed 's/^ *//' | sort -u)
timing_ok=0
if [ "$core_rc" -ne 0 ] && [ "$failed" = "fast_enough_for_each_key" ] && [ "$(grep -c '^test result: FAILED' "$OUT/core-tests.log")" = 1 ]; then
  for n in 1 2 3; do
    if cargo test --release -p ccez-keys-core --test typing -- --exact fast_enough_for_each_key >"$OUT/timing-$n.log" 2>&1; then
      timing_ok=1
      echo "      (fast_enough_for_each_key missed its time in the full run, passed alone on try $n)"
      break
    fi
  done
fi
if { [ "$core_rc" -eq 0 ] || [ "$timing_ok" = 1 ]; } && grep -q '^test result: ok' "$OUT/core-tests.log"; then
  pass "every core test passes (cargo test --release -p ccez-keys-core)"
else
  fail "core tests fail or don't build (cargo test exited $core_rc)"
  grep -E '^error(\[E[0-9]+\])?:|^test .* FAILED$|^failures:$|^    [a-z_]+$' "$OUT/core-tests.log" | head -12 | detail
fi

# --- 3. The Android app builds; its unit tests pass, the placement's by name ---
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
xml="$results/TEST-uk.ccez.keys.RubyTest.xml"
for t in \
  oneReadingIsCentredOverItsOwnBase \
  theOwnersWordEachRunGetsItsOwnReading \
  wideReadingsSpreadButNeverOverlapTheNext \
  kanaWithoutReadingsNeitherPushNorArePushed \
  aWordWithNoReadingsHasNothingToPlace \
  theWholeReadingOverTheWholeWordIsCentred \
  theListsMustMatch; do
  # A passing JUnit case is a <testcase .../> or <testcase ...></testcase> with no <failure>, <error> or <skipped> in it.
  case_xml=$(python3 - "$xml" "$t" <<'PY'
import sys, xml.etree.ElementTree as ET
try:
    root = ET.parse(sys.argv[1]).getroot()
except Exception:
    print("missing"); sys.exit()
for c in root.iter("testcase"):
    if c.get("name") == sys.argv[2]:
        print("bad" if any(k.tag in ("failure", "error", "skipped") for k in c) else "ok"); sys.exit()
print("missing")
PY
)
  if [ "$case_xml" = ok ]; then
    pass "placement test $t"
  else
    fail "placement test $t ($case_xml)"
  fi
done

# --- 4. The review word draws its reading through the split and the placement --
if grep -qF 'rubySplit(' "$REVIEW"; then
  pass "the review word's reading is split by the core ($REVIEW)"
else
  fail "the review word's reading isn't split by the core's rubySplit ($REVIEW)"
fi
calls=$(cat "$STRIPUI" "$RUBYUI" 2>/dev/null | grep -F 'rubyStarts(' | grep -vcF 'fun rubyStarts(')
if [ -f "$RUBYUI" ] && grep -qF 'fun rubyStarts(' "$RUBYUI" && [ "${calls:-0}" -ge 1 ] && grep -qF 'RubyWord(' "$STRIPUI" && ! grep -qF 'Text(f.reading' "$STRIPUI"; then
  pass "the review row draws the word with RubyWord, placed by rubyStarts"
else
  fail "the review row doesn't draw the word with RubyWord placed by rubyStarts ($STRIPUI, $RUBYUI)"
fi

if [ "$FAILS" -eq 0 ]; then
  echo "GATE PASS"
  exit 0
fi
echo "GATE FAIL"
exit 1
