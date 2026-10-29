#!/usr/bin/env python3
"""Convert a shim framebuffer dump to PNG and report stats.

Dump format (little endian), written by harness/proxy.c:
    uint32 width, height, bpp, stride, present_index
    then stride*height bytes of pixel data

The 8bpp surface holds palette *indices*. Until the shim also dumps the game's
palette, this renders indices through a grey ramp — enough to see structure and
prove the frame is not blank, not enough to judge colour.

Usage: dib2png.py dump.raw [out.png]
"""
import struct
import sys
from PIL import Image


def main():
    path = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else path.rsplit(".", 1)[0] + ".png"

    data = open(path, "rb").read()
    w, h, bpp, stride, frame, has_pal = struct.unpack_from("<IIIIII", data, 0)
    off = 24
    pal = None
    if has_pal:
        pal = data[off:off + 1024]
        off += 1024
    pix = data[off:]
    if len(pix) < stride * h:
        raise SystemExit(f"short dump: have {len(pix)}, need {stride*h}")

    rows = [pix[y * stride:y * stride + w] for y in range(h)]
    flat = b"".join(rows)

    if bpp == 8:
        img = Image.frombytes("P", (w, h), flat)
        if pal:
            img.putpalette([c for i in range(256) for c in (pal[i*4+2], pal[i*4+1], pal[i*4+0])])
        else:
            img.putpalette([v for i in range(256) for v in (i, i, i)])
    elif bpp == 32:
        img = Image.frombytes("RGBA", (w, h), flat).convert("RGB")
    else:
        raise SystemExit(f"unsupported bpp {bpp}")

    img.save(out)

    rgb = img.convert("RGB")
    colours = rgb.getcolors(1 << 22) or []
    px = list(rgb.getdata())
    nonblack = sum(1 for p in px if any(p))
    print(f"{path}: {w}x{h} {bpp}bpp stride={stride} present={frame} palette={'yes' if pal else 'no'} -> {out}")
    print(f"  distinct colours {len(colours)}  non-black {nonblack}/{w*h} "
          f"({100.0*nonblack/(w*h):.1f}%)")


if __name__ == "__main__":
    main()
