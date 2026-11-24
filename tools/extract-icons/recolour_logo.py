"""Recolour the Chrome logo into Chrust's palette.

Chrome's mark is three outer segments (red, green, yellow) around a blue disc,
with a white ring and gap between them. Recolouring pixel by pixel by hue keeps
the original shading, the soft gradients and the antialiased edges instead of
flattening the logo into four solid colours.

The white ring and gap are left alone: they carry almost no saturation, so they
are outside the hue mapping and keep Chrome's white, which is what separates the
segments on any background.
"""
import colorsys
import sys
from PIL import Image

# Chrome's four colours, measured on the icon, and what they become.
# Each entry: (source hue in degrees, target RGB).
MAPPINGS = {
    "aurora": {
        "name": "Aurora",
        "segments": [
            (4.0, (0x8B, 0x7B, 0xFF)),    # red    -> accent   violet
            (45.0, (0x4F, 0x7C, 0xFF)),   # yellow -> sky      blue
            (136.0, (0x2D, 0xD4, 0xBF)),  # green  -> accent2  teal
            (217.0, (0xE6, 0xE8, 0xF2)),  # centre -> foreground off-white
        ],
    },
    "aurora-dark": {
        "name": "Aurora, dark centre",
        "segments": [
            (4.0, (0x8B, 0x7B, 0xFF)),
            (45.0, (0x4F, 0x7C, 0xFF)),
            (136.0, (0x2D, 0xD4, 0xBF)),
            (217.0, (0x10, 0x13, 0x1F)),  # centre -> the night background
        ],
    },
    "aurora-two": {
        "name": "Aurora, two colours",
        "segments": [
            (4.0, (0x8B, 0x7B, 0xFF)),
            (45.0, (0xA7, 0x8B, 0xFA)),   # yellow -> a lighter violet
            (136.0, (0x2D, 0xD4, 0xBF)),
            (217.0, (0x10, 0x13, 0x1F)),
        ],
    },
}

# Below this saturation a pixel is white or grey (the ring, the gap, the
# highlights) and is not part of a coloured segment.
SATURATION_FLOOR = 0.10


def hue_distance(a, b):
    d = abs(a - b) % 360.0
    return min(d, 360.0 - d)


def recolour(img, segments):
    out = img.copy()
    src_px, dst_px = img.load(), out.load()
    width, height = img.size

    # The value the icon gives each segment, so shading is kept: a pixel that is
    # 10% darker than its segment stays 10% darker than its new colour.
    peak = {h: 0.0 for h, _ in segments}
    for y in range(height):
        for x in range(width):
            r, g, b, a = src_px[x, y]
            if a == 0:
                continue
            h, s, v = colorsys.rgb_to_hsv(r / 255, g / 255, b / 255)
            if s < SATURATION_FLOOR:
                continue
            hue = h * 360.0
            nearest = min(segments, key=lambda seg: hue_distance(hue, seg[0]))
            peak[nearest[0]] = max(peak[nearest[0]], v)

    for y in range(height):
        for x in range(width):
            r, g, b, a = src_px[x, y]
            if a == 0:
                continue
            h, s, v = colorsys.rgb_to_hsv(r / 255, g / 255, b / 255)
            hue = h * 360.0
            if s < SATURATION_FLOOR:
                continue
            source_hue, (tr, tg, tb) = min(
                segments, key=lambda seg: hue_distance(hue, seg[0])
            )
            th, ts, tv = colorsys.rgb_to_hsv(tr / 255, tg / 255, tb / 255)
            # Keep the pixel's own light: bright stays bright, the faint rim of
            # an antialiased edge stays faint.
            # Mostly the target's own brightness, with just enough of the
            # source's light to keep the shading readable.
            top = peak[source_hue] or 1.0
            value = min(1.0, tv * (0.88 + 0.12 * min(1.0, v / top)))
            sat = ts * (0.80 + 0.20 * min(1.0, s / 0.85))
            nr, ng, nb = colorsys.hsv_to_rgb(th, sat, value)
            dst_px[x, y] = (round(nr * 255), round(ng * 255), round(nb * 255), a)
    return out


def main():
    source = sys.argv[1]
    outdir = sys.argv[2]
    key = sys.argv[3]
    palette = MAPPINGS[key]
    base = Image.open(source).convert("RGBA")
    if base.size != (256, 256):
        base = base.resize((256, 256), Image.LANCZOS)

    big = recolour(base, palette["segments"])
    suffix = "" if key == "aurora" else "-" + key
    big.save(f"{outdir}/chrome-chrust{suffix}-256.png")

    sizes = [16, 32, 48, 64, 128, 256]
    small = {s: big.resize((s, s), Image.LANCZOS) for s in sizes}
    for s, im in small.items():
        im.save(f"{outdir}/chrome-chrust{suffix}-{s}.png")

    ico = f"{outdir}/chrome-chrust{suffix}.ico"
    big.save(ico, sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    print(f"{palette['name']}: {ico}")


if __name__ == "__main__":
    main()
