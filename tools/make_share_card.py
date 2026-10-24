"""Render the website's link preview card (og:image) from tools/share_card.html.

    python3 tools/make_share_card.py

Writes site/assets/share/oyenza-studio-share.jpg at 1200x630, the size Facebook,
LinkedIn, X and WhatsApp all show in full. The card is drawn at 2x and scaled
down so the type and the island stay sharp, and saved as a JPEG well under
300 KB (WhatsApp skips bigger preview images). Rename the file, or bump the
?v= on og:image in site/index.html, whenever the card changes: the networks
cache a preview by its URL.

Needs Playwright for Python with Chromium (pip install playwright, then
playwright install chromium) and Pillow.
"""
import io
from pathlib import Path

from PIL import Image
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "tools" / "share_card.html"
OUT = ROOT / "site" / "assets" / "share" / "oyenza-studio-share.jpg"
W, H = 1200, 630


def main() -> None:
    with sync_playwright() as p:
        browser = p.chromium.launch()
        page = browser.new_page(viewport={"width": W, "height": H}, device_scale_factor=2)
        page.goto(SRC.as_uri(), wait_until="networkidle")
        page.evaluate("document.fonts.ready.then(() => true)")
        if not page.evaluate("document.fonts.check('600 78px Outfit')"):
            raise SystemExit("Outfit did not load (the card needs fonts.googleapis.com)")
        broken = page.evaluate("[...document.images].filter(i => !i.naturalWidth).map(i => i.src)")
        if broken:
            raise SystemExit(f"images did not load: {broken}")
        png = page.screenshot(type="png")
        browser.close()
    img = Image.open(io.BytesIO(png)).convert("RGB").resize((W, H), Image.LANCZOS)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    img.save(OUT, "JPEG", quality=90, subsampling=0, optimize=True, progressive=True)
    print(f"{OUT.relative_to(ROOT)}  {W}x{H}  {OUT.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
