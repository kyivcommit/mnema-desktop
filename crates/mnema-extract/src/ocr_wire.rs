//! The wire between the worker and an `--ocr` helper process (D196).
//!
//! **Request:** one JSON line `{"seq","width","height","stride","dpi","len"}`,
//! then exactly `len` bytes of BGRA rows. **Reply:** one JSON line carrying the
//! same `seq` and one of `ok` / `none` / `unavailable` / `failed` — the four
//! answers [`crate::recognise`] can give.
//!
//! **An image, not a page number.** The helper never opens the file: the worker
//! has read its bytes once and hashed them, and the file on disk may have
//! changed since. Text recognised from other bytes, stored under the digest of
//! these, would be text the indexed file does not contain.
//!
//! The header is checked before any pixel is read: a helper allocates only
//! what a render could have produced, whatever the line says.

use std::io::{self, BufRead, Read, Write};

use serde::{Deserialize, Serialize};

use crate::{OcrError, OwnedPageImage, PageImage, Recognised};

/// The longest side an image on this wire may have. A render's long side is at
/// most `OCR_MAX_SIDE_PX` (4000); the rest is margin, not a second limit.
pub const MAX_IMAGE_SIDE: u32 = 4096;

/// The most pixel bytes one request may carry: a [`MAX_IMAGE_SIDE`] square,
/// with room for pdfium's row padding.
pub const MAX_IMAGE_BYTES: u64 = (MAX_IMAGE_SIDE as u64 * 4 + 256) * MAX_IMAGE_SIDE as u64;

/// Set to `k`, an `--ocr` helper aborts after its `k`-th reply, the way a
/// crashed process dies. For tests of the worker's fallback only; read once per
/// helper.
pub const OCR_CRASH_AFTER_ENV: &str = "MNEMA_OCR_CRASH_AFTER";

/// Set to a number of milliseconds, an `--ocr` helper waits that long before
/// recognising each page — a page that takes as long as a cold start does. For
/// the test that a helper does not outlive a killed worker only.
pub const OCR_STALL_MS_ENV: &str = "MNEMA_OCR_STALL_MS";

/// Set to `k`, an `--ocr` helper answers every page after its `k`-th with
/// the previous page's seq and reply — a helper that has lost track of which
/// page it holds. For the test of the worker's seq check only.
pub const OCR_STALE_AFTER_ENV: &str = "MNEMA_OCR_STALE_AFTER";

/// A request header line longer than this is not a header.
const MAX_HEADER_LINE: u64 = 64 * 1024;

/// A reply line longer than this is not a reply. Far above any page's text (a
/// dense A4 page is ~5 k characters), so a reply is never cut for its length:
/// a refused reply retires the helper for the rest of the file.
const MAX_REPLY_LINE: u64 = 4 * 1024 * 1024;

/// What the engine made of one image — [`crate::recognise`]'s result.
pub type Reply = Result<Option<Recognised>, OcrError>;

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("OCR wire I/O: {0}")]
    Io(#[from] io::Error),
    #[error("OCR wire: {0}")]
    Malformed(String),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    seq: u32,
    width: u32,
    height: u32,
    stride: u64,
    dpi: u32,
    len: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRecognised {
    text: String,
    confidence: f32,
    width_px: u32,
    height_px: u32,
    dpi: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Answer {
    Ok(WireRecognised),
    None(bool),
    Unavailable(String),
    Failed(String),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireReply {
    seq: u32,
    #[serde(flatten)]
    answer: Answer,
}

/// Writes one request: the header line, then the `stride * height` bytes of
/// rows — not whatever lies past them in `image.bgra`. Nothing is written for
/// a buffer shorter than its geometry.
pub fn write_request(w: &mut impl Write, seq: u32, image: &PageImage) -> io::Result<()> {
    let len = image
        .stride
        .checked_mul(image.height as usize)
        .filter(|&len| len <= image.bgra.len())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{}x{} stride {} does not fit {} bytes",
                    image.width,
                    image.height,
                    image.stride,
                    image.bgra.len()
                ),
            )
        })?;
    let header = Header {
        seq,
        width: image.width,
        height: image.height,
        stride: image.stride as u64,
        dpi: image.dpi,
        len: len as u64,
    };
    let mut line = serde_json::to_vec(&header).map_err(io::Error::other)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.write_all(&image.bgra[..len])
}

