#!/usr/bin/env bash
# Determinism run: cold-start the game with the shim loaded, let it settle, and
# collect the framebuffer dumps the shim wrote.
#
# The shim (harness/proxy.c, installed as UVocab.dll) pins the clock and dumps
# the game's 8bpp drawing surface every 1000 presents. Two runs that are truly
# reproducible produce identical files with identical present indices.
#
# Usage: pin-run.sh NAME [SETTLE_SECONDS] [DISPLAY]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
NAME="${1:?usage: pin-run.sh NAME [SETTLE_SECONDS] [DISPLAY]}"
SETTLE="${2:-60}"
DISP="${3:-:95}"

export WINEPREFIX="${WINEPREFIX:-$BUILD/wineprefix}"
export WINEDEBUG="${PETZ_WINEDEBUG:--all}"
export WINEDLLOVERRIDES="mscoree,mshtml="
export DISPLAY="$DISP"

OUT="$BUILD/frames/$NAME"
mkdir -p "$OUT"
rm -f "$OUT"/*.raw

pkill -x "Petz 4.exe" 2>/dev/null || true
sleep 1
/usr/lib/x86_64-linux-gnu/wine/wineserver -k 2>/dev/null || true
sleep 2

rm -f "$BUILD/install/petzshim.log" "$BUILD"/install/dib-f*.raw

"$ROOT/tools/xvfb.sh" start "$DISP" 640x480x24 >/dev/null 2>&1 || true
"$ROOT/tools/shim.sh" on >/dev/null 2>&1 || true
"$ROOT/tools/launch.sh" --detach >/dev/null

sleep 18
"$ROOT/tools/dismiss-dialogs.sh" >/dev/null 2>&1 || true
sleep "$SETTLE"

cp "$BUILD"/install/dib-f*.raw "$OUT"/ 2>/dev/null || true
echo "$NAME: $(ls "$OUT"/*.raw 2>/dev/null | wc -l) dumps"
ls "$OUT"/*.raw 2>/dev/null | xargs -n1 basename 2>/dev/null | sort | head -10
