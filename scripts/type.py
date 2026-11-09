#!/usr/bin/env python3
# Types on ccez-keys by tapping its keys where `keys keymap` says they are,
# in whatever layout is up (Hindi, Thai, Korean...). For checks.
#   KEYS_SERIAL=<serial> scripts/type.py "text"
# "⌫" backspace, "⏎" enter, "‖" a pause; capitals and second-layer
# characters tap ⇧ first. Characters on no key are an error.
import json, os, subprocess, sys, time
S = os.environ["KEYS_SERIAL"]
here = os.path.dirname(os.path.abspath(__file__))
def keymap():
    out = subprocess.run(["cargo", "run", "-q", "--release", "-p", "ccez-keys-cli", "--bin", "keys", "--", "keymap"],
                         capture_output=True, text=True, cwd=os.path.join(here, ".."), env={**os.environ}).stdout
    return json.loads(out)
# The keyboard may still be coming up: a few tries.
keys = []
for _ in range(10):
    try:
        keys = keymap()
    except ValueError:
        keys = []
    if keys: break
    time.sleep(0.5)
if not keys: sys.exit("keyboard not up")
def find(kind, text=None, alt=None):
    for k in keys:
        if k["kind"] == kind and (text is None or k["text"] == text) and (alt is None or k["alt"] == alt):
            return k
    return None
cmds = []
def tap(k): cmds.append(f"input tap {k['x']} {k['y']}")
for c in sys.argv[1]:
    if c == "‖": cmds.append("sleep 0.6"); continue
    if c == "⌫": tap(find("backspace")); continue
    if c == "⏎": tap(find("enter")); continue
    if c == " ": tap(find("space")); continue
    k = find("char", text=c)
    if k: tap(k); continue
    k = find("char", alt=c) or (find("char", text=c.lower()) if c != c.lower() else None)
    if k:
        tap(find("shift")); tap(k); continue
    sys.exit(f"no key for {c!r}")
subprocess.run(["adb", "-s", S, "shell", "; ".join(cmds)])
