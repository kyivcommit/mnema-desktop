# Mutation cases for Task 3: the `reader` and `reader_version` columns on
# `path`. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-3.sh
#
# Two columns whose whole purpose is to be *compared* later, by Task 4, against
# a worker's manifest. That makes every way of getting them wrong silent in the
# same way: the build works, the index fills up, and a file whose format changed
# hands is never read again — or is re-read on every walk for ever. Neither
# leaves a crash, a log line or a failing query. So each wrong is broken here
# deliberately and required to turn a test red.
#
# The case the whole file is arranged around is C1. `schema.sql` IS migration 1:
# it runs in full on every fresh database before migration 2, so the two columns
# may live in one place or the other and never both. Writing them into
# `schema.sql` — the tidy-looking change, the one that makes the file describe
# the current schema again — breaks the FRESH INSTALL, which is the install
# nobody testing an upgrade will run. Measured while writing this task, not
# supposed.

# C1. The frozen schema. `duplicate column name: reader` on a fresh database is
# what this catches, and `validate()` is the test because it runs every
# migration for real against a new in-memory database — exactly the case a
# migration test on an existing database cannot see.
case_ "schema.sql is frozen at migration 1 and must not grow these columns" \
  crates/mnema-index/src/schema.sql \
  's~    mtime           INTEGER NOT NULL,\n    PRIMARY KEY \(watched_root_id, relative_path\)~    mtime           INTEGER NOT NULL,\n    reader          TEXT NOT NULL,\n    PRIMARY KEY (watched_root_id, relative_path)~' \
  '    reader          TEXT NOT NULL,
    PRIMARY KEY (watched_root_id, relative_path)' \
  mnema-index 'migrations::tests::the_migration_set_is_valid' --lib

# C2. The migration has to be in the list at all. Without it `apply` leaves a
# database at version 1 and the columns simply never arrive — the shape a
# session that edits `SCHEMA_VERSION` and forgets the `M::up` produces.
# Marker updated for PR 8a task 1 (`8f13e5f`), which appended
# `M::up(ADD_IGNORE_RULE_UNIQUE)` after this migration — so deleting this
# line no longer collapses the list down to the bare schema entry, it now
# leaves that third migration immediately behind it. Staleness debt from
# that commit, not from this case's own subject.
case_ "the migration is registered, not merely written" \
  crates/mnema-index/src/migrations.rs \
  's~        M::up\(ADD_PATH_READER\),\n~~' \
  '        M::up(include_str!("schema.sql")),
        M::up(ADD_IGNORE_RULE_UNIQUE),
    ])' \
  mnema-index \
  'migrations::tests::an_existing_database_keeps_its_rows_and_the_migration_credits_them_all_to_text' --lib

# C3 and C4. The defaults are values, not decoration, and this is the pair the
# brief calls out: `NOT NULL` is satisfied by the empty string and by 0, so the
# schema cannot tell either of these apart from the correct answer. A row
# migrated to `''` or to `0` matches no manifest Task 4 will ever hold, so its
# file is handed to a worker on every single walk, for ever.
case_ "the default reader is 'text', and the empty string satisfies NOT NULL" \
  crates/mnema-index/src/migrations.rs \
  "s~ADD COLUMN reader TEXT NOT NULL DEFAULT 'text';~ADD COLUMN reader TEXT NOT NULL DEFAULT '';~" \
  "ADD COLUMN reader TEXT NOT NULL DEFAULT '';" \
  mnema-index \
  'migrations::tests::an_existing_database_keeps_its_rows_and_the_migration_credits_them_all_to_text' --lib

case_ "the default version is 1, and 0 satisfies NOT NULL just as well" \
  crates/mnema-index/src/migrations.rs \
  's~ADD COLUMN reader_version INTEGER NOT NULL DEFAULT 1;~ADD COLUMN reader_version INTEGER NOT NULL DEFAULT 0;~' \
  'ADD COLUMN reader_version INTEGER NOT NULL DEFAULT 0;' \
  mnema-index \
  'migrations::tests::an_existing_database_keeps_its_rows_and_the_migration_credits_them_all_to_text' --lib

# C5. `ADD COLUMN` rather than a rebuild, and the mutation is the rebuild
# written out the way it is usually written: rename, create, copy, drop. It
# produces a `path` table with both new columns, all the old rows, and the
# right defaults — the migration test's first three assertions all pass — while
# `WITHOUT ROWID` and `ix_path_document` are gone. That is the reason those two
# assertions are in the test at all, and this is what holds them there.
case_ "the migration adds columns rather than rebuilding the table" \
  crates/mnema-index/src/migrations.rs \
  "s~ALTER TABLE path ADD COLUMN reader TEXT NOT NULL DEFAULT 'text';\nALTER TABLE path ADD COLUMN reader_version INTEGER NOT NULL DEFAULT 1;~ALTER TABLE path RENAME TO path_old;\nCREATE TABLE path (watched_root_id INTEGER NOT NULL, relative_path TEXT NOT NULL, document_id TEXT NOT NULL, size_bytes INTEGER NOT NULL, mtime INTEGER NOT NULL, reader TEXT NOT NULL DEFAULT 'text', reader_version INTEGER NOT NULL DEFAULT 1, PRIMARY KEY (watched_root_id, relative_path));\nINSERT INTO path SELECT watched_root_id, relative_path, document_id, size_bytes, mtime, 'text', 1 FROM path_old;\nDROP TABLE path_old;~" \
  'ALTER TABLE path RENAME TO path_old;' \
  mnema-index \
  'migrations::tests::an_existing_database_keeps_its_rows_and_the_migration_credits_them_all_to_text' --lib

