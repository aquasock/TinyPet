#!/usr/bin/env bash
# Capture the Petz window to a PNG.
#
# Captures the X window of the running game (or the Wine virtual desktop when
# launched with PETZ_DESKTOP). For the oracle we want the client area only, so
# the window is picked by title and captured by id rather than by screen region.
#
# Usage: capture.sh OUT.png [--frame N]   (--frame writes OUT.NNN.png)
# Environment: PETZ_WINDOW_TITLE (default: 'Catz 4|Dogz 4|Petz 4|petz')
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:?usage: capture.sh OUT.png}"
SHIFT_LABEL="${2:-}"
FRAME="${3:-}"

: "${DISPLAY:=:0.0}"; export DISPLAY

pat="${PETZ_WINDOW_CLASS:-petz 4.exe}"
wid=""
# Match on WM_CLASS, not the title: a browser tab called "Petz 4" would
# otherwise win, and capturing someone's browser is both wrong and useless.
# Several windows share the class (IME, hidden helpers), so pick the largest
# mapped one - that is the game surface.
best_area=0
for w in $(xdotool search --class "$pat" 2>/dev/null); do
    geo=$(xwininfo -id "$w" 2>/dev/null) || continue
    case "$geo" in *"IsViewable"*) ;; *) continue ;; esac
    case "$geo" in *"Map State: IsUnMapped"*) continue ;; esac
    ww=$(printf '%s\n' "$geo" | awk '/Width:/{print $2}')
    wh=$(printf '%s\n' "$geo" | awk '/Height:/{print $2}')
    [ -n "$ww" ] && [ -n "$wh" ] || continue
    area=$((ww * wh))
    if [ "$area" -gt "$best_area" ]; then best_area=$area; wid=$w; fi
done

if [ -z "$wid" ]; then
    echo "no game window found (class: $pat). Is it running?" >&2
    for w in $(xdotool search --class . 2>/dev/null); do
        echo "  $w class=$(xdotool getwindowclassname "$w" 2>/dev/null) name=$(xdotool getwindowname "$w" 2>/dev/null)" >&2
    done
    exit 1
fi

tmp="${OUT}.xwd"
xwd -id "$wid" -silent > "$tmp"
convert "$tmp" "png:$OUT"
rm -f "$tmp"

if [ -n "$SHIFT_LABEL" ] && [ -n "$FRAME" ]; then
    base="${OUT%.png}"
    mv "$OUT" "${base}.${FRAME}.png"
    OUT="${base}.${FRAME}.png"
fi

# Reject degenerate captures: an all-black frame means we grabbed the wrong
# window or the game had not drawn yet.
python3 "$ROOT/tools/framehash.py" "$OUT" --require-nonblack
echo "captured $OUT  window=$wid"
