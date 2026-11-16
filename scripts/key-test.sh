#!/bin/sh
# keys key end to end with stand-ins for every tool it drives (no real
# clipboard, keychain, phone, Cloudflare, git push or API): fake keys go
# in, and the check is that each reaches its places on stdin only, never
# on a command line or in what the command prints.
#   sh scripts/key-test.sh   (from the repo; builds keys first)
set -eu
cd "$(dirname "$0")/.."
cargo build -q -p ccez-keys-cli --bin keys
KEYS="$PWD/target/debug/keys"
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
FAKE=sk-test0fake0key0000000000000000000
FAKEG=AQ.Ab8test0fake0gemini0key000000000000000
b64() { printf %s "$1" | base64; }
: > "$T/kc"
: > "$T/phone_keys"

mk() { printf '#!/bin/sh\n%s\n' "$2" > "$T/$1"; chmod +x "$T/$1"; }
# Each stand-in logs its arguments to args.log and its stdin to stdin.log.
LOG='echo "$0 $*" >> '"$T"'/args.log'
mk pbpaste "printf '%s\n' '$FAKE'"
# z's keychain: the names in kc, one a line.
mk security "$LOG
case \"\$*\" in
  -i) l=\$(cat); printf '%s\n' \"\$l\" >> $T/stdin.log; n=\$(printf %s \"\$l\" | sed -n 's/.* -a \([a-z0-9-]*\) .*/\1/p'); grep -qx \"\$n\" $T/kc || echo \"\$n\" >> $T/kc;;
  dump-keychain) while read -r n; do printf 'keychain: \"/x/login.keychain-db\"\nclass: \"genp\"\nattributes:\n    \"acct\"<blob>=\"%s\"\n    \"mdat\"<timedate>=0x00  \"20261116154411Z\\\\000\"\n    \"svce\"<blob>=\"uk.ccez.ai\"\n' \"\$n\"; done < $T/kc
    printf 'keychain: \"/x/login.keychain-db\"\nclass: \"genp\"\nattributes:\n    \"acct\"<blob>=\"someone\"\n    \"svce\"<blob>=\"other.service\"\n';;
  *find-generic-password*gemini*) echo '$FAKEG';;
  *delete-generic-password*) n=\$(echo \"\$*\" | sed 's/.* -a //'); grep -vx \"\$n\" $T/kc > $T/kc2 || true; mv $T/kc2 $T/kc;;
esac"
mk launchctl "$LOG"
mk wrangler "$LOG
case \"\$*\" in
  *'secret put'*) cat >> $T/stdin.log; echo >> $T/stdin.log; echo 'Success! Uploaded secret';;
  *'secret delete'*) echo 'Success! Deleted secret';;
  *'pages secret list'*) echo '  - DEEPSEEK_API_KEY: Value Encrypted'; echo '  - GEMINI_API_KEY: Value Encrypted';;
  *'secret list'*) echo '[{\"name\":\"DEEPSEEK_API_KEY\",\"type\":\"secret_text\"}]';;
esac"
mk git "$LOG
case \"\$*\" in *commit-tree*) echo 0123456789abcdef0123456789abcdef01234567;; esac"
mk curl "$LOG; cat >> $T/curl-stdin.log
case \"\$*\" in
  *chat/completions*) printf '{\"choices\":[{\"message\":{\"content\":\"OK\"}}]}\n200';;
  *user/balance*) printf '{\"is_available\":true,\"balance_infos\":[{\"currency\":\"USD\",\"total_balance\":\"4.20\"}]}\n200';;
  *gemini-4-hydrogen:generateContent*) printf '{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"OK\"}]}}]}\n200';;
