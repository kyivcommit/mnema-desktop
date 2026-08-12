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
    let db = fixture::db_with_chunks(1);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![Reply::status(400, r#"{"error":{"message":"nope"}}"#)]);
    let first = mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {}).expect("run");
    assert_eq!(first.failed, 1);

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
    let db = fixture::db_with_chunks(1);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![
        Reply::status(400, r#"{"error":{"message":"nope"}}"#),
        fixture::reply_with(1),
    ]);
    mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {}).expect("first run");
    assert_eq!(db.failed_chunk_count(space).expect("failed count"), 1);

    fixture::rewrite_chunk_text(&db, fixture::chunk_ids(&db)[0], "цілком інший текст");

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
/// At a batch of one, so that the batch size cannot be what saves it.
#[test]
fn an_exhausted_account_stops_the_run_and_condemns_nothing() {
    let db = fixture::db_with_chunks(2);
    let space = fixture::active_space_1024(&db);
    let mock = fixture::mock(vec![Reply::status(
        402,
        r#"{"error":{"message":"insufficient credits"}}"#,
    )]);

    let out = mnema_embed::run(&db, mock.base(), "k", 1, &|| false, &mut |_| {});

    assert!(matches!(out, Err(Error::Provider(_))), "got {out:?}");
    assert_eq!(
        fixture::failed_rows(&db),
        0,
        "an empty account condemned a chunk"
    );
    assert_eq!(db.failed_chunk_count(space).expect("failed"), 0);
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
