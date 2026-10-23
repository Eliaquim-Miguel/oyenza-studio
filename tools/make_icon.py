"""Generate res/poni.ico: an ink tile with the black island and two eyes.

Run: python3 tools/make_icon.py  (needs Pillow)
"""
import sys
from pathlib import Path
from PIL import Image, ImageDraw

SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
OUT = Path(__file__).resolve().parent.parent / "crates" / "poni" / "res" / "poni.ico"


def render(px: int) -> Image.Image:
    s = 1024  # draw large, downsample for clean edges
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # Tile: deep indigo → violet vertical gradient, rounded square.
    grad = Image.new("RGBA", (s, s))
    gd = ImageDraw.Draw(grad)
    top, bot = (64, 72, 255), (168, 85, 247)
    for y in range(s):
        t = y / (s - 1)
        c = tuple(int(top[i] + (bot[i] - top[i]) * t) for i in range(3)) + (255,)
        gd.line([(0, y), (s, y)], fill=c)
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle([40, 40, s - 40, s - 40], radius=230, fill=255)
    img.paste(grad, (0, 0), mask)
    # The island: black pill hanging near the top.
    iw, ih = 600, 250
    x0 = (s - iw) // 2
    y0 = 250
    d.rounded_rectangle([x0, y0, x0 + iw, y0 + ih], radius=ih // 2, fill=(0, 0, 0, 255))
    # Two friendly eyes.
    ew, eh = 70, 120
    cy = y0 + ih // 2
    for cx in (s // 2 - 95, s // 2 + 95):
        d.rounded_rectangle([cx - ew // 2, cy - eh // 2, cx + ew // 2, cy + eh // 2], radius=ew // 2, fill=(255, 255, 255, 255))
    return img.resize((px, px), Image.LANCZOS)


def main() -> int:
    big = render(256)
    big.save(OUT, format="ICO", sizes=[(n, n) for n in SIZES])
    render(512).save(OUT.with_name("poni-512.png"))
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
