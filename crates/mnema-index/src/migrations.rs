use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::Error;

/// Bumped whenever the DDL changes. Stored in PRAGMA user_version.
pub const SCHEMA_VERSION: i64 = 3;

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
/// **`'text'` is not true of every row it is written onto, and saying otherwise
/// here would be the more expensive mistake.** The markdown reader shipped in
/// `fb3a924` — `git show fb3a924:crates/mnema-extract/src/typing.rs` has
/// `Some("md") => (…, Reader::Markdown)`, and `worker.rs` has the branch — so a
/// database in the field holds `path` rows the markdown reader made, and this
/// migration credits every one of them to text.
///
/// It is still the right default, because there is nothing better available:
/// the row records no reader (that is the whole point of the migration) and the
/// extension cannot stand in for one — which reader takes `.md` is a property of
/// the *build*, not of the database, and a migration that guessed from the name
/// would be inventing history rather than admitting it has none.
///
/// What the default really buys is **how much of an existing index is re-read
/// once** on the first walk after Task 4 starts comparing these columns. `'text'`
/// costs a re-read of the markdown files only; `''` or `0` — values that match no
/// manifest at all — would cost a re-read of every file in the index. Either way
/// it is once: the mismatch sends the file to a worker, and `repoint` then writes
/// what the worker actually said, so the second walk matches. "For ever" belongs
/// to a *writer* that keeps writing the wrong value, not to a default that is
/// written once; that failure is `insert_path` or `repoint` answering with a
/// constant, and `scripts/mutations/task-3.sh` C6–C9 are what hold them to it.
///
/// **The markdown re-read is not optional and not avoidable later.** The first
/// cheap arm compares size, mtime and the chunk stage and nothing else
/// (`crates/mnema-ingest/src/lib.rs:202-210`) — it never looks at
/// `INDEX_FORMAT_VERSION` — so bumping that in Task 15 does not wash this out.
/// One extra read per already-indexed `.md`, on the first pass after Task 4,
/// predictable and paid once. Written down here so it is a known cost rather
/// than a surprise in a walk report.
///
/// **`NOT NULL` does not make either column meaningful.** The empty string
/// satisfies it and a `reader_version` of 0 satisfies the other, and the two are
/// not equally guarded: `mnema-pool` refuses a header whose reader is blank
/// (`crates/mnema-pool/src/lib.rs:1080`, `reader.trim().is_empty()`) and checks
/// nothing at all about the version. A version of 0 is unreachable today only
/// because `reader_version` is a required wire field (`mnema-core/src/wire.rs`,
/// no `#[serde(default)]`) and every worker branch sends a published constant —
/// which is an argument about the workers that exist, not a guard.
const ADD_PATH_READER: &str = "\
ALTER TABLE path ADD COLUMN reader TEXT NOT NULL DEFAULT 'text';
ALTER TABLE path ADD COLUMN reader_version INTEGER NOT NULL DEFAULT 1;";

