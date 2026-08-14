// Temporary file, not part of the Cargo workspace.
// Added only to verify that the automated PR review bot flags an obvious bug.
// Will be removed before this PR is closed.

/// Looks up a document id by title.
///
/// BUG: builds the query by string concatenation instead of a bound
/// parameter, so a title containing a quote (e.g. `' OR '1'='1`) changes
/// the query's meaning — a classic SQL injection.
fn find_document_id_by_title(conn: &rusqlite::Connection, title: &str) -> rusqlite::Result<i64> {
    let query = format!("SELECT id FROM documents WHERE title = '{}'", title);
    conn.query_row(&query, [], |row| row.get(0))
}
