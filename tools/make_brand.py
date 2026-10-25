"""Draw Oyenza Studio's brand files: the app icon, the installer pictures and the logo used in the docs.

Oyenza Studio belongs to the Oyen family. Oyen is the orange cat of the Oyenza logo; docs/brand/oyen-mark*.svg are
that logo, copied unchanged from the brand kit. The logo is an orange tile with Poni's island hanging from its top
edge, playing music (album cover and equalizer, as on the real island), and the white Oyen below it. The plush Oyen
pictures in docs/brand/art are the source for the installer's welcome picture.

Run: python3 tools/make_brand.py  (needs Pillow)

Writes:
  crates/poni/res/poni.ico           app, window and tray icon; every size is drawn on its own
  installer/art/wizard-small-*.png   Inno Setup header logo (WizardSmallImageFile)
  installer/art/wizard-*.png         Inno Setup welcome and finish picture (WizardImageFile)
  installer/art/nsis-*.bmp           the same two for installer/poni.nsi
  docs/brand/oyen-poni-logo.svg      the logo as a vector
  docs/brand/oyen-poni-logo-512.png
"""
import re
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / "docs" / "brand"
RES = ROOT / "crates" / "poni" / "res"
ART = ROOT / "installer" / "art"

ORANGE, FOLD, INK = "#F0892E", "#C9581A", "#171717"
ISLAND, EQ, SUN = "#0A0A0A", "#FFA24A", "#FFE2B8"
COVER = [(0.0, "#FFB37A"), (0.45, "#F0708A"), (0.55, "#2B4F86"), (1.0, "#1B2F55")]  # sunset over the sea
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
# Inno Setup picks the file closest to its image area: 100%, 150% and 200% scaling.
WIZARD_SIZES = [(202, 386), (336, 643), (430, 824)]
WIZARD_SMALL_SIZES = [58, 77, 97, 116, 124, 143, 159]

# Geometry as fractions of the tile's side T.
INSET = 0.047  # margin around the tile, of the icon's side
TILE_R = 0.2245
ISL_W, ISL_H, ISL_R, ISL_FLARE = 0.655, 0.209, 0.075, 0.041
EQ_BARS = (0.45, 0.85, 0.6, 1.0)
EQ_W, EQ_GAP = 0.022, 0.016
OYEN_W, OYEN_CY = 0.51, 0.667  # white Oyen's width and centre, below the island
MARK_W, MARK_CY = 104.0, -11.04  # the mark's own width and vertical centre, in its units


def mark_paths(name: str) -> list[str]:
    svg = (BRAND / name).read_text()
    return re.findall(r'<path d="([^"]+)"', svg)


def flatten(d: str, steps: int = 16) -> list[list[tuple[float, float]]]:
    """Absolute M/L/Q/Z path data (all the mark uses) to polygons."""
    toks = re.findall(r"[MLQZ]|-?\d*\.?\d+(?:e-?\d+)?", d)
    polys, pts, i = [], [], 0
    num = lambda k: float(toks[k])
    while i < len(toks):
        t = toks[i]
        if t == "M":
            if pts:
                polys.append(pts)
            pts, i = [(num(i + 1), num(i + 2))], i + 3
        elif t == "L":
            pts.append((num(i + 1), num(i + 2)))
            i += 3
        elif t == "Q":
            (x0, y0), (cx, cy), (x1, y1) = pts[-1], (num(i + 1), num(i + 2)), (num(i + 3), num(i + 4))
            for s in range(1, steps + 1):
                u = s / steps
                pts.append(((1 - u) ** 2 * x0 + 2 * (1 - u) * u * cx + u * u * x1, (1 - u) ** 2 * y0 + 2 * (1 - u) * u * cy + u * u * y1))
            i += 5
        elif t == "Z":
            polys.append(pts)
            pts, i = [], i + 1
        else:
            raise ValueError(f"unexpected path token {t!r}")
    return polys


def evenodd(size: tuple[int, int], polys: list[list[tuple[float, float]]]) -> Image.Image:
    """A mask filled with the even-odd rule, the way the white mark's fold and eyes cut through its head."""
    mask = Image.new("1", size, 0)
    for poly in polys:
        layer = Image.new("1", size, 0)
        ImageDraw.Draw(layer).polygon(poly, fill=1)
        mask = ImageChops.logical_xor(mask, layer)
    return mask.convert("L")


def island_points(cx: float, y0: float, w: float, h: float, r: float, f: float) -> str:
    """Path data for Poni's island: flat top on the edge, flared top corners, rounded bottom corners."""
    x0, x1 = cx - w / 2, cx + w / 2
    return (
        f"M{x0 - f} {y0} Q{x0} {y0} {x0} {y0 + f} L{x0} {y0 + h - r} Q{x0} {y0 + h} {x0 + r} {y0 + h} "
        f"L{x1 - r} {y0 + h} Q{x1} {y0 + h} {x1} {y0 + h - r} L{x1} {y0 + f} Q{x1} {y0} {x1 + f} {y0} Z"
    )


