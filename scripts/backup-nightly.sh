#!/bin/sh
# Once a day, the keyboard's state from the owner's phone into
# ~/Backups/ccez-keys (keys backup keeps the newest 30). Run hourly by a
# launchd agent on the Mac mini: a day's backup is taken the first hour the
# phone is reachable (USB, or adb over the tailnet). Emulators are skipped.
#   scripts/backup-nightly.sh
PATH="$HOME/.cargo/bin:$HOME/Library/Android/sdk/platform-tools:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
DIR="$HOME/Backups/ccez-keys"
[ -f "$DIR/$(date +%F).json" ] && exit 0
for s in $(adb devices | awk 'NR>1 && $2=="device" {print $1}'); do
  case "$s" in emulator-*) continue ;; esac
  adb -s "$s" shell pm path uk.ccez.keys >/dev/null 2>&1 || continue
  KEYS_SERIAL="$s" keys backup "$DIR" && exit 0
done
exit 0
