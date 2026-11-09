#!/usr/bin/env bash
# How heavy typing is: types a paragraph in the app's own test field and
# reports the keyboard's CPU time, its dropped frames, and the typing-speed
# line it logs (key handling times, translations, AI calls). Never on the
# owner's typing: it types its own text.
#   KEYS_SERIAL=<serial> scripts/perf.sh ["text to type"]
set -euo pipefail
cd "$(dirname "$0")/.."
S="${KEYS_SERIAL:?set KEYS_SERIAL}"
TEXT="${1:-i think we should meet at the cafe tomorrow morning. the weather looks good and i want to try the new bakery near the station. let me know if that works for you.}"
a() { adb -s "$S" shell "$@"; }
a am start -n uk.ccez.keys/.MainActivity >/dev/null 2>&1; sleep 2
a input tap 540 570; sleep 1
a input keyevent 123; for _ in $(seq 1 40); do a input keyevent 67; done
PID=$(a pidof uk.ccez.keys | tr -d '\r')
ticks() { a cat /proc/$PID/stat | awk '{print $14+$15}'; }
a dumpsys gfxinfo uk.ccez.keys reset >/dev/null
T0=$(ticks); START=$(date +%s)
python3 scripts/phone-type.py "$TEXT"
sleep 4  # the strip's and the AI's work after the last key
T1=$(ticks); END=$(date +%s)
a input keyevent 123; for _ in $(seq 1 ${#TEXT}); do :; done
a input keyevent 3; sleep 1   # closing the field logs typing-speed
echo "cpu: $(( (T1 - T0) * 10 )) ms over $(( END - START )) s for ${#TEXT} keys"
a dumpsys gfxinfo uk.ccez.keys | grep -E "Total frames rendered|Janky frames|90th percentile|99th percentile" | sed 's/^ *//'
cargo run -q --release -p ccez-keys-cli --bin keys -- log -n 20 | grep typing-speed | tail -1