def gradient(w: int, h: int, stops: list[tuple[float, str]]) -> Image.Image:
    rgb = [(u, Image.new("RGB", (1, 1), c).getpixel((0, 0))) for u, c in stops]
    col = Image.new("RGB", (1, max(1, h)))
    for y in range(h):
        u = y / max(1, h - 1)
        (u0, a), (u1, b) = next((rgb[k], rgb[k + 1]) for k in range(len(rgb) - 1) if u <= rgb[k + 1][0] or k == len(rgb) - 2)
        t = 0 if u1 == u0 else min(1, max(0, (u - u0) / (u1 - u0)))
        col.putpixel((0, y), tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3)))
    return col.resize((max(1, w), max(1, h)))


def layout(s: float) -> dict:
    """Pixel positions of every part on a canvas of side s."""
    inset = s * INSET
    t = s - 2 * inset
    w, h = ISL_W * t, ISL_H * t
    pad = 0.2 * h
    art = h - 2 * pad - 0.007 * t
    art_x, art_y = s / 2 - w / 2 + pad + 0.012 * t, inset + pad
    bw, gap = EQ_W * t, EQ_GAP * t
    eq_x = s / 2 + w / 2 - pad - 0.012 * t - (4 * bw + 3 * gap)
    return dict(inset=inset, t=t, w=w, h=h, art=art, art_x=art_x, art_y=art_y, bw=bw, gap=gap, eq_x=eq_x)


def draw_logo(size: int, ss: int = 8) -> Image.Image:
    """The logo on a transparent square, drawn large and scaled down for clean edges."""
    s = size * ss
    L = layout(s)
    inset, t = L["inset"], L["t"]
    tile = Image.new("L", (s, s), 0)
    ImageDraw.Draw(tile).rounded_rectangle([inset, inset, inset + t, inset + t], radius=TILE_R * t, fill=255)
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    img.paste(ORANGE, (0, 0), tile)

    # The island hangs from the tile's top edge; its flared corners follow the tile's rounded corners.
    isl = Image.new("L", (s, s), 0)
    pts = flatten(island_points(s / 2, inset - ss, L["w"], L["h"] + ss, ISL_R * t, ISL_FLARE * t))
    ImageDraw.Draw(isl).polygon(pts[0], fill=255)
    img.paste(ISLAND, (0, 0), ImageChops.multiply(isl, tile))

    if size >= 32:  # smaller, the cover and the bars are less than a pixel
        a, x, y = L["art"], L["art_x"], L["art_y"]
        cover = gradient(round(a), round(a), COVER).convert("RGBA")
        d = ImageDraw.Draw(cover)
        sun = (0.66 * a, 0.5 * a, 0.15 * a)
        d.pieslice([sun[0] - sun[2], sun[1] - sun[2], sun[0] + sun[2], sun[1] + sun[2]], 180, 360, fill=SUN)
        d.polygon([(0.24 * a, 0.80 * a), (0.34 * a, 0.56 * a), (0.42 * a, 0.80 * a)], fill="white")
        round_mask = Image.new("L", cover.size, 0)
        ImageDraw.Draw(round_mask).rounded_rectangle([0, 0, cover.width - 1, cover.height - 1], radius=0.24 * a, fill=255)
        img.paste(cover, (round(x), round(y)), round_mask)
        draw = ImageDraw.Draw(img)
        cy = y + a / 2
        for i, k in enumerate(EQ_BARS):
            bh = a * 0.9 * k
            bx = L["eq_x"] + i * (L["bw"] + L["gap"])
            draw.rounded_rectangle([bx, cy - bh / 2, bx + L["bw"], cy + bh / 2], radius=L["bw"] / 2, fill=EQ)

    # White Oyen: the brand kit's white mark, its fold and eyes cut out of the head.
    k = OYEN_W * t / MARK_W
    ox, oy = s / 2, inset + OYEN_CY * t - MARK_CY * k
    polys = [[(ox + px * k, oy + py * k) for px, py in poly] for d in mark_paths("oyen-mark-white.svg") for poly in flatten(d)]
    img.paste("white", (0, 0), evenodd((s, s), polys))
    return img.resize((size, size), Image.LANCZOS)


