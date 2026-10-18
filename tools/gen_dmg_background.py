#!/usr/bin/env python3
"""Draws the .dmg window background (packaging/macos/dmg-background{,@2x}.png): a title, an arrow
from the app to the Applications folder and a hint. Icons sit at (170,190) and (490,190) of the
660x400 window, see tools/package-macos.sh.   python tools/gen_dmg_background.py  (needs Pillow)"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "packaging" / "macos"
FONTS = ROOT / "assets" / "fonts"
W, H = 660, 400


def draw(scale: int) -> Image.Image:
    s = scale
    img = Image.new("RGB", (W * s, H * s), (0xF6, 0xF2, 0xEC))
    d = ImageDraw.Draw(img)
    title = ImageFont.truetype(str(FONTS / "Bricolage-Bold.ttf"), 27 * s)
    hint = ImageFont.truetype(str(FONTS / "Geist-Regular.ttf"), 14 * s)

    def centered(text, y, font, fill):
        w = d.textlength(text, font=font)
        d.text(((W * s - w) / 2, y * s), text, font=font, fill=fill)

    centered("Arrastra Updateeer a Aplicaciones", 44, title, (0x2A, 0x1F, 0x14))
    centered("Después ábrela desde la carpeta Aplicaciones", 336, hint, (0x8A, 0x7C, 0x6E))

    # Arrow: shaft plus head, orange like the app icon.
    orange = (0xFF, 0x8A, 0x2A)
    y, x0, x1 = 190 * s, 262 * s, 398 * s
    d.line([(x0, y), (x1 - 8 * s, y)], fill=orange, width=7 * s)
    d.polygon([(x1 + 4 * s, y), (x1 - 22 * s, y - 20 * s), (x1 - 22 * s, y + 20 * s)], fill=orange)
    return img


draw(1).save(OUT / "dmg-background.png")
draw(2).save(OUT / "dmg-background@2x.png")
