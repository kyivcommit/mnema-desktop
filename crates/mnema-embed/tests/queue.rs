//! The embedding pass, against a database and a rude little HTTP server.
//!
//! Every test here goes through `mnema_embed::run` and asks the database what
//! is in it afterwards. Nothing asserts on an intermediate value the pass
//! computed, because the class of defect this cycle is built against is a
//! chunk that reports as handled and is not in the index.

use mnema_embed::{EmbedProgress, Error};
use mnema_mock_provider::Reply;

mod fixture;

/// The queue is the chunks with no vector, and nothing else — no list is kept
/// anywhere, so a chunk that already has one is simply not among the answers.
#[test]
fn the_queue_is_the_chunks_with_no_vector() {
    let db = fixture::db_with_chunks(5);
    let space = fixture::active_space_1024(&db);
    db.upsert_vector(
        space,
        fixture::chunk_ids(&db)[0],
        &fixture::unit_vector_1024(),
    )
    .expect("one already embedded");

    let mock = fixture::mock(vec![fixture::reply_with(4)]);
    let out = mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {}).expect("run");

    assert_eq!(out.embedded, 4, "the already-embedded chunk was done again");
    assert_eq!(out.failed, 0);
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 5);
}

/// `upsert_vector` takes a `space_id` and forbids nothing — deliberately, since
/// building a new space beside the old one is the sanctioned migration — so a
/// writer that puts new chunks past the active space splits the archive in two
/// and search reads one half. Nothing in the index can see that happen. This
/// test is the guard, and there will never be another.
#[test]
fn the_pass_writes_only_into_the_active_space() {
    let db = fixture::db_with_chunks(3);
    let idle = fixture::space_1024_not_active(&db);
    let active = fixture::active_space_1024(&db);
    assert_ne!(idle, active, "the fixture built one space, not two");

    let mock = fixture::mock(vec![fixture::reply_with(3)]);
    mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {}).expect("run");

    assert_eq!(db.embedded_chunk_count(active).expect("active"), 3);
    assert_eq!(
        db.embedded_chunk_count(idle).expect("idle"),
        0,
        "the archive split"
    );
}

/// `clear_document_content` sets the document back to `pending` and its chunks
/// are about to be replaced. Embedding them spends the user's money on rows
/// that will not exist in a minute.
///
/// This is also where `document.status`'s permitted values stop being invented:
/// `'pending' | 'indexed' | 'failed' | 'skipped'`, from the schema's own CHECK,
/// and this pass is the first reader of them in the product.
#[test]
fn chunks_of_a_document_being_rebuilt_are_not_embedded() {
    let db = fixture::db_with_chunks(2);
    let space = fixture::active_space_1024(&db);
    fixture::set_status(&db, &fixture::first_document(&db), "pending");

    let mock = fixture::mock(vec![fixture::reply_with(2)]);
    let out = mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {}).expect("run");

    assert_eq!(out.embedded, 0);
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 0);
    // The assertion that carries it. Zero embedded is also what a pass that
    // asked the provider and threw the answer away would report, and that pass
    // has already been paid for.
    assert!(
        mock.request_if_any().is_none(),
        "the provider was asked about a document that is being rebuilt"
    );
}

/// Cancellation stops the pass; it does not undo it. The second assertion is
/// the one that matters: the job's own report and the contents of the database
/// are one number, not two.
#[test]
fn cancelling_keeps_what_was_already_written() {
    let db = fixture::db_with_chunks(10);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock((0..5).map(|_| fixture::reply_with(2)).collect());

    // A `Cell`, not a `mut` local: `cancel` reads what `on_progress` writes, so
    // the two closures are alive at once and one of them would have to hold a
    // mutable borrow of it. This is the shape any caller that stops on progress
    // has to take, and the shell's own job loop is one.
    let seen = std::cell::Cell::new(0u64);
    let out = mnema_embed::run(
        &db,
        mock.base(),
        "k",
        2,
        &|| seen.get() >= 4,
        &mut |p: EmbedProgress| seen.set(p.done),
    )
    .expect("run");

    assert!(
        out.embedded >= 4 && out.embedded < 10,
        "embedded {} of 10",
        out.embedded
    );
    assert_eq!(
        db.embedded_chunk_count(space).expect("count"),
        out.embedded as i64
    );
}

/// The silent-disappearance case. A chunk the provider refuses leaves the queue
/// and from that moment vector search never returns it — while keyword search
/// still does, and the document still shows it. `failed` is what keeps a number
/// on it; without one, the chunk lives forever inside `M − N`, which reads as
/// "not got to it yet" in the one state where nobody ever will.
#[test]
fn a_refused_chunk_is_counted_as_failed_not_merely_missing() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![
        fixture::reply_with(1),
        Reply::status(400, r#"{"error":{"message":"input is too long"}}"#),
        fixture::reply_with(1),
    ]);

    let out = mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {}).expect("run");

    assert_eq!(out.embedded, 2);
    assert_eq!(
        out.failed, 1,
        "the refused chunk vanished instead of being reported"
    );
    assert_eq!(fixture::failed_rows(&db), 1);
    assert_eq!(
        db.failed_chunk_count(space).expect("failed count"),
        1,
        "the number the settings screen reads disagrees with the row that was written"
    );
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 2);
}

/// A second run does not retry it, and does not pay for it either. The
/// consequence of the rule, asserted rather than left in a comment: the mock
/// has one reply and there is nothing left in the queue to spend it on, so a
/// pass that queued the refused chunk again would get the `599` sentinel and
/// fail here.
#[test]
fn a_refused_chunk_is_not_offered_again_while_its_text_is_unchanged() {
    let db = fixture::db_with_chunks(2);
    let space = fixture::active_space_1024(&db);
    // The first chunk embeds, which is what makes the second one's refusal
    // evidence about its text rather than about the provider — see
    // `the_first_chunk_of_a_run_is_not_condemned_on_no_evidence`.
    let mock = fixture::mock(vec![
        fixture::reply_with(1),
        Reply::status(400, r#"{"error":{"message":"nope"}}"#),
    ]);
    let first = mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {}).expect("run");
    assert_eq!((first.embedded, first.failed), (1, 1));

    let second = mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {}).expect("run");

    assert_eq!(second.embedded, 0);
    assert_eq!(
        second.failed, 0,
        "the second run counted a refusal it never made"
    );
    assert_eq!(
        db.failed_chunk_count(space).expect("failed count"),
        1,
        "one chunk was given up on, and it is still one"
    );
}

