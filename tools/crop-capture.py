#!/usr/bin/env python3
"""Crop a shim capture to its bounding box, for committing as evidence.

The shim writes whole surfaces (5.8 MB at 2176x1336). For evidence we only need
the changed region, so this produces a compact file with the same header
semantics but stride == bbox width.

Usage: crop-capture.py IN.raw OUT.raw
"""
import struct
import sys

src, dst = sys.argv[1], sys.argv[2]
d = open(src, "rb").read()
w, h, bpp, stride, minx, miny, bw, bh, changed, present = struct.unpack_from(
    "<IIIIIIIIII", d, 0)
pal = d[40:40 + 1024]
block = d[40 + 1024:40 + 1024 + 512]
off = 40 + 1024 + 512
need = stride * h
before = d[off:off + need]
after = d[off + need:off + need * 2]

def crop(buf):
    return b"".join(buf[(miny + y) * stride + minx:(miny + y) * stride + minx + bw]
                    for y in range(bh))

cb, ca = crop(before), crop(after)
if len(cb) != bw * bh or len(ca) != bw * bh:
    raise SystemExit("crop size mismatch")

with open(dst, "wb") as fh:
    fh.write(struct.pack("<IIIIIIIIII", w, h, bpp, bw, minx, miny, bw, bh,
                         changed, present))
    fh.write(pal)
    fh.write(block)
    fh.write(cb)
    fh.write(ca)

print(f"{src} -> {dst}: {bw}x{bh} present={present} changed={changed}")
print(f"  {len(cb)*2 + 1024 + 512 + 40} bytes")
