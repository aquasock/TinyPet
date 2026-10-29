#!/usr/bin/env python3
"""Dump the named export table of a PE file as TSV: rva<TAB>ordinal<TAB>symbol.

Petz 4.exe exports 5,719 named C++ methods. That table is effectively a
free architecture map of the engine: every class, every method, real names.

Usage:
    pe_exports.py <pe> [-o out.tsv] [--filter REGEX]
"""
import argparse
import re
import struct
import sys


def read_sections(data):
    e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
    nsec = struct.unpack_from("<H", data, e_lfanew + 6)[0]
    optsz = struct.unpack_from("<H", data, e_lfanew + 20)[0]
    base = e_lfanew + 24 + optsz
    sections = []
    for i in range(nsec):
        s = base + 40 * i
        name = data[s:s + 8].rstrip(b"\0").decode("latin1")
        vsize, vaddr, rawsize, rawoff = struct.unpack_from("<IIII", data, s + 8)
        sections.append((name, vaddr, vsize, rawsize, rawoff))
    return e_lfanew, sections


def make_rva_to_off(sections):
    def rva_to_off(rva):
        for _name, vaddr, vsize, rawsize, rawoff in sections:
            if vaddr <= rva < vaddr + max(vsize, rawsize):
                return rawoff + (rva - vaddr)
        return None
    return rva_to_off


def dump_exports(data):
    e_lfanew, sections = read_sections(data)
    rva_to_off = make_rva_to_off(sections)

    # Data directory 0 = export table.
    export_rva = struct.unpack_from("<I", data, e_lfanew + 24 + 96)[0]
    if not export_rva:
        return []
    off = rva_to_off(export_rva)

    # IMAGE_EXPORT_DIRECTORY
    (_name_rva, base, nfuncs, nnames, aof, anon, aoo) = struct.unpack_from(
        "<IIIIIII", data, off + 12
    )
    aof = rva_to_off(aof)
    anon = rva_to_off(anon)
    aoo = rva_to_off(aoo)

    rows = []
    for i in range(nnames):
        name_rva = struct.unpack_from("<I", data, anon + 4 * i)[0]
        no = rva_to_off(name_rva)
        end = data.index(b"\0", no)
        symbol = data[no:end].decode("latin1")
        ordinal_index = struct.unpack_from("<H", data, aoo + 2 * i)[0]
        rva = struct.unpack_from("<I", data, aof + 4 * ordinal_index)[0]
        rows.append((rva, base + ordinal_index, symbol))
    rows.sort()
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pe")
    ap.add_argument("-o", "--output")
    ap.add_argument("--filter", help="only symbols matching this regex")
    args = ap.parse_args()

    data = open(args.pe, "rb").read()
    rows = dump_exports(data)
    if args.filter:
        pat = re.compile(args.filter)
        rows = [r for r in rows if pat.search(r[2])]

    out = open(args.output, "w") if args.output else sys.stdout
    for rva, ordinal, symbol in rows:
        print(f"{rva:08x}\t{ordinal}\t{symbol}", file=out)
    if args.output:
        out.close()
        print(f"{len(rows)} exports -> {args.output}", file=sys.stderr)


if __name__ == "__main__":
    main()
