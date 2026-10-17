#!/usr/bin/env python3
"""Draws assets/icon.png (app/Dock icon): the orange tile with three fading chevrons,
same geometry as `widgets::logo`.   v/bin/python tools/gen_icon.py"""
from pathlib import Path
from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "assets" / "icon.png"
SS, SIZE = 4, 256            # supersampling, final size
W = SIZE * SS
inset = int(W * 0.1)         # macOS icons leave a margin around the tile
tile = W - 2 * inset
img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
ImageDraw.Draw(img).rounded_rectangle((inset, inset, W - inset - 1, W - inset - 1), radius=int(tile * 0.225), fill=(0xFF, 0x8A, 0x2A, 255))
s = tile / 32
for y, alpha in ((13, 255), (19, 158), (25, 77)):
    layer = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    pts = [(inset + x * s, inset + yy * s) for x, yy in ((10, y), (16, y - 5.5), (22, y))]
    d = ImageDraw.Draw(layer)
    d.line(pts, fill=(0x1A, 0x0F, 0x05, alpha), width=int(3 * s), joint="curve")
    for p in (pts[0], pts[2]):  # round the ends
        r = 1.5 * s
        d.ellipse((p[0] - r, p[1] - r, p[0] + r, p[1] + r), fill=(0x1A, 0x0F, 0x05, alpha))
    img.alpha_composite(layer)
img = img.resize((SIZE, SIZE), Image.LANCZOS)
img.quantize(colors=64, method=Image.Quantize.FASTOCTREE).save(OUT, "PNG", optimize=True)
print(OUT, OUT.stat().st_size, "bytes")
