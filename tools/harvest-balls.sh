#!/usr/bin/env bash
# Harvest several ball captures by keeping the pet active.
#
# Fills only happen when the pet is actually redrawn, and a static pet re-fills
# identically (the interval diff sees nothing). Nudging the scene with pointer
# input produces movement and therefore fresh fills.
#
# The prefix is restored from a snapshot first, so any mess the clicking makes
# is discarded.
#
# Usage: harvest-balls.sh [SECONDS] [DISPLAY]
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
DURATION="${1:-240}"
DISP="${2:-:95}"

export WINEPREFIX="${WINEPREFIX:-$BUILD/wineprefix}"
export WINEDEBUG="${PETZ_WINEDEBUG:--all}"
export WINEDLLOVERRIDES="mscoree,mshtml="
export DISPLAY="$DISP"
export PETZ_FILL_HOOK=1

"$ROOT/tools/prefix-snapshot.sh" restore snowball
"$ROOT/tools/shim.sh" on
"$ROOT/tools/xvfb.sh" start "$DISP" 640x480x24 >/dev/null 2>&1 || true

rm -f "$BUILD"/install/petzshim.log "$BUILD"/install/ball-input*.raw
"$ROOT/tools/launch.sh" --detach >/dev/null
sleep 20
"$ROOT/tools/dismiss-dialogs.sh" >/dev/null 2>&1 || true

wid="$("$ROOT/tools/game-window.sh" 2>/dev/null || true)"
echo "window: ${wid:-none}; harvesting for ${DURATION}s"

end=$((SECONDS + DURATION))
i=0
while [ "$SECONDS" -lt "$end" ]; do
    i=$((i + 1))
    if [ -n "${wid:-}" ]; then
        # wander the pointer around the scene to provoke movement
        x=$(( 120 + (i * 53) % 400 ))
        y=$(( 140 + (i * 97) % 280 ))
        xdotool mousemove --window "$wid" "$x" "$y" 2>/dev/null || true
        if [ $((i % 4)) -eq 0 ]; then xdotool click 1 2>/dev/null || true; fi
    fi
    sleep 3
done

n=$(ls "$BUILD"/install/ball-input*.raw 2>/dev/null | wc -l)
echo "captures: $n"
ls "$BUILD"/install/ball-input*.raw 2>/dev/null | xargs -n1 basename 2>/dev/null
