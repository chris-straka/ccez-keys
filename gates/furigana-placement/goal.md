# Goal: furigana over each kanji, not one line over the word

Repo: ccez-keys, an Android keyboard (Kotlin and Compose in `android/`, a
Rust core in `core/` reached through UniFFI). The gate,
`gates/furigana-placement/gate.sh`, judges the work.

## The owner's ask (2026-11-26)

"Furigana positioning is bad too." On an empty field the strip shows a
word to review: flag, the word, its meaning, three grade buttons. For
思い浮かべる it shows the whole reading おもいうかべる as one small line,
centred over the whole word, so the reading doesn't sit over the kanji it
belongs to.

Wanted: proper ruby. Each run of kanji gets its own part of the reading,
centred over it (おも over 思, う over 浮); kana in the word (い, かべる)
get nothing over them. A reading wider than its kanji may spread past
them, but never over the next reading.

## Where it is today

- `ReviewRow` in `android/app/src/main/kotlin/uk/ccez/keys/ui/StripUi.kt`
  draws a `Column` with `Text(f.reading, …)` (9.5.sp, `Colors.muted`) over
  `Text(f.word, …)` (17.sp, SemiBold, `Colors.fg`), and the Column is
  aligned with the meaning by `Modifier.alignBy(LastBaseline)`.
- `Review.face()` in `android/app/src/main/kotlin/uk/ccez/keys/learn/Review.kt`
  fills `Face.reading` from the dictionary card (`card?.reading`): the
  whole reading, in hiragana for Japanese, pinyin for Chinese.

## What to build (follow this design)

