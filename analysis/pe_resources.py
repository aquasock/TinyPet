#!/usr/bin/env python3
"""List or extract resources from a PE file.

Petz stores its art and data as ordinary PE resources with custom type strings
rather than numeric RT_* ids:

    BDT  ball data table      BMP  8bpp palettized DIB
    BHD  ball header          FLM  filmstrip (sprite frames)
    LNZ  "linez" brain        FLH  filmstrip header
    SCP  script               WAV  sound       TXT/LOG

Usage:
    pe_resources.py <pe> --list
    pe_resources.py <pe> --extract BMP --out DIR [--limit N]
    pe_resources.py <pe> --extract '*' --out DIR
"""
import argparse
import os
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


class Resources:
    def __init__(self, data):
        self.data = data
        e_lfanew, sections = read_sections(data)
        # Data directory 2 = resource table (index 0 is export, 1 is import).
        rsrc_rva = struct.unpack_from("<I", data, e_lfanew + 24 + 96 + 2 * 8)[0]
        for _name, vaddr, vsize, rawsize, rawoff in sections:
            if vaddr <= rsrc_rva < vaddr + max(vsize, rawsize):
                self.root = rawoff + (rsrc_rva - vaddr)
                break
        else:
            raise SystemExit("no resource directory")

    def entries(self, offset):
        data = self.data
        nnamed, nid = struct.unpack_from("<HH", data, offset + 12)
        out = []
        for i in range(nnamed + nid):
            name_field, sub = struct.unpack_from("<II", data, offset + 16 + 8 * i)
            if name_field & 0x80000000:
                so = self.root + (name_field & 0x7FFFFFFF)
                length = struct.unpack_from("<H", data, so)[0]
                label = data[so + 2:so + 2 + length * 2].decode("utf-16le")
            else:
                label = str(name_field)
            out.append((label, sub))
        return out

    def walk(self):
        """Yield (type, name, lang, bytes)."""
        for type_name, type_sub in self.entries(self.root):
            if not type_sub & 0x80000000:
                continue
            for res_name, name_sub in self.entries(self.root + (type_sub & 0x7FFFFFFF)):
                if not name_sub & 0x80000000:
                    continue
                for lang, lang_sub in self.entries(self.root + (name_sub & 0x7FFFFFFF)):
                    entry = self.root + (lang_sub & 0x7FFFFFFF)
                    data_rva, size = struct.unpack_from("<II", self.data, entry)
                    yield type_name, res_name, lang, self.slice(data_rva, size)

    def slice(self, rva, size):
        # Resources live in .rsrc; map linearly within the containing section.
        e_lfanew, sections = read_sections(self.data)
        for _name, vaddr, vsize, rawsize, rawoff in sections:
            if vaddr <= rva < vaddr + max(vsize, rawsize):
                off = rawoff + (rva - vaddr)
                return self.data[off:off + size]
        raise SystemExit(f"rva {rva:x} outside any section")


def sanitize(name):
    return "".join(c if c.isalnum() or c in "._- " else "_" for c in name).strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pe")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--extract", metavar="TYPE")
    ap.add_argument("--out")
    ap.add_argument("--limit", type=int, default=0)
    args = ap.parse_args()

    res = Resources(open(args.pe, "rb").read())

    if args.list:
        counts = {}
        for type_name, res_name, _lang, data in res.walk():
            counts.setdefault(type_name, []).append((res_name, len(data)))
        for type_name in sorted(counts):
            items = counts[type_name]
            total = sum(n for _r, n in items)
            print(f"{type_name:6s} {len(items):5d} entries  {total:9d} bytes")
            for res_name, size in items[:3]:
                print(f"          e.g. {res_name} ({size} bytes)")
        return

    if not args.extract or not args.out:
        raise SystemExit("need --extract TYPE --out DIR (or --list)")

    os.makedirs(args.out, exist_ok=True)
    n = 0
    for type_name, res_name, _lang, data in res.walk():
        if args.extract != "*" and type_name != args.extract:
            continue
        path = os.path.join(args.out, f"{sanitize(res_name)}.{type_name}")
        with open(path, "wb") as fh:
            fh.write(data)
        n += 1
        if args.limit and n >= args.limit:
            break
    print(f"{n} resources -> {args.out}", file=sys.stderr)


if __name__ == "__main__":
    main()
