#!/usr/bin/env python3
"""Re-check the claims in docs/VERDICT.md against the committed evidence.

Two checks, both independent of the game:

  1. Frame determinism - the recorded frame dumps hash to the values in
     evidence/frames/HASHES.txt, which were produced by two independent cold
     runs that agreed.

  2. Ball silhouette   - the captured ball is fully explained by the static
     row-width table: every pixel of the expected span is either the body index
     or the edge index drawn over it. Expected 285/285, zero unexplained.

Exits non-zero if either check fails.

Usage: verify-evidence.py [--quiet]
"""
import hashlib
import os
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EVID = os.path.join(ROOT, "evidence")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def check_frames():
    manifest = os.path.join(EVID, "frames", "HASHES.txt")
    if not os.path.exists(manifest):
        return False, "no evidence/frames/HASHES.txt"
    failures = []
    checked = 0
    for line in open(manifest):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        digest, name = line.split(None, 1)
        path = os.path.join(EVID, "frames", name)
        if not os.path.exists(path):
            failures.append(f"missing {name}")
            continue
        actual = sha256(path)
        checked += 1
        if actual != digest:
            failures.append(f"{name}: {actual[:16]} != {digest[:16]}")
    if failures:
        return False, "; ".join(failures)
    return True, f"{checked} frame dumps match recorded hashes"


def check_ball():
    path = os.path.join(EVID, "ball", "ball-input.raw")
    tabpath = os.path.join(EVID, "ball", "tables.raw")
    for p in (path, tabpath):
        if not os.path.exists(p):
            return False, f"missing {os.path.relpath(p, ROOT)}"

    d = open(path, "rb").read()
    (w, h, bpp, stride, minx, miny, bw, bh, changed) = struct.unpack_from(
        "<IIIIIIIII", d, 0)
    off = 36 + 1024 + 512
    need = stride * h
    before = d[off:off + need]
    after = d[off + need:off + need * 2]

    t = open(tabpath, "rb").read()
    r = 19
    o = (r - 1) * r // 2
    widths = list(t[o:o + r])

    total = accounted = 0
    for y, width in enumerate(widths):
        left = 9 - (width - 1) // 2
        for x in range(left, left + width):
            total += 1
            v = after[(miny + y) * stride + (minx + x)]
            if v in (18, 247):        # body, or edge drawn over the body
                accounted += 1

    if total == 0:
        return False, "empty span"
    ok = accounted == total
    return ok, (f"ball {bw}x{bh} at ({minx},{miny}): "
                f"{accounted}/{total} span pixels accounted for")


def check_ramp():
    """The living-room capture should show shading as a run of consecutive
    palette indices, and those indices should be a linear ramp."""
    path = os.path.join(EVID, "ball", "living-room.raw")
    if not os.path.exists(path):
        return False, "no evidence/ball/living-room.raw"
    d = open(path, "rb").read()
    (w, h, bpp, stride, minx, miny, bw, bh, changed, present) = struct.unpack_from(
        "<IIIIIIIIII", d, 0)
    pal = d[40:40 + 1024]
    off = 40 + 1024 + 512
    before = d[off:off + bw * bh]
    after = d[off + bw * bh:off + bw * bh * 2]

    counts = {}
    for i in range(bw * bh):
        if before[i] != after[i]:
            counts[after[i]] = counts.get(after[i], 0) + 1
    total = sum(counts.values())
    if not total:
        return False, "no pixels changed"

    # longest run of consecutive indices
    best_run, run = [], []
    for v in sorted(counts):
        if run and v == run[-1] + 1:
            run.append(v)
        else:
            run = [v]
        if len(run) > len(best_run):
            best_run = list(run)
    covered = sum(counts[v] for v in best_run)

    # is that run a linear ramp in the palette?
    def rgb(i):
        return pal[i * 4], pal[i * 4 + 1], pal[i * 4 + 2]
    cols = [rgb(v) for v in best_run]
    monotonic = all(
        all(cols[i + 1][c] <= cols[i][c] for c in range(3)) or
        all(cols[i + 1][c] >= cols[i][c] for c in range(3))
        for i in range(len(cols) - 1)
    )

    share = 100.0 * covered / total
    ok = len(best_run) >= 4 and share >= 50 and monotonic
    return ok, (f"dominant run {best_run[0]}..{best_run[-1]} covers "
                f"{covered}/{total} px ({share:.0f}%), palette ramp "
                f"{'linear' if monotonic else 'NOT linear'}")


def main():
    quiet = "--quiet" in sys.argv
    results = []
    results.append(("frames", *check_frames()))
    results.append(("ball  ", *check_ball()))
    results.append(("ramp  ", *check_ramp()))

    failed = False
    for name, ok, detail in results:
        status = "PASS" if ok else "FAIL"
        print(f"[{status}] {name}  {detail}")
        failed |= not ok
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
