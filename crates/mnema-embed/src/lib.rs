//! The pass that finds what has no vector and fills it.
//!
//! The only crate that knows about both sides: `mnema-provider` stays
//! database-free and `mnema-index` stays provider-free, and they meet here and
//! nowhere else.
//!
//! **There is no queue.** Nothing is written down anywhere saying which chunks
//! are waiting — the question is asked of the database again on every batch,
//! and the answer is whatever has no row in the active space's vector table.
//! That is what makes a document cleared for a rebuild rejoin the work by the
//! same route a brand new one arrives by, with nothing having to be told; and
//! it is what makes a run that stopped — a network that went, a person who
//! pressed Stop — resumable by simply starting it again, with nothing to
//! recover. [`mnema_index::Db::chunks_needing_embedding`] is the question.
//!
//! **§2.6 (f): the pass writes into the active space and has no way to write
//! anywhere else.** [`run`] takes no `space_id`; it reads
//! [`mnema_index::Db::active_space`] and that is the only space name it ever
//! holds. `insert_vector` and `upsert_vector` both take one and forbid nothing
//! — deliberately, because building a new space beside the old one is the
//! sanctioned migration — so nothing inside the index can catch a writer that
//! puts new chunks where the index is not pointing. That is the invariant this
//! crate exists to establish, and `tests/queue.rs`'s
//! `the_pass_writes_only_into_the_active_space` is the whole of its
//! enforcement.

use mnema_index::{Db, PendingChunk, VectorRole};

/// What a run reports while it is going.
///
/// **Not `Progress`.** The name is taken twice over — `job::Progress` in the
/// shell, and `mnema_pool::Outcome` beside it for the other pair — and the
/// translation from this type into `job::Progress` is a real step that happens
/// in the shell. Two different types with one name would make that step
/// invisible at exactly the place it is easiest to forget.
///
/// `done + failed <= total`, and the inequality is not slack: `total` is the
/// queue measured once, when the run began, so that a bar built from it moves.
/// A run that ends with `done + failed < total` was cancelled or stopped, and a
/// run with `failed > 0` never reaches `done == total` at all — which whatever
/// renders this has to survive, since `job::progress_is_due` sends an unthrottled
/// report on `done == total` and will not get one here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbedProgress {
    pub done: u64,
    pub total: u64,
    /// The third number, and the reason the design is allowed to be as
    /// unforgiving as it is. `total - done` on its own reads as "not got to
    /// them yet", and for these chunks nobody ever will.
    pub failed: u64,
}

