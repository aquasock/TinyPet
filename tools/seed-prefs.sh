#!/usr/bin/env bash
# Seed Petz 4 preferences into the Wine prefix.
#
# The game stores its settings as 1-byte REG_BINARY values under
# HKCU\Software\PF.Magic\Petz 4 and reads them once at startup. Pre-seeding the
# "Warn About ..." values removes the modal dialogs that would otherwise block
# an unattended capture run.
#
# Note: the game rewrites this key when it exits, so seeding is stable as long
# as the session itself does not re-enable the warnings.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
export WINEPREFIX="${WINEPREFIX:-$BUILD/wineprefix}"
export WINEDEBUG="${PETZ_WINEDEBUG:--all}"
export WINEDLLOVERRIDES="mscoree,mshtml="

REG="${1:-$ROOT/prefs/petz4-prefs.reg}"
[ -f "$REG" ] || { echo "no such .reg: $REG" >&2; exit 1; }

wine regedit /S "$REG"
echo "seeded $REG into $WINEPREFIX"

# Read back, so this script proves it did something rather than assuming.
echo "--- values now present ---"
wine reg query "HKCU\\Software\\PF.Magic\\Petz 4" 2>/dev/null || echo "(query returned nothing)"
