#!/usr/bin/env bash
# Build the instrumentation shim (harness/UVocab_shim.dll).
#
# The shim is a 32-bit Windows DLL: a proxy for UVocab.dll that forwards its one
# imported symbol to the renamed retail DLL, and installs:
#   - pinned clock hooks (timeGetTime / GetTickCount / QueryPerformanceCounter)
#   - drawing surface discovery (CreateDIBSection) and palette capture
#   - an optional inline hook on XDrawPort::XFillRotatedCircle
#
# Requires a mingw-w64 i686 toolchain. Like unshield, it is bootstrapped into
# build/tools so no root or system install is needed.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
TOOLS="$BUILD/tools"
MINGW="$TOOLS/mingw"

bootstrap_mingw() {
    mkdir -p "$TOOLS/mingw-deb"
    cd "$TOOLS/mingw-deb"
    local pkgs
    pkgs=$(apt-cache depends --recurse --no-recommends --no-suggests \
             --no-conflicts --no-breaks --no-replaces --no-enhances \
             gcc-mingw-w64-i686-win32 2>/dev/null \
           | grep -E '^[a-z0-9][a-z0-9.+-]*$' | sort -u)
    # Only fetch what is missing.
    local need=()
    for p in $pkgs; do
        ls "${p}"_*.deb >/dev/null 2>&1 || need+=("$p")
    done
    if [ ${#need[@]} -gt 0 ]; then
        echo "fetching ${#need[@]} packages"
        apt-get download "${need[@]}"
    fi
    mkdir -p "$MINGW"
    for deb in ./*.deb; do dpkg-deb -x "$deb" "$MINGW/"; done
}

[ -x "$MINGW/usr/bin/i686-w64-mingw32-gcc-win32" ] || bootstrap_mingw

export PATH="$MINGW/usr/bin:$PATH"
OUT="$ROOT/harness/UVocab_shim.dll"

i686-w64-mingw32-gcc-win32 -O2 -shared -static-libgcc -Wall \
    -o "$OUT" \
    "$ROOT/harness/proxy.c" \
    "$ROOT/harness/fill_hook.S" \
    "$ROOT/harness/uvocab_proxy.def"

ls -l "$OUT"
echo "install with: tools/shim.sh on"
