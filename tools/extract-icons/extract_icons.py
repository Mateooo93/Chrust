"""Extract every RT_GROUP_ICON / RT_ICON from a Windows PE file.

Writes one .ico per icon group (all sizes the file carries), and a .png per
icon image that is already PNG-compressed inside the group (Vista and later
store 256x256 and friends that way).
"""
import struct
import sys
from pathlib import Path

RT_ICON = 3
RT_GROUP_ICON = 14


def read_pe(path: Path):
    data = path.read_bytes()
    if data[:2] != b"MZ":
        raise SystemExit("not a PE file")
    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe_off:pe_off + 4] != b"PE\0\0":
        raise SystemExit("no PE header")
    coff = pe_off + 4
    nsections = struct.unpack_from("<H", data, coff + 2)[0]
    opt_size = struct.unpack_from("<H", data, coff + 16)[0]
    opt = coff + 20
    magic = struct.unpack_from("<H", data, opt)[0]
    dirs = opt + (112 if magic == 0x20B else 96)
    res_rva, res_size = struct.unpack_from("<II", data, dirs + 2 * 8)
    sec = opt + opt_size
    sections = []
    for i in range(nsections):
        off = sec + i * 40
        vsize, vaddr, rawsize, rawptr = struct.unpack_from("<IIII", data, off + 8)
        sections.append((vaddr, max(vsize, rawsize), rawptr))
    return data, res_rva, res_size, sections


def to_offset(rva, sections):
    for vaddr, size, rawptr in sections:
        if vaddr <= rva < vaddr + size:
            return rawptr + (rva - vaddr)
    raise SystemExit(f"rva {rva:#x} is not in any section")


def entries(data, base, offset, sections):
    """Yields (name_or_id, is_directory, offset_of_entry_data)."""
    named, ids = struct.unpack_from("<HH", data, base + offset + 12)
    count = named + ids
    for i in range(count):
        off = base + offset + 16 + i * 8
        name, target = struct.unpack_from("<II", data, off)
        yield name, bool(target & 0x80000000), base + (target & 0x7FFFFFFF)


def collect(data, base, sections):
    """{group_id: [(width, height, colorcount, planes, bitcount, bytes)]}"""
    groups = {}
    for typ, _, type_off in entries(data, base, 0, sections):
        if typ != RT_GROUP_ICON:
            continue
        for name_id, _, name_off in entries(data, base, type_off - base, sections):
            for _, _, lang_off in entries(data, base, name_off - base, sections):
                rva, size = struct.unpack_from("<II", data, lang_off)
                start = to_offset(rva, sections)
                group = data[start:start + size]
                images = []
                _, _, count = struct.unpack_from("<HHH", group, 0)
                for i in range(count):
                    (w, h, colors, _res, planes, bits,
                     nbytes, nid) = struct.unpack_from("<BBBBHHIH", group, 6 + i * 14)
                    images.append((w, h, colors, planes, bits, nid, nbytes))
                groups[name_id] = images
    return groups


def icon_bytes(data, base, sections, wanted):
    """{icon_id: raw bytes} for every RT_ICON."""
    found = {}
    for typ, _, type_off in entries(data, base, 0, sections):
        if typ != RT_ICON:
            continue
        for name_id, _, name_off in entries(data, base, type_off - base, sections):
            if wanted and name_id not in wanted:
                continue
            for _, _, lang_off in entries(data, base, name_off - base, sections):
                rva, size = struct.unpack_from("<II", data, lang_off)
                start = to_offset(rva, sections)
                found[name_id] = data[start:start + size]
    return found


def write_ico(out: Path, images, blobs):
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    body = b""
    table = b""
    for w, h, colors, planes, bits, nid, nbytes in images:
        blob = blobs[nid]
        table += struct.pack("<BBBBHHII", w, h, colors, 0, planes, bits, len(blob), offset)
        offset += len(blob)
        body += blob
    out.write_bytes(header + table + body)


def main():
    source = Path(sys.argv[1])
    outdir = Path(sys.argv[2])
    outdir.mkdir(parents=True, exist_ok=True)
    data, res_rva, _res_size, sections = read_pe(source)
    base = to_offset(res_rva, sections)

    groups = collect(data, base, sections)
    all_blobs = icon_bytes(data, base, sections, set())
    print(f"{source.name}: {len(groups)} icon group(s), {len(all_blobs)} image(s)")

    for group_id, images in groups.items():
        out = outdir / f"icon_group{group_id}.ico"
        write_ico(out, images, all_blobs)
        sizes = ", ".join(f"{w or 256}x{h or 256}" for w, h, *_ in images)
        print(f"  {out.name}: {len(images)} sizes ({sizes})")
        for w, h, _c, _p, _b, nid, _n in images:
            blob = all_blobs[nid]
            if blob[:8] == b"\x89PNG\r\n\x1a\n":
                png = outdir / f"icon_group{group_id}_{w or 256}x{h or 256}.png"
                png.write_bytes(blob)
                print(f"    png: {png.name}")


if __name__ == "__main__":
    main()
