# Types on ccez-keys on the phone by tapping key centres (px), for checks.
# Geometry from KeyboardView: density 3 (S24), bar 46dp, padTop 6dp, rows 44dp, gaps 8dp, side 3dp.
# KEYS_SERIAL=<serial> scripts/phone-type.py "deja " (" " space, "<" backspace, "~" next language, "!" pause)
import os, subprocess, sys, re, time
S = os.environ["KEYS_SERIAL"]
d = 3.0
def sh(c): return subprocess.run(["adb", "-s", S, "shell", c], capture_output=True, text=True).stdout
# From the bottom: the keys sit on the system's tappable strip (48dp on the
# S24), whatever is above them (strip, bar, Ask). Key size from keyboard.height.
H = int(re.search(r"Physical size: \d+x(\d+)", sh("wm size")).group(1))
scale = int(subprocess.run(["cargo", "run", "-q", "--release", "-p", "ccez-keys-cli", "--bin", "keys", "--", "get", "keyboard.height"],
                           capture_output=True, text=True, env={**os.environ, "KEYS_SERIAL": S}).stdout.strip() or 100) / 100
row_h = 44 * d * scale
bottom = H - 48 * d - 6 * d
row_y = lambda r: bottom - (3 - r) * (row_h + 8 * d) - row_h / 2
u = (1080 - 6 * d) / 10
side = 3 * d
rows = ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
pos = {}
for i, c in enumerate(rows[0]): pos[c] = (side + (i + .5) * u, row_y(0))
for i, c in enumerate(rows[1]): pos[c] = (side + (.5 + i + .5) * u, row_y(1))
for i, c in enumerate(rows[2]): pos[c] = (side + (1.5 + i + .5) * u, row_y(2))
pos["<"] = (side + 9.25 * u, row_y(2))
bu = (1080 - 2 * side) / 9.0
pos[" "] = (side + 5.0 * bu, row_y(3))
pos["."] = (side + 7.3 * bu, row_y(3))
pos[","] = (side + 2.7 * bu, row_y(3))
pos["^"] = (side + 0.75 * u, row_y(2))
cmds = []
for ch in sys.argv[1]:
    if ch == "~":
        x, y = pos[" "]; cmds.append(f"input swipe {int(x-100)} {int(y)} {int(x+120)} {int(y)} 250")
    elif ch == "!":
        cmds.append("sleep 0.6")
    else:
        x, y = pos[ch]; cmds.append(f"input tap {int(x)} {int(y)}")
sh("; ".join(cmds))
