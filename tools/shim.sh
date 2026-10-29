#!/usr/bin/env bash
# Enable or disable the instrumentation shim in the install tree.
#
# on   installs harness/UVocab_shim.dll (the build output from
#      tools/build-shim.sh) as build/install/UVocab.dll
# off  restores the retail UVocab.dll saved by extract-gamedata.sh
#
# History: an earlier version of this script copied from a "backup" slot, which
# meant rebuilds were never deployed and runs silently used a stale DLL. Always
# deploy from the build output; verify with `status`.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
INST="$ROOT/build/install"
SHIM="$INST/UVocab.dll"
REAL="$INST/UVocab_real.dll"
BUILT="$ROOT/harness/UVocab_shim.dll"

digest() { sha256sum "$1" 2>/dev/null | cut -c1-12; }

case "${1:-status}" in
on)
    [ -f "$BUILT" ] || { echo "no build at $BUILT - run tools/build-shim.sh" >&2; exit 1; }
    [ -f "$REAL" ]  || { echo "no UVocab_real.dll - re-run tools/extract-gamedata.sh" >&2; exit 1; }
    cp -f "$BUILT" "$SHIM"
    echo "shim: ON   installed $(digest "$SHIM") (build $(digest "$BUILT"))"
    ;;
off)
    [ -f "$REAL" ] || { echo "no UVocab_real.dll - re-run tools/extract-gamedata.sh" >&2; exit 1; }
    cp -f "$REAL" "$SHIM"
    echo "shim: OFF  retail $(digest "$SHIM")"
    ;;
status)
    if cmp -s "$SHIM" "$REAL" 2>/dev/null; then
        echo "shim: OFF"
    elif cmp -s "$SHIM" "$BUILT" 2>/dev/null; then
        echo "shim: ON   (installed == current build, $(digest "$SHIM"))"
    else
        echo "shim: ON   STALE (installed $(digest "$SHIM") != build $(digest "$BUILT"))"
        echo "      run tools/build-shim.sh then tools/shim.sh on"
    fi
    ;;
*) echo "usage: shim.sh on|off|status" >&2; exit 2 ;;
esac
