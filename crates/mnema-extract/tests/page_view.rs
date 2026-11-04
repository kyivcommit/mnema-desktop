//! `render_page`: the page as a PNG and the boxes of a citation on it.
//!
//! The numbers written as literals below were measured once with
//! `pdftotext -bbox` on the fixtures (points, origin top left of the page as
//! shown) and are the independent witness for where a word sits.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use mnema_core::render::*;
use mnema_extract::{extract_pdf, render_page};
use sha2::{Digest, Sha256};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The page text exactly as the reader stores it.
fn stored(name: &str, page_no: u32) -> String {
    let doc = extract_pdf(&std::fs::read(fixture(name)).unwrap()).unwrap();
    doc.pages
        .iter()
        .find(|p| p.page_no == page_no)
        .unwrap()
        .blocks[0]
        .text
        .clone()
}

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Char offset of `needle` in `text`, as a range counts it.
fn range_of(text: &str, needle: &str, primary: bool) -> RenderRange {
    RenderRange {
        start: text[..text.find(needle).unwrap()].chars().count() as u32,
        len: needle.chars().count() as u32,
        primary,
    }
}

fn request(name: &str, page_no: u32, width_px: u32, ranges: Vec<RenderRange>) -> RenderRequest {
    RenderRequest {
        path: fixture(name).display().to_string(),
        page_no,
        width_px,
        text_sha256: Some(sha(&stored(name, 1))),
        ranges,
    }
}

struct Rendered {
    pages: u32,
    width_px: u32,
    height_px: u32,
    png: Vec<u8>,
    text: TextMatch,
    rects: Vec<RenderRect>,
}

fn rendered(req: &RenderRequest) -> Rendered {
    match render_page(req) {
        RenderReply::Rendered {
            pages,
            width_px,
            height_px,
            png_base64,
            text,
            rects,
        } => Rendered {
            pages,
            width_px,
            height_px,
            png: base64::engine::general_purpose::STANDARD
                .decode(png_base64)
                .expect("standard base64"),
            text,
            rects,
        },
        other => panic!("expected a page, got {other:?}"),
    }
}

fn refusal(req: &RenderRequest) -> RenderError {
    match render_page(req) {
        RenderReply::Refused { error, .. } => error,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Decoded RGBA pixels of a PNG and its size.
fn decode(png_bytes: &[u8]) -> (Vec<u8>, u32, u32) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png_bytes))
        .read_info()
        .expect("a PNG");
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).expect("a frame");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    buf.truncate(info.buffer_size());
    (buf, info.width, info.height)
}

/// Pixels darker than mid-grey in the region `x, y, w, h` (fractions of the
/// image, as `RenderRect` carries them), shifted by `(dx, dy)` fractions.
fn ink(r: &Rendered, rect: &RenderRect, dx: f32, dy: f32) -> usize {
    let (px, w, h) = decode(&r.png);
    let x0 = ((rect.x + dx) * w as f32).round() as usize;
    let y0 = ((rect.y + dy) * h as f32).round() as usize;
    let (rw, rh) = (
        (rect.w * w as f32).round() as usize,
        (rect.h * h as f32).round() as usize,
    );
    let mut dark = 0;
    for y in y0..(y0 + rh).min(h as usize) {
        for x in x0..(x0 + rw).min(w as usize) {
            let o = (y * w as usize + x) * 4;
            if px[o] < 128 && px[o + 1] < 128 && px[o + 2] < 128 {
                dark += 1;
            }
        }
    }
    dark
}

#[test]
fn a_word_of_the_stored_text_is_boxed_where_the_page_shows_it() {
    let text = stored("one-page-text.pdf", 1);
    // 1224 px is twice the page's 612 pt, so a fraction maps to points by 612 x 792.
    let req = request(
        "one-page-text.pdf",
        1,
        1224,
        vec![range_of(&text, "Northwind", true)],
    );
    let r = rendered(&req);

    assert_eq!((r.pages, r.width_px, r.height_px), (1, 1224, 1584));
    assert_eq!(r.text, TextMatch::Match);
    assert_eq!(r.rects.len(), 1, "{:?}", r.rects);
    let b = r.rects[0];
    assert!(b.primary);
    for v in [b.x, b.y, b.x + b.w, b.y + b.h] {
        assert!((0.0..=1.0).contains(&v), "{b:?}");
    }

    // `pdftotext -bbox`: "Northwind" is x 246.108..300.120, y 63.384..74.484.
    let (left, right) = (b.x * 612.0, (b.x + b.w) * 612.0);
    let (top, bottom) = (b.y * 792.0, (b.y + b.h) * 792.0);
    assert!(
        (left - 246.108).abs() < 2.0 && (right - 300.120).abs() < 2.0,
        "{left}..{right}"
    );
    // The loose box is the line's box: it holds the word's ink box top to bottom.
    // The tight one is the glyphs' own and starts below 63.4.
    assert!(
        top <= 63.384 + 0.5 && bottom >= 74.484 - 0.5,
        "{top}..{bottom}"
    );
    assert!(ink(&r, &b, 0.0, 0.0) > 100, "no ink under the box");
}

