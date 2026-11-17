"""Generate the app logo (SVG, PNG, ICO) from one shared geometry.

Run from app/:  python assets/make_logo.py
Needs Pillow. Uses Segoe UI Bold (Windows) for the digits.
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

OUT = Path(__file__).parent
S = 1024  # design canvas

# palette (same as src/main.rs)
BG_TOP, BG_BOT = (0x2e, 0x3b, 0x56), (0x17, 0x20, 0x33)
ACCENT = (0x49, 0x90, 0xff)
SCREEN_OFF = (0x1f, 0x2a, 0x42)
FRAME = (0x8a, 0xb1, 0xed)
TEXT = (0xf1, 0xf1, 0xf1)

RADIUS = 224
MON_W, MON_H, MON_R, STROKE = 360, 252, 44, 30
Y0 = 318
LEFT_X, RIGHT_X = 132, 532
NECK_W, NECK_H = 44, 70
BASE_W, BASE_H, BASE_R = 168, 30, 15
FONT_SIZE = 176
FONT = "C:/Windows/Fonts/segoeuib.ttf"

monitors = [  # (x, label, selected)
    (LEFT_X, "1", False),
    (RIGHT_X, "2", True),
]


def hexc(c):
    return "#%02x%02x%02x" % c


def svg():
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {S} {S}" width="{S}" height="{S}">',
        '<defs><linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">'
        f'<stop offset="0" stop-color="{hexc(BG_TOP)}"/><stop offset="1" stop-color="{hexc(BG_BOT)}"/>'
        '</linearGradient></defs>',
        f'<rect width="{S}" height="{S}" rx="{RADIUS}" fill="url(#bg)"/>',
    ]
    for x, label, sel in monitors:
        frame = ACCENT if sel else FRAME
        screen = ACCENT if sel else SCREEN_OFF
        cx = x + MON_W / 2
        h = STROKE / 2
        parts.append(
            f'<rect x="{x + h}" y="{Y0 + h}" width="{MON_W - STROKE}" height="{MON_H - STROKE}" '
            f'rx="{MON_R - h}" fill="{hexc(screen)}" stroke="{hexc(frame)}" stroke-width="{STROKE}"/>'
        )
        parts.append(
            f'<rect x="{cx - NECK_W / 2}" y="{Y0 + MON_H}" width="{NECK_W}" height="{NECK_H}" fill="{hexc(frame)}"/>'
        )
        parts.append(
            f'<rect x="{cx - BASE_W / 2}" y="{Y0 + MON_H + NECK_H}" width="{BASE_W}" height="{BASE_H}" '
            f'rx="{BASE_R}" fill="{hexc(frame)}"/>'
        )
        parts.append(
            f'<text x="{cx}" y="{Y0 + MON_H / 2}" fill="{hexc(TEXT)}" font-family="Segoe UI, Arial, sans-serif" '
            f'font-weight="700" font-size="{FONT_SIZE}" text-anchor="middle" dominant-baseline="central">{label}</text>'
        )
    parts.append("</svg>")
    return "\n".join(parts) + "\n"


def raster(size):
    k = 4  # supersample
    W = size * k
    f = W / S
    img = Image.new("RGBA", (W, W), (0, 0, 0, 0))

    grad = Image.new("RGBA", (W, W))
    gd = ImageDraw.Draw(grad)
    for y in range(W):
        t = y / (W - 1)
        gd.line([(0, y), (W, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(BG_TOP, BG_BOT)) + (255,))
    mask = Image.new("L", (W, W), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, W - 1, W - 1], RADIUS * f, fill=255)
    img.paste(grad, (0, 0), mask)

    d = ImageDraw.Draw(img)
    font = ImageFont.truetype(FONT, round(FONT_SIZE * f))
    for x, label, sel in monitors:
        frame = ACCENT if sel else FRAME
        screen = ACCENT if sel else SCREEN_OFF
        cx = x + MON_W / 2
        d.rounded_rectangle(
            [x * f, Y0 * f, (x + MON_W) * f, (Y0 + MON_H) * f], MON_R * f, fill=frame
        )
        d.rounded_rectangle(
            [(x + STROKE) * f, (Y0 + STROKE) * f, (x + MON_W - STROKE) * f, (Y0 + MON_H - STROKE) * f],
            (MON_R - STROKE) * f,
            fill=screen,
        )
        d.rectangle(
            [(cx - NECK_W / 2) * f, (Y0 + MON_H) * f, (cx + NECK_W / 2) * f, (Y0 + MON_H + NECK_H) * f], fill=frame
        )
        by = Y0 + MON_H + NECK_H
        d.rounded_rectangle(
            [(cx - BASE_W / 2) * f, by * f, (cx + BASE_W / 2) * f, (by + BASE_H) * f], BASE_R * f, fill=frame
        )
        d.text((cx * f, (Y0 + MON_H / 2) * f), label, font=font, fill=TEXT, anchor="mm")
    return img.resize((size, size), Image.LANCZOS)


if __name__ == "__main__":
    (OUT / "logo.svg").write_text(svg(), encoding="utf-8")
    raster(1024).save(OUT / "logo.png")
    raster(256).save(OUT / "icon-256.png")
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    big = raster(256)
    big.save(OUT / "icon.ico", sizes=[(s, s) for s in sizes],
             append_images=[raster(s) for s in sizes if s != 256])
    print("ok")
