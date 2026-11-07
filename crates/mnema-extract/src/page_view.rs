//! One PDF page as a PNG with the boxes of a citation (`--render-page`).
//!
//! The boxes come from pdfium's own char boxes, through the char index each
//! char of the stored page text was read from ([`mapped_page_text`]) — and only
//! when the page's text, read again now, hashes to what the offsets were counted
//! in. A page that changed under the index gets its image and no boxes: a
//! highlight on the wrong words is worse than none.

use std::fmt::Write as _;

use base64::Engine as _;
use mnema_core::nfc;
use mnema_core::render::{
    RenderError, RenderRange, RenderRect, RenderReply, RenderRequest, TextMatch,
};
use pdfium_render::prelude::{PdfBitmapFormat, PdfPage, PdfPoints, PdfRenderConfig};
use sha2::{Digest, Sha256};

use crate::ocr::recognise_ranges;
use crate::pdf::render_for_ocr;
use crate::pdfium_probe::{lock_pdfium, mapped_page_text, pdfium};
use crate::typing::PDF_MAGIC;

/// The widest image one request may ask for. The window shows a page a few
/// hundred points wide; past this the PNG only costs encode time and memory.
pub const MAX_WIDTH_PX: u32 = 3000;

/// The tallest bitmap drawn. The width cap alone does not bound memory: a page
/// a few points wide and metres tall, drawn at a capped width, is still
/// millions of rows. pdfium scales the page down to fit, keeping its shape.
/// 4500 leaves an A4 page at the full 3000 width (3882 px) untouched.
pub const MAX_HEIGHT_PX: u32 = 4500;

fn refused(error: RenderError, message: impl Into<String>) -> RenderReply {
    RenderReply::Refused {
        error,
        message: message.into(),
    }
}

/// Draws the page `req` names and, when its text is still the text the ranges
/// were counted in, the boxes of those ranges.
pub fn render_page(req: &RenderRequest) -> RenderReply {
    let bytes = match std::fs::read(&req.path) {
        Ok(bytes) => bytes,
        Err(e) => return refused(RenderError::Unreadable, format!("{}: {e}", req.path)),
    };
    if !bytes.starts_with(PDF_MAGIC) {
        return refused(RenderError::NotPdf, format!("{} is not a PDF", req.path));
    }
    let pdfium = match pdfium() {
        Ok(pdfium) => pdfium,
        Err(e) => return refused(RenderError::Unreadable, e.to_string()),
    };
    let _serialised = lock_pdfium();
    let document = match pdfium.load_pdf_from_byte_slice(&bytes, None) {
        Ok(document) => document,
        Err(e) => return refused(RenderError::Unreadable, e.to_string()),
    };

    let pages = document.pages();
    let count = pages.len() as u32;
    if req.page_no == 0 || req.page_no > count {
        return refused(
            RenderError::NoSuchPage,
            format!("page {} of {count}", req.page_no),
        );
    }
    let page = match pages.get((req.page_no - 1) as _) {
        Ok(page) => page,
        Err(e) => return refused(RenderError::Unreadable, e.to_string()),
    };

    let width = req.width_px.clamp(1, MAX_WIDTH_PX);
    let config = PdfRenderConfig::new()
        .set_target_width(width as _)
        .set_maximum_height(MAX_HEIGHT_PX as _)
        .set_format(PdfBitmapFormat::BGRA);
    let bitmap = match page.render_with_config(&config) {
        Ok(bitmap) => bitmap,
        Err(e) => return refused(RenderError::Unreadable, e.to_string()),
    };
    let (w, h) = (bitmap.width() as u32, bitmap.height() as u32);
    // RGBA already: `PdfRenderConfig` sets FPDF_REVERSE_BYTE_ORDER by default,
    // so pdfium writes the BGRA format's bytes in R, G, B, A order.
    let raw = bitmap.as_raw_bytes();
    let stride = raw.len() / h.max(1) as usize;
    // A buffer shorter than the rows it claims would slice past its end below.
    if w == 0 || h == 0 || stride < w as usize * 4 {
        return refused(
            RenderError::Unreadable,
            format!(
                "pdfium drew an empty or short bitmap ({w}x{h}, {} bytes)",
                raw.len()
            ),
        );
    }
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for row in raw.chunks_exact(stride).take(h as usize) {
        rgba.extend_from_slice(&row[..w as usize * 4]);
    }
    let png = match encode_png(&rgba, w, h) {
        Ok(png) => png,
        Err(e) => return refused(RenderError::Unreadable, format!("PNG: {e}")),
    };

    let (text, rects) = if req.recognise {
        recognised_text_and_rects(&page, req)
    } else {
        match mapped_page_text(&page) {
            Ok(mapped) => text_and_rects(&page, &config, req, mapped, (w, h)),
            // The file read and drew; only its text layer did not. Nothing says
            // the file changed, so the page is not reported as changed.
            Err(e) => {
                eprintln!("mnema-extract-worker: page {} text: {e}", req.page_no);
                (TextMatch::Unplaced, Vec::new())
            }
        }
    };

    RenderReply::Rendered {
        pages: count,
        width_px: w,
        height_px: h,
        png_base64: base64::engine::general_purpose::STANDARD.encode(png),
        text,
        rects,
    }
}

