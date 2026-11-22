//! Decoding a picture into the bitmap [`crate::recognise`] reads.
//!
//! **macOS ImageIO, then a redraw.** ImageIO opens every format the OS knows
//! (PNG, JPEG, HEIC, TIFF, WebP, …) and hands back frames in whatever pixel
//! format the file carries — an 8-bit gray PNG arrives as 8 bpp. Vision reads
//! a gray bitmap as zero lines without an error (`ocr.rs`'s module doc), so
//! every frame is redrawn into 32-bit BGRA, premultiplied first,
//! little-endian, before it leaves this module. The redraw is onto **white**:
//! a transparent PNG of black text would otherwise be black on black.
//!
//! A frame is the ImageIO thumbnail with the EXIF orientation applied (a phone
//! photo stored a quarter-turn round comes out upright) and its long side
//! capped at [`crate::OCR_MAX_SIDE_PX`], the same cap a PDF page is rendered under.
//!
//! **One frame at a time.** [`Frames`] holds the decoder, not the bitmaps: a
//! multi-page TIFF is decoded frame by frame as [`Frames::frame`] is called.

use crate::OwnedPageImage;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImageError {
    /// The bytes are not a picture this decoder can read, or the frame asked
    /// for is not in it.
    #[error("malformed image: {0}")]
    Malformed(String),
    /// This platform has no decoder at all. Not a fact about the file.
    #[error("image decoding unsupported: {0}")]
    Unsupported(String),
}

/// An opened picture: its frames, decoded on request.
pub struct Frames(platform::Source);

/// Opens `bytes` as a picture. `Malformed` when they are not one, or hold no
/// frame.
pub fn open(bytes: &[u8]) -> Result<Frames, ImageError> {
    platform::open(bytes).map(Frames)
}

impl Frames {
    pub fn count(&self) -> usize {
        platform::count(&self.0)
    }

