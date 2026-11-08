# ccez-keys

An Android keyboard for learning languages while you type. As you write,
a strip above the suggestions shows your sentence in a language you're
learning (furigana, pinyin, sound), skips words you already know, and
lets you mark words Learning or Known in the same list ccez-tube,
ccez-lang and ccez-llm use (ccez-words).

Status: stage 1 of `PLAN.md` (a keyboard for every day) runs on the owner's
phone; the learning strip comes next. Layout: `docs/layout.html`.

- `core/`: Rust (autocorrect, suggestions, layouts, settings, log), to
  Kotlin through UniFFI. `cargo test -p ccez-keys-core --release`.
- `android/`: the keyboard (Kotlin; keys on one canvas, panels in Compose).
- `data/`: word lists (`keys-words` builds them), layouts, faces.
- `cli/`: `keys` (the keyboard from the Mac, over adb) and `keys-words`.

    scripts/build.sh                  # release APK, installed on the phone
    cargo run -p ccez-keys-cli --bin keys -- status | get [key] | set <key> <value> | schema | log
