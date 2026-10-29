#!/usr/bin/env bash
# Fetch `unshield` into build/tools without needing root.
#
# The retail install payload is an InstallShield 6 cabinet; cabextract and 7z
# both refuse it. unshield handles it, but we do not want to require a system
# install (and sudo is unavailable in many sandboxes), so we unpack the .deb
# into a private prefix.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/build/tools"
mkdir -p "$DEST/deb" "$DEST/root"

cd "$DEST/deb"
for pkg in unshield libunshield1; do
    if ! ls "${pkg}"_*.deb >/dev/null 2>&1; then
        echo "fetching $pkg"
        apt-get download "$pkg"
    fi
done

for deb in ./*.deb; do
    dpkg-deb -x "$deb" "$DEST/root"
done

BIN="$DEST/root/usr/bin/unshield"
[ -x "$BIN" ] || { echo "unshield missing from package" >&2; exit 1; }

# Confirm it can actually run (this is the only thing that matters).
if ! LD_LIBRARY_PATH="$DEST/root/usr/lib/x86_64-linux-gnu" "$BIN" -V >/dev/null 2>&1; then
    echo "unshield built but will not run" >&2
    exit 1
fi
echo "ok: $BIN"
echo "offline alternative: unpack any unshield .deb yourself into $DEST/root"
