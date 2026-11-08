//! A document rendered as HTML for the window's page view (`--render-doc`,
//! D199): read again from disk, marked only when its blocks still hash to the
//! index's, and passed through one allow-list on the way out.

// shortcut: unused until the Markdown renderer (plan task 4) calls it; drop the allow then.
#[allow(dead_code)]
pub(crate) mod sanitize;
