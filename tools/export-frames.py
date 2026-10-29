#!/usr/bin/env python3
"""Convert the shim's render-block ring into a frame-oriented animation file.

The ring (`blocks.bin`) is a flat stream of 72-byte records:

    bytes  0..63  CircleRenderBlock as seen at fill time
    bytes 64..67  drawport pointer, or the marker 'MARF' for a frame boundary
    bytes 68..71  sequence counter

Within the block, as decoded from the capture:

    +4   int16  palette index (colour)
    +32  int16  rect left      +36  rect top
    +40  int16  rect right     +44  rect bottom
    +52  int16  angle-ish      +56  int16  small parameter

Every ball is drawn twice (body and emboss), so consecutive records pair up.

Usage: export-frames.py [blocks.bin] [out.json]
"""
import json
import struct
import sys

SRC = sys.argv[1] if len(sys.argv) > 1 else "build/install/blocks.bin"
OUT = sys.argv[2] if len(sys.argv) > 2 else "build/animation.json"

raw = open(SRC, "rb").read()
n = len(raw) // 72


def field(i, off):
    return struct.unpack_from("<h", raw, i * 72 + off)[0]


def is_marker(i):
    return raw[i * 72 + 64:i * 72 + 68] == b"MARF"


frames = []
current = []
for i in range(n):
    if is_marker(i):
        if current:
            frames.append(current)
        current = []
        continue
    current.append({
        "x0": field(i, 32), "y0": field(i, 36),
        "x1": field(i, 40), "y1": field(i, 44),
        "colour": field(i, 4),
        "angle": field(i, 52),
        "param": field(i, 56),
    })
if current:
    frames.append(current)

# Drop the duplicate pass: keep the first of each identical consecutive pair.
def dedupe(fills):
    out = []
    for f in fills:
        if out and (f["x0"], f["y0"], f["x1"], f["y1"], f["colour"]) == (
                out[-1]["x0"], out[-1]["y0"], out[-1]["x1"], out[-1]["y1"], out[-1]["colour"]):
            continue
        out.append(f)
    return out

balls_per_frame = []
for f in frames:
    balls = dedupe(f)
    balls_per_frame.append([{
        "cx": (b["x0"] + b["x1"]) // 2,
        "cy": (b["y0"] + b["y1"]) // 2,
        "w": b["x1"] - b["x0"],
        "h": b["y1"] - b["y0"],
        "colour": b["colour"],
    } for b in balls])

sizes = [len(b) for b in balls_per_frame if b]
doc = {
    "source": SRC,
    "frames": balls_per_frame,
    "stats": {
        "records": n,
        "frames": len(balls_per_frame),
        "balls_per_frame_min": min(sizes) if sizes else 0,
        "balls_per_frame_max": max(sizes) if sizes else 0,
        "balls_per_frame_mean": round(sum(sizes) / len(sizes), 1) if sizes else 0,
    },
}
with open(OUT, "w") as fh:
    json.dump(doc, fh)
print(json.dumps(doc["stats"], indent=2))
print(f"wrote {OUT}")

# Compact binary for the Rust side (no JSON dependency there).
BIN = OUT.rsplit(".", 1)[0] + ".petz"
with open(BIN, "wb") as fh:
    fh.write(b"PZAN")
    fh.write(struct.pack("<II", 1, len(balls_per_frame)))
    for balls in balls_per_frame:
        fh.write(struct.pack("<I", len(balls)))
        for b in balls:
            fh.write(struct.pack("<hhHHB3x", b["cx"], b["cy"], b["w"], b["h"], b["colour"]))
print(f"wrote {BIN} ({BIN and __import__('os').path.getsize(BIN)} bytes)")
