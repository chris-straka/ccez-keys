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

## Words on screen and videos (2026-11-12)

Learning words (float.rs) go only where the front window has no content.
"While a video plays" off: they leave while one plays.

- A video is playing when something holds the display awake (browsers do),
  or when the front window has a playing video with the browser's own
  controls. Claude's videos hold nothing, so only the second sees them.
- Such a video has a shape in the Accessibility tree (nothing names it): a
  group whose first child is an empty, classless group over the same
  frame. Other things have that shape too (Discord has one: the first
  build took it for a video and hid the words there), so it counts only
  with its time slider, which shows whenever it's paused or the pointer is
  on it. Seen so, its frame is content and no word sits on it; its play
  button carries the class "pause" unless it's playing. While it plays the
  controls leave and only the shape is left: it's still that video, playing,
  where one of its size was seen with controls in the last ten minutes
  (idle.rs `Videos`, tested on Claude's layout).
- Not seen: a silent or muted video with a site's own controls (YouTube
  muted, Discord), which Accessibility shows as an empty pane, and one
  that starts by itself and never shows controls.
- `ccez-keys-mac --look <pid>` prints what the words see in that app's
  window (content count, video shapes, free spots); mac.log notes a video
  in the front window starting (app and size) and stopping.
- Checks never open a browser window on the owner's Mac: a new Chromium
  takes focus and asks for the keychain password (owner, 2026-11-12).

## Windows and Linux (built 2026-11-10)

Owner asked for both; not Tauri: one Rust app (`desktop/`), the Mac's own
panel, egui on Linux and Windows. What it does and when is shared code
(`preview.rs`, tested), so a change to the rules changes all three.

- Translation: Mozilla's Firefox Translations models through fxtranslate
  (pure Rust), ~35-50 MB a language, fetched once from this repo's
  `models-firefox-1` release (`scripts/desktop-models.py`), then offline.
  A question typed without its "?" gets one first (the models drop the verb).
- Readings: the core's own Japanese and Chinese dictionaries.
- Linux: the field through AT-SPI (focus and caret events, a tree walk at
  start), replaced through EditableText; speech through `spd-say`. The
  preview is an X11 override-redirect window (never focused). GNOME on
  Wayland can't place windows: there it sits where the compositor puts it.
  Settings: the app list's "ccez-keys" (`--settings`); GNOME has no tray.
  Checked on f in a virtual screen (Xvfb) against a GTK field: reads it,
  translates, flags, furigana, card, right-click replace.
- Windows: the field through UI Automation (on its own thread, 300 ms
  limit), replaced by selecting the sentence and typing; speech through
  System.Speech; the window never activates (WS_EX_NOACTIVATE); a tray icon
  (Settings…, Pause, Quit). Built and tested on GitHub's Windows machines;
  no one has seen it on screen (the owner has no Windows PC).
- Releases: GitHub builds both on each release tag and attaches
  `ccez-keys-linux-<tag>.tar.gz` (with install.sh) and
  `ccez-keys-windows-<tag>.zip`.
- Not yet on Linux and Windows: updates by themselves, the cursor's own
  position on Windows (the field's frame is used), Arabic shaping (egui
  draws letters unjoined), the card's meaning-here (Apple's model only).
