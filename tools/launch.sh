#!/usr/bin/env bash
# Launch Petz 4 from the extracted tree, in a local Wine prefix.
#
# Environment:
#   PETZ_DESKTOP=640x480   run inside a fixed-size Wine virtual desktop.
#                          Recommended for capture: it gives a stable target
#                          and stops the WM from resizing the game window.
#   WINEPREFIX=...         override the prefix (default build/wineprefix)
#   PETZ_WINEDEBUG=...     override WINEDEBUG (default: silent)
#
# Usage: launch.sh [--detach]
#   --detach  run in a new session so the game outlives the calling shell
#             (a killed shell job otherwise takes the game down with it)
set -euo pipefail

DETACH=0
[ "${1:-}" = "--detach" ] && DETACH=1

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
INSTALL="${PETZ_INSTALL:-$BUILD/install}"

[ -f "$INSTALL/Petz 4.exe" ] || { echo "run tools/extract-gamedata.sh first" >&2; exit 1; }

export WINEPREFIX="${WINEPREFIX:-$BUILD/wineprefix}"
export WINEDEBUG="${PETZ_WINEDEBUG:--all}"
# No .NET/Gecko install prompts; the game needs neither.
export WINEDLLOVERRIDES="mscoree,mshtml="

mkdir -p "$BUILD/frames" "$WINEPREFIX"
cd "$INSTALL"

if [ -n "${PETZ_DESKTOP:-}" ]; then
    echo "launching in virtual desktop ${PETZ_DESKTOP}"
    set -- wine explorer "/desktop=petz,${PETZ_DESKTOP}" "Petz 4.exe"
else
    echo "launching in windowed mode"
    set -- wine "Petz 4.exe"
fi

if [ "$DETACH" = "1" ]; then
    LOG="$BUILD/frames/launch.log"
    setsid nohup "$@" >"$LOG" 2>&1 < /dev/null &
    sleep 1
    echo "detached, log: $LOG"
    echo "pid: $(pgrep -x 'Petz 4.exe' | head -1)"
    exit 0
fi

exec "$@"
