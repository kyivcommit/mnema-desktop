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

# `F2`, the font of `replacement-chars.pdf`: Helvetica with a `/ToUnicode` map
# that sends every code to U+FFFD. That is the shape of a real book whose prose
# font carries a map and the map is wrong, and pdfium answers it with U+FFFD for
# every glyph, spaces included — measured on a probe built exactly like this
# before the fixture was written, not assumed from the PDF specification.
GARBLED_CMAP = (
    b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n"
    b"/CMapName /Garbled def\n/CMapType 2 def\n"
    b"1 begincodespacerange\n<00> <FF>\nendcodespacerange\n"
    b"1 beginbfrange\n<00> <FF> [" + b" ".join([b"<FFFD>"] * 256) + b"]\nendbfrange\n"
    b"endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend"
)

# A line drawn in two fonts: a book's table of contents, whose leader dots are
# in a font without a usable map. The dots come back as a run of U+FFFD between
# two words that have to stay two words.
TOC_LINE = [("F1", "About the Authors"), ("F2", "...................."), ("F1", "xxi")]

# A page drawn wholly in `F2`. Comfortably above TEXT_LAYER_MIN_CHARS as drawn,
# so it is the replacement characters and not the length that decide the page.
GARBLED_LINES = [
    [("F2", "This whole page is drawn in a font with a broken map")],
    [("F2", "and so is this second line of it, word for word.")],
]

# `F2` of `broken-utf16.pdf`: pdfium's own answer to a font whose map sends one
# code to a lone UTF-16 surrogate. Identity for printable ASCII, except code
# 0x58, which becomes U+D800. **0x58 must lie outside every bfrange**: a range
# wins over a bfchar for the same code, and the probe that established this map
# read the page back as ordinary text until 0x58 was cut out of the range.
# Written as an array so every other code maps to itself — the probe's single
# destination `<0059>` turned every letter above 0x58 into `Y`, which a fixture
# asserting an exact line cannot use.
BROKEN_CMAP = (
    b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n"
    b"/CMapName /Broken def\n/CMapType 2 def\n"
    b"1 begincodespacerange\n<00> <FF>\nendcodespacerange\n"
    b"2 beginbfrange\n<20> <57> <0020>\n<59> <7E> ["
    + b" ".join(b"<%04X>" % code for code in range(0x59, 0x7F))
    + b"]\nendbfrange\n"
    b"1 beginbfchar\n<58> <D800>\nendbfchar\n"
    b"endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend"
)

# `broken-utf16.pdf`, page 2: a page that clears the threshold on its own words,
# with the word drawn in `F2` in the middle of a line — the shape of a real page
# where one glyph's map is wrong and the reader loses the whole page. `X` is the
# one code `BROKEN_CMAP` sends to a surrogate.
BROKEN_LINE = "A damaged mapping in the middle of this line"
BROKEN_SEGMENTS = [
    ("F1", "drops the word"),
    ("F2", "X"),
    ("F1", "from the index silently."),
]

# Page 3 of the same file: nothing but the broken code, well over the threshold,
# so the page is decided by the characters and not by the page being short.
BROKEN_ONLY = [("F2", "X" * 60)]

# Page 2 carries a CropBox as well, and a slug above it, because the two
# mechanisms meet on a real page: the box says what is visible, the broken glyph
# says the string pdfium built for that box cannot be decoded. A fallback that
# reads every character instead of the box's characters brings this slug back.
BROKEN_CROP_BOX = "[89 72 522 720]"

# …and its first line starts at x=85, four points left of that box, so `A` is a
# glyph crossing the box's edge. pdfium's bounded read keeps it (its test is an
# intersection, not a containment), and the per-character fallback has to agree
# or the page it rescues loses its first letter.
BROKEN_CROP_ORIGIN = 85

# `offset-cropbox.pdf`, page 1: the box measured on a library of books (D187) —
# width 531, height 666, starting at (130.5, 279). The reader's `all()` reads
# width×height from `(0,0)`, so a page whose text is drawn above y=666 comes
# back empty however much text it holds.
OFFSET_BOX = "[130.5 279 661.5 945]"

