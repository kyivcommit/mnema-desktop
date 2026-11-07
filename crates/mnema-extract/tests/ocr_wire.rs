//! The wire between the worker and an `--ocr` helper process: a page image
//! one way, what the engine made of it the other. Everything here runs without
//! Vision, so it holds on Linux CI too.

use std::io::{BufReader, Cursor};

use mnema_extract::ocr_wire::{
    MAX_IMAGE_BYTES, MAX_IMAGE_SIDE, Reply, WireError, read_reply, read_request, write_reply,
    write_request,
};
use mnema_extract::{OcrError, OwnedPageImage, Recognised};

fn image(width: u32, height: u32, pad: usize) -> OwnedPageImage {
    let stride = width as usize * 4 + pad;
    OwnedPageImage {
        bgra: (0..stride * height as usize)
            .map(|i| (i % 251) as u8)
            .collect(),
        width,
        height,
        stride,
        dpi: 300,
    }
}

fn reader(bytes: Vec<u8>) -> BufReader<Cursor<Vec<u8>>> {
    BufReader::new(Cursor::new(bytes))
}

#[test]
fn a_request_comes_back_as_the_same_image_under_the_same_seq() {
    let sent = image(7, 5, 4);
    let mut bytes = Vec::new();
    write_request(&mut bytes, 42, &sent.as_image()).unwrap();
    let (seq, got) = read_request(&mut reader(bytes))
        .unwrap()
        .expect("one frame");
    assert_eq!(seq, 42);
    assert_eq!(got, sent);
}

#[test]
fn two_requests_in_a_row_arrive_in_order_and_then_a_clean_end() {
    let (a, b) = (image(3, 2, 0), image(4, 3, 8));
    let mut bytes = Vec::new();
    write_request(&mut bytes, 1, &a.as_image()).unwrap();
    write_request(&mut bytes, 2, &b.as_image()).unwrap();
    let mut r = reader(bytes);
    assert_eq!(read_request(&mut r).unwrap(), Some((1, a)));
    assert_eq!(read_request(&mut r).unwrap(), Some((2, b)));
    assert_eq!(
        read_request(&mut r).unwrap(),
        None,
        "EOF between frames is the end of the work, not an error"
    );
}

/// A buffer longer than `stride * height` (allowed by `PageImage`) sends only
/// the rows: the receiver checks the length against the geometry exactly.
#[test]
fn only_the_rows_the_geometry_names_are_sent() {
    let mut sent = image(3, 2, 0);
    let rows = sent.bgra.clone();
    // Not 9 (a tab): JSON would skip leading whitespace before the next header.
    sent.bgra.extend_from_slice(b"tail!");
    let mut bytes = Vec::new();
    write_request(&mut bytes, 7, &sent.as_image()).unwrap();
    let next = image(2, 2, 0);
    write_request(&mut bytes, 8, &next.as_image()).unwrap();
    let mut r = reader(bytes);
    let (_, got) = read_request(&mut r).unwrap().unwrap();
    assert_eq!(got.bgra, rows);
    // A tail sent after the rows would be where the next header is read from.
    assert_eq!(read_request(&mut r).unwrap(), Some((8, next)));
}

#[test]
fn a_buffer_shorter_than_its_geometry_is_not_sent() {
    let mut short = image(3, 2, 0);
    short.bgra.pop();
    let mut bytes = Vec::new();
    assert!(write_request(&mut bytes, 1, &short.as_image()).is_err());
    assert!(bytes.is_empty(), "nothing half-written");
}

#[test]
fn eof_inside_a_frame_is_an_error_not_an_end() {
    let mut bytes = Vec::new();
    write_request(&mut bytes, 1, &image(5, 5, 0).as_image()).unwrap();
    bytes.truncate(bytes.len() - 3);
    assert!(matches!(
        read_request(&mut reader(bytes)),
        Err(WireError::Io(_))
    ));
}

