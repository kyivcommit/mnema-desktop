use rusqlite::params;

use crate::{Db, Error};

/// How the terms of a query are combined into one FTS5 expression.
///
/// Meant to be a value the application passes in, rather than a branch
/// inside the product, and walked in full by `mnema-eval`'s sweep. Pinned
/// by `the_unparameterised_search_is_the_all_terms_rule`, which pins only
/// that `search_lexical` equals `search_lexical_with` under `AllTerms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum QueryRule {
    AllTerms,
    AnyTerm,
    TermsInIndex,
    TermsInIndexOrAnyTerm,
}

impl QueryRule {
    /// Every variant, in a fixed order — meant for a sweep that does not
    /// exist yet.
    pub const ALL: [QueryRule; 4] = [
        QueryRule::AllTerms,
        QueryRule::AnyTerm,
        QueryRule::TermsInIndex,
        QueryRule::TermsInIndexOrAnyTerm,
    ];

    pub fn label(self) -> &'static str {
        match self {
            QueryRule::AllTerms => "all-terms",
            QueryRule::AnyTerm => "any-term",
            QueryRule::TermsInIndex => "terms-in-index",
            QueryRule::TermsInIndexOrAnyTerm => "terms-in-index-or-any",
        }
    }
}

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
        self.search_lexical_with(query, QueryRule::AllTerms, limit)
    }

    /// `TermsInIndexOrAnyTerm` is `TermsInIndex` tried first, and only when
    /// that comes back with nothing does `AnyTerm` run as a second attempt —
    /// never as a second clause of the same expression, or it would widen
    /// results the stricter rule already found. Pinned by
    /// `the_fallback_fires_only_where_the_stricter_rule_came_back_empty`.
    pub fn search_lexical_with(
        &self,
        query: &str,
        rule: QueryRule,
        limit: i64,
    ) -> Result<Vec<i64>, Error> {
        let prepared = crate::prepare_for_search(query, mnema_core::SourceKind::Document);
        let expr = match rule {
            QueryRule::AllTerms => as_fts5_phrases(&prepared),
            QueryRule::AnyTerm => as_fts5_any(&prepared),
            QueryRule::TermsInIndex | QueryRule::TermsInIndexOrAnyTerm => {
                as_fts5_all_of(&self.terms_present(&prepared)?)
            }
        };
        let first = if expr.is_empty() {
            // `MATCH ''` is a syntax error, and a query of nothing but
            // separators prepares down to exactly that. No terms, no rows.
            // Pinned by
            // `a_query_with_no_terms_returns_no_rows_rather_than_an_error`.
            Vec::new()
        } else {
            self.matching(&expr, limit)?
        };
        if !first.is_empty() || rule != QueryRule::TermsInIndexOrAnyTerm {
            return Ok(first);
        }
        let wide = as_fts5_any(&prepared);
        if wide.is_empty() {
            return Ok(Vec::new());
        }
        self.matching(&wide, limit)
    }

    /// The query's terms that occur somewhere in the index, in the order they
    /// were typed.
    ///
    /// One `MATCH` per term rather than a vocabulary table: `fts5vocab` would
    /// need a line of DDL and a migration, and this runs on a query's worth of
    /// words against a local file. If it ever measures as the narrow place, that
    /// table is where to go. Pinned by
    /// `a_terms_presence_is_asked_of_the_whole_index`.
    pub fn terms_present(&self, prepared: &str) -> Result<Vec<String>, Error> {
        let mut stmt = self.conn().prepare(
            "SELECT 1 FROM chunk_fts
               JOIN chunk ON chunk.id = chunk_fts.rowid
               JOIN document ON document.id = chunk.document_id
              WHERE chunk_fts MATCH ?1 AND document.status = 'indexed'
              LIMIT 1",
        )?;
        let mut out = Vec::new();
        for term in crate::text_prep::terms(prepared) {
            if stmt.exists(params![quote_term(term)])? {
                out.push(term.to_string());
            }
        }
        Ok(out)
    }

    fn matching(&self, expr: &str, limit: i64) -> Result<Vec<i64>, Error> {
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

/// Quotes each term of a prepared query, joined by `sep`. Quoting is what
/// keeps FTS5's own operators — OR, NEAR, prefix `*` — out of reach, so a
/// person's literal `OR` arrives as one more quoted word instead of an
/// operator: pinned for `as_fts5_phrases`'s separator by
/// `fts5_operators_are_not_a_query_language_here`, and for `as_fts5_any`'s
/// by `any_term_survives_a_word_no_document_has`.
///
/// No escaping is needed inside the quotes, and that is a property of the
/// term rule rather than an omission — `terms` yields letters, digits and
/// internal apostrophes only, so a double quote cannot appear in one.
fn quoted_terms(prepared: &str, sep: &str) -> String {
    let mut out = String::new();
    for term in crate::text_prep::terms(prepared) {
        if !out.is_empty() {
            out.push_str(sep);
        }
        out.push_str(&quote_term(term));
    }
    out
}

/// One term, quoted for FTS5's MATCH grammar. The one place that owns the
/// quoting rule — `quoted_terms`, `terms_present` and `as_fts5_all_of` all
/// call this rather than each spelling `"` + term + `"` out again.
fn quote_term(term: &str) -> String {
    format!("\"{term}\"")
}

/// Separate phrases rather than one: FTS5 joins them with an implicit AND,
/// while a single phrase would demand the words be adjacent and quietly cost
/// recall on every multi-word query.
fn as_fts5_phrases(prepared: &str) -> String {
    quoted_terms(prepared, " ")
}

/// Quotes and ANDs an already-chosen list of terms. Separate from
/// `as_fts5_phrases`, which starts from a prepared string and takes every term
/// it holds — here the choosing has already happened. Pinned by
/// `terms_in_index_drops_the_unseen_word_and_still_demands_the_rest`.
fn as_fts5_all_of(terms: &[String]) -> String {
    let mut out = String::new();
    for term in terms {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&quote_term(term));
    }
    out
}

/// Joins each quoted term with FTS5's `OR`, so any one term is enough. The
/// server's lexical arm works this way — `app/search/hybrid.py:36`, "BM25
/// match is `content ||| :q` (OR-tokenized)".
///
/// Pinned by `any_term_survives_a_word_no_document_has`.
fn as_fts5_any(prepared: &str) -> String {
    quoted_terms(prepared, " OR ")
}
