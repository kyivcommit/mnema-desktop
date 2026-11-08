//! `render_doc`, the one-shot call for a document shown as HTML, against the
//! stand-in worker's `--render-doc` branch. The exchange is `render_page`'s
//! own (`one_shot`), so its timeouts and crashes are `render_page.rs`'s tests.

#![cfg(unix)]

use std::path::PathBuf;
use std::time::Duration;

use mnema_core::render::{DocReader, DocRenderReply, DocRenderRequest};
use mnema_pool::render_doc;

#[test]
fn the_reply_comes_back_from_the_branch_that_reads_a_doc_request() {
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_mnema-pool-test-worker"));
    let request = DocRenderRequest {
        path: "/notes/a.md".into(),
        reader: DocReader::Markdown,
        page_no: None,
        text_sha256: Some("ab".into()),
        ranges: Vec::new(),
    };
    let reply = render_doc(&worker, &request, Duration::from_secs(5)).unwrap();
    assert_eq!(
        reply,
        DocRenderReply::Rendered {
            html: "/notes/a.md|true".into(),
            pages: 4,
            text: mnema_core::render::TextMatch::None,
        }
    );
}
