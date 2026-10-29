#!/usr/bin/env bash
# Capture a burst of frames from the running game and write a manifest.
#
# Interval is wall-clock sampling, not game time: it is good enough to build a
# corpus and see motion, but it does NOT prove determinism. Determinism needs
# the same simulation time across runs, which needs the clock pinned. Keep the
# distinction in mind before quoting these hashes as reproducible.
#
# Usage: capture-burst.sh OUTDIR [COUNT] [INTERVAL_MS]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTDIR="${1:?usage: capture-burst.sh OUTDIR [COUNT] [INTERVAL_MS]}"
COUNT="${2:-10}"
INTERVAL="${3:-200}"

mkdir -p "$OUTDIR"
rm -f "$OUTDIR"/frame-*.png

for i in $(seq -w 0 $((COUNT - 1))); do
    "$ROOT/tools/capture.sh" "$OUTDIR/frame-$i.png" >/dev/null 2>&1 || {
        echo "capture $i failed" >&2; exit 1; }
    sleep "$(awk -v ms="$INTERVAL" 'BEGIN{printf "%.3f", ms/1000}')"
done

python3 - "$OUTDIR" "$INTERVAL" <<'PY'
import hashlib, json, os, sys
from PIL import Image

outdir, interval = sys.argv[1], int(sys.argv[2])
frames = sorted(f for f in os.listdir(outdir) if f.startswith("frame-"))
manifest = {"interval_ms": interval, "count": len(frames), "frames": []}
prev = None
for name in frames:
    path = os.path.join(outdir, name)
    img = Image.open(path).convert("RGB")
    digest = hashlib.sha256(img.tobytes()).hexdigest()
    colours = img.getcolors(maxcolors=1 << 24) or []
    entry = {
        "file": name,
        "size": list(img.size),
        "sha256": digest,
        "colours": len(colours),
        "pixels": img.size[0] * img.size[1],
        "changed_vs_prev": None,
    }
    if prev is not None:
        a, b = prev.tobytes(), img.tobytes()
        entry["changed_vs_prev"] = sum(1 for i in range(0, len(a), 3) if a[i:i+3] != b[i:i+3])
    prev = img
    manifest["frames"].append(entry)

with open(os.path.join(outdir, "manifest.json"), "w") as fh:
    json.dump(manifest, fh, indent=2)

print(f"{len(frames)} frames in {outdir}")
print(f"{'file':14s} {'size':10s} {'colours':>7s} {'changed vs prev':>16s}  sha256")
for f in manifest["frames"]:
    ch = "-" if f["changed_vs_prev"] is None else f"{f['changed_vs_prev']}"
    print(f"{f['file']:14s} {f['size'][0]}x{f['size'][1]:<5d} {f['colours']:7d} {ch:>16s}  {f['sha256'][:16]}")
PY