/// The other half of the same rule, and the half that stops it from being a
/// trap: the refusal was about text that no longer exists, so an edited chunk
/// is back in the queue and the failed number falls to zero on its own.
#[test]
fn an_edited_chunk_leaves_the_failed_number_and_is_tried_again() {
    let db = fixture::db_with_chunks(2);
    let space = fixture::active_space_1024(&db);
    // Chunk 0 embeds so that chunk 1's refusal is corroborated; chunk 1 is the
    // one this test then edits.
    let mock = fixture::mock(vec![
        fixture::reply_with(1),
        Reply::status(400, r#"{"error":{"message":"nope"}}"#),
        fixture::reply_with(1),
    ]);
    mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {}).expect("first run");
    assert_eq!(db.failed_chunk_count(space).expect("failed count"), 1);

    fixture::rewrite_chunk_text(&db, fixture::chunk_ids(&db)[1], "цілком інший текст");

    // The count first, and while the row is still there. `failed_chunk_count`
    // is a predicate over rows, not a count of them: the pair below says the
    // number fell to zero *without* anything being deleted, which is the only
    // way to tell it from a count that happens to agree.
    assert_eq!(
        db.failed_chunk_count(space).expect("failed count"),
        0,
        "the screen reports a failure about text that no longer exists"
    );
    assert_eq!(
        fixture::failed_rows(&db),
        1,
        "the row went instead of ceasing to apply"
    );

    let out =
        mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {}).expect("second run");

    assert_eq!(out.embedded, 1, "an edited chunk was left out of the queue");
    assert_eq!(db.failed_chunk_count(space).expect("failed count"), 0);
    assert_eq!(
        fixture::failed_rows(&db),
        0,
        "embedding the chunk left the row that said it had been given up on"
    );
}

/// vec0 accepts a vector of the wrong width without complaint at some call
/// shapes, and the damage shows up only as a confident wrong answer at query
/// time. The refusal has to happen before the write, not as a repair
/// afterwards — which is what the second assertion is for: without it this
/// passes just as well when the vector was stored and the error raised after.
#[test]
fn a_vector_of_the_wrong_width_is_refused_before_it_is_stored() {
    let db = fixture::db_with_chunks(1);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![fixture::reply_of_width(1, 512)]);

    let out = mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {});

    assert!(
        matches!(
            out,
            Err(Error::WidthMismatch {
                expected: 1024,
                got: 512
            })
        ),
        "expected a width refusal, got {out:?}"
    );
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 0);
}

/// "Before the write" is a claim about the batch, not about each vector, and a
/// batch of one cannot tell the two apart. Here the first vector is the right
/// width and the second is not: a pass that checks each one just before storing
/// it refuses correctly *and* leaves the first chunk holding a vector in a space
/// its own answer has just been declared incompatible with.
#[test]
fn a_batch_carrying_one_bad_width_stores_none_of_it() {
    let db = fixture::db_with_chunks(2);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![fixture::reply_of_mixed_widths(&[1024, 512])]);

    let out = mnema_embed::run(&db, mock.base(), "k", 2, &|| false, &mut |_| {});

    assert!(
        matches!(out, Err(Error::WidthMismatch { .. })),
        "got {out:?}"
    );
    assert_eq!(
        db.embedded_chunk_count(space).expect("count"),
        0,
        "the good vector of a bad batch was stored anyway"
    );
}

/// A vector the index will not rank is a verdict on one text, not on the run.
///
/// This is the second way the pass could have stalled for ever on one chunk,
/// and the quieter one: the request succeeded, the count and the width and the
/// stated positions were all right, and one row is simply a vector vec0 cannot
/// divide by. Nothing in `mnema-provider` looks at the numbers, so it arrives
/// at `upsert_vector` and comes back as an *index* error — which, propagated,
/// stops the run at the same chunk on every restart and records nothing anybody
/// can read.
#[test]
fn a_vector_the_index_will_not_rank_fails_its_chunk_and_not_the_run() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let ids = fixture::chunk_ids(&db);
    let mock = fixture::mock(vec![fixture::reply_with_a_degenerate_row(3, 1)]);

    let out = mnema_embed::run(&db, mock.base(), "k", 3, &|| false, &mut |_| {}).expect("run");

    assert_eq!((out.embedded, out.failed), (2, 1));
    assert_eq!(db.failed_chunk_count(space).expect("failed"), 1);
    assert!(fixture::has_vector(&db, space, ids[0]));
    assert!(
        !fixture::has_vector(&db, space, ids[1]),
        "a vector the index refuses was stored anyway"
    );
    assert!(
        fixture::has_vector(&db, space, ids[2]),
        "the run stopped at the unusable vector instead of recording it"
    );
}

/// The narrow half of the same rule, and the one that keeps it from becoming a
/// swallow-everything. An index that will not accept a write is a fact about
/// the machine, not about any chunk's text — recorded as `failed` it would take
/// a batch of perfectly good chunks out of vector search permanently, for a
/// reason that may be gone a second later.
///
/// The database is hand-edited into a state where the one call that fails is
/// the `INSERT` into `vec0` — see `active_space_that_lies_about_its_width`,
/// which explains why the two obvious ways of breaking a write both make this
/// test pass without asking anything.
#[test]
fn an_index_error_that_is_not_about_the_vector_still_stops_the_run() {
    let db = fixture::db_with_chunks(2);
    let space = fixture::active_space_that_lies_about_its_width(&db);
    let mock = fixture::mock(vec![fixture::reply_with(2)]);

    let out = mnema_embed::run(&db, mock.base(), "k", 2, &|| false, &mut |_| {});

    assert!(matches!(out, Err(Error::Index(_))), "got {out:?}");
    assert_eq!(
        fixture::failed_rows(&db),
        0,
        "an index that would not take the write was recorded as chunks that cannot be embedded"
    );
    assert_eq!(db.failed_chunk_count(space).expect("failed"), 0);
}

