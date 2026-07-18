#!/usr/bin/env python3
"""Verify ratex_ffi.dll PE export table (fails CI if P/Invoke entry points are missing)."""

from __future__ import annotations

import struct
import sys
from pathlib import Path

REQUIRED = [
    "ratex_parse_and_layout",
    "ratex_free_display_list",
    "ratex_get_last_error",
    "ratex_render_bitmap",
    "ratex_free_bitmap",
    "ratex_render_png",
    "ratex_free_bytes",
    "ratex_render_svg",
    "ratex_free_svg",
]


def rva_to_offset(data: bytes, rva: int, sections: list[tuple[int, int, int, int]]) -> int | None:
    for virt_size, virt_addr, raw_size, raw_ptr in sections:
        span = max(virt_size, raw_size)
        if virt_addr <= rva < virt_addr + span:
            return rva - virt_addr + raw_ptr
    return None


def read_exports(path: Path) -> list[str]:
    data = path.read_bytes()
    if data[:2] != b"MZ":
        raise ValueError(f"{path}: not a PE file")

    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe_off : pe_off + 4] != b"PE\0\0":
        raise ValueError(f"{path}: invalid PE signature")

    num_sections = struct.unpack_from("<H", data, pe_off + 6)[0]
    opt_size = struct.unpack_from("<H", data, pe_off + 20)[0]
    opt_hdr = pe_off + 24
    magic = struct.unpack_from("<H", data, opt_hdr)[0]
    if magic != 0x20B:
        raise ValueError(f"{path}: expected PE32+ (x64), got magic {magic:#x}")

    dd_off = opt_hdr + 112
    export_rva, export_size = struct.unpack_from("<II", data, dd_off)
    if export_rva == 0 or export_size == 0:
        raise ValueError(f"{path}: no export directory")

    sec_off = pe_off + 24 + opt_size
    sections: list[tuple[int, int, int, int]] = []
    for i in range(num_sections):
        o = sec_off + i * 40
        virt_size, virt_addr, raw_size, raw_ptr = struct.unpack_from("<IIII", data, o + 8)
        sections.append((virt_size, virt_addr, raw_size, raw_ptr))

    exp_off = rva_to_offset(data, export_rva, sections)
    if exp_off is None:
        raise ValueError(f"{path}: export directory RVA not mapped")

    # IMAGE_EXPORT_DIRECTORY layout (see PE-COFF spec)
    num_funcs = struct.unpack_from("<I", data, exp_off + 20)[0]
    num_names = struct.unpack_from("<I", data, exp_off + 24)[0]
    name_ptr_rva = struct.unpack_from("<I", data, exp_off + 32)[0]

    if num_names == 0 or num_names > 10_000:
        raise ValueError(
            f"{path}: invalid export name count (num_funcs={num_funcs}, num_names={num_names})"
        )

    names_off = rva_to_offset(data, name_ptr_rva, sections)
    if names_off is None:
        raise ValueError(f"{path}: export name table not mapped")

    names: list[str] = []
    for i in range(num_names):
        name_rva = struct.unpack_from("<I", data, names_off + i * 4)[0]
        no = rva_to_offset(data, name_rva, sections)
        if no is None:
            continue
        end = data.find(b"\0", no)
        if end == -1:
            continue
        names.append(data[no:end].decode("ascii", "replace"))
    return names


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <ratex_ffi.dll>", file=sys.stderr)
        return 2

    dll = Path(sys.argv[1])
    if not dll.is_file():
        print(f"error: file not found: {dll}", file=sys.stderr)
        return 1

    try:
        exports = read_exports(dll)
    except ValueError as exc:
        print(f"EXPORT CHECK FAILED: {exc}", file=sys.stderr)
        return 1

    export_set = set(exports)
    missing = [name for name in REQUIRED if name not in export_set]
    if missing:
        print("EXPORT CHECK FAILED: missing symbols:", ", ".join(missing), file=sys.stderr)
        print("Found ratex_* exports:", sorted(n for n in exports if n.startswith("ratex_")), file=sys.stderr)
        return 1

    print(f"OK: {dll} exports {len(exports)} symbols; all required ratex_* entry points present.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