#[test]
fn the_box_follows_a_page_whose_box_does_not_start_at_the_origin() {
    // Page 1 of this file: a CropBox away from (0, 0); "Northwind" at x 193.608..247.620.
    let text = stored("offset-cropbox.pdf", 1);
    let req = request(
        "offset-cropbox.pdf",
        1,
        531,
        vec![range_of(&text, "Northwind", true)],
    );
    let r = rendered(&req);
    assert_eq!((r.width_px, r.height_px), (531, 666));
    assert_eq!(r.rects.len(), 1, "{:?}", r.rects);
    let b = r.rects[0];
    assert!(
        (b.x * 531.0 - 193.608).abs() < 2.0 && ((b.x + b.w) * 531.0 - 247.620).abs() < 2.0,
        "{b:?}"
    );
    assert!(ink(&r, &b, 0.0, 0.0) > 100, "no ink under the box");
    // The same region a hundred pixels lower is blank page.
    assert_eq!(ink(&r, &b, 0.0, 100.0 / 666.0), 0);
}

#[test]
fn the_box_follows_a_page_shown_rotated() {
    // /Rotate 90: the page shows 792 x 612 and its lines run down the image.
    let text = stored("rotated-90.pdf", 1);
    let req = request(
        "rotated-90.pdf",
        1,
        792,
        vec![range_of(&text, "Northwind", true)],
    );
    let r = rendered(&req);
    assert_eq!((r.width_px, r.height_px), (792, 612));
    // One box for the word, not one per char: the line is merged before it is turned.
    assert_eq!(r.rects.len(), 1, "{:?}", r.rects);
    let b = r.rects[0];
    assert!(b.h > b.w, "a word that runs down the image: {b:?}");
    assert!(ink(&r, &b, 0.0, 0.0) > 100, "no ink under the box");
    // The column to its left, where the page is blank.
    assert_eq!(ink(&r, &b, -100.0 / 792.0, 0.0), 0);
}

#[test]
fn ranges_are_boxed_per_line_and_never_merged_with_each_other() {
    let text = stored("one-page-text.pdf", 1);
    let one_line = range_of(&text, "Invented contract 4417", false);
    let two_lines = range_of(&text, "Freight,\r\nsigned", false);
    let neighbours = [
        range_of(&text, "Invented", true),
        range_of(&text, "contract", false),
    ];

    let rects = |ranges| rendered(&request("one-page-text.pdf", 1, 612, ranges)).rects;
    assert_eq!(rects(vec![one_line]).len(), 1);
    assert_eq!(rects(vec![two_lines]).len(), 2);
    // Side by side on one line, and still two boxes — each with its own flag.
    let both = rects(neighbours.to_vec());
    assert_eq!(both.len(), 2, "{both:?}");
    assert_eq!((both[0].primary, both[1].primary), (true, false));
}

#[test]
fn a_range_past_the_end_of_the_text_is_clamped_and_draws_what_is_there() {
    let text = stored("one-page-text.pdf", 1);
    let n = text.chars().count() as u32;
    let past = |start, len| {
        rendered(&request(
            "one-page-text.pdf",
            1,
            612,
            vec![RenderRange {
                start,
                len,
                primary: true,
            }],
        ))
    };
    let tail = past(n - 7, 1000);
    assert_eq!((tail.text, tail.rects.len()), (TextMatch::Match, 1));
    let none = past(n + 5, 3);
    assert_eq!((none.text, none.rects.len()), (TextMatch::Match, 0));
    assert_eq!(past(u32::MAX, u32::MAX).rects.len(), 0);
}

#[test]
fn text_that_is_not_the_text_the_offsets_were_counted_in_gets_the_page_and_no_boxes() {
    let text = stored("one-page-text.pdf", 1);
    let mut req = request(
        "one-page-text.pdf",
        1,
        612,
        vec![range_of(&text, "Northwind", true)],
    );
    req.text_sha256 = Some(sha("some other text"));
    let r = rendered(&req);
    assert_eq!((r.text, r.rects.len()), (TextMatch::Mismatch, 0));
    assert!(!r.png.is_empty());

    // An uppercase hash is not the lowercase hex the contract names.
    req.text_sha256 = Some(sha(&text).to_uppercase());
    assert_eq!(rendered(&req).text, TextMatch::Mismatch);

    req.text_sha256 = None;
    let r = rendered(&req);
    assert_eq!((r.text, r.rects.len()), (TextMatch::None, 0));
}

