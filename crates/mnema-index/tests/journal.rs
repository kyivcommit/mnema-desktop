//! The three journals `schema.sql` sets aside for work that did not make it
//! into the index, or that already has: `skipped`, `document.status` and
//! `ingest_stage`. Before this file none of the three had a writer anywhere —
//! `grep "INTO skipped"` over the crates returned nothing but the schema
//! itself.

use mnema_core::{OnDisk, SourceKind};
use mnema_index::{Db, DocumentStatus, SkipRule, open, register_vector_extension};

fn fresh(dir: &tempfile::TempDir) -> Db {
    register_vector_extension().unwrap();
    open(&dir.path().join("index.sqlite")).unwrap()
}

/// Keeps the backing `TempDir` alive alongside the `Db` that opened a file
/// inside it — the same shape `tests/citation.rs`'s `OnePage` uses, and for
/// the same reason: dropping the directory out from under an open connection
/// is not something a fixture should risk.
struct Fixture {
    db: Db,
    _dir: tempfile::TempDir,
}

impl std::ops::Deref for Fixture {
    type Target = Db;
    fn deref(&self) -> &Db {
        &self.db
    }
}

/// One watched root, id 1, and nothing else. `record_skip` takes a root id and
/// `skipped.watched_root_id` is a foreign key, so a skip cannot be recorded
/// against a root that was never inserted.
fn fixture_empty() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    db.insert_watched_root("/Volumes/Archive").unwrap();
    Fixture { db, _dir: dir }
}

/// `fixture_empty` plus one document, `"doc-1"`, for the status tests.
fn fixture_one_document() -> Fixture {
    let f = fixture_empty();
    f.db.insert_document("doc-1", "application/pdf", 1, SourceKind::Document)
        .unwrap();
    f
}

#[test]
fn a_skipped_file_names_the_rule_that_fired() {
    let db = fixture_empty();
    db.record_skip(
        1,
        "reports/broken.pdf",
        None,
        "worker died on SIGSEGV",
        SkipRule::Crash,
        None,
    )
    .unwrap();
    let rows = db.skips_for_root(1).unwrap();
    assert_eq!(rows[0].rule, "crash");
    assert_eq!(rows[0].page_no, None);
}

