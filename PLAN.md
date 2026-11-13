# Bug review, 2026-11-13: left for later

- Mac: the ⌥⏎ key tap runs on the main run loop beside the 0.15 s tick's
  Accessibility reads, so every key-down can wait on a slow app
  (chatswap.rs:122): put the tap on its own thread, `on` in an atomic.

# Strip: which language leads (2026-11-13)

Owner, 2026-11-13: the line showed Hindi, then Chinese, for English typed
("Idk how it works", "Really iron out a good solution"). The strip kept a
position that walked on by itself (an all-known line moved to the next
language after 1.5 s; since 0.1.196 a translator echo moved it at once),
kept it for the rest of the sentence, and a new message starting with the
same letter counted as the same sentence. Now: worked out afresh on every
refresh, the first of the owner's languages whose line has a word not yet
known among the first four, else the first; a swipe pins a language for
that sentence only (sameSentenceAs, tested); the swipe needs 36 dp, not
16. `keys log` has a `strip-step` line per language tried (codes and
counts, never text). Checked on the emulator with the owner's settings
and statuses: both sentences stay on French.

# Review, 2026-11-12 (the code against what the owner has asked for)

Read-only, cut short; nothing here is checked on screen. Delete each as it ships.

Fixed (v0.1.192): what the phone sends to the AI (see Privacy below).

To fix:
- The phone's review word runs on its own 1/3/7/14/30-day ladder
  (`learn/Review.kt`), not the core's FSRS schedule the Mac uses and
  ccez-words syncs: a word answered on one isn't known to the other, and
  the buttons differ (Known / Later against Good / Again / Easy).