fn encode_png(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    // Measured by the owner: the middle setting is the one worth its time.
    encoder.set_compression(png::Compression::Balanced);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(out)
}

/// The page of an `ocr:*` row: there is no text layer to read, so the page is
/// recognised again -- from the raster indexing used ([`render_for_ocr`], no
/// other), by the call indexing used ([`recognise_ranges`]) -- and the boxes come
/// back as fractions of the page, which fit the display bitmap whatever its width.
///
/// A different hash here most likely means a different Vision, not a different
/// file, so it is `Unplaced`, never `Mismatch` (spec R3).
fn recognised_text_and_rects(page: &PdfPage, req: &RenderRequest) -> (TextMatch, Vec<RenderRect>) {
    let image = match render_for_ocr(page, req.page_no) {
        Ok(image) => image,
        // The display bitmap drew; only the second raster did not.
        Err(e) => {
            eprintln!("mnema-extract-worker: page {} raster: {e}", req.page_no);
            return (TextMatch::Unplaced, Vec::new());
        }
    };
    let boxed = match recognise_ranges(&image.as_image(), &req.ranges) {
        Ok(Some(boxed)) => boxed,
        Ok(None) => {
            eprintln!("mnema-extract-worker: no OCR engine on this platform");
            return (TextMatch::None, Vec::new());
        }
        Err(e) => {
            eprintln!("mnema-extract-worker: page {} OCR: {e}", req.page_no);
            return (TextMatch::None, Vec::new());
        }
    };
    let Some(wanted) = &req.text_sha256 else {
        return (TextMatch::None, Vec::new());
    };
    if boxed.text.chars().all(char::is_whitespace) {
        return (TextMatch::None, Vec::new());
    }
    // Indexing stores the NFC of what Vision read; offsets and hash are of that.
    if matches!(nfc::normalise(&boxed.text), std::borrow::Cow::Owned(_)) {
        return (TextMatch::Unplaced, Vec::new());
    }
    if sha256_hex(&boxed.text) != *wanted {
        return (TextMatch::Unplaced, Vec::new());
    }
    if boxed.ranges.len() != req.ranges.len() {
        return (TextMatch::Unplaced, Vec::new());
    }
    for (range, boxes) in req.ranges.iter().zip(&boxed.ranges) {
        // A citation with no box would read as "nothing cited here".
        if range.len > 0 && boxes.is_empty() {
            return (TextMatch::Unplaced, Vec::new());
        }
    }
    (
        TextMatch::Match,
        boxed.ranges.into_iter().flatten().collect(),
    )
}

fn sha256_hex(text: &str) -> String {
    let mut hex = String::new();
    for byte in Sha256::digest(text.as_bytes()) {
        write!(hex, "{byte:02x}").expect("a String takes a write");
    }
    hex
}

