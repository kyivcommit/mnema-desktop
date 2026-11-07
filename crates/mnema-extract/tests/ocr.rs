//! `ocr::recognise` on a real scan, and its "engine unavailable" seam.
//!
//! A file of its own, so its own process: the seam is an environment variable,
//! and setting one while another test thread may read the environment (through
//! libc, which std's lock does not cover) is the race `set_var` is `unsafe` for.
//! Here there is one test per platform and nothing else runs beside it.
//!
//! The macOS test may pay Vision's ~46 s cold start (measured 45.9 s on this
//! binary's first run, 0.4 s after rebuilds; what triggers it is in `ocr.rs`'s
//! module doc). A new test binary may pay it again, which is why this is the
//! only lib-level Vision test; the rest go through the worker.

use mnema_extract::{OCR_UNAVAILABLE_ENV, OcrError, PageImage, recognise};

#[cfg(target_os = "macos")]
#[test]
fn a_scanned_page_is_read_and_the_seam_makes_the_engine_unavailable() {
    use std::path::Path;

    use pdfium_render::prelude::*;

    // This binary binds Pdfium itself: the crate's own handle is `pub(crate)`,
    // and nothing in this process asks the crate for one, so this is the only
    // `Pdfium` here. Same directory the crate would find in a checkout.
    let dir = std::env::var_os(mnema_extract::PDFIUM_LIB_DIR_ENV)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium/lib"));
    let pdfium = Pdfium::new(
        Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&dir))
            .expect("pdfium binds"),
    );
    let doc = pdfium
        .load_pdf_from_file(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan-uk.pdf"),
            None,
        )
        .expect("fixture opens");
    let page = doc.pages().get(0).expect("page 1");
    let config = PdfRenderConfig::new()
        .scale_page_by_factor(300.0 / 72.0)
        .set_format(PdfBitmapFormat::BGRA);
    let bitmap = page.render_with_config(&config).expect("renders");
    let bytes = bitmap.as_raw_bytes();
    let (width, height) = (bitmap.width() as u32, bitmap.height() as u32);
    let image = PageImage {
        bgra: &bytes,
        width,
        height,
        stride: bytes.len() / height as usize,
        dpi: 300,
    };

    let started = std::time::Instant::now();
    let read = recognise(&image)
        .expect("Vision runs")
        .expect("macOS has an engine");
    eprintln!("recognise took {:.1}s", started.elapsed().as_secs_f64());

    // Only words measured to survive Vision verbatim (fixtures' report): `ґ`
    // is not reliably kept (this page has come back with `ґанку` and with
    // `ганку`) and the apostrophe returns as U+02BC, so neither is asserted in
    // its drawn form.
    for word in ["їжак", "єнот", "мʼясо", "памʼятна", "collegium"] {
        assert!(
            read.text.contains(word),
            "{word:?} missing from {:?}",
            read.text
        );
    }
    assert!(read.confidence > 0.0, "confidence {}", read.confidence);
    assert_eq!(
        (read.width_px, read.height_px, read.dpi),
        (width, height, 300)
    );

    // SAFETY: the only test in this binary, so no other thread reads the
    // environment while it changes.
    unsafe { std::env::set_var(OCR_UNAVAILABLE_ENV, "1") };
    let seamed = recognise(&image);
    unsafe { std::env::remove_var(OCR_UNAVAILABLE_ENV) };
    assert!(
        matches!(seamed, Err(OcrError::Unavailable(_))),
        "with {OCR_UNAVAILABLE_ENV} set: {seamed:?}"
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn without_vision_there_is_no_engine_and_the_seam_still_reports_unavailable() {
    let pixels = [255u8; 4];
    let image = PageImage {
        bgra: &pixels,
        width: 1,
        height: 1,
        stride: 4,
        dpi: 300,
    };
    assert_eq!(recognise(&image), Ok(None));

    // SAFETY: the only test in this binary.
    unsafe { std::env::set_var(OCR_UNAVAILABLE_ENV, "1") };
    let seamed = recognise(&image);
    unsafe { std::env::remove_var(OCR_UNAVAILABLE_ENV) };
    assert!(
        matches!(seamed, Err(OcrError::Unavailable(_))),
        "{seamed:?}"
    );
}
