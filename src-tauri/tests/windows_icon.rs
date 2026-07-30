//! The one build input whose absence is invisible on the machine that builds here.
//!
//! `tauri-build` generates a Windows resource file, and that step reads
//! `icons/icon.ico` and nothing else. It is not a warning: without the file the
//! build script exits non-zero and the shell crate does not compile at all, so
//! `cargo test --workspace` on Windows fails before a single test runs.
//!
//! On macOS nothing reads it. That asymmetry is the whole reason for this file:
//! the icon can be absent, deleted, or replaced by something that is not an ICO,
//! and every check on the machine this product is developed on stays green. It
//! was absent — measured on a Windows 11 stand on 2026-07-29, where the whole
//! workspace stopped on `` `icons/icon.ico` not found; required for generating a
//! Windows Resource file during tauri-build ``, while the same commit was green
//! on macOS and on Linux.
//!
//! D3 puts Windows on the horizon rather than in v1, which is exactly when this
//! guard is cheap: the file costs nothing to keep correct today and costs a
//! debugging session to rediscover on the day someone first builds for Windows.
//!
//! Scope, stated because the previous lint in this directory had to state its
//! own: this reads bytes and a config line. It does not check that the icon is
//! the right *picture*, that its sizes suit a taskbar, or that `tauri-build`
//! accepts it — none of those are observable from here, and only a real Windows
//! build settles the last one.

use std::path::{Path, PathBuf};

fn src_tauri(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn a_windows_icon_is_vendored_beside_the_other_icons() {
    let path = src_tauri("icons/icon.ico");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "{} could not be read: {e}\n\n\
             `tauri-build` requires this exact path to emit the Windows resource \
             file. Without it the shell crate fails to compile on Windows — not a \
             warning, a build error — while every check on macOS stays green, \
             which is how it came to be missing in the first place.",
            path.display()
        )
    });

    // The ICONDIR header: reserved 0, type 1 (icon, not cursor), then the image
    // count. A PNG copied to a `.ico` name satisfies "the file exists" and fails
    // the build the same way a missing file does, so existence alone is not the
    // property worth asserting.
    assert!(
        bytes.len() >= 6,
        "{} is {} bytes — too short to carry even an ICONDIR header",
        path.display(),
        bytes.len()
    );
    assert_eq!(
        &bytes[0..4],
        &[0x00, 0x00, 0x01, 0x00],
        "{} does not start with the ICONDIR magic 00 00 01 00. A PNG or ICNS \
         renamed to `.ico` gets past a file-exists check and then fails the \
         Windows build exactly as a missing file does.",
        path.display()
    );

    let images = u16::from_le_bytes([bytes[4], bytes[5]]);
    assert!(
        images > 0,
        "{} declares {images} images. An ICO with an empty directory is a valid \
         file and a useless icon.",
        path.display()
    );

    assert_no_indexed_png(&bytes, images as usize, &path.display().to_string());
}

/// Refuses palettised PNG inside the ICO.
///
/// This assertion exists because the header check above passed a file that did
/// not build. An ICO may store each image as a BMP or as a PNG, and Tauri's
/// decoder reads a **truecolour** PNG only: `tauri::generate_context!` panicked
/// at compile time with `Unsupported PNG color type: Indexed`, which is a
/// `proc macro panicked` error against `src/lib.rs` and names neither the icon
/// pipeline nor a fix. Measured on Windows 11 on 2026-07-29, from an ICO whose
/// renditions an image tool had helpfully optimised into a 256-colour palette.
///
/// The check is worth the twenty lines because the failure is invisible from
/// macOS in both directions: the icon is not decoded there, and the tool that
/// writes it chooses the palette on its own, by content. A future regeneration
/// on a machine with a different tool reintroduces it silently.
fn assert_no_indexed_png(bytes: &[u8], images: usize, name: &str) {
    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    // Inside a PNG: 8 bytes of signature, then the IHDR chunk — 4 length, 4 tag,
    // 4 width, 4 height, 1 bit depth, and then the colour type.
    const COLOUR_TYPE_OFFSET: usize = 25;
    const INDEXED: u8 = 3;

    for index in 0..images {
        let entry = 6 + index * 16;
        assert!(
            bytes.len() >= entry + 16,
            "{name} declares {images} images but the directory is truncated at \
             image {index}"
        );
        let size = u32::from_le_bytes(bytes[entry + 8..entry + 12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(bytes[entry + 12..entry + 16].try_into().unwrap()) as usize;
        assert!(
            offset + size <= bytes.len(),
            "{name} image {index} claims {size} bytes at {offset}, past the end \
             of a {}-byte file",
            bytes.len()
        );

        let image = &bytes[offset..offset + size];
        // A BMP-encoded entry is equally valid and simply not what this checks.
        if !image.starts_with(&PNG_SIGNATURE) {
            continue;
        }
        assert!(
            image.len() > COLOUR_TYPE_OFFSET,
            "{name} image {index} starts as a PNG and ends before its header does"
        );
        assert_ne!(
            image[COLOUR_TYPE_OFFSET], INDEXED,
            "{name} image {index} is a palettised PNG. Tauri's ICO decoder \
             rejects colour type 3, and it rejects it inside \
             `tauri::generate_context!` — so the build fails as `proc macro \
             panicked` at src/lib.rs, pointing at the macro rather than at this \
             file. Write the renditions as truecolour RGBA (`PNG32:`) instead; \
             the size difference is a few kilobytes."
        );
    }
}

#[test]
fn the_bundle_configuration_lists_the_windows_icon() {
    // Two separate consumers, and only one of them is `tauri-build`. The bundler
    // reads this list when it packages, so an icon present on disk but absent
    // from the list produces an installer carrying the default Tauri artwork —
    // green build, wrong product.
    let path = src_tauri("tauri.conf.json");
    let config = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    assert!(
        config.contains("\"icons/icon.ico\""),
        "{} does not list `icons/icon.ico` among the bundle icons. The file on \
         disk satisfies `tauri-build`; this list is what the bundler reads, and \
         an icon missing from it ships as the default artwork without any step \
         going red.",
        path.display()
    );
}
