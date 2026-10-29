#!/usr/bin/env python3
"""Hash and diff captured frames.

A frame is identified by its exact pixel bytes, not by how it looks. Two frames
from identical inputs must produce the same digest, or determinism is broken and
the oracle is not usable yet.

Usage:
    framehash.py shot.png                      size + digest + stats
    framehash.py shot.png --require-nonblack   fail if the frame is blank
    framehash.py a.png b.png                   diff two frames
"""
import argparse
import hashlib
import sys

from PIL import Image, ImageChops


def load(path):
    return Image.open(path).convert("RGB")


def digest(img):
    return hashlib.sha256(img.tobytes()).hexdigest()


def stats(img):
    colours = img.getcolors(maxcolors=1 << 24) or []
    nonblack = sum(n for n, c in colours if any(c))
    total = img.size[0] * img.size[1]
    return len(colours), nonblack, total


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("images", nargs="+")
    ap.add_argument("--require-nonblack", action="store_true")
    args = ap.parse_args()

    if len(args.images) == 2:
        a, b = (load(p) for p in args.images)
        if a.size != b.size:
            print(f"size differs: {a.size} vs {b.size}")
            return 2
        diff = ImageChops.difference(a, b)
        bbox = diff.getbbox()
        changed = sum(1 for p in diff.getdata() if any(p))
        total = a.size[0] * a.size[1]
        same = digest(a) == digest(b)
        print(f"A    {args.images[0]}  {digest(a)[:16]}")
        print(f"B    {args.images[1]}  {digest(b)[:16]}")
        print(f"identical: {same}")
        print(f"changed pixels: {changed}/{total} ({100.0 * changed / total:.4f}%)")
        if bbox:
            print(f"bounding box of differences: {bbox}")
        return 0 if same else 1

    img = load(args.images[0])
    colours, nonblack, total = stats(img)
    print(f"file     {args.images[0]}")
    print(f"size     {img.size[0]}x{img.size[1]}")
    print(f"sha256   {digest(img)}")
    print(f"colours  {colours}")
    print(f"nonblack {nonblack}/{total} ({100.0 * nonblack / total:.2f}%)")
    if args.require_nonblack and nonblack == 0:
        print("ERROR: frame is entirely black", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