/// What a run reports when it ends. **Not `Outcome`**, for the reason
/// [`EmbedProgress`] is not `Progress`.
///
/// No `total`: whoever reads this has the database and can ask it anything,
/// and a total copied into the answer is a number that was true a moment ago.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbedTally {
    pub embedded: u64,
    pub failed: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("index: {0}")]
    Index(#[from] mnema_index::Error),
    #[error("provider: {0}")]
    Provider(#[from] mnema_provider::Error),
    /// Nobody has chosen an embedding model, so there is nowhere to put a
    /// vector.
    ///
    /// A refusal and not a guess. [`mnema_index::Db::active_space`]'s own doc
    /// comment forbids itself a fallback — "if the key is absent, use the only
    /// space there is" would break a guard two functions away — and this is the
    /// caller that would otherwise be tempted to add one here instead.
    #[error("no embedding model has been chosen, so there is no space to embed into")]
    NoActiveSpace,
    /// The vector the provider returned is not the width this space's table was
    /// created with.
    ///
    /// Checked here, before the write, and **not** left to `check_rankable`:
    /// that function asks whether a vector can be ranked at all — finite
    /// components, a norm that does not underflow — which is a different
    /// question from whether it belongs in *this* space, and it answers yes to
    /// a perfectly good 512-wide vector on its way into a 1024-wide table.
    /// vec0 accepts the mismatch at some call shapes without complaint, and the
    /// damage then shows up only as a confident wrong answer at query time.
    ///
    /// Both sides are `i64` because one of them is: the space's width comes
    /// from `embedding_space.dim`, and comparing it against a `usize` length
    /// means a conversion that can fail, on a path where the failure would have
    /// to be reported as something it is not.
    #[error("this space stores {expected}-wide vectors and the provider returned a {got}-wide one")]
    WidthMismatch { expected: i64, got: i64 },
    /// A batch of nothing. It would ask the database for zero chunks forever.
    ///
    /// A refusal rather than a silent floor of one: `batch` reaches this crate
    /// from a setting somebody computed, and a zero arriving here means that
    /// computation is wrong. A floor would embed the archive one chunk at a
    /// time and never say why it was slow.
    #[error("a batch size of zero would ask for nothing, forever")]
    EmptyBatch,
    /// The provider returned a different number of vectors than there were
    /// texts.
    ///
    /// [`mnema_provider::embed`] refuses this first, as `CountMismatch`, so
    /// nothing can currently reach this variant — and it is here anyway,
    /// because it is what keeps the loop below provably finite. The loop makes
    /// progress by every chunk it takes leaving the queue, and a short answer
    /// silently zipped against the batch would leave some of them in it with
    /// nothing recorded either way, against a paid provider, for as long as the
    /// application is open.
    #[error("asked the provider for {asked} vectors and got {got}")]
    ShortAnswer { asked: usize, got: usize },
}

/// Embeds everything the active space still owes a vector for, in batches of
/// `batch`, until the queue is empty or `cancel` says to stop.
///
/// **No `space_id` parameter, and that is the point** — see the module comment.
/// The model and the width come from the space too
/// ([`mnema_index::Db::space_model`]), so there is no way for a caller to embed
/// with one model into a space built for another either.
///
/// The text sent is `chunk.text`: the original, which the schema documents as
/// "the original, for display", and **not** the
/// [`mnema_index::prepare_for_search`] copy. The prepared text exists for the
/// lexical branch; a vector is searched against what a person will read in the
/// citation, so that is what the provider has to see.
///
/// **What stops the run, and what does not.**
///
/// - `cancel` between batches, including before the first. Everything already
///   written stays: nothing is rolled back, because there is nothing to roll
///   back to — the next run recomputes the queue and simply finds less in it.
/// - A provider failure is first asked whether it can be about the texts at all
///   — `speaks_only_about_these_texts`, whose `_` arm stops the run. Everything
///   that is not on its list ends the run and writes nothing, whatever the
///   batch size.
/// - A failure that *can* be about the texts, over a batch of more than one, is
///   re-sent one text at a time. `embed` is all-or-nothing over the batch it was
///   given, so the answer names no particular text and the only way to find out
///   which one it was is to ask about each of them. This costs nothing when
///   nothing is wrong, and one extra round of calls per bad chunk when
///   something is.
/// - A failure that can be about the texts, over a batch of exactly one, is
///   about that text — **provided something earlier in this run embedded**.
///   It becomes a `failed` row and the run carries on. What separates "this
///   text can never be embedded" from "try again in an hour" needs a
///   measurement of what a provider actually answers to an over-long chunk, and
///   there is none, so this crate does not pretend to make that distinction:
///   whoever eventually retries failures gets to.
/// - **Nothing in this run has embedded yet: an attributable refusal stops the
///   run rather than blaming a chunk.** One rule, three clauses, one reason —
///   a refusal is evidence about a text only if we have seen the same provider,
///   in the same run, answer some other text correctly. On the first chunk of an
///   archive there is no such evidence, and the first chunk is the worst thing
///   to blame without it.
///
/// ⚠️ **Known limit, decided rather than missed.** At `batch == 1` the
/// corroboration is any earlier success in the run, so a provider that breaks
/// *midway* is corroborated by what it did before it broke, and chunks from
/// that point on are condemned. It is accepted: the production batch size is
/// above one, where the split's own successes are the evidence and the case is
/// covered properly, and closing it at a batch of one needs a notion of
/// recency — how recent a success has to be to still count — that nobody has
/// measured.
/// - A width that does not match the space stops the run before anything from
///   that batch is written. A vector of the *right* width that the index will
///   not rank — non-finite components, a norm that underflows — is the opposite
///   case and is treated as the opposite way round: it is a verdict on one
///   chunk's text, so it becomes a `failed` row and the run carries on. See
///   `refuses_this_chunks_vector`.
///
/// `on_progress` is called once per batch, unthrottled. The throttle belongs to
/// whoever owns the channel on the other side — `job::progress_is_due` — for the
/// reason `job::REPORT_INTERVAL`'s own doc gives; at one call per batch of tens
/// of chunks over a network round trip, there is nothing here to flood it with
/// anyway.
pub fn run(
    db: &Db,
    base: &str,
    key: &str,
    batch: usize,
    cancel: &dyn Fn() -> bool,
    on_progress: &mut dyn FnMut(EmbedProgress),
) -> Result<EmbedTally, Error> {
    if batch == 0 {
        return Err(Error::EmptyBatch);
    }
    let space = db.active_space()?.ok_or(Error::NoActiveSpace)?;
    let (model, width) = db.space_model(space)?;
    let call = Call {
        base,
        key,
        model: &model,
    };
    // Measured once, before anything is taken out of it — see `EmbedProgress`.
    let total = db.queued_chunk_count(space)? as u64;

    let mut tally = EmbedTally {
        embedded: 0,
        failed: 0,
    };
    loop {
        // Asked before the first batch and not only between them: a person who
        // presses Start and then Stop in the same second has cancelled a job,
        // and must not be charged for one.
        if cancel() {
            break;
        }
        let pending = db.chunks_needing_embedding(space, batch)?;
        if pending.is_empty() {
            break;
        }
        let texts: Vec<String> = pending.iter().map(|c| c.text.clone()).collect();
        match mnema_provider::embed(call.base, call.key, call.model, &texts) {
            Ok(vectors) => store(db, space, width, &pending, &vectors, &mut tally)?,
            Err(refusal) => {
                // Asked first, and of every batch size: a failure that cannot be
                // about these texts ends the run and condemns nobody.
                if !speaks_only_about_these_texts(&refusal) {
                    return Err(Error::Provider(refusal));
                }
                if pending.len() == 1 {
                    // One text, and an answer that can only be about it — but
                    // only once something else in this run has shown that the
                    // provider answers at all. Nothing has, on the very first
                    // chunk of an archive, and the first chunk is the worst
                    // possible thing to condemn on no evidence: a provider that
                    // is simply broken this morning would be recorded as nine
                    // thousand texts that cannot be embedded, one batch at a
                    // time, each of them needing an edit to the file before
                    // anything will look at it again.
                    if tally.embedded == 0 {
                        return Err(Error::Provider(refusal));
                    }
                    // `content_hash` is read from the chunk inside
                    // `record_embedding_failure`, so the row is about the text
                    // that was actually sent and cannot be about anything else.
                    db.record_embedding_failure(space, pending[0].id)?;
                    tally.failed += 1;
                } else {
                    one_at_a_time(db, space, &call, width, &pending, cancel, &mut tally)?;
                }
            }
        }
        on_progress(EmbedProgress {
            done: tally.embedded,
            total,
            failed: tally.failed,
        });
    }
    Ok(tally)
}

/// Where a request goes and what it asks for, carried together because the
/// three travel together and none of them is this pass's to choose: `model`
/// comes from the space, and `base`/`key` from the caller.
struct Call<'a> {
    base: &'a str,
    key: &'a str,
    model: &'a str,
}

