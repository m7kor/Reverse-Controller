"""Draw the Reverse Controller app icon and write assets/icon.ico.

An amber rounded square (the app's accent colour) with a dark gamepad, the same
silhouette as the tray icon. Run with Python 3 and Pillow:

    python assets/make_icon.py
"""
from pathlib import Path

from PIL import Image, ImageDraw

S = 1024  # drawn large, then scaled down for each icon size
AMBER = (255, 170, 0, 255)
DARK = (22, 23, 28, 255)

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle([40, 40, S - 40, S - 40], radius=220, fill=AMBER)

# Gamepad body: a rounded bar plus two grips.
d.rounded_rectangle([170, 330, 854, 640], radius=150, fill=DARK)
d.ellipse([170, 470, 450, 790], fill=DARK)
d.ellipse([574, 470, 854, 790], fill=DARK)

# D-pad (cut out in amber) on the left, four face buttons on the right.
cx, cy, arm, w = 340, 485, 95, 42
d.rounded_rectangle([cx - w, cy - arm, cx + w, cy + arm], radius=14, fill=AMBER)
d.rounded_rectangle([cx - arm, cy - w, cx + arm, cy + w], radius=14, fill=AMBER)
for bx, by in [(690, 405), (690, 565), (610, 485), (770, 485)]:
    d.ellipse([bx - 40, by - 40, bx + 40, by + 40], fill=AMBER)

out = Path(__file__).with_name("icon.ico")
sizes = [(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (24, 24), (16, 16)]
img.save(out, sizes=sizes)
img.resize((256, 256), Image.LANCZOS).save(Path(__file__).with_name("icon.png"))
print(f"wrote {out}")
