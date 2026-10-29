#!/usr/bin/env bash
# Start a paired-capture session: the game records both the render blocks and,
# at every INTERVAL-th present, the scene surface it drew them into.
#
#   ./tools/pair-capture.sh [INTERVAL]
#
# Writes, in build/install/:
#   blocks.bin     the block ring (render blocks + MARF frame markers)
#   pf-N.raw       scene surface snapshots (header, palette, pixels)
#   pf-index.txt   "N <ring slot> <sprite> <w>x<h>" per snapshot
#
# Then: python3 tools/pairdiff.py
#
# Keep the interval modest: the scene surface is 1920x1080 8bpp, 2 MB a pop,
# and the shim caps a session at 600 snapshots (1.2 GB).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
INTERVAL="${1:-30}"

rm -f "$ROOT"/build/install/blocks.bin \
      "$ROOT"/build/install/pf-*.raw \
      "$ROOT"/build/install/pf-index.txt

export PETZ_TRACE=1 PETZ_FILL_HOOK=1 PETZ_BLOCKLOG=1
export PETZ_FRAMEDUMP="$INTERVAL"
exec "$ROOT/tools/launch.sh" "$@"
