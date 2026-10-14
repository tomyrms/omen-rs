"""Draws the OMEN Hub RS logo (a red fan in a dark rounded square).

Outputs, next to this script:
  omen-hub-rs.png   1024 px preview
  omen-hub-rs.ico   16..256 px, embedded in omen-hub.exe
  icon-64.rgba      raw RGBA for the window and tray icon
  icon-256.rgba     raw RGBA for the in-app header

Run: python assets/make_logo.py  (needs Pillow)
"""
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

S = 1024
OUT = Path(__file__).parent
RED = (226, 35, 46, 255)
RED_DARK = (150, 18, 28, 255)
BG = (22, 23, 28, 255)


def blade(d, angle, r_in, r_out, sweep, color):
    """Curved fan blade: a polygon swept along an arc."""
    pts = []
    steps = 40
    c = S / 2
    for i in range(steps + 1):
        t = i / steps
        r = r_in + (r_out - r_in) * t
        a = angle + sweep * t * t
        pts.append((c + r * math.cos(a), c + r * math.sin(a)))
    width = 0.55
    for i in range(steps, -1, -1):
        t = i / steps
        r = r_in + (r_out - r_in) * t
        a = angle + sweep * t * t - width * (1 - 0.65 * t)
        pts.append((c + r * math.cos(a), c + r * math.sin(a)))
    d.polygon(pts, fill=color)


def draw():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([24, 24, S - 24, S - 24], radius=210, fill=BG)

    # Soft red glow behind the fan.
    glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse([200, 200, S - 200, S - 200], fill=(226, 35, 46, 90))
    img.alpha_composite(glow.filter(ImageFilter.GaussianBlur(70)))
    d = ImageDraw.Draw(img)

    c = S / 2
    # Outer ring.
    ring = 370
    d.ellipse([c - ring, c - ring, c + ring, c + ring], outline=RED, width=46)
    # Five curved blades.
    for k in range(5):
        a = k * 2 * math.pi / 5 - math.pi / 2
        blade(d, a, 95, 318, 0.9, RED)
    # Hub.
    d.ellipse([c - 112, c - 112, c + 112, c + 112], fill=BG)
    d.ellipse([c - 78, c - 78, c + 78, c + 78], fill=RED)
    d.ellipse([c - 30, c - 30, c + 30, c + 30], fill=RED_DARK)
    return img


def main():
    img = draw()
    img.save(OUT / "omen-hub-rs.png")
    img.save(OUT / "omen-hub-rs.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    for n in (64, 256):
        small = img.resize((n, n), Image.LANCZOS)
        (OUT / f"icon-{n}.rgba").write_bytes(small.tobytes())


if __name__ == "__main__":
    main()
