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
//! reads the same. Where no such text exists (a scan, a path that now names
//! another document) the page is drawn alone and the window is told why.

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

/// The widest page asked of the worker, whatever the window sends: a PNG of a
/// page is width x height x 4 bytes in the worker before it is encoded.
const MAX_WIDTH_PX: u32 = 3000;

/// How long the worker may take to draw one page before it is killed.
const RENDER_TIMEOUT: Duration = Duration::from_secs(10);

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
    )
}

/// [`render_pdf_page`] with the worker and its deadline given, which is what
/// lets a test point it at a stand-in.
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
    if mnema_walk::stat(&full).is_none() {
        return unavailable(PageUnavailable::FileMissing);
    }
    let Some(path) = full.to_str() else {
        return unavailable(PageUnavailable::Failed);
    };

    // `fixed` is a verdict reached without asking the file.
    let (text_sha256, fixed) = match stored {
        None => (None, Some(Highlight::Changed)),
        Some(Some((_, source))) if source.starts_with("ocr:") => (None, Some(Highlight::Scan)),
        Some(Some((text, _))) => (Some(sha256_hex(&text)), None),
        Some(None) => (None, None),
    };
    let request = RenderRequest {
        path: path.to_string(),
        page_no,
        width_px: width_px.min(MAX_WIDTH_PX),
        text_sha256,
        ranges: ranges
            .iter()
            .map(|r| RenderRange {
                start: r.block_start,
                len: r.length,
                primary: r.primary,
            })
            .collect(),
    };

    match mnema_pool::render_page(worker, &request, timeout) {
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
                Duration::from_secs(10),
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
    fn a_path_that_now_names_another_document_is_drawn_bare_and_changed() {
        let s = stand();
        // Even a worker that claimed a match could not make this `shown`.
        let worker = s.worker_answering(&rendered(TextMatch::Match));

        // The window cites h2; the index says doc.pdf now holds h1.
        let view = s.render(&worker, "h2", "doc.pdf", 800);

        assert_eq!(highlight(&view), Highlight::Changed);
        assert_eq!(s.request().text_sha256, None);
    }

    #[test]
    fn a_scan_page_is_drawn_bare_and_marked_scan() {
        let s = stand();
        s.sql("UPDATE page SET text_source = 'ocr:vision' WHERE document_id = 'h1'");
        let worker = s.worker_answering(&rendered(TextMatch::Match));

        let view = s.render(&worker, "h1", "doc.pdf", 800);

        assert_eq!(highlight(&view), Highlight::Scan);
        assert_eq!(s.request().text_sha256, None);
    }

    #[test]
    fn a_page_the_reader_skipped_is_asked_for_without_a_hash() {
        let s = stand();
        s.sql("DELETE FROM block");
        s.sql("DELETE FROM page");
        let worker = s.worker_answering(&rendered(TextMatch::None));

        let view = s.render(&worker, "h1", "doc.pdf", 800);

        assert_eq!(highlight(&view), Highlight::Changed);
        assert_eq!(s.request().text_sha256, None);
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
        for (h, tag) in [(Highlight::Changed, "changed"), (Highlight::Scan, "scan")] {
            assert_eq!(serde_json::to_value(h).unwrap(), tag);
        }

        let arg: RangeArg =
            serde_json::from_str(r#"{"blockId":5,"blockStart":4,"length":2,"primary":false}"#)
                .unwrap();
        assert_eq!((arg.block_start, arg.length, arg.primary), (4, 2, false));
    }
}
