#!/usr/bin/env python3
"""Diff our fill model against the game's own pixels, frame by frame.

The shim (PETZ_FRAMEDUMP=N) snapshots every live 8bpp surface at present time
and records the block-ring position at that moment. Between two consecutive
snapshots we therefore know both the pixels and the exact list of render
blocks that produced them:

    snapshot[i]  --apply blocks (slot[i], slot[i+1])-->  snapshot[i+1]

If the model is right that reconstruction matches snapshot[i+1] exactly. Where
it does not, the difference points at the pass we mis-modelled.

Several surfaces are dumped per snapshot because geometry alone does not say
which one the pets draw into - the screen-sized one turns out to be static.
`--surface auto` picks the surface that changes most.

Usage:
    python3 tools/pairdiff.py [--dir build/install] [--limit 20]
                              [--surface auto|0xADDR] [-v]
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
    w, h, bpp, stride, snap, has_pal, bits = struct.unpack_from("<7I", blob, 0)
    p = 28
    pal = None
    if has_pal:
        pal = blob[p:p + 1024]
        p += 1024
    data = np.frombuffer(blob, dtype=np.uint8, count=stride * h, offset=p)
    return w, h, stride, data.reshape(h, stride)[:, :w].copy(), pal, bits


def read_index(dirname):
    """-> {(snapshot, bits): (slot, width, height, filename)}"""
    out = {}
    path = os.path.join(dirname, "pf-index.txt")
    if not os.path.exists(path):
        return out
    with open(path) as f:
        for line in f:
            bits = line.split()
            if len(bits) != 6:
                continue
            snap, slot, _surf, addr, dims, _lastdp = bits
            w, _, h = dims.partition("x")
            name = f"pf-{snap}-{_surf}.raw"
            if not os.path.exists(os.path.join(dirname, name)):
                continue
            out[(int(snap), int(addr, 16))] = (int(slot), int(w), int(h or 0), name)
    return out


def read_blocks(dirname):
    path = os.path.join(dirname, "blocks.bin")
    with open(path, "rb") as f:
        blob = f.read()
    n = len(blob) // RING_REC
    markers = sum(1 for i in range(n) if blob[i * RING_REC + 64:i * RING_REC + 68] == b"MARF")
    return blob, n, markers


def parse_block(rec):
    colour = struct.unpack_from("<h", rec, 4)[0]
    x0 = struct.unpack_from("<i", rec, 32)[0]
    y0 = struct.unpack_from("<i", rec, 36)[0]
    x1 = struct.unpack_from("<i", rec, 40)[0]
    y1 = struct.unpack_from("<i", rec, 44)[0]
    return {
        "colour": colour & 0xFF,
        "kind": rec[6],
        "x0": x0, "y0": y0, "x1": x1, "y1": y1,
        "cx": (x0 + x1) // 2,
        "cy": (y0 + y1) // 2,
        "w": x1 - x0,
        "param": struct.unpack_from("<h", rec, 56)[0],
    }


def motion_by_surface(index, dirname):
    """Total pixels changed between consecutive snapshots, per surface."""
    by_bits = {}
    for (snap, bits) in index:
        by_bits.setdefault(bits, []).append(snap)
    out = {}
    for bits, snaps in by_bits.items():
        snaps.sort()
        total = 0
        prev = None
        for s in snaps[:12]:
            p = os.path.join(dirname, index[(s, bits)][3])
            img = read_dump(p)[3]
            if prev is not None and prev.shape == img.shape:
                total += int((img != prev).sum())
            prev = img
        out[bits] = total
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", default=os.path.join(REPO, "build/install"))
    ap.add_argument("--limit", type=int, default=20)
    ap.add_argument("--surface", default="auto")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()

    table = load_table()
    dmax = max_diameter(table)
    index = read_index(args.dir)
    if not index:
        sys.exit(f"no pf-index.txt in {args.dir} - run tools/pair-capture.sh and play")

    blob, nrec, nmarkers = read_blocks(args.dir)
    snaps = sorted({s for (s, _) in index})
    print(f"{len(snaps)} snapshots, {len({b for (_, b) in index})} surfaces, "
          f"{nmarkers} frame markers, ring {nrec} records, table dmax {dmax}")

    motion = motion_by_surface(index, args.dir)
    for bits, m in sorted(motion.items(), key=lambda kv: -kv[1]):
        print(f"  surface {bits:#x}: {m} px of change across the first snapshots")

    if args.surface == "auto":
        bits = max(motion, key=motion.get)
        if motion[bits] == 0:
            sys.exit("every surface is static - the dump is not catching the scene")
    else:
        bits = int(args.surface, 16)
    print(f"using surface {bits:#x}")

    seq = [(index[(s, bits)][0], os.path.join(args.dir, index[(s, bits)][3]))
           for s in snaps if (s, bits) in index]
    seq.sort()

    pairs = clean = 0
    for i in range(len(seq) - 1):
        (slot_a, path_a), (slot_b, path_b) = seq[i], seq[i + 1]
        if slot_b <= slot_a:
            continue
        _, _, _, ia, _, _ = read_dump(path_a)
        _, _, _, ib, _, _ = read_dump(path_b)
        if ia.shape != ib.shape:
            continue
        if np.array_equal(ia, ib):
            continue

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
            applied.append(blk)

        diff = work != ib
        ndiff = int(diff.sum())
        pairs += 1
        if ndiff == 0:
            clean += 1
            if args.verbose:
                print(f"  snap {i}->{i+1}: exact ({len(applied)} passes)")
            continue
        ys, xs = np.nonzero(diff)
        pct = 100.0 * ndiff / diff.size
        print(f"  snap {i}->{i+1}: {ndiff} px ({pct:.3f}%) differ  "
              f"bbox x{xs.min()}..{xs.max()} y{ys.min()}..{ys.max()}  "
              f"{len(applied)} passes")
        if args.verbose:
            seen = set()
            for blk in applied:
                if not (xs.min() <= blk["x1"] and blk["x0"] <= xs.max()
                        and ys.min() <= blk["y1"] and blk["y0"] <= ys.max()):
                    continue
                key = (blk["kind"], blk["colour"])
                if key in seen:
                    continue
                seen.add(key)
                print(f"     kind={blk['kind']} col={blk['colour']:3d} "
                      f"({blk['x0']},{blk['y0']})-({blk['x1']},{blk['y1']}) "
                      f"w={blk['w']} p={blk['param']}")
        if pairs >= args.limit:
            break

    if pairs:
        print(f"\n{pairs} comparable frame pairs, {clean} reconstruct exactly "
              f"({100.0 * clean / pairs:.0f}%)")
    else:
        print("\nno comparable frame pairs")


if __name__ == "__main__":
    main()
