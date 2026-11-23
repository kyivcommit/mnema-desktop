//! `image::open` / `Frames::frame` on the fixtures: every frame comes out as
//! the BGRA bitmap Vision reads, upright, on white, under the size cap.

#[cfg(not(target_os = "macos"))]
#[test]
fn there_is_no_decoder_off_macos() {
    let bytes = std::fs::read(fixture("image-uk.png")).unwrap();
    assert!(
        matches!(
            mnema_extract::open_image(&bytes),
            Err(mnema_extract::ImageError::Unsupported(_))
        ),
        "expected Unsupported"
    );
}

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[cfg(target_os = "macos")]
mod macos {
    use mnema_extract::{
        Frames, ImageError, OCR_MAX_SIDE_PX, OwnedPageImage, open_image, recognise,
    };

    use super::fixture;

    fn open(name: &str) -> Frames {
        open_image(&std::fs::read(fixture(name)).unwrap()).expect("the fixture opens")
    }

    fn first(name: &str) -> OwnedPageImage {
        open(name).frame(0).expect("frame 0 decodes")
    }

    fn assert_bgra(img: &OwnedPageImage, what: &str) {
        let (w, h) = (img.width as usize, img.height as usize);
        assert!(w > 0 && h > 0, "{what}: {w}x{h}");
        assert!(
            img.stride >= w * 4,
            "{what}: stride {} for width {w}",
            img.stride
        );
        assert!(
            img.bgra.len() >= img.stride * h,
            "{what}: {} bytes for stride {} x {h}",
            img.bgra.len(),
            img.stride
        );
    }

    #[test]
    fn every_frame_of_every_fixture_is_bgra() {
        for name in [
            "image-uk.png",
            "image-uk.jpg",
            "image-uk.heic",
            "image-uk.webp",
            "image-uk-gray.png",
            "image-uk-transparent.png",
            "image-uk-exif6.jpg",
            "image-blank.jpg",
            "image-three-frames.tif",
        ] {
            let frames = open(name);
            for i in 0..frames.count() {
                assert_bgra(&frames.frame(i).unwrap(), &format!("{name} frame {i}"));
            }
        }
    }

    #[test]
    fn a_transparent_picture_is_drawn_on_white() {
        let img = first("image-uk-transparent.png");
        let last_row = img.stride * (img.height as usize - 1);
        let last_px = last_row + (img.width as usize - 1) * 4;
        for at in [0, (img.width as usize - 1) * 4, last_row, last_px] {
            assert_eq!(img.bgra[at..at + 4], [0xFF; 4], "pixel at byte {at}");
        }
    }

    /// Red lands in the third byte, blue in the first: every other fixture is
    /// black, white or gray, which reads the same in either channel order.
    /// `image-blank.jpg` has a red ellipse centred at (710, 450) from the
    /// bottom of a 1000x750 picture: red 0.8 over the gradient, about
    /// R 195, B 70.
    #[test]
    fn a_red_pixel_is_decoded_into_bgra_order() {
        let img = first("image-blank.jpg");
        assert_eq!((img.width, img.height), (1000, 750));
        let at = img.stride * (750 - 1 - 450) + 710 * 4;
        let [b, _, r, a] = [
            img.bgra[at],
            img.bgra[at + 1],
            img.bgra[at + 2],
            img.bgra[at + 3],
        ];
        assert_eq!(a, 0xFF);
        assert!(r > b + 80, "red {r} over blue {b} at the ellipse's centre");
    }

    #[test]
    fn exif_orientation_is_applied() {
        let img = first("image-uk-exif6.jpg");
        assert_eq!((img.width, img.height), (2479, 1166));
    }

    #[test]
    fn a_multi_frame_tiff_gives_each_frame_its_own_size() {
        let frames = open("image-three-frames.tif");
        assert_eq!(frames.count(), 3);
        let sizes: Vec<_> = (0..3)
            .map(|i| {
                let f = frames.frame(i).unwrap();
                (f.width, f.height)
            })
            .collect();
        assert_eq!(sizes, [(2479, 1166), (1000, 750), (2479, 1166)]);
        assert!(
            matches!(frames.frame(3), Err(ImageError::Malformed(_))),
            "{:?}",
            frames.frame(3).map(|f| (f.width, f.height))
        );
    }

    /// An animation is one page: its frames repeat the picture, not continue it.
    #[test]
    fn an_animated_png_is_its_first_frame_only() {
        let frames = open("image-animated.png");
        assert_eq!(frames.count(), 1);
        assert!(frames.frame(0).is_ok());
        assert!(matches!(frames.frame(1), Err(ImageError::Malformed(_))));
    }

    #[test]
    fn bytes_that_are_not_a_picture_are_malformed() {
        let binary = std::fs::read(fixture("binary.bin")).unwrap();
        let jpeg_magic_then_garbage = [0xFF, 0xD8, 0xFF, 0, 0, 0, 0];
        for (what, bytes) in [
            ("binary.bin", &binary[..]),
            ("jpeg magic", &jpeg_magic_then_garbage[..]),
        ] {
            assert!(
                matches!(open_image(bytes), Err(ImageError::Malformed(_))),
                "{what}: {:?}",
                open_image(bytes).map(|f| f.count())
            );
        }
    }

    #[test]
    fn a_picture_larger_than_the_cap_is_scaled_to_it() {
        let (w, h) = (5000u32, 100u32);
        let mut png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png, w, h);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().unwrap();
            writer
                .write_image_data(&vec![0xFF; (w * h) as usize])
                .unwrap();
        }
        let img = open_image(&png).unwrap().frame(0).unwrap();
        assert_eq!(img.width.max(img.height), OCR_MAX_SIDE_PX as u32);
        assert_eq!(img.height, 80, "aspect kept");
    }

    // The one call into Vision: it pays the cold start (`ocr.rs`). Words with
    // `ґ` or `’` are not asserted — Vision changes them.
    #[test]
    fn a_gray_picture_reads_as_text_after_the_redraw() {
        let img = first("image-uk-gray.png");
        let text = recognise(&img.as_image()).unwrap().expect("engine").text;
        for word in ["їжак", "єнот", "collegium"] {
            assert!(text.contains(word), "{word} not in {text:?}");
        }
    }
}