- No per-app no-AI / no-learning setting (asked below, "No-AI /
  no-learning is a setting (per app)").
- Camera text: a tap shows "word · meaning", a hold sets Learning with no
  sync; no Known, no readings (`CameraActivity.kt:167-182`).
- `updates.self` is off and `updates.server` empty by default
  (`core/src/settings.rs:88-93`): a fresh or restored phone never updates.
- The language grid is fixed rows of five with "+" last
  (`ui/KeyboardUi.kt:889-915`): twenty languages leave "+" alone.
- The Mac card is rebuilt at a new height when its meaning-here arrives,
  so its buttons move (`desktop/src/main.rs`, `view.rs`).
- Linux prints the field's text to stderr under `CCEZ_DEBUG`
  (`desktop/src/pc/app.rs:168-171`).
- Asked and unbuilt: hover reads a preview word aloud, pinyin tone
  numbers, searching the Add list, Arabic digits.

The owner's call:
- Sentences Muse reworded are kept as cache keys in `refined.json` (500),
  and 500 translated sentences in the Mac's `meanings.json`: his typed
  text on disk in plain text, by his own "answers kept on disk" note.
- The Mac's ⌥⏎ swap puts a tap on every key-down, even switched off
  (`desktop/src/chatswap.rs:122`); docs/mac.md says "never a key tap".
- Phone and Mac cards differ (which form is bold, reading over or under,
  senses unasked on the Mac, underline against tint for Learning).
- Hold a strip word for Learning and glosses under suggestions were
  removed (1d9c117, 57d44ae) with no note from him.

# Phone: learning words on screen — removed (2026-11-11)

The phone overlay (one window per word over other apps, through an
accessibility service) was built and then removed at the owner's word
("remove the overlay thing on my android phone"): ccez-launcher shows the
words on the home screen instead. Its core half stays, because the
launcher uses it: `core/src/review.rs` (FSRS, synced through ccez-words'
`/review`), `core/src/gloss.rs`, `Engine::screen_words` and the
`learning.screen*` counts. The overlay code is in git history before
this note if it's ever wanted back.

# Owner notes, 2026-11-11 (desktop preview)

Work these off and delete each when it ships.

- Checked 2026-11-11: the preview's languages can't translate in parallel.
  Apple's translator does one at a time whatever asks: six languages took
  1.9 s sent at once to one helper, 1.9 s one by one, 3.6 s from six
  helpers. What helps is asking early and the session cache (both in).

# Owner notes, 2026-11-10 (from ccez-keeps)

Work these off and delete each when it ships.

Keyboard (phone):
- SwiftKey's key placement and spacing may be better: compare. Left for
  the owner (a layout change goes through docs/layout.html first).
  Ours, measured: keys 44 dp tall, 8 dp between rows, 5 dp between keys,
  3 dp at the sides, so a letter on a 360 dp wide screen is a 30 x 44 dp
  face in a 35 dp cell; bottom row ?123, ☺, comma, space (3.8 keys),
  period, enter; shift and delete look narrower than they touch.
  SwiftKey, as I know its default (not checked side by side: it can't
  run on the emulator): about the same row height but much thinner gaps
  (2-3 dp), so each key's face is bigger for the same keyboard; no face
  key in the bottom row (emoji on its own key or held on enter), so the
  space bar is wider; and it learns where each thumb lands, shifting the
  keys' touch areas. Worth trying, if the owner wants: gaps 5 → 3 dp
  and rows 8 → 4 dp with keys 48 dp tall (about the same height). A
  SwiftKey screenshot from the S24 would settle the rest.


# Plan

Owner's ask (2026-11-08): a language-learning keyboard for Android, mostly
Rust, with its own autocorrect, clipboard (ccez-clip), emoji and kaomoji
(tap to paste, smarter in TextNow / Google Messages), dictation, an AI
provider (Muse key), settings to hide buttons, every setting controllable
by agents, many languages switched with the space bar or detected, more
layouts later, no GIFs. The core of it: while typing, preview the text in
the languages being learned, with TTS, furigana and pinyin; glosses under
suggestions in languages where the word isn't known yet (ccez-words);
add words to Learning by holding; Muse teaching on screen while typing.

## Layout (proposed, waiting on the owner)

`docs/layout.html`: five screens. Learning strip above the suggestions,
shown only while typing; suggestion row becomes the button row when the
field is empty; settings open in place of the keys. Gestures are listed
on the page.

## Architecture

- Android: Kotlin `InputMethodService`. Keys on one custom canvas view
  (a tap must draw in the same frame); panels (kaomoji, clipboard,
  settings, cards) in Compose. No Tauri or web view.
- `core/` Rust crate through UniFFI (same setup as ccez-tube's core:
  cargo-ndk, uniffi-bindgen from the .so, JNA): autocorrect (SymSpell-style
  edits weighted by key distance, word frequencies per language, the
  owner's own words), next-word, language detection (ccez-llm's
  `langid.rs` approach, ML Kit language-id as a second opinion), kaomoji
  ranking by app and message, settings store and schema, logs.
- Shared crate, extracted from ccez-tube `core/` (words.rs, dict.rs,
  vocab.rs, ask.rs, sense.rs + ccez-lexicon): dictionaries, Japanese and
  Chinese segmentation, furigana/pinyin/pitch, word statuses and
  ccez-words sync, the OpenAI-compatible client (Muse
  `muse-spark-1.3-contributor`, DeepSeek, Zen, custom). Own error type,
  UniFFI behind a feature. ccez-tube moves onto it too. Romaja and word
  parts get ported from Kotlin into it.
- Kotlin side for Android services: ML Kit Translate (live strip, offline,
  one translator at a time: two starting at once crash, see ccez-tube),
  TTS, SpeechRecognizer (live dictation), ccez-whisper over AIDL
  (`uk.ccez.whisper.HEAR`, batch, about half real time: the clean pass
  after you stop), Gemini Nano through ML Kit GenAI (ccez-llm has it) as
  the private on-device AI.
- Clipboard: ccez-clip's `core/` through UniFFI (its PLAN already lists
  Android), history on the phone only (owner: no sync between machines).
- Agents: `keys` CLI on the Mac, like `clip`, with `--json`: settings,
  languages, buttons, words, logs, status. Reaches the phone over adb
  (USB or tailnet) through a provider only the shell user can call.
