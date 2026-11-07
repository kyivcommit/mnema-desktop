//! One PDF page drawn for the window's page view, with the cited passage boxed.
//!
//! The picture comes from `mnema-extract-worker --render-page` (the application
//! may not link the crate that draws it, D40), and **the file it draws is found
//! only through the index**: the window names a document and a place, and the
//! place is resolved the way the freshness verdict resolves it. A path from the
//! window is never opened.
//!
//! What the boxes are worth is decided here, before the worker is asked: the
//! offsets of a citation were counted in the text the reader stored, so the
//! worker gets the hash of that text and draws boxes only when the file still
//! reads the same. A scan page has no text layer, so the worker recognises it
//! again and judges the stored text against that. Where no stored text exists
//! (or a path now names another document) the page is drawn alone and the
//! window is told why.

use std::path::Path;
use std::time::Duration;

use mnema_core::render::{
    RenderError, RenderRange, RenderRect, RenderReply, RenderRequest, TextMatch,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;

use crate::error::Error;
use crate::state::AppState;
use crate::tree::cited_occupant;
use mnema_pool::PoolConfig;

/// The widest page asked of the worker, whatever the window sends: a PNG of a
/// page is width x height x 4 bytes in the worker before it is encoded.
const MAX_WIDTH_PX: u32 = 3000;

/// How long the worker may take to draw one page before it is killed.
const RENDER_TIMEOUT: Duration = Duration::from_secs(10);

/// The same for a scan page, which the worker recognises again. Vision's cold
/// start under a new binary path takes a minute and a process killed during it
/// leaves the path cold, so a shorter deadline would never box a scan. It is
/// `PoolConfig::timeout`'s number, and a test keeps the two equal.
pub(crate) const RENDER_TIMEOUT_SCAN: Duration = Duration::from_secs(300);

/// One stretch of the page's stored text to box: the window's `PaneSpan` for
/// the page's block, in the unit `RenderRange` counts (Unicode scalar values).
/// Other fields of the span (`blockId`) are ignored.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeArg {
    pub block_start: u32,
    pub length: u32,
    pub primary: bool,
}

/// Whether the boxes can be trusted, as the window's note under the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Highlight {
    /// The file still reads as it did when indexed: the boxes are the citation.
    Shown,
    /// The file or the path changed since: the page is drawn without boxes.
    Changed,
    /// The page is a scan: it has no text layer to box.
    Scan,
    /// The file reads as indexed, but the passage could not be placed on the
    /// page (the worker's [`TextMatch::Unplaced`]). Not a change to the file.
    Unplaced,
    /// The index holds no text for this page (the reader skipped it): there is
    /// no citation on it to box, and nothing has changed — no note.
    NoText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PageUnavailable {
    /// The index holds no place for this document, so there is no file to open.
    NoPath,
    FileMissing,
    /// The worker could not draw it. The reason is logged, not shown.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum PdfPageView {
    Page {
        /// Every page the document declares, for "page N of M".
        pages: u32,
        width_px: u32,
        height_px: u32,
        /// A `data:image/png;base64,…` URL, passed through as the worker wrote it.
        image: String,
        highlight: Highlight,
        rects: Vec<RenderRect>,
    },
    Unavailable {
        reason: PageUnavailable,
    },
}

#[allow(clippy::too_many_arguments)]
#[tauri::command(async)]
pub fn render_pdf_page(
    state: State<'_, AppState>,
    document_id: String,
    root_id: Option<i64>,
    relative_path: Option<String>,
    page_no: u32,
    width_px: u32,
    ranges: Vec<RangeArg>,
) -> Result<PdfPageView, Error> {
    render_page_in(
        &state,
        state.worker_path(),
        &document_id,
        root_id,
        relative_path.as_deref(),
        page_no,
        width_px,
        &ranges,
        RENDER_TIMEOUT,
        RENDER_TIMEOUT_SCAN,
    )
}

