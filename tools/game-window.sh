#!/usr/bin/env bash
# Print the X window id of the largest viewable window owned by the game.
# Several windows share WM_CLASS ("petz 4.exe"): 1x1 helpers, an 80x80 hidden
# window, and the real surface. Size is the reliable discriminator.
set -euo pipefail
: "${DISPLAY:=:0.0}"; export DISPLAY
CLASS="${1:-petz 4.exe}"

best=""; best_area=0
for w in $(xdotool search --class "$CLASS" 2>/dev/null); do
    info=$(xwininfo -id "$w" 2>/dev/null) || continue
    case "$info" in *"Map State: IsViewable"*) ;; *) continue ;; esac
    ww=$(printf '%s\n' "$info" | awk '/Width:/{print $2}')
    wh=$(printf '%s\n' "$info" | awk '/Height:/{print $2}')
    [ -n "$ww" ] && [ -n "$wh" ] || continue
    area=$((ww * wh))
    if [ "$area" -gt "$best_area" ]; then best_area=$area; best=$w; fi
done

[ -n "$best" ] || { echo "no viewable game window" >&2; exit 1; }
echo "$best"
