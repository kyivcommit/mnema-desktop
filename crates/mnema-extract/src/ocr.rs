//! Text recognition for a rendered PDF page that has no text layer (D189).
//!
//! **Why Apple Vision.** Measured on the owner's scanned volumes (M2 Max,
//! macOS 27.2): `VNRecognizeTextRequest` at `.accurate` reads Ukrainian with
//! `і`/`ї`/`є` intact, keeps two columns apart, and costs 0.18–0.37 s per
//! 300 dpi page on the Neural Engine with a peak under 200 MB — and it ships
//! with the OS, so there is no model to bundle or download. Its word match on a
//! sampled page was 0.988 against the page's embedded text. What it does *not*
//! reproduce verbatim (`ґ` becomes `г`, the apostrophe comes back as U+02BC) is
//! recorded with the fixtures' report and is why tests assert only on words
//! measured to survive.
//!
//! **BGRA only.** [`PageImage`] is 32-bit BGRA, premultiplied alpha first,
//! little-endian — pdfium's `PdfBitmapFormat::BGRA` as rendered. An 8-bit gray
//! bitmap handed to Vision the same way returns *zero* observations with no
//! error, on every page measured: a silent "this scan has no text", which is the
//! failure this module exists to remove. There is therefore no gray input type
//! to choose by mistake.
//!
//! **Cold start.** The first recognition can cost ~46 s, and what decides
//! when is not known. It was first read as once per executable *path* (a copy
//! at a new path paid again, a rebuild at the old path did not; `tests/ocr.rs`
//! 45.9 s on its first run, 0.4 s after rebuilds). On 2026-11-06 both halves
//! failed once: four copies of a rebuilt probe in a new directory warmed up in
//! 0.6–0.7 s, and the first run of a rebuilt worker at its old path paid
//! 46.9 s. So any first run may pay it, and `--ocr` helpers started together
//! pay it together (D196). A caller's per-page deadline has to cover it, two
//! workers starting cold together included (66 s and 86 s, measured): the
//! pool's 300 s does.
//!
//! **Memory.** Resident size grows ~35 MB per recognised page and keeps
//! growing (2 GB after 60 calls), but `phys_footprint` does not (178 → 183 MB
//! over 20 calls): the growth is IOSurface memory Vision leaves *reclaimable*,
//! which the OS takes back under pressure. Measured with `footprint`; a peak
//! read from RSS will look like a leak and is not one.
//!
//! The text is the observations' top candidates in the order Vision returned
//! them, joined with `\n`. It is **not** NFC-normalised and not thresholded:
//! both are the caller's, so that "is this a page of text" stays one rule
//! (`TEXT_LAYER_MIN_CHARS`) for the native and the recognised branch alike.

/// Set to any value, this makes [`recognise`] report the engine as
/// unavailable without touching Vision — on every platform, so a test of the
/// "engine missing" path runs on Linux CI as well. It exists for tests (a
/// worker started with it must fail a scan, never journal it as textless) and
/// costs one environment lookup per recognised page.
pub const OCR_UNAVAILABLE_ENV: &str = "MNEMA_OCR_UNAVAILABLE";

use mnema_core::render::{RenderRange, RenderRect};

/// A rendered page, borrowed: 32-bit BGRA rows of `stride` bytes each.
#[derive(Debug, Clone, Copy)]
pub struct PageImage<'a> {
    pub bgra: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// Bytes per row; at least `width * 4`, pdfium may pad it.
    pub stride: usize,
    /// The resolution the page was rendered at, carried through to
    /// [`Recognised::dpi`] for the `page` row.
    pub dpi: u32,
}

/// A rendered page that owns its pixels — what crosses to another thread or
/// process, where a borrow of pdfium's bitmap cannot go. Same layout as
/// [`PageImage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedPageImage {
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub dpi: u32,
}

