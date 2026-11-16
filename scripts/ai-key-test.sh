#!/bin/sh
# keys ai-key end to end with stand-ins for every tool it drives (no real
# clipboard, keychain, phone, Cloudflare, git push or API): a fake key goes
# in, and the check is that it reaches each place on stdin only, never on
# a command line or in what the command prints.
#   sh scripts/ai-key-test.sh   (from the repo; builds keys first)
set -eu
cd "$(dirname "$0")/.."
cargo build -q -p ccez-keys-cli --bin keys
KEYS="$PWD/target/debug/keys"
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
FAKE=sk-test0fake0key0000000000000000000
b64() { printf %s "$1" | base64; }

mk() { printf '#!/bin/sh\n%s\n' "$2" > "$T/$1"; chmod +x "$T/$1"; }
# Each stand-in logs its arguments to args.log and its stdin to stdin.log.
LOG='echo "$0 $*" >> '"$T"'/args.log'
mk pbpaste "printf '%s\n' '$FAKE'"
mk security "$LOG; cat >> $T/stdin.log"
mk launchctl "$LOG"
mk wrangler "$LOG
case \"\$*\" in
  *'secret put'*) cat >> $T/stdin.log; echo >> $T/stdin.log; echo 'Success! Uploaded secret DEEPSEEK_API_KEY';;
  *'pages secret list'*) echo '  - DEEPSEEK_API_KEY: Value Encrypted';;
  *'secret list'*) echo '[{\"name\":\"DEEPSEEK_API_KEY\",\"type\":\"secret_text\"}]';;
esac"
mk git "$LOG
case \"\$*\" in *commit-tree*) echo 0123456789abcdef0123456789abcdef01234567;; esac"
mk curl "$LOG; cat > /dev/null
case \"\$*\" in
  *chat/completions*) printf '{\"choices\":[{\"message\":{\"content\":\"OK\"}}]}\n200';;
  *user/balance*) printf '{\"is_available\":true,\"balance_infos\":[{\"currency\":\"USD\",\"total_balance\":\"4.20\"}]}\n200';;
esac"
mk adb "$LOG
case \"\$*\" in
  devices) printf 'List of devices attached\nFAKE\tdevice\n';;
  *'method key'*) cat >> $T/stdin.log; echo 'Result: Bundle[{out=$(b64 saved)}]';;
  *'method ai-test'*) echo 'Result: Bundle[{out=$(b64 OK)}]';;
esac"

export KEYS_PBPASTE="$T/pbpaste" KEYS_SECURITY="$T/security" KEYS_LAUNCHCTL="$T/launchctl" KEYS_WRANGLER="$T/wrangler" \
  KEYS_GIT="$T/git" KEYS_CURL="$T/curl" KEYS_ADB="$T/adb" KEYS_SWE="$T/swe" KEYS_HUB="http://127.0.0.1:9/ai/deepseek"
unset KEYS_SERIAL || true

fail() { echo "FAIL: $*"; exit 1; }
"$KEYS" ai-key > "$T/out.txt" 2>&1 || { cat "$T/out.txt"; fail "keys ai-key exited non-zero"; }
cat "$T/out.txt"
grep -q "$FAKE" "$T/out.txt" && fail "the key was printed"
grep -q "$FAKE" "$T/args.log" && fail "the key was on a command line"
grep -q "add-generic-password -U -s uk.ccez.ai -a deepseek .* -w $FAKE" "$T/stdin.log" || fail "keychain didn't get it on stdin"
[ "$(grep -c "^$FAKE\$" "$T/stdin.log")" = 3 ] || fail "the three Cloudflare secrets didn't each get it on stdin"
grep -q "^$(b64 "$FAKE")\$" "$T/stdin.log" || fail "the phone didn't get it on stdin"
grep -q "secret put DEEPSEEK_API_KEY --name ccez-keeps" "$T/args.log" || fail "ccez-keeps"
grep -q "pages secret put DEEPSEEK_API_KEY --project-name art-website" "$T/args.log" || fail "art-website"
grep -q "pages secret put DEEPSEEK_API_KEY --project-name personalized-llm-client" "$T/args.log" || fail "ccez-llm"
grep -q "swe/ccez-llm push -q origin 0123456789abcdef0123456789abcdef01234567:refs/heads/main" "$T/args.log" || fail "ccez-llm redeploy"
grep -q "kickstart -k gui/.*/uk.ccez.words" "$T/args.log" || fail "ccez-words restart"
grep -q "balance 4.20 USD" "$T/out.txt" || fail "balance"

# A clipboard without a key (and no prompt): nothing is stored.
: > "$T/stdin.log"
mk pbpaste "echo 'some copied text'"
KEYS_NO_PROMPT=1 "$KEYS" ai-key < /dev/null > "$T/out2.txt" 2>&1 && fail "accepted a non-key"
[ -s "$T/stdin.log" ] && fail "stored something without a key"
# --stdin refuses a non-key too.
echo "not-a-key" | "$KEYS" ai-key --stdin > /dev/null 2>&1 && fail "--stdin accepted a non-key"
# A 402 from the hub is named as the prepaid balance.
mk curl "cat > /dev/null; printf '{\"error\":{\"message\":\"Insufficient Balance\"}}\n402'"
"$KEYS" ai-key --test > "$T/out3.txt" 2>&1 && fail "--test passed on a 402"
grep -q "prepaid, top it up" "$T/out3.txt" || fail "402 wording"
echo "ai-key: all checks passed"
