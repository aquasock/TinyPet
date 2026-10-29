#!/usr/bin/env python3
"""Inspect a captured ball: the renderer's input block and the pixels it made.

Reads ball-input.raw written by the shim when a single XFillRotatedCircle call
has been isolated by consecutive-snapshot differencing.

Layout:
    uint32 width, height, bpp, stride, minx, miny, bboxw, bboxh, changed
    256*4 palette (RGBQUAD)
    512   CircleRenderBlock bytes as seen at call entry
    stride*height  surface before
    stride*height  surface after

Usage: analyze-ball.py [path]
"""
import struct
import sys

from PIL import Image

PATH = sys.argv[1] if len(sys.argv) > 1 else "build/install/ball-input.raw"
raw = open(PATH, "rb").read()

(w, h, bpp, stride, minx, miny, bw, bh, changed) = struct.unpack_from("<IIIIIIIII", raw, 0)
pal = raw[36:36 + 1024]
block = raw[36 + 1024:36 + 1024 + 512]
off = 36 + 1024 + 512
need = stride * h
before = raw[off:off + need]
after = raw[off + need:off + need * 2]

print(f"surface : {w}x{h} {bpp}bpp stride={stride}")
print(f"ball    : {bw}x{bh} at ({minx},{miny})  changed={changed}")
print(f"block   : first 32 bytes {block[:32].hex()}")

def crop(buf, x, y, cw, ch):
    rows = []
    for yy in range(y, y + ch):
        rows.append(buf[yy * stride + x: yy * stride + x + cw])
    return rows

b = crop(before, minx, miny, bw, bh)
a = crop(after, minx, miny, bw, bh)

# How many distinct indices appear inside the ball region after the fill?
vals = sorted({v for row in a for v in row})
print(f"\ndistinct indices inside bbox: {len(vals)}")
print(f"  values: {vals[:20]}{' ...' if len(vals) > 20 else ''}")

# Per-row analysis: is each row a run of one index (flat scanline fill), or
# does it vary (texture/fur in the same pass)?
multi = sum(1 for row in a if len(set(row)) > 1)
print(f"\nrows with more than one index: {multi}/{bh}")
for i, row in enumerate(a[:min(bh, 10)]):
    runs = []
    cur = row[0]; n = 0
    for v in row:
        if v == cur: n += 1
        else: runs.append((cur, n)); cur = v; n = 1
    runs.append((cur, n))
    print(f"  row {i:2d}: {len(runs):2d} runs  {runs[:6]}")

# ASCII map of the bbox using index values, so structure is visible in text.
symbols = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
index_of = {v: symbols[i % len(symbols)] for i, v in enumerate(vals)}
print("\nchanged-mask map ('#' = after differs from before, '.' = same):")
for yy in range(bh):
    line = "".join("#" if a[yy][x] != b[yy][x] else "." for x in range(bw))
    print("  " + line)

print("\nindex map (one symbol per distinct index):")
for yy in range(bh):
    print("  " + "".join(index_of[v] for v in a[yy]))

# Save crops for visual inspection.
def save(buf_rows, name):
    flat = b"".join(bytes(r) for r in buf_rows)
    img = Image.frombytes("P", (bw, bh), flat)
    img.putpalette([c for i in range(256) for c in (pal[i*4+2], pal[i*4+1], pal[i*4+0])])
    img.resize((bw * 8, bh * 8), Image.NEAREST).convert("RGB").save(name)
    print(f"wrote {name}")

save(a, "build/frames/ball-after.png")
save(b, "build/frames/ball-before.png")
