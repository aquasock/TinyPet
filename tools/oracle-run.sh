#!/usr/bin/env bash
# Full cold-start capture run: private display -> game -> dialogs -> burst.
#
# This is the reproducible unit the oracle compares. Everything is torn down and
# rebuilt so a run cannot inherit state from the previous one (except the Wine
# prefix, which the game writes its settings into).
#
# Usage: oracle-run.sh OUTDIR [COUNT] [INTERVAL_MS] [DISPLAY] [GEOMETRY]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
OUTDIR="${1:?usage: oracle-run.sh OUTDIR [COUNT] [INTERVAL] [DISPLAY] [GEOMETRY]}"
COUNT="${2:-8}"
INTERVAL="${3:-250}"
DISP="${4:-:97}"
GEOM="${5:-640x480x24}"

export WINEPREFIX="${WINEPREFIX:-$BUILD/wineprefix}"
export WINEDEBUG="${PETZ_WINEDEBUG:--all}"
export WINEDLLOVERRIDES="mscoree,mshtml="
export DISPLAY="$DISP"

# Silence the game. Xvfb is video only: audio still goes to the host, and an
# unattended capture run blasting pet sounds at the person using the machine is
# both rude and a source of nondeterminism (DirectSound timing).
wine reg add "HKCU\\Software\\Wine\\Drivers" /v Audio /t REG_SZ /d "" /f >/dev/null 2>&1 || true

# Ensure the game renders fullscreen at exactly the display size. Without this,
# Wine reserves 8x34px for decorations that no window manager is drawing, and
# the framebuffer is cropped.
wine reg add "HKCU\\Software\\Wine\\X11 Driver" /v Decorated /t REG_SZ /d N /f >/dev/null 2>&1 || true

echo "== teardown =="
pkill -x "Petz 4.exe" 2>/dev/null || true
sleep 1
/usr/lib/x86_64-linux-gnu/wine/wineserver -k 2>/dev/null || true
sleep 1
"$ROOT/tools/xvfb.sh" stop "$DISP" >/dev/null 2>&1 || true

echo "== display $DISP ($GEOM) =="
"$ROOT/tools/xvfb.sh" start "$DISP" "$GEOM" >/dev/null

echo "== launch =="
"$ROOT/tools/launch.sh" --detach >/dev/null

echo "== capture =="
"$ROOT/tools/capture-session.sh" "$OUTDIR" "$COUNT" "$INTERVAL"

echo "== done: $OUTDIR =="
