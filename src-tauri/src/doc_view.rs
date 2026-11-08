//! A document drawn as HTML for the window's page view (D199), the cited
//! passage marked — today Markdown.
//!
//! The same shape as `page_view.rs`: the HTML comes from
//! `mnema-extract-worker --render-doc` (the application may not link `comrak`
//! or `mnema-extract`, D40), and **the file is found only through the index** —
//! a path from the window is never opened. What the marks are worth is decided
//! here before the worker is asked: the citation's offsets were counted in the
//! blocks the index holds, so the worker gets those blocks' hash and marks only
//! when the file still reads the same. A path that now names another document
//! is rendered with no hash and no ranges.

use std::path::Path;
use std::time::Duration;

use mnema_core::render::{
    DocRange, DocReader, DocRenderReply, DocRenderRequest, RenderError, TextMatch, doc_text_sha256,
};
use mnema_pool::PoolConfig;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::Error;
use crate::page_view::Highlight;
use crate::state::AppState;
use crate::tree::{MAX_BLOCKS, cited_occupant};

/// How long the worker may take to render one document before it is killed.
const RENDER_TIMEOUT: Duration = Duration::from_secs(10);

/// One cited stretch of one block: the window's `PaneSpan` as it stands.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocSpanArg {
    pub block_id: i64,
    pub block_start: u32,
    pub length: u32,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DocUnavailable {
    /// The index holds no place for this document, so there is no file to open.
    NoPath,
    FileMissing,
    /// Too big for the page view; the text view still shows it.
    TooLarge,
    /// The worker could not render it. The reason is logged, not shown.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum DocPageView {
    Page {
        /// Allow-listed by the worker; put into the window as it stands.
        html: String,
        pages: u32,
        highlight: Highlight,
    },
    Unavailable {
        reason: DocUnavailable,
    },
}

/// The reader for a `document.mime`, or `None` for one the page view does
/// not draw as HTML.
fn reader_for(mime: &str) -> Option<DocReader> {
    match mime {
        "text/markdown" => Some(DocReader::Markdown),
        _ => None,
    }
}

#[tauri::command(async)]
pub fn render_doc(
    state: State<'_, AppState>,
    document_id: String,
    root_id: Option<i64>,
    relative_path: Option<String>,
    spans: Vec<DocSpanArg>,
) -> Result<DocPageView, Error> {
    render_doc_in(
        &state,
        state.worker_path(),
        &document_id,
        root_id,
        relative_path.as_deref(),
        &spans,
        RENDER_TIMEOUT,
    )
}