    /// Frame `index`, as BGRA. `Malformed` when there is no such frame or
    /// ImageIO cannot produce it.
    pub fn frame(&self, index: usize) -> Result<OwnedPageImage, ImageError> {
        platform::frame(&self.0, index)
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{ImageError, OwnedPageImage};

    /// Uninhabited: `open` never succeeds here.
    pub(super) enum Source {}

    pub(super) fn open(_: &[u8]) -> Result<Source, ImageError> {
        Err(ImageError::Unsupported(
            "there is no image decoder on this platform".into(),
        ))
    }

    pub(super) fn count(source: &Source) -> usize {
        match *source {}
    }

    pub(super) fn frame(source: &Source, _: usize) -> Result<OwnedPageImage, ImageError> {
        match *source {}
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::rc::autoreleasepool;
    use objc2_core_foundation::{
        CFBoolean, CFData, CFDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect,
        CGSize,
    };
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
        CGImageByteOrderInfo,
    };
    use objc2_image_io::{
        CGImageSource, kCGImagePropertyDPIWidth, kCGImagePropertyPixelWidth,
        kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform,
        kCGImageSourceThumbnailMaxPixelSize,
    };

    use super::{ImageError, OwnedPageImage};
    use crate::OCR_MAX_SIDE_PX;

    /// Recorded when the file states no resolution; nothing decides on it.
    const DEFAULT_DPI: u32 = 72;

    pub(super) struct Source(CFRetained<CGImageSource>);

    pub(super) fn open(bytes: &[u8]) -> Result<Source, ImageError> {
        let data = CFData::from_bytes(bytes);
        // SAFETY: `options` is None; `data` is a CFData we own and the source
        // retains it.
        let source = unsafe { CGImageSource::with_data(&data, None) }
            .ok_or_else(|| ImageError::Malformed("not a picture ImageIO can open".into()))?;
        let source = Source(source);
        if count(&source) == 0 {
            return Err(ImageError::Malformed("the picture has no frames".into()));
        }
        // Measured: JPEG magic followed by garbage is a source of one frame
        // with status Complete — but no pixel size, which no real frame lacks.
        // SAFETY: reading an immutable CFString constant ImageIO exports.
        if number(&source, 0, unsafe { kCGImagePropertyPixelWidth }).is_none() {
            return Err(ImageError::Malformed(
                "the first frame has no pixel size".into(),
            ));
        }
        Ok(source)
    }

    pub(super) fn count(source: &Source) -> usize {
        // SAFETY: a plain getter on a source we own.
        unsafe { source.0.count() }
    }

    fn thumbnail_options() -> CFRetained<CFDictionary> {
        let yes: &CFType = CFBoolean::new(true).as_ref();
        let side = CFNumber::new_i32(OCR_MAX_SIDE_PX as i32);
        // SAFETY: reading three immutable CFString constants ImageIO exports.
        let keys: [&CFString; 3] = unsafe {
            [
                kCGImageSourceCreateThumbnailFromImageAlways,
                kCGImageSourceCreateThumbnailWithTransform,
                kCGImageSourceThumbnailMaxPixelSize,
            ]
        };
        let typed =
            CFDictionary::<CFString, CFType>::from_slices(&keys, &[yes, yes, side.as_ref()]);
        // SAFETY: erasing the key and value types of a dictionary to the
        // untyped one ImageIO takes; nothing is read back through the cast.
        unsafe { CFRetained::cast_unchecked(typed) }
    }

    /// The number under `key` in frame `index`'s properties, if there is one.
    fn number(source: &Source, index: usize, key: &CFString) -> Option<f64> {
        // SAFETY: `options` is None; ImageIO returns None for an index out of
        // range.
        let props = unsafe { source.0.properties_at_index(index, None) }?;
        // SAFETY: ImageIO's property dictionaries have CFString keys; values
        // are only read as CFType and downcast with a type check.
        let props: CFRetained<CFDictionary<CFString, CFType>> =
            unsafe { CFRetained::cast_unchecked(props) };
        props.get(key)?.downcast::<CFNumber>().ok()?.as_f64()
    }

    /// `kCGImagePropertyDPIWidth` of frame `index`, when stated and positive.
    fn dpi(source: &Source, index: usize) -> Option<u32> {
        // SAFETY: reading an immutable CFString constant ImageIO exports.
        let dpi = number(source, index, unsafe { kCGImagePropertyDPIWidth })?;
        (dpi >= 1.0).then(|| dpi.round() as u32)
    }

    pub(super) fn frame(source: &Source, index: usize) -> Result<OwnedPageImage, ImageError> {
        let frames = count(source);
        if index >= frames {
            return Err(ImageError::Malformed(format!(
                "frame {index} asked of a picture with {frames}"
            )));
        }
        // Per call, as in `ocr.rs`: a worker decodes many frames on one thread
        // whose outermost pool is never drained.
        autoreleasepool(|_| {
            // SAFETY: the options dictionary holds the keys and value types
            // ImageIO documents for them (two CFBooleans, one CFNumber).
            let image = unsafe {
                source
                    .0
                    .thumbnail_at_index(index, Some(&thumbnail_options()))
            }
            .ok_or_else(|| ImageError::Malformed(format!("frame {index} did not decode")))?;
            let (w, h) = (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
            if w == 0 || h == 0 {
                return Err(ImageError::Malformed(format!("frame {index} is {w}x{h}")));
            }
            let stride = w * 4;
            let mut bgra = vec![0u8; stride * h];
            let space = CGColorSpace::new_device_rgb()
                .ok_or_else(|| ImageError::Malformed("CGColorSpaceCreateDeviceRGB".into()))?;
            let info =
                CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
            // SAFETY: `bgra` is exactly `stride * h` bytes, 8 bits per
            // component and 4 components per pixel as `info` states, and it
            // outlives `ctx`, which is dropped before `bgra` is moved below.
            let ctx = unsafe {
                CGBitmapContextCreate(
                    bgra.as_mut_ptr().cast(),
                    w,
                    h,
                    8,
                    stride,
                    Some(&space),
                    info,
                )
            }
            .ok_or_else(|| ImageError::Malformed("CGBitmapContextCreate".into()))?;
            let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64));
            // White first: transparent pixels would otherwise stay 0 — black.
            CGContext::set_rgb_fill_color(Some(&ctx), 1.0, 1.0, 1.0, 1.0);
            CGContext::fill_rect(Some(&ctx), rect);
            CGContext::draw_image(Some(&ctx), rect, Some(&image));
            drop(ctx);
            Ok(OwnedPageImage {
                bgra,
                width: w as u32,
                height: h as u32,
                stride,
                dpi: dpi(source, index).unwrap_or(DEFAULT_DPI),
            })
        })
    }
}