/// One exclusion per (watched root, path prefix).
///
/// `ignore_rule` shipped in `schema.sql` with both CHECKs and no UNIQUE, so
/// nothing stopped the same folder being excluded twice — and the table had no
/// reader and no writer at all until the exclusion commands arrived, which is
/// why the gap survived this long. Adding the constraint now costs nothing:
/// there are no rows anywhere to conflict with it.
///
/// **Partial, and the predicate is load-bearing.** A tag rule carries
/// `path_prefix IS NULL` (§14.5's `ignore_rule` CHECK allows exactly one of the
/// two), and SQLite treats NULLs as DISTINCT inside a UNIQUE index — so a
/// non-partial index would neither dedup tag rules nor stop them, it would just
/// be a claim about rows it never actually constrains. `WHERE path_prefix IS
/// NOT NULL` says what the index is for.
///
/// ⚠️ A targeted `ON CONFLICT` against this index must repeat the predicate —
/// `ON CONFLICT (watched_root_id, path_prefix) WHERE path_prefix IS NOT NULL` —
/// or SQLite refuses the statement outright with "ON CONFLICT clause does not
/// match any PRIMARY KEY or UNIQUE constraint". `schema.sql:306-311` records
/// the same trap for `ux_skipped_current`.
const ADD_IGNORE_RULE_UNIQUE: &str = "\
CREATE UNIQUE INDEX ux_ignore_rule_path
    ON ignore_rule(watched_root_id, path_prefix)
 WHERE path_prefix IS NOT NULL;";

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
        M::up(ADD_IGNORE_RULE_UNIQUE),
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
    /// with its rows intact, and the migration credits every `path` row it was
    /// carrying to the text reader.
    ///
    /// **"Credits", not "records".** The markdown reader shipped in `fb3a924`,
    /// so some of the rows this runs over were made by it and are about to be
    /// labelled `text` — see `ADD_PATH_READER` for why that is still the right
    /// default and what it costs. The name of this test says what the migration
    /// *does*, deliberately, because an earlier name said the rows really were
    /// the text reader's and that is false.
    ///
    /// This is the side a fresh-database test cannot see, and it is the side
    /// that matters here: `path` is `WITHOUT ROWID`, and the obvious way to add
    /// a column to a table SQLite will not let you alter freely is to rebuild
    /// it — create, copy, drop, rename — which loses `ix_path_document`, the
    /// `WITHOUT ROWID` clause itself, and (if the last step is forgotten) leaves
    /// the old table standing beside the new one, all silently and with no error
    /// anywhere. `ADD COLUMN` produces none of those; this test is what says so
    /// rather than assuming it.
    ///
    /// The defaults are asserted as *values*, not as schema decoration: Task 4
    /// compares exactly these two columns against a live manifest, so what they
    /// migrate to decides how much of an existing index is re-read on the first
    /// walk after it lands.
    #[test]
    fn an_existing_database_keeps_its_rows_and_the_migration_credits_them_all_to_text() {
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
            "the three old columns must survive, and the two new ones must carry \
             the defaults Task 4 will compare against a manifest"
        );

        // A rebuild that copies into a new table and forgets its last step
        // leaves the original standing under a scratch name: every row of the
        // index duplicated, invisible to every query, and growing a second copy
        // on each future migration that does the same.
        let leftovers: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master
                  WHERE type = 'table' AND name LIKE 'path\\_%' ESCAPE '\\'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            leftovers, 0,
            "the migration must leave no scratch copy of `path` behind"
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

    /// The DDL a version-1 database ends up with, comments removed and
    /// whitespace collapsed, as one sha256.
    ///
    /// Comments are stripped so that explaining the schema stays free and only
    /// a change to what SQLite actually enforces trips the guard below.
    fn version_one_fingerprint(conn: &Connection) -> String {
        let statements: Vec<String> = {
            let mut q = conn
                .prepare(
                    "SELECT sql FROM sqlite_master
                      WHERE sql IS NOT NULL ORDER BY type, name",
                )
                .unwrap();
            q.query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        let bare: Vec<String> = statements
            .iter()
            .map(|sql| {
                sql.lines()
                    .map(|line| match line.find("--") {
                        Some(at) => &line[..at],
                        None => line,
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();

        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(bare.join("\n").as_bytes());
        h.finalize().iter().fold(String::new(), |mut out, b| {
            out.push_str(&format!("{b:02x}"));
            out
        })
    }

    /// `schema.sql` is migration 1 and migration 1 has shipped, so its DDL is
    /// frozen. This is the guard that makes that a rule rather than a sentence.
    ///
    /// **The gap it fills is the one the `path` columns nearly walked into.**
    /// Adding a column in `schema.sql` that a later `ALTER` also adds is caught
    /// loudly, by `the_migration_set_is_valid`, because SQLite refuses the fresh
    /// install with "duplicate column name". Every *other* edit in place is
    /// silent: change a CHECK, change the tokenizer, drop an index, and
    /// `validate()` still passes — a fresh database gets the new rule while
    /// every database already on disk keeps the old one, for ever, with nothing
    /// anywhere reporting that the two now disagree. Divergence, not breakage,
    /// which is why no existing test could see it.
    ///
    /// Three comments in this repository invited exactly that edit — they were
    /// written at `SCHEMA_VERSION` 1 and said the file was still free to change
    /// in place. They have been corrected; this test is what keeps the rule
    /// true after the next person stops reading comments.
    ///
    /// **When this fails**, the question is which kind of change it is. A change
    /// to what SQLite enforces belongs in a new `M::up` in `migrations()` with
    /// `SCHEMA_VERSION` bumped — never here. Only a deliberate, DDL-visible
    /// correction that genuinely cannot ship as a migration justifies updating
    /// the constant, and there is no such case today.
    #[test]
    fn the_shipped_schema_is_frozen_and_changes_belong_in_a_new_migration() {
        register_vector_extension().unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        migrations()
            .to_version(&mut conn, 1)
            .expect("version 1 is the schema that shipped");

        assert_eq!(
            version_one_fingerprint(&conn),
            "b5dbb0908a1ac8fb5f546745b613f6a84405427959eea136135e49ac1079a065",
            "schema.sql is migration 1 and has shipped: its DDL is frozen. \
             Express the change as a new M::up in migrations() and bump \
             SCHEMA_VERSION — editing this file in place leaves every database \
             already on disk with the old rule and nothing to say so."
        );
    }
}