/// The provider's answer is bound to the texts by position all the way down,
/// and the last hop is this crate's own: vector *i* belongs to the chunk whose
/// text was *i*-th in the request. Nothing above notices that swapped —
/// `mnema-provider` has already placed the rows correctly by then, the widths
/// all match, the count is right, and the index stores whatever it is handed.
/// The result would be an archive that answers every question with a citation
/// pointing at its neighbour.
#[test]
fn each_chunk_gets_the_vector_that_was_made_from_its_own_text() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![fixture::reply_with(3)]);

    mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {}).expect("run");

    // `reply_with(n)` answers position *i* with the unit vector along axis *i*,
    // and the queue hands chunks over in id order, so a correct pass leaves
    // chunk *i* holding axis *i* and any permutation shows up here.
    for (position, chunk) in fixture::chunk_ids(&db).into_iter().enumerate() {
        let stored = fixture::stored_vector(&db, space, chunk);
        let hot = stored.iter().position(|f| *f == 1.0);
        assert_eq!(
            hot,
            Some(position),
            "chunk at position {position} is holding another chunk's vector"
        );
    }
}

/// The text on the wire is `chunk.text` — the original, which the schema
/// documents as "the original, for display". The `prepare_for_search` copy is
/// the lexical branch's, and a vector is searched against what a person reads
/// in the citation, so the provider has to see that.
///
/// The negative half is what makes this a test rather than a coincidence: the
/// fixture's text is chosen so that the prepared form differs from it, and the
/// prepared form is asserted absent.
#[test]
fn the_provider_is_sent_the_original_text_and_not_the_prepared_copy() {
    let db = fixture::db_with_display_text(fixture::TEXT_WHOSE_PREPARED_COPY_DIFFERS);
    fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![fixture::reply_with(1)]);

    mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {}).expect("run");

    let sent = mock.request();
    let original = fixture::TEXT_WHOSE_PREPARED_COPY_DIFFERS;
    let prepared = fixture::prepared_form(original);
    assert_ne!(original, prepared, "the fixture's text proves nothing");
    assert!(
        sent.contains(original),
        "the original text was not sent: {sent}"
    );
    assert!(
        !sent.contains(&prepared),
        "the lexical index's prepared copy went to the provider: {sent}"
    );
}

/// Nobody has chosen a model, so there is nowhere to put a vector. The pass
/// says which fact stopped it rather than embedding into whatever space
/// happens to exist — `active_space` has no fallback, and this is the caller
/// that would be tempted to add one.
#[test]
fn a_pass_with_no_active_space_refuses_rather_than_guessing() {
    let db = fixture::db_with_chunks(1);
    let orphan = fixture::space_1024_not_active(&db);
    let mock = fixture::mock(vec![fixture::reply_with(1)]);

    let out = mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {});

    assert!(matches!(out, Err(Error::NoActiveSpace)), "got {out:?}");
    assert_eq!(db.embedded_chunk_count(orphan).expect("count"), 0);
    assert!(
        mock.request_if_any().is_none(),
        "the provider was paid for nothing"
    );
}

/// Progress counts against the queue as it stood when the run began, so a bar
/// built from it reaches its end.
///
/// Two things at once, and the fixture is built so that neither can stand in
/// for the other. `total` is **measured once**: asked again each batch it would
/// answer a shrinking number and the bar would never move. And it is the
/// **queue**, not the archive: one of the six chunks is embedded before the run
/// starts, so a `total` taken from `chunk_count` says six where the run has
/// five to do — a bar that stops one short of the end, which is what
/// `job::progress_is_due` treats as a hang.
#[test]
fn progress_counts_against_the_queue_as_it_was_when_the_run_began() {
    let db = fixture::db_with_chunks(6);
    let space = fixture::active_space_1024(&db);
    db.upsert_vector(
        space,
        fixture::chunk_ids(&db)[0],
        &fixture::unit_vector_1024(),
    )
    .expect("one already embedded");
    let mock = fixture::mock(vec![
        fixture::reply_with(2),
        fixture::reply_with(2),
        fixture::reply_with(1),
    ]);

    let mut reports: Vec<(u64, u64)> = Vec::new();
    mnema_embed::run(
        &db,
        mock.base(),
        "k",
        2,
        &|| false,
        &mut |p: EmbedProgress| {
            reports.push((p.done, p.total));
        },
    )
    .expect("run");

    assert_eq!(
        reports,
        vec![(2, 5), (4, 5), (5, 5)],
        "progress does not describe one run of six chunks"
    );
}

/// Untouched documents are not the pass's business, but a document that failed
/// to index is not the same fact as one that is being rebuilt, and neither is
/// one the walk skipped. All three are outside `'indexed'`, and the filter has
/// to be the status rather than "not pending".
#[test]
fn only_indexed_documents_are_embedded() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    for status in ["failed", "skipped", "pending"] {
        fixture::set_status(&db, &fixture::first_document(&db), status);
        let mock = fixture::mock(vec![fixture::reply_with(3)]);
        let out = mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {})
            .unwrap_or_else(|e| panic!("run with status {status}: {e}"));
        assert_eq!(out.embedded, 0, "a {status} document was embedded");
        assert_eq!(db.embedded_chunk_count(space).expect("count"), 0);
    }
}