/// [`render_pdf_page`] with the worker and its deadlines (native page, scan
/// page) given, which is what lets a test point it at a stand-in.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_page_in(
    state: &AppState,
    worker: &Path,
    document_id: &str,
    root_id: Option<i64>,
    relative_path: Option<&str>,
    page_no: u32,
    width_px: u32,
    ranges: &[RangeArg],
    timeout: Duration,
    scan_timeout: Duration,
) -> Result<PdfPageView, Error> {
    let unavailable = |reason| Ok(PdfPageView::Unavailable { reason });

    // The index half in one snapshot; the `stat` and the worker after it.
    let (occupant, stored) = state.with_index(|db| {
        db.read_snapshot(|db| {
            let Some(occupant) = cited_occupant(db, root_id, relative_path)? else {
                return Ok((None, None));
            };
            // The stored text is this document's; a path that now names another
            // one has nothing of ours to compare against.
            let stored = if occupant.current_document_id == document_id {
                Some(db.page_view_text(document_id, page_no)?)
            } else {
                None
            };
            Ok((Some(occupant), stored))
        })
    })?;
    let Some(occupant) = occupant else {
        return unavailable(PageUnavailable::NoPath);
    };
    let full = Path::new(&occupant.root_absolute_path).join(&occupant.relative_path);
    // Only "not found" is a missing file; a permission or I/O error on a file
    // that is there is a failure, and its cause goes to the log.
    match std::fs::metadata(&full) {
        Ok(meta) if meta.len() > PoolConfig::new(worker).pdf_max_bytes => {
            // The worker reads the whole file; ingest would refuse it too.
            eprintln!(
                "mnema: the page view refused {}: {} bytes, over the PDF ceiling",
                full.display(),
                meta.len()
            );
            return unavailable(PageUnavailable::Failed);
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return unavailable(PageUnavailable::FileMissing);
        }
        Err(e) => {
            eprintln!(
                "mnema: the page view could not stat {}: {e}",
                full.display()
            );
            return unavailable(PageUnavailable::Failed);
        }
    }
    let Some(path) = full.to_str() else {
        return unavailable(PageUnavailable::Failed);
    };

    // `fixed` is a verdict reached without asking the file; `scan` pages are
    // asked for with a second recognition.
    let (text_sha256, fixed, scan) = match stored {
        None => (None, Some(Highlight::Changed), false),
        Some(Some((text, source))) => (Some(sha256_hex(&text)), None, source.starts_with("ocr:")),
        // Not `Changed`: nothing was indexed here, so nothing can have moved.
        Some(None) => (None, Some(Highlight::NoText), false),
    };
    let request = RenderRequest {
        path: path.to_string(),
        page_no,
        width_px: width_px.min(MAX_WIDTH_PX),
        text_sha256,
        recognise: scan,
        ranges: ranges
            .iter()
            .map(|r| RenderRange {
                start: r.block_start,
                len: r.length,
                primary: r.primary,
            })
            .collect(),
    };

    match mnema_pool::render_page(worker, &request, if scan { scan_timeout } else { timeout }) {
        Ok(RenderReply::Rendered {
            pages,
            width_px,
            height_px,
            png_base64,
            text,
            rects,
        }) => Ok(PdfPageView::Page {
            pages,
            width_px,
            height_px,
            image: format!("data:image/png;base64,{png_base64}"),
            highlight: fixed.unwrap_or(match text {
                TextMatch::Match => Highlight::Shown,
                TextMatch::Unplaced => Highlight::Unplaced,
                // A scan is judged against a second recognition, never against
                // the file's bytes: a different Vision is not a changed file,
                // and no engine is the old note.
                TextMatch::Mismatch if scan => Highlight::Unplaced,
                TextMatch::None if scan => Highlight::Scan,
                TextMatch::Mismatch | TextMatch::None => Highlight::Changed,
            }),
            rects,
        }),
        Ok(RenderReply::Refused { error, message }) => {
            eprintln!(
                "mnema: the page view was refused ({}): {message}",
                refusal(error)
            );
            unavailable(PageUnavailable::Failed)
        }
        Err(e) => {
            eprintln!("mnema: the page worker failed: {e}");
            unavailable(PageUnavailable::Failed)
        }
    }
}