fn text_and_rects(
    page: &PdfPage,
    config: &PdfRenderConfig,
    req: &RenderRequest,
    mapped: crate::pdfium_probe::MappedPage,
    size: (u32, u32),
) -> (TextMatch, Vec<RenderRect>) {
    let Some(wanted) = &req.text_sha256 else {
        return (TextMatch::None, Vec::new());
    };
    if mapped.text.chars().all(char::is_whitespace) {
        return (TextMatch::None, Vec::new());
    }
    let mut hex = String::new();
    for byte in Sha256::digest(mapped.text.as_bytes()) {
        write!(hex, "{byte:02x}").expect("a String takes a write");
    }
    if hex != *wanted {
        return (TextMatch::Mismatch, Vec::new());
    }
    // The same text, but the map lines up with it only when NFC left it alone.
    if !mapped.nfc_ok {
        return (TextMatch::Unplaced, Vec::new());
    }
    let mut rects = Vec::new();
    for range in &req.ranges {
        let boxes = range_rects(page, config, range, &mapped.map, size);
        // A citation with no box would read as "nothing cited here".
        if range.len > 0 && boxes.is_empty() {
            return (TextMatch::Unplaced, Vec::new());
        }
        rects.extend(boxes);
    }
    (TextMatch::Match, rects)
}

/// One box per line of `range`, in fractions of the bitmap.
fn range_rects(
    page: &PdfPage,
    config: &PdfRenderConfig,
    range: &RenderRange,
    map: &[Option<usize>],
    (w, h): (u32, u32),
) -> Vec<RenderRect> {
    let text = match page.text() {
        Ok(text) => text,
        Err(e) => {
            eprintln!("mnema-extract-worker: page text for boxes: {e}");
            return Vec::new();
        }
    };
    let chars = text.chars();
    let start = (range.start as usize).min(map.len());
    let end = start.saturating_add(range.len as usize).min(map.len());

    // Merged in page space (points, y up), where a line of text has one height
    // whatever /Rotate shows: on a rotated page the line runs along the image's
    // y axis and a merge over pixel rows would find a box per char.
    let mut lines: Vec<(f32, f32, f32, f32)> = Vec::new();
    for index in map[start..end].iter().flatten() {
        let Ok(c) = chars.get(*index) else { continue };
        // Loose, not tight: a line's boxes then share one height and one top,
        // so the line reads as a single band instead of a staircase of glyphs.
        let Ok(b) = c.loose_bounds() else { continue };
        if b.width().value == 0.0 {
            continue;
        }
        let r = (
            b.left().value,
            b.bottom().value,
            b.right().value,
            b.top().value,
        );
        match lines.last_mut() {
            // Same line: vertical overlap of at least half the shorter box, and
            // no step back to the left (that is the next line's first char).
            Some(l)
                if (l.3.min(r.3) - l.1.max(r.1)) * 2.0 >= (l.3 - l.1).min(r.3 - r.1)
                    && r.0 >= l.0 =>
            {
                *l = (l.0.min(r.0), l.1.min(r.1), l.2.max(r.2), l.3.max(r.3));
            }
            _ => lines.push(r),
        }
    }

    let (w, h) = (w as i32, h as i32);
    lines
        .into_iter()
        .filter_map(|(left, bottom, right, top)| {
            // `points_to_pixels` is where /Rotate and a CropBox away from (0,0)
            // are applied; doing that arithmetic here would be a second copy.
            let (Ok((xa, ya)), Ok((xb, yb))) = (
                page.points_to_pixels(PdfPoints::new(left), PdfPoints::new(top), config),
                page.points_to_pixels(PdfPoints::new(right), PdfPoints::new(bottom), config),
            ) else {
                return None;
            };
            let (x0, y0, x1, y1) = (xa.min(xb), ya.min(yb), xa.max(xb), ya.max(yb));
            let (x0, y0, x1, y1) = (x0.max(0), y0.max(0), x1.min(w), y1.min(h));
            (x1 > x0 && y1 > y0).then(|| RenderRect {
                x: x0 as f32 / w as f32,
                y: y0 as f32 / h as f32,
                w: (x1 - x0) as f32 / w as f32,
                h: (y1 - y0) as f32 / h as f32,
                primary: range.primary,
            })
        })
        .collect()
}
