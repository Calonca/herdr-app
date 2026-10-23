# Fake pointer input for the sandbox's private X display (XTest). Coordinates
# are in the Herdr window, px. Run through `scripts/sandbox`, not directly.
#   move x0 y0 x1 y1        glide the pointer, no buttons
#   click x y [button]      button: 1 left (default), 3 right
#   drag x0 y0 x1 y1        press, glide, release
#   hold|release <keysym>...   keep keys down across commands, e.g. `hold Control_L`
#   type <text>             type it (keys of the X keymap; shift as needed)
#   key <keysym> [mods...]  one key, e.g. `key Return`, `key l Control_L`
import sys
import time

from Xlib import X, XK, display
from Xlib.ext import xtest

d = display.Display()
root = d.screen().root


def find(w):
    if w.get_wm_name() == "Herdr":
        return w
    for c in w.query_tree().children:
        r = find(c)
        if r:
            return r
    return None


win = find(root)
if win is None:
    sys.exit("no Herdr window on this display")
pos = win.translate_coords(root, 0, 0)
ox, oy = -pos.x, -pos.y


def to(x, y):
    xtest.fake_input(d, X.MotionNotify, x=int(ox + x), y=int(oy + y))
    d.sync()


def glide(x0, y0, x1, y1, steps=24):
    for i in range(steps + 1):
        to(x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps)
        time.sleep(0.016)


def button(n, down):
    xtest.fake_input(d, X.ButtonPress if down else X.ButtonRelease, n)
    d.sync()


def key(sym, mods=()):
    code = d.keysym_to_keycode(XK.string_to_keysym(sym))
    held = [d.keysym_to_keycode(XK.string_to_keysym(m)) for m in mods]
    for m in held:
        xtest.fake_input(d, X.KeyPress, m)
    xtest.fake_input(d, X.KeyPress, code)
    xtest.fake_input(d, X.KeyRelease, code)
    for m in reversed(held):
        xtest.fake_input(d, X.KeyRelease, m)
    d.sync()
    time.sleep(0.03)


if sys.argv[1] in ("type", "key"):
    if sys.argv[1] == "key":
        key(sys.argv[2], sys.argv[3:])
    for ch in sys.argv[2] if sys.argv[1] == "type" else "":
        name = {" ": "space", ".": "period", "/": "slash", ":": "colon", "-": "minus"}.get(ch, ch)
        keysym = XK.string_to_keysym(name)
        shifted = d.keysym_to_keycodes(keysym) and any(i % 2 for _, i in d.keysym_to_keycodes(keysym))
        key(name, ["Shift_L"] if shifted or ch.isupper() else [])
    time.sleep(0.1)
    sys.exit(0)

if sys.argv[1] in ("hold", "release"):
    for sym in sys.argv[2:]:
        code = d.keysym_to_keycode(XK.string_to_keysym(sym))
        xtest.fake_input(d, X.KeyPress if sys.argv[1] == "hold" else X.KeyRelease, code)
    d.sync()
    time.sleep(0.1)
    sys.exit(0)

cmd, args = sys.argv[1], [float(a) for a in sys.argv[2:]]
if cmd == "move":
    glide(*args[:4])
elif cmd == "click":
    to(args[0], args[1])
    time.sleep(0.05)
    b = int(args[2]) if len(args) > 2 else 1
    button(b, True)
    time.sleep(0.05)
    button(b, False)
elif cmd == "drag":
    to(args[0], args[1])
    time.sleep(0.05)
    button(1, True)
    time.sleep(0.05)
    glide(*args[:4])
    time.sleep(0.1)
    button(1, False)
else:
    sys.exit(f"unknown input command {cmd}")
time.sleep(0.1)