/// Warms the OCR engine for `worker`'s path when the index holds a scanned page
/// (spec R6): the first recognition under a new binary path costs ~46 s, and
/// paying it here, in the background, keeps it out of the first scan page the
/// person opens. Blocks for as long as the warm-up takes, so the caller runs it
/// on a thread of its own; never panics, and says how it ended on stderr. A
/// warm-up killed by `timeout` leaves the path cold, so `timeout` is the scan
/// deadline, not a shorter one.
pub(crate) fn warm_ocr_if_scanned(state: &AppState, worker: &Path, timeout: Duration) {
    let scanned = state
        .with_index(|db| db.has_scanned_page())
        .unwrap_or(false);
    if !scanned {
        return;
    }
    let began = std::time::Instant::now();
    match mnema_pool::warm_ocr(worker, timeout) {
        Ok(()) => eprintln!("mnema: the OCR engine was warmed in {:?}", began.elapsed()),
        Err(e) => eprintln!("mnema: the OCR warm-up failed: {e}"),
    }
}

fn refusal(error: RenderError) -> &'static str {
    match error {
        RenderError::NotPdf => "not a pdf",
        RenderError::NoSuchPage => "no such page",
        RenderError::Unreadable => "unreadable",
    }
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mnema_core::{Block, BlockType, OnDisk, SourceKind};
    use mnema_index::DocumentStatus;
    use std::path::PathBuf;

    /// sha256("abc"), the standard vector — written out so the test does not
    /// check `sha256_hex` against itself.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    struct Stand {
        _data: tempfile::TempDir,
        root_dir: tempfile::TempDir,
        scripts: tempfile::TempDir,
        state: AppState,
        root: i64,
        /// The request line the stand-in worker was handed. Absent until it runs.
        log: PathBuf,
    }

    /// An index with one root holding `doc.pdf` (on disk, and indexed as
    /// document `h1`, page 1 native with the text "abc"), and the other rows
    /// the cases below move around.
    fn stand() -> Stand {
        let data = tempfile::tempdir().unwrap();
        let root_dir = tempfile::tempdir().unwrap();
        let scripts = tempfile::tempdir().unwrap();
        std::fs::write(root_dir.path().join("doc.pdf"), "%PDF-").unwrap();
        let disk = mnema_walk::stat(&root_dir.path().join("doc.pdf")).unwrap();
        let state = AppState::new(
            data.path().to_path_buf(),
            PathBuf::new(),
            "http://127.0.0.1:1".to_string(),
            format!("mnema-desktop-page-view-test-{}", data.path().display()),
        );
        state.open_index().unwrap();
        let root = state
            .with_index(|db| {
                let root = db.insert_watched_root(root_dir.path().to_str().unwrap())?;
                for (id, source) in [("h1", "native:pdf"), ("h2", "native:pdf")] {
                    db.insert_document(id, "application/pdf", 1, SourceKind::Document)?;
                    let page = db.insert_page(id, 1, source, None)?;
                    db.insert_block(
                        page,
                        &Block {
                            block_type: BlockType::Paragraph,
                            reading_order: 0,
                            language: None,
                            text: "abc".to_string(),
                            line_start: None,
                            line_end: None,
                        },
                    )?;
                    db.set_document_status(id, DocumentStatus::Indexed)?;
                }
                db.insert_path(
                    root,
                    "doc.pdf",
                    "h1",
                    OnDisk {
                        size_bytes: disk.size_bytes,
                        mtime: disk.mtime,
                    },
                    "pdf",
                    1,
                )?;
                Ok(root)
            })
            .unwrap();
        let log = scripts.path().join("request.json");
        Stand {
            _data: data,
            root_dir,
            scripts,
            state,
            root,
            log,
        }
    }

    impl Stand {
        /// A worker that records the line it is handed and answers `reply`.
        fn worker_answering(&self, reply: &RenderReply) -> PathBuf {
            self.script(&format!(
                "printf '%s\\n' '{}'\n",
                serde_json::to_string(reply).unwrap()
            ))
        }

        fn sql(&self, statement: &str) {
            self.state
                .with_index(|db| Ok(db.conn().execute(statement, [])?))
                .unwrap();
        }

        fn script(&self, tail: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let path = self.scripts.path().join("worker");
            std::fs::write(
                &path,
                format!(
                    "#!/bin/sh\nIFS= read -r line || exit 0\nprintf '%s\\n' \"$line\" > '{}'\n{tail}",
                    self.log.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            wait_until_it_will_run(&path);
            assert!(!self.log.exists(), "the pre-run wrote the request log");
            path
        }

        fn render(
            &self,
            worker: &Path,
            document_id: &str,
            path: &str,
            width_px: u32,
        ) -> PdfPageView {
            let ten = Duration::from_secs(10);
            self.render_within(worker, document_id, path, width_px, ten, ten)
        }

        fn render_within(
            &self,
            worker: &Path,
            document_id: &str,
            path: &str,
            width_px: u32,
            native: Duration,
            scan: Duration,
        ) -> PdfPageView {
            let ranges = [RangeArg {
                block_start: 1,
                length: 2,
                primary: true,
            }];
            render_page_in(
                &self.state,
                worker,
                document_id,
                Some(self.root),
                Some(path),
                1,
                width_px,
                &ranges,
                native,
                scan,
            )
            .unwrap()
        }

        fn request(&self) -> RenderRequest {
            serde_json::from_str(&std::fs::read_to_string(&self.log).expect("the worker ran"))
                .unwrap()
        }
    }

    /// Runs the script once before use, retrying while Linux says it is still
    /// open for writing (the same function `scan_job`'s tests carry, for the
    /// same reason).
    fn wait_until_it_will_run(path: &Path) {
        use std::process::{Command, Stdio};
        for _ in 0..400 {
            match Command::new(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(mut child) => {
                    let _ = child.wait();
                    return;
                }
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("the stand-in will not run: {e}"),
            }
        }
        panic!("{} stayed busy", path.display());
    }

    fn rendered(text: TextMatch) -> RenderReply {
        RenderReply::Rendered {
            pages: 9,
            width_px: 800,
            height_px: 1000,
            png_base64: "AAAA".into(),
            text,
            rects: vec![RenderRect {
                x: 0.1,
                y: 0.2,
                w: 0.3,
                h: 0.04,
                primary: true,
            }],
        }
    }

    fn highlight(view: &PdfPageView) -> Highlight {
        match view {
            PdfPageView::Page { highlight, .. } => *highlight,
            other => panic!("not a page: {other:?}"),
        }
    }

    #[test]
    fn a_native_page_that_matches_is_shown_with_the_hash_of_its_stored_text() {
        let s = stand();
        let worker = s.worker_answering(&rendered(TextMatch::Match));

        // Wider than the ceiling, to see it clamped on the wire.
        let view = s.render(&worker, "h1", "doc.pdf", 5000);

        assert_eq!(
            view,
            PdfPageView::Page {
                pages: 9,
                width_px: 800,
                height_px: 1000,
                image: "data:image/png;base64,AAAA".into(),
                highlight: Highlight::Shown,
                rects: vec![RenderRect {
                    x: 0.1,
                    y: 0.2,
                    w: 0.3,
                    h: 0.04,
                    primary: true
                }],
            }
        );
        let sent = s.request();
        assert_eq!(sent.text_sha256.as_deref(), Some(ABC));
        assert!(!sent.recognise, "a native page is not recognised again");
        assert_eq!(sent.width_px, 3000);
        assert_eq!(sent.page_no, 1);
        assert_eq!(
            sent.ranges,
            vec![RenderRange {
                start: 1,
                len: 2,
                primary: true
            }]
        );
        assert_eq!(
            Path::new(&sent.path),
            s.root_dir.path().join("doc.pdf"),
            "the file is the index's path under the index's root"
        );
    }

    #[test]
    fn a_text_that_differs_or_is_absent_is_changed_not_shown() {
        let s = stand();
        for text in [TextMatch::Mismatch, TextMatch::None] {
            let worker = s.worker_answering(&rendered(text));
            assert_eq!(
                highlight(&s.render(&worker, "h1", "doc.pdf", 800)),
                Highlight::Changed,
                "{text:?}"
            );
            std::fs::remove_file(&s.log).unwrap();
        }
    }

    #[test]
    fn a_path_the_index_does_not_hold_is_no_path_and_starts_no_worker() {
        let s = stand();
        let worker = s.worker_answering(&rendered(TextMatch::Match));
        // A file that exists on disk under the root, but that no row names.
        std::fs::write(s.root_dir.path().join("stray.pdf"), "%PDF-").unwrap();

        let view = s.render(&worker, "h1", "stray.pdf", 800);

        assert_eq!(
            view,
            PdfPageView::Unavailable {
                reason: PageUnavailable::NoPath
            }
        );
        assert!(!s.log.exists(), "the worker was started");
    }

    #[test]
    fn a_file_that_is_gone_is_file_missing_and_starts_no_worker() {
        let s = stand();
        let worker = s.worker_answering(&rendered(TextMatch::Match));
        std::fs::remove_file(s.root_dir.path().join("doc.pdf")).unwrap();

        let view = s.render(&worker, "h1", "doc.pdf", 800);

        assert_eq!(
            view,
            PdfPageView::Unavailable {
                reason: PageUnavailable::FileMissing
            }
        );
        assert!(!s.log.exists(), "the worker was started");
    }

    #[test]
    fn a_passage_the_worker_could_not_place_is_unplaced_not_changed() {
        let s = stand();
        let worker = s.worker_answering(&rendered(TextMatch::Unplaced));
        assert_eq!(
            highlight(&s.render(&worker, "h1", "doc.pdf", 800)),
            Highlight::Unplaced
        );
    }

    /// Only "not found" means the file is gone. A path that cannot be stat'ed
    /// for another reason (here a symlink loop, ELOOP) is a failure.
    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_stated_is_failed_not_missing_and_starts_no_worker() {
        let s = stand();
        let worker = s.worker_answering(&rendered(TextMatch::Match));
        let doc = s.root_dir.path().join("doc.pdf");
        std::fs::remove_file(&doc).unwrap();
        std::os::unix::fs::symlink(&doc, &doc).unwrap();

        let view = s.render(&worker, "h1", "doc.pdf", 800);

        assert_eq!(
            view,
            PdfPageView::Unavailable {
                reason: PageUnavailable::Failed
            }
        );
        assert!(!s.log.exists(), "the worker was started");
    }

    /// The worker reads the whole file, so a file over the pool's PDF ceiling
    /// is refused before it is started. Sparse: no disk is spent.
    #[test]
    fn a_file_over_the_pdf_ceiling_is_failed_and_starts_no_worker() {
        let s = stand();
        let worker = s.worker_answering(&rendered(TextMatch::Match));
        let ceiling = PoolConfig::new(&worker).pdf_max_bytes;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(s.root_dir.path().join("doc.pdf"))
            .unwrap();
        file.set_len(ceiling + 1).unwrap();

        assert_eq!(
            s.render(&worker, "h1", "doc.pdf", 800),
            PdfPageView::Unavailable {
                reason: PageUnavailable::Failed
            }
        );
        assert!(!s.log.exists(), "the worker was started");

        // At the ceiling exactly it is drawn.
        file.set_len(ceiling).unwrap();
        assert_eq!(
            highlight(&s.render(&worker, "h1", "doc.pdf", 800)),
            Highlight::Shown
        );
    }

    #[test]
    fn a_path_that_now_names_another_document_is_drawn_bare_and_changed() {
        let s = stand();
        // Even a worker that claimed a match could not make this `shown`.
        let worker = s.worker_answering(&rendered(TextMatch::Match));

        // The window cites h2; the index says doc.pdf now holds h1.
        let view = s.render(&worker, "h2", "doc.pdf", 800);

        assert_eq!(highlight(&view), Highlight::Changed);
        assert_eq!(s.request().text_sha256, None);
    }

    /// Even when the pages are scans: no stored text of this document, nothing
    /// to recognise the page against.
    #[test]
    fn a_path_that_now_names_another_document_is_not_recognised_even_for_a_scan() {
        let s = stand();
        s.sql("UPDATE page SET text_source = 'ocr:vision'");
        let worker = s.worker_answering(&rendered(TextMatch::Match));

        let view = s.render(&worker, "h2", "doc.pdf", 800);

        assert_eq!(highlight(&view), Highlight::Changed);
        let sent = s.request();
        assert_eq!(sent.text_sha256, None);
        assert!(!sent.recognise);
    }

    #[test]
    fn a_scan_page_is_asked_for_with_the_hash_of_its_text_and_a_second_recognition() {
        let s = stand();
        s.sql("UPDATE page SET text_source = 'ocr:vision' WHERE document_id = 'h1'");
        let worker = s.worker_answering(&rendered(TextMatch::Match));

        let view = s.render(&worker, "h1", "doc.pdf", 800);

        assert_eq!(highlight(&view), Highlight::Shown);
        let sent = s.request();
        assert_eq!(sent.text_sha256.as_deref(), Some(ABC));
        assert!(sent.recognise);
    }

    /// `None` and `Mismatch` would be `Changed` for a native page: these are
    /// the answers that tell the two branches apart.
    #[test]
    fn each_answer_for_a_scan_page_has_its_own_note() {
        let s = stand();
        s.sql("UPDATE page SET text_source = 'ocr:vision' WHERE document_id = 'h1'");
        for (text, expected) in [
            (TextMatch::Match, Highlight::Shown),
            (TextMatch::Unplaced, Highlight::Unplaced),
            (TextMatch::None, Highlight::Scan),
            (TextMatch::Mismatch, Highlight::Unplaced),
        ] {
            let worker = s.worker_answering(&rendered(text));
            assert_eq!(
                highlight(&s.render(&worker, "h1", "doc.pdf", 800)),
                expected,
                "{text:?}"
            );
            std::fs::remove_file(&s.log).unwrap();
        }
    }

    /// A worker slower than the native deadline and quicker than the scan one:
    /// the scan page waits for it, the native page does not.
    #[test]
    fn a_scan_page_is_given_the_longer_deadline() {
        let s = stand();
        let slow = s.script(&format!(
            "sleep 1\nprintf '%s\\n' '{}'\n",
            serde_json::to_string(&rendered(TextMatch::Match)).unwrap()
        ));
        let (native, scan) = (Duration::from_millis(300), Duration::from_secs(20));

        assert_eq!(
            s.render_within(&slow, "h1", "doc.pdf", 800, native, scan),
            PdfPageView::Unavailable {
                reason: PageUnavailable::Failed
            },
            "a native page is held to the native deadline"
        );
        std::fs::remove_file(&s.log).unwrap();

        s.sql("UPDATE page SET text_source = 'ocr:vision' WHERE document_id = 'h1'");
        assert_eq!(
            highlight(&s.render_within(&slow, "h1", "doc.pdf", 800, native, scan)),
            Highlight::Shown,
            "a scan page is held to the scan deadline"
        );
    }

    /// A worker that records the arguments it was started with, to `s.log`.
    fn warm_stand_in(s: &Stand, tail: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = s.scripts.path().join("warm-worker");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\n[ -n \"$1\" ] || exit 0\necho \"$*\" > '{}'\n{tail}",
                s.log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_file(&s.log);
        wait_until_it_will_run(&path);
        assert!(!s.log.exists(), "the pre-run wrote the log");
        path
    }

    #[test]
    fn a_library_with_a_scan_warms_the_ocr_engine_and_one_without_does_not() {
        let s = stand();
        let worker = warm_stand_in(&s, "");

        warm_ocr_if_scanned(&s.state, &worker, Duration::from_secs(10));
        assert!(!s.log.exists(), "native pages only: the worker was started");

        s.sql("UPDATE page SET text_source = 'ocr:vision' WHERE document_id = 'h2'");
        warm_ocr_if_scanned(&s.state, &worker, Duration::from_secs(10));
        assert_eq!(
            std::fs::read_to_string(&s.log).expect("the worker ran"),
            "--warm-ocr\n"
        );
    }

    #[test]
    fn a_warm_up_is_held_to_the_deadline_it_is_given_and_never_panics() {
        let s = stand();
        s.sql("UPDATE page SET text_source = 'ocr:vision'");
        let hung = warm_stand_in(&s, "exec sleep 30\n");
        let began = std::time::Instant::now();
        warm_ocr_if_scanned(&s.state, &hung, Duration::from_millis(300));
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "{:?}",
            began.elapsed()
        );

        let crashing = warm_stand_in(&s, "exit 3\n");
        warm_ocr_if_scanned(&s.state, &crashing, Duration::from_secs(10));
        warm_ocr_if_scanned(
            &s.state,
            Path::new("/nonexistent/worker"),
            Duration::from_secs(10),
        );
    }

    #[test]
    fn the_scan_deadline_is_the_pools_own() {
        assert_eq!(
            RENDER_TIMEOUT_SCAN,
            PoolConfig::new(Path::new("worker")).timeout
        );
        assert!(RENDER_TIMEOUT < RENDER_TIMEOUT_SCAN);
    }

    #[test]
    fn a_page_the_reader_skipped_is_asked_for_without_a_hash() {
        let s = stand();
        s.sql("DELETE FROM block");
        s.sql("DELETE FROM page");
        let worker = s.worker_answering(&rendered(TextMatch::None));

        let view = s.render(&worker, "h1", "doc.pdf", 800);

        assert_eq!(highlight(&view), Highlight::NoText);
        let sent = s.request();
        assert_eq!(sent.text_sha256, None);
        assert!(!sent.recognise);
    }

    #[test]
    fn a_worker_that_crashes_or_refuses_is_failed() {
        let s = stand();
        let crashing = s.script("exit 3\n");
        let view = s.render(&crashing, "h1", "doc.pdf", 800);
        assert_eq!(
            view,
            PdfPageView::Unavailable {
                reason: PageUnavailable::Failed
            }
        );
        std::fs::remove_file(&s.log).unwrap();

        let refusing = s.worker_answering(&RenderReply::Refused {
            error: RenderError::NoSuchPage,
            message: "page 1 of 0".into(),
        });
        let view = s.render(&refusing, "h1", "doc.pdf", 800);
        assert_eq!(
            view,
            PdfPageView::Unavailable {
                reason: PageUnavailable::Failed
            }
        );
    }

    #[test]
    fn the_wire_shape_is_tagged_and_camel_case() {
        let page = serde_json::to_value(PdfPageView::Page {
            pages: 3,
            width_px: 10,
            height_px: 20,
            image: "data:image/png;base64,AA".into(),
            highlight: Highlight::Shown,
            rects: vec![],
        })
        .unwrap();
        assert_eq!(page["kind"], "page");
        assert_eq!(page["widthPx"], 10);
        assert_eq!(page["heightPx"], 20);
        assert_eq!(page["highlight"], "shown");
        assert!(page["rects"].is_array());
        assert!(page.get("width_px").is_none());

        for (reason, tag) in [
            (PageUnavailable::NoPath, "noPath"),
            (PageUnavailable::FileMissing, "fileMissing"),
            (PageUnavailable::Failed, "failed"),
        ] {
            let v = serde_json::to_value(PdfPageView::Unavailable { reason }).unwrap();
            assert_eq!(
                (v["kind"].as_str(), v["reason"].as_str()),
                (Some("unavailable"), Some(tag))
            );
        }
        for (h, tag) in [
            (Highlight::Changed, "changed"),
            (Highlight::Scan, "scan"),
            (Highlight::NoText, "noText"),
            (Highlight::Unplaced, "unplaced"),
        ] {
            assert_eq!(serde_json::to_value(h).unwrap(), tag);
        }

        let arg: RangeArg =
            serde_json::from_str(r#"{"blockId":5,"blockStart":4,"length":2,"primary":false}"#)
                .unwrap();
        assert_eq!((arg.block_start, arg.length, arg.primary), (4, 2, false));
    }
}
