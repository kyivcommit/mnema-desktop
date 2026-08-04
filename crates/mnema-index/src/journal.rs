//! Writers for the three tables `schema.sql` calls "journals": `skipped`,
//! `document.status` and `ingest_stage`. Requirements §13 requires every
//! skipped file and every PDF page with no text layer to be recorded with the
//! rule that fired — a scanned page silently indexed as empty is the worst
//! available behaviour — and D26 requires a checkpoint a multi-hour indexing
//! job can resume from. Before this file all three tables existed with no
//! writer anywhere.

use std::collections::HashSet;

use mnema_core::OnDisk;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::{Db, Error, INDEX_FORMAT_VERSION};

/// Declares [`SkipRule`] and, from that same list of variants, the slice
/// [`SkipRule::every`] hands out. One list, two products: a variant cannot
/// exist without being enumerated, because the enumeration is generated from
/// the declaration rather than written beside it.
///
/// A macro for an enum this small is a heavy instrument, and it is here
/// because the lighter ones were measured failing. The list was first an array
/// of pairs in the tests, then a hand-written `after()` chain here that
/// `every()` walked. Both looked like coverage:
///
/// * deleting a variant from the three arrays in `tests/journal.rs` left every
///   test in that file green;
/// * a variant added to the enum with `Fictitious => return None` in `after()`
///   compiled, ended the chain early, and left the suite green at sixteen
///   passed — including the test whose whole job was to assert the chain
///   reaches every rule. The exhaustive `match` forced the new variant to
///   *appear*; nothing forced the chain to *reach* it, and the only guard was
///   a length assertion that the truncated chain still satisfied.
///
/// Neither failure is a mistake anyone made twice on purpose. Both are the same
/// shape: a list that promises to grow with the enum and has no way to.
macro_rules! declare_skip_rules {
    ($($(#[$attr:meta])* $variant:ident,)+) => {
        /// Which rule caused a file, or one page of it, to be skipped. The
        /// vocabulary is closed on purpose — an open `rule` column turns a
        /// writer's typo into a row `skips_for_root` can still list but a
        /// future query grouping by rule can never match again.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum SkipRule {
            $($(#[$attr])* $variant,)+
        }

        impl SkipRule {
            /// Every variant, in declaration order, generated from the list
            /// that declares them. Read [`every`](SkipRule::every) instead —
            /// this exists so that there is nothing to keep in step.
            const ALL: &[SkipRule] = &[$(SkipRule::$variant,)+];
        }
    };
}

declare_skip_rules! {
    Crash,
    Timeout,
    Memory,
    Unsupported,
    NoTextLayer,
    /// The file could not be read at all: the path did not exist, was not a
    /// regular file, or permissions refused it. Added by the pool (task 8),
    /// which is the first code that had to name this outcome: the extraction
    /// worker reports it as `wire::Frame::Failed` and none of the five rules
    /// above covers it — a file that was never there is not a crash, not a
    /// timeout, not a memory kill, and not an unsupported format.
    ///
    /// It earns a rule of its own rather than being folded into `Unsupported`
    /// because the two demand different things of the user. An unsupported
    /// format is a limit of this product and stays skipped until the product
    /// grows a reader; an unreadable file is a fact about the user's disk that
    /// may well be transient (a file moved mid-scan, a permission fixed
    /// afterwards) and is worth retrying on the next pass.
    ///
    /// **It also covers a reader that could not be brought up at all** — the
    /// library it needs missing, the wrong build, or refused by code signing —
    /// which is *not* a fact about the user's disk and is the one place this
    /// rule's name undersells what it holds. It is here because its three
    /// answers are the ones that case needs and no other rule's are: keep the
    /// document, count towards a broken environment, and do not remember the
    /// verdict, so the walk after the repair asks again.
    /// [`SkipRule::Malformed`](Self::Malformed) carries what routing it onto a
    /// content rule would cost instead.
    ///
    /// The name is therefore coarser than the cases under it, and `reason` is
    /// where the difference lives: the pool copies the worker's own message
    /// through unchanged, so "could not load libpdfium" and "no such file" are
    /// one rule and two sentences. A window grouping by rule alone will show
    /// them together; that is a known limit of this vocabulary rather than an
    /// oversight, and the alternative — a rule per environmental cause — is a
    /// decision of its own.
    ///
    /// `skipped.rule` is a plain `TEXT` column with no CHECK constraint
    /// (`schema.sql:233`), so adding this value needed no migration and no
    /// `SCHEMA_VERSION` bump.
    Unreadable,
    /// The file is larger than the ceiling the pool was configured with, and
    /// was refused from `stat` before a byte of it was read
    /// (`crates/mnema-extract/src/bin/worker.rs`).
    ///
    /// Split out of `Unsupported`, which it was folded into until the two were
    /// found to want different things.
    ///
    /// It is a different answer to the user. `Unsupported` says this product
    /// has no reader for that format and the file stays skipped until the
    /// product grows one; this says the file is fine and a *setting* excluded
    /// it. "Which files were too large?" is a question someone deciding
    /// whether to raise the ceiling needs the journal to answer, and while the
    /// two shared a rule it could not.
    ///
    /// And it is a different answer to the index. `mnema-ingest` removes what
    /// it holds under a path when the worker read a file and declined its
    /// content — a `.txt` overwritten by a PDF must stop answering under its
    /// own name. This branch never opens the file; it decides from `stat`
    /// alone, so the refusal itself says nothing about whether the content
    /// changed. That is settled there against what the walk measured — the size
    /// **and** the modification time the `path` row recorded — and settled
    /// against nothing else, because a refusal made without opening the file
    /// leaves no reading of the content to compare. The size alone is not
    /// enough, and the argument that it was refuted itself: it excluded a
    /// same-length rewrite by assuming the ceiling had not moved, inside the one
    /// rule that exists because it can. `mnema_ingest`'s `displaces` carries it
    /// in full, along with what is left over and why it cannot be closed there.
    TooLarge,
    /// The file is not text at all — a photo, a video, a database — decided
    /// by its own bytes rather than its name (D51).
    ///
    /// Its own rule rather than `Unsupported` for the reason `TooLarge` got
    /// one: the two answer the user differently. `Unsupported` says this
    /// product has no reader *yet* and the file waits for one; this says the
    /// file is not the kind of thing this product reads, and no release will
    /// change that. Someone asking "why is my file not found?" needs the two
    /// apart.
    ///
    /// Unlike `TooLarge`, whose lever back to re-examination is a *setting*
    /// (`PoolConfig::max_bytes`), this rule's only lever is a constant:
    /// `INDEX_FORMAT_VERSION` (`mnema_ingest::ingest_file`, the second cheap
    /// arm). A file skipped `NotText` is never looked at again for the life
    /// of the index unless that version moves — so any future loosening of
    /// `classify` (the first candidate: UTF-16 without a byte-order
    /// mark) must bump it, or the files it would now accept stay refused
    /// forever.
    NotText,
    /// The file is text for its first bytes and binary after that — it *began*
    /// as text and stopped being one (D51). Decided by its own bytes, like
    /// `NotText`, and refused like it.
    ///
    /// Its own rule rather than `NotText` for one reason, and that reason is
    /// the whole of why it exists: **this one must not displace.** `NotText`
    /// says the path now holds a photo, so whatever text the index has under
    /// that name belongs to a file that is gone. This says the path holds a
    /// note whose append was interrupted — the power went out, the tail came
    /// back zeroed, and the prose the index holds is still, byte for byte, the
    /// opening of the file on disk. Deleting it loses text that is readable
    /// nowhere else. `mnema_ingest`'s `displaces` is where that lands.
    ///
    /// It shares `NotText`'s other half: the verdict is reproducible on the
    /// same bytes, so `is_about_content` is true and the next walk answers from
    /// `stat` without spending a worker. The consequence above is the same one
    /// too — a file refused here is not looked at again until
    /// `INDEX_FORMAT_VERSION` moves — and it costs less here, because what the
    /// user still has under that path is the document they had before rather
    /// than an absence.
    BinaryTail,
    /// The file is the format its magic claims and this product has a reader
    /// for it, and the bytes are damaged: a PDF that ends mid-object, a zip
    /// whose central directory does not parse.
    ///
    /// **Not `Unsupported`** — that one promises a reader that will arrive, and
    /// for this file one already has. The two answer the user differently:
    /// "this product cannot read that format yet" is a limit of the product,
    /// and "this file is broken" is a fact about the file, which is the answer
    /// someone looking at the skip list needs in order to go and fetch the file
    /// again.
    ///
    /// **Not `Unreadable`** — that is a fact about the *disk*: the path was
    /// gone, or not a regular file, or permissions refused it, so no reader saw
    /// a byte. Here a reader opened the file, read it and could not finish, and
    /// the two part company where it costs a document: `Unreadable` never
    /// displaces, because a share that drops mid-walk would otherwise empty the
    /// index, and this rule displaces when the digest says the file is not the
    /// one the index was built from.
    ///
    /// A determination about the bytes, so `is_about_content` is true and the
    /// next walk answers from `stat` without spending a worker. That carries
    /// `NotText`'s consequence with it: a file refused here is not looked at
    /// again for the life of the index unless `INDEX_FORMAT_VERSION` moves — so
    /// a release whose reader survives damage this one gives up on must bump
    /// it, or the files it could now read stay refused forever.
    ///
    /// ⚠️ **The limit that makes both of those answers correct: this rule means
    /// the *content* is damaged, and a reader that never got as far as the
    /// content does not belong here.** A reader whose library would not load —
    /// missing, the wrong build, or refused by code signing, three causes
    /// `crates/mnema-extract/src/pdfium_probe.rs` already separates as
    /// `Stage::LibraryDir`, `Stage::VerifyBuild` and `Stage::Bind` — has learned
    /// nothing whatever about the file it was handed. Routing that here is the
    /// failure `mnema_pool`'s `PoolError` names in its own doc comment, arriving
    /// one layer lower: "ten thousand files as damaged when the real fault is a
    /// half-finished install". The shape it takes is a reader that folds every
    /// error it can produce into this rule with one catch-all arm.
    ///
    /// It costs more here than there, because all three of this rule's other
    /// answers are tuned for damage and all three are wrong for a broken
    /// install.
    ///
    /// * `suggests_broken_environment` is false — right for a folder of
    ///   truncated downloads, and it means a run of these will **not** stop the
    ///   walk.
    /// * Worse than not stopping it: `walk_root` **resets**
    ///   `consecutive_environmental` to zero on any rule that answers false
    ///   (`crates/mnema-ingest/src/walk.rs`, the `else` beside the counter). So
    ///   a folder holding PDFs among other files does not merely fail to raise
    ///   the alarm — it wipes the count of a genuine environmental run passing
    ///   through at the same time, and the walk that should have stopped
    ///   carries on.
    /// * `is_about_content` is true, so every one of those rows outlives the
    ///   repair: quarantine a library, walk a folder, fix the installation, and
    ///   nothing comes back until `INDEX_FORMAT_VERSION` moves.
    ///
    /// A whole library of PDFs journalled as broken files, by a walk that
    /// reports success and has had its one alarm silenced.
    ///
    /// **The way out is `Frame::Failed`**, which the pool reads as
    /// `Unreadable`: the document is kept, the run counts as evidence about the
    /// environment, and nothing is remembered, so the walk after the repair
    /// asks again. Its `message` is carried into `skipped.reason` verbatim,
    /// which is what makes a quarantined library diagnosable — the rule says
    /// `unreadable` and the reason says which library and why. Both
    /// `wire::Frame::Failed` and `SkipRule::Unreadable` say so on their own
    /// side; this is the same agreement read from here.
    ///
    /// Dying instead lands on `Crash`, which has the same two flags and is
    /// honest as far as it goes, but the cause then survives only in the
    /// worker's stderr file: the user is told the worker died, not that a
    /// library would not load. Note also that `"crash"` is not among the rule
    /// strings the pool's wire `match` accepts, so a worker cannot ask for that
    /// one by name.
    Malformed,
    /// The file is whole, the reader is the right one, and the text is behind a
    /// password.
    ///
    /// **Not `Malformed`**, although both arrive from a reader that opened the
    /// file and produced nothing, and although `displaces` gives them the
    /// identical condition today. They are different things to the person
    /// reading the skip list: one is fixed by supplying a password and the
    /// other is not fixed. "Which of my documents are locked?" is a question
    /// this journal can only answer while the two have separate rules, and it
    /// is the question that decides whether a password prompt is worth
    /// building.
    ///
    /// **Not `Unsupported`** either, for a sharper reason than `Malformed`'s: a
    /// locked file is not a format this product lacks a reader for. Folding it
    /// there would say a future release might read it, when what is missing is
    /// not code but a key the user has.
    ///
    /// `is_about_content` for the same reason as `Malformed` and with the same
    /// consequence: the same bytes stay locked, so the journal answers for them
    /// until `INDEX_FORMAT_VERSION` moves. Whoever builds a password prompt has
    /// to move it, or the files the prompt exists for are the ones it never
    /// gets asked about.
    Encrypted,
}

impl SkipRule {
    pub fn as_str(self) -> &'static str {
        match self {
            SkipRule::Crash => "crash",
            SkipRule::Timeout => "timeout",
            SkipRule::Memory => "memory",
            SkipRule::Unsupported => "unsupported",
            SkipRule::NoTextLayer => "no_text_layer",
            SkipRule::Unreadable => "unreadable",
            SkipRule::TooLarge => "too_large",
            SkipRule::NotText => "not_text",
            SkipRule::BinaryTail => "binary_tail",
            SkipRule::Malformed => "malformed",
            SkipRule::Encrypted => "encrypted",
        }
    }

    /// Not a public `FromStr`, mirroring `DocumentStatus::parse` for the same
    /// reason: the only source of this string is the `rule` column, and that
    /// column carries no CHECK, so `None` here means some row was written
    /// around every write path this crate exposes — not that the caller made
    /// a mistake.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "crash" => SkipRule::Crash,
            "timeout" => SkipRule::Timeout,
            "memory" => SkipRule::Memory,
            "unsupported" => SkipRule::Unsupported,
            "no_text_layer" => SkipRule::NoTextLayer,
            "unreadable" => SkipRule::Unreadable,
            "too_large" => SkipRule::TooLarge,
            "not_text" => SkipRule::NotText,
            "binary_tail" => SkipRule::BinaryTail,
            "malformed" => SkipRule::Malformed,
            "encrypted" => SkipRule::Encrypted,
            _ => return None,
        })
    }

    /// Every variant, in declaration order.
    ///
    /// A test that means "every rule" iterates this instead of writing its own
    /// list, so that adding a variant cannot leave one of them quietly covering
    /// every rule except the new one. What makes that true is not this function but
    /// [`declare_skip_rules`]: the slice it reads is generated from the same
    /// list that declares the variants, so there is no step at which a variant
    /// can be declared and left out. The chain of `after()` links this replaced
    /// could be — and was, when measured.
    ///
    /// `pub` rather than `pub(crate)` or `#[cfg(test)]` because the tests that
    /// need it are integration tests, which link this crate the way any other
    /// caller does and see neither. The surface is small and honest — a closed
    /// vocabulary that can be enumerated is a reasonable thing for a journal to
    /// expose, and a window listing "which rules can appear here" wants exactly
    /// this.
    pub fn every() -> impl Iterator<Item = Self> {
        SkipRule::ALL.iter().copied()
    }

    /// Whether this rule is a **reproducible** determination about the file's
    /// own bytes: the same bytes will earn the same verdict from the worker
    /// again, with nothing outside the file able to change the answer.
    ///
    /// `record_skip` uses this to decide whether `bytes` is worth keeping: a
    /// content rule earns the same verdict again from the same bytes, so the
    /// next walk can answer from `stat` alone and skip the worker process
    /// entirely (`mnema_ingest::ingest_file`'s second cheap arm). Getting this
    /// wrong in the content direction is the expensive mistake — it makes a
    /// verdict that *can* change look permanent, and the file is never looked
    /// at again for the life of the index.
    ///
    /// Only `Unsupported`, `NoTextLayer`, `NotText`, `BinaryTail`, `Malformed`
    /// and `Encrypted` qualify — the last two because damage and a password are
    /// both properties of the bytes: the same truncated file truncates the same
    /// reader again, and the same locked file stays locked.
    /// `Crash`, `Timeout` and `Memory` are readings of the environment that
    /// apply to every file in the walk alike — `displaces` draws the same line
    /// for the same reason (D44) — and `Unreadable` is a fact about the disk,
    /// not the bytes, that may well be transient (a file moved mid-scan, a
    /// permission fixed afterwards).
    ///
    /// `BinaryTail` is the one variant where this predicate and `displaces` no
    /// longer answer alike, and that is not a slip. The verdict *is* about the
    /// bytes and is reproducible on them, so it belongs here; what `displaces`
    /// asks is a different question — whether the text the index already holds
    /// under that path has stopped being what the file says — and for an
    /// interrupted append the answer is no.
    ///
    /// **`TooLarge` looks like it belongs here, and does not.** The refusal
    /// does come from `stat` alone, and `displaces` does treat it as
    /// reproducible enough to compare against `path.size_bytes` — which is
    /// exactly the reasoning that put it on this side once, until a branch
    /// review measured the case it breaks. The verdict is not a fact about
    /// the bytes; it is a fact about `PoolConfig::max_bytes`, a setting the
    /// user can change, and `INDEX_FORMAT_VERSION` does not move when a
    /// slider does. Measured directly: a file refused under a low ceiling
    /// stayed `Skipped { TooLarge }` after the ceiling was raised well past
    /// its size, because the second cheap arm kept answering from the
    /// journal and the pool was never asked again —
    /// `a_raised_ceiling_re_examines_a_file_it_used_to_refuse` in
    /// `mnema-ingest/tests/slice.rs` pins it.
    ///
    /// Keeping it off this side has a price, and it is the one this function
    /// exists to avoid paying: every file over the ceiling costs a full worker
    /// round-trip on every walk, because the ceiling is checked *inside* the
    /// worker (`crates/mnema-extract/src/bin/worker.rs`), not before it is
    /// asked. A folder of large archives therefore pays per walk what a folder
    /// of scans used to. That is the honest cost of the ceiling being a live
    /// setting; the alternative was a file the user can never make the product
    /// look at again.
    ///
    /// An exhaustive `match` rather than a `matches!`, for the reason
    /// `displaces` spells out at greater length: a variant added to the enum
    /// without a decision here would otherwise fall to `false` silently. That
    /// direction is the safe one — never remembering the bytes only costs a
    /// worker round-trip — but "safe by accident" is not what the test named
    /// after this function claims to guarantee, and it was measured claiming
    /// it falsely: a new variant added with no line here left the whole
    /// journal suite green.
    pub fn is_about_content(self) -> bool {
        match self {
            SkipRule::Unsupported
            | SkipRule::NoTextLayer
            | SkipRule::NotText
            | SkipRule::BinaryTail
            | SkipRule::Malformed
            | SkipRule::Encrypted => true,
            SkipRule::Crash
            | SkipRule::Timeout
            | SkipRule::Memory
            | SkipRule::Unreadable
            | SkipRule::TooLarge => false,
        }
    }

    /// Whether this rule's *recurrence* — many of these in a row — says
    /// something is wrong with the worker, the machine or the volume, rather
    /// than with the files it keeps naming.
    ///
    /// A DIFFERENT question from [`is_about_content`](Self::is_about_content),
    /// not a variant of it. `is_about_content` asks whether the same bytes
    /// earn the same verdict again, which decides whether the skip journal is
    /// worth trusting on its own, without spending a worker on the file a
    /// second time. This asks whether a run of them means a walker should
    /// stop asking a worker to do more work at all. The two questions split
    /// the same variants differently, and `TooLarge` is the case that
    /// proves it has to: it answers **no** to both. It is not a fact about
    /// the bytes (`is_about_content` — a setting can move the ceiling out
    /// from under a file that never changed), and it is not a fact about the
    /// environment either — a folder that happens to hold several large
    /// archives in a row is not a broken machine, it is an ordinary folder.
    /// Counting it here would be the mistake `is_about_content`'s own doc
    /// comment records as measured (a folder of large archives paying a
    /// worker round-trip per walk was accepted as the honest cost of a
    /// *live* ceiling), made a second time one level up: a few consecutive
    /// large files would read as a dying worker and end a walk that has done
    /// nothing wrong.
    ///
    /// Only `Crash`, `Timeout`, `Memory` and `Unreadable` qualify — four of the
    /// five `displaces` (`mnema_ingest`) keeps content for, and for the same
    /// reason spelled out there at length: each is a reading of the
    /// environment, not of one file, so a run of them in a row is evidence
    /// about what is *outside* the files rather than a coincidence of which
    /// files happened to be next in the walk.
    ///
    /// The fifth is `BinaryTail`, and it is the second variant after `TooLarge`
    /// to show that these predicates cannot be derived from one another. It is
    /// kept by `displaces` and answers **no** here: a folder holding several
    /// interrupted or truncated files in a row says something happened to those
    /// files — a power cut, a copy that stopped — not that the worker reading
    /// them is dying, and ending the walk would leave the rest of the folder
    /// unindexed over it.
    ///
    /// `Malformed` and `Encrypted` answer **no** on exactly that reasoning, and
    /// their answers depend on a limit held somewhere else rather than on
    /// anything visible here. `SkipRule::Malformed`'s own doc comment carries
    /// it: a reader that could not load its library at all must not report
    /// either rule, because a broken install then produces a long run of them
    /// and this predicate — correctly, for damage and for a password — declines
    /// to stop the walk, and `walk_root` clears the counter besides.
    ///
    /// An exhaustive `match`, matching `is_about_content`'s own reasoning for
    /// being one: a variant added to the enum with no line here would
    /// otherwise answer silently, and neither default is safe to assume —
    /// "not broken" lets a genuinely dying worker run to the end of a
    /// multi-hour walk, and "broken" stops a walk over an ordinary folder
    /// that has nothing wrong with it.
    pub fn suggests_broken_environment(self) -> bool {
        match self {
            SkipRule::Crash | SkipRule::Timeout | SkipRule::Memory | SkipRule::Unreadable => true,
            SkipRule::Unsupported
            | SkipRule::NoTextLayer
            | SkipRule::TooLarge
            | SkipRule::NotText
            | SkipRule::BinaryTail
            | SkipRule::Malformed
            | SkipRule::Encrypted => false,
        }
    }
}

