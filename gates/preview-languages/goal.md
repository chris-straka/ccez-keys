# Goal: choose which languages the strip previews

Repo: ccez-keys, an Android keyboard (Kotlin and Compose in `android/`, a
Rust core in `core/` reached through UniFFI). The gate,
`gates/preview-languages/gate.sh`, judges the work.

## The owner's ask (2026-11-26)

"the previews that I get in my keyboard, I should have a setting option to
limit the number of languages for that so I don't have all my languages
there."

The previews are the learning strip: while typing, the sentence shows in
the owner's learning languages, one leading and the next ones after it as
they fit ("chat | Katze | gato"). Today every learning language
(`languages.learning`, up to 24) can show there. The owner wants to pick
which. Typing languages, suggestions, the learning order and everything
else stay as they are.

## How the strip picks its languages today

`Strip.targets(typing)` in
`android/app/src/main/kotlin/uk/ccez/keys/learn/Strip.kt`: typing a
learning language that isn't the default language, the default
(`languages.default`); otherwise every learning language but the one typed,
in the owner's order, that ML Kit's translator supports. Everything the
strip shows (the lead line, `addMore`, `prefetch`, `refine`, `step`) goes
through `targets`.

## What to build (follow this design)

1. **The setting** (`core/src/settings.rs`): a new `Def` right after
   `learning.strip`: key `learning.strip_hidden`, `Kind::Langs`, default
   `[]`, about: "Learning languages the strip's previews leave out (empty:
   all of them)." It is the list of languages switched off; its default
   (none) keeps today's previews, and a language added later shows until
   it is switched off. `keys get/set/schema` and backups pick it up through
   the table, as every setting does.
2. **The rule, in the core** (`core/src/lib.rs`, in the
   `#[uniffi::export] impl Engine` block, next to `setting_strings`):
   `pub fn strip_languages(&self, typing: String) -> Vec<String>`. Typing
   a learning language that isn't the default: `vec![default]`, whatever is
   hidden. Otherwise: `languages.learning` in its order, without `typing`
   and without any language in `learning.strip_hidden`. It reads the
   settings once under the lock. The tests in
   `core/tests/preview_languages.rs` are its contract.
3. **The strip** (`Strip.kt`): `targets(typing)` becomes
   `engine.stripLanguages(typing).filter { Translator.supports(it) }` and
   nothing else changes in that file. `Strip.kt` no longer reads
   `languages.learning` itself (not even in a comment; the gate looks).
4. **The settings panel** (`android/app/src/main/kotlin/uk/ccez/keys/ui/KeyboardUi.kt`):
   one control, a checklist. In `SettingsMain`, right after the Languages
   `FlagGrid(...)` and before `Heading("Default language")`, add
   `Heading("Preview")` and a new `@Composable private fun PreviewFlags(version: Int)`:
   a `FlowRow` (6.dp apart both ways) of the learning languages' flags in
   the owner's order (`learning()`), one chip each: 44.dp wide, 36.dp tall,
   `RoundedCornerShape(10.dp)`, the flag (`Langs.of(ctx, code)?.flag ?: code`)
   at 20.sp, centred. A shown language: background `Colors.key`, flag at
   full colour. A hidden one: no background, a 1.dp `Colors.line` border,
   the flag at 30% alpha. A tap (`combinedClickableCompat`, which buzzes)
   toggles that code in `learning.strip_hidden` with
   `set("learning.strip_hidden", JSONArray(newList).toString())`, keeping
   the other hidden codes. The chips read the setting with
   `remember(version) { App.instance.engine.settingStrings("learning.strip_hidden") }`
   so they redraw after each change, as `TwoColumns` does. No helper
   text, no counts, no other labels (the repo's wording rule: pithy, no
   helper text).
5. **Removing a language** in `FlagActions` also takes it out of
   `learning.strip_hidden`, so a language removed and added back later
   shows again.
6. **The layout page**: `docs/layout.html` already shows the Preview row
   in the settings screen; nothing to do there.

## What must hold (the gate checks each)

- The ten tests in `core/tests/preview_languages.rs` pass by name: the
  default previews exactly what today's rule gives (all 24 default
  languages, the owner's order, and other lists and default languages);
  hidden languages leave and the order stays; typing a learning language
  still previews in the default language; hiding all leaves none; typing
  languages and the learning list don't change; the setting survives a
  restart (a new `Engine` on the same folder) and is saved in
  `settings.json` under its key; an older settings file without it
  previews every language; it is in `keys schema`, kind `languages`,
  default `[]`, and refuses anything but language codes.
- Every core test passes: `cargo test --release -p ccez-keys-core`.
- The Android app builds and its unit tests pass:
  `./gradlew :app:testDebugUnitTest` in `android/`.
- The core's public API is unchanged: every `pub fn` line of `core/src`
  at main is still there, word for word (`gates/preview-languages/api-base.txt`),
  and every setting key (`keys-base.txt`). Another app (the owner's
  launcher) builds this core read-only, so add, never change or remove.
- `Strip.kt` calls `stripLanguages(` and doesn't mention
  `languages.learning`; `KeyboardUi.kt` has `Heading("Preview")` and writes
  `"learning.strip_hidden"`.

## Out of scope; don't

- Don't edit the gate (`gates/preview-languages/`), its recorded files, or
  `core/tests/preview_languages.rs`. They are protected; a change to them
  fails the run.
- Don't change typing languages, the language detection (`written`), how
  the lead language is chosen (`SEARCH`, the known-words rule), the Mac
  app (`desktop/`), the CLI (`cli/`), suggestions, or any other setting.
- Don't reformat files (`cargo fmt` is not used in this repo), and don't
  touch lines you don't need to.
- No new dependencies. Never log typed text.
- Commit with a message naming the change, e.g. "Settings → Preview: pick
  which learning languages the strip previews (learning.strip_hidden)".

## Running the checks

Run `check` (the gate through fleet) when you think it passes; read its
output, fix, and run it again. The gate prints PASS or FAIL per check and
ends with GATE PASS or GATE FAIL.

## Log

- 2026-11-26: the gate runs on a Mac only (`--fleet-arg=--mac`, baseline
  on mac). On main the Android build passes on the Mac (18 unit tests) and
  fails on the Linux worker, so a Linux run can't tell the work from the
  machine. SWE builds and checks run on the Mac mini anyway.
- 2026-11-26: typing.rs's `fast_enough_for_each_key` (a 4 ms clock) missed
  its time on the busy Mac in the first attempt's check while every other
  check passed. The gate now runs it again alone, up to three times, when
  it is the suite's only failure. Don't change that test.