impl OwnedPageImage {
    pub fn as_image(&self) -> PageImage<'_> {
        PageImage {
            bgra: &self.bgra,
            width: self.width,
            height: self.height,
            stride: self.stride,
            dpi: self.dpi,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recognised {
    pub text: String,
    /// Mean confidence of the lines' top candidates; 0 when there are none.
    pub confidence: f32,
    pub width_px: u32,
    pub height_px: u32,
    pub dpi: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OcrError {
    /// The engine cannot run here at all (no Ukrainian model, or the test
    /// seam). Not a fact about the page.
    #[error("OCR engine unavailable: {0}")]
    Unavailable(String),
    /// The engine ran and failed on this image.
    #[error("OCR failed: {0}")]
    Failed(String),
}

/// Recognises the text on `image`.
///
/// `Ok(None)` means this platform has no engine — the non-macOS build, where a
/// textless page stays skipped as it was before OCR existed. On macOS an image
/// with nothing legible is `Ok(Some(..))` with empty `text`; deciding that it
/// is not a page of text is the caller's threshold, not a second one here.
pub fn recognise(image: &PageImage) -> Result<Option<Recognised>, OcrError> {
    refuse_if_seam_set()?;
    Ok(platform::recognise(image, &[])?.map(|(read, _)| read))
}

/// What the platform's one request returns: the page, and the boxes of the
/// requested ranges.
type Read = (Recognised, Vec<Vec<RenderRect>>);

/// The text of a recognised page and, for each requested range, the boxes of
/// the lines it touches.
#[derive(Debug, Clone, PartialEq)]
pub struct Boxed {
    /// Exactly what [`recognise`] returns as `text` for the same image.
    pub text: String,
    /// One list per input range, in the input's order: a rect per line the
    /// range touches. Empty where Vision gave no box for any of its lines.
    pub ranges: Vec<Vec<RenderRect>>,
}

/// [`recognise`], and the boxes of `ranges` (chars of the recognised text, the
/// unit [`RenderRange`] counts) on the page, as fractions with the origin at
/// the top left.
///
/// **The same function as [`recognise`], not a copy of it.** The offsets stored
/// at indexing were counted in the text that request produced; a second request
/// configured by hand would agree with it only until one of them changed.
/// `Ok(None)` where there is no engine, as for [`recognise`].
pub fn recognise_ranges(
    image: &PageImage,
    ranges: &[RenderRange],
) -> Result<Option<Boxed>, OcrError> {
    refuse_if_seam_set()?;
    Ok(
        platform::recognise(image, ranges)?.map(|(read, ranges)| Boxed {
            text: read.text,
            ranges,
        }),
    )
}

fn refuse_if_seam_set() -> Result<(), OcrError> {
    if std::env::var_os(OCR_UNAVAILABLE_ENV).is_some() {
        return Err(OcrError::Unavailable(format!(
            "{OCR_UNAVAILABLE_ENV} is set"
        )));
    }
    Ok(())
}

/// The part of the chars `[start, start + len)` of the page text that falls in
/// `line` — which starts at char `line_start` of that text — as (offset, length)
/// in UTF-16 units of the line, which is what `NSRange` counts. `None` when
/// they do not overlap.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn utf16_range(line: &str, line_start: usize, start: usize, len: usize) -> Option<(usize, usize)> {
    let line_end = line_start + line.chars().count();
    let (from, to) = (start.max(line_start), (start + len).min(line_end));
    if from >= to {
        return None;
    }
    let units = |chars: usize| line.chars().take(chars).map(char::len_utf16).sum::<usize>();
    let (a, b) = (units(from - line_start), units(to - line_start));
    Some((a, b - a))
}

/// A Vision box (fractions of the image, origin bottom left) as a
/// [`RenderRect`] (origin top left).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn to_frac(origin_x: f64, origin_y: f64, w: f64, h: f64, primary: bool) -> RenderRect {
    RenderRect {
        x: origin_x as f32,
        y: (1.0 - origin_y - h) as f32,
        w: w as f32,
        h: h as f32,
        primary,
    }
}

/// The languages asked for, in priority order. Measured: an English scan reads
/// byte-for-byte the same under this list as under `[en-US]` alone.
const LANGUAGES: [&str; 3] = ["uk-UA", "ru-RU", "en-US"];

/// [`LANGUAGES`] that `supported` contains, in [`LANGUAGES`]' order — or
/// `Unavailable` without `uk-UA`: an engine that cannot read Ukrainian would
/// turn every scan in the library into "no text", which is not a fact about
/// the scans.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn languages(supported: &[String]) -> Result<Vec<&'static str>, OcrError> {
    let chosen: Vec<&'static str> = LANGUAGES
        .into_iter()
        .filter(|l| supported.iter().any(|s| s == l))
        .collect();
    if chosen.first() != Some(&LANGUAGES[0]) {
        return Err(OcrError::Unavailable(format!(
            "{} is not among the supported languages {supported:?}",
            LANGUAGES[0]
        )));
    }
    Ok(chosen)
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use mnema_core::render::{RenderRange, RenderRect};

