//! The four ways a worker dies, and what the supervisor owes each one.
//!
//! Every test here drives a real child process through the real NDJSON
//! protocol. Three of the four failure modes were measured on this machine
//! before the pool was designed (D40): the 65,536-byte stderr deadlock, the
//! ~4 ms process cost that makes batching worth having, and a read on a named
//! pipe that blocks forever instead of erroring.
//!
//! Two rules shape the whole file. A test may fail, but it may not **hang** —
//! a wedged supervisor in CI is indistinguishable from a slow one — so every
//! test arms a `Watchdog`. And a test may not leave a process behind: the stand-in
//! worker self-destructs after two minutes, and the pool kills and reaps every
//! child it owns when it is dropped, which is at the end of each test.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use mnema_index::SkipRule;
use mnema_pool::{Document, Failure, Pool, PoolConfig, PoolError, Skip};
// Used only by `a_memory_ceiling_this_platform_cannot_impose_is_refused_rather_than_faked`,
// which is itself `cfg(not(target_os = "linux"))` — Linux is the one platform where the
// ceiling can be imposed. Imported under the same condition as its only user, because
// `-D warnings` makes an import that is dead on one platform a build failure there, and
// this file's other assertions all pass on both.
#[cfg(not(target_os = "linux"))]
use mnema_pool::{MemoryCeiling, memory_ceiling};

/// The stand-in worker (`src/bin/test_worker.rs`), whose behaviour is selected
/// by the prefix on the requested path.
fn config() -> PoolConfig {
    PoolConfig::new(env!("CARGO_BIN_EXE_mnema-pool-test-worker"))
}

fn extract(pool: &Pool, path: &str) -> Result<mnema_pool::Outcome, PoolError> {
    pool.extract(Path::new(path))
}

fn document(outcome: mnema_pool::Outcome) -> Document {
    match outcome {
        mnema_pool::Outcome::Extracted(document) => document,
        mnema_pool::Outcome::Skipped(skip) => panic!("expected a document, got {skip:?}"),
    }
}

/// Every block of a document, pages flattened away. For the tests that only
/// care that text came back at all — which page it sat on is another test's
/// question.
fn blocks_of(document: &Document) -> Vec<&mnema_core::Block> {
    document
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .collect()
}

fn skip(outcome: mnema_pool::Outcome) -> Skip {
    match outcome {
        mnema_pool::Outcome::Skipped(skip) => skip,
        mnema_pool::Outcome::Extracted(document) => {
            panic!("expected a skip, got {} pages", document.pages.len())
        }
    }
}

/// `cargo test` has no per-test timeout, so this is one. It aborts the whole
/// run rather than letting a hang wedge CI; the stand-in workers left behind
/// self-destruct on their own timer, which is the only reason exiting this
/// abruptly is acceptable.
struct Watchdog(Arc<AtomicBool>);

