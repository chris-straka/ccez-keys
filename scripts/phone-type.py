# Types on ccez-keys on the phone by tapping key centres (px), for checks.
# Geometry from KeyboardView: density 3 (S24), bar 46dp, padTop 6dp, rows 44dp, gaps 8dp, side 3dp.
# KEYS_SERIAL=<serial> scripts/phone-type.py "deja " (" " space, "<" backspace, "~" next language, "!" pause)
import os, subprocess, sys, re, time
S = os.environ["KEYS_SERIAL"]
def sh(c): return subprocess.run(["adb", "-s", S, "shell", c], capture_output=True, text=True).stdout
d = 3.0
frame = re.search(r"InputMethod, frame=\[Rect\(0, (\d+) - 1080, (\d+)\)", sh("dumpsys window InputMethod"))
top = int(frame.group(1)) + 46 * d
row_y = lambda r: top + 6 * d + r * (44 * d + 8 * d) + 22 * d
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