/// A batch refused for something that *can* be about its texts is re-sent one
/// text at a time, because `embed` is all-or-nothing over the batch it was
/// given and the answer names none of them. Four chunks, one bad: the bad one
/// is found, and the other three are embedded rather than stalled behind it.
///
/// Without this the archive can never finish. The run stops at that batch, the
/// next run recomputes the queue, meets the same batch first, and stops at the
/// same number — for ever, with nothing a person can do about it.
#[test]
fn a_batch_refused_for_its_content_is_re_sent_one_text_at_a_time() {
    let db = fixture::db_with_chunks(4);
    let space = fixture::active_space_1024(&db);
    let ids = fixture::chunk_ids(&db);
    let mock = fixture::mock(vec![
        // The batch of four, refused for its content.
        Reply::status(400, r#"{"error":{"message":"input is too long"}}"#),
        // Then one call per text, and the second of them is the bad one.
        fixture::reply_with(1),
        Reply::status(400, r#"{"error":{"message":"input is too long"}}"#),
        fixture::reply_with(1),
        fixture::reply_with(1),
    ]);

    let out = mnema_embed::run(&db, mock.base(), "k", 4, &|| false, &mut |_| {}).expect("run");

    assert_eq!((out.embedded, out.failed), (3, 1));
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 3);
    assert_eq!(db.failed_chunk_count(space).expect("failed"), 1);
    assert!(
        !fixture::has_vector(&db, space, ids[1]),
        "the split blamed the wrong chunk"
    );
}

/// And the gate in front of it. A batch refused for something that cannot be
/// about its texts is **not** split: the run stops on the first answer.
///
/// The request count is the assertion that carries this. Splitting would send
/// four more calls to learn what the first one already said — and, far worse,
/// each of those four would fail alone and be written down as a chunk that
/// cannot be embedded, when the truth was one provider having a bad minute.
/// That is the disappearance this whole rule exists to prevent, and it would
/// have been manufactured by the fix for a different one.
#[test]
fn a_batch_refused_for_something_that_is_not_about_the_texts_is_not_split() {
    let db = fixture::db_with_chunks(4);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![Reply::status(
        503,
        r#"{"error":{"message":"upstream is down"}}"#,
    )]);

    let out = mnema_embed::run(&db, mock.base(), "k", 4, &|| false, &mut |_| {});

    assert!(matches!(out, Err(Error::Provider(_))), "got {out:?}");
    assert_eq!(db.failed_chunk_count(space).expect("failed"), 0);
    assert_eq!(fixture::failed_rows(&db), 0);
    let _ = mock.request();
    assert!(
        mock.request_if_any().is_none(),
        "the batch was split over a failure that named none of its texts"
    );
}

/// The gate is asked again inside the split, and it has to be: the single calls
/// are a second place the same decision is made, in a different function, and a
/// batch that was split for a good reason can still meet a provider that falls
/// over halfway through it. The chunk in flight when that happens has done
/// nothing wrong.
#[test]
fn a_split_that_meets_an_unclassified_failure_stops_and_condemns_nothing() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![
        // The batch of three, refused for its content — so it is split.
        Reply::status(400, r#"{"error":{"message":"input is too long"}}"#),
        // The first single lands; the provider then falls over.
        fixture::reply_with(1),
        Reply::status(503, r#"{"error":{"message":"upstream is down"}}"#),
        // A fourth reply nothing should ever reach, and that is the point.
        // Without it the mock runs out here, the `599` is unattributable too,
        // and this test would report `Err` and `failed_rows == 0` even in a
        // world where 503 *was* attributed — passing for a reason that has
        // nothing to do with what it claims. With it, that world gives
        // `Ok(embedded 2, failed 1)` and both assertions below fire.
        fixture::reply_with(1),
    ]);

    let out = mnema_embed::run(&db, mock.base(), "k", 3, &|| false, &mut |_| {});

    assert!(matches!(out, Err(Error::Provider(_))), "got {out:?}");
    assert_eq!(
        db.embedded_chunk_count(space).expect("count"),
        1,
        "the single call that landed before the outage was undone"
    );
    assert_eq!(
        fixture::failed_rows(&db),
        0,
        "the chunk in flight when the provider fell over was condemned for it"
    );
}

/// The status that decides the rule is not "any 4xx". **402 Payment Required is
/// what this provider answers for an exhausted account**, and it says nothing
/// about any text. Under a 4xx-wide rule, the moment a person's credit ran out
/// mid-run every chunk in the batch in flight would be written down as
/// impossible to embed, permanently, and the third number would report a
/// failure that was never about them.
///
/// At a batch of one, so that the batch size cannot be what saves it — and
/// **after a chunk has embedded**, so that the corroboration rule cannot be
/// what saves it either. Without that first success this test would pass
/// against a build that classified 402 as attributable, because the run would
/// stop for the other reason and nothing here could tell the two apart.
#[test]
fn an_exhausted_account_stops_the_run_and_condemns_nothing() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![
        fixture::reply_with(1),
        Reply::status(402, r#"{"error":{"message":"insufficient credits"}}"#),
    ]);

    let out = mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {});

    assert!(matches!(out, Err(Error::Provider(_))), "got {out:?}");
    assert_eq!(
        db.embedded_chunk_count(space).expect("count"),
        1,
        "the corroborating success is what makes this test about 402"
    );
    assert_eq!(
        fixture::failed_rows(&db),
        0,
        "an empty account condemned a chunk"
    );
    assert_eq!(db.failed_chunk_count(space).expect("failed"), 0);
}

/// The third clause of the corroboration rule: nothing in this run has embedded,
/// so an attributable refusal is not evidence about the text — it is the first
/// thing we have heard from this provider today.
///
/// The catastrophe it prevents is not the one chunk in front of it. A provider
/// answering `400` to everything — a model withdrawn behind a gateway that says
/// `400` rather than `404`, a rewriting proxy, a changed body format — would
/// otherwise have the pass condemn the first chunk, then the next, then the
/// next, to the end of the archive, and finish reporting `Ok` with nine
/// thousand chunks that each need somebody to edit a file before anything will
/// look at them again.
#[test]
fn the_first_chunk_of_a_run_is_not_condemned_on_no_evidence() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![Reply::status(
        400,
        r#"{"error":{"message":"input is too long"}}"#,
    )]);

    let out = mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {});

    assert!(matches!(out, Err(Error::Provider(_))), "got {out:?}");
    assert_eq!(
        fixture::failed_rows(&db),
        0,
        "the first chunk of the archive was condemned on no evidence at all"
    );
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 0);
}

