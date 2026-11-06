//! `extract_pdf_pages_with`: pages without a text layer go to an OCR backend
//! that may answer out of order, and still reach the caller in page order
//! (D196). The backends here are fakes, so this runs without Vision — on Linux
//! CI too.

use std::collections::VecDeque;

use mnema_extract::{
    OcrBackend, OcrError, OwnedPageImage, PdfError, Recognised, TEXT_SOURCE_NATIVE,
    TEXT_SOURCE_OCR, extract_pdf_pages_with,
};

const INTERLEAVED: &[u8] = include_bytes!("fixtures/interleaved-native-scan.pdf");
const ALL_SCANNED: &[u8] = include_bytes!("fixtures/all-scanned.pdf");

/// Text long enough to clear `TEXT_LAYER_MIN_CHARS`, and different per page.
fn read_text(page_no: u32) -> String {
    format!("recognised words of scanned page number {page_no}, enough of them to count as text")
}

type Answer = fn(u32) -> Result<Option<Recognised>, OcrError>;

fn recognised(page_no: u32) -> Result<Option<Recognised>, OcrError> {
    Ok(Some(Recognised {
        text: read_text(page_no),
        confidence: 0.5,
        width_px: 10,
        height_px: 10,
        dpi: 300,
    }))
}

/// Holds up to `capacity` pages and answers the most recent first.
struct Fake {
    capacity: usize,
    held: VecDeque<u32>,
    answer: Answer,
    /// Added to the seq of every reply: non-zero is a backend answering a
    /// page it was not asked about.
    seq_shift: u32,
    most_in_flight: usize,
    submitted: Vec<u32>,
}

impl Fake {
    fn new(capacity: usize, answer: Answer) -> Self {
        Fake {
            capacity,
            held: VecDeque::new(),
            answer,
            seq_shift: 0,
            most_in_flight: 0,
            submitted: Vec::new(),
        }
    }
}

impl OcrBackend for Fake {
    fn capacity(&self) -> usize {
        self.capacity
    }
    fn submit(&mut self, seq: u32, image: OwnedPageImage) {
        assert!(image.width > 0 && image.height > 0, "a rendered image");
        self.held.push_back(seq);
        self.submitted.push(seq);
        self.most_in_flight = self.most_in_flight.max(self.held.len());
    }
    fn next(&mut self) -> (u32, Result<Option<Recognised>, OcrError>) {
        let seq = self
            .held
            .pop_back()
            .expect("next() only with a page in flight");
        (seq + self.seq_shift, (self.answer)(seq))
    }
}

/// `(page_no, text_source, text)` of every page handed over, and `skipped`.
type Read = (Vec<(u32, &'static str, String)>, Vec<u32>);

fn read(bytes: &[u8], backend: &mut Fake) -> Result<Read, PdfError> {
    let mut pages = Vec::new();
    let skipped = extract_pdf_pages_with(bytes, backend, |_, page| {
        pages.push((
            page.page_no,
            page.text_source,
            page.blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<String>(),
        ));
    })?;
    Ok((pages, skipped))
}

#[test]
fn pages_answered_out_of_order_reach_the_caller_in_page_order() {
    let mut backend = Fake::new(3, recognised);
    let (pages, skipped) = read(INTERLEAVED, &mut backend).unwrap();
    let numbers: Vec<u32> = pages.iter().map(|p| p.0).collect();
    assert_eq!(numbers, [1, 2, 3, 4, 5, 6, 7]);
    assert!(skipped.is_empty());
    for (page_no, source, text) in &pages {
        if [2, 3, 5, 6].contains(page_no) {
            assert_eq!(*source, TEXT_SOURCE_OCR, "page {page_no}");
            assert_eq!(
                *text,
                read_text(*page_no),
                "page {page_no} carries its own text"
            );
        } else {
            assert_eq!(*source, TEXT_SOURCE_NATIVE, "page {page_no}");
        }
    }
    assert_eq!(
        backend.submitted,
        [2, 3, 5, 6],
        "only textless pages go to OCR"
    );

    // And the same pages as a backend that holds one at a time.
    let mut one = Fake::new(1, recognised);
    assert_eq!(read(INTERLEAVED, &mut one).unwrap(), (pages, skipped));
}

#[test]
fn never_more_pages_in_flight_than_the_backend_holds() {
    for capacity in [1, 2, 3] {
        let mut backend = Fake::new(capacity, recognised);
        read(ALL_SCANNED, &mut backend).unwrap();
        assert_eq!(
            backend.most_in_flight, capacity,
            "three textless pages, capacity {capacity}: filled, never exceeded"
        );
    }
}

/// A page Vision finds nothing on — or no engine — is skipped, and the
/// skipped list is in page order however the answers arrived.
#[test]
fn pages_with_nothing_recognised_are_skipped_in_page_order() {
    fn nothing_on_three_and_six(page_no: u32) -> Result<Option<Recognised>, OcrError> {
        match page_no {
            3 => Ok(None),
            6 => Ok(Some(Recognised {
                text: "too short".into(),
                confidence: 0.1,
                width_px: 1,
                height_px: 1,
                dpi: 300,
            })),
            n => recognised(n),
        }
    }
    let mut backend = Fake::new(3, nothing_on_three_and_six);
    let (pages, skipped) = read(INTERLEAVED, &mut backend).unwrap();
    assert_eq!(
        pages.iter().map(|p| p.0).collect::<Vec<_>>(),
        [1, 2, 4, 5, 7]
    );
    assert_eq!(skipped, [3, 6]);
}

#[test]
fn an_answer_for_a_page_not_asked_about_refuses_the_document() {
    let mut backend = Fake::new(2, recognised);
    backend.seq_shift = 1000;
    match read(INTERLEAVED, &mut backend) {
        Err(PdfError::Malformed(message)) => {
            assert!(message.contains("1003"), "names the page: {message}")
        }
        other => panic!("expected Malformed, got {other:?}"),
    }
}

/// An engine failure is the document's failure, at the page it happened on:
/// every page before it was handed over, none after it.
#[test]
fn an_engine_failure_stops_at_its_page() {
    fn fails_on_five(page_no: u32) -> Result<Option<Recognised>, OcrError> {
        if page_no == 5 {
            Err(OcrError::Failed("vision said no".into()))
        } else {
            recognised(page_no)
        }
    }
    let mut pages = Vec::new();
    let mut backend = Fake::new(3, fails_on_five);
    let result = extract_pdf_pages_with(INTERLEAVED, &mut backend, |_, page| {
        pages.push(page.page_no)
    });
    assert!(
        matches!(result, Err(PdfError::Ocr(OcrError::Failed(_)))),
        "{result:?}"
    );
    assert_eq!(pages, [1, 2, 3, 4]);
}
