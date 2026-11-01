"""Draw the Windows icon and the installer splash for Warcraft Survivors.

A gold-ringed medallion with two crossed swords over an ember glow, drawn from shapes (no game
art), at 1024 px and scaled down for the small sizes.

    uv run --with pillow python scripts/make-icon.py

Writes `assets/windows/icon.ico` (16 to 256 px), `assets/windows/icon.png` (256 px) and
`assets/windows/splash.png` (the picture Velopack shows while it installs).
"""

import math
import pathlib

from PIL import Image, ImageDraw, ImageFilter, ImageFont

OUT = pathlib.Path(__file__).resolve().parent.parent / "assets" / "windows"
S = 1024  # the drawing size; everything below is in these pixels
C = S / 2

GOLD_LIGHT = (255, 226, 140)
GOLD = (214, 160, 52)
GOLD_DARK = (120, 78, 20)
STEEL_LIGHT = (236, 240, 248)
STEEL = (170, 180, 196)
STEEL_DARK = (92, 100, 116)


def radial(size, inner, outer, power=1.0):
    """A disc-filling radial gradient from `inner` at the centre to `outer` at the edge."""
    img = Image.new("RGBA", (size, size))
    px = img.load()
    half = size / 2
    for y in range(size):
        for x in range(size):
            t = min(1.0, math.hypot(x - half, y - half) / half) ** power
            px[x, y] = tuple(round(a + (b - a) * t) for a, b in zip(inner, outer))
    return img


def ring(draw, r, width, fill):
    draw.ellipse((C - r, C - r, C + r, C + r), outline=fill, width=width)


def sword(angle):
    """One sword, point up, rotated by `angle` degrees about the centre, on its own layer."""
    layer = Image.new("RGBA", (S, S))
    d = ImageDraw.Draw(layer)
    blade_w, tip, guard_y = 46, 190, 640
    # The blade: a long pointed shape with a light and a dark half for the edge.
    d.polygon([(C, tip), (C + blade_w, tip + 90), (C + blade_w, guard_y), (C, guard_y)], fill=STEEL)
    d.polygon([(C, tip), (C - blade_w, tip + 90), (C - blade_w, guard_y), (C, guard_y)], fill=STEEL_LIGHT)
    d.line([(C, tip + 40), (C, guard_y - 10)], fill=STEEL_DARK, width=8)
    # The crossguard, the grip and the pommel.
    d.rounded_rectangle((C - 150, guard_y, C + 150, guard_y + 44), radius=20, fill=GOLD)
    d.rounded_rectangle((C - 150, guard_y, C + 150, guard_y + 16), radius=8, fill=GOLD_LIGHT)
    d.rectangle((C - 24, guard_y + 44, C + 24, guard_y + 190), fill=(92, 46, 26))
    for y in range(guard_y + 56, guard_y + 186, 26):
        d.line([(C - 24, y), (C + 24, y + 12)], fill=(60, 28, 14), width=6)
    d.ellipse((C - 44, guard_y + 180, C + 44, guard_y + 268), fill=GOLD)
    d.ellipse((C - 26, guard_y + 196, C + 10, guard_y + 232), fill=GOLD_LIGHT)
    return layer.rotate(angle, resample=Image.BICUBIC, center=(C, C))


def medallion():
    img = Image.new("RGBA", (S, S))
    # The field: a deep crimson fading to near black, inside the ring.
    field = radial(S, (150, 28, 12, 255), (18, 6, 6, 255), power=0.8)
    mask = Image.new("L", (S, S))
    ImageDraw.Draw(mask).ellipse((C - 430, C - 430, C + 430, C + 430), fill=255)
    img.paste(field, (0, 0), mask)
    # The ember behind the swords: a soft orange glow.
    glow = Image.new("RGBA", (S, S))
    ImageDraw.Draw(glow).ellipse((C - 230, C - 230, C + 230, C + 230), fill=(255, 140, 40, 200))
    img.alpha_composite(glow.filter(ImageFilter.GaussianBlur(90)))
    # The swords, with a dark shadow under them.
    swords = Image.new("RGBA", (S, S))
    swords.alpha_composite(sword(38))
    swords.alpha_composite(sword(-38))
    shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    shadow.paste((0, 0, 0, 170), (0, 0), swords.split()[3])
    img.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(14)), (10, 16))
    img.alpha_composite(swords)
    # The gold ring: a dark outer line, a broad gold band with a light inner edge.
    d = ImageDraw.Draw(img)
    ring(d, 484, 22, GOLD_DARK)
    ring(d, 466, 56, GOLD)
    ring(d, 452, 12, GOLD_LIGHT)
    ring(d, 424, 10, GOLD_DARK)
    # Four studs on the ring.
    for a in range(4):
        x = C + 466 * math.cos(math.radians(45 + 90 * a)) - 26
        y = C + 466 * math.sin(math.radians(45 + 90 * a)) - 26
        d.ellipse((x, y, x + 52, y + 52), fill=GOLD_LIGHT, outline=GOLD_DARK, width=6)
    return img


def font(size):
    for path in (
        "/System/Library/Fonts/Supplemental/Baskerville.ttc",
        "/System/Library/Fonts/Supplemental/Georgia Bold.ttf",
        "C:/Windows/Fonts/georgiab.ttf",
    ):
        if pathlib.Path(path).exists():
            return ImageFont.truetype(path, size)
    return ImageFont.load_default(size)


def splash(icon):
    w, h = 720, 360
    img = radial(w, (70, 18, 10, 255), (10, 4, 4, 255), power=0.9).resize((w, h))
    img.alpha_composite(icon.resize((260, 260), Image.LANCZOS), (40, 50))
    d = ImageDraw.Draw(img)
    for text, size, y, fill in (
        ("WARCRAFT", 58, 112, GOLD_LIGHT),
        ("SURVIVORS", 58, 176, GOLD),
        ("Installing...", 22, 262, (220, 200, 170)),
    ):
        d.text((330, y), text, font=font(size), fill=fill)
    return img


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    big = medallion()
    icon = big.resize((256, 256), Image.LANCZOS)
    icon.save(OUT / "icon.png")
    big.save(OUT / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    splash(big).convert("RGB").save(OUT / "splash.png")
    print("wrote", *(p.name for p in OUT.iterdir()))


if __name__ == "__main__":
    main()
