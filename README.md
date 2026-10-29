# petz-oracle

Working repository for a **bit-exact** reimplementation of Petz 4 (Catz 4 / Dogz 4,
PF.Magic / The Learning Company, 1999).

The requirement is not "a game that feels like Petz". It is: *same pixels, same
behaviour*. This repo currently contains the machinery that makes that claim
testable, rather than a matter of opinion.

Nothing here redistributes game code or art. You supply your own discs.

## Status

Milestone **M0** — the frame oracle. No engine code yet, by design: the
rasterizer must be proven reproducible before any of the game is rewritten.

## The core idea

The original is a table-driven integer rasterizer writing palette indices. That
is provable, so we prove it:

1. Run the retail exe under Wine with pinned inputs.
2. Capture the framebuffer every frame.
3. Build a golden corpus of frame hashes.
4. Reimplement one ball. Diff pixel-for-pixel. Require **zero** differences.

If one ball can be reproduced exactly, the approach is sound and everything
after it is scale. If it cannot, we learn that while the blast radius is one
object.

## Layout

```
source/                 your ISOs (gitignored)
tools/
  bootstrap-tools.sh    fetch unshield into build/tools (no root needed)
  extract-gamedata.sh   ISO -> runnable install tree in build/install
  launch.sh             run the game under a local Wine prefix
  capture.sh            capture the game window to PNG
  framehash.py          hash / crop / diff captured frames
analysis/
  pe_exports.py         dump named exports (rva, ordinal, symbol)
  pe_resources.py       list or extract PE resources (BMP/FLM/BDT/...)
docs/
  BINARY_NOTES.md       reverse-engineering findings with addresses
build/                  everything generated (gitignored)
  tools/  iso/  install/  wineprefix/  frames/
```

## Running it

```sh
./tools/bootstrap-tools.sh      # once
./tools/extract-gamedata.sh     # once, ~1 min
./tools/launch.sh               # runs the game
./tools/capture.sh out.png      # capture the game window
```

Two discs are needed for a complete install: Catz 4 and Dogz 4 merge into one
tree (the second disc drops a `.pfm` marker so the installer merges rather than
replaces). `extract-gamedata.sh` handles that.

## Legal posture

Reimplementation from observation, with assets supplied by the user, is the
posture ScummVM has operated under for two decades. Shipping the ISOs, the
extracted art, or transliterated decompiler output is not. Read the original;
write fresh code; let the diff harness prove equivalence.

The name "Petz" is not available for the project. That is a branding problem to
solve later, not a reason to delay the engineering.
# TinyPet
