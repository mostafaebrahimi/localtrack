#!/usr/bin/env python3
"""Generate the LocalTrack icon set.

The mark is a clock face on a rounded tile: a ring with a filled sector for
elapsed time, plus hands at larger sizes. Everything is rasterized with 4x
supersampling so the edges stay clean without an image library.

Usage: python3 scripts/generate-icons.py
"""
from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BRAND = (0x2E, 0x7D, 0x5B)
BRAND_DARK = (0x24, 0x63, 0x48)
INK = (0xFF, 0xFF, 0xFF)
SS = 4  # supersampling factor


def _rounded_tile(x: float, y: float, size: float, radius: float) -> bool:
    dx = min(x, size - x)
    dy = min(y, size - y)
    if dx < radius and dy < radius:
        return (radius - dx) ** 2 + (radius - dy) ** 2 <= radius * radius
    return 0 <= x <= size and 0 <= y <= size


def _ring(dist: float, radius: float, stroke: float) -> bool:
    return abs(dist - radius) <= stroke / 2


def _hand(px: float, py: float, cx: float, cy: float, angle: float, length: float, width: float) -> bool:
    # Distance from the point to the segment from the centre outwards.
    ax, ay = cx, cy
    bx = cx + math.cos(angle) * length
    by = cy + math.sin(angle) * length
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    seg_len2 = vx * vx + vy * vy
    t = 0.0 if seg_len2 == 0 else max(0.0, min(1.0, (wx * vx + wy * vy) / seg_len2))
    dx = wx - vx * t
    dy = wy - vy * t
    return dx * dx + dy * dy <= (width / 2) ** 2


def render(size: int, background: tuple[int, int, int] | None = BRAND, hands: bool | None = None) -> bytes:
    """Return RGBA bytes for one icon."""
    if hands is None:
        hands = size >= 48

    radius = size * 0.23
    cx = cy = size / 2
    ring_radius = size * (0.30 if hands else 0.28)
    stroke = size * (0.075 if hands else 0.11)

    pixels = bytearray()
    for py in range(size):
        for px in range(size):
            r_acc = g_acc = b_acc = a_acc = 0.0
            for sy in range(SS):
                for sx in range(SS):
                    x = px + (sx + 0.5) / SS
                    y = py + (sy + 0.5) / SS
                    if background is not None and not _rounded_tile(x, y, size, radius):
                        continue

                    if background is None:
                        base = (0, 0, 0)
                        base_a = 0.0
                    else:
                        # Subtle vertical gradient keeps the tile from looking flat.
                        mix = y / size
                        base = tuple(
                            int(background[i] * (1 - mix) + BRAND_DARK[i] * mix) for i in range(3)
                        )
                        base_a = 1.0

                    colour, alpha = base, base_a
                    dist = math.hypot(x - cx, y - cy)

                    if _ring(dist, ring_radius, stroke):
                        colour, alpha = INK, 1.0
                    elif hands:
                        # Distinct hand lengths and weights so the mark reads as
                        # a clock rather than a tick.
                        if _hand(x, y, cx, cy, math.radians(-90), ring_radius * 0.72, size * 0.05):
                            colour, alpha = INK, 1.0
                        elif _hand(x, y, cx, cy, math.radians(0), ring_radius * 0.48, size * 0.07):
                            colour, alpha = INK, 1.0
                        elif dist <= size * 0.05:
                            colour, alpha = INK, 1.0
                    elif dist <= ring_radius - stroke and y < cy and x > cx:
                        # A filled quadrant stands in for the hands when small.
                        colour, alpha = INK, 1.0

                    r_acc += colour[0] * alpha
                    g_acc += colour[1] * alpha
                    b_acc += colour[2] * alpha
                    a_acc += alpha

            samples = SS * SS
            if a_acc == 0:
                pixels.extend((0, 0, 0, 0))
            else:
                pixels.extend(
                    (
                        int(r_acc / a_acc),
                        int(g_acc / a_acc),
                        int(b_acc / a_acc),
                        int(255 * a_acc / samples),
                    )
                )
    return bytes(pixels)


def write_png(path: Path, size: int, rgba: bytes) -> None:
    raw = b"".join(b"\x00" + rgba[y * size * 4 : (y + 1) * size * 4] for y in range(size))

    def chunk(tag: bytes, data: bytes) -> bytes:
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png)


def write_ico(path: Path, sizes: list[int]) -> None:
    images = []
    for size in sizes:
        rgba = render(size)
        raw = b"".join(b"\x00" + rgba[y * size * 4 : (y + 1) * size * 4] for y in range(size))

        def chunk(tag: bytes, data: bytes) -> bytes:
            body = tag + data
            return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

        png = (
            b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9))
            + chunk(b"IEND", b"")
        )
        images.append((size, png))

    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, blobs = b"", b""
    for size, png in images:
        entries += struct.pack(
            "<BBBBHHII", size if size < 256 else 0, size if size < 256 else 0, 0, 0, 1, 32, len(png), offset
        )
        offset += len(png)
        blobs += png
    path.write_bytes(header + entries + blobs)


def main() -> None:
    desktop = ROOT / "apps/desktop/src-tauri/icons"
    extension = ROOT / "apps/chrome-extension/public/icons"

    for size, name in [
        (32, "32x32.png"),
        (128, "128x128.png"),
        (256, "128x128@2x.png"),
        (512, "icon.png"),
    ]:
        write_png(desktop / name, size, render(size))
    write_ico(desktop / "icon.ico", [16, 32, 48, 256])

    for size in (16, 32, 128):
        write_png(extension / f"icon{size}.png", size, render(size))

    print("icons written to", desktop, "and", extension)


if __name__ == "__main__":
    main()
