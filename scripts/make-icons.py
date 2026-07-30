#!/usr/bin/env python3
"""Generate the application icons.

`tauri::generate_context!()` refuses to compile without `src-tauri/icons/icon.png`,
so an icon is a build input, not decoration — which is why this exists rather
than a binary of unrecorded origin sitting in the tree. The PNGs it writes ARE
committed: the build needs them, and a build that depends on someone having run
a script is not a build.

The mark is a placeholder and says so: a centre joined to three satellites, for
a name that means memory. What Mnema actually looks like belongs to the
packaging and delivery spec, along with the `.icns` and `.ico` a real bundle
needs and this does not produce.

Deterministic: same output every run, so re-running it never shows up as a diff.

Usage: python3 scripts/make-icons.py
"""

import math
import struct
import zlib
from pathlib import Path

# Deep slate ground, warm off-white mark. Chosen to stay legible at 32 px, where
# a two-tone shape is all that survives.
BACKGROUND = (24, 27, 38)
FOREGROUND = (236, 233, 225)

# Rendered at four times the target and averaged down: the only anti-aliasing
# available without an image library, and enough for discs and straight edges.
SUPERSAMPLE = 4

SIZES = {
    "icon.png": 512,
    "128x128@2x.png": 256,
    "128x128.png": 128,
    "32x32.png": 32,
}


def satellites(radius: float) -> list[tuple[float, float]]:
    """Three points on a circle, the first pointing straight up."""
    return [
        (
            radius * math.sin(2 * math.pi * i / 3),
            -radius * math.cos(2 * math.pi * i / 3),
        )
        for i in range(3)
    ]


def distance_to_segment(px, py, ax, ay, bx, by) -> float:
    dx, dy = bx - ax, by - ay
    length_squared = dx * dx + dy * dy
    if length_squared == 0:
        return math.hypot(px - ax, py - ay)
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / length_squared))
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy))


def is_mark(x: float, y: float, size: float) -> bool:
    """Is this point, in coordinates centred on the icon, part of the mark?"""
    orbit = size * 0.30
    hub = size * 0.115
    node = size * 0.072
    spoke = size * 0.022

    if math.hypot(x, y) <= hub:
        return True
    for sx, sy in satellites(orbit):
        if math.hypot(x - sx, y - sy) <= node:
            return True
        if distance_to_segment(x, y, 0.0, 0.0, sx, sy) <= spoke:
            return True
    return False


def is_tile(x: float, y: float, size: float) -> bool:
    """The rounded-square ground, as a superellipse rather than a square with
    four arcs bolted on — one expression, and no seams where they meet."""
    half = size * 0.46
    return abs(x / half) ** 4 + abs(y / half) ** 4 <= 1.0


def render(size: int) -> bytes:
    big = size * SUPERSAMPLE
    half = big / 2.0
    samples = SUPERSAMPLE * SUPERSAMPLE

    # Two coverages per output pixel: how much of it the ground covers, and how
    # much the mark. Both are needed — one becomes alpha, the other the blend
    # between the two colours.
    tile = [0] * (size * size)
    mark = [0] * (size * size)
    for by in range(big):
        y = by + 0.5 - half
        row = (by // SUPERSAMPLE) * size
        for bx in range(big):
            x = bx + 0.5 - half
            if not is_tile(x, y, float(big)):
                continue
            index = row + bx // SUPERSAMPLE
            tile[index] += 1
            if is_mark(x, y, float(big)):
                mark[index] += 1

    raw = bytearray()
    for y in range(size):
        raw.append(0)  # filter type 0: no prediction
        for x in range(size):
            index = y * size + x
            ground = tile[index] / samples
            # Relative to the ground, so the mark does not fade twice at the
            # rim where the ground is itself part transparent.
            ink = mark[index] / tile[index] if tile[index] else 0.0
            for channel in range(3):
                value = BACKGROUND[channel] * (1 - ink) + FOREGROUND[channel] * ink
                raw.append(int(round(value)))
            raw.append(int(round(255 * ground)))
    return bytes(raw)


def chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
    )


def png(size: int) -> bytes:
    # Colour type 6, RGBA. Not 2: `generate_context!` rejects a window icon that
    # is not RGBA outright — "icon ... is not RGBA" — so the alpha channel is a
    # build requirement, not a nicety.
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(render(size), 9))
        + chunk(b"IEND", b"")
    )


def main() -> None:
    out = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
    out.mkdir(parents=True, exist_ok=True)
    for name, size in SIZES.items():
        (out / name).write_bytes(png(size))
        print(f"wrote {out / name} ({size}x{size})")


if __name__ == "__main__":
    main()
