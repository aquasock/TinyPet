#!/usr/bin/env bash
# Manage a private Xvfb display for the game.
#
# Why: Petz 4 idles its animation loop when it loses activation. Sharing the
# desktop with a human means every alt-tab freezes the capture subject. A
# headless display has no window manager and no competing focus, so the game
# stays active, and captures come out with no title bar or decorations.
#
# Usage: xvfb.sh start|stop|status [DISPLAY] [GEOMETRY]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
XVFB="$ROOT/build/tools/root/usr/bin/Xvfb"
DISP="${2:-:99}"
GEOM="${3:-1024x768x24}"
RUN="$ROOT/build/xvfb"
PIDFILE="$RUN/xvfb${DISP#:}.pid"

mkdir -p "$RUN"

alive() {
    [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null
}

case "${1:-status}" in
start)
    [ -x "$XVFB" ] || { echo "Xvfb not bootstrapped; run tools/bootstrap-tools.sh" >&2; exit 1; }
    if alive; then echo "already running on $DISP (pid $(cat "$PIDFILE"))"; exit 0; fi
    setsid "$XVFB" "$DISP" -screen 0 "$GEOM" -nolisten tcp -noreset \
        >"$RUN/xvfb${DISP#:}.log" 2>&1 < /dev/null &
    echo $! > "$PIDFILE"
    for _ in $(seq 1 30); do
        DISPLAY="$DISP" xdpyinfo >/dev/null 2>&1 && break
        sleep 0.2
    done
    DISPLAY="$DISP" xdpyinfo >/dev/null 2>&1 || { echo "Xvfb failed to start" >&2; exit 1; }
    echo "Xvfb up on $DISP ($GEOM, pid $(cat "$PIDFILE"))"
    ;;
stop)
    if alive; then kill "$(cat "$PIDFILE")" && echo "stopped $DISP"; else echo "not running"; fi
    rm -f "$PIDFILE"
    ;;
status)
    if alive; then
        echo "running on $DISP (pid $(cat "$PIDFILE"))"
        DISPLAY="$DISP" xdpyinfo 2>/dev/null | grep -m1 dimensions || true
    else
        echo "not running"
    fi
    ;;
*)
    echo "usage: xvfb.sh start|stop|status [DISPLAY] [GEOMETRY]" >&2; exit 2 ;;
esac
