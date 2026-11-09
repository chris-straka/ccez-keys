#!/usr/bin/env bash
# Typing checks on a device (the emulator by default): types real key
# sequences with scripts/type.py into the app's test field and compares
# what the field holds. Language and statuses are put back after.
#   KEYS_SERIAL=emulator-5554 scripts/typing-check.sh
set -uo pipefail
cd "$(dirname "$0")/.."
S="${KEYS_SERIAL:?set KEYS_SERIAL}"
export KEYS_SERIAL="$S"
a() { adb -s "$S" shell "$@"; }
keys() { cargo run -q --release -p ccez-keys-cli --bin keys -- "$@"; }
was=$(keys status | python3 -c 'import json,sys; print(json.load(sys.stdin)["current"])')
a settings put secure show_ime_with_hard_keyboard 1
# The app asks for these on start; a dialog over the field would take the taps.
a pm grant uk.ccez.keys android.permission.RECORD_AUDIO 2>/dev/null || true
a pm grant uk.ccez.keys android.permission.CAMERA 2>/dev/null || true
a am start -n uk.ccez.keys/.MainActivity >/dev/null 2>&1; sleep 2
field() {
  a uiautomator dump /sdcard/ck.xml >/dev/null 2>&1
  a cat /sdcard/ck.xml | python3 -c 'import re,sys,html; m=re.search(r"<node[^>]*class=\"android.widget.EditText\"[^>]*>", sys.stdin.read()); t=re.search(r" text=\"([^\"]*)\"", m.group(0)) if m else None; print(html.unescape(t.group(1)) if t else "")'
}
# Both ways: a field's text can sit after the cursor (seen with Gboard too).
clear_field() { a input tap 540 570; sleep 0.5; a "input keyevent 123; for i in \$(seq 1 40); do input keyevent 67; done; input keyevent 122; for i in \$(seq 1 40); do input keyevent 112; done"; }
pass=0; fail=0
check() { # lang, typed, expected, name
  clear_field; keys use "$1" >/dev/null; a input tap 540 570; sleep 1.2
  scripts/type.py "$2" || { echo "FAIL $4: could not type"; fail=$((fail+1)); return; }
  sleep "${WAIT:-1.5}"
  got=$(field)
  if [ "$got" = "$3" ]; then echo "ok   $4"; pass=$((pass+1)); else echo "FAIL $4: got [$got] want [$3]"; fail=$((fail+1)); fi
}
check en "teh dog " "the dog " "autocorrect"
check en "teh ⌫" "teh" "backspace undoes a correction"
check en "hello a" "hello a" "fast typing keeps words whole"
check en "hi  " "hi. " "double space"
check en "swim ⌫ming " "swimming " "letters after a word go on with it"
check en "guessin " "guessing " "missed last letter"
check de "einerr " "einer " "German capitals aren't rivals"
check ja "konnichiha" "こんにちは" "Japanese nn before a vowel"
check ja "minna" "みんな" "Japanese minna"
check hi "नमस्ते " "नमस्ते " "Hindi vowel signs stay in the word"
check ko "ㅎㅏㄴ" "한" "Korean syllable"
check ru "привет " "привет " "Russian"
check th "สวัสดี " "สวัสดี " "Thai marks stay in the word"
# One clearly French word switches to French.
check en "chez " "chez " "chez typed on English"
now=$(keys status | python3 -c 'import json,sys; print(json.load(sys.stdin)["current"])')
if [ "$now" = fr ]; then echo "ok   chez switches to French"; pass=$((pass+1)); else echo "FAIL chez switches to French: on $now"; fail=$((fail+1)); fi
clear_field; keys use "$was" >/dev/null; a input keyevent 3
echo "$pass passed, $fail failed"
[ "$fail" = 0 ]
