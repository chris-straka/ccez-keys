# ccez-keys

Read `~/AGENTS.md` and `~/SWE/AGENTS.md` first, then `PLAN.md`.

- The layout is the owner's call: change `docs/layout.html` and show it
  (as an Artifact link, with the screens) before building UI that differs.
- Check on the owner's S24 (`adb -s <serial>`, never a bare adb).
  Dark only. Tests and examples in the languages the owner learns, not English.
- A keyboard sees everything typed: never log typed text, never send it
  anywhere from a password or no-learning field, and say in the commit when
  a change sends typed text to a new place.
- Wording: pithy, no helper text, icon names only as hold labels.
- This repo is public: no tailnet hostnames, serials, keys or personal data.
  Server addresses (ccez-words, update mirror) are settings set with `keys`.

## Building and checking

- `scripts/build.sh` builds the release APK (key in `~/.config/ccez-keys`,
  never regenerate it) and installs it on the connected phone.
- `keys status | get | set | schema | log` (cli/) talks to the keyboard
  over adb; `keys log` holds its events and errors (never typed text).
- `scripts/phone-type.py "deja "` types on the phone by tapping keys, for
  checks; screenshot with `adb exec-out screencap -p`.
- Never `am force-stop uk.ccez.keys` while it's the chosen keyboard:
  Android falls back to Samsung Keyboard. Put the owner's keyboard back
  after a check (`ime set com.touchtype.swiftkey/com.touchtype.KeyboardService`).
