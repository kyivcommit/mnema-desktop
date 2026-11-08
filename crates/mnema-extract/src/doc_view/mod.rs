//! A document rendered as HTML for the window's page view (`--render-doc`,
//! D199): read again from disk, marked only when its blocks still hash to the
//! index's, and passed through one allow-list on the way out.

// shortcut: unused until the worker's --render-doc (plan task 5) calls it; drop the allow then.
#[allow(dead_code)]
pub(crate) mod markdown;
pub(crate) mod sanitize;
