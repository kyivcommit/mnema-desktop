# Mutation cases for Task 4: the cheap arm's fourth condition, and the manifest
# it compares against. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-4.sh
#
# Every wrong answer here is silent, and the two of them are opposite. Too
# strict and the file is handed to a worker on every walk for ever, on a folder
# where nothing changed. Too lax and a document made by a reader this build no
# longer has answers `Unchanged` for the life of the index — no crash, no log
# line, no failing query, just search results out of a reading nothing performs
# any more. So both directions are broken deliberately below and each is
# required to turn a test red.
#
# The pair the whole file is arranged around is C1/C2: the condition compares a
# name AND a version, and no single fixture can show both. A build that drops
# the name comparison still notices a version bump; one that drops the version
# comparison still notices `.html` changing hands. Each mutation is satisfied by
# whichever test the other one breaks.

# C1. The reader's name is compared. Dropped, `text@1` and `html@1` differ only
# in a field nothing reads, so the day html grows a reader of its own not one
# already-indexed page is read again — the exact failure the extension-keyed
# manifest exists to prevent.
case_ "the cheap arm compares which reader made the row" \
  crates/mnema-ingest/src/lib.rs \
  's~        && recorded\.reader == expected\.reader\n~~' \
  '        && recorded.reader_version == i64::from(expected.version)' \
  mnema-ingest 'a_file_is_reread_when_its_extension_changed_hands' --test slice

# C2. And its version. Dropped, a reader that changes what it produces from the
# same bytes — markdown learning tables — leaves every document it already made
# standing, because the name did not move. This is the case a name-only
# comparison passes C1 with.
case_ "the cheap arm compares the reader's version too" \
  crates/mnema-ingest/src/lib.rs \
  's~        && recorded\.reader_version == i64::from\(expected\.version\)\n~~' \
  '        && recorded.reader == expected.reader
        && db' \
  mnema-ingest 'a_bumped_reader_version_is_a_reader_that_changed_too' --test slice

# C3. The other direction, and the one a condition written to always fire
# produces: comparing the row against itself is true for every file, for ever,
# which is what "re-read nothing, ever" looks like from inside the arm. It
# passes C1 and C2 both — the clauses are still there — and is caught only by
# the half of the html test that asserts the file IS read again.
case_ "the fourth condition compares the row against the manifest, not against itself" \
  crates/mnema-ingest/src/lib.rs \
  's~        && recorded\.reader == expected\.reader\n        && recorded\.reader_version == i64::from\(expected\.version\)~        \&\& recorded.reader == recorded.reader\n        \&\& recorded.reader_version == recorded.reader_version~' \
  '        && recorded.reader == recorded.reader' \
  mnema-ingest 'a_file_is_reread_when_its_extension_changed_hands' --test slice

# C4. And the opposite excess: an arm that never answers `Unchanged` re-reads
# every file on every walk, which no assertion about a file being read again can
# see. The unchanged-file test is the one that can, and it is why the html test
# asserts both directions rather than only the interesting one.
case_ "an unchanged file under an unchanged manifest still costs nothing" \
  crates/mnema-ingest/src/lib.rs \
  's~        && recorded\.reader == expected\.reader~        \&\& recorded.reader != expected.reader~' \
  '        && recorded.reader != expected.reader' \
  mnema-ingest 'an_unchanged_file_is_not_read_a_second_time' --test slice

# C5. The lookup is keyed on the extension, and `None` is a real answer rather
# than a safe one: it sends every file to the default reader, so every `.md`
# already indexed disagrees with the manifest on every walk and is handed to a
# worker for ever. Caught by the third pass of the migration test, which is the
# only assertion anywhere that the re-read a migrated row costs is a ONE-off.
case_ "the extension is taken from the path, not answered as unknown" \
  crates/mnema-ingest/src/lib.rs \
  's~    Path::new\(relative\)\.extension\(\)\.and_then\(\|ext\| ext\.to_str\(\)\)~    let _ = relative;\n    None~' \
  '    let _ = relative;
    None' \
  mnema-ingest \
  'a_row_the_migration_credited_to_text_is_read_again_once_and_then_settles' --test slice

# C6. The whole file name is not the extension. `Manifest::for_extension` takes
# whatever it is handed and looks it up exactly, so `notes.md` misses the map
# and falls to the default — indistinguishable from C5 in effect, and written
# separately because it is the mistake someone actually makes: a split that
# forgets the extension is the part after the last dot, not the name.
case_ "the extension is the part after the dot, not the file name" \
  crates/mnema-ingest/src/lib.rs \
  's~    Path::new\(relative\)\.extension\(\)\.and_then\(\|ext\| ext\.to_str\(\)\)~    Path::new(relative).file_name().and_then(|ext| ext.to_str())~' \
  '    Path::new(relative).file_name().and_then(|ext| ext.to_str())' \
  mnema-ingest \
  'a_row_the_migration_credited_to_text_is_read_again_once_and_then_settles' --test slice

# C7. The manifest is asked of the worker, or it is invented. A parent that
# answers its own question when the binary cannot — with the defaults it happens
# to know, here — decides the freshness of every file in the index from a value
# nothing measured, and does it without a word. This is the case that makes
# `Pool::manifest` return `Result` at all rather than `Manifest`.
case_ "a binary that cannot state its readers is refused, not guessed at" \
  crates/mnema-pool/src/lib.rs \
  's~        serde_json::from_slice\(&out\.stdout\)\.map_err\(\|source\| \{~        if true {\n            return Ok(Manifest {\n                default: mnema_core::manifest::ReaderId::new("text", 1),\n                by_extension: Default::default(),\n            });\n        }\n        serde_json::from_slice(\&out.stdout).map_err(|source| {~' \
  '            return Ok(Manifest {' \
  mnema-ingest \
  'a_worker_that_cannot_state_its_readers_stops_the_walk_before_any_file' --test walk

# C8. The walk asks once and hands the same answer to every file. Asking per
# file would be a process per file — the cost the whole cheap arm exists to
# avoid — and there is no fixture that can tell "asked once" from "asked forty
# thousand times" by its answers, so the case here breaks the OTHER half: a walk
# that never asks at all, and hands out a manifest of its own. Same silence as
# C7, one level up, and it is `walk_root` rather than `Pool` that owns it.
case_ "the walk compares against the worker's manifest, not one of its own" \
  crates/mnema-ingest/src/walk.rs \
  's~    let manifest = pool\.manifest\(\)\?;~    let manifest = mnema_core::manifest::Manifest {\n        default: mnema_core::manifest::ReaderId::new("text", 1),\n        by_extension: Default::default(),\n    };~' \
  '    let manifest = mnema_core::manifest::Manifest {' \
  mnema-ingest \
  'a_worker_that_cannot_state_its_readers_stops_the_walk_before_any_file' --test walk

# What has no case here, named rather than left to be discovered: the exit-status
# check inside `Pool::manifest`. Removing it leaves every test green, and that is
# honest rather than a gap in the tests — both stand-ins that fail the handshake
# fail it by printing nothing, so the parse below catches them first. Separating
# the two needs a worker that prints a VALID manifest and then exits non-zero,
# which is a stand-in written to make one line red rather than to model anything
# this product meets. The check stays, uncased.
