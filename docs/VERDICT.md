# Verdict: can a modern port reproduce Petz 4 ballz bit-exactly?

**Provisional GO**, on the evidence below. The renderer is table-driven integer
code, it is deterministic to the byte, and one ball's silhouette has been
reproduced exactly — 285 of 285 expected pixels accounted for, zero
unexplained.

"Provisional" is doing real work in that sentence. Read the Limits section
before quoting this to anyone.

## Why the answer is yes

**1. The pipeline is integer and table-driven.**
`XDrawPort::XFillRotatedCircle` reads per-row widths from a static table built
at startup, then fills spans with `rep stos`. No floating point in the pixel
loop; 1.16% of the whole executable is x87 and none of it is per-pixel.

**2. The ball's shape is a table lookup, not computed geometry.**
The static table at VA `0x63b458` stores, for each diameter `r`, the `r` row
widths of a disc, at offset `(r-1)*r/2`:

```
r=19: 7,11,13,15,15,17,17,19,19,19,19,19,17,17,15,15,13,11,7
```

A captured ball (index 18, at 250,276 in an 896x736 8bpp surface) has exactly
those widths, centred at x=9.

**3. The one apparent discrepancy is overdraw, not geometry.**
Six rows looked one pixel narrow. The missing column holds index 247 **before
and after** the capture interval — an edge pass drawn over the body. Treating
18 (body) and 247 (overdrawn edge) as accounted:

```
expected span pixels: 285
accounted:            285  (100.0%)
unexplained:            0
```

So a model of "look up row widths for diameter r, centre, fill" reproduces the
ball completely.

**4. Large balls are shaded by palette-ramp offset, not float colour.**
Captured from a live session (2176x1336 surface, pet at full size):

```
capture    bbox      distinct idx   dominant run
living-15  70x86     19             14,15,16,17,18,19 = 81% of pixels
living-18  77x121    51             46,47 + 81,82 + 15,16
```

Those consecutive indices are linear ramps in the palette:

```
12..19: (212,218,223) -> (182,190,195)   ~5 per index, each channel
45..48: (122,162,187) -> (105,144,168)
80..83: (1,41,168)    -> (1,38,155)
```

So the colour model is 8bpp with pre-baked ramps and shading is "base index
plus an offset". The output of the lighting stage is an **integer index**, which
is the difference between a port that can match and one that cannot.

**5. The renderer is deterministic end to end.**
Two cold runs from a restored prefix snapshot produced byte-identical output at
three levels: whole frames (`f0a18ba4`, `08a01b15`, `c680379a`, `53536de2`), and
a single captured ball (`2e2d1b47...`).

## Why this matters for the port

A reimplementation must **ship the table and do the lookup**. Anyone writing a
renderer that draws spheres, or ellipses, or anything mathematically
respectable, will produce something that looks better and is wrong — off by a
pixel on specific rows of specific sizes, on every ball, every frame. That is
the difference between "a Petz remake" and "Petz".

The good news: the actual work is small and boring. A table, an offset formula,
a centring rule, span fills, and an edge pass. No shaders, no floating point, no
GPU feature dependence.

## Limits — what this does and does not establish

- **Shape verified on one small ball; colour verified only in outline.** The
  285/285 silhouette result is a flat disc. Large balls are demonstrably
  ramp-shaded and textured, but the *offset function* that chooses a ramp index
  per pixel has not been derived. That function is where the remaining risk
  sits, and it is likely the x87 setup arithmetic noted in BINARY_NOTES.
- **Fur pattern mapping** (`XFurInfo`) and the texture path
  (`SetBallTextureInfo`) remain unexamined beyond observing that they exist.
- **Two balls per capture.** The isolation method diffs consecutive fills, and
  the interval held two overlapping balls. They separate cleanly by index, but
  no single isolated call has been captured.
- **Edge pass unidentified.** Index 247 is drawn over the body, but the code
  that emits it has not been located.
- **Headless only.** The instrumentation shim faults with `c0000005` on a real X
  desktop while the retail DLL is fine there. Capture runs on Xvfb.
- **Retail exe, not a port.** Everything here was measured against
  `Petz 4.exe` under Wine. No reimplementation has been written in a shipping
  language; the "reimplementation" is a model validated against captured pixels.

## What would turn this into a full verdict

1. Capture a single isolated fill (per-call hook rather than interval diffing).
2. Identify and reproduce the edge pass (index 247).
3. Capture a large ball with fur variation and repeat the exercise; that is
   where `XFurInfo` and the texture path live, and where the real risk sits.
4. Port the span fill and the table to the target language and diff against the
   golden corpus in CI, requiring zero differing pixels.

## Reproducing the evidence

```sh
./tools/bootstrap-tools.sh
./tools/extract-gamedata.sh
./tools/prefix-snapshot.sh restore snowball
./tools/shim.sh on
PETZ_FILL_HOOK=1 ./tools/launch.sh --detach     # on a headless display
python3 tools/analyze-ball.py build/install/ball-input-0.raw
```

Artefacts: `evidence/` (raw capture, decoded table, frame hashes — committed so
the claims above can be re-checked), `docs/BINARY_NOTES.md` (all binary findings
with addresses).

Re-check the verdict without running the game:

```sh
python3 tools/verify-evidence.py
```

It verifies the frame hashes and re-derives the 285/285 silhouette result from
the committed capture.
