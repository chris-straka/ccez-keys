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
    keys clip put <text|-> | now | ls | get <id> | rm <id> | clear   # the phone's clipboard
    keys ai-key [--test]              # the DeepSeek key, once, everywhere (below)

`keys clip` sets and clears the phone's clipboard through the keyboard with
the keyboard closed (only adb can call it). While the phone is locked
Android takes the write but hides the clipboard from everyone, so `now`
says so and `put` can't read itself back. `ccez-clip`'s `clip --on phone`
uses it.

## The AI key, once (owner, 2026-11-16)

Copy the DeepSeek key, then on z run `keys ai-key`. It checks the key looks
right (`sk-…`), then puts it in z's keychain (ccez-words serves it to the
Mac and tailnet apps), on the phone (ccez-keys lends it to the phone apps
through its keyring, `docs/keyring.md`), and in the Cloudflare apps' secret
`DEEPSEEK_API_KEY` (ccez-keeps, art-website, ccez-llm's web build; the two
Pages sites redeploy to pick it up). No key on the clipboard: it asks with
a hidden prompt. It never prints the key or puts it on a command line.
`keys ai-key --test` makes one tiny real call through z and the phone
(with z's balance; 402 means DeepSeek's prepaid balance is used up) and
checks the Cloudflare secrets. `scripts/ai-key-test.sh` runs it all
against stand-ins with a fake key.
