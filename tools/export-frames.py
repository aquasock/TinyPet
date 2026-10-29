#!/usr/bin/env python3
"""Export one pet's animation from the shim's render-block ring.

Record layout (72 bytes, written by harness/proxy.c):

    0..63   CircleRenderBlock as seen at fill time
    64..67  'MARF' for a frame marker, else the drawport pointer
    68..71  sprite pointer (markers only)

Fields decoded from CircleRenderBlock:

    +4   int16  palette index        +32/+36  rect top-left
    +6   u8     fill-pass kind       +40/+44  rect bottom-right
                (0 rotated, 1 normal, 2 emboss, 3 outline, 4 circleEx)
    +52  int16  angle-ish            +56  int16  small parameter

Passes: each ball is drawn by normal/rotated (body), emboss (a smaller offset
disc), and outline (a ring at the same rect). XFillCircleEx calls the others
internally, so its records duplicate them - harmless to replay.

Output `animation.petz`:

    'PZAN' u32 version u32 width u32 height u32 frame_count
    per frame: u32 ball_count, then per ball:
        i16 cx, i16 cy, u16 w, u16 h, u8 colour, u8 kind, u16 pad

Usage: export-frames.py [blocks.bin] [out.petz]
"""
import json
import math
import struct
import sys
import collections

SRC = sys.argv[1] if len(sys.argv) > 1 else "build/install/blocks.bin"
OUT = sys.argv[2] if len(sys.argv) > 2 else "build/animation.petz"

raw = open(SRC, "rb").read()
n = len(raw) // 72

def s16(i, off):
    return struct.unpack_from("<h", raw, i * 72 + off)[0]

def kind_of(i):
    return raw[i * 72 + 6]

def is_marker(i):
    return raw[i * 72 + 64:i * 72 + 68] == b"MARF"

def sprite_of(i):
    return struct.unpack_from("<I", raw, i * 72 + 68)[0]

# Split the stream into frames, tagged by which sprite drew them.
segments = []
cur = []
sprite = 0
for i in range(n):
    if is_marker(i):
        if cur:
            segments.append((sprite, cur))
        cur = []
        sprite = sprite_of(i)
    else:
        cur.append(i)
if cur:
    segments.append((sprite, cur))

by_sprite = collections.defaultdict(list)
for s, cells in segments:
    by_sprite[s].append(cells)

def centroid(cells):
    xs = [(s16(i, 32) + s16(i, 40)) // 2 for i in cells]
    ys = [(s16(i, 36) + s16(i, 44)) // 2 for i in cells]
    return (sum(xs) / len(xs), sum(ys) / len(ys)) if xs else None

def motion(segs):
    cs = [c for c in (centroid(s) for s in segs) if c]
    steps = sorted(math.hypot(b[0] - a[0], b[1] - a[1]) for a, b in zip(cs, cs[1:]))
    return steps[len(steps) // 2] if steps else 0.0

# The pet the player interacted with is the one that moves - but only consider
# sprites with enough frames for that estimate to mean anything. A 5-frame
# sprite can look "fast" from a single jump.
candidates = [(sp, sg) for sp, sg in by_sprite.items() if len(sg) >= 30] or list(by_sprite.items())
sprite, segs = max(candidates, key=lambda kv: motion(kv[1]))

frames = []
for cells in segs:
    out = []
    seen = set()
    for i in cells:
        key = (s16(i, 32), s16(i, 36), s16(i, 40), s16(i, 44), s16(i, 4), kind_of(i))
        if key in seen:          # exact duplicate draws (circleEx repeats them)
            continue
        seen.add(key)
        x0, y0, x1, y1 = key[0], key[1], key[2], key[3]
        out.append({
            "cx": (x0 + x1) // 2, "cy": (y0 + y1) // 2,
            "w": x1 - x0, "h": y1 - y0,
            "colour": key[4], "kind": key[5],
        })
    frames.append(out)

# Crop to the pet's own bounds across the whole clip: the game canvas is mostly
# empty and the pet would be a speck in it.
allb = [b for f in frames for b in f]
minx = min(b["cx"] - b["w"] // 2 for b in allb) if allb else 0
miny = min(b["cy"] - b["h"] // 2 for b in allb) if allb else 0
maxx = max(b["cx"] + b["w"] // 2 for b in allb) if allb else 640
maxy = max(b["cy"] + b["h"] // 2 for b in allb) if allb else 480
pad = 24
for f in frames:
    for b in f:
        b["cx"] -= minx - pad
        b["cy"] -= miny - pad
W = (maxx - minx + 2 * pad + 15) // 16 * 16
H = (maxy - miny + 2 * pad + 15) // 16 * 16

with open(OUT, "wb") as fh:
    fh.write(b"PZAN")
    fh.write(struct.pack("<IIII", 1, W, H, len(frames)))
    for balls in frames:
        fh.write(struct.pack("<I", len(balls)))
        for b in balls:
            fh.write(struct.pack("<hhHHBBH", b["cx"], b["cy"], b["w"], b["h"],
                                 b["colour"], b["kind"], 0))

kinds = collections.Counter(b["kind"] for f in frames for b in f)
print(f"sprite 0x{sprite:08x}: {len(frames)} frames, canvas {W}x{H}")
print(f"balls/frame: {min(len(f) for f in frames)}..{max(len(f) for f in frames)}")
print(f"pass mix: {dict(sorted(kinds.items()))}  (0 rot,1 normal,2 emboss,3 outline,4 circleEx)")
json.dump({"width": W, "height": H, "frames": frames}, open(OUT.rsplit('.', 1)[0] + ".json", "w"))
print(f"wrote {OUT}")