/// Re-sends a refused batch one text at a time, to find out which of them the
/// answer was about.
///
/// Only ever reached for a refusal that already passed
/// [`speaks_only_about_these_texts`]. That gate is the whole reason this is
/// affordable *and* the reason it is safe: a batch of ten refused with
/// `Transport` is not re-sent, because ten more calls would only learn what the
/// first one said, and — the half that matters — because each of those ten
/// would then fail alone and be written down as ten chunks that cannot be
/// embedded, when the truth was one network that was down for a minute.
///
/// A chunk whose own call succeeds is stored. A chunk whose own call fails
/// with an answer that can be about its text is **written down at the end, and
/// only if some other text in this same split embedded**. Anything else ends
/// the run, mid-batch, with what landed left where it landed — there is nothing
/// to undo, because the queue is recomputed from the index and not from a list.
///
/// **The corroboration is what stops this from condemning an archive.** A
/// refusal is evidence about a text only if we have seen the same provider, in
/// the same run, answer some other text correctly. Without that, a provider
/// answering `400` to everything — a model withdrawn and a gateway saying `400`
/// instead of `404`, a rewriting proxy, a changed body format — would have this
/// split write a `failed` row for all ten of its texts, then do it again for
/// the next ten, to the end of the archive, and `run` would report `Ok` with
/// nine thousand chunks that now need somebody to edit the file before anything
/// will look at them again. Zero successes in a split is a fact about the
/// provider, not about any of the texts, so the rows are held until there is
/// one and the refusal is returned instead.
///
/// The successes counted are *stored vectors*, not merely calls that returned:
/// it is the stricter of the two readings, and this whole rule is built on
/// preferring the loud mistake to the silent one.
///
/// A cancelled split never turns into an error and never writes a held row.
/// Stopping is not evidence either way, and the chunks stay in the queue for
/// the next run exactly as if the split had not happened.
///
/// `cancel` is asked between the single calls and not only between batches. A
/// batch is normally one round trip and cancellation between them is prompt;
/// split, it is as many round trips as the batch is wide, and a person who
/// pressed Stop would otherwise wait out all of them.
fn one_at_a_time(
    db: &Db,
    space: i64,
    call: &Call<'_>,
    width: i64,
    pending: &[PendingChunk],
    cancel: &dyn Fn() -> bool,
    tally: &mut EmbedTally,
) -> Result<(), Error> {
    let embedded_before = tally.embedded;
    let mut condemned: Vec<i64> = Vec::new();
    let mut last_refusal: Option<mnema_provider::Error> = None;
    let mut cancelled = false;
    for chunk in pending {
        if cancel() {
            cancelled = true;
            break;
        }
        let texts = [chunk.text.clone()];
        match mnema_provider::embed(call.base, call.key, call.model, &texts) {
            Ok(vectors) => store(
                db,
                space,
                width,
                std::slice::from_ref(chunk),
                &vectors,
                tally,
            )?,
            Err(refusal) => {
                if !speaks_only_about_these_texts(&refusal) {
                    return Err(Error::Provider(refusal));
                }
                condemned.push(chunk.id);
                last_refusal = Some(refusal);
            }
        }
    }
    if tally.embedded == embedded_before {
        // Nothing here corroborated anything. The held rows are dropped rather
        // than written, and the chunks simply stay in the queue.
        return match last_refusal {
            Some(refusal) if !cancelled => Err(Error::Provider(refusal)),
            _ => Ok(()),
        };
    }
    for chunk_id in condemned {
        db.record_embedding_failure(space, chunk_id)?;
        tally.failed += 1;
    }
    Ok(())
}