/// The same clause at the other batch size, where the corroboration is the
/// split's own successes rather than the run's history. Every one of the four
/// single calls is refused, so the split has learned nothing about any text —
/// it has learned something about the provider.
///
/// The request count is asserted too: the split must still happen (the batch
/// refusal was attributable, so re-sending is the right move), and it must
/// simply write nothing at the end of it.
#[test]
fn a_split_in_which_nothing_succeeded_condemns_nothing() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let refusal = || Reply::status(400, r#"{"error":{"message":"input is too long"}}"#);
    let mock = fixture::mock(vec![refusal(), refusal(), refusal(), refusal()]);

    let out = mnema_embed::run(&db, mock.base(), "k", 3, &|| false, &mut |_| {});

    assert!(matches!(out, Err(Error::Provider(_))), "got {out:?}");
    assert_eq!(
        fixture::failed_rows(&db),
        0,
        "a split that succeeded at nothing condemned every text in it"
    );
    assert_eq!(db.failed_chunk_count(space).expect("failed"), 0);
    for _ in 0..4 {
        let _ = mock.request();
    }
    assert!(
        mock.request_if_any().is_none(),
        "the batch was re-sent more times than it had texts"
    );
}

/// And the boundary between the two: one success in the split is enough, and
/// the rows held while it was in doubt are then written. Without this, the
/// clause above could be satisfied by a build that simply never writes a
/// `failed` row from a split at all.
#[test]
fn one_success_in_a_split_corroborates_the_refusals_beside_it() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let refusal = || Reply::status(400, r#"{"error":{"message":"input is too long"}}"#);
    let mock = fixture::mock(vec![
        refusal(),
        refusal(),
        fixture::reply_with(1),
        refusal(),
    ]);

    let out = mnema_embed::run(&db, mock.base(), "k", 3, &|| false, &mut |_| {}).expect("run");

    assert_eq!((out.embedded, out.failed), (1, 2));
    assert_eq!(db.failed_chunk_count(space).expect("failed"), 2);
}

/// The default arm, asked of a variant that is named rather than unclassified.
/// Rate limiting is the provider saying "later" in as many words; recorded as a
/// `failed` row it would mean "never", about a chunk there is nothing wrong
/// with.
///
/// The first chunk is embedded before the refusal so that the run is stopped
/// rather than never started, and what was written is asserted to stay.
#[test]
fn rate_limiting_stops_the_run_instead_of_condemning_the_chunk() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![
        fixture::reply_with(1),
        Reply::status(429, r#"{"error":{"message":"slow down"}}"#),
    ]);

    let out = mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {});

    assert!(matches!(out, Err(Error::Provider(_))), "got {out:?}");
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 1);
    assert_eq!(
        fixture::failed_rows(&db),
        0,
        "a rate limit condemned a chunk"
    );
}

/// Stop must be heard inside a split, not only between batches. A batch is
/// normally one round trip; split, it is as many as the batch is wide, and a
/// person who pressed Stop would otherwise wait out all of them and pay for
/// them.
#[test]
fn cancelling_during_a_split_stops_it_between_the_single_calls() {
    let db = fixture::db_with_chunks(4);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![
        Reply::status(400, r#"{"error":{"message":"input is too long"}}"#),
        fixture::reply_with(1),
        fixture::reply_with(1),
        fixture::reply_with(1),
        fixture::reply_with(1),
    ]);

    // Raised on the third time it is asked: once by the run before the batch,
    // then once before each single call. So exactly one single call goes out.
    let asked = std::cell::Cell::new(0u32);
    let out = mnema_embed::run(
        &db,
        mock.base(),
        "k",
        4,
        &|| {
            asked.set(asked.get() + 1);
            asked.get() >= 3
        },
        &mut |_| {},
    )
    .expect("run");

    assert_eq!(out.embedded, 1, "the split ran on past the cancellation");
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 1);
    let _ = mock.request();
    let _ = mock.request();
    assert!(
        mock.request_if_any().is_none(),
        "a third request went out after Stop"
    );
}

/// A whole batch the provider refuses is not attributed to any one of its
/// texts, because nothing says which of them caused it: `embed` is
/// all-or-nothing over the batch it was given. The run stops and says so, and —
/// the half that matters — writes no `failed` row, since a `failed` row is
/// never reconsidered until the text changes, and a network that was down for
/// a minute would otherwise take a batch of perfectly good chunks out of the
/// index permanently.
#[test]
fn a_refused_batch_stops_the_run_and_condemns_nothing() {
    let db = fixture::db_with_chunks(4);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![
        fixture::reply_with(2),
        Reply::status(503, r#"{"error":{"message":"upstream is down"}}"#),
        // Two replies nothing should reach. They are what make the assertions
        // below mean something: without them the mock runs out during the
        // split, the `599` is unattributable in its own right, and this test
        // would still see `Err` and no failed rows in a world where 503 was
        // attributed. With them, that world embeds all four and returns `Ok`.
        fixture::reply_with(1),
        fixture::reply_with(1),
    ]);

    let out = mnema_embed::run(&db, mock.base(), "k", 2, &|| false, &mut |_| {});

    assert!(matches!(out, Err(Error::Provider(_))), "got {out:?}");
    assert_eq!(
        db.embedded_chunk_count(space).expect("count"),
        2,
        "the batch that landed before the failure was rolled back"
    );
    assert_eq!(
        db.failed_chunk_count(space).expect("failed count"),
        0,
        "chunks nobody could name were condemned for a failure of the batch"
    );
}

/// And what that leaves is a run that can simply be started again: the queue is
/// recomputed, the two that landed are gone from it, and the two that never
/// went are still in it. Nothing was recorded that has to be undone first.
#[test]
fn a_run_that_stopped_continues_where_it_left_off() {
    let db = fixture::db_with_chunks(4);
    let space = fixture::active_space_1024(&db);
    let stopped = fixture::mock(vec![
        fixture::reply_with(2),
        Reply::status(503, r#"{"error":{"message":"upstream is down"}}"#),
    ]);
    mnema_embed::run(&db, stopped.base(), "k", 2, &|| false, &mut |_| {}).expect_err("stops");

    let resumed = fixture::mock(vec![fixture::reply_with(2)]);
    let out = mnema_embed::run(&db, resumed.base(), "k", 2, &|| false, &mut |_| {}).expect("run");

    assert_eq!(out.embedded, 2, "the second run redid work or skipped some");
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 4);
}

