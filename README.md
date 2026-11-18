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
    keys key add <name> | ls | rm <name> | test [<name>]   # API keys, once, everywhere (below)

`keys clip` sets and clears the phone's clipboard through the keyboard with
the keyboard closed (only adb can call it). While the phone is locked
Android takes the write but hides the clipboard from everyone, so `now`
says so and `put` can't read itself back. `ccez-clip`'s `clip --on phone`
uses it.

## Keys, once (owner, 2026-11-16)

ccez-keys keeps the owner's API keys by name ("Keys should store more than
just deepseek key"). Copy a key, then on z run `keys key add <name>`:

    keys key add deepseek   # z's keychain (ccez-words serves it to the Mac and tailnet
                            # apps), the phone, and DEEPSEEK_API_KEY on ccez-keeps,
                            # art-website and ccez-llm's web build
    keys key add gemini     # z's keychain and art-website's GEMINI_API_KEY (the
                            # writing helper)
    keys key add github     # z's keychain and the phone: the token the phone apps
                            # update themselves with (below)
    keys key add <other>    # z's keychain and the phone, which lends it by name
                            # (docs/keyring.md; Settings → AI lists each key)

It checks the key looks right for its name (DeepSeek's `sk-…`, Gemini's
`AQ.…` or `AIza…`, GitHub's fine-grained `github_pat_…`), stores it in z's login keychain (service `uk.ccez.ai`,
account the name), puts it everywhere that name is used (the table in
`cli/src/keystore.rs`) and redeploys the Pages sites that use it (an empty
commit on main). No key on the clipboard: it asks with a hidden prompt. It
never prints the key or puts it on a command line.

- `keys key ls`: each name, where it's placed, when it was set. Never values.
- `keys key rm <name>`: off every place, after a y/N.
- `keys key test [<name>]`: one tiny real call per key (DeepSeek through
  ccez-words with z's balance, and from the phone; Gemini 4 Hydrogen from z,
  where a 429 means Google's free limit is used up; GitHub's reads each
  app's latest release), and the Cloudflare secrets checked.
- `keys ai-key [--test]` still works: it's `keys key add|test deepseek`.

### The apps' updates (owner, 2026-11-18)

The owner's phone apps whose repos are private (ccez-tube, ccez-calc,
ccez-gallery, ccez-home, ccez-recorder, ccez-translate) update themselves
from their GitHub releases with one token, kept here as `github` and lent
to each app by the keyring, instead of a copy in each ("I don't have to
store github tokens redundantly"). Make it once on GitHub: a fine-grained
token, Repository access those six repos (or All repositories), Repository
permissions → Contents: Read-only, nothing else. Copy it, then on z:

    keys key add github     # then `keys key test github` reads each app's release

Each app asks for it when it checks for an update and keeps it in memory
only; the copy an older build kept is removed once ccez-keys answers. An
app not yet allowed shows ccez-keys' Allow screen once when opened.
`keys keyring allow <package>` allows one from z instead. A public repo
(ccez-keys, ccez-launcher, ccez-clock, ccez-keeps, ccez-whisper) needs no
token.

The phone: the one adb sees; the same phone over USB and wireless
debugging is picked by itself, USB first. `scripts/key-test.sh` runs it
all against stand-ins with fake keys.
