#!/usr/bin/env python3
"""Builds the PDF fixtures used by tests/pdfium_binding.rs.

Run from anywhere:  python3 crates/mnema-extract/tests/fixtures/make_fixtures.py

The PDFs are assembled by hand rather than produced by a writer library, for two
reasons. The fixtures have to be small enough to read in a diff, and their content
has to be invented outright — no real document is ever admitted as a test input.

Every string here is fabricated. The contract numbers, dates and names are not
references to anything.
"""

import pathlib
import zlib

HERE = pathlib.Path(__file__).parent

# Deliberately longer than TEXT_LAYER_MIN_CHARS (48 non-whitespace characters):
# this page has to read as a genuine text layer.
BODY_TEXT = "Invented contract 4417 between Northwind Depot and Ravella Freight,"
BODY_TEXT_2 = "signed 2026-07-25, covering pallet haulage for one calendar quarter."

# Deliberately far below the threshold. A scanner footer or a Bates stamp is what
# a scanned page carries when it carries nothing else, and the point of the
# threshold is that such a page must not count as having a text layer.
STAMP_TEXT = "Page 2 of 2"


def content_stream(lines: list[str]) -> bytes:
    """A minimal text-drawing content stream, one Tj per line."""
    out = ["BT", "/F1 12 Tf", "14 TL", "72 720 Td"]
    for line in lines:
        # No escaping logic on purpose: the fixture text is chosen to contain no
        # parentheses or backslashes, so a naive writer stays correct and obvious.
        assert "(" not in line and ")" not in line and "\\" not in line
        out.append(f"({line}) Tj")
        out.append("T*")
    out.append("ET")
    return "\n".join(out).encode("ascii")


def build(pages: list[list[str]]) -> bytes:
    """Assembles a PDF whose pages draw the given lines, in the given order."""
    n_pages = len(pages)
    # Object numbering: 1 catalog, 2 page tree, 3..(2+n) pages,
    # (3+n)..(2+2n) content streams, (3+2n) the font.
    first_page_obj = 3
    first_stream_obj = first_page_obj + n_pages
    font_obj = first_stream_obj + n_pages

    kids = " ".join(f"{first_page_obj + i} 0 R" for i in range(n_pages))
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [%s] /Count %d >>" % (kids.encode("ascii"), n_pages),
    ]
    for i in range(n_pages):
        objs.append(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>"
            % (font_obj, first_stream_obj + i)
        )
    for lines in pages:
        stream = zlib.compress(content_stream(lines))
        objs.append(
            b"<< /Length %d /Filter /FlateDecode >>\nstream\n" % len(stream)
            + stream
            + b"\nendstream"
        )
    objs.append(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")

    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for i, body in enumerate(objs, start=1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % i + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    for off in offsets:
        out += b"%010d 00000 n \n" % off
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objs) + 1,
        xref,
    )
    return bytes(out)


def write(name: str, pages: list[list[str]]) -> None:
    data = build(pages)
    (HERE / name).write_bytes(data)
    counts = [
        sum(1 for c in "".join(lines) if not c.isspace()) for lines in pages
    ]
    print(f"{name}: {len(data)} bytes, non-whitespace chars per page {counts}")


def write_solid_png() -> None:
    """An 8x8 solid-colour PNG: a genuine binary file, invented outright.

    Used by typing.rs to check that content decides what is text. Assembled by
    hand for the same reason the PDFs are — small enough to read in a diff, and
    reproducible on any machine with a Python interpreter.
    """
    import struct

    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    w = h = 8
    raw = b"".join(b"\x00" + bytes([40, 80, 160]) * w for _ in range(h))
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )
    (HERE / "solid.png").write_bytes(png)


if __name__ == "__main__":
    write("one-page-text.pdf", [[BODY_TEXT, BODY_TEXT_2]])
    # Page order is load-bearing here: the test asserts that the page carrying the
    # text arrives first and the stamp page second, which is what proves the probe
    # reports document order rather than whatever order pdfium happens to yield.
    write("text-then-stamp.pdf", [[BODY_TEXT, BODY_TEXT_2], [STAMP_TEXT]])
    write_solid_png()