/// Mirrors `document.status`'s own CHECK (`schema.sql:71-72`). Answers only
/// "may this document be searched?" — which stage it reached and why it
/// stopped there is `ingest_stage`'s business, not this column's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentStatus {
    Pending,
    Indexed,
    Failed,
    Skipped,
}

impl DocumentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentStatus::Pending => "pending",
            DocumentStatus::Indexed => "indexed",
            DocumentStatus::Failed => "failed",
            DocumentStatus::Skipped => "skipped",
        }
    }

    /// Not a public `FromStr`: the only source of this string is the `status`
    /// column, guarded by the schema's own CHECK on every write path this
    /// crate exposes — so `None` here means some row was written around it,
    /// not that the caller made a mistake. `document_status` turns that into
    /// `Error::UnknownDocumentStatus` rather than trusting the CHECK as a
    /// proof and panicking on the gap.
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => DocumentStatus::Pending,
            "indexed" => DocumentStatus::Indexed,
            "failed" => DocumentStatus::Failed,
            "skipped" => DocumentStatus::Skipped,
            _ => return None,
        })
    }
}

/// One row of the skip journal, read back for a watched root.
///
/// `Serialize` with `camelCase`: the `skips` shell command (`src-tauri/src/
/// bridge.rs`) returns this straight to the webview, and every other type
/// that crosses that seam renders its fields the way the window's JavaScript
/// reads them — the rename lives on the type that crosses, not on a
/// bridge-local copy, because nothing about this shape needs translating
/// first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedFile {
    pub relative_path: String,
    /// `None` for a whole-file skip (a worker crash, a timeout); `Some` when a
    /// single PDF page was skipped inside an otherwise readable document.
    pub page_no: Option<i64>,
    pub reason: String,
    pub rule: String,
}