#[test]
fn a_page_without_a_text_layer_is_recorded_against_that_page() {
    let db = fixture_empty();
    db.record_skip(
        1,
        "scan.pdf",
        Some(4),
        "no text layer",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();
    assert_eq!(db.skips_for_root(1).unwrap()[0].page_no, Some(4));
}

// ------------------------------------------------- what every rule answers

/// What this file expects of one rule: the string it is stored under, and its
/// side of each of the two classifications.
struct Expected {
    string: &'static str,
    about_content: bool,
    broken_environment: bool,
}

/// The one place this file writes down what a rule answers, and an exhaustive
/// `match` rather than the array of pairs the three tests below used to carry
/// one each.
///
/// **The arrays could not fail the way they claimed to.** Measured twice,
/// independently: deleting `SkipRule::NotText` from all three of them left all
/// fifteen tests in this file green. An array asserts about its own elements
/// and can say nothing about an element that is not there — so "every variant"
/// was a promise made in a doc comment and kept nowhere, which is the shape of
/// an `assert_ne!` satisfied by anything, one level up. What the arrays *did*
/// catch was a wrong value in a row they already had: renaming `not_text` to
/// `nottext` failed two tests, including the one that goes through SQLite.
///
/// Two things had to change together, and neither is sufficient alone:
///
/// * this `match` is exhaustive, so a variant added to `SkipRule` stops this
///   file **compiling** until someone writes down what the new rule answers —
///   the decision cannot be deferred, and it cannot be made silently;
/// * the tests iterate [`SkipRule::every`], so the new variant is actually
///   *run* through all three assertions. Without that half, an arm could be
///   added here and the rule still never tested.
fn expected(rule: SkipRule) -> Expected {
    match rule {
        SkipRule::Crash => Expected {
            string: "crash",
            about_content: false,
            broken_environment: true,
        },
        SkipRule::Timeout => Expected {
            string: "timeout",
            about_content: false,
            broken_environment: true,
        },
        SkipRule::Memory => Expected {
            string: "memory",
            about_content: false,
            broken_environment: true,
        },
        SkipRule::Unsupported => Expected {
            string: "unsupported",
            about_content: true,
            broken_environment: false,
        },
        SkipRule::NoTextLayer => Expected {
            string: "no_text_layer",
            about_content: true,
            broken_environment: false,
        },
        SkipRule::Unreadable => Expected {
            string: "unreadable",
            about_content: false,
            broken_environment: true,
        },
        // Neither: not a fact about the bytes (a setting can move the ceiling
        // out from under a file that never changed), and not a broken machine
        // either (a folder with a few large archives is an ordinary folder).
        SkipRule::TooLarge => Expected {
            string: "too_large",
            about_content: false,
            broken_environment: false,
        },
        SkipRule::NotText => Expected {
            string: "not_text",
            about_content: true,
            broken_environment: false,
        },
        // A determination about the bytes, like `NotText`. The two part company
        // one level up, in `mnema_ingest`'s `displaces`, not here.
        SkipRule::BinaryTail => Expected {
            string: "binary_tail",
            about_content: true,
            broken_environment: false,
        },
        // Both are readings of the file: the same damage stops the same reader
        // again, and the same password is still missing. Neither says anything
        // about the machine — a folder holding several interrupted downloads is
        // an ordinary folder, which is the mistake `TooLarge`'s own row exists
        // to keep out of the other column.
        SkipRule::Malformed => Expected {
            string: "malformed",
            about_content: true,
            broken_environment: false,
        },
        SkipRule::Encrypted => Expected {
            string: "encrypted",
            about_content: true,
            broken_environment: false,
        },
    }
}

/// `SkipRule::every` is what makes the three tests below mean "every rule", so
/// what it yields is worth an assertion of its own.
///
/// **What this test no longer has to check, and why.** It used to assert the
/// enumeration's length against the literal `9`, because `every()` walked a
/// hand-written chain of `after()` links and a chain can stop early. It did
/// stop early when measured: a tenth variant whose only `after()` arm was
/// `=> return None` compiled, left the chain nine long, and left this file
/// green. The length assertion was the sole guard and it was satisfied by
/// exactly the fault it existed to catch. `every()` now reads a slice generated
/// from the list that declares the variants (`declare_skip_rules` in
/// `journal.rs`), so a count here would compare a generated list against a
/// literal someone has to remember to bump — noise, not a guard.
///
/// What is left to check is what the generator does *not* cover, and both are
/// real:
///
/// * `as_str` is a hand-written exhaustive `match`, so a new variant has to
///   name a string — but nothing stops it naming one another variant already
///   uses, and a collision is invisible to a `match` the compiler is happy
///   with;
/// * `parse` matches on strings, so the compiler cannot force it to grow an arm
///   for a new variant at all. A rule written to the journal and unable to come
///   back out is a row `skips_for_root` lists as an unknown.
#[test]
fn every_rule_is_enumerated_once_and_answers_to_its_own_string() {
    let all: Vec<SkipRule> = SkipRule::every().collect();
    assert!(!all.is_empty(), "`every` yielded nothing at all");

    // Keyed by the variant, not by its string: a string collision is a
    // different fault, checked next, and keying by string here would report it
    // under a message that names the wrong one.
    let mut seen = std::collections::HashSet::new();
    for rule in &all {
        assert!(seen.insert(*rule), "{rule:?} is enumerated twice");
    }

    let mut strings = std::collections::HashSet::new();
    for rule in &all {
        assert!(
            strings.insert(rule.as_str()),
            "{rule:?} is stored under {:?}, which another rule already uses — \
             two rules sharing a string are one rule to every query",
            rule.as_str()
        );
    }

    for rule in &all {
        assert_eq!(
            SkipRule::parse(rule.as_str()),
            Some(*rule),
            "{rule:?} is written as {:?} and does not come back as itself",
            rule.as_str()
        );
    }
}

/// The two tests above between them pin only `Crash` and `NoTextLayer`, and
/// only through `page_no`/one literal — neither ever checks `Timeout`,
/// `Memory` or `Unsupported`, and neither compares `rows[0].rule` against
/// anything for the page-without-a-text-layer case. A mutation pass proved
/// the gap: renaming `SkipRule::NoTextLayer`'s string to `"crash"` left every
/// test in this file green. Each variant is asserted against its own string
/// here, so a rename or a collision between any two of the five is caught
/// regardless of which one moved.
#[test]
fn every_skip_rule_is_recorded_under_its_own_string() {
    let db = fixture_empty();
    let rules: Vec<SkipRule> = SkipRule::every().collect();
    for (i, rule) in rules.iter().enumerate() {
        db.record_skip(1, &format!("file-{i}.pdf"), None, "reason", *rule, None)
            .unwrap();
    }

    // Round-tripped through SQLite, not just through `as_str`: this is the one
    // test that proves the string a rule is written under is the string it
    // comes back as.
    let rows = db.skips_for_root(1).unwrap();
    let got: Vec<&str> = rows.iter().map(|r| r.rule.as_str()).collect();
    let want: Vec<&str> = rules.iter().map(|r| expected(*r).string).collect();
    assert_eq!(got, want);
}

/// Every `SkipRule` variant, sorted onto its side of `is_about_content`
/// explicitly rather than derived from a count or a default. The mis-sorting
/// this pins — `TooLarge`, a fact about the *setting* `PoolConfig::max_bytes`
/// rather than about the file, wrongly placed on the content side — was
/// caught only by the randomised harness in `mnema-ingest`, on a random seed,
/// which means it was caught *sometimes*. This is the deterministic form.
///
/// On its own it does not force a decision about a NEW variant, and it was
/// measured making exactly that mistake look covered: a variant added to the
/// enum with no line here left this whole suite green. What forces the
/// decision is that `is_about_content` is an exhaustive `match` — adding a
/// variant stops the crate compiling until someone picks a side. This list is
/// the other half: it says which side each existing variant is on, in one
/// place, where a wrong answer is legible.
#[test]
fn every_skip_rule_is_sorted_onto_its_side_of_is_about_content() {
    for rule in SkipRule::every() {
        assert_eq!(
            rule.is_about_content(),
            expected(rule).about_content,
            "{rule:?} is on the wrong side of is_about_content"
        );
    }
}

/// Every `SkipRule` variant, sorted onto its side of
/// `suggests_broken_environment` explicitly — the same discipline as the test
/// above, and for the same reason: the mis-sorting this pins is `TooLarge`
/// again, this time on the OTHER predicate. A folder that holds a few large
/// archives in a row is not a broken worker, and until this line existed the
/// mistake of treating "not about content" as "suggests a broken
/// environment" had nothing here to catch it — mirroring exactly the defect
/// `every_skip_rule_is_sorted_onto_its_side_of_is_about_content`'s own doc
/// comment records for `is_about_content`, one predicate over.
#[test]
fn every_skip_rule_is_sorted_onto_its_side_of_suggests_broken_environment() {
    for rule in SkipRule::every() {
        assert_eq!(
            rule.suggests_broken_environment(),
            expected(rule).broken_environment,
            "{rule:?} is on the wrong side of suggests_broken_environment"
        );
    }
}

/// The two states the vocabulary had no word for until now, and the two it
/// must not be mistaken for.
///
/// A file that *is* the format its magic claims, that this product *has* a
/// reader for, and that the reader could not finish — a truncated PDF, a zip
/// whose central directory does not parse — used to have only `Unsupported` to
/// go under, which promises a reader that will arrive when one already has;
/// and a file whose text sits behind a password used to have the same. Neither
/// is `Unreadable` either: that rule is for a worker that came back having
/// learned nothing about the content — no bytes, or no reader to read them
/// with — and it is on the keeping side of `displaces`. These two are the
/// opposite, a reader that ran on bytes it had.
///
/// Both are determinations about the bytes — the same damaged bytes damage the
/// same reader again, and the same encrypted bytes stay encrypted — so both are
/// `is_about_content`, and neither says anything about the machine: a folder
/// holding several broken downloads is not a dying worker.
#[test]
fn a_damaged_file_is_not_the_same_verdict_as_an_unread_one() {
    assert!(SkipRule::Malformed.is_about_content());
    assert!(SkipRule::Encrypted.is_about_content());
    assert_eq!(SkipRule::parse("malformed"), Some(SkipRule::Malformed));
    assert_eq!(SkipRule::parse("encrypted"), Some(SkipRule::Encrypted));

    assert!(!SkipRule::Malformed.suggests_broken_environment());
    assert!(!SkipRule::Encrypted.suggests_broken_environment());

    // The pair is only worth two variants if the journal can tell them apart,
    // which is the whole of why `Encrypted` is not folded into `Malformed`:
    // one of the two is fixed with a password and the other is not fixed.
    assert_ne!(SkipRule::Malformed, SkipRule::Encrypted);
    assert_ne!(SkipRule::Malformed.as_str(), SkipRule::Encrypted.as_str());
}

/// D51. A refusal by content is not the same promise as "no reader yet".
#[test]
fn not_text_round_trips_and_is_classified_on_both_sides() {
    assert_eq!(SkipRule::NotText.as_str(), "not_text");
    assert_eq!(SkipRule::parse("not_text"), Some(SkipRule::NotText));

    // A determination about the bytes: the same bytes earn it again, so
    // the next walk may answer from `stat` alone.
    assert!(SkipRule::NotText.is_about_content());
    // And it says nothing about the machine: a folder of photos is not a
    // broken worker.
    assert!(!SkipRule::NotText.suggests_broken_environment());
}

/// The journal is a current state, not a history. Before this, `record_skip`
/// was an unconditional INSERT: a folder of a thousand scans grew a thousand
/// rows per walk, and every walk spent a worker process on each of them to
/// learn the same thing again.
#[test]
fn a_second_skip_of_the_same_file_replaces_the_first() {
    let db = fixture_empty();
    let root = db.insert_watched_root("/tmp/x").unwrap();

    db.record_skip(
        root,
        "a.pdf",
        None,
        "no text layer",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();
    db.record_skip(
        root,
        "a.pdf",
        None,
        "still none",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();

    let skips = db.skips_for_root(root).unwrap();
    assert_eq!(skips.len(), 1);
    assert_eq!(skips[0].reason, "still none");
}

/// What this test actually pins is `page_no` being *in* the unique key at
/// all, not the `COALESCE` wrapped around it. Drop `page_no` from the index
/// (and from `record_skip`'s `ON CONFLICT` arbiter) entirely and this fails
/// loudly — `left: 1, right: 2` — because the whole-file skip and the page-4
/// skip now share one key and the second overwrites the first. The COALESCE
/// trap proper — two whole-file skips of the same path silently failing to
/// dedup because SQLite treats NULL as DISTINCT from NULL — is what
/// `a_second_skip_of_the_same_file_replaces_the_first` above pins; this test
/// covers the other row that indexing shares, not that one.
#[test]
fn page_skips_and_file_skips_do_not_collide_but_each_still_dedups() {
    let db = fixture_empty();
    let root = db.insert_watched_root("/tmp/x").unwrap();

    db.record_skip(
        root,
        "a.pdf",
        None,
        "whole file",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();
    db.record_skip(
        root,
        "a.pdf",
        Some(4),
        "page four",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();
    db.record_skip(
        root,
        "a.pdf",
        Some(4),
        "page four again",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();

    let skips = db.skips_for_root(root).unwrap();
    assert_eq!(skips.len(), 2);
}

/// Content rules remember the bytes; environmental rules must not. A crash is a
/// statement about the worker, not about the file, and every file in the walk
/// is subject to it — D44's own asymmetry, reused rather than invented twice.
#[test]
fn only_content_rules_remember_the_bytes() {
    let db = fixture_empty();
    let root = db.insert_watched_root("/tmp/x").unwrap();

    db.record_skip(
        root,
        "a.bin",
        None,
        "no reader",
        SkipRule::Unsupported,
        Some(OnDisk {
            size_bytes: 10,
            mtime: 99,
        }),
    )
    .unwrap();
    db.record_skip(
        root,
        "b.pdf",
        None,
        "worker died",
        SkipRule::Crash,
        Some(OnDisk {
            size_bytes: 10,
            mtime: 99,
        }),
    )
    .unwrap();

    assert_eq!(
        db.skip_entry(root, "a.bin").unwrap().unwrap().size_bytes,
        Some(10)
    );
    assert_eq!(
        db.skip_entry(root, "b.pdf").unwrap().unwrap().size_bytes,
        None
    );
}

#[test]
fn a_document_that_failed_says_so() {
    let db = fixture_one_document();
    db.set_document_status("doc-1", DocumentStatus::Failed)
        .unwrap();
    assert_eq!(db.document_status("doc-1").unwrap(), DocumentStatus::Failed);
}

/// Same gap, the other table: only `Failed` had ever been written and read
/// back. Checked against the raw stored string rather than through
/// `document_status()`, so a bug shared between `as_str` and `parse` — the
/// two swapping the same pair of variants — cannot cancel itself out the way
/// a pure round trip could.
#[test]
fn every_document_status_is_written_as_its_own_string() {
    let cases = [
        (DocumentStatus::Pending, "pending"),
        (DocumentStatus::Indexed, "indexed"),
        (DocumentStatus::Failed, "failed"),
        (DocumentStatus::Skipped, "skipped"),
    ];
    for (status, expected) in cases {
        let db = fixture_one_document();
        db.set_document_status("doc-1", status).unwrap();
        let stored: String = db
            .conn()
            .query_row("SELECT status FROM document WHERE id = 'doc-1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stored, expected);
    }
}

/// `ingest_stage` is keyed on `(content_hash, stage)`, so re-recording the same
/// stage for the same document — exactly what happens when a multi-hour job is
/// resumed and repeats a stage it did not finish — must update the row rather
/// than fail with a uniqueness violation. Read back with raw SQL rather than
/// through `stage_status`, so that a bug shared between the writer and the
/// reader cannot cancel itself out.
#[test]
fn recording_a_stage_twice_updates_it_instead_of_failing() {
    let db = fixture_empty();
    let hash = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.record_stage(&hash, "extract", "done").unwrap();
    db.record_stage(&hash, "extract", "failed").unwrap();

    let status: String = db
        .conn()
        .query_row(
            "SELECT status FROM ingest_stage WHERE content_hash = ?1 AND stage = ?2",
            rusqlite::params![hash, "extract"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "failed");

    let rows: i64 = db
        .conn()
        .query_row(
            "SELECT count(*) FROM ingest_stage WHERE content_hash = ?1 AND stage = ?2",
            rusqlite::params![hash, "extract"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1, "the second call must update the row, not add one");
}

/// Until now `ingest_stage` could only be written. A checkpoint nothing reads
/// is not a checkpoint: the whole reason D26 asks for the table is that a
/// second pass over the same folder can skip a document whose stages are
/// already finished, and skipping needs an answer to "did this one finish?".
///
/// `Option`, and the distinction is the point. A stage that was never recorded
/// and a stage recorded as `failed` demand opposite things of the next run —
/// do the work, and do not do it again — so they must not both arrive as some
/// falsy value.
#[test]
fn a_stage_can_be_read_back_and_an_unrecorded_one_is_absent() {
    let db = fixture_empty();
    let hash = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();

    assert_eq!(
        db.stage_status(&hash, "chunk").unwrap(),
        None,
        "a document nobody has worked on has no stage recorded"
    );

    db.record_stage(&hash, "chunk", "done").unwrap();
    assert_eq!(
        db.stage_status(&hash, "chunk").unwrap().as_deref(),
        Some("done")
    );

    // The key is the pair, not the hash: another stage of the same document is
    // a different row and must not answer for this one.
    assert_eq!(db.stage_status(&hash, "embed").unwrap(), None);
    // …and neither must the same stage of another document.
    assert_eq!(db.stage_status(&"b".repeat(64), "chunk").unwrap(), None);
}

/// A checkpoint may not outlive the document it describes.
///
/// `ingest_stage` is keyed on the *content* hash, and content comes back: an
/// undo, a restore from backup, a file moved out and back. Without the cascade
/// a `done` stage sat waiting for the next document to carry that hash — and
/// since a fresh document is inserted at `status = 'pending'`, any interruption
/// before its own checkpoint left `done` over `pending` permanently, with every
/// future walk short-circuiting on the stage before it could repair the status.
///
/// Found twice, independently: by a review reasoning about what can disappear
/// from the index, and by the randomised harness, which reached it from seven
/// of eight disjoint seed ranges.
#[test]
fn a_documents_stages_go_when_the_document_does() {
    let db = fixture_empty();
    let doc = db
        .insert_document(&"c".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.record_stage(&doc, "chunk", "done").unwrap();
    db.record_stage(&doc, "embed", "done").unwrap();
    assert_eq!(
        db.stage_status(&doc, "chunk").unwrap().as_deref(),
        Some("done")
    );

    db.delete_document(&doc).unwrap();

    assert_eq!(
        db.stage_status(&doc, "chunk").unwrap(),
        None,
        "a checkpoint outlived its document, and content addressing means that \
         hash comes back"
    );
    assert_eq!(db.stage_status(&doc, "embed").unwrap(), None);

    // …and the hash coming back finds nothing waiting for it.
    let again = db
        .insert_document(&"c".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    assert_eq!(db.stage_status(&again, "chunk").unwrap(), None);
}

/// A stage cannot be recorded for content no `document` row names.
///
/// The other side of the cascade, and the constraint the embedding stages
/// inherit: every stage this design has is written after the document exists,
/// and a checkpoint for a document that was never inserted is a checkpoint for
/// nothing. A future stage that wants to check-point earlier inserts the row
/// first rather than removing the foreign key.
#[test]
fn a_stage_for_a_document_that_does_not_exist_is_refused() {
    let db = fixture_empty();
    let err = db
        .record_stage(&"d".repeat(64), "chunk", "done")
        .expect_err("a checkpoint for nothing must not be storable");
    assert!(
        err.to_string().contains("FOREIGN KEY constraint failed"),
        "expected a foreign key violation, got: {err}"
    );
}
