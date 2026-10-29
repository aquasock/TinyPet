#!/usr/bin/env bash
# Save and restore everything that makes a run start from a known state.
#
# State lives in TWO places, which is easy to get wrong:
#   - the Wine prefix            settings (HKCU\Software\PF.Magic\Petz 4)
#   - the install tree           adopted pets: <install>/Adopted Petz/*.pet
#
# The second one is the important one: it is what decides whether the game has
# a pet to draw at all. Snapshotting only the prefix produces a run that looks
# identical on paper and then behaves differently.
#
# Usage:
#   prefix-snapshot.sh save NAME
#   prefix-snapshot.sh restore NAME
#   prefix-snapshot.sh list
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
PREFIX="${WINEPREFIX:-$BUILD/wineprefix}"
INSTALL="$BUILD/install"
SNAPDIR="$BUILD/snapshots"

mkdir -p "$SNAPDIR"

case "${1:-list}" in
save)
    NAME="${2:?usage: prefix-snapshot.sh save NAME}"
    OUT="$SNAPDIR/$NAME.tar.gz"
    echo "archiving:"
    echo "  $PREFIX"
    echo "  $INSTALL/Adopted Petz"
    tar czf "$OUT" -C "$BUILD" \
        --exclude='wineprefix/dosdevices' \
        --exclude='wineprefix/.update-timestamp' \
        wineprefix "install/Adopted Petz" 2>/dev/null
    ls -lh "$OUT" | awk '{print "  -> " $5, $9}'
    ;;
restore)
    NAME="${2:?usage: prefix-snapshot.sh restore NAME}"
    SRC="$SNAPDIR/$NAME.tar.gz"
    [ -f "$SRC" ] || { echo "no snapshot $SRC" >&2; exit 1; }
    pkill -x "Petz 4.exe" 2>/dev/null || true
    sleep 1
    /usr/lib/x86_64-linux-gnu/wine/wineserver -k 2>/dev/null || true
    sleep 1
    echo "restoring $SRC"
    rm -rf "$PREFIX" "$INSTALL/Adopted Petz"
    tar xzf "$SRC" -C "$BUILD"
    echo "  prefix + adopted pets restored"
    ;;
list)
    echo "snapshots in $SNAPDIR:"
    ls -lh "$SNAPDIR"/*.tar.gz 2>/dev/null | awk '{print "  " $5, $9}' || echo "  (none)"
    ;;
*)
    echo "usage: prefix-snapshot.sh save|restore|list [NAME]" >&2; exit 2 ;;
esac