esac"
# The phone, seen twice (USB and wireless debugging); its ccez-keys holds
# named keys (phone_keys) unless OLD_PHONE is set.
mk adb "$LOG
case \"\$*\" in
  devices) printf 'List of devices attached\n100.64.0.9:40465\tdevice\nUSBPHONE\tdevice\n';;
  *'getprop ro.serialno'*) echo SAMEPHONE;;
  *'method status'*) ks=\$(sed 's/.*/\"&\"/' $T/phone_keys | paste -sd, -)
    if [ -n \"\${OLD_PHONE:-}\" ]; then j='{\"ai_key\":true}'; else j=\"{\\\"keys\\\":[\$ks]}\"; fi
    echo \"Result: Bundle[{out=\$(printf %s \"\$j\" | base64)}]\";;
  *'method key'*) n=\$(echo \"\$*\" | sed -n \"s/.*--arg '\\([a-z0-9-]*\\)'.*/\\1/p\"); v=\$(cat); printf '%s\n' \"\$v\" >> $T/stdin.log
    if [ \"\$v\" = \"\$(printf ' ' | base64)\" ]; then grep -vx \"\$n\" $T/phone_keys > $T/pk2 || true; mv $T/pk2 $T/phone_keys; echo \"Result: Bundle[{out=\$(printf removed | base64)}]\"
    else grep -qx \"\$n\" $T/phone_keys || echo \"\$n\" >> $T/phone_keys; echo \"Result: Bundle[{out=\$(printf saved | base64)}]\"; fi;;
  *'method ai-test'*) echo 'Result: Bundle[{out=$(b64 OK)}]';;
esac"

export KEYS_PBPASTE="$T/pbpaste" KEYS_SECURITY="$T/security" KEYS_LAUNCHCTL="$T/launchctl" KEYS_WRANGLER="$T/wrangler" \
  KEYS_GIT="$T/git" KEYS_CURL="$T/curl" KEYS_ADB="$T/adb" KEYS_SWE="$T/swe" \
  KEYS_HUB="http://127.0.0.1:9/ai/deepseek" KEYS_GEMINI="http://127.0.0.1:9/v1beta"
unset KEYS_SERIAL || true

fail() { echo "FAIL: $*"; exit 1; }
leaked() { grep -q -e "$FAKE" -e "$FAKEG" "$1"; }

# DeepSeek, by its old name: z, the phone and the three Cloudflare apps.
"$KEYS" ai-key > "$T/out.txt" 2>&1 || { cat "$T/out.txt"; fail "keys ai-key exited non-zero"; }
cat "$T/out.txt"
leaked "$T/out.txt" && fail "the key was printed"
leaked "$T/args.log" && fail "the key was on a command line"
grep -q "add-generic-password -U -s uk.ccez.ai -a deepseek .* -w $FAKE" "$T/stdin.log" || fail "keychain didn't get it on stdin"
[ "$(grep -c "^$FAKE\$" "$T/stdin.log")" = 3 ] || fail "the three Cloudflare secrets didn't each get it on stdin"
grep -q "^$(b64 "$FAKE")\$" "$T/stdin.log" || fail "the phone didn't get it on stdin"
grep -q "adb -s USBPHONE shell read -r k; content call .* --method key --arg 'deepseek'" "$T/args.log" || fail "the phone over USB, by name"
grep -q "secret put DEEPSEEK_API_KEY --name ccez-keeps" "$T/args.log" || fail "ccez-keeps"
grep -q "pages secret put DEEPSEEK_API_KEY --project-name art-website" "$T/args.log" || fail "art-website"
grep -q "pages secret put DEEPSEEK_API_KEY --project-name personalized-llm-client" "$T/args.log" || fail "ccez-llm"
grep -q "swe/ccez-llm push -q origin 0123456789abcdef0123456789abcdef01234567:refs/heads/main" "$T/args.log" || fail "ccez-llm redeploy"
grep -q "kickstart -k gui/.*/uk.ccez.words" "$T/args.log" || fail "ccez-words restart"
grep -q "balance 4.20 USD" "$T/out.txt" || fail "balance"

# Gemini: z and art-website's GEMINI_API_KEY only; its test call gets the
# key through curl's config on stdin.
: > "$T/args.log"
mk pbpaste "printf '%s\n' '$FAKEG'"
"$KEYS" key add gemini > "$T/out.txt" 2>&1 || { cat "$T/out.txt"; fail "keys key add gemini exited non-zero"; }
cat "$T/out.txt"
leaked "$T/out.txt" && fail "the Gemini key was printed"
leaked "$T/args.log" && fail "the Gemini key was on a command line"
grep -q "add-generic-password -U -s uk.ccez.ai -a gemini .* -w $FAKEG" "$T/stdin.log" || fail "keychain didn't get the Gemini key on stdin"
grep -q "^$FAKEG\$" "$T/stdin.log" || fail "art-website's GEMINI_API_KEY didn't get it on stdin"
grep -q "pages secret put GEMINI_API_KEY --project-name art-website" "$T/args.log" || fail "GEMINI_API_KEY"
grep -q "swe/art-website push -q origin" "$T/args.log" || fail "art-website redeploy"
grep -q "method key" "$T/args.log" && fail "the Gemini key went to the phone"
grep -q "ccez-keeps\|personalized-llm-client" "$T/args.log" && fail "the Gemini key went to a DeepSeek place"
grep -q "kickstart" "$T/args.log" && fail "ccez-words restarted for Gemini"
grep -q "header = \"x-goog-api-key: $FAKEG\"" "$T/curl-stdin.log" || fail "the Gemini test didn't send the key in curl's config"
grep -q '"thinkingLevel\\":\\"low\\"' "$T/curl-stdin.log" || fail "the Gemini test isn't low thinking"
grep -q "gemini-4-hydrogen answered “OK”" "$T/out.txt" || fail "the Gemini test"

# A name it doesn't know: z and the phone.
mk pbpaste "printf '%s\n' 'other-fake-key-0000'"
"$KEYS" key add other > "$T/out.txt" 2>&1 || { cat "$T/out.txt"; fail "keys key add other"; }
grep -q "adb -s USBPHONE shell read -r k; content call .* --method key --arg 'other'" "$T/args.log" || fail "other to the phone"
grep -qx other "$T/phone_keys" || fail "other on the phone"
# …but not to a phone whose ccez-keys holds only DeepSeek's (it would overwrite it).
OLD_PHONE=1 "$KEYS" key add other2 > "$T/out.txt" 2>&1 && fail "an old phone took a second key"
grep -q "holds only the DeepSeek key" "$T/out.txt" || fail "old phone wording"
grep -qx other2 "$T/phone_keys" && fail "the old phone was sent other2"

# ls: names, places and times; never a value.
"$KEYS" key ls > "$T/ls.txt" 2>&1 || { cat "$T/ls.txt"; fail "keys key ls"; }
cat "$T/ls.txt"
leaked "$T/ls.txt" && fail "ls printed a key"
grep -q "^gemini: set 2026-11-16 [0-9][0-9]:44" "$T/ls.txt" || fail "ls gemini"
grep -q "^  z, art-website GEMINI_API_KEY" "$T/ls.txt" || fail "ls gemini places"
grep -q "^deepseek: set" "$T/ls.txt" || fail "ls deepseek"
grep -q "^other: set" "$T/ls.txt" || fail "ls other"
grep -q "someone" "$T/ls.txt" && fail "ls showed another service's item"

# rm: nothing without a yes; with one, off every place.
: > "$T/args.log"
echo n | "$KEYS" key rm gemini > "$T/out.txt" 2>&1 && fail "rm went on without a yes"
grep -q "delete" "$T/args.log" && fail "rm deleted without a yes"
echo y | "$KEYS" key rm gemini > "$T/out.txt" 2>&1 || { cat "$T/out.txt"; fail "keys key rm gemini"; }
grep -q "delete-generic-password -s uk.ccez.ai -a gemini" "$T/args.log" || fail "rm: keychain"
grep -q "pages secret delete GEMINI_API_KEY --project-name art-website" "$T/args.log" || fail "rm: art-website"
grep -qx gemini "$T/kc" && fail "rm: gemini still in the keychain"
echo y | "$KEYS" key rm other > /dev/null 2>&1 || fail "keys key rm other"
grep -qx other "$T/phone_keys" && fail "rm: other still on the phone"

# Not a key: nothing is stored.
: > "$T/stdin.log"
mk pbpaste "echo 'some copied text'"
KEYS_NO_PROMPT=1 "$KEYS" key add deepseek < /dev/null > "$T/out2.txt" 2>&1 && fail "accepted a non-key"
[ -s "$T/stdin.log" ] && fail "stored something without a key"
mk pbpaste "printf '%s\n' '$FAKE'"
KEYS_NO_PROMPT=1 "$KEYS" key add gemini < /dev/null > /dev/null 2>&1 && fail "took a DeepSeek key as Gemini's"
echo "not-a-key" | "$KEYS" key add deepseek --stdin > /dev/null 2>&1 && fail "--stdin accepted a non-key"
"$KEYS" key add 'Bad Name' > /dev/null 2>&1 && fail "accepted a bad name"
echo x | "$KEYS" key > "$T/out.txt" 2>&1 && fail "keys key < file did something"
grep -q "keys key add deepseek --stdin" "$T/out.txt" || fail "keys key < file wording"

# Answers in words: DeepSeek's prepaid balance, Google's free limit.
mk curl "cat > /dev/null; case \"\$*\" in *generateContent*) printf '{\"error\":{\"code\":429,\"status\":\"RESOURCE_EXHAUSTED\"}}\n429';; *) printf '{\"error\":{\"message\":\"Insufficient Balance\"}}\n402';; esac"
"$KEYS" ai-key --test > "$T/out3.txt" 2>&1 && fail "--test passed on a 402"
grep -q "prepaid, top it up" "$T/out3.txt" || fail "402 wording"
echo gemini >> "$T/kc"
"$KEYS" key test gemini > "$T/out4.txt" 2>&1 && fail "test passed on a 429"
grep -q "Google's free limit is used up" "$T/out4.txt" || fail "429 wording"

# No phone at all: plain words.
mk adb "case \"\$*\" in devices) printf 'List of devices attached\n';; esac"
"$KEYS" key test deepseek > "$T/out5.txt" 2>&1 || true
grep -q "phone not reachable: plug it in or turn on wireless debugging" "$T/out5.txt" || { cat "$T/out5.txt"; fail "no-phone wording"; }
echo "keys key: all checks passed"
