//! A document rendered as HTML for the window's page view (`--render-doc`,
//! D199): read again from disk, marked only when its blocks still hash to the
//! index's, and passed through one allow-list on the way out.

pub(crate) mod markdown;
pub(crate) mod sanitize;

use mnema_core::render::{DocReader, DocRenderReply, DocRenderRequest, RenderError};

/// Answers one `--render-doc` request: reads `req.path` and renders it with
/// the reader the application named.
///
/// The file's size is the application's to bound before it asks (it `stat`s
/// the file it found through the index, as for a PDF page); what is bounded
/// here is the HTML that comes back.
pub fn render_doc(req: &DocRenderRequest) -> DocRenderReply {
    let bytes = match std::fs::read(&req.path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return DocRenderReply::Refused {
                error: RenderError::Unreadable,
                message: format!("{}: {e}", req.path),
            };
        }
    };
    match req.reader {
        DocReader::Markdown => markdown::render_markdown(&bytes, req),
    }
}