#[test]
fn a_page_without_a_text_layer_is_drawn_and_has_no_text_to_compare() {
    let req = RenderRequest {
        path: fixture("scan-uk.pdf").display().to_string(),
        page_no: 1,
        width_px: 600,
        text_sha256: Some(sha("anything")),
        ranges: vec![RenderRange {
            start: 0,
            len: 10,
            primary: true,
        }],
    };
    let r = rendered(&req);
    assert_eq!((r.text, r.rects.len()), (TextMatch::None, 0));
    assert_eq!(r.width_px, 600);
}

#[test]
fn what_is_not_a_page_of_a_pdf_is_refused_by_kind() {
    let base = request("one-page-text.pdf", 1, 612, vec![]);
    let with = |path: String, page_no| RenderRequest {
        path,
        page_no,
        ..base.clone()
    };
    let pdf = base.path.clone();

    assert_eq!(
        refusal(&with(fixture("solid.png").display().to_string(), 1)),
        RenderError::NotPdf
    );
    assert_eq!(refusal(&with(pdf.clone(), 0)), RenderError::NoSuchPage);
    assert_eq!(refusal(&with(pdf.clone(), 2)), RenderError::NoSuchPage);
    assert_eq!(
        refusal(&with(fixture("no-such-file.pdf").display().to_string(), 1)),
        RenderError::Unreadable
    );

    // The magic and nothing after it: a PDF by name that pdfium cannot open.
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("broken.pdf");
    std::fs::write(&broken, b"%PDF-1.4\nnot really\n").unwrap();
    assert_eq!(
        refusal(&with(broken.display().to_string(), 1)),
        RenderError::Unreadable
    );
}

#[test]
fn the_png_is_exactly_the_size_reported_and_a_wide_request_is_capped() {
    let r = rendered(&request("one-page-text.pdf", 1, 900, vec![]));
    let (_, w, h) = decode(&r.png);
    assert_eq!((w, h), (r.width_px, r.height_px));
    assert_eq!((w, h), (900, 1165));

    let r = rendered(&request("one-page-text.pdf", 1, 5000, vec![]));
    let (_, w, h) = decode(&r.png);
    assert_eq!((w, h), (r.width_px, r.height_px));
    assert_eq!(w, 3000);
}

/// The width cap alone bounds nothing on a page that is a narrow strip: drawn
/// 3000 px wide, a 10 x 20000 pt page would be six million rows tall.
#[test]
fn a_page_far_taller_than_wide_is_drawn_within_the_height_cap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("strip.pdf");
    std::fs::write(&path, decomposed_pdf("[0 0 10 20000]")).unwrap();
    let r = rendered(&RenderRequest {
        path: path.display().to_string(),
        page_no: 1,
        width_px: 3000,
        text_sha256: None,
        ranges: vec![],
    });
    let (_, w, h) = decode(&r.png);
    assert_eq!((w, h), (r.width_px, r.height_px));
    assert!(
        (4000..=mnema_extract::page_view::MAX_HEIGHT_PX).contains(&h),
        "{w}x{h}"
    );
    assert!(w <= 3, "the page keeps its shape: {w}x{h}");
}

/// A one-page PDF whose font maps code `A` to U+0438 and `B` to U+0306: the
/// page's text reads "и" + a combining breve, which NFC then recomposes into
/// one "й". Written here rather than as a fixture file because it is a handful
/// of lines and nothing else wants it.
fn decomposed_pdf(media_box: &str) -> Vec<u8> {
    let cmap = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
        /CMapName /D def /CMapType 2 def \
        1 begincodespacerange <00> <FF> endcodespacerange \
        2 beginbfrange <20> <40> <0020> <43> <7E> <0043> endbfrange \
        2 beginbfchar <41> <0438> <42> <0306> endbfchar \
        endcmap CMapName currentdict /CMap defineresource pop end end";
    let content = "BT /F1 12 Tf 72 700 Td (xAB the quick brown fox jumps over the lazy dog \
        again and again) Tj ET";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox {media_box} \
             /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>".to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        format!("<< /Length {} >>\nstream\n{cmap}\nendstream", cmap.len()),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .bytes(),
    );
    out
}

#[test]
fn a_page_whose_text_nfc_rewrote_has_no_boxes_because_its_offsets_no_longer_line_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("decomposed.pdf");
    std::fs::write(&path, decomposed_pdf("[0 0 612 792]")).unwrap();
    let doc = extract_pdf(&std::fs::read(&path).unwrap()).unwrap();
    let text = doc.pages[0].blocks[0].text.clone();
    assert!(text.starts_with("x\u{0439} "), "the reader stored {text:?}");

    // The hash is the stored text's own, so only the NFC guard can refuse it.
    let req = RenderRequest {
        path: path.display().to_string(),
        page_no: 1,
        width_px: 612,
        text_sha256: Some(sha(&text)),
        ranges: vec![range_of(&text, "quick", true)],
    };
    let r = rendered(&req);
    assert_eq!((r.text, r.rects.len()), (TextMatch::Mismatch, 0));
}
