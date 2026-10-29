#!/usr/bin/env bash
# Build a runnable Petz 4 install tree from the retail ISOs.
#
# Both discs are needed. Petz 4 shipped as two CDs (Catz 4, Dogz 4) which
# install into the SAME directory; the second installer drops a .pfm marker so
# it merges instead of replacing. That is why both are extracted into one tree
# here, Catz first.
#
# The component -> install-path mapping below is *reconstructed*, not read from
# the installer. It is verified by launching the game and observing that areas
# and content load. See docs/BINARY_NOTES.md.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
UNSHIELD="$BUILD/tools/root/usr/bin/unshield"
export LD_LIBRARY_PATH="$BUILD/tools/root/usr/lib/x86_64-linux-gnu"
INSTALL="$BUILD/install"
WORK="$BUILD/unpacked"

[ -x "$UNSHIELD" ] || { echo "run tools/bootstrap-tools.sh first" >&2; exit 1; }

merge_component() {
    local comp="$1" src="$2" dst="$3"
    if [ -d "$src" ]; then
        mkdir -p "$dst"
        cp -r "$src"/. "$dst"/
        return 0
    fi
    return 1
}

install_disc() {
    local name="$1"              # catz4 | dogs4
    local iso="$ROOT/source/$name.iso"
    [ -f "$iso" ] || { echo "missing $iso" >&2; exit 1; }

    local iso_dir="$BUILD/iso/$name"
    mkdir -p "$iso_dir"
    # Pull the installer cabinet plus the disc marker (.pfm).
    7z x -y -o"$iso_dir" "$iso" 'data1.cab' 'data1.hdr' '*.pfm' >/dev/null

    local unpacked="$WORK/$name"
    rm -rf "$unpacked"
    "$UNSHIELD" -D 1 x -d "$unpacked" "$iso_dir/data1.cab" >/dev/null
    echo "unpacked $name -> $unpacked"

    # Disc marker: lets the game/installer see both discs are present.
    for pfm in "$iso_dir"/*.pfm; do
        [ -e "$pfm" ] && cp "$pfm" "$INSTALL"/
    done

    merge_component exe      "$unpacked/Program_Executable" "$INSTALL"            || true
    merge_component progdll  "$unpacked/Program_DLLs"       "$INSTALL/Resource"   || true
    merge_component shared   "$unpacked/Shared_DLLs"        "$INSTALL/Resource"   || true
    merge_component clothes  "$unpacked/Clothez"            "$INSTALL/Resource"   || true
    merge_component toys     "$unpacked/Toyz"               "$INSTALL/Resource"   || true
    merge_component songz    "$unpacked/Songz"              "$INSTALL/Resource"   || true
    merge_component wallpaper "$unpacked/Wallpaper"         "$INSTALL/Resource"   || true
    merge_component editor   "$unpacked/Playscene_Editor"   "$INSTALL/Resource"   || true
    merge_component online   "$unpacked/Online"             "$INSTALL/Resource"   || true
    # Paths below are not guesses: they are hardcoded literals inside
    # Petz 4.exe (\Resource\Area\*.env, \Resource\Catz\*.cat,
    # \Resource\Dogz\*.dog, \Resource\Clothes\*.clo, \Resource\Toyz\*.toy,
    # \Resource\Wallpaper\*.bmp).
    merge_component areaz    "$unpacked/Areaz"              "$INSTALL/Resource"   || true
    merge_component webpack  "$unpacked/Web_Fun_Pack"       "$INSTALL"            || true
    merge_component screensaver "$unpacked/Screen_Saver"    "$INSTALL/Resource"   || true
    # Publisher and ViaFamily (speech) are not needed to play; keep them out of
    # the install tree but leave them unpacked for reference.
}

mkdir -p "$INSTALL"
install_disc catz4
install_disc dogs4

echo
echo "installed tree: $INSTALL"
echo "  Petz 4.exe        $( [ -f "$INSTALL/Petz 4.exe" ] && echo present || echo MISSING )"
echo "  breeds            $(find "$INSTALL/Resource" -iname '*.cat' -o -iname '*.dog' | wc -l)"
echo "  areas             $(find "$INSTALL/Resource/Area" -iname '*.env' 2>/dev/null | wc -l)"
echo "  clothes           $(find "$INSTALL/Resource/Clothes" -iname '*.clo' 2>/dev/null | wc -l)"
echo "  toys              $(find "$INSTALL/Resource/Toyz" -iname '*.toy' 2>/dev/null | wc -l)"
du -sh "$INSTALL"
