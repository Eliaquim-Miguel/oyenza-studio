"""Draw the Microsoft Store package images (MSIX) from the app icon.

    python tools/make_msix_assets.py crates/poni/res/poni.ico <layout>/Assets

The icon's small frames (16-48 px) are drawn pixel by pixel, so the taskbar and
Start list sizes copy them unchanged; every other size is scaled down from the
256 px frame. The names carry the scale-/targetsize- qualifiers that Windows
picks from (tools/make_msix.ps1 indexes them into resources.pri). Pillow only.
"""
import sys
from pathlib import Path

from PIL import Image

# Start list, taskbar, Settings > Apps: the icon at full size, no plate.
TARGET_SIZES = (16, 24, 32, 48, 256)
# (name, canvas px at 100 % scale, share of the canvas the icon fills)
LOGOS = (
    ("Square44x44Logo", 44, 1.0),
    ("Square150x150Logo", 150, 0.66),  # Windows 10 Start tile: icon on the accent colour
    ("StoreLogo", 50, 1.0),  # install dialog and Store fallback
)
SCALES = (100, 200)


def icon_frames(ico_path):
    ico = Image.open(ico_path)
    return {size[0]: ico.ico.getimage(size).convert("RGBA") for size in ico.ico.sizes()}


def icon_at(frames, px):
    if px in frames:
        return frames[px].copy()
    return frames[max(frames)].resize((px, px), Image.LANCZOS)


def logo(frames, canvas, share):
    px = max(1, round(canvas * share))
    out = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    off = (canvas - px) // 2
    out.alpha_composite(icon_at(frames, px), (off, off))
    return out


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    frames = icon_frames(sys.argv[1])
    if 256 not in frames:
        sys.exit(f"{sys.argv[1]}: needs a 256 px frame (has {sorted(frames)})")
    out = Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    written = []
    for size in TARGET_SIZES:
        img = icon_at(frames, size)
        for suffix in ("", "_altform-unplated"):
            name = f"Square44x44Logo.targetsize-{size}{suffix}.png"
            img.save(out / name)
            written.append(name)
    for base, canvas, share in LOGOS:
        for scale in SCALES:
            name = f"{base}.scale-{scale}.png"
            logo(frames, canvas * scale // 100, share).save(out / name)
            written.append(name)
    print(f"{len(written)} images in {out}")


if __name__ == "__main__":
    main()