/// Reads one request. `Ok(None)` is a clean end — EOF where the next header
/// would start; EOF anywhere inside a frame is `Io`.
pub fn read_request(r: &mut impl BufRead) -> Result<Option<(u32, OwnedPageImage)>, WireError> {
    let Some(line) = read_line(r, MAX_HEADER_LINE)? else {
        return Ok(None);
    };
    let h: Header = serde_json::from_str(&line)
        .map_err(|e| WireError::Malformed(format!("not a request header: {e}")))?;
    let fits = h.width > 0
        && h.height > 0
        && h.width <= MAX_IMAGE_SIDE
        && h.height <= MAX_IMAGE_SIDE
        && h.stride >= u64::from(h.width) * 4
        && h.stride.checked_mul(u64::from(h.height)) == Some(h.len)
        && h.len <= MAX_IMAGE_BYTES;
    if !fits {
        return Err(WireError::Malformed(format!(
            "{}x{} stride {} len {} is not an image this wire carries",
            h.width, h.height, h.stride, h.len
        )));
    }
    let mut bgra = vec![0; h.len as usize];
    r.read_exact(&mut bgra)?;
    Ok(Some((
        h.seq,
        OwnedPageImage {
            bgra,
            width: h.width,
            height: h.height,
            stride: h.stride as usize,
            dpi: h.dpi,
        },
    )))
}

pub fn write_reply(w: &mut impl Write, seq: u32, reply: &Reply) -> io::Result<()> {
    let answer = match reply {
        Ok(Some(r)) => Answer::Ok(WireRecognised {
            text: r.text.clone(),
            confidence: r.confidence,
            width_px: r.width_px,
            height_px: r.height_px,
            dpi: r.dpi,
        }),
        Ok(None) => Answer::None(true),
        Err(OcrError::Unavailable(m)) => Answer::Unavailable(m.clone()),
        Err(OcrError::Failed(m)) => Answer::Failed(m.clone()),
    };
    let mut line = serde_json::to_vec(&WireReply { seq, answer }).map_err(io::Error::other)?;
    line.push(b'\n');
    w.write_all(&line)
}

/// Reads one reply; `Ok(None)` is EOF where the next one would start.
pub fn read_reply(r: &mut impl BufRead) -> Result<Option<(u32, Reply)>, WireError> {
    let Some(line) = read_line(r, MAX_REPLY_LINE)? else {
        return Ok(None);
    };
    let WireReply { seq, answer } = serde_json::from_str(&line)
        .map_err(|e| WireError::Malformed(format!("not a reply: {e}")))?;
    let reply = match answer {
        Answer::Ok(r) => Ok(Some(Recognised {
            text: r.text,
            confidence: r.confidence,
            width_px: r.width_px,
            height_px: r.height_px,
            dpi: r.dpi,
        })),
        Answer::None(_) => Ok(None),
        Answer::Unavailable(m) => Err(OcrError::Unavailable(m)),
        Answer::Failed(m) => Err(OcrError::Failed(m)),
    };
    Ok(Some((seq, reply)))
}

/// One `\n`-terminated line of at most `max` bytes; `None` at EOF before its
/// first byte.
fn read_line(r: &mut impl BufRead, max: u64) -> Result<Option<String>, WireError> {
    let mut line = String::new();
    let n = r.take(max).read_line(&mut line).map_err(|e| {
        if e.kind() == io::ErrorKind::InvalidData {
            WireError::Malformed(format!("a line is not UTF-8: {e}"))
        } else {
            WireError::Io(e)
        }
    })?;
    if n == 0 {
        return Ok(None);
    }
    if !line.ends_with('\n') {
        return Err(if n as u64 == max {
            WireError::Malformed(format!("a line longer than {max} bytes"))
        } else {
            WireError::Io(io::ErrorKind::UnexpectedEof.into())
        });
    }
    Ok(Some(line))
}
