#!/usr/bin/env python3
"""Turn a line-art PNG of a face marking into the raw asset the sandbox paints.

The sandbox has no image decoder and `petz-core` has no dependencies by design,
so the marking ships as a plain coverage grid: a 8-byte header (magic, width,
height) and then one byte per pixel, 0 for no ink and 255 for full ink. It is
compiled into the binary with `include_bytes!`, so there is no asset path to
find at runtime and no format to parse.

The image is cropped to its ink before conversion. A supplied drawing comes with
whatever padding the artist left, and the caller's size constant should mean the
size of the mark, not the size of the canvas it was drawn on.

`--trim-top PCT` blanks the top PCT percent of rows *without* shrinking the
canvas. Cropping would move the mark, because the renderer anchors the drawing's
fork to its top row: taking rows off the top and keeping the height means the
drawing stays where it is and simply loses the part that was clipping the nose.

Coverage is `ink = 1 - luminance / 170`, clamped: the source is black strokes on
white, and anything lighter than 170 is treated as paper. Coverage rather than a
1-bit mask because the rasterizer minifies it several times, and the draw code
wants a number to compare against a threshold, not a boolean it cannot refine.

Usage: make-marking.py in.png out.raw [--trim-top=PCT]
"""
import struct
import sys

from PIL import Image

# What counts as paper. Everything above this luminance contributes no ink.
PAPER = 170


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    trim_top = 0
    for a in sys.argv[1:]:
        if a.startswith("--trim-top="):
            trim_top = float(a.split("=", 1)[1])
    if len(args) < 2:
        sys.exit(__doc__.strip().splitlines()[-1].strip())

    src, out = args[0], args[1]
    img = Image.open(src).convert("RGB")
    w, h = img.size
    px = img.load()

    def ink_at(x, y):
        r, g, b = px[x, y]
        # Rec. 601 luma, integer where it matters.
        lum = (299 * r + 587 * g + 114 * b) // 1000
        return 0 if lum >= PAPER else (PAPER - lum) * 255 // PAPER

    # Crop to the ink, so the caller's span means the mark's own width.
    (x0, y0, x1, y1) = (w, h, -1, -1)
    for y in range(h):
        for x in range(w):
            if ink_at(x, y):
                x0, y0 = min(x0, x), min(y0, y)
                x1, y1 = max(x1, x), max(y1, y)
    if x1 < 0:
        sys.exit(f"{src}: no ink found - nothing to convert")
    w, h = x1 - x0 + 1, y1 - y0 + 1

    blank = int(round(h * trim_top / 100.0))
    cov = bytearray(w * h)
    inked = 0
    for y in range(h):
        for x in range(w):
            c = 0 if y < blank else ink_at(x + x0, y + y0)
            if c:
                cov[y * w + x] = c
                if c >= 128:
                    inked += 1

    with open(out, "wb") as f:
        f.write(b"MKNG")
        f.write(struct.pack("<HH", w, h))
        f.write(bytes(cov))

    note = f", top {blank} of {h} rows blanked" if blank else ""
    print(f"{src} -> {out}: {w}x{h}, {inked} inked pixels{note}, {len(cov) + 8} bytes")


if __name__ == "__main__":
    main()
