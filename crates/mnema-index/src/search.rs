use rusqlite::params;

use crate::{Db, Error};

impl Db {
    pub fn index_chunk_text(&self, chunk_id: i64, prepared: &str) -> Result<(), Error> {
        write_search_row(self.conn(), chunk_id, prepared)
    }

    /// The query passes through the same preparation as indexed *document* text
    /// — the same folding, the same apostrophe unification — because the two
    /// sides must tokenize alike or matches are lost asymmetrically.
    ///
    /// `SourceKind::Document` always, never the kind of the thing being
    /// searched, and that is a choice rather than an oversight. Code chunks are
    /// indexed with camelCase expanded, so `getUserName` is stored as
    /// `getUserName get User Name` and a plain `getUserName` query still finds
    /// them: the whole identifier survives the expansion. Preparing the QUERY as
    /// code instead would turn it into four terms, and `as_fts5_phrases` joins
    /// terms with an implicit AND, so all four would then be demanded. Measured
    /// on two chunks, one document and one code, both naming the identifier: as
    /// it stands the query returns both, and prepared as code it returns the
    /// code chunk alone and loses the prose that merely mentions the name.
    ///
    /// It is then split into terms and quoted, which is not optional. A search
    /// box must not answer a syntax error to `витрати (2024)`, and in FTS5's
    /// MATCH *expression* grammar `(`, `-` and `"` are all syntax — as U+0027
    /// was, before the canonical apostrophe moved to U+02BC. Quoting is what
    /// makes the user's text data rather than syntax.
    pub fn search_lexical(&self, query: &str, limit: i64) -> Result<Vec<i64>, Error> {
        let prepared = crate::prepare_for_search(query, mnema_core::SourceKind::Document);
        let expr = as_fts5_phrases(&prepared);
        if expr.is_empty() {
            // `MATCH ''` is a syntax error, and a query of nothing but
            // separators prepares down to exactly that. No terms, no rows.
            return Ok(Vec::new());
        }
        let mut stmt = self.conn().prepare(
            "SELECT chunk_fts.rowid FROM chunk_fts
               JOIN chunk ON chunk.id = chunk_fts.rowid
               JOIN document ON document.id = chunk.document_id
              WHERE chunk_fts MATCH ?1 AND document.status = 'indexed'
              ORDER BY rank LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![expr, limit], |r| r.get::<_, i64>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

/// Upserts one chunk's prepared text into the lexical index. `chunk_fts`
/// itself is never written directly — the triggers on `chunk_search` keep it
/// in sync — so this is the one place that owns the statement.
///
/// Takes `&rusqlite::Connection` rather than `&Db`, so it runs equally well on
/// its own, from `index_chunk_text`, and inside `insert_chunk`'s transaction:
/// `Transaction` derefs to `Connection`, and passing `&tx` through here is
/// what keeps the chunk row and its search row one atomic write.
pub(crate) fn write_search_row(
    conn: &rusqlite::Connection,
    chunk_id: i64,
    prepared: &str,
) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO chunk_search (chunk_id, text) VALUES (?1, ?2)
         ON CONFLICT(chunk_id) DO UPDATE SET text = excluded.text",
        params![chunk_id, prepared],
    )?;
    Ok(())
}

/// Quotes each term of a prepared query as an FTS5 phrase. Separate phrases
/// rather than one: FTS5 joins them with an implicit AND, while a single phrase
/// would demand the words be adjacent and quietly cost recall on every
/// multi-word query.
///
/// No escaping is needed inside the quotes, and that is a property of the term
/// rule rather than an omission — `terms` yields letters, digits and internal
/// apostrophes only, so a double quote cannot appear in one.
///
/// Quoting per term, not passing the expression through, means FTS5's own
/// operators — OR, NEAR, prefix `*` — are not reachable from here, and a bare
/// `OR` arrives as a third required word instead. That is pinned by a test
/// rather than fixed: offering the user a query language is the search/RAG
/// spec's decision, not a side effect of escaping.
fn as_fts5_phrases(prepared: &str) -> String {
    let mut out = String::with_capacity(prepared.len() + 8);
    for term in crate::text_prep::terms(prepared) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push('"');
        out.push_str(term);
        out.push('"');
    }
    out
}