/// [`render_doc`] with the worker and its deadline given, which is what lets a
/// test point it at a stand-in.
pub(crate) fn render_doc_in(
    state: &AppState,
    worker: &Path,
    document_id: &str,
    root_id: Option<i64>,
    relative_path: Option<&str>,
    spans: &[DocSpanArg],
    timeout: Duration,
) -> Result<DocPageView, Error> {
    let unavailable = |reason| Ok(DocPageView::Unavailable { reason });

    // The index half in one snapshot: where the file is, what it holds now,
    // and — only when the path still names this document — its blocks.
    let found = state.with_index(|db| {
        db.read_snapshot(|db| {
            let Some(occupant) = cited_occupant(db, root_id, relative_path)? else {
                return Ok(None);
            };
            let ours = occupant.current_document_id == document_id;
            let view = db.document_view(&occupant.current_document_id, MAX_BLOCKS)?;
            Ok(Some((occupant, ours, view)))
        })
    })?;
    let Some((occupant, ours, view)) = found else {
        return unavailable(DocUnavailable::NoPath);
    };
    let Some(view) = view else {
        return unavailable(DocUnavailable::NoPath);
    };
    if view.truncated {
        return unavailable(DocUnavailable::TooLarge);
    }
    let Some(reader) = reader_for(&view.mime) else {
        eprintln!("mnema: the document view does not draw {}", view.mime);
        return unavailable(DocUnavailable::Failed);
    };

    let full = Path::new(&occupant.root_absolute_path).join(&occupant.relative_path);
    match std::fs::metadata(&full) {
        Ok(meta) if meta.len() > PoolConfig::new(worker).max_bytes => {
            return unavailable(DocUnavailable::TooLarge);
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return unavailable(DocUnavailable::FileMissing);
        }
        Err(e) => {
            eprintln!(
                "mnema: the document view could not stat {}: {e}",
                full.display()
            );
            return unavailable(DocUnavailable::Failed);
        }
    }
    let Some(path) = full.to_str() else {
        return unavailable(DocUnavailable::Failed);
    };

    // Another document at this path: nothing of ours to compare or mark.
    let (text_sha256, ranges) = if ours {
        let hash = doc_text_sha256(
            view.blocks
                .iter()
                .map(|b| (b.page_no as u32, b.kind.as_str(), b.text.as_str())),
        );
        // A span is sent only for a block of this document: an id the window
        // holds from another document would name the wrong block here.
        let ranges = spans
            .iter()
            .filter_map(|s| {
                let block = view.blocks.iter().find(|b| b.block_id == s.block_id)?;
                Some(DocRange {
                    page_no: block.page_no as u32,
                    reading_order: block.reading_order as u32,
                    start: s.block_start,
                    len: s.length,
                    primary: s.primary,
                })
            })
            .collect();
        (Some(hash), ranges)
    } else {
        (None, Vec::new())
    };
    let request = DocRenderRequest {
        path: path.to_string(),
        reader,
        page_no: None,
        text_sha256,
        ranges,
    };

    match mnema_pool::render_doc(worker, &request, timeout) {
        Ok(DocRenderReply::Rendered { html, pages, text }) => Ok(DocPageView::Page {
            html,
            pages,
            highlight: match text {
                TextMatch::Match => Highlight::Shown,
                TextMatch::Unplaced => Highlight::Unplaced,
                TextMatch::Mismatch => Highlight::Changed,
                // No hash was sent: the path names another document now.
                TextMatch::None => Highlight::Changed,
            },
        }),
        Ok(DocRenderReply::Refused {
            error: RenderError::TooLarge,
            ..
        }) => unavailable(DocUnavailable::TooLarge),
        Ok(DocRenderReply::Refused { message, .. }) => {
            eprintln!("mnema: the document view was refused: {message}");
            unavailable(DocUnavailable::Failed)
        }
        Err(e) => {
            eprintln!("mnema: the document worker failed: {e}");
            unavailable(DocUnavailable::Failed)
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use mnema_core::{Block, BlockType, OnDisk, SourceKind};
    use mnema_index::DocumentStatus;
    use std::path::PathBuf;

    struct Stand {
        _data: tempfile::TempDir,
        root_dir: tempfile::TempDir,
        scripts: tempfile::TempDir,
        state: AppState,
        root: i64,
        log: PathBuf,
        /// Block ids of `m1`, in reading order, and one of `m2`.
        ids: Vec<i64>,
        other_id: i64,
    }

    /// One root holding `notes.md` (on disk, indexed as document `m1`: two
    /// blocks on page 1, one on page 2) and a second document `m2` the path
    /// can be repointed to.
    fn stand() -> Stand {
        let data = tempfile::tempdir().unwrap();
        let root_dir = tempfile::tempdir().unwrap();
        let scripts = tempfile::tempdir().unwrap();
        std::fs::write(root_dir.path().join("notes.md"), "# a\n").unwrap();
        let disk = mnema_walk::stat(&root_dir.path().join("notes.md")).unwrap();
        let state = AppState::new(
            data.path().to_path_buf(),
            PathBuf::new(),
            "http://127.0.0.1:1".to_string(),
            format!("mnema-desktop-doc-view-test-{}", data.path().display()),
        );
        state.open_index().unwrap();
        let block = |order: i64, block_type, text: &str| Block {
            block_type,
            reading_order: order,
            language: None,
            text: text.to_string(),
            line_start: None,
            line_end: None,
        };
        let (root, ids, other_id) = state
            .with_index(|db| {
                let root = db.insert_watched_root(root_dir.path().to_str().unwrap())?;
                db.insert_document("m1", "text/markdown", 1, SourceKind::Document)?;
                let p1 = db.insert_page("m1", 1, "native:md", None)?;
                let p2 = db.insert_page("m1", 2, "native:md", None)?;
                let ids = vec![
                    db.insert_block(p1, &block(0, BlockType::Headline, "# a"))?,
                    db.insert_block(p1, &block(1, BlockType::Paragraph, "текст"))?,
                    db.insert_block(p2, &block(0, BlockType::Code, "```\nx\n```"))?,
                ];
                db.set_document_status("m1", DocumentStatus::Indexed)?;
                db.insert_document("m2", "text/markdown", 1, SourceKind::Document)?;
                let q = db.insert_page("m2", 1, "native:md", None)?;
                let other_id = db.insert_block(q, &block(0, BlockType::Paragraph, "інше"))?;
                db.set_document_status("m2", DocumentStatus::Indexed)?;
                db.insert_path(
                    root,
                    "notes.md",
                    "m1",
                    OnDisk {
                        size_bytes: disk.size_bytes,
                        mtime: disk.mtime,
                    },
                    "md",
                    1,
                )?;
                Ok((root, ids, other_id))
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
            ids,
            other_id,
        }
    }

    impl Stand {
        /// A worker that records the line it is handed and answers `reply`.
        fn worker_answering(&self, reply: &DocRenderReply) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let path = self.scripts.path().join("worker");
            std::fs::write(
                &path,
                format!(
                    "#!/bin/sh\nIFS= read -r line || exit 0\nprintf '%s\\n' \"$line\" > '{}'\nprintf '%s\\n' '{}'\n",
                    self.log.display(),
                    serde_json::to_string(reply).unwrap()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            crate::page_view::tests::wait_until_it_will_run(&path);
            path
        }

        fn render(&self, worker: &Path, spans: &[DocSpanArg]) -> DocPageView {
            render_doc_in(
                &self.state,
                worker,
                "m1",
                Some(self.root),
                Some("notes.md"),
                spans,
                Duration::from_secs(10),
            )
            .unwrap()
        }

        fn request(&self) -> DocRenderRequest {
            serde_json::from_str(&std::fs::read_to_string(&self.log).expect("the worker ran"))
                .unwrap()
        }

        fn sql(&self, statement: &str) {
            self.state
                .with_index(|db| Ok(db.conn().execute(statement, [])?))
                .unwrap();
        }
    }

    fn rendered(text: TextMatch) -> DocRenderReply {
        DocRenderReply::Rendered {
            html: "<p>x</p>".into(),
            pages: 2,
            text,
        }
    }

    fn span(block_id: i64) -> DocSpanArg {
        DocSpanArg {
            block_id,
            block_start: 1,
            length: 3,
            primary: true,
        }
    }

    #[test]
    fn the_worker_gets_the_file_from_the_index_and_the_hash_of_its_blocks() {
        let s = stand();
        let worker = s.worker_answering(&rendered(TextMatch::Match));
        let view = s.render(&worker, &[span(s.ids[2])]);
        assert_eq!(
            view,
            DocPageView::Page {
                html: "<p>x</p>".into(),
                pages: 2,
                highlight: Highlight::Shown
            }
        );
        let sent = s.request();
        assert_eq!(
            sent.path,
            s.root_dir.path().join("notes.md").to_str().unwrap()
        );
        assert_eq!(sent.reader, DocReader::Markdown);
        assert_eq!(
            sent.text_sha256.as_deref(),
            Some(
                doc_text_sha256([
                    (1, "headline", "# a"),
                    (1, "paragraph", "текст"),
                    (2, "code", "```\nx\n```"),
                ])
                .as_str()
            )
        );
        assert_eq!(
            sent.ranges,
            [DocRange {
                page_no: 2,
                reading_order: 0,
                start: 1,
                len: 3,
                primary: true
            }]
        );
    }

    #[test]
    fn a_span_of_another_document_is_not_sent() {
        let s = stand();
        let worker = s.worker_answering(&rendered(TextMatch::Match));
        s.render(&worker, &[span(s.other_id), span(s.ids[1])]);
        let sent = s.request();
        assert_eq!(sent.ranges.len(), 1, "{:?}", sent.ranges);
        assert_eq!(
            (sent.ranges[0].page_no, sent.ranges[0].reading_order),
            (1, 1)
        );
    }

    #[test]
    fn a_path_that_names_another_document_now_is_rendered_bare() {
        let s = stand();
        s.sql("UPDATE path SET document_id = 'm2' WHERE relative_path = 'notes.md'");
        let worker = s.worker_answering(&rendered(TextMatch::None));
        let view = s.render(&worker, &[span(s.ids[1])]);
        let sent = s.request();
        assert_eq!(sent.text_sha256, None);
        assert!(sent.ranges.is_empty());
        assert!(
            matches!(
                view,
                DocPageView::Page {
                    highlight: Highlight::Changed,
                    ..
                }
            ),
            "{view:?}"
        );
    }

    #[test]
    fn what_the_worker_says_about_the_text_becomes_the_note() {
        let s = stand();
        for (text, note) in [
            (TextMatch::Mismatch, Highlight::Changed),
            (TextMatch::Unplaced, Highlight::Unplaced),
        ] {
            let worker = s.worker_answering(&rendered(text));
            let view = s.render(&worker, &[span(s.ids[1])]);
            assert!(
                matches!(view, DocPageView::Page { highlight, .. } if highlight == note),
                "{text:?}: {view:?}"
            );
        }
    }

    #[test]
    fn a_missing_file_an_unknown_path_and_a_refusal_are_unavailable() {
        let s = stand();
        let worker = s.worker_answering(&DocRenderReply::Refused {
            error: RenderError::TooLarge,
            message: "big".into(),
        });
        assert_eq!(
            s.render(&worker, &[]),
            DocPageView::Unavailable {
                reason: DocUnavailable::TooLarge
            }
        );
        let worker = s.worker_answering(&DocRenderReply::Refused {
            error: RenderError::Unreadable,
            message: "no".into(),
        });
        assert_eq!(
            s.render(&worker, &[]),
            DocPageView::Unavailable {
                reason: DocUnavailable::Failed
            }
        );
        let none = render_doc_in(
            &s.state,
            &worker,
            "m1",
            Some(s.root),
            Some("elsewhere.md"),
            &[],
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(
            none,
            DocPageView::Unavailable {
                reason: DocUnavailable::NoPath
            }
        );
        std::fs::remove_file(s.root_dir.path().join("notes.md")).unwrap();
        assert_eq!(
            s.render(&worker, &[]),
            DocPageView::Unavailable {
                reason: DocUnavailable::FileMissing
            }
        );
    }

    #[test]
    fn a_document_the_view_does_not_draw_is_not_sent_to_the_worker() {
        let s = stand();
        s.sql("UPDATE document SET mime = 'text/plain' WHERE id = 'm1'");
        let worker = s.worker_answering(&rendered(TextMatch::Match));
        assert_eq!(
            s.render(&worker, &[]),
            DocPageView::Unavailable {
                reason: DocUnavailable::Failed
            }
        );
        assert!(!s.log.exists(), "the worker was asked");
    }
}