    use super::{OcrError, PageImage, Read};

    pub(super) fn recognise(_: &PageImage, _: &[RenderRange]) -> Result<Option<Read>, OcrError> {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use mnema_core::render::{RenderRange, RenderRect};
    use objc2::AllocAnyThread;
    use objc2::rc::{Retained, autoreleasepool};
    use objc2_core_foundation::CFData;
    use objc2_core_graphics::{
        CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
        CGImageAlphaInfo, CGImageByteOrderInfo,
    };
    use objc2_foundation::{NSArray, NSDictionary, NSError, NSRange, NSString};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
    };

    use super::{OcrError, PageImage, Read, Recognised, languages, to_frac, utf16_range};

    fn describe(e: &NSError) -> String {
        e.localizedDescription().to_string()
    }

    /// The one Vision request: the page's text, and the boxes of `ranges` taken
    /// while the line observations are alive.
    pub(super) fn recognise(
        image: &PageImage,
        ranges: &[RenderRange],
    ) -> Result<Option<Read>, OcrError> {
        let (w, h) = (image.width as usize, image.height as usize);
        // Checked here because CGImage reads `stride * height` bytes from what
        // it is given and would read past a short buffer rather than refuse it.
        let fits = image.stride.checked_mul(h).filter(|&len| {
            w > 0 && h > 0 && image.stride >= w.saturating_mul(4) && len <= image.bgra.len()
        });
        let Some(len) = fits else {
            return Err(OcrError::Failed(format!(
                "bitmap geometry {}x{} stride {} does not fit {} bytes",
                image.width,
                image.height,
                image.stride,
                image.bgra.len()
            )));
        };
        let pixels = &image.bgra[..len];

        // Per call: the worker serves 100 requests, and anything Vision
        // autoreleases would otherwise live until the thread's outermost pool,
        // which a worker never drains. Insurance: over 20 calls the footprint
        // was the same without it.
        autoreleasepool(|_| {
            // Copied, not lent. `CGDataProvider::with_data` would avoid the
            // copy, but then nothing bounds how long CoreGraphics keeps reading
            // a slice the caller frees when this returns — the release callback
            // is the only signal, and it is not promised to fire before we do.
            // One memcpy of a page (~35 MB at 300 dpi A4) is a few ms against
            // hundreds for the recognition.
            let data = CFData::from_bytes(pixels);
            let provider = CGDataProvider::with_cf_data(Some(&data))
                .ok_or_else(|| OcrError::Failed("CGDataProviderCreateWithCFData".into()))?;
            let space = CGColorSpace::new_device_rgb()
                .ok_or_else(|| OcrError::Failed("CGColorSpaceCreateDeviceRGB".into()))?;
            let info = CGBitmapInfo(
                CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0,
            );
            // SAFETY: `decode` is null, which CGImageCreate documents as "no
            // decode array"; the provider owns its own copy of exactly
            // `stride * height` bytes, checked above.
            let cg = unsafe {
                CGImage::new(
                    w,
                    h,
                    8,
                    32,
                    image.stride,
                    Some(&space),
                    info,
                    Some(&provider),
                    std::ptr::null(),
                    false,
                    CGColorRenderingIntent::RenderingIntentDefault,
                )
            }
            .ok_or_else(|| OcrError::Failed("CGImageCreate".into()))?;

            let request = VNRecognizeTextRequest::new();
            request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
            request.setUsesLanguageCorrection(true);
            // SAFETY: a plain getter on a request we own, called on the thread
            // that created it; the error out-parameter is handled by the binding.
            let supported = unsafe { request.supportedRecognitionLanguagesAndReturnError() }
                .map_err(|e| OcrError::Unavailable(describe(&e)))?;
            let supported: Vec<String> = supported.iter().map(|s| s.to_string()).collect();
            let languages: Vec<Retained<NSString>> = languages(&supported)?
                .into_iter()
                .map(NSString::from_str)
                .collect();
            request.setRecognitionLanguages(&NSArray::from_retained_slice(&languages));

            // SAFETY: an empty options dictionary is the documented default.
            let handler = unsafe {
                VNImageRequestHandler::initWithCGImage_options(
                    VNImageRequestHandler::alloc(),
                    &cg,
                    &NSDictionary::new(),
                )
            };
            let as_request: Retained<VNRequest> =
                Retained::into_super(Retained::into_super(request.clone()));
            handler
                .performRequests_error(&NSArray::from_retained_slice(&[as_request]))
                .map_err(|e| OcrError::Failed(describe(&e)))?;

            let mut lines = Vec::new();
            let mut confidence = 0.0f32;
            let mut boxes: Vec<Vec<RenderRect>> = vec![Vec::new(); ranges.len()];
            // Char offset in the joined text of the line being read.
            let mut line_start = 0usize;
            for observation in request.results().unwrap_or_default().iter() {
                if let Some(top) = observation.topCandidates(1).firstObject() {
                    let line = top.string().to_string();
                    for (range, found) in ranges.iter().zip(&mut boxes) {
                        let Some((at, len)) = utf16_range(
                            &line,
                            line_start,
                            range.start as usize,
                            range.len as usize,
                        ) else {
                            continue;
                        };
                        // SAFETY: a plain getter on an observation we hold; the
                        // error out-parameter is handled by the binding. A line
                        // Vision refuses a box for is left out, not guessed.
                        let Ok(rect) =
                            (unsafe { top.boundingBoxForRange_error(NSRange::new(at, len)) })
                        else {
                            continue;
                        };
                        // SAFETY: a plain getter.
                        let b = unsafe { rect.boundingBox() };
                        // Measured: a range of whitespace comes back as the
                        // all-zero box, not as an error. That is Vision having
                        // no box, and a rect at the page's corner would be one.
                        if b.size.width <= 0.0 || b.size.height <= 0.0 {
                            continue;
                        }
                        found.push(to_frac(
                            b.origin.x,
                            b.origin.y,
                            b.size.width,
                            b.size.height,
                            range.primary,
                        ));
                    }
                    // `\n` between lines is one char of the joined text.
                    line_start += line.chars().count() + 1;
                    lines.push(line);
                    confidence += top.confidence();
                }
            }
            if !lines.is_empty() {
                confidence /= lines.len() as f32;
            }
            Ok(Some((
                Recognised {
                    text: lines.join("\n"),
                    confidence,
                    width_px: image.width,
                    height_px: image.height,
                    dpi: image.dpi,
                },
                boxes,
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn languages_are_the_intersection_in_priority_order() {
        let supported = owned(&["en-US", "fr-FR", "uk-UA"]);
        assert_eq!(languages(&supported), Ok(vec!["uk-UA", "en-US"]));
    }

    #[test]
    fn an_engine_without_ukrainian_is_unavailable() {
        let supported = owned(&["en-US", "ru-RU"]);
        assert!(
            matches!(languages(&supported), Err(OcrError::Unavailable(_))),
            "{:?}",
            languages(&supported)
        );
    }

    #[test]
    fn a_char_outside_the_bmp_before_the_range_shifts_utf16_by_two() {
        // '𝔸' is one char and two UTF-16 units; the line starts at char 10.
        let line = "𝔸bc def";
        // chars 12..15 of the page = "bc " : after '𝔸' (1 char, 2 units).
        assert_eq!(utf16_range(line, 10, 11, 3), Some((2, 3)));
        // The char itself: one char of range, two units long.
        assert_eq!(utf16_range(line, 10, 10, 1), Some((0, 2)));
        // Units after it: 'd' is char 4 of the line, UTF-16 offset 5.
        assert_eq!(utf16_range(line, 10, 14, 2), Some((5, 2)));
    }

    #[test]
    fn a_range_ending_on_the_newline_gives_the_next_line_nothing() {
        // Page text "ab\ncd": line 1 = chars 0..2, the `\n` is char 2, line 2 starts at 3.
        assert_eq!(utf16_range("ab", 0, 1, 2), Some((1, 1)), "ends on the \\n");
        assert_eq!(utf16_range("cd", 3, 1, 2), None);
        // A range starting on the `\n` reaches the next line, not the previous.
        assert_eq!(utf16_range("ab", 0, 2, 2), None);
        assert_eq!(utf16_range("cd", 3, 2, 2), Some((0, 1)));
        // Clipped on both sides, and empty.
        assert_eq!(utf16_range("abcd", 5, 0, 100), Some((0, 4)));
        assert_eq!(utf16_range("abcd", 5, 6, 0), None);
    }

    #[test]
    fn a_vision_box_is_flipped_to_an_origin_at_the_top() {
        let r = to_frac(0.25, 0.5, 0.5, 0.125, true);
        assert_eq!(
            (r.x, r.y, r.w, r.h, r.primary),
            (0.25, 0.375, 0.5, 0.125, true)
        );
        // A box at the very bottom of the image sits at the very bottom.
        let r = to_frac(0.0, 0.0, 1.0, 0.25, false);
        assert_eq!((r.y, r.primary), (0.75, false));
    }

    // Returns before Vision is reached, so no cold start. The seam variable is
    // only read here, never set: setting it belongs to `tests/ocr.rs`.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_buffer_shorter_than_its_geometry_is_refused_not_read() {
        let pixels = [0u8; 15];
        let image = PageImage {
            bgra: &pixels,
            width: 2,
            height: 2,
            stride: 8,
            dpi: 300,
        };
        assert!(
            matches!(recognise(&image), Err(OcrError::Failed(_))),
            "{:?}",
            recognise(&image)
        );
    }
}

#[cfg(all(test, target_os = "macos"))]
mod vision_tests {
    use super::*;

    /// Page `index` of a fixture, rasterised exactly as indexing does.
    fn page(fixture: &str, index: i32) -> OwnedPageImage {
        let pdfium = crate::pdfium_probe::pdfium().expect("pdfium binds");
        let _serialised = crate::pdfium_probe::lock_pdfium();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let doc = pdfium
            .load_pdf_from_file(&path, None)
            .expect("fixture opens");
        let page = doc.pages().get(index).expect("page exists");
        crate::pdf::render_for_ocr(&page, index as u32 + 1).expect("renders")
    }

    /// `needle`'s place in `text` as a [`RenderRange`], in chars.
    fn range_of(text: &str, needle: &str, primary: bool) -> RenderRange {
        let byte = text.find(needle).expect("the needle is on the page");
        RenderRange {
            start: text[..byte].chars().count() as u32,
            len: needle.chars().count() as u32,
            primary,
        }
    }

    #[test]
    fn recognise_and_recognise_ranges_read_the_same_text() {
        let img = page("scan-two-columns.pdf", 0);
        let plain = recognise(&img.as_image()).unwrap().unwrap();
        let boxed = recognise_ranges(&img.as_image(), &[]).unwrap().unwrap();
        assert!(plain.text.starts_with("ЛІВА 1 Київ"), "{:?}", plain.text);
        assert_eq!(plain.text, boxed.text);
        assert!(boxed.ranges.is_empty());
    }

    #[test]
    fn a_range_is_boxed_where_its_words_are() {
        let img = page("scan-two-columns.pdf", 0);
        let text = recognise(&img.as_image()).unwrap().unwrap().text;
        let whole = range_of(&text, "ЛІВА 1 Київ стоїть на високому березі", true);
        let tail = range_of(&text, "високому березі", false);
        let head = range_of(&text, "ЛІВА 1 Київ", true);
        let across = range_of(&text, "березі\nЛІВА 2", true);
        // Beyond the text: no line to box.
        let beyond = RenderRange {
            start: 10_000,
            len: 5,
            primary: true,
        };
        // The first word of line 2: its offset in that line rests on the one
        // `\n` between the lines having been counted.
        let second_head = range_of(&text, "ЛІВА 2", true);
        let got = recognise_ranges(
            &img.as_image(),
            &[whole, tail, head, across, beyond, second_head],
        )
        .unwrap()
        .unwrap();
        assert_eq!(got.text, text);
        assert_eq!(got.ranges.len(), 6, "one list per input range");

        let [line] = got.ranges[0][..] else {
            panic!("{:?}", got.ranges[0])
        };
        let [tail_box] = got.ranges[1][..] else {
            panic!("{:?}", got.ranges[1])
        };
        let [head_box] = got.ranges[2][..] else {
            panic!("{:?}", got.ranges[2])
        };
        // Mid-line start: right of the margin, and (it runs to the end) the
        // right edge is the line's.
        assert!(tail_box.x > line.x + 0.1, "{tail_box:?} vs {line:?}");
        assert!(
            ((tail_box.x + tail_box.w) - (line.x + line.w)).abs() < 0.02,
            "{tail_box:?} vs {line:?}"
        );
        // And the other way: a range from the margin starts at the margin and
        // stops well short of the end, so no range is boxed as its whole line.
        assert!(
            (head_box.x - line.x).abs() < 0.02,
            "{head_box:?} vs {line:?}"
        );
        assert!(
            head_box.x + head_box.w < line.x + line.w - 0.1,
            "{head_box:?} vs {line:?}"
        );
        assert_eq!((line.primary, tail_box.primary), (true, false));

        // Two lines: two boxes, the second below the first.
        let [first, second] = got.ranges[3][..] else {
            panic!("{:?}", got.ranges[3])
        };
        assert!(
            second.y > first.y + first.h / 2.0,
            "{first:?} then {second:?}"
        );
        assert!(got.ranges[4].is_empty(), "{:?}", got.ranges[4]);
        let [word] = got.ranges[5][..] else {
            panic!("{:?}", got.ranges[5])
        };
        assert!((word.x - line.x).abs() < 0.006, "{word:?} vs {line:?}");
    }

    #[test]
    fn a_page_stood_upright_by_rotate_is_read_line_by_line() {
        let img = page("scan-rotated-90.pdf", 0);
        let text = recognise(&img.as_image()).unwrap().unwrap().text;
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text:?}");
        assert!(lines.iter().all(|l| l.starts_with("ПОВОРОТ")), "{text:?}");
    }

    // Vision boxes whole words (measured: every char of "Київ" gives the same
    // box), so a shifted offset shows only where it leaves the word. The last
    // char of a word is where one char of drift lands on the space after it.
    #[test]
    fn an_offset_is_exact_to_the_char_at_the_edge_of_a_word() {
        let img = page("scan-two-columns.pdf", 0);
        let text = recognise(&img.as_image()).unwrap().unwrap().text;
        let one = |needle: &str, nth: u32| {
            let r = range_of(&text, needle, true);
            RenderRange {
                start: r.start + nth,
                len: 1,
                primary: true,
            }
        };
        let ranges = [
            one("ЛІВА 1", 5), // "1", on line 1
            one("ЛІВА 2", 3), // "А", on line 2: offset rests on the `\n` counted
            one("ЛІВА 2", 4), // the space after it
        ];
        let got = recognise_ranges(&img.as_image(), &ranges).unwrap().unwrap();
        assert_eq!(got.ranges[0].len(), 1, "{:?}", got.ranges[0]);
        assert_eq!(got.ranges[1].len(), 1, "{:?}", got.ranges[1]);
        // Whitespace has no box: nothing, not a rect at the corner.
        assert!(got.ranges[2].is_empty(), "{:?}", got.ranges[2]);
        // The "1" sits right of the line's first word, the "А" ends the word.
        assert!(got.ranges[0][0].x > got.ranges[1][0].x + 0.05);
    }
}
