#!/usr/bin/env bash
# Retry cold starts until the game actually draws a pet.
#
# The rasterizer hook only fires when a pet is rendered, and whether that
# happens depends on state the prefix carries between launches. Rather than
# guess, this launches, clears dialogs, optionally nudges past a "click to
# continue" screen, and watches the shim log for fill activity. Retries a
# bounded number of times and leaves the successful game running.
#
# Usage: find-scene.sh [MAX_ATTEMPTS] [SECONDS_PER_ATTEMPT] [DISPLAY]
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
ATTEMPTS="${1:-4}"
WAIT="${2:-90}"
DISP="${3:-:95}"

export WINEPREFIX="${WINEPREFIX:-$BUILD/wineprefix}"
export WINEDEBUG="${PETZ_WINEDEBUG:--all}"
export WINEDLLOVERRIDES="mscoree,mshtml="
export DISPLAY="$DISP"
export PETZ_FILL_HOOK=1

"$ROOT/tools/xvfb.sh" start "$DISP" 640x480x24 >/dev/null 2>&1 || true

for attempt in $(seq 1 "$ATTEMPTS"); do
    echo "== attempt $attempt/$ATTEMPTS =="

    pkill -x "Petz 4.exe" 2>/dev/null || true
    sleep 1
    /usr/lib/x86_64-linux-gnu/wine/wineserver -k 2>/dev/null || true
    sleep 2
    rm -f "$BUILD/install/petzshim.log" "$BUILD/install/ball-input.raw"

    "$ROOT/tools/shim.sh" on >/dev/null 2>&1 || true
"$ROOT/tools/launch.sh" --detach >/dev/null
    sleep 18
    "$ROOT/tools/dismiss-dialogs.sh" >/dev/null 2>&1 || true
    sleep 3

    # Nudge past "press any key"/"click to continue" style screens. Containment:
    # this is a disposable prefix on a headless display.
    wid="$("$ROOT/tools/game-window.sh" 2>/dev/null || true)"
    if [ -n "$wid" ]; then
        xdotool windowfocus "$wid" 2>/dev/null || true
        xdotool mousemove --window "$wid" 320 240 2>/dev/null || true
        xdotool click 1 2>/dev/null || true
    fi

    shot="$BUILD/frames/attempt-$attempt.png"
    "$ROOT/tools/capture.sh" "$shot" >/dev/null 2>&1 || true
    echo "   state shot: $shot"

    for _ in $(seq 1 $((WAIT / 5))); do
        sleep 5
        fills=$(grep -c 'fill call' "$BUILD/install/petzshim.log" 2>/dev/null)
        fills=${fills:-0}
        if [ "$fills" -gt 0 ]; then
            echo "   scene reached: $fills fill-call log lines"
            if [ -f "$BUILD/install/ball-input.raw" ]; then
                echo "   ball capture present"
            fi
            exit 0
        fi
    done
    echo "   no fills in ${WAIT}s"
done

echo "no scene reached after $ATTEMPTS attempts"
exit 1
