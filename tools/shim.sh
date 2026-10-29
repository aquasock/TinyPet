#!/usr/bin/env bash
# Enable or disable the instrumentation shim in the install tree.
#
# The shim is our proxy DLL sitting in place of UVocab.dll. It is only needed
# for capture/experiment runs, and it is known to misbehave on a real desktop
# (access violation), so keep it off for normal play.
#
# Usage: shim.sh on|off|status
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
INST="$ROOT/build/install"
SHIM="$INST/UVocab.dll"
REAL="$INST/UVocab_real.dll"
BAK="$ROOT/harness/UVocab_shim.dll"

case "${1:-status}" in
on)
    [ -f "$REAL" ] || { echo "no UVocab_real.dll - re-run extract-gamedata.sh" >&2; exit 1; }
    if [ -f "$SHIM" ] && ! cmp -s "$SHIM" "$REAL"; then cp -f "$SHIM" "$BAK"; fi
    [ -f "$BAK" ] || { echo "no saved shim build" >&2; exit 1; }
    cp -f "$BAK" "$SHIM"; echo "shim: ON"
    ;;
off)
    [ -f "$REAL" ] || { echo "no UVocab_real.dll - re-run extract-gamedata.sh" >&2; exit 1; }
    if [ -f "$SHIM" ] && ! cmp -s "$SHIM" "$REAL"; then cp -f "$SHIM" "$BAK"; fi
    cp -f "$REAL" "$SHIM"; echo "shim: OFF (retail DLL restored)"
    ;;
status)
    if cmp -s "$SHIM" "$REAL" 2>/dev/null; then echo "shim: OFF"; else echo "shim: ON"; fi
    ;;
*) echo "usage: shim.sh on|off|status" >&2; exit 2 ;;
esac
