#!/usr/bin/env bash
# One reproducible capture session: wait for the game surface, clear startup
# dialogs, capture a burst, write a manifest.
#
# Assumes a display is already up (tools/xvfb.sh) and the game is running on it
# (tools/launch.sh --detach). tools/oracle-run.sh chains the whole thing.
#
# Usage: capture-session.sh OUTDIR [COUNT] [INTERVAL_MS]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTDIR="${1:?usage: capture-session.sh OUTDIR [COUNT] [INTERVAL_MS]}"
COUNT="${2:-8}"
INTERVAL="${3:-250}"
: "${DISPLAY:=:99}"; export DISPLAY

# Wait for a game surface to exist at all.
for _ in $(seq 1 60); do
    wid="$("$ROOT/tools/game-window.sh" 2>/dev/null || true)"
    [ -n "$wid" ] && break
    sleep 1
done
[ -n "${wid:-}" ] || { echo "game window never appeared on $DISPLAY" >&2; exit 1; }

# Clear whatever first-run dialogs are standing between us and the scene.
"$ROOT/tools/dismiss-dialogs.sh" 2>&1 | sed 's/^/  /'

# The surface can be destroyed and recreated during startup transitions, so
# re-resolve it after the dialogs settle rather than trusting the earlier id.
sleep 2
wid="$("$ROOT/tools/game-window.sh" 2>/dev/null || true)"
[ -n "$wid" ] || { echo "game surface disappeared on $DISPLAY" >&2; exit 1; }

# Confirm the surface is the real thing: full-screen-size and drawing.
geo=$(xwininfo -id "$wid" 2>/dev/null | awk '/Width:/{w=$2} /Height:/{h=$2} END{print w"x"h}' || true)
echo "surface: $wid ${geo:-unknown}"

# Wait until something is actually drawn. Right after startup the game sits on
# a nearly-black surface while the intro plays, which would otherwise fill the
# corpus with identical empty frames.
echo "waiting for the scene to render..."
ready=0
for _ in $(seq 1 120); do
    "$ROOT/tools/capture.sh" "$OUTDIR/.probe.png" >/dev/null 2>&1 || { sleep 0.5; continue; }
    verdict=$(python3 - "$OUTDIR/.probe.png" <<'PY'
import sys
from PIL import Image
img = Image.open(sys.argv[1]).convert("RGB")
n = img.size[0] * img.size[1]
px = list(img.getdata())
nonblack = sum(1 for p in px if any(p))
colours = len(img.getcolors(1 << 22) or [])
print(f"{100.0*nonblack/n:.1f} {colours}")
PY
)
    pct=${verdict%% *}; cols=${verdict##* }
    if awk -v p="$pct" 'BEGIN{exit !(p>20)}' || [ "${cols:-0}" -gt 256 ]; then
        echo "  ready: ${pct}% non-black, $cols colours"
        ready=1
        break
    fi
    sleep 0.5
done
rm -f "$OUTDIR/.probe.png"
[ "$ready" = 1 ] || { echo "scene never rendered (${pct:-0}% non-black)" >&2; exit 1; }

"$ROOT/tools/capture-burst.sh" "$OUTDIR" "$COUNT" "$INTERVAL"