1. **The split, in the core**: a new file `core/src/ruby.rs`
   (`pub mod ruby;` in `core/src/lib.rs`, and `pub use ruby::ruby_split;`
   beside the other `pub use` lines) with
   `#[uniffi::export] pub fn ruby_split(word: String, reading: String) -> Vec<Ruby>`
   (`Ruby` is the existing record in `core/src/learn.rs`: `text`, `rt`;
   don't change it). Kotlin calls it as `uk.ccez.keys.core.rubySplit`.
   - The reading is trimmed. Empty, or the word has no kanji: one part,
     `Ruby { text: word, rt: "" }` (nothing over it).
   - Kanji: CJK ideographs (U+4E00–U+9FFF, U+3400–U+4DBF, U+F900–U+FAFF)
     and 々 〆 ヶ ヵ. Kana: hiragana U+3041–U+3096, katakana U+30A1–U+30FA
     but ヵ ヶ, ー, ゝ ゞ ヽ ヾ. Anything else in a word that has kanji (a digit, a
     letter): the fallback below.
   - Cut the word into runs: each run of kanji, each run of kana. Match
     the reading against them in order: a kana run must match the same
     letters in the reading (katakana and hiragana compare equal: fold
     katakana U+30A1–U+30F4 down by 0x60 on both sides); a kanji run
     takes one or more letters of the reading. Find every way the whole
     reading matches (backtracking; words are short).
   - Exactly one way: one `Ruby` per run, in order. A kanji run's `rt` is
     its part of the reading as written in the reading (a katakana reading
     stays katakana); a kana run's `rt` is "".
   - No way, or more than one (物の怪 / もののけ: もの|の|け or
     も|の|のけ): the fallback, one part, `Ruby { text: word, rt: reading }`,
     the whole reading over the whole word. A word that is one kanji run
     (今日 / きょう, jukujikun) comes out the same: one part.
   - The parts' texts always join to the word.
   The tests in `core/tests/ruby.rs` are its contract.
2. **The placement, in Kotlin**: a new file
   `android/app/src/main/kotlin/uk/ccez/keys/ui/Ruby.kt` with
   `internal fun rubyStarts(baseWidths: List<Float>, rubyWidths: List<Float>, gap: Float): List<Float>`:
   where each reading starts, in px, with the bases laid side by side
   from x = 0 (base i starts at the sum of the widths before it).
   - `require` the two lists to be the same size.
   - A reading (width > 0) is centred over its own base:
     `baseStart + (baseWidth - rubyWidth) / 2`. Going left to right, one
     that would start before the end of the reading before it plus `gap`
     starts there instead (pushed right). The first may start below 0
     (spreading left of the word).
   - A part with no reading (width 0) gets the centre of its base, and
     neither pushes nor is pushed.
   The tests in `android/app/src/test/kotlin/uk/ccez/keys/RubyTest.kt` are
   its contract.
3. **The word with its ruby**, in the same `Ruby.kt`:
   `@Composable fun RubyWord(parts: List<Ruby>, color: Color, fontSize: TextUnit, fontWeight: FontWeight?, rubyColor: Color, rubySize: TextUnit, modifier: Modifier = Modifier)`.
   A `Layout` whose children are one base `Text` per part (the word's
   style, `maxLines = 1`) and one reading `Text` per part with a reading
   (`rubySize`, `lineHeight` 11.sp, `rubyColor`, `maxLines = 1`). Measure
   them all; place the bases side by side under a reading band as tall as
   the tallest reading (no band when no part has a reading); place each
   reading at `rubyStarts(baseWidths, rubyWidths, 1.dp in px)` plus a
   shift that moves everything right by `-min(0, first start)`, so nothing
   draws outside the layout. Width: that shift plus the larger of the
   bases' total and the last reading's end. Report `FirstBaseline` and
   `LastBaseline` as the band's height plus the bases' baseline, so
   `Modifier.alignBy(LastBaseline)` lines it up with the meaning as now.
4. **The review row**: `Review.Face` gains `val ruby: List<Ruby>` (keep
   `reading`: the card uses it). In `Review.face()`: for Japanese
   (`lang == "ja"`) `rubySplit(word, reading)`; for any other language
   `listOf(Ruby(word, reading))` (pinyin stays over the whole word, as
   today). In `ReviewRow`, the `Column` with `Text(f.reading, …)` over
   `Text(f.word, …)` becomes
   `RubyWord(f.ruby, Colors.fg, 17.sp, FontWeight.SemiBold, Colors.muted, 9.5.sp, Modifier.alignBy(LastBaseline))`.
   Nothing else in the row changes.

## What must hold (the gate checks each)

- The eleven tests in `core/tests/ruby.rs` pass by name: okurigana in the
  middle and at the end (思い浮かべる, 取り扱い, 申し込む, 食べる, 新しい,
  勉強する …), kana before the kanji (お茶), one kanji run keeping the
  whole reading (今日, 明日, 大人, 人々, 一ヶ月), the ambiguous 物の怪 and
  readings that don't fit falling back to the whole word, words without
  kanji and words without a reading getting nothing, katakana matching
  hiragana, and every split spelling its reading.
- The seven tests in `RubyTest.kt` pass by name.
- Every core test passes (`cargo test --release -p ccez-keys-core`), and
  the Android app builds with its unit tests passing
  (`./gradlew :app:testDebugUnitTest` in `android/`).
- The core's public API is unchanged: every `pub fn` line of `core/src`
  at the base is still there, word for word
  (`gates/furigana-placement/api-base.txt`), and the `Ruby` record keeps
  its fields. Another app (the owner's launcher) builds this core
  read-only, so add, never change or remove.
- `Review.kt` calls `rubySplit(`; `Ruby.kt` defines `rubyStarts` and
  `RubyWord` calls it; `StripUi.kt` uses `RubyWord(` and no longer has
  `Text(f.reading`.

## Out of scope; don't

- Don't edit the gate (`gates/furigana-placement/`), its recorded file,
  `core/tests/ruby.rs` or `RubyTest.kt`. They are protected; a change to
  them fails the run.
- Don't change the strip's own words (`Word`, `StripLine`: readings stay
  off there), the word card, the Mac app (`desktop/`), the CLI, the
  dictionaries or ccez-lang-core, or how readings are looked up.
- Don't reformat files (`cargo fmt` is not used in this repo), and don't
  touch lines you don't need to. No new dependencies.
- Commit with a message naming the change, e.g. "Review word: furigana
  over each kanji run (ruby_split, rubyStarts), whole reading when the
  split is unsure".

## Running the checks

Run `check` (the gate through fleet) when you think it passes; read its
output, fix, and run it again. The gate prints PASS or FAIL per check and
ends with GATE PASS or GATE FAIL.

## Log

- 2026-11-26: the gate runs on a Mac only (`--fleet-arg=--mac`, baseline
  on mac): the Android build passes on the Mac at main and fails on the
  Linux worker (gates/preview-languages/goal.md). typing.rs's
  `fast_enough_for_each_key` (a 4 ms clock) runs again alone when it is
  the core suite's only failure; don't change that test.
- 2026-11-26: started beside preview-languages rather than after it (the
  fleet's Mac queue ran hours long); the two touch different code apart
  from separate hunks of `core/src/lib.rs`, and both gates run again on
  main once both are merged.
