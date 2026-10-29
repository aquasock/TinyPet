#!/usr/bin/env python3
"""Turn the shim's EIP histogram into a ranked hot-method list.

perf cannot be used here (perf_event_paranoid=4 and no sudo), so the shim samples
the main thread's instruction pointer from inside the process and writes a
histogram of 4-byte buckets covering 0x400000..0x600000.

This maps those buckets onto the executable's own export table, which is a free
symbol map: every method is exported by name.

Usage: profile-report.py [TOP] [--raw profile.bin]
"""
import collections
import os
import struct
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, "build", "install", "Petz 4.exe")
PROFILE = os.path.join(ROOT, "build", "install", "profile.bin")
BASE = 0x400000


def load_symbols():
    out = subprocess.run(
        [sys.executable, os.path.join(ROOT, "analysis", "pe_exports.py"), EXE],
        capture_output=True, text=True, check=True).stdout
    rows = []
    for line in out.splitlines():
        rva, _ordinal, name = line.split("\t")
        rows.append((int(rva, 16), name))
    rows.sort()
    return rows


def owner_of(rva, syms, starts):
    import bisect
    i = bisect.bisect_right(starts, rva) - 1
    if i < 0:
        return None
    return syms[i][1]


def main():
    top = 25
    path = PROFILE
    args = sys.argv[1:]
    if "--raw" in args:
        path = args[args.index("--raw") + 1]
    for a in args:
        if a.isdigit():
            top = int(a)

    data = open(path, "rb").read()
    slots = struct.unpack("<%dI" % (len(data) // 4), data)
    syms = load_symbols()
    starts = [r for r, _ in syms]

    counts = collections.Counter()
    total = 0
    outside = 0
    for i, n in enumerate(slots):
        if not n:
            continue
        total += n
        name = owner_of(i * 4, syms, starts)
        if name is None:
            outside += n
        else:
            counts[name] += n

    if not total:
        print("empty profile")
        return 1

    print(f"samples: {total}   mapped: {total - outside} ({100*(total-outside)/total:.1f}%)")
    print(f"distinct methods hit: {len(counts)}\n")
    print(f"{'share':>7} {'samples':>8}  method")
    for name, n in counts.most_common(top):
        print(f"{100*n/total:6.2f}% {n:8d}  {name[:96]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
