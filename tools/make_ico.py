#!/usr/bin/env python3
"""把 ime/build/icon/icon-*.png 打成 Windows ICO（32 位 BMP），不依赖第三方库。

payload/pangyi.ico 在 .gitignore 里，发布和本地打安装包前用这个脚本生成。
PNG 均为 8-bit RGBA、非隔行。ICO 用经典 DIB（自下而上 BGRA + AND 掩码），
llvm-mingw 的 windres / Windows 资源编译器都能读。
"""

from __future__ import annotations

import struct
import sys
import zlib
from pathlib import Path

PNG_SIG = b"\x89PNG\r\n\x1a\n"
SIZES = (16, 24, 32, 48, 64, 128, 256)


def _paeth(a: int, b: int, c: int) -> int:
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    if pb <= pc:
        return b
    return c


def decode_rgba(blob: bytes) -> tuple[int, int, bytes]:
    if blob[:8] != PNG_SIG:
        raise SystemExit("不是 PNG")
    pos = 8
    width = height = None
    idat = bytearray()
    while pos + 8 <= len(blob):
        length = struct.unpack(">I", blob[pos : pos + 4])[0]
        kind = blob[pos + 4 : pos + 8]
        data = blob[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            width, height, depth, color, comp, filt, inter = struct.unpack(">IIBBBBB", data)
            if (depth, color, comp, filt, inter) != (8, 6, 0, 0, 0):
                raise SystemExit(
                    f"只支持 8-bit RGBA 非隔行 PNG，实际 depth={depth} color={color} interlace={inter}"
                )
        elif kind == b"IDAT":
            idat += data
        elif kind == b"IEND":
            break
    if width is None or height is None:
        raise SystemExit("PNG 缺少 IHDR")
    raw = zlib.decompress(bytes(idat))
    stride = width * 4
    expect = height * (1 + stride)
    if len(raw) != expect:
        raise SystemExit(f"PNG 解压后长度 {len(raw)} != {expect}")
    rows: list[bytearray] = []
    prev = bytearray(stride)
    i = 0
    for _y in range(height):
        filt = raw[i]
        i += 1
        row = bytearray(raw[i : i + stride])
        i += stride
        if filt == 1:
            for x in range(stride):
                left = row[x - 4] if x >= 4 else 0
                row[x] = (row[x] + left) & 255
        elif filt == 2:
            for x in range(stride):
                row[x] = (row[x] + prev[x]) & 255
        elif filt == 3:
            for x in range(stride):
                left = row[x - 4] if x >= 4 else 0
                row[x] = (row[x] + ((left + prev[x]) // 2)) & 255
        elif filt == 4:
            for x in range(stride):
                a = row[x - 4] if x >= 4 else 0
                b = prev[x]
                c = prev[x - 4] if x >= 4 else 0
                row[x] = (row[x] + _paeth(a, b, c)) & 255
        elif filt != 0:
            raise SystemExit(f"不支持的 PNG 滤波器 {filt}")
        rows.append(row)
        prev = row
    return width, height, b"".join(rows)


def dib_32(width: int, height: int, rgba: bytes) -> bytes:
    """ICO 内嵌的 BITMAPINFOHEADER + XOR(BGRA, 自下而上) + AND 掩码。"""
    xor = bytearray()
    row_bytes = width * 4
    for y in range(height - 1, -1, -1):
        row = rgba[y * row_bytes : (y + 1) * row_bytes]
        for x in range(width):
            r, g, b, a = row[x * 4 : x * 4 + 4]
            xor += bytes((b, g, r, a))
    and_stride = ((width + 31) // 32) * 4
    and_mask = bytes(and_stride * height)
    image_size = len(xor) + len(and_mask)
    header = struct.pack(
        "<IIIHHIIIIII",
        40,
        width,
        height * 2,
        1,
        32,
        0,
        image_size,
        0,
        0,
        0,
        0,
    )
    return header + bytes(xor) + and_mask


def build_ico(icon_dir: Path) -> bytes:
    images: list[tuple[int, int, bytes]] = []
    for size in SIZES:
        path = icon_dir / f"icon-{size}.png"
        if not path.is_file():
            raise SystemExit(f"缺少 {path}")
        width, height, rgba = decode_rgba(path.read_bytes())
        if (width, height) != (size, size):
            raise SystemExit(f"{path.name} 实际是 {width}x{height}")
        images.append((width, height, dib_32(width, height, rgba)))
    count = len(images)
    header = struct.pack("<HHH", 0, 1, count)
    offset = 6 + 16 * count
    entries = bytearray()
    payload = bytearray()
    for width, height, dib in images:
        entries += struct.pack(
            "<BBBBHHII",
            0 if width >= 256 else width,
            0 if height >= 256 else height,
            0,
            0,
            1,
            32,
            len(dib),
            offset,
        )
        offset += len(dib)
        payload += dib
    return header + bytes(entries) + bytes(payload)


def main(argv: list[str]) -> None:
    if len(argv) != 3:
        raise SystemExit(f"用法: {argv[0]} <icon 目录> <输出.ico>")
    icon_dir = Path(argv[1])
    out = Path(argv[2])
    data = build_ico(icon_dir)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(data)
    print(f"wrote {out} ({len(data)} bytes, {len(SIZES)} images)")


if __name__ == "__main__":
    main(sys.argv)
