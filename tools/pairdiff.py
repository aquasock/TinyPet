#!/usr/bin/env python3
"""Diff our fill model against the game's own pixels, frame by frame.

The shim (PETZ_FRAMEDUMP=N) dumps the scene surface at frame markers and
records which ring slot each marker occupies. Between two consecutive dumps we
therefore know both the pixels and the exact list of render blocks that
produced them:

    dump[i]  --apply blocks (slot[i], slot[i+1])-->  dump[i+1]

If our model is right that reconstruction matches dump[i+1] exactly. Where it
does not, the difference points at the pass we mis-modelled - no guessing about
which of five circle fills does what.

Usage:
    python3 tools/pairdiff.py [--dir build/install] [--limit 20] [-v]
"""

import argparse
import os
import struct
import sys

import numpy as np

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RING_REC = 72


def load_table():
    with open(os.path.join(REPO, "evidence/ball/tables.raw"), "rb") as f:
        return f.read()


def offset(d):
    return 0 if d == 0 else (d - 1) * d // 2


def rows(table, d):
    if d == 0:
        return b""
    start = offset(d)
    end = min(start + d, len(table))
    return table[start:end] if start < end else b""


def max_diameter(table):
    d = 0
    while offset(d + 1) + (d + 1) <= len(table):
        d += 1
    return d


def fill_disc(img, table, cx, cy, d, colour):
    """Table-driven disc: row widths looked up, centred by integer rule."""
    rs = rows(table, d)
    top = cy - (d - 1) // 2
    h, w = img.shape
    for row, width in enumerate(rs):
        if width == 0:
            continue
        y = top + row
        if y < 0 or y >= h:
            continue
        left = cx - (width - 1) // 2
        x0, x1 = max(left, 0), min(left + width, w)
        if x1 > x0:
            img[y, x0:x1] = colour


def read_dump(path):
    with open(path, "rb") as f:
        blob = f.read()
    w, h, bpp, stride, idx, has_pal = struct.unpack_from("<6I", blob, 0)
    p = 24
    pal = None
    if has_pal:
        pal = blob[p:p + 1024]
        p += 1024
    data = np.frombuffer(blob, dtype=np.uint8, count=stride * h, offset=p)
    return w, h, stride, data.reshape(h, stride)[:, :w].copy(), pal


def read_index(dirname):
    """idx -> (ring slot, sprite ptr, width, height)"""
    out = {}
    path = os.path.join(dirname, "pf-index.txt")
    if not os.path.exists(path):
        return out
    with open(path) as f:
        for line in f:
            bits = line.split()
            if len(bits) != 4:
                continue
            idx, slot, sprite, dims = bits
            w, _, h = dims.partition("x")
            out[int(idx)] = (int(slot), int(sprite), int(w), int(h or 0))
    return out


def read_blocks(dirname):
    path = os.path.join(dirname, "blocks.bin")
    with open(path, "rb") as f:
        blob = f.read()
    n = len(blob) // RING_REC
    markers = {}
    for i in range(n):
        rec = blob[i * RING_REC:(i + 1) * RING_REC]
        if rec[64:68] == b"MARF":
            markers[i] = struct.unpack_from("<I", rec, 68)[0]
    return blob, n, markers


def parse_block(rec):
    colour = struct.unpack_from("<h", rec, 4)[0]
    kind = rec[6]
    x0 = struct.unpack_from("<i", rec, 32)[0]
    y0 = struct.unpack_from("<i", rec, 36)[0]
    x1 = struct.unpack_from("<i", rec, 40)[0]
    y1 = struct.unpack_from("<i", rec, 44)[0]
    param = struct.unpack_from("<h", rec, 56)[0]
    return {
        "colour": colour & 0xFF,
        "kind": kind,
        "x0": x0, "y0": y0, "x1": x1, "y1": y1,
        "cx": (x0 + x1) // 2,
        "cy": (y0 + y1) // 2,
        "w": x1 - x0,
        "h": y1 - y0,
        "param": param,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", default=os.path.join(REPO, "build/install"))
    ap.add_argument("--limit", type=int, default=20)
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()

    table = load_table()
    dmax = max_diameter(table)
    index = read_index(args.dir)
    if not index:
        sys.exit(f"no pf-index.txt in {args.dir} - run the game with PETZ_FRAMEDUMP=15")

    blob, nrec, markers = read_blocks(args.dir)
    order = sorted(k for k in index if os.path.exists(os.path.join(args.dir, f"pf-{k}.raw")))
    print(f"{len(order)} dumps, {len(markers)} frame markers, ring {nrec} records, "
          f"table max diameter {dmax}")

    pairs = 0
    clean = 0
    for i in range(len(order) - 1):
        a, b = order[i], order[i + 1]
        slot_a = index[a][0]
        slot_b = index[b][0]
        if slot_b <= slot_a:
            continue
        wa, ha, stra, ia, _ = read_dump(os.path.join(args.dir, f"pf-{a}.raw"))
        wb, hb, strb, ib, _ = read_dump(os.path.join(args.dir, f"pf-{b}.raw"))
        if (wa, ha) != (wb, hb):
            continue
        if np.array_equal(ia, ib):
            continue  # nothing was drawn between these markers

        work = ia.copy()
        applied = []
        for r in range(slot_a + 1, slot_b + 1):
            rec = blob[r * RING_REC:(r + 1) * RING_REC]
            if rec[64:68] == b"MARF":
                continue
            blk = parse_block(rec)
            if blk["w"] <= 0 or blk["w"] > dmax:
                continue
            fill_disc(work, table, blk["cx"], blk["cy"], blk["w"], blk["colour"])
            applied.append((r, blk))

        diff = work != ib
        ndiff = int(diff.sum())
        pairs += 1
        if ndiff == 0:
            clean += 1
            if args.verbose:
                print(f"  dumps {a}->{b}: exact  ({len(applied)} passes)")
            continue
        ys, xs = np.nonzero(diff)
        print(f"  dumps {a}->{b}: {ndiff} px differ  "
              f"bbox x{xs.min()}..{xs.max()} y{ys.min()}..{ys.max()}  "
              f"({len(applied)} passes)")
        if args.verbose:
            inside = [blk for _, blk in applied
                      if xs.min() <= blk["x1"] and blk["x0"] <= xs.max()
                      and ys.min() <= blk["y1"] and blk["y0"] <= ys.max()]
            for blk in inside[:12]:
                print(f"     kind={blk['kind']} col={blk['colour']:3d} "
                      f"({blk['x0']},{blk['y0']})-({blk['x1']},{blk['y1']}) "
                      f"w={blk['w']} p={blk['param']}")
        if pairs >= args.limit:
            break

    print(f"\n{pairs} comparable frame pairs, {clean} reconstruct exactly "
          f"({100.0 * clean / pairs:.0f}%)" if pairs else "\nno comparable frame pairs")


if __name__ == "__main__":
    main()
