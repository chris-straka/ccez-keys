# ccez-keys for Mac (decided 2026-11-09)

A menu bar app, on at login: while the owner types English anywhere on the
Mac, a small preview shows the sentence in the languages they're learning,
like the phone's strip. Design page with mockups:
https://claude.ai/artifact/BMPuXSqMHX8s5msaQwv6NV

## Decided by the owner

1. Under the text cursor; where an app gives no cursor position, a band at
   the bottom of the screen.
2. As many languages as fit (the owner's order, skipping all-known ones).
3. Shows while typing, fades ~3 s after; ⌥ held brings it back.
4. Everywhere except password fields and a short list (Terminal, code
   editors, Claude), editable from the menu bar icon.
5. Click a word: the same card as the phone (meaning here via Muse,
   Learning / Known, synced through ccez-words). Hover reads it aloud.
- Long messages: only the sentence being typed, one line; a long one shows
  its end with "…" before it.
- Never Muse (and no preview) for password fields or anything that looks
  like a password, API key or token (long mixed letter/digit runs, sk-…,
  ghp_…). Security, privacy and performance first.

## How it's built (agent's call)

- Rust, like ccez-clip (menu bar, objc2), sharing ccez-keys' core and
  ccez-lang-core (dictionaries, cards, statuses, Muse).
- Reads the focused field through Accessibility (AXUIElement: value,
  selected range, bounds for the caret), never a key tap.
- Apple's on-device translation (macOS 15+, Translation framework) through
  a small Swift helper; Muse rewords after a pause, answers cached.

## Built so far

- Each language on its own line (1/2/4/6/8/All, default 6), furigana and
  per-character pinyin, kept clear of the box being typed in (under it, or over it when there is no room), shows only once typing stops (half a second) and every language is in, all together, fading in; stays 8 s (up to 20 s for a long sentence) and while the pointer is over it, fading out; a language still coming holds its line with "…".
- Known words are skipped: a language whose words are all known isn't shown.
- Words are coloured by status (Learning tinted); clicking one opens a card
  (meaning, Learning / Known) without taking focus. Statuses sync with
  ccez-words every 5 minutes; the server comes from
  `~/.config/ccez-keys/words-server` (one line, outside the repo).
- Dictionaries (ja, zh) come from ccez-lang-core's release, checked
  against SHA256SUMS.
- Click a line's flag or blank space: read aloud and copied. A pointing hand over words; the card has Copy. Right-click a line: it replaces the sentence typed (Accessibility where the app takes it, else typed over; ⌘Z undoes it). Not yet: hover to hear.

## Next (owner, 2026-11-09)

- Card meanings from Apple's on-device model (Foundation Models, macOS 26+):
  the word's meaning in this sentence, free and offline, kept on disk
  (meanings.json, 500); the dictionary's alone while the model isn't there.
  Built; on the owner's Mac the model is still downloading ("not ready"), so
  its answers are unchecked.
- The menu's "ⓘ Translation languages…" opens System Settings' Language &
  Region page (Translation Languages is there) and names any language the
  Mac couldn't translate into.

## Windows and Linux, later (owner, 2026-11-09)

Not Tauri: the hard parts are each system's own (reading the focused field,
a window that floats by the cursor without taking focus), and a web view
adds a browser engine to a few lines of text. One Rust core, a thin native
shell per system, as on the Mac.

### Linux
- The focused field and caret through AT-SPI (the accessibility bus GTK, Qt,
  Firefox and Chromium apps talk on).
- Placing a window by the cursor: X11 allows it; Wayland lets apps place
  their own windows only through layer-shell (KDE, Sway, Hyprland), so on
  GNOME Wayland the preview sits at a fixed spot.
- Translation as on Windows (Bergamot); speech through Speech Dispatcher.

### Windows

The same app for Microsoft Windows, sharing the Rust core and the menu's
choices:
- The focused field and its cursor through UI Automation (TextPattern2's
  caret range and bounds), a tray icon in place of the menu bar.
- Windows has no built-in offline translator for apps: Firefox's offline
  models (Bergamot) bundled per language, or Windows' on-device AI where the
  PC has it. The ⓘ item then opens the app's own language downloads.
- Speech through Windows' own voices (natural ones where installed).