/// An empty queue is not an error and costs nothing: no request goes out, and
/// the tally is two zeroes. The screen this feeds says "9 000 of 9 000", which
/// is the ordinary state of an archive that has finished.
#[test]
fn nothing_to_do_asks_the_provider_nothing() {
    let db = fixture::db_with_chunks(0);
    fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![]);

    let out = mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {}).expect("run");

    assert_eq!((out.embedded, out.failed), (0, 0));
    assert!(mock.request_if_any().is_none());
}

/// The model is the space's own, read from `space_model` — not a parameter, so
/// there is no way for a caller to embed with one model into a space built for
/// another. `run`'s signature is the enforcement; this checks the value that
/// signature makes unavoidable actually reaches the wire.
#[test]
fn the_request_names_the_model_the_space_was_built_for() {
    let db = fixture::db_with_chunks(1);
    fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![fixture::reply_with(1)]);

    mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {}).expect("run");

    let sent = mock.request();
    assert!(
        sent.contains(fixture::ACTIVE_MODEL),
        "the request does not name {}: {sent}",
        fixture::ACTIVE_MODEL
    );
}

/// A guard against a caller, not against the provider: `batch` reaches this
/// crate from a settings value, and zero would ask for a batch of nothing
/// forever. It is a refusal rather than a silent floor of one, because a zero
/// arriving here means whoever computed it is wrong and a floor would hide it.
#[test]
fn a_batch_size_of_zero_is_refused_rather_than_looped_on() {
    let db = fixture::db_with_chunks(1);
    fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![fixture::reply_with(1)]);

    let out = mnema_embed::run(&db, mock.base(), "k", 0, &|| false, &mut |_| {});

    assert!(matches!(out, Err(Error::EmptyBatch)), "got {out:?}");
    assert!(mock.request_if_any().is_none());
}

/// Cancellation asked before the first batch stops the run without paying for
/// anything. `cancel` is checked between batches, and "between" has to include
/// before the first one — a person who presses Start and Stop in the same
/// second has cancelled a job, not started one.
#[test]
fn cancelling_before_the_first_batch_asks_the_provider_nothing() {
    let db = fixture::db_with_chunks(4);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![fixture::reply_with(4)]);

    let out = mnema_embed::run(&db, mock.base(), "k", 4, &|| true, &mut |_| {}).expect("run");

    assert_eq!((out.embedded, out.failed), (0, 0));
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 0);
    assert!(
        mock.request_if_any().is_none(),
        "a cancelled run paid the provider"
    );
}

/// `MockServer` hands reply *n* to request *n*, so a pass that sent one request
/// per chunk where it should have sent one per batch runs out of replies and
/// meets the `599` sentinel. This asserts the batching directly instead:
/// twelve chunks at five to a batch is three requests, not twelve.
#[test]
fn chunks_go_out_in_batches_of_the_size_asked_for() {
    let db = fixture::db_with_chunks(12);
    fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![
        fixture::reply_with(5),
        fixture::reply_with(5),
        fixture::reply_with(2),
    ]);

    let out = mnema_embed::run(&db, mock.base(), "k", 5, &|| false, &mut |_| {}).expect("run");

    assert_eq!(out.embedded, 12);
    let sizes: Vec<usize> = (0..3).map(|_| fixture::texts_in(&mock.request())).collect();
    assert_eq!(
        sizes,
        vec![5, 5, 2],
        "the batches were not the size asked for"
    );
    assert!(mock.request_if_any().is_none(), "a fourth request went out");
}

/// The tally and the database agree about failures the same way
/// `cancelling_keeps_what_was_already_written` makes them agree about
/// successes — asserted over a run that has some of each, since a run with only
/// one kind cannot tell a counter that counts the wrong thing from one that
/// counts nothing.
#[test]
fn the_tally_and_the_index_agree_after_a_run_with_both_kinds() {
    let db = fixture::db_with_chunks(4);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![
        fixture::reply_with(1),
        Reply::status(400, r#"{"error":{"message":"too long"}}"#),
        Reply::status(400, r#"{"error":{"message":"too long"}}"#),
        fixture::reply_with(1),
    ]);

    let out = mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {}).expect("run");

    assert_eq!((out.embedded, out.failed), (2, 2));
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 2);
    assert_eq!(db.failed_chunk_count(space).expect("failed"), 2);
    // Everything is accounted for: nothing is still waiting, and nothing is in
    // both columns. `M − N` would have said two were on their way.
    assert_eq!(db.chunk_count().expect("chunks"), 4);
}

/// A refusal must not take the vectors of the batch before it with it, and must
/// not stop the pass reaching the ones after. Both directions in one run,
/// because a pass that rolled back on failure and one that stopped on failure
/// are different defects and each satisfies half of the pair.
#[test]
fn a_refusal_neither_undoes_what_came_before_nor_stops_what_comes_after() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let ids = fixture::chunk_ids(&db);
    let mock = fixture::mock(vec![
        fixture::reply_with(1),
        Reply::status(400, r#"{"error":{"message":"too long"}}"#),
        fixture::reply_with(1),
    ]);

    mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {}).expect("run");

    assert!(
        fixture::has_vector(&db, space, ids[0]),
        "the first was undone"
    );
    assert!(
        !fixture::has_vector(&db, space, ids[1]),
        "the refused one was stored"
    );
    assert!(
        fixture::has_vector(&db, space, ids[2]),
        "the pass stopped at the refusal"
    );
}