impl Watchdog {
    fn new(label: &'static str, bound: Duration) -> Self {
        let finished = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&finished);
        std::thread::spawn(move || {
            let deadline = Instant::now() + bound;
            while Instant::now() < deadline {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            eprintln!(
                "watchdog: {label} did not finish within {bound:?}. Failing loudly \
                 rather than hanging: the supervisor's own deadline is broken."
            );
            std::process::exit(103);
        });
        Watchdog(finished)
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

// --- 1. The measured deadlock ------------------------------------------------

#[test]
fn a_worker_that_floods_stderr_does_not_deadlock_the_parent() {
    let _watchdog = Watchdog::new("stderr flood", Duration::from_secs(30));
    let noise = tempfile::tempdir().unwrap();
    let log = noise.path().join("worker.log");

    let pool = Pool::new(PoolConfig {
        workers: 1,
        batch: 100,
        timeout: Duration::from_secs(5),
        diagnostics: Some(log.clone()),
        ..config()
    })
    .unwrap();

    let document = document(extract(&pool, "noisy:simple.txt").unwrap());
    assert!(
        !blocks_of(&document).is_empty(),
        "65 KB of child stderr is where a parent draining only stdout hangs"
    );

    // The bytes are not merely harmless, they are kept. A pool that passed
    // `Stdio::null()` would also survive the flood — and would throw away
    // exactly the diagnostics pdfium writes on the malformed input this
    // boundary exists for.
    let written = std::fs::metadata(&log).unwrap().len();
    assert!(
        written >= 256 * 1024,
        "the child's stderr must land in the diagnostics file, got {written} bytes"
    );
}

// --- 2. The hung worker ------------------------------------------------------

#[test]
fn a_hung_worker_is_killed_and_named() {
    let _watchdog = Watchdog::new("hung worker", Duration::from_secs(30));
    let pool = Pool::new(PoolConfig {
        workers: 1,
        batch: 100,
        timeout: Duration::from_millis(200),
        ..config()
    })
    .unwrap();

    let skipped = skip(extract(&pool, "hang:x").unwrap());
    assert_eq!(skipped.failure, Failure::Timeout);

    // `extract` only returns once the child has been reaped, so arriving here
    // at all is the evidence that the kill worked: a failed kill would leave
    // the pool's own `wait` blocking until the worker self-destructs, two
    // minutes past the watchdog.
    assert_eq!(
        pool.live_workers(),
        0,
        "a killed worker is not still counted"
    );
}

// --- 3. What resets the batch counter ---------------------------------------

#[test]
fn the_batch_counter_is_reset_only_by_success() {
    let _watchdog = Watchdog::new("batch counter", Duration::from_secs(30));
    let pool = Pool::new(PoolConfig {
        workers: 1,
        batch: 100,
        ..config()
    })
    .unwrap();

    document(extract(&pool, "ok:one.txt").unwrap());
    let after_first = pool.worker_generation();
    assert_eq!(after_first, 1, "one worker has been started");

    document(extract(&pool, "ok:two.txt").unwrap());
    assert_eq!(
        pool.worker_generation(),
        after_first,
        "a success well under the batch must reuse the same process — \
         that is the whole point of batching"
    );

    let skipped = skip(extract(&pool, "fail:three.txt").unwrap());
    assert_eq!(skipped.failure, Failure::Unreadable);

    document(extract(&pool, "ok:four.txt").unwrap());
    assert_ne!(
        pool.worker_generation(),
        after_first,
        "any error retires the worker, so a bad file never shares a process \
         with the next one"
    );
}

#[test]
fn a_worker_retires_once_its_batch_of_successes_is_spent() {
    let _watchdog = Watchdog::new("batch ceiling", Duration::from_secs(30));
    let pool = Pool::new(PoolConfig {
        workers: 1,
        batch: 2,
        ..config()
    })
    .unwrap();

    document(extract(&pool, "ok:one.txt").unwrap());
    document(extract(&pool, "ok:two.txt").unwrap());
    assert_eq!(
        pool.worker_generation(),
        1,
        "two successes fit in a batch of two"
    );

    document(extract(&pool, "ok:three.txt").unwrap());
    assert_eq!(
        pool.worker_generation(),
        2,
        "the third file must not reuse a process that has spent its batch"
    );
}

// --- 4. The file that killed a worker ---------------------------------------

#[test]
fn a_file_that_killed_a_worker_is_not_retried() {
    let _watchdog = Watchdog::new("no requeue", Duration::from_secs(30));
    let pool = Pool::new(PoolConfig {
        workers: 1,
        batch: 100,
        ..config()
    })
    .unwrap();

    let first = skip(extract(&pool, "crash:malformed.pdf").unwrap());
    assert_eq!(first.failure, Failure::Crash);
    let after_crash = pool.worker_generation();

    let second = skip(extract(&pool, "crash:malformed.pdf").unwrap());
    assert_eq!(
        second, first,
        "the second ask must be answered from the record, verbatim"
    );
    assert_eq!(
        pool.worker_generation(),
        after_crash,
        "and answered without starting a process: otherwise one malformed \
         document loops for as long as the job runs"
    );

    // A different file is not tarred with the same brush.
    document(extract(&pool, "ok:innocent.txt").unwrap());
}

// --- The vocabulary ---------------------------------------------------------

#[test]
fn a_refusal_and_an_unreadable_file_are_named_apart() {
    let _watchdog = Watchdog::new("refusal vs failure", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    let refused = skip(extract(&pool, "refuse:book.epub").unwrap());
    assert_eq!(refused.failure, Failure::Unsupported);
    assert!(
        refused.reason.contains("book.epub"),
        "the worker's own reason must survive into the journal: {}",
        refused.reason
    );

    let failed = skip(extract(&pool, "fail:vanished.txt").unwrap());
    assert_eq!(failed.failure, Failure::Unreadable);
}

/// D51. `every_failure_maps_onto_its_own_skip_rule` proves the *type*
/// `Failure::NotText` maps onto `SkipRule::NotText`; it says nothing about
/// whether the wire string `"not_text"` that a real worker sends is ever
/// parsed into that type in the first place. Frame parsing is strict on
/// purpose (an unknown rule is a protocol error), so a worker that speaks
/// `"not_text"` against a pool that does not recognise the string would fail
/// every such file loudly rather than skip it — this drives a fake worker
/// that actually sends the frame, the same way `a_refusal_under_an_unknown_rule_stops_the_job`
/// drives one that sends a rule nobody knows.
#[test]
fn a_refusal_by_content_crosses_the_wire() {
    let _watchdog = Watchdog::new("not_text refusal", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    let skipped = skip(extract(&pool, "notext:ransom.note").unwrap());
    assert_eq!(skipped.failure, Failure::NotText);
    assert!(
        skipped.reason.contains("ransom.note"),
        "the worker's own reason must survive into the journal: {}",
        skipped.reason
    );
    // The path alone would pass even if the pool fabricated a reason from the
    // request it sent itself — "is not text" is the worker's own wording,
    // absent from the request, so its presence proves the answer came back
    // over the wire rather than being invented locally.
    assert!(
        skipped.reason.contains("is not text"),
        "the worker's own words must survive into the journal: {}",
        skipped.reason
    );
}

/// Both refusals arrive as `Frame::Refused`, and the parent has to tell them
/// apart from the rule string alone — `mnema-extract` may not depend on
/// `mnema-index`, so the wire carries a name and this crate does the mapping.
///
/// It matters downstream rather than here: `mnema-ingest` removes what the
/// index holds under a path when a worker read a file and declined its
/// content, and the ceiling branch never opens the file. Folded together, a
/// lowered `max_bytes` silently deleted indexed content.
#[test]
fn a_file_over_the_ceiling_is_named_apart_from_a_format_with_no_reader() {
    let _watchdog = Watchdog::new("ceiling vs no reader", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    let too_large = skip(extract(&pool, "toobig:archive.zip").unwrap());
    assert_eq!(too_large.failure, Failure::TooLarge);
    assert_ne!(
        too_large.failure,
        Failure::Unsupported,
        "the two refusals must not collapse onto one failure"
    );
    assert_eq!(SkipRule::from(too_large.failure), SkipRule::TooLarge);

    let unsupported = skip(extract(&pool, "refuse:book.epub").unwrap());
    assert_eq!(unsupported.failure, Failure::Unsupported);
}

/// The two rules a reader that can fail *part-way* needs, driven across the
/// wire the way `a_refusal_by_content_crosses_the_wire` drives `"not_text"` —
/// and for a sharper reason than that one had.
///
/// Nothing in this build sends either string yet. The first reader that meets a
/// truncated document or a password-protected one will, and frame parsing is
/// strict: a pool that does not know the word answers `PoolError::Protocol` and
/// **stops the whole job**, on a file that should have been one skipped row.
/// That failure would read as a mismatched worker binary rather than as a
/// missing match arm, which is the most expensive way for this to be wrong.
///
/// `From<Failure>` is asserted here as well as in
/// `every_failure_maps_onto_its_own_skip_rule` because the two answer different
/// questions: that one asks whether every failure has a rule, this one asks
/// whether the *string a worker actually sends* reaches that rule. The mapping
/// can be right and the parse arm missing.
#[test]
fn a_damaged_file_and_a_locked_one_cross_the_wire_apart() {
    let _watchdog = Watchdog::new("malformed and encrypted", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    let damaged = skip(extract(&pool, "damaged:zvit.pdf").unwrap());
    assert_eq!(damaged.failure, Failure::Malformed);
    assert_eq!(SkipRule::from(damaged.failure), SkipRule::Malformed);
    // The worker's own wording, absent from the request this pool sent, so its
    // presence proves the answer came back over the wire rather than being
    // invented locally — the same guard `a_refusal_by_content_crosses_the_wire`
    // uses, and for the same reason.
    assert!(
        damaged.reason.contains("ends mid-object"),
        "the worker's own words must survive into the journal: {}",
        damaged.reason
    );

    let locked = skip(extract(&pool, "locked:vidomist.pdf").unwrap());
    assert_eq!(locked.failure, Failure::Encrypted);
    assert_eq!(SkipRule::from(locked.failure), SkipRule::Encrypted);
    assert!(
        locked.reason.contains("password-protected"),
        "the worker's own words must survive into the journal: {}",
        locked.reason
    );

    // Both directions of the split, because a single-sided assertion here is
    // satisfied by an arm that maps every unknown-ish rule onto one failure.
    assert_ne!(
        damaged.failure, locked.failure,
        "damage and a password must not collapse onto one failure — the journal \
         is the only place a user can be told which of the two their file is"
    );
}

#[test]
fn a_pool_with_no_workers_and_a_batch_of_none_are_both_refused_at_construction() {
    // Not pedantry about zero. A pool with no slots would block its first
    // `extract` forever, waiting for a permit that nobody can ever return —
    // a silent hang inside the one call a job cannot do without, and the exact
    // failure this crate exists to prevent. A batch of zero would retire every
    // worker before its first file, turning batching into its opposite.
    assert!(matches!(
        Pool::new(PoolConfig {
            workers: 0,
            ..config()
        }),
        Err(PoolError::Config(_))
    ));
    assert!(matches!(
        Pool::new(PoolConfig {
            batch: 0,
            ..config()
        }),
        Err(PoolError::Config(_))
    ));
}

/// The wire string a scanned PDF is refused under reaches its rule.
///
/// **This is the arm whose absence would have stopped a walk on the first
/// scanned PDF in a folder.** `SkipRule::NoTextLayer` has existed since the
/// skeleton — declared, judged `is_about_content`, given a `displaces` answer —
/// and nothing sent the string, so nothing needed the parse arm. Frame parsing
/// is strict by design: an unknown rule is `PoolError::Protocol` and stops the
/// whole job, reading as "this worker binary is from another release" rather
/// than as a missing match arm. The first PDF reader is what makes it
/// reachable, and this is the test that says it arrived.
///
/// Both directions, in the shape `a_damaged_file_and_a_locked_one_cross_the_wire_apart`
/// established: the rule must be `NoTextLayer` **and** must not be whichever
/// neighbour a careless arm would fold it into. `Unsupported` is the near miss
/// — both are refusals about a format — and they are opposite promises: one
/// says a reader is coming, this one says the reader came and there is no text.
#[test]
fn a_scanned_pdf_crosses_the_wire_as_its_own_rule() {
    let _watchdog = Watchdog::new("no text layer", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    let scanned = skip(extract(&pool, "scanned:dohovir.pdf").unwrap());
    assert_eq!(scanned.failure, Failure::NoTextLayer);
    assert_eq!(SkipRule::from(scanned.failure), SkipRule::NoTextLayer);
    assert_ne!(
        scanned.failure,
        Failure::Unsupported,
        "a reader that ran and found no text is not a format waiting for a reader"
    );
    // The worker's own wording, absent from the request this pool sent, so its
    // presence proves the answer came back over the wire rather than being
    // invented locally.
    assert!(
        scanned.reason.contains("carries a text layer"),
        "the worker's own words must survive into the journal: {}",
        scanned.reason
    );
}

/// What this file expects one failure to be journalled as, written here rather
/// than read out of `impl From<Failure> for SkipRule` — a test that asks the
/// code under test what it does and agrees is not a test.
///
/// Exhaustive, so a variant added to `Failure` stops this file **compiling**
/// until someone writes down what the journal should record it as.
fn journalled_as(failure: Failure) -> SkipRule {
    match failure {
        Failure::Crash => SkipRule::Crash,
        Failure::Timeout => SkipRule::Timeout,
        Failure::Memory => SkipRule::Memory,
        Failure::Unsupported => SkipRule::Unsupported,
        Failure::Unreadable => SkipRule::Unreadable,
        Failure::TooLarge => SkipRule::TooLarge,
        Failure::NotText => SkipRule::NotText,
        // The pair the whole D51 cycle turns on: refused like `NotText` and
        // journalled unlike it, because `mnema_ingest::displaces` reads the
        // rule and one of the two deletes a document.
        Failure::BinaryTail => SkipRule::BinaryTail,
        // Two rules that behave alike in `displaces` and must still not share a
        // journal row: "this file is broken" and "this file is locked" are the
        // same instruction to the index and different instructions to the
        // person holding the file.
        Failure::Malformed => SkipRule::Malformed,
        Failure::Encrypted => SkipRule::Encrypted,
        // The rule that existed for a whole skeleton with no way to arrive:
        // `SkipRule::NoTextLayer` was declared, judged and given a `displaces`
        // answer before anything could send the string.
        Failure::NoTextLayer => SkipRule::NoTextLayer,
    }
}

/// **This test used to carry its own list, and the list did not work.**
///
/// It was seven pairs written out, under a comment saying they were "written
/// out rather than derived, so that a future variant added to either enum has
/// to face this list". `Failure::BinaryTail` was the first variant added after
/// that comment and it never faced the list — the list stayed seven long, and
/// a reviewer measured what that cost: mapping `Failure::BinaryTail` onto
/// `SkipRule::NotText` left every test in this crate green, and reddened only
/// `mnema-ingest/tests/slice.rs` — a crate away from the line that owns the
/// mapping, and only because that crate happens to ingest such a file.
///
/// Two halves, and neither is sufficient alone: `journalled_as` is exhaustive,
/// so a new variant cannot compile without a decision, and the loop runs over
/// `Failure::every`, so the decision is actually asserted.
#[test]
fn every_failure_maps_onto_its_own_skip_rule() {
    let mut checked = 0;
    // A skip is only useful if the journal can group by it later, so the
    // mapping must be injective as well as correct.
    let mut seen: Vec<SkipRule> = Vec::new();
    for failure in Failure::every() {
        let rule: SkipRule = failure.into();
        assert_eq!(
            rule,
            journalled_as(failure),
            "{failure:?} maps to the wrong rule"
        );
        assert!(
            !seen.contains(&rule),
            "{failure:?} shares {:?} with an earlier failure",
            rule.as_str()
        );
        seen.push(rule);
        checked += 1;
    }
    // The loop above is vacuously true over an empty enumeration, and an
    // emptied `every` would satisfy every assertion in it. A lower bound rather
    // than an equality: the generated list cannot fall short of the enum, so
    // what is left to guard is `every` itself — and a bound does that without
    // becoming a literal someone has to remember to bump.
    assert!(
        checked >= 8,
        "`Failure::every` yielded only {checked} variants"
    );
}

#[test]
fn a_path_that_is_not_utf8_is_recorded_rather_than_mangled() {
    let _watchdog = Watchdog::new("non-utf8 path", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    // The request carries the path as JSON, so a path the platform allows but
    // JSON cannot express has to be refused honestly. `Path::display()` would
    // have replaced the bad byte and asked the worker to read a file that does
    // not exist.
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStrExt;
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/\xff\xfeinvalid.txt"))
    };
    #[cfg(windows)]
    let path = {
        use std::os::windows::ffi::OsStringExt;
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&[
            0x2f, 0xD800, 0x2e, 0x74, 0x78, 0x74,
        ]))
    };

    let skipped = skip(pool.extract(&path).unwrap());
    assert_eq!(skipped.failure, Failure::Unreadable);
    assert_eq!(
        pool.worker_generation(),
        0,
        "and decided before a process was started"
    );
}

#[test]
fn a_worker_speaking_a_foreign_protocol_stops_the_job_rather_than_skipping_the_file() {
    let _watchdog = Watchdog::new("foreign protocol", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    // A line that is not a frame means the worker binary does not match this
    // pool — a partial install, say. Recording ten thousand files as "crashed"
    // would be the worst available answer; stopping with a nameable error is
    // the right one.
    let error = extract(&pool, "garbage:x").unwrap_err();
    assert!(
        matches!(error, PoolError::Protocol { .. }),
        "expected a protocol error, got {error:?}"
    );
}

/// A worker that promises three pages and sends one does not speak this
/// pool's protocol, and that costs the job rather than the file.
///
/// The scenario is a worker binary from another release — the same one
/// `a_worker_speaking_a_foreign_protocol_stops_the_job_rather_than_skipping_the_file`
/// worries about, one layer up from a line that is not a frame. Every frame
/// here is well-formed and the summary arrives; without the count the file
/// would be indexed with its later pages simply absent and nothing recording
/// that they were expected, which is a wrong answer written into the index
/// rather than a failure.
///
/// It is **not** a truncation check, though it is easy to read as one: a
/// worker killed part-way through loses its summary too and never reaches the
/// comparison.
#[test]
fn a_page_count_that_disagrees_with_the_frames_stops_the_job() {
    let _watchdog = Watchdog::new("short page count", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    let error = extract(&pool, "short-count:x").unwrap_err();
    let PoolError::Protocol { detail, .. } = &error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert!(
        detail.contains('3') && detail.contains('1'),
        "the detail must name both counts, or nobody can tell which side lied: {detail}"
    );
}

/// A page reported as read **and** as skipped stops the job, and a page
/// reported only as skipped goes through.
///
/// The summary is the one frame carrying both lists, and this is the only
/// place they are ever side by side: `Document` hands the parent a vector of
/// pages and a vector of numbers, and the parent writes a journal row for
/// every number without being able to ask whether the page is also in the
/// index. So the contradiction has to be caught here or not at all, and what
/// it costs is the skip window telling someone page 1 of their contract is
/// missing while a search cites it.
///
/// **Both directions, and the second is not decoration.** A pool that refused
/// every summary carrying numbers at all would satisfy the first assertion and
/// stop every walk over a folder with one scanned page in it — which is the
/// same outcome `SkipRule::NoTextLayer`'s missing parse arm had.
#[test]
fn a_page_that_arrived_and_was_reported_skipped_stops_the_job() {
    let _watchdog = Watchdog::new("page in both lists", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    let error = extract(&pool, "both-lists:x").unwrap_err();
    let PoolError::Protocol { detail, .. } = &error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert!(
        detail.contains("page 1"),
        "the detail must name the page that was in both lists: {detail}"
    );

    let document = document(extract(&pool, "skipped-page:x").unwrap());
    assert_eq!(
        document.skipped_pages,
        vec![2],
        "the ordinary shape — a gap, and the number that fills it — still \
         reaches the parent"
    );
    assert_eq!(
        document
            .pages
            .iter()
            .map(|page| page.page_no)
            .collect::<Vec<_>>(),
        vec![1, 3],
        "and the pages that did arrive are not disturbed by the check"
    );
}

/// A header whose reader has no name stops the job, and one that names a
/// reader goes through.
///
/// Both halves in one test, because either alone is satisfied by a mistake: a
/// pool that rejected every header would pass the first, and the pool as it
/// stood — which checked nothing — passed the second.
///
/// The gap this closes is narrow and was reachable. Making `reader` a required
/// field on the wire stops a header that *omits* it; it does nothing about
/// `""`, which parses as a valid `String` and travelled all the way into
/// `Document` unexamined. That is the same placeholder the required field was
/// chosen to prevent, arriving by another road — and the column that will hold
/// it is `NOT NULL`, which an empty string satisfies. Its cost is not a bad
/// name in a row: no manifest names the empty reader, so every document from
/// such a worker mismatches for ever and is re-extracted on every run.
#[test]
fn a_header_that_names_no_reader_stops_the_job_and_a_named_one_does_not() {
    let _watchdog = Watchdog::new("nameless reader", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    let error = extract(&pool, "nameless-reader:x").unwrap_err();
    let PoolError::Protocol { detail, .. } = &error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert!(
        detail.contains("reader"),
        "the detail must name what was missing: {detail}"
    );

    // The other direction, through the same pool: an ordinary header still
    // produces a document, and the name it carried is the one that arrives.
    let document = document(extract(&pool, "x").unwrap());
    assert_eq!(document.reader, "text");
    assert_eq!(document.reader_version, 1);
}

/// A block with no page open before it is the older protocol still speaking,
/// and it must not be filed under an invented page 1 — that is precisely the
/// state `IngestError::Unpaginated` used to refuse, now impossible to reach
/// because it is caught one layer earlier.
#[test]
fn a_block_that_belongs_to_no_page_stops_the_job() {
    let _watchdog = Watchdog::new("pageless block", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    let error = extract(&pool, "pageless:x").unwrap_err();
    let PoolError::Protocol { detail, .. } = &error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert!(detail.contains("page"), "{detail}");
}

/// Plain text is one page, and the pool reports it as one page rather than as
/// a bare list of blocks (D37).
#[test]
fn a_readable_document_arrives_as_pages_with_their_blocks() {
    let _watchdog = Watchdog::new("pages arrive", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    let document = document(extract(&pool, "ok:simple.txt").unwrap());
    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].page_no, 1);
    assert_eq!(
        document.pages[0].section_title, None,
        "a text file has no sections to name"
    );
    assert!(!document.pages[0].blocks.is_empty());
}

#[test]
fn a_worker_retired_with_output_still_coming_does_not_wedge_the_pool() {
    let _watchdog = Watchdog::new("retire mid-flood", Duration::from_secs(30));
    let pool = Pool::new(PoolConfig {
        workers: 1,
        batch: 100,
        ..config()
    })
    .unwrap();

    // The pool gives up on the first line and retires the worker while the
    // worker is still writing. Retiring means ending the process and joining
    // the thread that reads its stdout — and that thread is blocked handing
    // over a line nobody will ever take. Getting this order wrong deadlocks
    // the pool inside `Drop`, where no timeout applies and no error is
    // returned: the job simply stops.
    let error = extract(&pool, "garbage-flood:x").unwrap_err();
    assert!(matches!(error, PoolError::Protocol { .. }), "{error:?}");

    // Still usable afterwards, and the slot came back.
    document(extract(&pool, "ok:after.txt").unwrap());
    assert_eq!(pool.live_workers(), 1, "one worker, the fresh one");
}

// --- An idle worker's death costs the file, never the job --------------------

/// Waits until process `pid` has terminated, reaped or not. A child the pool has
/// not waited for stays visible as a zombie, and `kill -0` still succeeds on one,
/// so the state column is what tells the truth.
#[cfg(unix)]
fn wait_until_terminated(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .expect("ps runs");
        let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if state.is_empty() || state.starts_with('Z') {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "worker {pid} never terminated; ps says {state:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn a_worker_that_died_while_idle_costs_the_next_file_nothing() {
    let _watchdog = Watchdog::new("idle worker died", Duration::from_secs(30));
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let pool = Pool::new(PoolConfig {
        workers: 1,
        batch: 100,
        ..config()
    })
    .unwrap();

    document(extract(&pool, &format!("pid:{}", pid_file.display())).unwrap());
    let pid: u32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();

    // The worker is now idle, between documents, and something outside this pool
    // ends it — which is exactly what the out-of-memory killer does on a
    // platform where no ceiling can be imposed, since it chooses by size and not
    // by what a process is doing.
    assert!(
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    wait_until_terminated(pid);

    // One idle worker's death must cost this file nothing at all: not a skip
    // recorded against an innocent document, and certainly not the job. A pool
    // of two workers over forty thousand files would otherwise abort because a
    // process died doing nothing.
    document(extract(&pool, "ok:next.txt").unwrap());
    assert_eq!(
        pool.worker_generation(),
        2,
        "the dead worker was replaced, not written to"
    );
}

#[cfg(unix)]
#[test]
fn a_worker_that_stopped_listening_costs_one_retry_not_the_job() {
    let _watchdog = Watchdog::new("deaf worker", Duration::from_secs(30));
    let pool = Pool::new(PoolConfig {
        workers: 1,
        batch: 100,
        ..config()
    })
    .unwrap();

    // This worker answers and then remains alive with its request pipe closed,
    // so asking whether it has exited says "no" and the write fails anyway. The
    // request provably never reached a parser, which is what makes one retry
    // safe: the no-requeue rule protects a file that killed a worker, and this
    // file was never read at all.
    document(extract(&pool, "deaf:first.txt").unwrap());
    document(extract(&pool, "ok:second.txt").unwrap());
    assert_eq!(
        pool.worker_generation(),
        2,
        "the second file was retried on a fresh worker, not failed"
    );
}

// --- Output that is not text is this file's problem, not the job's -----------

#[test]
fn output_that_is_not_text_skips_the_file_and_keeps_the_job() {
    let _watchdog = Watchdog::new("raw bytes on stdout", Duration::from_secs(30));
    let pool = Pool::new(PoolConfig {
        workers: 1,
        batch: 100,
        timeout: Duration::from_secs(5),
        ..config()
    })
    .unwrap();

    // A C++ library that writes bytes to stdout instead of stderr is the reason
    // this process boundary exists. Calling that a mismatch between binaries
    // would accuse the wrong thing and stop a job of forty thousand files over
    // one document.
    let skipped = skip(extract(&pool, "raw-bytes:page.pdf").unwrap());
    assert_eq!(skipped.failure, Failure::Crash);
    assert!(
        skipped.reason.contains("could not be read"),
        "the reason must say what the wait status cannot: {}",
        skipped.reason
    );

    document(extract(&pool, "ok:next.txt").unwrap());
}

// --- The worker count is real, not decorative -------------------------------

#[test]
fn the_worker_count_bounds_how_many_processes_run_at_once() {
    let _watchdog = Watchdog::new("worker cap", Duration::from_secs(60));
    let hold = tempfile::tempdir().unwrap();
    let release = hold.path().join("release");
    let request = format!("gate:{}", release.display());

    let pool = Arc::new(
        Pool::new(PoolConfig {
            workers: 2,
            batch: 100,
            timeout: Duration::from_secs(45),
            ..config()
        })
        .unwrap(),
    );

    // Three files, two permitted workers: the third must wait for a process,
    // not get one of its own.
    let start = Arc::new(Barrier::new(4));
    let mut threads = Vec::new();
    for _ in 0..3 {
        let pool = Arc::clone(&pool);
        let request = request.clone();
        let start = Arc::clone(&start);
        threads.push(std::thread::spawn(move || {
            start.wait();
            document(pool.extract(Path::new(&request)).unwrap());
        }));
    }
    start.wait();

    // Both permitted workers reach the gate — so the cap is not a mutex in
    // disguise, two processes really do run at once.
    let deadline = Instant::now() + Duration::from_secs(30);
    while pool.live_workers() < 2 {
        assert!(Instant::now() < deadline, "two workers never started");
        std::thread::sleep(Duration::from_millis(10));
    }

    // And the third stays queued while they are held. The gated workers cannot
    // finish until this test says so, so a third process appearing here could
    // only mean the cap was not enforced.
    let watch_until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < watch_until {
        assert!(
            pool.worker_generation() <= 2,
            "a third process was started for a pool of two workers"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    std::fs::write(&release, b"go").unwrap();
    for thread in threads {
        thread.join().unwrap();
    }
}

// --- The memory ceiling, and the platforms that have none -------------------

#[cfg(target_os = "linux")]
#[test]
fn a_child_that_blows_the_memory_ceiling_dies_and_a_child_under_it_does_not() {
    let _watchdog = Watchdog::new("memory ceiling", Duration::from_secs(60));
    // One ceiling, two demands, opposite outcomes: that is what separates "the
    // ceiling stopped it" from "the helper would have died anyway".
    let ceiling = 512 << 20;
    let pool = Pool::new(PoolConfig {
        workers: 1,
        batch: 100,
        memory_limit: Some(ceiling),
        timeout: Duration::from_secs(45),
        ..config()
    })
    .unwrap();

    let under = document(extract(&pool, "hog:64").unwrap());
    assert!(
        !blocks_of(&under).is_empty(),
        "64 MiB is well under the ceiling"
    );

    let over = skip(extract(&pool, "hog:1024").unwrap());
    assert_ne!(
        over.failure,
        Failure::Timeout,
        "the ceiling must stop it, not the deadline: {}",
        over.reason
    );
}

#[cfg(not(target_os = "linux"))]
#[test]
fn a_memory_ceiling_this_platform_cannot_impose_is_refused_rather_than_faked() {
    // The zip bomb this ceiling exists for is no less real on macOS, and no
    // language-level guard closes it. Accepting the number and quietly not
    // applying it would make a platform gap look like a feature, so `Pool::new`
    // refuses it and the caller has to decide knowingly.
    let error = Pool::new(PoolConfig {
        memory_limit: Some(512 << 20),
        ..config()
    })
    .unwrap_err();
    match error {
        PoolError::MemoryCeilingUnavailable { reason } => {
            assert!(!reason.is_empty(), "the refusal has to say what is missing")
        }
        other => panic!("expected the ceiling to be refused, got {other:?}"),
    }
    assert!(matches!(memory_ceiling(), MemoryCeiling::Unavailable(_)));
}

#[cfg(target_os = "macos")]
#[test]
fn this_macos_still_refuses_an_address_space_rlimit() {
    // Measured 2026-07-26 on Darwin 25.5.0/arm64: setrlimit(RLIMIT_AS) fails
    // with EINVAL, and `ulimit -v` agrees. The pool's Linux-only ceiling rests
    // on that, so the fact is pinned here rather than trusted to a comment: if
    // a future macOS starts honouring the call, this test goes red and the
    // ceiling can be switched on for a platform that has one.
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut command = Command::new("/usr/bin/true");
    command.stdout(Stdio::null()).stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: 512 << 20,
                rlim_max: 512 << 20,
            };
            if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let error = command
        .status()
        .expect_err("macOS is expected to reject an address-space limit");
    assert_eq!(
        error.raw_os_error(),
        Some(libc::EINVAL),
        "expected EINVAL from setrlimit(RLIMIT_AS), got {error:?}"
    );
}

/// A rule this pool does not know stops the job. It must never be guessed at.
///
/// The strictness is load-bearing far outside this crate, which its own
/// comment understates. Every rule this pool *does* know maps onto a
/// `SkipRule`, and `mnema-ingest` removes what the index holds under a path for
/// most of them — conditionally on the digest for `Unsupported`, `NotText`,
/// `Malformed` and `Encrypted`, and on the size and time on disk for
/// `TooLarge`. So a worker from another release refusing under a name this
/// build has never seen would — if this arm guessed `Unsupported` — delete the
/// indexed content of every file it named, returning `Ok` each time and
/// stopping nothing. `crates/mnema-ingest/tests/slice.rs` asserts the other
/// half, that the index is untouched.
///
/// **The stand-in rule used to be `"encrypted"`, and it stopped being unknown.**
/// The task that added `SkipRule::Encrypted` made this test fail, and the
/// failure was not subtle: `extract` returns `Ok` for a rule the pool now
/// recognises, so `unwrap_err()` below panics before `detail` is read at all.
/// Nothing about the assertion on the message saved it, and no weaker form of
/// this test would have gone quiet either — the sibling in
/// `mnema-ingest/tests/slice.rs` does not name the rule and reddened just the
/// same, because it too asserts an `Err`.
///
/// What that leaves is a lesson about what a red *says* rather than whether one
/// happens. "called `Result::unwrap_err()` on an `Ok` value" reads as this pool
/// having stopped rejecting unknown rules — a defect in the code under test —
/// when what actually happened is that the test's own premise expired. An
/// unasserted premise fails in the voice of the thing it was holding up, which
/// is why the premise is now asserted first, on its own line.
#[test]
fn a_refusal_under_an_unknown_rule_stops_the_job() {
    let _watchdog = Watchdog::new("unknown rule", Duration::from_secs(30));
    let pool = Pool::new(config()).unwrap();

    // The premise, asserted rather than assumed, and first so that it is what
    // fails when it stops holding. The last stand-in expired without a line
    // like this one, and the red that followed accused the pool instead.
    assert_eq!(
        SkipRule::parse("rule_from_a_later_release"),
        None,
        "the stand-in for an unknown rule has become a rule this build knows"
    );
    let error = extract(&pool, "newrule:sealed.docx").unwrap_err();
    match &error {
        PoolError::Protocol { detail, .. } => assert!(
            detail.contains("rule_from_a_later_release"),
            "the error must name the rule nobody recognised: {detail}"
        ),
        other => panic!("expected a protocol error, got {other:?}"),
    }

    // The worker that spoke it is retired, and the pool is still usable — a
    // mismatch is the job's problem, not this pool's permanent state.
    document(extract(&pool, "ok:after.txt").unwrap());
}