# Page 2 of the same file: its own origin, a CropBox moved off it — width 433,
# so a line ending at x≈478 is cut, which is the shape D187 measured on a
# book's line ends.
CROPPED_LINE = "Invented ledger entry 4417 lists every pallet hauled to the Ravella yard,"
CROPPED_LINE_2 = "and the depot clerk signs each page of it before the wagon leaves."

# Between the CropBox top (720) and the MediaBox top (792), and above the
# rectangle `all()` reads (0..648): visible to the file, invisible on the page.
# A press slug is exactly this (D187).
CROPPED_SLUG = "INVENTED PRESS SLUG 4417 2012-08-27"

# Page 3: the same moved MediaBox and no CropBox at all, its text above the
# `page_size` rectangle as page 1's is. A reader that falls back to
# `page_size()` rather than to the MediaBox loses this page the way it loses
# page 1 — and no existing fixture can tell the two boxes apart, because every
# one of them has a MediaBox at `[0 0 612 792]`.
UNBOXED_PAGE_TEXT = [BODY_TEXT_3, BODY_TEXT_4]

# `broken-utf16.pdf` page 2 also carries a line that runs past its CropBox's
# right edge (522), which is the shape D187 measured. pdfium puts the generated
# line break at the end of such a line — outside the box — so a fallback that
# drops everything outside the box fuses the cut line with the one after it
# (`…weekand the depot…`). The break has to be rebuilt.
CUT_LINE = "Invented ledger entry 4417 lists every pallet hauled to the Ravella yard gate that week,"

# `page-boxes.pdf`: the three ways a page's boxes are stated, and the one way
# they are not. Both boxes sit on the `/Pages` node, where a real book often
# keeps them (D187): a CropBox inherited that way is resolved by `pdfinfo`,
# while `pdfium-render`'s `crop()` and `media()` read only the page dictionary
# and answer `Err`. A reader that asks those two boxes loses this document.
TREE_BOX = "[100 200 700 900]"  # MediaBox on /Pages: 600 × 700
TREE_CROP = "[130.5 279 661.5 890]"  # CropBox on /Pages: 531 × 611

# Page 2's own CropBox is degenerate, the shape broken producers write. pdfium
# treats an empty box as absent and shows the MediaBox instead; a reader that
# takes the raw box finds no character in it and calls the page a scan.
DEGENERATE_CROP = "[0 0 0 0]"

# Page 3's own CropBox is wider than the MediaBox on every side, which mixed
# A4/Letter conversions produce. The viewer clips at the MediaBox, so text
# between the two boxes is invisible and must not be indexed.
WIDER_CROP = "[0 0 900 1000]"
OUTSIDE_MEDIA_SLUG = "INVENTED COVER SPREAD 4417"


def line_text(line: str | list[tuple[str, str]]) -> str:
    return line if isinstance(line, str) else "".join(text for _, text in line)


def uses_garbled(pages: list[list]) -> bool:
    return any(
        font == "F2" for lines in pages for line in lines if not isinstance(line, str)
        for font, _ in line
    )


def content_stream(lines: list, x: int = 72, y: int = 720, size: int = 12) -> bytes:
    """A minimal text-drawing content stream, one Tj per line.

    A line is a string drawn in `F1`, or a list of `(font, text)` segments; a
    segmented line returns to `F1` after itself, so plain lines after it are
    drawn as they always were.

    The origin and size default to the ones every fixture before
    `offset-cropbox.pdf` was drawn at, so that their bytes do not move.
    """
    out = ["BT", f"/F1 {size} Tf", f"{size + 2} TL", f"{x} {y} Td"]
    for line in lines:
        # No escaping logic on purpose: the fixture text is chosen to contain no
        # parentheses or backslashes, so a naive writer stays correct and obvious.
        text = line_text(line)
        assert "(" not in text and ")" not in text and "\\" not in text
        if isinstance(line, str):
            out.append(f"({line}) Tj")
        else:
            for font, segment in line:
                out.append(f"/{font} {size} Tf ({segment}) Tj")
            out.append(f"/F1 {size} Tf")
        out.append("T*")
    out.append("ET")
    return "\n".join(out).encode("ascii")


