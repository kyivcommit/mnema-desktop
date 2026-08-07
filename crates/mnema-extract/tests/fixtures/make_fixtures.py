#!/usr/bin/env python3
"""Builds the PDF fixtures used by tests/pdfium_binding.rs and tests/pdf.rs.

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

# A *second* body, and its words share none of the first's. `text-stamp-text.pdf`
# asserts that the page which survives a skipped neighbour is page 3 and carries
# page 3's own words — a reader that emitted page 1 twice, or that renumbered the
# survivors 1..n, would satisfy a count and fail on these sentences.
BODY_TEXT_3 = "Schedule B lists forty pallets of dried barley, collected weekly"
BODY_TEXT_4 = "from the Ravella yard, each delivery note countersigned on arrival."

# A third body, for `unloadable-middle-page.pdf`: the page *after* the broken one
# has to be distinguishable from both of its neighbours, or "the reader stopped
# at the break" and "the reader carried on past it" look alike.
BODY_TEXT_5 = "Annex C records the arbitration venue and the notice period agreed,"
BODY_TEXT_6 = "together with the schedule of penalties for a late collection."

# Deliberately far below the threshold. A scanner footer or a Bates stamp is what
# a scanned page carries when it carries nothing else, and the point of the
# threshold is that such a page must not count as having a text layer.
STAMP_TEXT = "Page 2 of 2"

# The same stamp for a three-page scan, numbered so that no two pages of
# `all-scanned.pdf` draw identical bytes. A fixture whose pages are
# indistinguishable cannot tell "every page was skipped" from "one page was
# skipped three times".
SCAN_STAMPS = ["Page 1 of 3", "Page 2 of 3", "Page 3 of 3"]


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


def build(
    pages: list[list[str]], locked: bool = False, break_page: int | None = None
) -> bytes:
    """Assembles a PDF whose pages draw the given lines, in the given order.

    With `break_page`, that page's object is the literal `null` while `/Count`
    still announces every page. `FPDF_LoadPage` then has no dictionary to load
    for it and fails, which is the one failure a page can have that is neither
    "no text layer" nor a document pdfium refuses outright — and the one
    `pdfium-render`'s page iterator turns into the end of the document.

    With `locked`, the file also carries a standard-security-handler /Encrypt
    dictionary whose /U cannot be produced from any password: a reader trying to
    open it with the empty password computes a different key and stops. That is
    exactly what a password-protected PDF looks like to a reader without the
    password, which is the only thing the test needs — and it avoids
    implementing RC4 and the MD5 key ladder here, which would be a second
    unverified thing in a fixture builder.

    The streams are therefore *not* encrypted. Nothing reads them: authentication
    fails at load, before any object is decrypted.
    """
    n_pages = len(pages)
    # Object numbering: 1 catalog, 2 page tree, 3..(2+n) pages,
    # (3+n)..(2+2n) content streams, (3+2n) the font, and — when locked —
    # (4+2n) the encryption dictionary.
    first_page_obj = 3
    first_stream_obj = first_page_obj + n_pages
    font_obj = first_stream_obj + n_pages
    encrypt_obj = font_obj + 1

    kids = " ".join(f"{first_page_obj + i} 0 R" for i in range(n_pages))
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [%s] /Count %d >>" % (kids.encode("ascii"), n_pages),
    ]
    for i in range(n_pages):
        if i == break_page:
            # The object exists, is referenced by /Kids, is counted by /Count —
            # and is not a page.
            objs.append(b"null")
        else:
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
    if locked:
        # V1/R2 is 40-bit RC4, the oldest standard handler and the one every
        # reader implements — so a reader that stops here stopped because it
        # has no key, not because it does not know the scheme.
        objs.append(
            b"<< /Filter /Standard /V 1 /R 2 /O <%s> /U <%s> /P -1 >>"
            % (b"a1" * 32, b"b2" * 32)
        )

    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for i, body in enumerate(objs, start=1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % i + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    for off in offsets:
        out += b"%010d 00000 n \n" % off
    # /ID is required alongside /Encrypt: the standard handler derives its key
    # from the first ID string, so a file without one is not one a reader can
    # even attempt to unlock.
    extra = (
        b" /Encrypt %d 0 R /ID [<%s> <%s>]"
        % (encrypt_obj, b"0123456789abcdef" * 2, b"0123456789abcdef" * 2)
        if locked
        else b""
    )
    out += b"trailer\n<< /Size %d /Root 1 0 R%s >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objs) + 1,
        extra,
        xref,
    )
    return bytes(out)


def write(
    name: str,
    pages: list[list[str]],
    locked: bool = False,
    break_page: int | None = None,
) -> None:
    data = build(pages, locked=locked, break_page=break_page)
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
    # The skipped page is neither the first nor the last, which is what makes
    # `tests/pdf.rs` sensitive: on a two-page fixture "dropped page 2" and
    # "dropped the last page" and "kept only page 1" are the same observation.
    write(
        "text-stamp-text.pdf",
        [[BODY_TEXT, BODY_TEXT_2], [STAMP_TEXT], [BODY_TEXT_3, BODY_TEXT_4]],
    )
    write("all-scanned.pdf", [[stamp] for stamp in SCAN_STAMPS])
    # Byte for byte the body of one-page-text.pdf, plus a lock. The pages it
    # would have read are the ones the reader proves it can read elsewhere, so
    # the refusal cannot be blamed on the content.
    write("password-locked.pdf", [[BODY_TEXT, BODY_TEXT_2]], locked=True)
    # Three pages, the middle one unloadable. The page it breaks is the middle
    # one for the same reason `text-stamp-text.pdf` skips the middle one: with
    # the break at the end, "stopped early" and "read everything" produce the
    # same page list.
    write(
        "unloadable-middle-page.pdf",
        [[BODY_TEXT, BODY_TEXT_2], [BODY_TEXT_3, BODY_TEXT_4], [BODY_TEXT_5, BODY_TEXT_6]],
        break_page=1,
    )
    # A catalogue, a page tree with `/Count 0`, and nothing to read. Degenerate
    # and not damaged, which is why it needs a fixture of its own: it is the
    # third way to reach "this document produced no pages", and the other two
    # are a scan and a document whose pages would not load.
    write("no-pages.pdf", [])
    write_solid_png()