# C5b. The other half of the rebuild, and the half C5 cannot show: a rebuild
# whose final `DROP TABLE path_old` is forgotten leaves every row of the index
# duplicated under a scratch name — invisible to every query, and a second copy
# again on the next migration that does the same. Written here as the leftover
# table directly, because a rebuild that produced it would trip the `WITHOUT
# ROWID` assertion first and this one would never be reached.
case_ "the migration leaves no scratch copy of path behind" \
  crates/mnema-index/src/migrations.rs \
  's~ALTER TABLE path ADD COLUMN reader_version INTEGER NOT NULL DEFAULT 1;~ALTER TABLE path ADD COLUMN reader_version INTEGER NOT NULL DEFAULT 1;\nCREATE TABLE path_old AS SELECT * FROM path;~' \
  'CREATE TABLE path_old AS SELECT * FROM path;' \
  mnema-index \
  'migrations::tests::an_existing_database_keeps_its_rows_and_the_migration_credits_them_all_to_text' --lib

# C10. The freeze, in the form C1 cannot see. C1 catches a column added to
# `schema.sql` only because SQLite refuses the fresh install with "duplicate
# column name" — loud, and specific to adding a column. Every other edit in
# place is silent: this one changes the `text_source` CHECK from GLOB back to
# the LIKE it was fixed away from, adds no column, and leaves
# `the_migration_set_is_valid` GREEN — a fresh database gets the new rule while
# every database already on disk keeps the old one, for ever, with nothing
# anywhere reporting the divergence. Measured: green on validate, red only here.
case_ "an in-place edit to the shipped DDL that adds no column is still caught" \
  crates/mnema-index/src/schema.sql \
  "s~CHECK \(text_source GLOB 'native:\*' OR text_source GLOB 'ocr:\*'\)~CHECK (text_source LIKE 'native:%' OR text_source LIKE 'ocr:%')~" \
  "CHECK (text_source LIKE 'native:%' OR text_source LIKE 'ocr:%')" \
  mnema-index \
  'migrations::tests::the_shipped_schema_is_frozen_and_changes_belong_in_a_new_migration' --lib

# C10's other direction has no case here, because this harness only reports
# red. It was measured directly instead, and it matters just as much: rewriting
# a comment INSIDE the `CREATE TABLE path` statement — text SQLite stores in
# `sqlite_master`, which a naive fingerprint would trip over — leaves
# `the_shipped_schema_is_frozen…` green. A guard that went red on prose would be
# switched off within a week, so explaining the schema has to stay free.

# C6. `insert_path` writes what it is handed. Ignoring both arguments and
# writing the migration's own defaults instead compiles, and — this is the
# point — leaves `a_path_row_remembers_which_reader_made_it` GREEN, because
# that test's expected values are `"text"` and `1` too. The second round-trip
# test exists solely for this mutation.
case_ "insert_path writes the reader it was given, not the migration's default" \
  crates/mnema-index/src/write.rs \
  's~                reader,\n                reader_version\n~                "text",\n                1\n~' \
  '                "text",
                1
            ],' \
  mnema-index 'a_path_row_carries_the_reader_it_was_given_and_not_the_default' --test roundtrip

# C7. And `path_entry` reads them back off the row rather than answering from a
# constant. Same mutation one direction later, and it stays green against the
# other round-trip test for the same reason.
case_ "path_entry reads the two columns instead of answering with a constant" \
  crates/mnema-index/src/write.rs \
  's~                        reader: r\.get\(3\)\?,\n                        reader_version: r\.get\(4\)\?,~                        reader: "text".to_string(),\n                        reader_version: 1,~' \
  '                        reader: "text".to_string(),
                        reader_version: 1,' \
  mnema-index 'a_path_row_carries_the_reader_it_was_given_and_not_the_default' --test roundtrip

# C8 and C9. The production write, and the only two cases that reach the real
# worker. `repoint` is where the column stops being a parameter and becomes a
# fact about a file, and nothing in `mnema-index` can hold it to that: every
# test there hands `insert_path` its own literals.
#
# Two cases, not one, because a single file cannot show both. Hardcoding
# `"text"` credits the text reader for markdown; hardcoding `"markdown"` does
# the reverse. Each mutation satisfies the half of the test the other breaks,
# which is what the two-file fixture is for — a one-file test would be silent
# about whichever direction it did not cover.
case_ "repoint names the worker's reader and not the text one" \
  crates/mnema-ingest/src/lib.rs \
  's~        &document\.reader,\n        i64::from\(document\.reader_version\),~        "text",\n        1,~' \
  '        "text",
        1,
    )?;' \
  mnema-ingest 'a_path_row_credits_the_reader_the_worker_actually_ran' --test slice

case_ "repoint does not credit markdown for what the text reader read" \
  crates/mnema-ingest/src/lib.rs \
  's~        &document\.reader,\n        i64::from\(document\.reader_version\),~        "markdown",\n        1,~' \
  '        "markdown",
        1,
    )?;' \
  mnema-ingest 'a_path_row_credits_the_reader_the_worker_actually_ran' --test slice
