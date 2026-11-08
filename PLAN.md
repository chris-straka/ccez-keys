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
6. Later: ccez-tube's words of the week first in the strip; a review card
   when the keyboard opens; send selected text to ccez-llm to annotate.

## Open questions for the owner

- The layout (docs/layout.html).
- Swipe (glide) typing from the start? The phone runs SwiftKey today.
- Which languages are typed, not only learned? (decides Japanese/Chinese input)
- Muse tips read what's typed: everywhere except passwords, or only in
  chat apps?
