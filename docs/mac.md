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

- Each language on its own line (2/4/6/8/All, default 6), furigana and
  per-character pinyin, kept clear of the box being typed in (under it, or over it when there is no room), lingers 8 s; a language still coming holds its line with "…".
- Known words are skipped: a language whose words are all known isn't shown.
- Words are coloured by status (Learning tinted); clicking one opens a card
  (meaning, Learning / Known) without taking focus. Statuses sync with
  ccez-words every 5 minutes; the server comes from
  `~/.config/ccez-keys/words-server` (one line, outside the repo).
- Dictionaries (ja, zh) come from ccez-lang-core's release, checked
  against SHA256SUMS.
- Not yet: Muse rewording, the card's "here" meaning, hover to hear.

## Next (owner, 2026-11-09)

- Card meanings from Apple's on-device model (Foundation Models, macOS 26+):
  the word's meaning in this sentence, free and offline, cached; the
  dictionary's when the model isn't there. On the owner's Mac it reports
  "not ready" (Apple Intelligence off or still downloading).
- The menu's "ⓘ Translation languages…" opens System Settings' Language &
  Region page (Translation Languages is there) and names any language the
  Mac couldn't translate into.

## Windows, later (owner, 2026-11-09)

The same app for Microsoft Windows, sharing the Rust core and the menu's
choices:
- The focused field and its cursor through UI Automation (TextPattern2's
  caret range and bounds), a tray icon in place of the menu bar.
- Windows has no built-in offline translator for apps: Firefox's offline
  models (Bergamot) bundled per language, or Windows' on-device AI where the
  PC has it. The ⓘ item then opens the app's own language downloads.
- Speech through Windows' own voices (natural ones where installed).