/// Whether a provider failure can **only** be a statement about the answer to
/// the texts that were sent — the one condition under which it may be attributed
/// to a chunk.
///
/// **The `_` arm stops the run, and that is the design rather than a
/// leftover.** The two mistakes this function can make are not symmetrical.
/// Attributing wrongly is silent and permanent: a `failed` row is not
/// reconsidered until its chunk's text changes, so good chunks leave vector
/// search for good and the third number says "failed" about text that was never
/// the problem. Refusing to attribute wrongly is loud and reversible: the run
/// stalls on the same batch, the numbers do not move, nothing is lost, and the
/// next run behaves identically. Where we cannot tell the two apart we take the
/// loud one — and a variant added to [`mnema_provider::Error`] later falls to
/// the loud one without anybody having to remember this.
///
/// Each arm is a reading of that variant's own doc comment, not a guess at HTTP:
///
/// - `Malformed`, `CountMismatch`, `PositionMismatch` — facts about the *answer
///   to this request*: a body that is not the shape this code expects, the wrong
///   number of vectors, rows that do not say which text they embed.
/// - `UnusableVector`, `EmptyVector` — facts about the numbers that came back
///   for these texts. Neither is reachable from `embed` today (both are raised
///   by `check_embedding_model`), and both are here for the same reason the
///   `_` arm is: so the answer is already right if that changes.
/// - `Provider { status }` for **400, 413 and 422**, and no other status. These
///   three mean the server understood the request and refuses its *content* —
///   which is exactly the shape an over-long chunk is expected to arrive in, and
///   the thing the spec says nobody has measured yet.
///
/// **Deliberately not every 4xx**, which is the one place this departs from the
/// rule as it was handed down, and it departs on evidence: **402 Payment
/// Required** is what this provider answers for an exhausted account. Under
/// "any 4xx" the moment a user's credit ran out mid-run, the batch would be
/// split and every chunk in it written down as impossible to embed, permanently
/// — the exact catastrophe the rule exists to prevent, arriving through a door
/// nobody would think to look at. `408` and `425` fall on the safe side for
/// free, and any status nobody has thought about at all falls there too.
///
/// `ErrorInsteadOfEmbeddings` and `AveragedBatch` are also deliberately absent.
/// `ErrorInsteadOfEmbeddings` **is** reachable from `embed` — through
/// `unreadable_embeddings_answer` (`probe.rs:1306` → `:1013`), so it is a live
/// case rather than a theoretical one, and that is why it stops rather than a
/// reason to attribute it: a `200` carrying the provider's own error envelope
/// may be saying "input too long" or "quota exceeded", this build does not read
/// it closely enough to tell which, and under the asymmetry above an answer we
/// cannot interpret is one we must not blame a chunk for. `AveragedBatch`'s own
/// doc says in as many words that it is a fact about a *model*, established
/// once by `check_embedding_model`, rather than about anything that was sent.
fn speaks_only_about_these_texts(error: &mnema_provider::Error) -> bool {
    use mnema_provider::Error as Refusal;
    match error {
        Refusal::Malformed(_)
        | Refusal::CountMismatch { .. }
        | Refusal::PositionMismatch(_)
        | Refusal::UnusableVector(_)
        | Refusal::EmptyVector => true,
        Refusal::Provider { status, .. } => matches!(status, 400 | 413 | 422),
        _ => false,
    }
}

