# Mutation cases from the whole-branch review. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/branch-review.sh
#
# Two families, both of them a floor that was set too low rather than a check
# that was missing.
#
# The first is the foreign-key walk in mnema-index: it asserted `checked >= 15`
# against a schema declaring 20, so a quarter of the referential integrity could
# go with every test green. Five constraints were deleted to prove it. Each one
# is a case below, and each must red now.
#
# The second is the workflow lint: it read only the `bundle` job, so three lines
# in `check` whose loss is silent were guarded by nothing — including the
# `--include-ignored` line that the lint's own header cites as the reason it
# exists.

# --- the foreign keys, one case per constraint --------------------------------
#
# Each removes the REFERENCES clause and leaves a valid column definition, so
# the schema still applies and the database still opens. That is the point: the
# damage is invisible until something is deleted at run time.

case_ "schema: a chunk's search row must go when the chunk does" \
  crates/mnema-index/src/schema.sql \
  's{CREATE TABLE chunk_search \(\n    chunk_id INTEGER PRIMARY KEY REFERENCES chunk\(id\) ON DELETE CASCADE,}{CREATE TABLE chunk_search (\n    chunk_id INTEGER PRIMARY KEY,}' \
  'CREATE TABLE chunk_search (
    chunk_id INTEGER PRIMARY KEY,' \
  mnema-index 'every_foreign_key_names_a_table_and_column_that_exist' --test schema

case_ "schema: a path belongs to a document" \
  crates/mnema-index/src/schema.sql \
  's{    document_id     TEXT NOT NULL REFERENCES document\(id\) ON DELETE CASCADE,\n    size_bytes}{    document_id     TEXT NOT NULL,\n    size_bytes}' \
  '    document_id     TEXT NOT NULL,
    size_bytes' \
  mnema-index 'every_foreign_key_names_a_table_and_column_that_exist' --test schema

case_ "schema: an ignore rule's tag is a tag" \
  crates/mnema-index/src/schema.sql \
  's{    tag_id          INTEGER REFERENCES tag\(id\) ON DELETE CASCADE,\n    CHECK}{    tag_id          INTEGER,\n    CHECK}' \
  '    tag_id          INTEGER,
    CHECK' \
  mnema-index 'every_foreign_key_names_a_table_and_column_that_exist' --test schema

case_ "schema: an embedding state belongs to a space" \
  crates/mnema-index/src/schema.sql \
  's{    space_id     INTEGER NOT NULL REFERENCES embedding_space\(id\) ON DELETE CASCADE,}{    space_id     INTEGER NOT NULL,}' \
  '    space_id     INTEGER NOT NULL,
    chunk_id' \
  mnema-index 'every_foreign_key_names_a_table_and_column_that_exist' --test schema

case_ "schema: an embedding state belongs to a chunk" \
  crates/mnema-index/src/schema.sql \
  's{    chunk_id     INTEGER NOT NULL REFERENCES chunk\(id\) ON DELETE CASCADE,}{    chunk_id     INTEGER NOT NULL,}' \
  '    chunk_id     INTEGER NOT NULL,
    content_hash' \
  mnema-index 'every_foreign_key_names_a_table_and_column_that_exist' --test schema

# --- the check job ------------------------------------------------------------
#
# `--ignored` selects ONLY ignored tests, so the mutation below is not a
# strawman: it is the exact line task 7 measured running zero tests and exiting
# 0. Until this review, putting it back left the workspace green.

case_ "workflow: the keychain step must not select only ignored tests" \
  .github/workflows/ci.yml \
  's{roundtrip -- --include-ignored}{roundtrip -- --ignored}' \
  'roundtrip -- --ignored' \
  mnema-desktop 'the_check_job_keeps_the_lines_whose_loss_would_not_show' --test packaging_workflow

# The two markers below name the lines LEFT BEHIND by the deletion, and they were
# re-anchored by task 16 after that task moved the vendoring step above `clippy`:
# both used to lean on `fetch-pdfium.sh` sitting immediately above
# `cargo test --workspace`, an adjacency that no longer exists. Anchored now on the
# comment line each deletion joins to, which neither mutation moves.
case_ "workflow: the matrix must actually run the tests" \
  .github/workflows/ci.yml \
  's{\n      - run: cargo test --workspace\n}{\n}' \
  'matrix have a pin in that script; Linux was added for this matrix.
      # `cargo test --workspace` above' \
  mnema-desktop 'the_check_job_keeps_the_lines_whose_loss_would_not_show' --test packaging_workflow

case_ "workflow: the tests need the library vendored first" \
  .github/workflows/ci.yml \
  's{        run: scripts/fetch-pdfium\.sh\n}{}' \
  'which src-tauri now needs to compile
      - run: cargo clippy' \
  mnema-desktop 'the_check_job_keeps_the_lines_whose_loss_would_not_show' --test packaging_workflow

# The line moves out of `check` and into `bundle`, where it does nothing for the
# test suite. A lint that reads the whole file, or the wrong job, is satisfied.
case_ "workflow: the keychain step must be in the job that runs the tests" \
  .github/workflows/ci.yml \
  's{\n          cargo test -p mnema-secrets --test roundtrip -- --include-ignored\n}{\n};s{\z}{\n      - name: not the check job\n        run: cargo test -p mnema-secrets --test roundtrip -- --include-ignored\n}' \
  '      - name: not the check job
        run: cargo test -p mnema-secrets --test roundtrip -- --include-ignored' \
  mnema-desktop 'the_check_job_keeps_the_lines_whose_loss_would_not_show' --test packaging_workflow

# The `check` slice has a real end in the real file, because `check` is not the
# last job — this is the only assertion in the suite that exercises the end
# condition against ci.yml itself rather than against a document written for it.
case_ "slice: the check job must not run on into bundle" \
  src-tauri/tests/packaging_workflow.rs \
  's{    line\.starts_with\("  "\)\n}{    false\n}' \
  '    false
        && !line.starts_with("   ")' \
  mnema-desktop 'the_job_slice_starts_at_the_job_it_names' --test packaging_workflow

# Dropping a matrix arm reddens nothing on its own: the run goes green having
# compiled the crate list on one target, which is the check ci.yml argues at
# length is almost no check at all. The silent-loss class, one layer out from
# the settings inside the job.
case_ "workflow: the check matrix must keep both platforms" \
  .github/workflows/ci.yml \
  's{os: \[macos-14, ubuntu-24\.04\]}{os: [macos-14]}' \
  'os: [macos-14]' \
  mnema-desktop 'the_check_job_keeps_the_lines_whose_loss_would_not_show' --test packaging_workflow
