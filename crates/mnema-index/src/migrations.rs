use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::Error;

/// Bumped whenever the DDL changes. Stored in PRAGMA user_version.
pub const SCHEMA_VERSION: i64 = 2;

/// Which reader made the document at each path, and which version of it.
///
/// The cheap arm decides "has this file changed?" from `path.size_bytes` and
/// `path.mtime` alone, and neither of those moves when the *code* that read the
/// file changes. `INDEX_FORMAT_VERSION` is the lever for that everywhere else —
/// it is on `chunk` and on `skipped` — but it never reaches this table, so
/// without these two columns a file whose format changes hands answers
/// "unchanged" for the life of the index, with nothing logged anywhere. `.html`
/// is the live case: it is read as text today, and an html reader arriving would
/// re-read nothing (spec §2.2).
///
/// `ADD COLUMN`, not a rebuild, and that is the whole reason this is two lines.
/// `path` is `WITHOUT ROWID` and the reflex for altering such a table is
/// create-copy-drop-rename, which silently drops `ix_path_document` and the
/// `WITHOUT ROWID` clause itself unless the rebuild restores both.
/// `ADD COLUMN` keeps every one of them.
///
/// The defaults are a claim about history rather than a formality: everything a
/// `path` row could be holding today was put there by the text reader, because
/// it is the only reader that has ever existed. They are also what Task 4
/// compares against a live manifest, so a row migrated to anything else — `''`,
/// `0`, `'unknown'` — would mismatch for ever and re-read its file on every walk.
///
/// **`NOT NULL` does not make either column meaningful.** The empty string
/// satisfies it, and a `reader_version` of 0 satisfies the other; what keeps a
/// nameless reader out of the index is `mnema-pool`'s refusal of a header that
/// names none (`crates/mnema-pool/src/lib.rs:1080`), not this DDL.
const ADD_PATH_READER: &str = "\
ALTER TABLE path ADD COLUMN reader TEXT NOT NULL DEFAULT 'text';
ALTER TABLE path ADD COLUMN reader_version INTEGER NOT NULL DEFAULT 1;";

/// The migrations in order; the index of each is the `user_version` it produces.
///
/// **`schema.sql` is migration 1 and is now frozen.** It is not a description of
/// the current schema and must not be edited to become one: it runs in full on
/// every fresh database, ahead of everything below it, so a column written both
/// there and in an `ALTER` below fails the fresh install outright with
/// "duplicate column name" — measured on this very pair, not supposed. Anything
/// the DDL gains from here on is another `M::up` in this list and another bump
/// of `SCHEMA_VERSION`.
fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("schema.sql")),
        M::up(ADD_PATH_READER),
    ])
}

/// Takes `&mut` because `rusqlite_migration::to_latest` wraps the whole set in a
/// transaction and so needs exclusive access to the connection.
pub fn apply(conn: &mut Connection) -> Result<(), Error> {
    migrations().to_latest(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::register_vector_extension;

    #[test]
    fn the_migration_set_is_valid() {
        // validate() is not a static check: it opens an in-memory database and
        // runs every migration for real. So the extension has to be registered
        // first — once schema.sql creates a vec0 table, an unregistered run
        // fails with "no such module: vec0".
        register_vector_extension().unwrap();
        migrations().validate().expect("migrations apply cleanly");
    }

    /// A database that already exists at version 1 reaches the current version
    /// with its rows intact, and every `path` row it was carrying is credited
    /// to the text reader.
    ///
    /// This is the side a fresh-database test cannot see, and it is the side
    /// that matters here: `path` is `WITHOUT ROWID`, and the obvious way to add
    /// a column to a table SQLite will not let you alter freely is to rebuild
    /// it — create, copy, drop, rename — which loses `ix_path_document` and the
    /// `WITHOUT ROWID` clause itself unless the rebuild remembers to restore
    /// both, silently and with no error anywhere. `ADD COLUMN` keeps them;
    /// this test is what says so rather than assuming it.
    ///
    /// The defaults are asserted as *values*, not as schema decoration. `'text'`
    /// and `1` are a claim about history — everything in an index today was put
    /// there by the text reader at version 1 — and Task 4 compares exactly these
    /// two columns against a live manifest. A row migrated to anything else
    /// mismatches for ever and re-reads the file on every walk.
    #[test]
    fn an_existing_version_one_database_keeps_its_rows_and_credits_the_text_reader() {
        register_vector_extension().unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        migrations()
            .to_version(&mut conn, 1)
            .expect("version 1 is the schema that shipped");

        // Written through version 1's own five-column statement, because that
        // is the only statement a database at version 1 could have been
        // written by.
        conn.execute(
            "INSERT INTO watched_root (id, absolute_path) VALUES (1, '/tmp/root')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO document (id, mime, size_bytes, source_kind)
             VALUES ('abc', 'text/plain', 10, 'document')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO path (watched_root_id, relative_path, document_id, size_bytes, mtime)
             VALUES (1, 'a.txt', 'abc', 10, 1234)",
            [],
        )
        .unwrap();

        apply(&mut conn).expect("an existing database migrates to the current version");

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        let row: (String, i64, i64, String, i64) = conn
            .query_row(
                "SELECT document_id, size_bytes, mtime, reader, reader_version FROM path
                  WHERE watched_root_id = 1 AND relative_path = 'a.txt'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("the row written before the migration is still there");
        assert_eq!(
            row,
            ("abc".to_string(), 10, 1234, "text".to_string(), 1),
            "the three old columns must survive and the two new ones must name \
             the reader that really made this row"
        );

        let ddl: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'path'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            ddl.contains("WITHOUT ROWID"),
            "the migration must not turn `path` back into a rowid table: {ddl}"
        );

        let indexes: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master
                  WHERE type = 'index' AND name = 'ix_path_document'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            indexes, 1,
            "ix_path_document is what `forget_if_unnamed` counts through; a \
             rebuild that drops it costs a table scan per deleted path"
        );
    }
}