/// The header is checked before a single pixel is read: a helper must not be
/// made to allocate what a corrupt or hostile line names.
#[test]
fn a_header_that_does_not_add_up_is_refused_before_reading_the_pixels() {
    let cases = [
        // len is not stride * height
        r#"{"seq":1,"width":2,"height":2,"stride":8,"dpi":300,"len":15}"#.to_string(),
        // stride narrower than a row of BGRA
        r#"{"seq":1,"width":2,"height":2,"stride":7,"dpi":300,"len":14}"#.to_string(),
        // zero-sized
        r#"{"seq":1,"width":0,"height":2,"stride":0,"dpi":300,"len":0}"#.to_string(),
        // a side past the ceiling
        format!(
            r#"{{"seq":1,"width":{w},"height":1,"stride":{s},"dpi":300,"len":{s}}}"#,
            w = MAX_IMAGE_SIDE + 1,
            s = (MAX_IMAGE_SIDE + 1) * 4
        ),
        // len past the ceiling with a geometry that adds up
        format!(
            r#"{{"seq":1,"width":{MAX_IMAGE_SIDE},"height":{MAX_IMAGE_SIDE},"stride":{s},"dpi":300,"len":{l}}}"#,
            s = MAX_IMAGE_SIDE * 4 + 512,
            l = (MAX_IMAGE_SIDE as u64 * 4 + 512) * MAX_IMAGE_SIDE as u64
        ),
        "not json".to_string(),
    ];
    for header in cases {
        // No pixel bytes follow: a reader that trusted the header would fail
        // with Io (EOF) instead of Malformed.
        let bytes = format!("{header}\n").into_bytes();
        assert!(
            matches!(
                read_request(&mut reader(bytes)),
                Err(WireError::Malformed(_))
            ),
            "{header}"
        );
    }
    // And the ceiling admits the largest image a render can produce.
    let biggest = (MAX_IMAGE_SIDE as u64 * 4) * MAX_IMAGE_SIDE as u64;
    assert!(biggest <= MAX_IMAGE_BYTES);
}

#[test]
fn every_reply_comes_back_as_itself() {
    let replies: Vec<Reply> = vec![
        Ok(Some(Recognised {
            text: "Київ\nрядок другий".into(),
            confidence: 0.912_345_7,
            width_px: 2479,
            height_px: 3508,
            dpi: 300,
        })),
        Ok(None),
        Err(OcrError::Unavailable("no uk-UA".into())),
        Err(OcrError::Failed("vision said no".into())),
    ];
    let mut bytes = Vec::new();
    for (seq, reply) in replies.iter().enumerate() {
        write_reply(&mut bytes, seq as u32 + 10, reply).unwrap();
    }
    let mut r = reader(bytes);
    for (seq, reply) in replies.iter().enumerate() {
        assert_eq!(
            read_reply(&mut r).unwrap(),
            Some((seq as u32 + 10, reply.clone()))
        );
    }
    assert_eq!(read_reply(&mut r).unwrap(), None);
}

#[test]
fn a_reply_that_is_not_one_is_malformed() {
    for line in ["{}", r#"{"seq":1}"#, r#"{"seq":1,"what":2}"#, "garbage"] {
        let bytes = format!("{line}\n").into_bytes();
        assert!(
            matches!(read_reply(&mut reader(bytes)), Err(WireError::Malformed(_))),
            "{line}"
        );
    }
}

/// A page with far more text than a dense A4 still comes back as itself: a
/// reply refused for its length would retire the helper for the rest of the
/// file (review of PR #92: 80 KB was refused at the header limit).
#[test]
fn a_reply_longer_than_a_header_may_be_comes_back_as_itself() {
    let reply: Reply = Ok(Some(Recognised {
        text: "слово ".repeat(40_000),
        confidence: 0.5,
        width_px: 2480,
        height_px: 3508,
        dpi: 300,
    }));
    let mut bytes = Vec::new();
    write_reply(&mut bytes, 7, &reply).unwrap();
    assert!(bytes.len() > 128 * 1024, "{} bytes", bytes.len());
    assert_eq!(read_reply(&mut reader(bytes)).unwrap(), Some((7, reply)));

    // A request header of that length is still not a header: refused at the
    // header limit (Malformed), not read on until the stream ends (Io).
    let long = " ".repeat(128 * 1024).into_bytes();
    assert!(matches!(
        read_request(&mut reader(long)),
        Err(WireError::Malformed(_))
    ));
}
