# Evidence

Raw artefacts behind [docs/VERDICT.md](../docs/VERDICT.md). Small enough to
commit, so the verdict is checkable from a clone instead of taken on trust.

Verify everything here with:

```sh
python3 tools/verify-evidence.py
```

## ball/

| file | what it is |
|---|---|
| `ball-input.raw` | One captured ball region: 896x736 8bpp surface, bounding box, palette, the `CircleRenderBlock` bytes as seen at call entry, and the surface **before** and **after** the fill. |
| `tables.raw` | 32 KB read from the retail process at VA `0x63b458` — the static per-diameter row-width table. |
| `shim.log` | Shim log from the run that produced the capture. |

Layout of `ball-input.raw` (little endian):

```
uint32 width, height, bpp, stride, minx, miny, bboxw, bboxh, changed
256 * 4  palette (RGBQUAD)
512      CircleRenderBlock bytes at call entry
stride*height  surface before
stride*height  surface after
```

The table in `tables.raw` stores, for each diameter `r`, the `r` row widths of a
disc starting at offset `(r-1)*r/2`:

```
r=19: 7,11,13,15,15,17,17,19,19,19,19,19,17,17,15,15,13,11,7
```

## frames/

Four framebuffer dumps from a cold run, with their sha256 values. A second,
independent cold run produced byte-identical files at the same present indices;
those matching hashes are the determinism claim.

`HASHES.txt` records the digests. `tools/verify-evidence.py` re-computes them.

## What these do not show

The ball is a small flat disc (body index 18, edge index 247). Fur patterns,
shading ramps and texture mapping are **not** represented here, and
`docs/VERDICT.md` is provisional for that reason.