- Privacy: no strip, AI, learning or clipboard in password fields or
  apps that ask for no personalised learning. Typed text never in logs.
  Since 2026-11-12 (a review found the phone short of this): the strip is
  off in no-learning fields too, not only passwords; nothing typed in an
  address, number, phone, date or no-suggestions line goes to the AI
  (the core's `ai_allowed`); text that looks like a key or a password
  gets no strip, no translation and no AI, as on the Mac (`looks_secret`);
  Ask opens only from a field the AI may hear. The clipboard chip still
  shows in no-learning fields (it reads, it doesn't record).

## Stage 1 status (2026-11-08)

Done, checked on the S24: QWERTY with accents (hold a key) for en/fr/de/es,
autocorrect and suggestions (accents back: deja → déjà; j'ai; German
capitals and ß), backspace undoes a correction and trusts the word,
next words from own typing, double-space period, capitals, space-bar
swipe between languages, face panel (emoji, kaomoji first in chat apps),
settings panel (buttons, typing, size, languages, licences), clipboard
chip, `keys` CLI, log, haptics (keys, held-key choices, language switch,
undo) and short fades (bar, panels, held-key choices). Tests: core, 13.

Owner's first try (2026-11-08), fixed: hold delete takes whole words after
about a second; haptics through the vibrator (performHapticFeedback is
skipped when the phone's touch feedback is off); smaller shift/delete
icons and narrower faces (letters stay put); settings: whole cells tap,
switches redraw (they didn't, so taps re-sent the same value), Languages
as ccez-tube's numbered flag grid (tap: First/Earlier/Later/Remove; + to
add, names only in the Add list, 🔊 where the phone has an offline voice),
"Default language" as ccez-tube calls it, Licences as its own page. The
owner's ccez-tube order is the default learning order.

Not yet: the clipboard chip is untested on the phone; hold space for the
language list; self-update (like ccez-tube's); swipe typing (setting, off
by default); number row; searching the Add list (needs typing into the
keyboard's own field, which Ask Muse needs too).

## Stage 2 status (2026-11-08)

Checked on the S24: the strip shows the sentence in the first learning
language (ML Kit on the phone), furigana/pinyin from ccez-lang-core
(dictionaries download on first use from its public "dictionaries"
release, checked against SHA256SUMS), known words grey, learning words
underlined, tap a word for its card (Learning / Known), hold a word for
Learning, tap the flag for the next language, swipe the strip, hold the
flag to put the translation in, 🔊 reads it. Glosses under the bold
suggestion in two learning languages, skipping ones where it's known;
hold the suggestion to mark them Learning. Statuses sync with ccez-words
(`keys set words.server …`; 60 came down on first sync).

Known gaps: ML Kit's wording is literal ("free" → 無料); Muse should refine
the strip once typing pauses. ccez-tube's implicit "known" (heard in two
videos) stays in ccez-tube, so few words are grey yet.

## Built since (2026-11-08, checked on the S24)

- Muse: corrections while typing a learning language (ccez-llm's diff, Fix,
  kept in `keys mistakes` for ccez-learn), Ask typed on the keyboard
  itself (carry on in ccez-llm), the strip reworded by the AI on a pause,
  casual or polite per app (hold 🔊). Key: Settings → AI → Paste, or
  `keys key < file`. Needs the owner's key to be tried for real.
- Noticing the language typed (two words), language per app.
- Kaomoji offered in chat apps by the message's mood; clipboard history.
- Dictation: tap (Android's recogniser; asks for a missing language pack),
  hold (ccez-whisper; silence never sent).
- Camera text (OCR, latest screenshot), Into message.
- Layouts: 16 Latin languages, Cyrillic (ru uk bg), Greek, Arabic,
  Persian, Hebrew, Armenian, Hindi, Thai, Korean (syllables), Japanese
  (romaji → kana → words), Chinese (pinyin → characters); draw a Japanese
  or Chinese character (hold ?123); swipe typing (off by default).
- Updates: `scripts/release.sh` → GitHub release → the keyboard installs it.

Not yet / ideas: searching the Add list; Arabic digits.
Declined (owner, 2026-11-08): weekly stats. Telex and Thai/Armenian lists: done.
Donate: only in a Play Store build (CCEZ_KEYS_STORE=1, CCEZ_KEYS_DONATE_URL).

## Order

1. Daily-usable keyboard: QWERTY, autocorrect + suggestions (en, fr),
   space-bar switching, button row, settings, `keys`, logs, release +
   update path like ccez-tube.
2. Learning strip: translation, readings, TTS, cards, ccez-words
   statuses, glosses under suggestions, hold to save.
3. Emoji, app-aware kaomoji, clipboard.
4. Dictation, Muse tips, automatic language switching.
5. More layouts (AZERTY, QWERTZ, Arabic, Hangul, Hindi; Japanese and
   Chinese input if the owner types them).
6. Later: send selected text to ccez-llm to annotate. Done 2026-11-09: a
   word to review in the strip when the keyboard opens on an empty field
   (learn/Review.kt, shared with the widget). Dropped (owner, 2026-11-09):
   ccez-tube's words of the week (Learning words are already tinted).

## Owner decisions (2026-11-08)

- Layout v2 approved in direction ("nice job"); the word card in the
  mockup is preferred over ccez-tube's current one.
- Swipe typing: a setting, off by default.
- Types in every language they learn. Today's SwiftKey (6-language limit):
  de, en, es, fr-CA on QWERTY; ja on Japanese QWERTY (romaji to
  kana/kanji); zh-Hans on full-key pinyin. Arabic is learned too.
- Muse reads what's typed everywhere except password fields.
- Ask Muse: start a chat about something just typed.
- Kaomoji first in chat apps: Messages, TextNow, Discord, Telegram,
  WeChat, WhatsApp, Messenger, Instagram, Facebook, Threads (a list
  editable with `keys`).
- Clipboard: the newest copy waits as a chip until pasted or replaced,
  no timer (SwiftKey's chip vanished when it was needed).
- Repo public.

- Corrections work like ccez-llm's correction mode (`src/lib/correction.ts`):
  a word diff of the sentence, dropped words crossed out, added ones green.
  Reuse its prompt.
- Language per app: yes (LINE opens in Japanese, WeChat in Chinese).
- Draw a character: Japanese and Chinese only, tucked away, no button.
- Typing without accents: yes (deja → déjà, uber → über, pinyin tone numbers).
- No-AI / no-learning is a setting (per app), not a one-tap toggle.
- Drills don't belong in ccez-llm: a separate app later (ccez-learn).
  The keyboard keeps a mistake log (accepted corrections) for it.
- The shared language crate can be public. ccez-tube's card stays as is.

- Formality: the strip's translation is casual or polite (tu/vous,
  du/Sie, だ/です); each app remembers its own, chat apps start casual,
  hold the flag to flip.
- Camera text (OCR) button: yes, on by default, removable. Reads the
  camera or the latest screenshot; words coloured by status. Owner uses
  Circle to Search today; this ties into their word lists.
- Kaomoji: the face key swaps the keys for a kaomoji panel of the same
  height (owner: don't crowd the message); ABC in the same spot swaps back.
- The owner may sell it (2026-11-08). See Selling.

## Selling (owner may monetize)

- Licence: MIT was picked when the repo went public; MIT lets anyone
  resell it. Owner to decide before outside contributions arrive.
- Owner-only parts must be optional: ccez-words on the tailnet, the
  update mirror, the owner's Muse key. A buyer needs word lists stored
  on the phone (and maybe a sync service), their own AI key or a paid
  proxy.
- Data licences allow selling with attribution: JMdict, CC-CEDICT,
  Wiktionary (kaikki), wordfreq, Kanjium are CC-BY-SA; ML Kit is free.
- Play Store: a keyboard that sends typed text to a server needs a clear
  privacy policy and an in-app disclosure; Android itself warns about
  keyboards collecting what you type.
- Anki: send saved words to AnkiDroid (its content-provider API, with
  the sentence you typed, reading, meaning, audio; ccez-lang's
  `src/core/anki.ts` has the card format), and read Anki's mature cards
  back as Known so the strip skips them (to verify: what AnkiDroid's API
  lets an app read).

## Owner notes (keeps, 2026-11-08), what came of them

- Star pushed the words left: it has a fixed place now.
- A full stop re-asked Muse: its answers are kept without end marks.
- Autocorrect sometimes left an obvious fix: the benchmark pass (core/tests/bench.rs)
  raised fixes from ~70% to 81-90%; watch for more.
- Clipboard layout: clips two to a row, screenshots on top. "A way to lay out
  words": asked the owner what they meant.
- Editing a word's end (swim → swimm): letters after a word go on with it.
- The four-square button was jarring: swipe the bar for the buttons.
- A long strip line couldn't be moved: dragging it sideways scrolls it
  (up and down change language, since 2026-11-12).
- Screenshots weren't in the clipboard: the panel shows the day's screenshots.
- Hungarian on an empty line: a new line is empty; a new sentence starts at
  the first language.

## Still open


## Future-proofing

- One settings schema in Rust: the settings screen, `keys` and Muse's
  tools are generated from it, so Muse in Ask can change settings too.
- Layouts, kaomoji packs and the chat-app list are data files: a new
  language is a new file, not code.
- Language packs (dictionaries, ML Kit models, word lists) download on
  first use; the APK stays small.
- AI provider per feature (Muse, DeepSeek, Gemini Nano on the phone).
- Core has no Android code, so a Mac input method could reuse it.
- Replay tests: typed strings per language through autocorrect and
  conversion, so a change can't quietly make typing worse.
- Settings and the personal dictionary back up to the Mac mini, so a new
  phone is set up in one step (done 2026-11-09: `keys backup` / `keys
  restore`, daily on the Mac mini).