def logo_svg() -> str:
    s = 1024.0
    L = layout(s)
    inset, t = L["inset"], L["t"]
    a, x, y = L["art"], L["art_x"], L["art_y"]
    k = OYEN_W * t / MARK_W
    stops = "".join(f'<stop offset="{u:g}" stop-color="{c}"/>' for u, c in COVER)
    bars = "".join(
        f'<rect x="{L["eq_x"] + i * (L["bw"] + L["gap"]):.1f}" y="{y + a / 2 - a * 0.45 * h:.1f}" width="{L["bw"]:.1f}" '
        f'height="{a * 0.9 * h:.1f}" rx="{L["bw"] / 2:.1f}" fill="{EQ}"/>'
        for i, h in enumerate(EQ_BARS)
    )
    white = mark_paths("oyen-mark-white.svg")[0]
    r = 0.15 * a
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {s:g} {s:g}"><title>Oyenza Studio</title>'
        f'<defs><clipPath id="tile"><rect x="{inset:.1f}" y="{inset:.1f}" width="{t:.1f}" height="{t:.1f}" rx="{TILE_R * t:.1f}"/></clipPath>'
        f'<linearGradient id="cover" x1="0" y1="0" x2="0" y2="1">{stops}</linearGradient></defs>'
        f'<g clip-path="url(#tile)"><rect x="0" y="0" width="{s:g}" height="{s:g}" fill="{ORANGE}"/>'
        f'<path d="{island_points(s / 2, inset - 4, L["w"], L["h"] + 4, ISL_R * t, ISL_FLARE * t)}" fill="{ISLAND}"/></g>'
        f'<rect x="{x:.1f}" y="{y:.1f}" width="{a:.1f}" height="{a:.1f}" rx="{0.24 * a:.1f}" fill="url(#cover)"/>'
        f'<path d="M{x + 0.66 * a - r:.1f} {y + 0.5 * a:.1f}a{r:.1f} {r:.1f} 0 0 1 {2 * r:.1f} 0Z" fill="{SUN}"/>'
        f'<path d="M{x + 0.24 * a:.1f} {y + 0.8 * a:.1f}L{x + 0.34 * a:.1f} {y + 0.56 * a:.1f}L{x + 0.42 * a:.1f} {y + 0.8 * a:.1f}Z" fill="#fff"/>'
        f"{bars}"
        f'<g transform="translate({s / 2:.1f} {inset + OYEN_CY * t - MARK_CY * k:.1f}) scale({k:.4f})"><path d="{white}" fill="#fff" fill-rule="evenodd"/></g>'
        "</svg>\n"
    )


def trimmed(path: Path) -> Image.Image:
    im = Image.open(path).convert("RGBA")
    return im.crop(im.getchannel("A").point(lambda v: 255 if v > 32 else 0).getbbox())


def wizard_image(w: int, h: int, oyen: Image.Image) -> Image.Image:
    """Welcome/finish picture: Oyen on the island, playing music, on a warm cream field."""
    img = gradient(w, h, [(0.0, "#FFF6EC"), (1.0, "#FFE3C4")]).convert("RGBA")
    ow = round(w * 0.88)
    oh = round(oyen.height * ow / oyen.width)
    art = oyen.resize((ow, oh), Image.LANCZOS)
    img.alpha_composite(art, ((w - ow) // 2, round(h * 0.47 - oh / 2)))
    return img


def main() -> int:
    # App icon: every size drawn on its own.
    icons = {n: draw_logo(n) for n in ICO_SIZES}
    icons[256].save(RES / "poni.ico", format="ICO", sizes=[(n, n) for n in ICO_SIZES], append_images=[icons[n] for n in ICO_SIZES[:-1]])
    stale = RES / "poni-512.png"
    if stale.exists():
        stale.unlink()

    BRAND.joinpath("oyen-poni-logo.svg").write_text(logo_svg())
    draw_logo(512).save(BRAND / "oyen-poni-logo-512.png", optimize=True)

    ART.mkdir(exist_ok=True)
    for n in WIZARD_SMALL_SIZES:
        draw_logo(n).save(ART / f"wizard-small-{n}.png", optimize=True)
    oyen = trimmed(BRAND / "art" / "oyen-poni-sit.webp")
    for w, h in WIZARD_SIZES:
        wizard_image(w, h, oyen).convert("RGB").save(ART / f"wizard-{w}x{h}.png", optimize=True)

    # NSIS (MUI2) only takes 24-bit BMPs: 164x314 welcome/finish picture, 150x57 header.
    wizard_image(164, 314, oyen).convert("RGB").save(ART / "nsis-welcome.bmp")
    header = Image.new("RGBA", (150, 57), "white")
    header.alpha_composite(draw_logo(48), (150 - 48 - 6, (57 - 48) // 2))
    header.convert("RGB").save(ART / "nsis-header.bmp")

    for p in sorted([RES / "poni.ico", BRAND / "oyen-poni-logo.svg", BRAND / "oyen-poni-logo-512.png", *ART.iterdir()]):
        print(f"{p.relative_to(ROOT)}  {p.stat().st_size} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