/// The bar must move inside a split. A split is up to `batch` network round
/// trips — the longest path in the whole pass — and reporting once at the end
/// of it freezes the bar exactly where the work is slowest.
///
/// The sequence is asserted rather than the count: it says the reports arrive
/// *as the work happens*, one per single call, and then once more from the
/// loop that owns the batch. A pass that reported only at the end would show a
/// single `4`.
#[test]
fn progress_moves_inside_a_split_not_only_between_batches() {
    let db = fixture::db_with_chunks(4);
    fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![
        Reply::status(400, r#"{"error":{"message":"input is too long"}}"#),
        fixture::reply_with(1),
        fixture::reply_with(1),
        fixture::reply_with(1),
        fixture::reply_with(1),
    ]);

    let mut done: Vec<u64> = Vec::new();
    mnema_embed::run(
        &db,
        mock.base(),
        "k",
        4,
        &|| false,
        &mut |p: EmbedProgress| {
            assert_eq!(
                p.total, 4,
                "a report from inside a split invented its own total"
            );
            done.push(p.done);
        },
    )
    .expect("run");

    assert_eq!(
        done,
        vec![1, 2, 3, 4, 4],
        "the bar stood still while the split made four round trips"
    );
}

/// The last number the shell sees must be true even when the run ends in an
/// error. The vectors from the batch that failed are already in the index; a
/// pass that returns `Err` without reporting leaves the shell short by up to a
/// batch of embeddings that really are there — the same class of defect as a
/// `failed` count including rows nobody can find.
///
/// The `Err` itself is unchanged and asserted: an error must not be shaped like
/// success. It simply goes back after the number is honest rather than before.
///
/// This path aborts without a split — `503` is never attributable, so the batch
/// is not re-sent — which is what keeps it about the report and not about the
/// split's own reporting.
#[test]
fn the_last_number_is_true_even_when_the_run_stops() {
    let db = fixture::db_with_chunks(4);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![
        fixture::reply_with(2),
        Reply::status(503, r#"{"error":{"message":"upstream is down"}}"#),
    ]);

    let mut reports: Vec<EmbedProgress> = Vec::new();
    let out = mnema_embed::run(
        &db,
        mock.base(),
        "k",
        2,
        &|| false,
        &mut |p: EmbedProgress| {
            reports.push(p);
        },
    );

    assert!(matches!(out, Err(Error::Provider(_))), "got {out:?}");
    assert_eq!(
        reports.len(),
        2,
        "the batch that ended the run reported nothing at all"
    );
    let last = reports.last().expect("a report");
    assert_eq!(
        (last.done, last.failed),
        (2, 0),
        "the last number the shell saw is not what the run had done"
    );
    assert_eq!(
        last.done as i64,
        db.embedded_chunk_count(space).expect("count"),
        "the last number on screen disagrees with the index"
    );
}

/// **The pass must use the protected write, not merely have one available.**
///
/// `mnema-index` proves `upsert_vector_for_text` behaves correctly. Nothing in
/// this crate proved that `store` *calls* it: reverting that one line to
/// `upsert_vector` left all 36 tests green and the harness reported STILL
/// GREEN. The guard against a vector landing on a rebuilt chunk was itself
/// standing on nothing — the branch's oldest pattern, arriving one layer above
/// the defence we had just built.
///
/// The seam is already in the signature. `run` calls `on_progress` from its own
/// thread, and inside a split it calls it *between* the single requests — after
/// the queue was read, before the rest of that batch is written. Rebuilding a
/// document there is exactly the race: the chunks still to be written were read
/// with one text, and by the time their vectors arrive they hold another.
///
/// The rebuild goes through `clear_document_content`, so the interaction is the
/// real one rather than a staged version of it: the clear takes that document's
/// vectors (D88), puts it back to `pending`, and frees chunk ids that the new
/// chunks are handed straight back — which is what makes the stale write land
/// on a *live* row instead of failing on a missing one.
///
/// The axes carry the verdict. The in-flight singles answer along 6 and 7,
/// while the second round — over chunks read fresh after the rebuild — answers
/// along 0 and 1. A rebuilt chunk holding axis 6 or 7 holds a vector made from
/// text its file no longer contains, which is the whole thing being prevented.
#[test]
fn a_vector_is_not_written_onto_a_chunk_rebuilt_while_the_request_was_in_flight() {
    let db = fixture::db_with_chunks_in_two_documents(1, 2);
    let space = fixture::active_space_1024(&db);
    let rebuilt = fixture::document_ids(&db)[1].clone();

    let mock = fixture::mock(vec![
        // Refused for its content, so the batch is split — which is what puts
        // an `on_progress` between the queue read and the writes after it.
        Reply::status(400, r#"{"error":{"message":"input is too long"}}"#),
        // The untouched document. Its success is also what corroborates the
        // split, so the run does not stop for an unrelated reason.
        fixture::reply_of_axis(5),
        // The two whose chunks are rebuilt while these are in flight.
        fixture::reply_of_axis(6),
        fixture::reply_of_axis(7),
        // The second round, over the rebuilt chunks read fresh.
        fixture::reply_with(2),
    ]);

    let rebuilt_once = std::cell::Cell::new(false);
    mnema_embed::run(&db, mock.base(), "k", 3, &|| false, &mut |_| {
        if !rebuilt_once.replace(true) {
            fixture::rebuild_document(&db, &rebuilt, &["новий текст 1", "новий текст 2"]);
        }
    })
    .expect("run");

    assert!(
        rebuilt_once.get(),
        "the rebuild never happened, so nothing was tested"
    );
    for chunk in fixture::chunk_ids_of(&db, &rebuilt) {
        let stored = fixture::stored_vector(&db, space, chunk);
        let hot = stored.iter().position(|f| *f == 1.0);
        assert!(
            matches!(hot, Some(0) | Some(1)),
            "chunk {chunk} holds a vector made from text the file no longer contains \
             (hot axis {hot:?}; 6 and 7 are the answers that were in flight during the rebuild)"
        );
    }
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 3);
}

// ── Task 7: a space stops being forever `building` (D95b) ─────────────────