def build(
    pages: list[list[str]],
    locked: bool = False,
    break_page: int | None = None,
    cmap: bytes = GARBLED_CMAP,
    boxes: dict[int, tuple[str | None, str | None]] | None = None,
    layout: dict[int, list[tuple[tuple[int, int, int], list]]] | None = None,
    pages_boxes: tuple[str | None, str | None] | None = None,
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
    # (3+n)..(2+2n) content streams, (3+2n) the font, then — only when a page
    # draws in `F2` — that font and its map, and — when locked — the
    # encryption dictionary. A file that never draws in `F2` gets neither, so
    # every fixture written before `F2` existed is still the same bytes.
    first_page_obj = 3
    first_stream_obj = first_page_obj + n_pages
    font_obj = first_stream_obj + n_pages
    garbled = uses_garbled(pages)
    garbled_font_obj = font_obj + 1
    cmap_obj = font_obj + 2
    encrypt_obj = font_obj + (3 if garbled else 1)
    fonts = b"/F1 %d 0 R" % font_obj + (b" /F2 %d 0 R" % garbled_font_obj if garbled else b"")

    kids = " ".join(f"{first_page_obj + i} 0 R" for i in range(n_pages))
    tree_boxes = b""
    if pages_boxes is not None:
        media, crop = pages_boxes
        tree_boxes = (b" /MediaBox %s" % media.encode("ascii") if media else b"") + (
            b" /CropBox %s" % crop.encode("ascii") if crop else b""
        )
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [%s] /Count %d%s >>"
        % (kids.encode("ascii"), n_pages, tree_boxes),
    ]
    for i in range(n_pages):
        if i == break_page:
            # The object exists, is referenced by /Kids, is counted by /Count —
            # and is not a page.
            objs.append(b"null")
        else:
            # `boxes` names a page's MediaBox and, when it has one, its CropBox;
            # the default is the one every fixture before `offset-cropbox.pdf`
            # was written with. With `pages_boxes`, a page that names no box of
            # its own inherits both from the tree.
            if i in (boxes or {}):
                media, crop = boxes[i]
            elif pages_boxes is not None:
                media, crop = (None, None)
            else:
                media, crop = ("[0 0 612 792]", None)
            page_boxes = (b" /MediaBox %s" % media.encode("ascii") if media else b"") + (
                b" /CropBox %s" % crop.encode("ascii") if crop else b""
            )
            objs.append(
                b"<< /Type /Page /Parent 2 0 R%s "
                b"/Resources << /Font << %s >> >> /Contents %d 0 R >>"
                % (page_boxes, fonts, first_stream_obj + i)
            )
    for i, lines in enumerate(pages):
        # `layout` splits a page into text blocks at different origins; the
        # default is the single block every other fixture is drawn as.
        blocks = (layout or {}).get(i, [((72, 720, 12), lines)])
        stream = zlib.compress(
            b"\n".join(content_stream(block, *origin) for origin, block in blocks)
        )
        objs.append(
            b"<< /Length %d /Filter /FlateDecode >>\nstream\n" % len(stream)
            + stream
            + b"\nendstream"
        )
    objs.append(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    if garbled:
        objs.append(
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode %d 0 R >>"
            % cmap_obj
        )
        objs.append(b"<< /Length %d >>\nstream\n" % len(cmap) + cmap + b"\nendstream")
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
    **build_options,
) -> None:
    data = build(pages, locked=locked, break_page=break_page, **build_options)
    (HERE / name).write_bytes(data)
    counts = [
        sum(1 for c in "".join(map(line_text, lines)) if not c.isspace()) for lines in pages
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
    # Three pages for the reader's answer to U+FFFD. Page 1 is a contents line
    # whose leader dots are replacement characters, followed by a body line so
    # the page clears the threshold on real text; page 2 is replacement
    # characters only; page 3 is a neighbour with words of its own, so "page 2
    # skipped" cannot be confused with "the document stopped at page 2".
    write(
        "replacement-chars.pdf",
        [[TOC_LINE, BODY_TEXT], GARBLED_LINES, [BODY_TEXT_5, BODY_TEXT_6]],
    )
    # The two ways `all()` loses text a person can see. Page 1 is moved off the
    # origin together with its text, the shape D187 measured (`left 130.5,
    # bottom 279`): every line sits above the width×height rectangle read from
    # `(0,0)`, and the page comes back as a scan. Page 2 keeps its origin and
    # moves only its CropBox, which cuts the ends of its lines instead of
    # emptying the page; its press slug lies between the CropBox top and the
    # MediaBox top, where a reader of every character would find it and a reader
    # of the visible page must not.
    write(
        "offset-cropbox.pdf",
        [
            [BODY_TEXT, BODY_TEXT_2],
            [CROPPED_LINE, CROPPED_LINE_2],
            UNBOXED_PAGE_TEXT,
        ],
        boxes={
            0: (OFFSET_BOX, OFFSET_BOX),
            1: ("[0 0 612 792]", "[89 72 522 720]"),
            2: (OFFSET_BOX, None),
        },
        layout={
            0: [((150, 760, 12), [BODY_TEXT, BODY_TEXT_2])],
            1: [
                ((100, 750, 10), [CROPPED_SLUG]),
                ((100, 600, 12), [CROPPED_LINE, CROPPED_LINE_2]),
            ],
            2: [((150, 760, 12), UNBOXED_PAGE_TEXT)],
        },
    )
    # A page whose `/ToUnicode` sends one code to a lone surrogate. Page 2 has
    # the broken word in the middle of ordinary text and has to survive it; page
    # 3 is nothing but the broken code and is a scan in every sense but the
    # reason, which is why it stays skipped. Page 2 also carries a CropBox and a
    # slug outside it, so the fallback that keeps the page must keep the box too.
    write(
        "broken-utf16.pdf",
        [[BODY_TEXT, BODY_TEXT_2], [BROKEN_LINE, BROKEN_SEGMENTS], [BROKEN_ONLY]],
        cmap=BROKEN_CMAP,
        boxes={1: ("[0 0 612 792]", BROKEN_CROP_BOX)},
        layout={
            1: [
                ((100, 750, 10), [CROPPED_SLUG]),
                ((BROKEN_CROP_ORIGIN, 600, 12), [BROKEN_LINE, BROKEN_SEGMENTS]),
                ((BROKEN_CROP_ORIGIN, 572, 12), [CUT_LINE, CROPPED_LINE_2]),
            ]
        },
    )
    # The box a page is read within, and the three ways a file can state it
    # wrongly: inherited, degenerate, and wider than the MediaBox. Page 1 has no
    # box of its own at all; page 2 declares an empty CropBox; page 3 declares a
    # CropBox that reaches past the MediaBox on every side, with its slug in the
    # part a viewer clips.
    write(
        "page-boxes.pdf",
        [
            [BODY_TEXT, BODY_TEXT_2],
            [BODY_TEXT_3, BODY_TEXT_4],
            [BODY_TEXT_5, BODY_TEXT_6],
        ],
        pages_boxes=(TREE_BOX, TREE_CROP),
        boxes={1: (None, DEGENERATE_CROP), 2: (None, WIDER_CROP)},
        layout={
            0: [((150, 760, 12), [BODY_TEXT, BODY_TEXT_2])],
            1: [((150, 760, 12), [BODY_TEXT_3, BODY_TEXT_4])],
            2: [
                ((150, 950, 10), [OUTSIDE_MEDIA_SLUG]),
                ((150, 500, 12), [BODY_TEXT_5, BODY_TEXT_6]),
            ],
        },
    )
    write_solid_png()