/// Writes one answered batch, after checking all of it.
///
/// Every width is checked **before the first write**, not each one before its
/// own: `WidthMismatch`'s own doc says the refusal happens before the write,
/// and a per-vector check makes that true of one chunk and false of the batch —
/// the mismatched vector at position three would be refused with the first two
/// already stored, in a space they do not describe.
fn store(
    db: &Db,
    space: i64,
    width: i64,
    pending: &[PendingChunk],
    vectors: &[Vec<f32>],
    tally: &mut EmbedTally,
) -> Result<(), Error> {
    if vectors.len() != pending.len() {
        return Err(Error::ShortAnswer {
            asked: pending.len(),
            got: vectors.len(),
        });
    }
    for vector in vectors {
        if vector.len() as i64 != width {
            return Err(Error::WidthMismatch {
                expected: width,
                got: vector.len() as i64,
            });
        }
    }
    for (chunk, vector) in pending.iter().zip(vectors) {
        // `upsert_vector` rather than `insert_vector`: a run that stopped
        // halfway through a batch must be able to finish without first asking
        // which half landed. It also clears any `chunk_embedding_state` row
        // for this chunk in the same transaction, which is what takes an
        // earlier refusal off the screen the moment the chunk is embedded.
        match db.upsert_vector(space, chunk.id, vector) {
            Ok(()) => tally.embedded += 1,
            Err(refusal) if refuses_this_chunks_vector(&refusal, chunk.id) => {
                db.record_embedding_failure(space, chunk.id)?;
                tally.failed += 1;
            }
            Err(other) => return Err(Error::Index(other)),
        }
    }
    Ok(())
}

/// Whether an error from `upsert_vector` is a verdict on *this chunk's vector*
/// rather than on the database.
///
/// It exists because the pass would otherwise stall permanently on one chunk,
/// silently, in the one place nothing else was watching. `upsert_vector` calls
/// `check_rankable`, which refuses a vector whose components are not finite or
/// whose norm underflows to something vec0 would rank as `NULL` or `-inf` —
/// and neither of those is a provider *error*: the request succeeded, the
/// answer was the right shape, the right width and the right count, and one of
/// its rows is simply unusable. Propagated as an error, that stops the run at
/// the same chunk on every restart, for ever, and writes nothing anybody could
/// read. Recorded as a failure, it is one number on a screen and the rest of
/// the archive gets embedded.
///
/// **Narrow on purpose, in two ways.** Only these two variants: everything else
/// `upsert_vector` can answer — a locked database, a missing space — is about
/// the machine and not about this text, and swallowing it into a `failed` row
/// would take a chunk out of vector search for a reason that will be gone in a
/// second. And only when the role names *this* chunk: `VectorRole::Stored(id)`
/// carries the id the refusal was about, and a refusal about some other one
/// would mean this crate has confused two chunks, which is not a thing to
/// record and carry on from.
fn refuses_this_chunks_vector(error: &mnema_index::Error, chunk_id: i64) -> bool {
    matches!(
        error,
        mnema_index::Error::NonFiniteVector {
            role: VectorRole::Stored(id),
            ..
        } | mnema_index::Error::UnrankableVector {
            role: VectorRole::Stored(id),
            ..
        } if *id == chunk_id
    )
}
