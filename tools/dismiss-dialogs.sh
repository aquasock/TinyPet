#!/usr/bin/env bash
# Clear startup dialogs that block the game from reaching its scene.
#
# Rule: any viewable window owned by the game whose _NET_WM_WINDOW_TYPE is
# _NET_WM_WINDOW_TYPE_DIALOG. A hardcoded title list is fragile - the first-run
# flow alone raises "Voice Recognition Error", "Welcome to Petz 4", and
# "Dogz 4 and Catz 4 are Great Gifts!" in that order, and which of them appear
# depends on how much state the Wine prefix already holds.
#
# Method: set X input focus to the dialog (works with no window manager), then
# send Return via XTEST. XSendEvent to the window is ignored by Wine's dialog
# loop, so focus + XTEST is what actually works.
#
# Usage: dismiss-dialogs.sh [MAX_PASSES]
set -uo pipefail
: "${DISPLAY:=:99}"; export DISPLAY

PASSES="${1:-8}"
dismissed=0

for pass in $(seq 1 "$PASSES"); do
    found=0
    for w in $(xdotool search --class 'petz 4.exe' 2>/dev/null); do
        info=$(xwininfo -id "$w" 2>/dev/null) || continue
        case "$info" in *"Map State: IsViewable"*) ;; *) continue ;; esac
        type=$(xprop -id "$w" _NET_WM_WINDOW_TYPE 2>/dev/null)
        case "$type" in *DIALOG*) ;; *) continue ;; esac
        ww=$(printf '%s\n' "$info" | awk '/Width:/{print $2}')
        wh=$(printf '%s\n' "$info" | awk '/Height:/{print $2}')
        # Skip degenerate helper windows that are technically dialogs.
        [ "${ww:-0}" -lt 50 ] && continue
        [ "${wh:-0}" -lt 50 ] && continue

        name=$(xprop -id "$w" WM_NAME 2>/dev/null | sed 's/WM_NAME(STRING) = //')
        xdotool windowfocus "$w" 2>/dev/null
        sleep 0.4
        xdotool key Return 2>/dev/null
        echo "dismissed: ${name:-<unnamed>} (${ww}x${wh})"
        dismissed=$((dismissed + 1))
        found=1
        sleep 0.8
    done
    [ "$found" = 0 ] && break
done

if [ "$dismissed" = 0 ]; then
    echo "no blocking dialogs found"
else
    echo "$dismissed dialog(s) dismissed"
fi