/// The whole point of D95b: a space that has actually finished must say so.
/// Before this, `state` was written once, at creation, and never again — a
/// fully embedded archive read exactly like one that had not started.
#[test]
fn a_space_becomes_ready_when_the_queue_empties() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    assert_eq!(fixture::space_state(&db, space), "building");

    let mock = fixture::provider_returning_unit_vectors();
    mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {}).expect("run");

    assert_eq!(fixture::space_state(&db, space), "ready");
}

/// A space with a chunk nobody could embed is not ready, and saying it is
/// would make the state mean "the pass finished" rather than "the space is
/// complete" — two different claims, and only one of them is useful.
#[test]
fn a_space_with_failures_does_not_become_ready() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::provider_refusing_the_second_text();

    let out = mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {}).expect("run");

    assert_eq!(out.failed, 1, "the middle chunk was meant to fail");
    assert_eq!(fixture::space_state(&db, space), "building");
}

/// The other direction, and the one a one-way state would get wrong: `ready`
/// is a claim that every chunk in the space has a vector, and a chunk that
/// arrives afterwards — a second document, an archive grown since the space
/// last finished — makes that claim false again immediately, not once some
/// later run gets back around to it.
///
/// The mid-run read is what proves the retraction actually happens rather
/// than the state merely landing back on `ready` for reasons that have
/// nothing to do with it: `on_progress` fires once, after the one batch that
/// embeds the two new chunks, and by then `run` has already written
/// `building` on entry (the queue was not empty) but has not yet written
/// `ready` back (that happens on the loop iteration after this one, once the
/// queue reads empty).
#[test]
fn a_ready_space_goes_back_to_building_when_new_chunks_arrive() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let first_pass = fixture::mock(vec![fixture::reply_with(3)]);
    mnema_embed::run(&db, first_pass.base(), "k", 10, &|| false, &mut |_| {}).expect("run");
    assert_eq!(fixture::space_state(&db, space), "ready");

    fixture::add_document_with_chunks(&db, 2);

    let seen_mid_run = std::cell::RefCell::new(None);
    let second_pass = fixture::mock(vec![fixture::reply_with(2)]);
    mnema_embed::run(&db, second_pass.base(), "k", 10, &|| false, &mut |_| {
        seen_mid_run
            .borrow_mut()
            .get_or_insert_with(|| fixture::space_state(&db, space));
    })
    .expect("run");

    assert_eq!(
        seen_mid_run.into_inner(),
        Some("building".to_string()),
        "the ready claim must be retracted before the new chunks are embedded, \
         not only after"
    );
    assert_eq!(fixture::space_state(&db, space), "ready");
}

// ── Task 7 fix round 1: `ready` was two predicates over two different sets
// of chunks — the queue is `d.status = 'indexed'` only, `failed_chunk_count`
// is every chunk that exists. Both rows below were probed by review, not
// reasoned, and both are regression tests now rather than a probe script
// that ran once and was deleted.

/// I1 — the dangerous direction. A document whose chunks are written but not
/// yet `indexed` is invisible to the queue (`space.rs:47`) and has no
/// failures (`failed_chunk_count` finds nothing to have given up on), so the
/// old two-predicate condition read both halves as satisfied and wrote
/// `ready` over a space holding zero vectors for three chunks that exist.
#[test]
fn a_space_with_chunks_behind_an_unindexed_document_does_not_become_ready() {
    let db = fixture::db_with_unindexed_chunks(3);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![]);

    mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {}).expect("run");

    assert!(
        mock.request_if_any().is_none(),
        "the provider was asked about a document that is not indexed"
    );
    assert_eq!(fixture::space_state(&db, space), "building");
}

/// I2 — the mirror, and the direction fix round 1 keeps rather than closes.
/// A chunk this space gave up on, whose document has since left `indexed`
/// (the same transition a rebuild starts with), is invisible to the queue
/// exactly like I1's chunk — but unlike I1's, it already carries a verdict,
/// and the space genuinely is not complete. Staying `building` here is not a
/// bug the state should hide: `failed_chunk_count` is what tells a person
/// why, once something reads it.
#[test]
fn a_space_with_a_failed_chunk_behind_a_document_that_left_indexed_stays_building() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::provider_refusing_the_second_text();
    mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {}).expect("run");
    assert_eq!(fixture::space_state(&db, space), "building");

    fixture::set_status(&db, &fixture::first_document(&db), "pending");

    let empty = fixture::mock(vec![]);
    mnema_embed::run(&db, empty.base(), "k", 10, &|| false, &mut |_| {}).expect("run");

    assert!(
        empty.request_if_any().is_none(),
        "the provider was asked about a document that is not indexed"
    );
    assert_eq!(fixture::space_state(&db, space), "building");
}

/// Fix round 2: the write moved to `space_is_complete`'s wide scope, but the
/// retraction on entry stayed on `total > 0` — `queued_chunk_count`, the
/// queue's own `d.status = 'indexed'`-scoped question. A space already
/// `ready`, then a second document's chunks arrive behind a status that is
/// not yet `indexed` (I1's window, reached the ordinary way an archive
/// grows): the queue never sees them, so `total` stays `0` and nothing asks
/// for a retraction — and the queue being empty on the very next check means
/// `space_is_complete` is never asked either. The space stays `ready` over
/// chunks with no vector, and no later run can ever clear it, because no
/// later run's queue will see them either.
#[test]
fn a_ready_space_is_retracted_by_chunks_behind_an_unindexed_document_too() {
    let db = fixture::db_with_chunks(3);
    let space = fixture::active_space_1024(&db);
    let first_pass = fixture::mock(vec![fixture::reply_with(3)]);
    mnema_embed::run(&db, first_pass.base(), "k", 10, &|| false, &mut |_| {}).expect("run");
    assert_eq!(fixture::space_state(&db, space), "ready");

    fixture::add_unindexed_document_with_chunks(&db, 2);

    let mock = fixture::mock(vec![]);
    mnema_embed::run(&db, mock.base(), "k", 10, &|| false, &mut |_| {}).expect("run");

    assert!(
        mock.request_if_any().is_none(),
        "the provider was asked about a document that is not indexed"
    );
    assert_eq!(fixture::space_state(&db, space), "building");
}
