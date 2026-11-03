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
//! **Cold start.** The first recognition in an executable costs ~46 s, once per
//! executable *path* — not per process, not per boot: the same binary copied to
//! a new path paid it again, a rebuilt binary at the old path did not. The
//! packaged worker pays it once per install; a test binary under
//! `target/*/deps/` pays it after every rebuild, because its path carries a
//! hash. A caller's per-page deadline has to cover it (the pool's 120 s does).
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
    if std::env::var_os(OCR_UNAVAILABLE_ENV).is_some() {
        return Err(OcrError::Unavailable(format!(
            "{OCR_UNAVAILABLE_ENV} is set"
        )));
    }
    platform::recognise(image)
}

/// The languages asked for, in priority order. Measured: an English scan reads
/// byte-for-byte the same under this list as under `[en-US]` alone.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const LANGUAGES: [&str; 3] = ["uk-UA", "ru-RU", "en-US"];

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{OcrError, PageImage, Recognised};

    pub(super) fn recognise(_: &PageImage) -> Result<Option<Recognised>, OcrError> {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::AllocAnyThread;
    use objc2::rc::{Retained, autoreleasepool};
    use objc2_core_foundation::CFData;
    use objc2_core_graphics::{
        CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
        CGImageAlphaInfo, CGImageByteOrderInfo,
    };
    use objc2_foundation::{NSArray, NSDictionary, NSError, NSString};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
    };

    use super::{LANGUAGES, OcrError, PageImage, Recognised};

    fn describe(e: &NSError) -> String {
        e.localizedDescription().to_string()
    }

    pub(super) fn recognise(image: &PageImage) -> Result<Option<Recognised>, OcrError> {
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

        // Per call: the worker serves 100 requests, and Vision's temporaries are
        // autoreleased — without a pool of our own they would live until the
        // process does.
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
            if !supported.iter().any(|s| s == LANGUAGES[0]) {
                return Err(OcrError::Unavailable(format!(
                    "{} is not among the supported languages {supported:?}",
                    LANGUAGES[0]
                )));
            }
            let languages: Vec<Retained<NSString>> = LANGUAGES
                .iter()
                .filter(|l| supported.iter().any(|s| s == *l))
                .map(|l| NSString::from_str(l))
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
            for observation in request.results().unwrap_or_default().iter() {
                if let Some(top) = observation.topCandidates(1).firstObject() {
                    lines.push(top.string().to_string());
                    confidence += top.confidence();
                }
            }
            if !lines.is_empty() {
                confidence /= lines.len() as f32;
            }
            Ok(Some(Recognised {
                text: lines.join("\n"),
                confidence,
                width_px: image.width,
                height_px: image.height,
                dpi: image.dpi,
            }))
        })
    }
}
