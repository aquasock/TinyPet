#!/usr/bin/env bash
# Write Petz 4 serial numbers into the Wine prefix.
#
# Why this is needed at all: the retail installer collects the serial and writes
# it here. We extract the discs directly instead of running InstallShield, so
# that step never happened and the game runs in demo mode (no toys, no food).
#
# Where it goes: HKCU\Software\PF.Magic\Petz 4, values "Serial Number Catz" and
# "Serial Number Dogz". Petz 4.exe::GetSerialNumber reads both into a 13-byte
# buffer, so the value is written as 13 raw bytes.
#
# Usage:
#   set-serial.sh                 # prompts, input hidden
#   set-serial.sh KEY             # non-interactive (key lands in shell history)
#   set-serial.sh --string KEY    # write REG_SZ instead, if binary is rejected
#   set-serial.sh --clear         # remove both values (back to demo)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export WINEPREFIX="${WINEPREFIX:-$ROOT/build/wineprefix}"
export WINEDEBUG="${PETZ_WINEDEBUG:--all}"
: "${DISPLAY:=:0.0}"; export DISPLAY

KEYVAL="HKCU\\Software\\PF.Magic\\Petz 4"
MODE=binary
CLEAR=0
SERIAL=""

while [ $# -gt 0 ]; do
    case "$1" in
    --string) MODE=string ;;
    --binary) MODE=binary ;;
    --clear) CLEAR=1 ;;
    *) SERIAL="$1" ;;
    esac
    shift
done

if [ "$CLEAR" = 1 ]; then
    for v in "Serial Number Catz" "Serial Number Dogz"; do
        wine reg delete "$KEYVAL" /v "$v" /f >/dev/null 2>&1 || true
    done
    echo "cleared; game will run in demo mode again"
    exit 0
fi

if [ -z "$SERIAL" ]; then
    printf 'Petz 4 serial: ' >&2
    read -rs SERIAL
    echo >&2
fi
[ -n "$SERIAL" ] || { echo "no serial given" >&2; exit 1; }

case "$MODE" in
binary)
    # Exactly 13 bytes: pad with NULs, truncate if longer.
    HEX=$(printf '%s' "$SERIAL" | head -c 13 | xxd -p | tr -d '\n')
    HEX=$(printf '%-26s' "$HEX" | tr ' ' '0')
    for v in "Serial Number Catz" "Serial Number Dogz"; do
        wine reg add "$KEYVAL" /v "$v" /t REG_BINARY /d "$HEX" /f >/dev/null
    done
    echo "wrote 13-byte REG_BINARY to both species"
    ;;
string)
    for v in "Serial Number Catz" "Serial Number Dogz"; do
        wine reg add "$KEYVAL" /v "$v" /t REG_SZ /d "$SERIAL" /f >/dev/null
    done
    echo "wrote REG_SZ to both species"
    ;;
esac

echo "--- read back ---"
wine reg query "$KEYVAL" 2>/dev/null | grep -i "serial" || echo "  (nothing found)"