/// The current verdict for one whole file, as read back by [`Db::skip_entry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipEntry {
    pub rule: SkipRule,
    /// The bytes the walk stat'ed when this verdict was recorded. `None`
    /// whenever `rule.is_about_content()` is false, since `record_skip` drops
    /// them on the floor for those rules regardless of what the caller passed
    /// — but also `None`, for any rule, when the caller had no measurement to
    /// hand `record_skip` in the first place (`bytes: None`, e.g. a file the
    /// walk could not stat at all). A bare `None` here does not by itself say
    /// which of the two happened.
    pub size_bytes: Option<i64>,
    pub mtime: Option<i64>,
    pub format_version: i64,
}

impl Db {
    /// Records that a file, or one page of it, did not make it into the index,
    /// replacing whatever this path last said.
    ///
    /// `bytes` is what the walk stat'ed. It is stored only when `rule` is a
    /// statement about the file's content; for an environmental rule it is
    /// dropped on the floor here rather than at the call site, so that no
    /// caller can get the asymmetry wrong.
    pub fn record_skip(
        &self,
        root_id: i64,
        relative_path: &str,
        page_no: Option<i64>,
        reason: &str,
        rule: SkipRule,
        bytes: Option<OnDisk>,
    ) -> Result<(), Error> {
        let bytes = if rule.is_about_content() { bytes } else { None };
        self.conn().execute(
            "INSERT INTO skipped
                (watched_root_id, relative_path, page_no, reason, rule,
                 size_bytes, mtime, format_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (COALESCE(watched_root_id, -1), relative_path, COALESCE(page_no, -1))
             DO UPDATE SET reason = excluded.reason,
                           rule = excluded.rule,
                           size_bytes = excluded.size_bytes,
                           mtime = excluded.mtime,
                           format_version = excluded.format_version,
                           at = unixepoch()",
            params![
                root_id,
                relative_path,
                page_no,
                reason,
                rule.as_str(),
                bytes.map(|b| b.size_bytes),
                bytes.map(|b| b.mtime),
                INDEX_FORMAT_VERSION
            ],
        )?;
        Ok(())
    }

    /// The current verdict for a whole file, if there is one.
    pub fn skip_entry(
        &self,
        root_id: i64,
        relative_path: &str,
    ) -> Result<Option<SkipEntry>, Error> {
        self.conn()
            .query_row(
                "SELECT rule, size_bytes, mtime, format_version FROM skipped
                  WHERE watched_root_id = ?1 AND relative_path = ?2 AND page_no IS NULL",
                params![root_id, relative_path],
                |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .map(|(rule, size_bytes, mtime, format_version)| {
                Ok(SkipEntry {
                    rule: SkipRule::parse(&rule).ok_or_else(|| Error::UnknownSkipRule(rule))?,
                    size_bytes,
                    mtime,
                    format_version,
                })
            })
            .transpose()
    }

    /// Every skip recorded under one watched root, oldest first.
    pub fn skips_for_root(&self, root_id: i64) -> Result<Vec<SkippedFile>, Error> {
        let mut stmt = self.conn().prepare(
            "SELECT relative_path, page_no, reason, rule FROM skipped
              WHERE watched_root_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![root_id], |r| {
                Ok(SkippedFile {
                    relative_path: r.get(0)?,
                    page_no: r.get(1)?,
                    reason: r.get(2)?,
                    rule: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Forgets the whole-file verdict recorded against one path, if there is
    /// one.
    ///
    /// The counterpart of [`record_skip`](Db::record_skip), and it exists
    /// because until it did there was exactly one `DELETE FROM skipped` in the
    /// tree — [`forget_skips_not_in`](Db::forget_skips_not_in), reconciliation's
    /// own, which fires only for paths a complete walk did **not** see. A file
    /// refused once and then indexed successfully therefore kept its refusal for
    /// the life of the index, and that row is not inert. It is what the window
    /// answering "why is this file not in my index?" reads, so that list named
    /// files that are in it; and it is what `mnema_ingest::ingest_file`'s second
    /// cheap arm answers from, which compares `(size, mtime, format_version)`
    /// and never asks whether the verdict was reached on *these* bytes. Put a
    /// previous version back with its own modification time — `cp -p`, `tar
    /// -xp`, a cloud client's "restore previous version" — and the stale row
    /// matched again and answered for a file nobody had looked at.
    ///
    /// Only whole-file rows (`page_no IS NULL`), matching what `skip_entry`
    /// reads and what `ingest_file` writes for a file. A per-page row belongs to
    /// one page of one document and is not this path's verdict; the reader that
    /// will produce those does not exist yet, and folding them in here would
    /// silently erase them the moment it does.
    pub fn forget_skip(&self, root_id: i64, relative_path: &str) -> Result<(), Error> {
        self.conn().execute(
            "DELETE FROM skipped
              WHERE watched_root_id = ?1 AND relative_path = ?2 AND page_no IS NULL",
            params![root_id, relative_path],
        )?;
        Ok(())
    }

    /// Removes skip rows under `root_id` whose path is not in `seen` and does
    /// not sit under one of `frozen_prefixes`, and returns how many were
    /// removed.
    ///
    /// `seen` must be built the same way reconciliation builds it for `path`
    /// (`mnema-ingest`'s `walk_root`, phase 3): `Walked::found` plus every
    /// pre-skip that carries a path — never `found` alone. A pre-skip with a
    /// path (`NotMaterialised`, `NotAFile`, `NotAFileSubtree`) means the file
    /// is still there and the walk chose not to touch it, not that it is
    /// gone. Forgetting its skip row on the strength of `found` alone would
    /// erase the very explanation `record_skip` exists to keep — and since
    /// the file is untouched, the very next walk offers the same file to the
    /// journal again, so "why is this file not in my index?" would have an
    /// answer that deletes itself and comes back on every single walk.
    ///
    /// `frozen_prefixes` must be the same set the `path` deletion loop uses
    /// to skip a `known` path — a symlinked directory, or an unmounted
    /// nested share, that the walk has no evidence about at all. `seen`
    /// alone under-protects here: a symlink's own row is in `seen` (its
    /// `relative` is the symlink's path, with a path), but a *stale* skip
    /// row for something that used to live underneath it — recorded on a
    /// walk before the symlink existed — has a `relative_path` `seen` never
    /// names either. Measured directly: without this, replacing an indexed
    /// directory with a symlink to a directory took a skip row for a file
    /// inside it from `["linked/inner.pdf"]` to `["linked"]` on the very
    /// next walk, even though the `path` row for `linked/inner.pdf` was
    /// correctly kept — and because the walk never descends into a
    /// `NotAFileSubtree` symlink, nothing ever re-creates that row, unlike
    /// an ordinarily pruned one.
    pub fn forget_skips_not_in(
        &self,
        root_id: i64,
        seen: &HashSet<&str>,
        frozen_prefixes: &[&str],
    ) -> Result<u64, Error> {
        let list = serde_json::to_string(seen)?;
        // Candidates only, not the delete itself: prefix matching is not
        // something a `NOT IN (json_each(...))` clause can express, and GLOB
        // would need `*`, `?` and `[...]` escaped out of every prefix before
        // it could be trusted as a literal string rather than a pattern —
        // simpler and safer to filter the (typically tiny) candidate list in
        // Rust with a plain string comparison, the same one the `path`
        // deletion loop uses.
        let candidates: Vec<String> = self
            .conn()
            .prepare(
                "SELECT relative_path FROM skipped
                  WHERE watched_root_id = ?1
                    AND relative_path NOT IN (SELECT value FROM json_each(?2))",
            )?
            .query_map(params![root_id, list], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;

        let mut n = 0u64;
        for relative in candidates {
            if frozen_prefixes
                .iter()
                .any(|prefix| under_prefix(&relative, prefix))
            {
                continue;
            }
            n += self.conn().execute(
                "DELETE FROM skipped WHERE watched_root_id = ?1 AND relative_path = ?2",
                params![root_id, relative],
            )? as u64;
        }
        Ok(n)
    }

    /// Sets a document's lifecycle status.
    pub fn set_document_status(&self, id: &str, status: DocumentStatus) -> Result<(), Error> {
        self.conn().execute(
            "UPDATE document SET status = ?1 WHERE id = ?2",
            params![status.as_str(), id],
        )?;
        Ok(())
    }

    pub fn document_status(&self, id: &str) -> Result<DocumentStatus, Error> {
        let s: String = self.conn().query_row(
            "SELECT status FROM document WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        DocumentStatus::parse(&s).ok_or_else(|| Error::UnknownDocumentStatus(s))
    }

    /// Records that `stage` reached `status` for the document hashing to
    /// `content_hash` — the checkpoint a multi-hour indexing job resumes from.
    ///
    /// `content_hash` here is the **document's** content hash, settled by D26:
    /// the unit of indexing work is one document, written in one transaction,
    /// so the checkpoint keys on the same thing the transaction does. That
    /// name collides with `chunk.content_hash`, which hashes a *chunk's* own
    /// text and is half of the embedding cache key (`write.rs`) — two columns
    /// sharing a name and meaning different things, not the same fact at two
    /// grains.
    ///
    /// Upserts rather than only inserting: `(content_hash, stage)` is the
    /// primary key, and resuming a job re-attempts whatever stage it did not
    /// finish, which must update the existing row instead of failing with a
    /// uniqueness violation.
    pub fn record_stage(&self, content_hash: &str, stage: &str, status: &str) -> Result<(), Error> {
        self.conn().execute(
            "INSERT INTO ingest_stage (content_hash, stage, status)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (content_hash, stage) DO UPDATE SET
                status = excluded.status,
                updated_at = unixepoch()",
            params![content_hash, stage, status],
        )?;
        Ok(())
    }

    /// What `stage` last reached for this document, or `None` if it has never
    /// been recorded.
    ///
    /// The half of D26's checkpoint that was missing: `record_stage` has been
    /// able to write since task 5 and nothing could read, which makes a
    /// checkpoint a log. A second pass over the same folder asks this before it
    /// spends a worker process on a document it has already finished.
    ///
    /// `Option<String>` rather than a bool or a typed enum. Not a bool, because
    /// "never attempted" and "attempted and failed" ask opposite things of the
    /// next run and a bool cannot hold both. Not an enum, because unlike
    /// `document.status` this column has **no CHECK** (`schema.sql:219-224`) and
    /// no closed vocabulary anywhere — the stages belong to whoever is running
    /// the pipeline, and the day a stage is added is not a day the index crate
    /// should have to be edited.
    pub fn stage_status(&self, content_hash: &str, stage: &str) -> Result<Option<String>, Error> {
        Ok(self
            .conn()
            .query_row(
                "SELECT status FROM ingest_stage WHERE content_hash = ?1 AND stage = ?2",
                params![content_hash, stage],
                |r| r.get(0),
            )
            .optional()?)
    }
}

/// Whether `relative` names something inside the subtree `prefix` names —
/// prefix-plus-separator, not a bare string prefix: `"linked_dirs/x"` must
/// not match against `"linked_dir"`. Mirrors `mnema-ingest`'s own `under()`
/// (`crates/mnema-ingest/src/walk.rs`) — duplicated rather than shared,
/// because `mnema-index` has no dependency on `mnema-ingest` to share it
/// through; the dependency runs the other way.
fn under_prefix(relative: &str, prefix: &str) -> bool {
    relative
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('/'))
}
