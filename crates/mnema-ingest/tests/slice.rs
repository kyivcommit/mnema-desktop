//! The slice, end to end: a file on disk becoming a citation a person can
//! read, through the real worker process, the real supervisor, the real
//! chunker and the real database.
//!
//! Nothing here is mocked. The point of the task is to find out which of the
//! pieces built before it were wrong about each other, and a stand-in for any
//! one of them is a way of not finding out.
//!
//! Every fixture is invented — names, places and numbers that belong to
//! nobody.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mnema_core::Coordinate;
use mnema_core::manifest::{Manifest, ReaderId};
#[cfg(unix)]
use mnema_core::wire::{Frame, to_line};
#[cfg(unix)]
use mnema_core::{Block, BlockType, SourceKind};
use mnema_index::{Db, SkipRule, open, register_vector_extension};
use mnema_ingest::{Ingested, ingest_file};
use mnema_pool::{Pool, PoolConfig};
use sha2::{Digest, Sha256};

// `worker` and `wrong_worker` live in `tests/support/mod.rs` now, shared with
// `walk.rs`: two integration-test binaries asking "where is the worker built"
// used to mean two answers that could drift apart, which is exactly the
// divergence that module's own doc comment is written to prevent.
mod support;

// -------------------------------------------------------------------- fixture

/// A watched root with an index beside it, and a pool over the real worker.
struct Fixture {
    db: Db,
    pool: Pool,
    root_id: i64,
    root: PathBuf,
    index_path: PathBuf,
    /// What the real worker says its readers are, asked once here rather than
    /// per call: it costs a process, and every test in this file that does not
    /// name a manifest of its own means "the manifest of the binary that is
    /// about to read the file".
    ///
    /// Taken from `pool` — the real worker — even for the calls that hand a
    /// *different* executable to `ingest_with`. That is the shape those tests
    /// already model: a parent that knows what the product's readers are, and a
    /// sidecar answering in their place.
    manifest: Manifest,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self::with_max_bytes(tempfile::tempdir().unwrap(), PoolConfig::new("").max_bytes)
    }

    /// A fixture whose pool refuses anything over `max_bytes`.
    ///
    /// The default is 64 MiB, which no fixture in this file is going to reach;
    /// a test about the ceiling has to lower it rather than write a file that
    /// large.
    fn with_max_bytes(dir: tempfile::TempDir, max_bytes: u64) -> Self {
        register_vector_extension().unwrap();
        let root = dir.path().join("watched");
        std::fs::create_dir_all(&root).unwrap();
        let index_path = dir.path().join("index.sqlite");
        let db = open(&index_path).unwrap();
        let root_id = db
            .insert_watched_root(root.to_str().expect("a temp path is UTF-8"))
            .unwrap();
        let pool = Pool::new(PoolConfig {
            workers: 1,
            batch: 100,
            // Well under the two-minute default: a plain text file that takes
            // ten seconds means something is wrong, and a test that fails is
            // better than one that waits.
            timeout: Duration::from_secs(10),
            max_bytes,
            ..PoolConfig::new(support::worker())
        })
        .unwrap();
        let manifest = pool.manifest().unwrap();
        Fixture {
            db,
            pool,
            root_id,
            root,
            index_path,
            manifest,
            _dir: dir,
        }
    }

    /// Writes `bytes` at `relative` inside the watched root and returns where.
    fn place(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// The same, with the modification time set rather than left to the clock.
    ///
    /// Every test whose outcome depends on an mtime uses this. Taking whatever
    /// the wall clock gave makes the assertion depend on where a second
    /// boundary happened to fall between two writes, which is a coin toss
    /// rather than a test.
    fn place_at(&self, relative: &str, bytes: &[u8], at: SystemTime) -> PathBuf {
        let path = self.place(relative, bytes);
        set_mtime(&path, at);
        path
    }

    fn ingest(&self, relative: &str) -> Ingested {
        self.try_ingest(relative)
            .expect("a per-file problem must never stop the job")
    }

    /// For the two tests that are about what happens when the database refuses
    /// a write — everywhere else, an `Err` is a failure of the test's premise
    /// and `ingest` is the right call.
    fn try_ingest(&self, relative: &str) -> Result<Ingested, mnema_ingest::IngestError> {
        self.try_ingest_against(relative, &self.manifest)
    }

    /// The same, against a manifest the caller names — a build with a reader
    /// this one does not have, or without one it does.
    fn try_ingest_against(
        &self,
        relative: &str,
        manifest: &Manifest,
    ) -> Result<Ingested, mnema_ingest::IngestError> {
        let absolute = self.root.join(relative);
        let on_disk = mnema_walk::stat(&absolute);
        ingest_file(
            &self.pool,
            &self.db,
            self.root_id,
            &absolute,
            relative,
            on_disk,
            manifest,
        )
    }

    fn ingest_against(&self, relative: &str, manifest: &Manifest) -> Ingested {
        self.try_ingest_against(relative, manifest)
            .expect("a per-file problem must never stop the job")
    }

    /// The same index, walked by a pool built differently — a lowered ceiling,
    /// a deadline nothing can meet, a sidecar that is not the worker. Each of
    /// those is fixed when a pool is constructed, so they are second pools over
    /// the same database rather than mutations of the fixture's.
    fn ingest_with(&self, relative: &str, config: PoolConfig) -> Ingested {
        self.try_ingest_with(relative, config)
            .expect("a per-file problem must never stop the job")
    }

    fn try_ingest_with(
        &self,
        relative: &str,
        config: PoolConfig,
    ) -> Result<Ingested, mnema_ingest::IngestError> {
        let pool = Pool::new(config).unwrap();
        let absolute = self.root.join(relative);
        let on_disk = mnema_walk::stat(&absolute);
        ingest_file(
            &pool,
            &self.db,
            self.root_id,
            &absolute,
            relative,
            on_disk,
            &self.manifest,
        )
    }

    fn ingest_under_ceiling(&self, relative: &str, max_bytes: u64) -> Ingested {
        self.ingest_with(
            relative,
            PoolConfig {
                max_bytes,
                ..self.config()
            },
        )
    }

    fn ingest_with_timeout(&self, relative: &str, timeout: Duration) -> Ingested {
        self.ingest_with(
            relative,
            PoolConfig {
                timeout,
                ..self.config()
            },
        )
    }

    fn ingest_with_worker(&self, relative: &str, worker: &Path) -> Ingested {
        self.ingest_with(relative, PoolConfig::new(worker))
    }

    fn try_ingest_with_worker(
        &self,
        relative: &str,
        worker: &Path,
    ) -> Result<Ingested, mnema_ingest::IngestError> {
        self.try_ingest_with(relative, PoolConfig::new(worker))
    }

    /// The pool settings every test starts from.
    fn config(&self) -> PoolConfig {
        PoolConfig {
            workers: 1,
            batch: 100,
            timeout: Duration::from_secs(10),
            ..PoolConfig::new(support::worker())
        }
    }

    fn count(&self, sql: &str) -> i64 {
        self.db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// Every chunk of one document, in `ord` order.
    ///
    /// The ids rather than the count: a document cleared and written again has
    /// the same number of chunks and different ids, and it is the ids that
    /// every citation and every embedding row points at.
    fn chunk_ids(&self, document_id: &str) -> Vec<i64> {
        self.db
            .conn()
            .prepare("SELECT id FROM chunk WHERE document_id = ?1 ORDER BY ord")
            .unwrap()
            .query_map([document_id], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// Makes the next write to `table` fail, once the trigger is in place.
    ///
    /// A `BEFORE` trigger that always aborts, which is the only way from
    /// outside to force a database error at a chosen point in a sequence of
    /// writes — the alternative is a fault-injection seam in production code,
    /// which would be a shape the product carries for the tests' sake.
    fn break_writes_to(&self, event: &str, table: &str) {
        self.db
            .conn()
            .execute_batch(&format!(
                "CREATE TRIGGER forced_failure BEFORE {event} ON {table} BEGIN
                     SELECT RAISE(ABORT, 'forced failure');
                 END;"
            ))
            .unwrap();
    }

    fn unbreak_writes(&self) {
        self.db
            .conn()
            .execute_batch("DROP TRIGGER forced_failure")
            .unwrap();
    }
}

/// An invented contract, three paragraphs over six lines. The blank lines are
/// what make three blocks out of it, and their positions are what
/// `line_numbers_survive_the_round_trip` checks against.
const CONTRACT: &str = "\
Договір № 17 з постачання

Сторона А — Равелла Комерц, місто Тернопіль.
Сторона Б — Гайворон Логістика.

Оплата протягом тридцяти днів.
";

/// The modification time every fixture whose mtime matters is written with.
///
/// A fixed instant, not `SystemTime::now()`: an assertion about whether the
/// index noticed a change must rest on values the test chose. The nanoseconds
/// are not decoration — they are what a writer storing whole seconds would
/// throw away.
///
/// A function rather than a `const`, because `SystemTime` arithmetic is not
/// const.
fn mtime() -> SystemTime {
    UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789)
}

/// A quarter of a second after [`mtime`], which is a difference whole seconds
/// cannot represent and every filesystem with sub-second timestamps can.
fn mtime_just_after() -> SystemTime {
    mtime() + Duration::from_millis(250)
}

fn set_mtime(path: &Path, at: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// Replaces a file's contents and then its modification time, in that order —
/// writing the bytes is itself what would otherwise move the mtime.
fn set_bytes_and_mtime(path: &Path, bytes: &[u8], at: SystemTime) {
    std::fs::write(path, bytes).unwrap();
    set_mtime(path, at);
}

/// The characters of `s` from `start`, as a string. Every offset that reaches
/// the database is a character offset; `&str` is indexed by bytes, and mixing
/// the two is a defect that only shows itself on non-ASCII text.
fn chars_from(s: &str, start: u32) -> String {
    s.chars().skip(start as usize).collect()
}

/// Every chunk of the index as `(source_kind, text)`, in `ord` order.
///
/// `source_kind` is per chunk and may differ from its document's (D41), so a
/// test about it has to read the column rather than infer it from the file.
fn chunk_kinds(fx: &Fixture) -> Vec<(String, String)> {
    fx.db
        .conn()
        .prepare("SELECT source_kind, text FROM chunk ORDER BY ord")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn char_slice(s: &str, start: u32, end: u32) -> String {
    s.chars()
        .skip(start as usize)
        .take((end - start) as usize)
        .collect()
}

/// The digest of some bytes, in the hex a worker's header carries.
fn sha256_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

// ------------------------------------------------------------------- the slice

/// The whole product in one test: a file goes in, a query finds it, and the
/// citation that comes back can be located character-for-character in the
/// source blocks it claims to quote.
///
/// The span assertion is the load-bearing one. It fails if any offset anywhere
/// in the chain is wrong — the chunker's, the locator's serialisation, the
/// column it is read back from — and it is the one thing that cannot be
/// checked by looking at the text alone, because a citation that quotes the
/// right words from the wrong place still reads correctly.
#[test]
fn a_txt_file_becomes_a_citation_that_can_be_highlighted() {
    let fx = Fixture::new();
    fx.place("contracts/ravella.txt", CONTRACT.as_bytes());

    let outcome = fx.ingest("contracts/ravella.txt");
    let Ingested::Indexed {
        chunks,
        document_id,
    } = &outcome
    else {
        panic!("expected the file to be indexed, got {outcome:?}");
    };
    assert!(*chunks > 0, "a file with text must produce chunks");

    // The two answers a second pass over this folder will ask for: may this be
    // searched, and did the chunking finish. Both are written after the rows
    // land, and neither is checked anywhere else.
    assert_eq!(
        fx.db.document_status(document_id).unwrap(),
        mnema_index::DocumentStatus::Indexed
    );
    assert_eq!(
        fx.db
            .stage_status(document_id, mnema_ingest::STAGE_CHUNK)
            .unwrap()
            .as_deref(),
        Some(mnema_ingest::STATUS_DONE)
    );

    let hits = fx.db.search_lexical("Равелла", 10).unwrap();
    let hit = *hits.first().expect("the indexed word must be findable");
    let citation = fx.db.citation(hit).unwrap().expect("the chunk exists");

    assert!(
        matches!(citation.coordinate, Coordinate::Line { .. }),
        "a .txt has no page number, got {:?}",
        citation.coordinate
    );
    assert_eq!(
        citation.relative_path.as_deref(),
        Some("contracts/ravella.txt")
    );
    assert!(
        !citation.spans.is_empty(),
        "a citation with no spans cannot be highlighted at all"
    );

    for span in &citation.spans {
        let block = fx
            .db
            .block_text(span.block_id)
            .unwrap()
            .expect("every span must name a real block");
        let quoted = char_slice(&citation.text, span.start, span.end);
        assert!(
            chars_from(&block, span.block_start).starts_with(&quoted),
            "block {} from character {} does not begin with the {} characters the \
             citation says came from there.\n  block: {block:?}\n  quoted: {quoted:?}",
            span.block_id,
            span.block_start,
            quoted.chars().count(),
        );
    }
}

/// D37 in the database: a text file is one page, and that page names no
/// section.
///
/// The page row is the level nothing else in this file looks at directly — a
/// citation reads `section_title` through three joins, and reads it as `None`
/// both when the column is NULL and when the join found nothing. Asserted on
/// the row itself so that a reader which started inventing section titles for
/// plain text, or which stopped opening a page at all, is caught here rather
/// than as a citation that merely looks unchanged.
#[test]
fn a_text_file_is_one_page_and_names_no_section() {
    let fx = Fixture::new();
    fx.place("contracts/ravella.txt", CONTRACT.as_bytes());
    fx.ingest("contracts/ravella.txt");

    assert_eq!(fx.count("SELECT count(*) FROM page"), 1);
    assert_eq!(fx.count("SELECT page_no FROM page"), 1);
    assert_eq!(
        fx.count("SELECT count(*) FROM page WHERE section_title IS NULL"),
        1,
        "a text file has no sections, and NULL is how that is said"
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM page WHERE text_source = 'native:txt'"),
        1
    );
}

/// D32, end to end and in both directions.
///
/// macOS hands over decomposed filenames and text; a query typed elsewhere is
/// precomposed. Under D29 the lexical arm is the only private way into this
/// index, so a document that cannot be found by its own spelling is not a
/// cosmetic defect.
///
/// The two directions are not one test twice, and the third assertion is not
/// decoration. What each one actually catches was measured, not reasoned:
///
/// * **Direction two** — precomposed document, decomposed query — goes red the
///   moment `prepare_for_search` stops normalising, and nothing else here does.
/// * **The stored spelling** goes red the moment extraction stops normalising,
///   and nothing else here does. It has to be asserted separately because
///   `prepare_for_search` runs over the document's text as well on the way into
///   the search row: with extraction's NFC gone, the *index* is still
///   precomposed and every search still succeeds, while the text the offsets
///   and hashes were taken from — and the text a citation displays — is not.
///   That is the symmetric weakening this file must not fall for.
/// * **Direction one** — decomposed document, precomposed query — is the
///   user-visible guarantee and is satisfied by either of the two above, so it
///   goes red only when both are gone. Kept because it is the property the
///   product promises, and stated here so nobody mistakes it for a sharp
///   detector of either half.
///
/// Two documents rather than one, so a hit cannot be the other direction's
/// document answering.
#[test]
fn a_word_is_found_whichever_way_its_accent_is_spelled() {
    let fx = Fixture::new();

    // "йоржбука", written with и + U+0306 COMBINING BREVE instead of й.
    let decomposed_doc = "Постачальник \u{0438}\u{0306}оржбука підтвердив замовлення.\n";
    // "гайвуртен", written with the precomposed й (U+0439).
    let precomposed_doc = "Замовник гайвуртен очікує відвантаження.\n";

    fx.place("a/decomposed.txt", decomposed_doc.as_bytes());
    fx.place("b/precomposed.txt", precomposed_doc.as_bytes());
    let a = fx.ingest("a/decomposed.txt");
    let b = fx.ingest("b/precomposed.txt");
    assert!(matches!(a, Ingested::Indexed { .. }), "{a:?}");
    assert!(matches!(b, Ingested::Indexed { .. }), "{b:?}");

    // Direction one: the document is decomposed on disk, the query is not.
    let hits = fx.db.search_lexical("йоржбука", 10).unwrap();
    assert_eq!(
        hits.len(),
        1,
        "a decomposed document must answer a precomposed query — this is what \
         normalising at extraction is for"
    );

    // Direction two: the document is precomposed, the query is decomposed.
    let hits = fx
        .db
        .search_lexical("га\u{0438}\u{0306}вуртен", 10)
        .unwrap();
    assert_eq!(
        hits.len(),
        1,
        "a precomposed document must answer a decomposed query — this is what \
         normalising in query preparation is for"
    );

    // And the stored text is the precomposed form, not merely findable by one:
    // offsets and hashes were taken after normalisation, so anything stored
    // decomposed would describe a string nothing downstream sees again.
    let stored: String = fx
        .db
        .conn()
        .query_row(
            "SELECT text FROM block WHERE text LIKE '%оржбука%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        stored.contains("йоржбука"),
        "extraction stored the decomposed spelling: {stored:?}"
    );
}

/// The cheap arm of a multi-hour job, in four phases: what it must skip, and
/// the two ways a file must be seen as changed.
///
/// The observable is never a counter that could move for other reasons — it is
/// the index's own contents. The file's bytes are replaced and its
/// modification time is set to a value this test chose, so a pass that read the
/// file could not fail to be noticed.
///
/// The `path` row credits the reader that really ran, not a constant.
///
/// This is the only place in the workspace where that claim is reachable.
/// `mnema-index`'s tests hand `insert_path` whatever literals they like, so a
/// `repoint` that wrote `"text"` and `1` for every document — the cheapest way
/// to make the two new columns compile — leaves every one of those tests green
/// while the index credits the text reader for work the markdown reader did.
/// The column then matches the manifest for ever, and the file it names is
/// never re-read no matter which reader takes it later. Nothing logs any of it.
///
/// **Two files, not one, and that is the whole design of the test.** An
/// implementation that always answers `"text"` satisfies the first half; one
/// that always answers `"markdown"` satisfies the second; only asserting both
/// rules out both. A single file constrains neither direction.
///
/// The versions are asserted as literals rather than read from
/// `mnema-extract`'s constants, which this crate does not depend on and must
/// not start depending on. A reader version bumped later turns this red, and
/// that is correct: the migration's `DEFAULT 1` is a claim about the same
/// number, and the two going out of step silently is the failure.
#[test]
fn a_path_row_credits_the_reader_the_worker_actually_ran() {
    let fx = Fixture::new();
    fx.place("plain.txt", "Кошторис на ремонт даху.".as_bytes());
    fx.place("notes.md", "# Заголовок\n\nОдин абзац.\n".as_bytes());

    fx.ingest("plain.txt");
    fx.ingest("notes.md");

    let text = fx
        .db
        .path_entry(fx.root_id, "plain.txt")
        .unwrap()
        .expect("the txt file is indexed, so it has a path row");
    assert_eq!(text.reader, "text");
    assert_eq!(text.reader_version, 1);

    let markdown = fx
        .db
        .path_entry(fx.root_id, "notes.md")
        .unwrap()
        .expect("the md file is indexed, so it has a path row");
    assert_eq!(
        markdown.reader, "markdown",
        "the markdown branch of the worker ran, so the row must say so rather \
         than inherit the text reader's name"
    );
    assert_eq!(markdown.reader_version, 1);
}

/// `before`, plus the entry a build that had grown an html reader would carry.
///
/// Built from a measured manifest rather than written out whole, so that the
/// two differ by exactly the one thing the test is about. A pair of literals
/// would also differ in whatever the product changed since they were written,
/// and the assertion would then be about the literals.
fn with_html_reader(before: &Manifest) -> Manifest {
    let mut after = before.clone();
    after
        .by_extension
        .insert("html".to_string(), ReaderId::new("html", 1));
    after
}

/// `before`, with the reader that takes `.md` moved one version on — what
/// bumping `MARKDOWN_READER_VERSION` produces, and nothing else.
fn with_markdown_one_version_on(before: &Manifest) -> Manifest {
    let mut after = before.clone();
    let current = after
        .by_extension
        .get("md")
        .expect("this build gives .md a reader of its own")
        .clone();
    after.by_extension.insert(
        "md".to_string(),
        ReaderId::new(&current.reader, current.version + 1),
    );
    after
}

/// The version half of the comparison, which the name half cannot stand in
/// for.
///
/// A reader whose output changes without its name changing is the ordinary way
/// this moves: the markdown reader learning tables produces different blocks
/// from the same bytes, and every document it already made is a reading no
/// build performs any more. `MARKDOWN_READER_VERSION` is what says so, and a
/// condition that compared only the name would leave every such document
/// exactly as it was — silently, and for the life of the index.
///
/// Both directions, against manifests differing in one integer: this build's
/// own leaves the file alone, and the bumped one does not.
#[test]
fn a_bumped_reader_version_is_a_reader_that_changed_too() {
    let fx = Fixture::new();
    fx.place_at(
        "notes.md",
        "# Заголовок\n\nОдин абзац.\n".as_bytes(),
        mtime(),
    );
    let first = fx.ingest("notes.md");
    assert!(matches!(first, Ingested::Indexed { .. }), "{first:?}");

    let again = fx.ingest("notes.md");
    assert!(
        matches!(again, Ingested::Unchanged { .. }),
        "the reader that made this file is the one this build has: {again:?}"
    );

    let bumped = with_markdown_one_version_on(&fx.manifest);
    let outcome = fx.ingest_against("notes.md", &bumped);
    assert!(
        matches!(outcome, Ingested::AlreadyIndexed { .. }),
        "the same reader at a new version reads the same bytes differently, so \
         the document it already made is worth replacing: {outcome:?}"
    );
}

/// A file is read again when the reader that takes its extension changes hands
/// — and is not, when nothing changed hands.
///
/// `.html` is the case the whole mechanism was built for, and the only one
/// available before a second reader exists. It is indexed today by the text
/// reader, because `identify_plain_text` has no arm for it, so its `path` row
/// says `text@1`. A manifest of reader *versions* alone would compare `text@1`
/// against `text@1` for ever: the day an html reader arrives, not one
/// already-indexed page would be read again, and the index would go on
/// answering out of a reading no part of the build performs any more. Keying
/// the manifest on the extension is what makes that visible, and this is where
/// it is asserted.
///
/// **Both directions, and neither alone is worth anything.** Without the
/// unchanged direction the test is satisfied by an arm that re-reads every file
/// on every walk; without the other, by an arm that never re-reads anything.
/// The two manifests differ by exactly one entry, and the first of them is the
/// real worker's own — measured, not written out here — so the unchanged
/// direction is a claim about this build rather than about a literal that
/// happens to match it.
///
/// The second manifest describes a build this repository does not have yet
/// (task 10 is what produces one), so the re-read it forces is answered by a
/// worker that still reads the file as text. That is all this test needs — its
/// subject is whether the file reaches a worker at all — and what the two
/// disagreeing costs is pinned by the test below.
#[test]
fn a_file_is_reread_when_its_extension_changed_hands() {
    let fx = Fixture::new();
    fx.place_at("notes.html", "<p>Кошторис</p>\n".as_bytes(), mtime());

    // The build that indexed it: no html reader, so the default took the file.
    let before = fx.manifest.clone();
    let first = fx.ingest_against("notes.html", &before);
    assert!(matches!(first, Ingested::Indexed { .. }), "{first:?}");
    let row = fx
        .db
        .path_entry(fx.root_id, "notes.html")
        .unwrap()
        .expect("the file is indexed, so it has a path row");
    assert_eq!(
        (row.reader.as_str(), row.reader_version),
        ("text", 1),
        "the premise of this test is that html is read by the default reader"
    );

    // Nothing moved: not the file, not the manifest.
    let again = fx.ingest_against("notes.html", &before);
    assert!(
        matches!(again, Ingested::Unchanged { .. }),
        "an unchanged file under an unchanged manifest must not cost a worker: {again:?}"
    );

    // The same file, against the manifest of a build that has an html reader.
    let after = with_html_reader(&before);
    let outcome = fx.ingest_against("notes.html", &after);
    assert!(
        !matches!(outcome, Ingested::Unchanged { .. }),
        "an html reader arriving must make the file worth re-reading: {outcome:?}"
    );
    // And a re-read is what it was, rather than some other way of not being
    // `Unchanged`: only the arms past the cheap one can answer `AlreadyIndexed`,
    // and reaching them means the file was handed to a worker.
    assert!(
        matches!(outcome, Ingested::AlreadyIndexed { .. }),
        "the bytes never moved, so a re-read lands back on the document the \
         index already holds: {outcome:?}"
    );
}

/// What a `path` row that neither the manifest nor the worker will ever agree
/// with costs — per walk, for ever.
///
/// The condition compares a stored fact, which reader made this row, against a
/// prediction keyed on the extension. Where the two cannot converge the file is
/// handed to a worker on every walk and the row is rewritten to the value it
/// already had. That is not a loop inside one call; it is one process and one
/// path row per walk, for as long as the disagreement lasts.
///
/// Pinned here rather than left as prose in a comment, so that a change which
/// makes it worse — a document rebuilt, chunk ids moved out from under every
/// citation into them, a skip journalled, the path lost — goes red instead of
/// disappearing into "that file is read a lot". `ingest_file`'s own comment on
/// the condition has why no such row can arise in this build, and what makes
/// one arise later.
#[test]
fn a_reader_no_build_agrees_on_is_re_read_every_pass_and_costs_only_that() {
    let fx = Fixture::new();
    fx.place_at("notes.html", "<p>Кошторис</p>\n".as_bytes(), mtime());

    // A manifest this pool's worker will never satisfy: it promises an html
    // reader, and the binary behind the pool has none.
    let disagreeing = with_html_reader(&fx.manifest);
    let Ingested::Indexed {
        document_id,
        chunks,
    } = fx.ingest_against("notes.html", &disagreeing)
    else {
        panic!("expected the first pass to index the file")
    };

    // A second document, indexed **after** this one, and the whole reason it is
    // here is the `chunk.id` assertion at the end. `chunk.id` is an `INTEGER
    // PRIMARY KEY` with no `AUTOINCREMENT` (`crates/mnema-index/src/
    // schema.sql:149`), so SQLite allocates one past the largest rowid in the
    // table — and over a table holding one document, a rebuild that empties it
    // hands back the very ids it just deleted. Measured, on this table's shape:
    // one document gives `1,2,3` before and `1,2,3` after, while a second
    // document's rows sitting above them give `1,2,3` before and `7,8,9` after.
    // Without this file the assertion is satisfied by the rebuild it names, and
    // the sentence under it describes something it cannot see. Measured here
    // too: this document's chunk ids are `[1]` against a table whose largest is
    // `2`, so a rebuild has to allocate past it.
    //
    // After, not before: indexed first, this document's chunks are still the
    // top of the table and a rebuild reuses their ids anyway.
    fx.place_at("other.txt", CONTRACT.as_bytes(), mtime());
    assert!(matches!(fx.ingest("other.txt"), Ingested::Indexed { .. }));

    let ids = fx.chunk_ids(&document_id);
    assert_eq!(ids.len(), chunks, "every chunk written has a row");

    for pass in 1..=3 {
        let outcome = fx.ingest_against("notes.html", &disagreeing);
        let Ingested::AlreadyIndexed { document_id: again } = &outcome else {
            panic!("pass {pass}: expected the file to be read again, got {outcome:?}")
        };
        assert_eq!(
            again, &document_id,
            "pass {pass}: the bytes never moved, so it is the same document"
        );
    }

    // …and that is the whole bill. Nothing was rebuilt, nothing was journalled,
    // and the path still names the document every citation into it names.
    //
    // Two documents: this file, and the one indexed above it to give the
    // `chunk.id` comparison something to be sensitive to.
    assert_eq!(fx.count("SELECT count(*) FROM document"), 2);
    assert_eq!(
        fx.chunk_ids(&document_id),
        ids,
        "the document was rebuilt under new chunk ids, which invalidates every \
         citation into it"
    );
    assert_eq!(
        fx.db.skips_for_root(fx.root_id).unwrap().len(),
        0,
        "a file that was read successfully is not a skip"
    );
    assert_eq!(
        fx.db
            .path_entry(fx.root_id, "notes.html")
            .unwrap()
            .expect("the path row survives being rewritten")
            .document_id,
        document_id
    );
}

/// The one-off this condition costs the first index it meets, and the proof
/// that it is a one-off.
///
/// Migration 3 credits every `path` row already on disk to `text@1`. It could
/// not do better — nothing was recorded about who made them — but the markdown
/// reader shipped in `fb3a924`, so on the first walk after the upgrade every
/// markdown row disagrees with the manifest and is handed to a worker. That is
/// the migration's bill, it is paid once, and this is where "once" is asserted
/// rather than assumed: a mistake that made the reader condition compare
/// something derived instead of something stored would re-read the same file on
/// every walk from then on, and only the third pass here would notice.
///
/// The row is rewritten to what the migration leaves behind rather than an
/// upgrade being staged, because the shape is the whole content of the case —
/// a real document, made by the markdown reader, under a row that credits the
/// text one.
#[test]
fn a_row_the_migration_credited_to_text_is_read_again_once_and_then_settles() {
    let fx = Fixture::new();
    fx.place_at(
        "notes.md",
        "# Заголовок\n\nОдин абзац.\n".as_bytes(),
        mtime(),
    );
    let first = fx.ingest("notes.md");
    assert!(matches!(first, Ingested::Indexed { .. }), "{first:?}");

    // What an index migrated from before Task 3 looks like: every row credited
    // to the text reader, because the column had nothing else to be filled with.
    fx.db
        .conn()
        .execute("UPDATE path SET reader = 'text', reader_version = 1", [])
        .unwrap();

    let second = fx.ingest("notes.md");
    assert!(
        matches!(second, Ingested::AlreadyIndexed { .. }),
        "a row crediting a reader the manifest does not give .md must be read \
         again: {second:?}"
    );
    let row = fx
        .db
        .path_entry(fx.root_id, "notes.md")
        .unwrap()
        .expect("the path row survives the re-read");
    assert_eq!(
        (row.reader.as_str(), row.reader_version),
        ("markdown", 1),
        "the re-read is what repairs the row, and it must write the reader that \
         actually ran"
    );

    // And once, not on every walk: nothing about the file or the manifest has
    // moved since, so the third pass must cost nothing.
    let third = fx.ingest("notes.md");
    assert!(
        matches!(third, Ingested::Unchanged { .. }),
        "the migration's re-read is a one-off, not a state the index stays in: \
         {third:?}"
    );
}

/// **Every modification time here is set explicitly**, in both directions, and
/// that is what makes the phases mean anything. Left to the wall clock, phase 3
/// distinguishes a nanosecond mtime from a whole-second one only when a second
/// boundary happens to fall between two writes: measured over six runs of a
/// build with `as_nanos()` mutated to `as_secs()`, the failure appeared three
/// times and not the other three. The gap between [`mtime`] and
/// [`mtime_just_after`] is a quarter of a second — inside one second, so a
/// writer that truncates to seconds cannot tell them apart, and coarse enough
/// for any filesystem with better than whole-second timestamps. On one that has
/// only whole seconds (FAT, HFS+) this test would fail, correctly: the cheap arm
/// really is blind there, and the doc comment on `mtime_nanos` says so.
#[test]
fn an_unchanged_file_is_not_read_a_second_time() {
    let fx = Fixture::new();
    let relative = "contracts/ravella.txt";

    // Phase 1: a file nobody has seen before.
    let path = fx.place_at(relative, CONTRACT.as_bytes(), mtime());
    let first = fx.ingest(relative);
    let Ingested::Indexed { document_id, .. } = first else {
        panic!("expected the first pass to index, got {first:?}");
    };

    // Phase 2: the bytes change, the size and the modification time do not.
    // "Равелла" and "Мурашка" are both seven Cyrillic characters, so the file
    // is the same length; the mtime is put back to the value phase 1 recorded.
    // Nothing the cheap arm can see has moved, so nothing may be read.
    let replaced = CONTRACT.replace("Равелла", "Мурашка");
    assert_eq!(
        replaced.len(),
        CONTRACT.len(),
        "the fixture must not resize"
    );
    set_bytes_and_mtime(&path, replaced.as_bytes(), mtime());

    assert_eq!(
        fx.ingest(relative),
        Ingested::Unchanged {
            document_id: document_id.clone()
        }
    );
    assert!(
        fx.db.search_lexical("Мурашка", 10).unwrap().is_empty(),
        "the new text reached the index, so the file was read after all"
    );
    assert!(
        !fx.db.search_lexical("Равелла", 10).unwrap().is_empty(),
        "the original text left the index, which is a different bug"
    );
    assert_eq!(fx.count("SELECT count(*) FROM document"), 1);

    // Phase 3: the same bytes, and only the modification time moves — by a
    // quarter of a second, which is a change a whole-second mtime cannot
    // represent. This is what makes the mtime half of the comparison, and the
    // sub-second resolution under it, load-bearing.
    set_bytes_and_mtime(&path, replaced.as_bytes(), mtime_just_after());
    let third = fx.ingest(relative);
    assert!(
        matches!(third, Ingested::Indexed { .. }),
        "a file of unchanged length whose modification time moved was not re-read: {third:?}"
    );
    assert!(
        !fx.db.search_lexical("Мурашка", 10).unwrap().is_empty(),
        "it was re-read but the new text did not reach the index"
    );
    assert!(
        fx.db.search_lexical("Равелла", 10).unwrap().is_empty(),
        "the old text is still findable, so the re-index wrote beside the old rows \
         instead of replacing them"
    );

    // Phase 4: the other half of the comparison. The length changes and the
    // modification time is put back to what the index recorded a moment ago —
    // which is not a contrived shape at all: `cp -p`, `rsync -a` and every
    // archive restore carry the original mtime onto content of a different
    // size, and the size column is the only thing that catches them.
    let longer = format!("{replaced}\nДодаток: Тарнавка Сервіс, місто Дубно.\n");
    assert_ne!(longer.len(), replaced.len());
    set_bytes_and_mtime(&path, longer.as_bytes(), mtime_just_after());
    let fourth = fx.ingest(relative);
    assert!(
        matches!(fourth, Ingested::Indexed { .. }),
        "a file whose length changed under an unchanged modification time was not \
         re-read — an mtime-preserving copy or restore is invisible: {fourth:?}"
    );
    assert!(
        !fx.db.search_lexical("Тарнавка", 10).unwrap().is_empty(),
        "it was re-read but the added text did not reach the index"
    );
}

/// The window this design leaves open on purpose, and the recovery it owes.
///
/// The rows of a document are written under transactions, and the checkpoint
/// that says "this document is finished" is written after them — so a job
/// killed in between leaves a complete document with no checkpoint. That state
/// must not be mistaken for a finished one (the file would never be
/// re-indexed) and must not be written beside (`UNIQUE(document_id, ord)`
/// collides, and blocks 2..n of a chunk live inside `char_span` where no
/// foreign key reaches them). The whole ladder goes and is rebuilt.
#[test]
fn a_document_whose_checkpoint_never_landed_is_rebuilt_rather_than_duplicated() {
    let fx = Fixture::new();
    fx.place("contracts/ravella.txt", CONTRACT.as_bytes());
    let Ingested::Indexed {
        document_id,
        chunks,
    } = fx.ingest("contracts/ravella.txt")
    else {
        panic!("expected the first pass to index")
    };

    // Exactly what a `kill -9` between step 4 and step 5 leaves behind, and
    // nothing more: the rows are all committed, the checkpoint was never
    // written, and **the path row is still there and still matches the disk**.
    // An earlier version of this test deleted the path row as well, which made
    // it pass over a cheap arm that answered `Unchanged` to this state on every
    // future walk — the test stepped around the defect it is named for. If the
    // stage check comes out of the cheap arm, this goes red.
    fx.db
        .conn()
        .execute_batch("DELETE FROM ingest_stage")
        .unwrap();
    fx.db
        .set_document_status(&document_id, mnema_index::DocumentStatus::Pending)
        .unwrap();

    let again = fx.ingest("contracts/ravella.txt");
    assert_eq!(
        again,
        Ingested::Indexed {
            document_id: document_id.clone(),
            chunks
        },
        "an unfinished document must be rebuilt, not treated as done and not \
         written beside"
    );
    assert_eq!(fx.count("SELECT count(*) FROM document"), 1);
    assert_eq!(fx.count("SELECT count(*) FROM page"), 1);
    assert_eq!(fx.count("SELECT count(*) FROM chunk"), chunks as i64);
    assert_eq!(
        fx.count("SELECT count(*) FROM chunk_search"),
        chunks as i64,
        "the rebuilt chunks must be findable, not merely stored"
    );
    // And the state that caused all this is gone, so the next walk over this
    // folder can take the cheap arm again.
    assert_eq!(
        fx.db
            .stage_status(&document_id, mnema_ingest::STAGE_CHUNK)
            .unwrap()
            .as_deref(),
        Some(mnema_ingest::STATUS_DONE)
    );
    assert_eq!(
        fx.db.document_status(&document_id).unwrap(),
        mnema_index::DocumentStatus::Indexed
    );
}

/// Content addressing: two copies of one file are one document (D33). A second
/// document row would double the embedding spend and cite the same text twice.
#[test]
fn the_same_content_under_two_paths_is_one_document() {
    let fx = Fixture::new();
    let original = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    fx.place_at("backup/ravella-copy.txt", CONTRACT.as_bytes(), mtime());

    let first = fx.ingest("contracts/ravella.txt");
    let Ingested::Indexed { document_id, .. } = first else {
        panic!("expected the first pass to index, got {first:?}");
    };
    let second = fx.ingest("backup/ravella-copy.txt");
    assert_eq!(
        second,
        Ingested::AlreadyIndexed {
            document_id: document_id.clone()
        },
        "the second copy must not be extracted or chunked again"
    );

    assert_eq!(fx.count("SELECT count(*) FROM document"), 1);
    assert_eq!(fx.db.path_count(&document_id).unwrap(), 2);
    assert_eq!(
        fx.count("SELECT count(*) FROM page"),
        1,
        "a second set of pages means the document was written twice"
    );

    // And the other half of D33, which is what makes the cleanup on the write
    // path safe: editing one copy must not take the other copy's document with
    // it. A document lives as long as some path names it.
    //
    // The edit keeps the length, so the modification time is the only thing
    // that says it happened, and it is set rather than left to the clock — see
    // `an_unchanged_file_is_not_read_a_second_time` for why a wall-clock write
    // makes this phase a coin toss.
    let edited = CONTRACT.replace("Равелла", "Мурашка");
    set_bytes_and_mtime(&original, edited.as_bytes(), mtime_just_after());
    let after_edit = fx.ingest("contracts/ravella.txt");
    assert!(
        matches!(after_edit, Ingested::Indexed { .. }),
        "{after_edit:?}"
    );

    assert_eq!(fx.count("SELECT count(*) FROM document"), 2);
    assert_eq!(
        fx.db.path_count(&document_id).unwrap(),
        1,
        "the untouched copy still names the original document"
    );
    assert!(
        !fx.db.search_lexical("Равелла", 10).unwrap().is_empty(),
        "the original document was deleted even though a copy of it is still on disk"
    );
    assert!(!fx.db.search_lexical("Мурашка", 10).unwrap().is_empty());
}

/// A folder nobody curated is full of formats this product cannot read. Each
/// one is a row in the journal and nothing else — not an error, and above all
/// not a document row, which would make an unread file searchable as empty.
#[test]
fn a_file_the_worker_refuses_is_recorded_and_the_walk_continues() {
    let fx = Fixture::new();
    // A PDF header is enough: typing decides by content, and there is no PDF
    // reader yet, so the worker refuses the file as unsupported.
    fx.place("scans/tender.pdf", b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n");
    fx.place("contracts/ravella.txt", CONTRACT.as_bytes());

    let refused = fx.ingest("scans/tender.pdf");
    assert_eq!(
        refused,
        Ingested::Skipped {
            rule: SkipRule::Unsupported
        }
    );

    let skips = fx.db.skips_for_root(fx.root_id).unwrap();
    assert_eq!(skips.len(), 1);
    assert_eq!(skips[0].relative_path, "scans/tender.pdf");
    assert_eq!(skips[0].rule, "unsupported");
    // The worker's own sentence, not one this crate invented. It names the
    // format rather than the file — `Frame::Refused` for an unsupported reader
    // does not interpolate the path, unlike the size-ceiling refusal beside it
    // (`crates/mnema-extract/src/bin/worker.rs:98-104,149-156`) — and the row's
    // own `relative_path` column is what says which file it was.
    assert!(
        skips[0].reason.contains("application/pdf"),
        "the worker's own reason must survive into the journal: {}",
        skips[0].reason
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM document"),
        0,
        "a file that was never read must not exist as a document"
    );

    // …and the walk continues.
    assert!(matches!(
        fx.ingest("contracts/ravella.txt"),
        Ingested::Indexed { .. }
    ));
}

/// A file replaced by content no reader can take must stop answering under its
/// own name.
///
/// The worst citation this product can produce is not the orphan a file edit
/// used to leave — that one cited no path at all, so a reader could tell
/// something was wrong. It is this: text the file has not contained since,
/// cited under a filename that exists, offering a highlight over characters
/// that are gone. Under D29 the lexical arm is the only private way into this
/// index, so a stale answer there is not cosmetic.
#[test]
fn a_file_replaced_by_content_no_reader_can_take_stops_answering() {
    let fx = Fixture::new();
    let path = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the text file to index")
    };
    assert!(!fx.db.search_lexical("Равелла", 10).unwrap().is_empty());

    // The user saves a PDF over it. The worker reads the bytes and declines
    // them: there is no PDF reader yet.
    set_bytes_and_mtime(
        &path,
        b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n",
        mtime_just_after(),
    );
    assert_eq!(
        fx.ingest("contracts/ravella.txt"),
        Ingested::Skipped {
            rule: SkipRule::Unsupported
        }
    );

    assert!(
        fx.db.search_lexical("Равелла", 10).unwrap().is_empty(),
        "the previous version of the file is still findable, and a citation for it \
         would name a file whose text is gone"
    );
    assert_eq!(
        fx.db.path_count(&document_id).unwrap(),
        0,
        "the path row still names a document whose content is not what is on disk"
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM document"),
        0,
        "nothing names that document any more, so it must not have survived"
    );
    // The skip is still recorded — displacing is in addition to the journal
    // entry, not instead of it.
    assert_eq!(fx.db.skips_for_root(fx.root_id).unwrap().len(), 1);
}

/// …and the asymmetry: a file that could not be opened at all must **not**
/// displace what the index holds.
///
/// The risk runs one way. A network share that drops mid-walk, a folder whose
/// permissions change, an external disk unplugged — every file on it reports
/// `Unreadable`, and displacing on that would empty the index for the whole
/// volume over a condition that is very often temporary. Deleting a file is the
/// watched-folder spec's business, and it knows the difference because it walks
/// the directory instead of being handed one name.
#[test]
fn a_file_that_cannot_be_opened_leaves_the_index_alone() {
    let fx = Fixture::new();
    let path = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the text file to index")
    };

    // The file goes away — a volume that is no longer mounted, seen through the
    // one path the walk had recorded.
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        fx.ingest("contracts/ravella.txt"),
        Ingested::Skipped {
            rule: SkipRule::Unreadable
        }
    );

    assert!(
        !fx.db.search_lexical("Равелла", 10).unwrap().is_empty(),
        "an index that empties itself because a volume was unplugged is worse than \
         a stale answer"
    );
    assert_eq!(fx.db.path_count(&document_id).unwrap(), 1);
    assert_eq!(fx.count("SELECT count(*) FROM document"), 1);
}

/// Line numbers are the coordinate of every text format, and until this task
/// `insert_block` dropped them: a `Coordinate::Line` was true in memory and
/// gone from the database. Checked against the fixture's real line numbers,
/// not against whatever the writer happened to store.
#[test]
fn line_numbers_survive_the_round_trip() {
    let fx = Fixture::new();
    fx.place("contracts/ravella.txt", CONTRACT.as_bytes());
    fx.ingest("contracts/ravella.txt");

    let mut stmt = fx
        .db
        .conn()
        .prepare("SELECT line_start, line_end FROM block ORDER BY reading_order")
        .unwrap();
    let lines: Vec<(Option<i64>, Option<i64>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(
        lines,
        vec![(Some(1), Some(1)), (Some(3), Some(4)), (Some(6), Some(6)),],
        "the three paragraphs of the fixture sit on lines 1, 3–4 and 6"
    );

    // And the coordinate the citation carries is inside that range rather than
    // some other numbering that happens to look plausible.
    let hit = fx.db.search_lexical("Равелла", 10).unwrap()[0];
    let citation = fx.db.citation(hit).unwrap().unwrap();
    let Coordinate::Line { start, end } = citation.coordinate else {
        panic!("expected a line coordinate, got {:?}", citation.coordinate);
    };
    assert!(
        (1..=6).contains(&start) && (1..=6).contains(&end) && start <= end,
        "lines {start}–{end} are not inside the six the fixture has"
    );
}

/// The indexing job holds its own connection, not the one behind the window's
/// mutex, so a write in flight does not stop the user searching.
///
/// No wall-clock assertion: a bound tight enough to prove "fast" is a bound a
/// loaded machine will miss. The writer here does not commit until the reader
/// has answered, so a read that blocks blocks forever, and the generous
/// timeout is a way of failing instead of hanging rather than a measurement.
///
/// **What this test pins is SQLite, not this product.** It opens the second
/// connection by hand, so no change to `mnema-ingest`, `mnema-index` or
/// `src-tauri` can redden it — it would go red only if WAL or the busy timeout
/// stopped behaving as `crates/mnema-index/src/open.rs:114-115` assumes, which
/// is a dependency bump away and worth knowing about. The product fact — that
/// the shell hands a job its own connection rather than the window's — is
/// `src-tauri/tests/commands.rs`'s
/// `the_indexing_job_is_given_its_own_connection_not_the_windows`, which has a
/// mutation behind both of its halves.
///
/// The assertion here was checked for vacuity rather than assumed: probed
/// directly, a read on the **writer's own** connection during the open
/// transaction returns 2 where this separate one returns 1.
#[test]
fn search_is_not_blocked_while_a_document_indexes() {
    let fx = Fixture::new();
    fx.place("contracts/ravella.txt", CONTRACT.as_bytes());
    fx.ingest("contracts/ravella.txt");

    // The window's connection: a second `Db` on the same file, exactly as the
    // shell's `AppState` holds one while a job runs on another.
    let window = open(&fx.index_path).unwrap();
    let roots_before: i64 = window
        .conn()
        .query_row("SELECT count(*) FROM watched_root", [], |r| r.get(0))
        .unwrap();

    // A write in flight on the job's connection, holding the write lock.
    fx.db.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
    fx.db.insert_watched_root("/Volumes/Second").unwrap();

    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let hits = window.search_lexical("Равелла", 10).unwrap();
        let roots: i64 = window
            .conn()
            .query_row("SELECT count(*) FROM watched_root", [], |r| r.get(0))
            .unwrap();
        tx.send((hits.len(), roots)).unwrap();
    });

    let (hits, roots_during) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the search never came back while a write was in flight");
    fx.db.conn().execute_batch("COMMIT").unwrap();
    reader.join().unwrap();

    assert_eq!(hits, 1, "the reader answered, but with the wrong rows");
    assert_eq!(
        roots_during, roots_before,
        "the reader saw the writer's uncommitted row, so the two share one \
         connection and the isolation this test claims to check does not exist"
    );
}

/// The same check as above, over a document long enough that the chunker has
/// to split it — which is the only way `Segment::block_start` is ever anything
/// but zero.
///
/// It matters because that field is this product's answer to the one thing the
/// server cannot do: locate a quote it is showing. The server searches the
/// block for the quote's text and gives up silently on zero or several hits
/// (`app/index/highlight.py:50-57`). Here the offset is carried, so it can be
/// wrong — and a wrong offset is invisible in the citation's own text, which
/// reads perfectly either way. Every chunk of the document is checked, not the
/// first: the first chunk of anything starts at zero and proves nothing.
#[test]
fn a_chunk_that_starts_inside_a_block_still_locates_itself() {
    let fx = Fixture::new();

    // One paragraph, no blank lines, so it is a single block — and long enough
    // that it cannot be one chunk (the ceiling is 1850 characters). Ukrainian
    // throughout: a byte-offset implementation passes this test written in
    // ASCII and fails it here.
    let mut long = String::new();
    for n in 1..=40 {
        long.push_str(&format!(
            "Пункт {n}. Сторона зобовʼязується передати партію товару у строк, \
             погоджений сторонами. "
        ));
    }
    long.push('\n');
    fx.place("contracts/long.txt", long.as_bytes());
    let outcome = fx.ingest("contracts/long.txt");
    let Ingested::Indexed {
        document_id,
        chunks,
    } = outcome
    else {
        panic!("expected the long file to be indexed")
    };
    assert!(
        chunks > 1,
        "the fixture is not long enough to be split, so nothing here is tested"
    );

    let mut stmt = fx
        .db
        .conn()
        .prepare("SELECT id FROM chunk WHERE document_id = ?1 ORDER BY ord")
        .unwrap();
    let ids: Vec<i64> = stmt
        .query_map([&document_id], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    let mut saw_an_inner_start = false;
    for id in ids {
        let citation = fx.db.citation(id).unwrap().expect("the chunk exists");
        for span in &citation.spans {
            let block = fx
                .db
                .block_text(span.block_id)
                .unwrap()
                .expect("every span must name a real block");
            let quoted = char_slice(&citation.text, span.start, span.end);
            assert!(
                chars_from(&block, span.block_start).starts_with(&quoted),
                "chunk {id}: block {} from character {} does not begin with what the \
                 citation says came from there.\n  quoted: {quoted:?}",
                span.block_id,
                span.block_start,
            );
            saw_an_inner_start |= span.block_start > 0;
        }
    }
    assert!(
        saw_an_inner_start,
        "every span still started at the beginning of its block, so the offset \
         arithmetic this test exists for was never exercised"
    );
}

// ------------------------------- rebuilding a document without losing its copies

/// Rebuilding one copy of a file must not take the other copy out of the index.
///
/// The recovery in step 3 used to delete the `document` row, and
/// `path.document_id` is `ON DELETE CASCADE` (`schema.sql:82`) — so every other
/// path naming that document went with it, to come back only when a walk
/// reached that copy again, a whole pass later if the walk had already gone by.
/// The fix is to clear the content and leave the document row standing: its id
/// is the sha256 of the bytes just re-read, so it is not the stale part.
///
/// This became reachable on every crashed document the moment the cheap arm
/// started consulting the checkpoint; before that, the arm answered `Unchanged`
/// and the rebuild never ran.
#[test]
fn rebuilding_a_document_leaves_its_other_copies_in_place() {
    let fx = Fixture::new();
    fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    fx.place_at("backup/ravella-copy.txt", CONTRACT.as_bytes(), mtime());

    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the first copy to index")
    };
    fx.ingest("backup/ravella-copy.txt");
    assert_eq!(fx.db.path_count(&document_id).unwrap(), 2);

    // A job killed between the rows and the checkpoint, and then a walk that
    // reaches the first copy.
    fx.db
        .conn()
        .execute_batch("DELETE FROM ingest_stage")
        .unwrap();
    assert!(matches!(
        fx.ingest("contracts/ravella.txt"),
        Ingested::Indexed { .. }
    ));

    assert_eq!(
        fx.db.path_count(&document_id).unwrap(),
        2,
        "rebuilding one copy dropped the other's path row — it would be missing \
         from the index until a walk reached it again"
    );
    let mut stmt = fx
        .db
        .conn()
        .prepare("SELECT relative_path FROM path ORDER BY relative_path")
        .unwrap();
    let paths: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        paths,
        vec!["backup/ravella-copy.txt", "contracts/ravella.txt"]
    );
    // And the rebuild really rebuilt: one page, not two, and still findable.
    assert_eq!(fx.count("SELECT count(*) FROM page"), 1);
    assert!(!fx.db.search_lexical("Равелла", 10).unwrap().is_empty());
}

/// The recovery must be atomic, because the thing it recovers from is a job
/// that stopped half-way.
///
/// Clearing the old content and writing the new happen in one transaction, so
/// a second interruption during the rebuild leaves the document exactly as it
/// was rather than empty. Before the fix the clear was its own statement
/// outside any transaction, and an interruption between it and slice 0's
/// commit left the document gone *and* both path rows gone — the crash
/// recovery losing data on a crash.
#[test]
fn a_rebuild_interrupted_half_way_leaves_the_document_as_it_was() {
    let fx = Fixture::new();
    fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    fx.place_at("backup/ravella-copy.txt", CONTRACT.as_bytes(), mtime());
    let Ingested::Indexed {
        document_id,
        chunks,
    } = fx.ingest("contracts/ravella.txt")
    else {
        panic!("expected the first copy to index")
    };
    fx.ingest("backup/ravella-copy.txt");
    let pages_before = fx.count("SELECT count(*) FROM page");
    let blocks_before = fx.count("SELECT count(*) FROM block");

    fx.db
        .conn()
        .execute_batch("DELETE FROM ingest_stage")
        .unwrap();

    // The rebuild gets as far as writing chunks and then the database refuses.
    // Chunks are written after the clear and after the page and block rows, so
    // this is a failure squarely in the middle of the recovery.
    fx.break_writes_to("INSERT", "chunk");
    let outcome = fx.try_ingest("contracts/ravella.txt");
    assert!(
        matches!(outcome, Err(mnema_ingest::IngestError::Index(_))),
        "expected the write to fail, got {outcome:?}"
    );
    fx.unbreak_writes();

    // Nothing was lost. Not the document, not either path, not the content
    // that was there before the attempt.
    assert!(fx.db.document_exists(&document_id).unwrap());
    assert_eq!(fx.db.path_count(&document_id).unwrap(), 2);
    assert_eq!(fx.count("SELECT count(*) FROM page"), pages_before);
    assert_eq!(fx.count("SELECT count(*) FROM block"), blocks_before);
    assert_eq!(fx.count("SELECT count(*) FROM chunk"), chunks as i64);
    assert!(
        !fx.db.search_lexical("Равелла", 10).unwrap().is_empty(),
        "the document was emptied by a rebuild that never finished"
    );

    // …and the next walk still repairs it.
    assert!(matches!(
        fx.ingest("contracts/ravella.txt"),
        Ingested::Indexed { .. }
    ));
    assert_eq!(
        fx.db
            .stage_status(&document_id, mnema_ingest::STAGE_CHUNK)
            .unwrap()
            .as_deref(),
        Some(mnema_ingest::STATUS_DONE)
    );
}

// --------------------------- the journal and the displacement are one fact

/// A skip that could not be carried out must not be journalled as though it
/// had been.
///
/// The two used to be separate transactions. Forced apart, what was left was a
/// journal row saying the file had been skipped as unsupported while the
/// lexical index went on answering with its old text under the same filename —
/// the exact citation the displacement exists to prevent, now with an entry
/// asserting it had been dealt with. One transaction makes them one fact.
#[test]
fn a_skip_that_could_not_displace_is_not_journalled_either() {
    let fx = Fixture::new();
    let path = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    fx.ingest("contracts/ravella.txt");

    set_bytes_and_mtime(
        &path,
        b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n",
        mtime_just_after(),
    );
    // The displacement's own delete is what fails here.
    fx.break_writes_to("DELETE", "document");
    let outcome = fx.try_ingest("contracts/ravella.txt");
    assert!(
        matches!(outcome, Err(mnema_ingest::IngestError::Index(_))),
        "expected the displacement to fail, got {outcome:?}"
    );
    fx.unbreak_writes();

    assert_eq!(
        fx.db.skips_for_root(fx.root_id).unwrap().len(),
        0,
        "the journal says this file was skipped while the index still answers \
         with its old text — the two must be one transaction"
    );
    // The index is untouched, which is the consistent state: nothing happened,
    // and the next walk can try again.
    assert!(!fx.db.search_lexical("Равелла", 10).unwrap().is_empty());
    assert_eq!(fx.count("SELECT count(*) FROM document"), 1);

    let retried = fx.ingest("contracts/ravella.txt");
    assert_eq!(
        retried,
        Ingested::Skipped {
            rule: SkipRule::Unsupported
        }
    );
    assert_eq!(fx.db.skips_for_root(fx.root_id).unwrap().len(), 1);
    assert!(fx.db.search_lexical("Равелла", 10).unwrap().is_empty());
}

// ------------------------------- what the ceiling decides from the two numbers it has
//
// It used to say "a setting must not delete indexed content", and the tests below
// now falsify that as written: lowering the ceiling deletes nothing by itself, but
// after it, anything that moves the file's mtime does — `a_touch_under_a_lowered_
// ceiling_gives_up_the_document` is that price, taken deliberately. The refusal is
// made from `stat` without opening the file, so "touched" and "rewritten in place at
// the same length" are the same two numbers, and one of the two answers has to be
// wrong. The side chosen is the one that leaves no citation pointing at text the file
// no longer holds.

/// A file that **grew** past the ceiling loses what the index held for it.
///
/// This was once the only case the ceiling branch was thought to meet on a
/// previously indexed file, and it took two rulings to find. The obvious worry
/// — someone lowers `max_bytes` under a file that has not changed — cannot
/// reach here at all: nothing about that file moved, so the cheap arm matches
/// size, mtime and stage and answers `Unchanged` before a worker starts. What
/// does reach here is a file rewritten to something bigger, and keeping the old
/// text for that one is the stale citation the displacement exists to prevent.
///
/// Two more cases reach it, and they have their own tests below: a file
/// rewritten in place at the **same** length under a lowered ceiling, and a
/// bare `touch` under one. The refusal cannot tell any of them apart — it is
/// made from `stat`, without opening the file — so the decision is made from
/// the two numbers there are: `displaces` displaces when the size **or** the
/// mtime moved, and only a file matching on both is treated as one the ceiling
/// merely excludes.
#[test]
fn a_file_grown_past_the_ceiling_loses_what_the_index_held() {
    let fx = Fixture::with_max_bytes(tempfile::tempdir().unwrap(), 400);
    let path = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    assert!(
        CONTRACT.len() < 400,
        "the fixture must start under the ceiling, or the first pass proves nothing"
    );
    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the file to index while it was under the ceiling")
    };

    let grown = format!("{CONTRACT}{}", "Додаток. ".repeat(60));
    assert!(grown.len() > 400);
    set_bytes_and_mtime(&path, grown.as_bytes(), mtime_just_after());

    assert_eq!(
        fx.ingest("contracts/ravella.txt"),
        Ingested::Skipped {
            rule: SkipRule::TooLarge
        },
        "the ceiling must have a rule of its own, so the journal can answer \
         'which files were too large?'"
    );
    assert!(
        fx.db.search_lexical("Равелла", 10).unwrap().is_empty(),
        "the index still answers with text this file no longer contains"
    );
    assert_eq!(fx.db.path_count(&document_id).unwrap(), 0);
    assert_eq!(fx.count("SELECT count(*) FROM document"), 0);
    assert_eq!(
        fx.db.skips_for_root(fx.root_id).unwrap()[0].rule,
        "too_large"
    );
}

/// The premise the whole ceiling rule rests on: the user who lowers
/// `max_bytes` under a file they have not touched never reaches `displaces` at
/// all.
///
/// Size, mtime and stage all still match, so the cheap arm answers and no
/// worker is started, however low the ceiling goes. It is asserted on its own
/// rather than as the first half of another test because everything
/// `displaces` says about this rule is written on top of it: if this ever stops
/// holding, that reasoning needs revisiting and not just its code.
#[test]
fn a_lowered_ceiling_is_not_even_asked_about_an_untouched_file() {
    let fx = Fixture::new();
    fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the file to index under the default ceiling")
    };

    assert_eq!(
        fx.ingest_under_ceiling("contracts/ravella.txt", (CONTRACT.len() - 1) as u64),
        Ingested::Unchanged {
            document_id: document_id.clone()
        },
        "a lowered ceiling under an untouched file must not even be asked about"
    );
    assert!(!fx.db.search_lexical("Равелла", 10).unwrap().is_empty());
    assert_eq!(fx.db.path_count(&document_id).unwrap(), 1);
}

/// A file rewritten in place, keeping its length, under a ceiling that has
/// dropped below it — the Critical a whole-branch review found, and the case
/// the size comparison alone was argued to make impossible.
///
/// The argument ran: a same-length replacement cannot fool the size test,
/// because a file of that length that is over the ceiling now was over the
/// ceiling then and could never have been indexed. It refutes itself, and the
/// sequence below is what that looks like: the ceiling is what moved, so the
/// file's own length says nothing at all. Every clause lines up — the cheap arm
/// misses on the modification time, the pool refuses from `stat`, the size
/// matches `path.size_bytes` — and before this test the document stayed.
///
/// Measured against the size-only rule: the old text was still found, the new
/// text never was, and each further pass repeated it, because the worker keeps
/// answering `TooLarge` and the journal is not consulted for it.
#[test]
fn a_file_rewritten_in_place_under_a_lowered_ceiling_stops_answering() {
    let fx = Fixture::new();
    let path = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the file to index under the default ceiling")
    };

    // Different words, deliberately the same number of bytes: the size column
    // is blind to this edit by construction, which is the whole point.
    let rewritten: String = CONTRACT
        .chars()
        .map(|c| if c == 'а' { 'о' } else { c })
        .collect();
    assert_eq!(
        rewritten.len(),
        CONTRACT.len(),
        "the rewrite must keep the file's length, or this is the grown-file test"
    );
    assert_ne!(rewritten, CONTRACT);
    set_bytes_and_mtime(&path, rewritten.as_bytes(), mtime_just_after());

    assert_eq!(
        fx.ingest_under_ceiling("contracts/ravella.txt", (CONTRACT.len() - 1) as u64),
        Ingested::Skipped {
            rule: SkipRule::TooLarge
        }
    );
    assert!(
        fx.db.search_lexical("Равелла", 10).unwrap().is_empty(),
        "the index still answers under this name with text the file no longer contains"
    );
    assert_eq!(fx.db.path_count(&document_id).unwrap(), 0);
    assert_eq!(fx.count("SELECT count(*) FROM document"), 0);
    assert_eq!(
        fx.db.skips_for_root(fx.root_id).unwrap()[0].rule,
        "too_large",
        "and it is journalled, so 'why is this file not in my index?' has an answer"
    );
}

/// The other half of the pair, on its own, so that neither half of the
/// comparison can be deleted without something going red: a replacement of a
/// **different** length carrying the **old** modification time.
///
/// `cp -p`, `rsync -a` and every archive restore do exactly this, and the
/// modification time is blind to them by construction. Without this the size
/// half of the condition was unpinned — the grown-file test above writes a
/// later time as well, so it stays green on the modification time alone.
#[test]
fn a_replacement_of_a_different_length_carrying_the_old_time_stops_answering() {
    let fx = Fixture::new();
    let path = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the file to index under the default ceiling")
    };

    let other = format!("{CONTRACT}{}", "Додаток про кошторис. ".repeat(20));
    assert!(other.len() > CONTRACT.len());
    // The same modification time the index recorded, which is the whole point.
    set_bytes_and_mtime(&path, other.as_bytes(), mtime());

    assert_eq!(
        fx.ingest_under_ceiling("contracts/ravella.txt", (CONTRACT.len() - 1) as u64),
        Ingested::Skipped {
            rule: SkipRule::TooLarge
        }
    );
    assert_eq!(
        fx.db.path_count(&document_id).unwrap(),
        0,
        "a different length is a proof the bytes moved, whatever the clock says"
    );
    assert!(fx.db.search_lexical("Равелла", 10).unwrap().is_empty());
}

/// The price the test above is bought with, asserted rather than left implied:
/// a file whose bytes never moved, only its modification time, loses its
/// document under a ceiling that has dropped below it.
///
/// From here a `touch` and a same-length rewrite in place are the same two
/// numbers. The refusal came from `stat` without the file being opened, so
/// there is no reading of the content to tell them apart — not by omission,
/// by construction — and the rule displaces, taking the loss over the stale
/// citation. Both are undone by the ceiling moving back up, so neither is
/// permanent; while they last, this one is a file missing from the index with a
/// `too_large` row saying why, and the other is a search result quoting text
/// that is not in the file.
///
/// It is written as its own test, with its own name, because it is a decision
/// and not a consequence: whoever reverses it has to delete an assertion that
/// says what reversing it costs.
#[test]
fn a_touch_under_a_lowered_ceiling_gives_up_the_document() {
    let fx = Fixture::new();
    let path = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the file to index under the default ceiling")
    };

    // Byte for byte what it was; only the modification time moved, which is
    // what a `touch`, a restore or a sync client does — and it is what makes
    // this reach the pool at all rather than stopping at the cheap arm.
    set_mtime(&path, mtime_just_after());
    assert_eq!(
        fx.ingest_under_ceiling("contracts/ravella.txt", (CONTRACT.len() - 1) as u64),
        Ingested::Skipped {
            rule: SkipRule::TooLarge
        }
    );
    assert_eq!(
        fx.db.path_count(&document_id).unwrap(),
        0,
        "the pair the product itself reads as 'this file changed' has to mean that here too"
    );
    assert_eq!(
        fx.db.skips_for_root(fx.root_id).unwrap()[0].rule,
        "too_large",
        "and the journal is what makes the loss visible rather than silent"
    );

    // Not permanent, and that is half of why the trade is the way round it is:
    // the same setting moving back brings the document straight back.
    let raised = fx.ingest_under_ceiling("contracts/ravella.txt", 1 << 20);
    assert!(
        matches!(raised, Ingested::Indexed { .. }),
        "raising the ceiling again must bring the file back: {raised:?}"
    );
    assert!(!fx.db.search_lexical("Равелла", 10).unwrap().is_empty());
}

/// The other side of the same line, and the only state in which `displaces`
/// keeps a `TooLarge`: the size **and** the modification time both still match
/// what the `path` row recorded.
///
/// That is the very pair the cheap arm above treats as "nothing happened to
/// this file", so treating it as anything else here would be the same product
/// disagreeing with itself one screen apart. Reaching it takes the third
/// member of the cheap arm's question: an interrupted job leaves a `path` row
/// matching the disk exactly over a document whose chunking nothing recorded as
/// finished, so the arm misses on the *stage* while both numbers agree.
///
/// Without this the keep side would be unasserted, and a rule that displaced
/// every `TooLarge` outright would pass the whole suite — which is the shape
/// this branch has already been caught in nine times: an assertion that binds
/// one direction only is satisfied by zero and looks like coverage.
#[test]
fn a_lowered_ceiling_keeps_what_it_still_recognises() {
    let fx = Fixture::new();
    fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());

    // The checkpoint is what fails, so the rows below it are committed and the
    // document is left without a finished stage — the state a job killed
    // between the last slice and its checkpoint leaves behind.
    fx.break_writes_to("INSERT", "ingest_stage");
    let interrupted = fx.try_ingest("contracts/ravella.txt");
    assert!(
        matches!(interrupted, Err(mnema_ingest::IngestError::Index(_))),
        "expected the checkpoint to fail, got {interrupted:?}"
    );
    fx.unbreak_writes();
    let document_id: String = fx
        .db
        .conn()
        .query_row("SELECT id FROM document", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fx.db.path_count(&document_id).unwrap(), 1);

    // Nothing on disk moved at all. The stage is the only thing that misses,
    // so the pool is asked under a ceiling now below the file's size.
    assert_eq!(
        fx.ingest_under_ceiling("contracts/ravella.txt", (CONTRACT.len() - 1) as u64),
        Ingested::Skipped {
            rule: SkipRule::TooLarge
        }
    );
    assert_eq!(
        fx.db.path_count(&document_id).unwrap(),
        1,
        "a setting deleted indexed content: neither of the two numbers the product \
         reads moved, only the ceiling did"
    );
    assert_eq!(fx.count("SELECT count(*) FROM document"), 1);
    assert_eq!(
        fx.db.skips_for_root(fx.root_id).unwrap()[0].rule,
        "too_large",
        "it is still journalled — kept is not the same as unnoticed"
    );
}

/// The Critical the branch review found: `TooLarge` is a statement about the
/// *setting* `PoolConfig::max_bytes`, not about the file, so a rule fired
/// against it must not survive that setting changing. `Unsupported` and
/// `NoTextLayer` earn the same verdict again from the same bytes forever —
/// that is what makes the second cheap arm (`ingest_file`, right after the
/// `path_entry` check) safe for them. `TooLarge` does not: the very same
/// bytes belong in the index the moment the ceiling is raised past them.
///
/// Measured before the fix, with this exact shape: a file refused under a low
/// ceiling stayed `Skipped { TooLarge }` after the ceiling was raised to
/// comfortably above the file's size, because the second cheap arm answered
/// from the journal without ever asking the pool again.
#[test]
fn a_raised_ceiling_re_examines_a_file_it_used_to_refuse() {
    let fx = Fixture::with_max_bytes(tempfile::tempdir().unwrap(), 100);
    fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    assert!(
        CONTRACT.len() > 100,
        "the fixture must start over the ceiling, or the first pass proves nothing"
    );

    assert_eq!(
        fx.ingest("contracts/ravella.txt"),
        Ingested::Skipped {
            rule: SkipRule::TooLarge
        }
    );

    // Nothing about the file moved, only the setting: same bytes, same
    // mtime — the second cheap arm's exact premise, and the one `TooLarge`
    // must refuse to answer from once the ceiling no longer excludes it.
    let raised = fx.ingest_under_ceiling("contracts/ravella.txt", 1 << 20);
    assert!(
        matches!(raised, Ingested::Indexed { .. }),
        "a raised ceiling did not re-examine a file only the old ceiling had \
         excluded: {raised:?}"
    );
    assert!(!fx.db.search_lexical("Равелла", 10).unwrap().is_empty());
}

// ------------------- a remembered content verdict must not ask the pool

/// A refusal the journal remembers must not answer for a `path` row it would
/// have taken away.
///
/// This is the half of the defect that survives the journal being cleaned up
/// after a successful index, and it needs no stale row at all: the row below is
/// current, correct, and about bytes that are no longer the ones on disk. The
/// sequence is a stricter release refusing a note it still recognises — which
/// keeps the document, since the bytes match what the index was built from —
/// followed by a `cp -p` of a **different** file of the same length carrying
/// that same modification time.
///
/// The `path` row's own pair is older, so the first cheap arm misses; the
/// journal's pair matches the disk exactly, so the second one answered. The
/// worker was never asked, `displaces` was never reached, and the note went on
/// answering under a name whose file says something else entirely. What breaks
/// it is asking `displaces` with no digest — the truth about what this arm
/// knows — before trusting a remembered verdict.
#[cfg(unix)]
#[test]
fn a_remembered_refusal_does_not_answer_for_a_document_it_would_remove() {
    let fx = Fixture::new();
    let first = "Нотатка перша, про терміни постачання.\n".repeat(40);
    let path = fx.place_at("notes/a.txt", first.as_bytes(), mtime());
    let Ingested::Indexed { document_id, .. } = fx.ingest("notes/a.txt") else {
        panic!("expected the note to index")
    };

    // A stricter release refuses these very bytes. The digest matches what the
    // index holds, so the document stays and the journal records the refusal
    // against the file's *current* size and modification time — while the
    // `path` row keeps the older one it was written with.
    set_mtime(&path, mtime_just_after());
    let stricter = support::wrong_worker(
        fx.root.parent().unwrap(),
        &format!(
            r#"printf '{{"frame":"refused","rule":"not_text","reason":"the threshold moved","sha256":"{document_id}"}}\n'"#
        ),
    );
    assert_eq!(
        fx.ingest_with_worker("notes/a.txt", &stricter),
        Ingested::Skipped {
            rule: SkipRule::NotText
        }
    );
    assert!(
        !fx.db.search_lexical("постачання", 10).unwrap().is_empty(),
        "the premise fails if the rule change already deleted the document"
    );

    // `cp -p` from a different note of exactly the same length: the size cannot
    // see it, and the modification time it carries is the one the journal
    // remembers.
    let second = "Нотатка друга, про витрати кошторисів.\n".repeat(40);
    assert_eq!(
        second.len(),
        first.len(),
        "the two notes must be the same length, or the first cheap arm decides this"
    );
    set_bytes_and_mtime(&path, second.as_bytes(), mtime_just_after());

    let outcome = fx.ingest("notes/a.txt");
    assert!(
        matches!(outcome, Ingested::Indexed { .. }),
        "a remembered refusal answered for bytes it was never reached on: {outcome:?}"
    );
    assert!(
        fx.db.search_lexical("постачання", 10).unwrap().is_empty(),
        "the index still answers under this name with a note the file no longer holds"
    );
    assert!(
        !fx.db.search_lexical("кошторисів", 10).unwrap().is_empty(),
        "and the note that IS there has to be findable, or this passes by deleting \
         everything"
    );
}

/// The journal's row goes when the file it refused is indexed — in the write's
/// own transaction, so the two cannot disagree.
///
/// Both directions, because either alone is satisfied by doing nothing: a
/// refusal has to leave a row, and a successful index has to take it away. Left
/// standing, that row is not inert in either of the two places it is read. The
/// window answering "why is this file not in my index?" listed files that are
/// in it; and `ingest_file`'s second cheap arm went on treating it as a live
/// verdict about a path whose content had been replaced twice over.
#[test]
fn indexing_a_file_forgets_the_refusal_that_kept_it_out() {
    let fx = Fixture::new();
    let path = fx.place_at(
        "scans/tender.pdf",
        b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n",
        mtime(),
    );
    assert_eq!(
        fx.ingest("scans/tender.pdf"),
        Ingested::Skipped {
            rule: SkipRule::Unsupported
        }
    );
    assert_eq!(
        fx.db
            .skips_for_root(fx.root_id)
            .unwrap()
            .iter()
            .map(|s| s.relative_path.clone())
            .collect::<Vec<_>>(),
        vec!["scans/tender.pdf".to_string()],
        "a refusal that records nothing leaves the user with no answer at all"
    );

    set_bytes_and_mtime(&path, CONTRACT.as_bytes(), mtime_just_after());
    assert!(matches!(
        fx.ingest("scans/tender.pdf"),
        Ingested::Indexed { .. }
    ));
    assert!(
        fx.db.skips_for_root(fx.root_id).unwrap().is_empty(),
        "the file is in the index and the journal still says why it is not"
    );

    // The same for the branch that writes no document of its own: a second path
    // onto content already chunked repoints and nothing else, and that is
    // exactly where a refusal for the new name would otherwise survive.
    let other = fx.place_at("scans/copy.pdf", b"%PDF-1.7\n<<>>\nendobj\n", mtime());
    assert_eq!(
        fx.ingest("scans/copy.pdf"),
        Ingested::Skipped {
            rule: SkipRule::Unsupported
        }
    );
    set_bytes_and_mtime(&other, CONTRACT.as_bytes(), mtime_just_after());
    assert!(matches!(
        fx.ingest("scans/copy.pdf"),
        Ingested::AlreadyIndexed { .. }
    ));
    assert!(
        fx.db.skips_for_root(fx.root_id).unwrap().is_empty(),
        "`AlreadyIndexed` writes a path row like any other and owes the journal the \
         same tidy-up"
    );
}

/// The second cheap arm exists to save a worker process on a file whose
/// content verdict is already known — pinned here by starving it. The file is
/// skipped once by the real worker as `Unsupported`, then ingested again,
/// unchanged, with a sidecar that is not the worker standing in for the pool.
/// If the second cheap arm answers from the journal, the sidecar is never
/// asked and the rule stays `Unsupported`. Cut the arm and the walk reaches
/// the pool instead, where the sidecar answers every request with bytes that
/// are not valid UTF-8 and the rule becomes `Crash`
/// (`a_worker_that_is_not_the_worker_does_not_empty_the_index` below is the
/// test that first pinned that translation).
#[cfg(unix)]
#[test]
fn a_remembered_content_skip_is_answered_without_asking_the_pool() {
    let fx = Fixture::new();
    fx.place_at(
        "scans/tender.pdf",
        b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n",
        mtime(),
    );
    assert_eq!(
        fx.ingest("scans/tender.pdf"),
        Ingested::Skipped {
            rule: SkipRule::Unsupported
        }
    );

    let broken = support::wrong_worker(fx.root.parent().unwrap(), r"printf '\377\376\n'");
    assert_eq!(
        fx.ingest_with_worker("scans/tender.pdf", &broken),
        Ingested::Skipped {
            rule: SkipRule::Unsupported
        },
        "the rule changed, so the pool was asked — a second cheap arm that \
         answers from the journal would never reach a worker at all, wrong or \
         not"
    );
}

/// The other half of the second cheap arm's premise: a remembered verdict is
/// only good while `format_version` still matches. Bumping
/// `INDEX_FORMAT_VERSION` is how a shipped fix (D51: a photo stopped being
/// `unsupported` and became its own `not_text` rule) reaches files a walk
/// already gave up on — but only if the arm actually checks the version
/// rather than the bytes alone, which nothing here had tested before this.
///
/// Same shape as the test above: starve the pool with a sidecar that is not
/// the worker. The file is skipped once by the real worker as `NotText`, then
/// its journal row is pushed one version behind by hand — same bytes, same
/// mtime, only the format version lags — and ingested again. If the second
/// cheap arm honoured a stale version, the sidecar would never be asked and
/// the rule would stay `NotText`. Cut the check and the walk reaches the pool
/// instead, where the sidecar answers with bytes that are not valid UTF-8 and
/// the rule becomes `Crash`.
#[cfg(unix)]
#[test]
fn a_stale_format_version_is_not_honoured_by_the_second_cheap_arm() {
    let fx = Fixture::new();
    fx.place_at(
        "photos/scan.png",
        include_bytes!("../../mnema-extract/tests/fixtures/solid.png"),
        mtime(),
    );
    assert_eq!(
        fx.ingest("photos/scan.png"),
        Ingested::Skipped {
            rule: SkipRule::NotText
        }
    );

    // Nothing about the file moved — only the remembered version, by hand,
    // to stand in for a walk that ran before today's build. Scoped to this
    // path so a second fixture added to this test later would not go stale
    // along with it, silently.
    fx.db
        .conn()
        .execute(
            "UPDATE skipped SET format_version = format_version - 1
              WHERE watched_root_id = ?1 AND relative_path = ?2",
            (fx.root_id, "photos/scan.png"),
        )
        .unwrap();

    let broken = support::wrong_worker(fx.root.parent().unwrap(), r"printf '\377\376\n'");
    assert_eq!(
        fx.ingest_with_worker("photos/scan.png", &broken),
        Ingested::Skipped {
            rule: SkipRule::Crash
        },
        "the rule did not change, so the journal answered — a second cheap \
         arm that trusts a stale format_version never reaches a worker at \
         all"
    );
}

/// D51 §5. A `.txt` overwritten by a photo must stop answering searches with
/// text the file no longer contains — the question this project asks of
/// everything that writes to the index.
#[test]
fn a_text_file_overwritten_by_a_photo_stops_answering() {
    let fx = Fixture::new();
    // `place_at`, not `place`: this test's outcome depends on the mtime moving
    // between the two writes, and the file's own comment (`slice.rs:89-95`)
    // says taking the wall clock there makes the assertion a coin toss.
    fx.place_at(
        "notes/kropyva.txt",
        "кропива росте попід тином\n".as_bytes(),
        mtime(),
    );
    assert!(matches!(
        fx.ingest("notes/kropyva.txt"),
        Ingested::Indexed { .. }
    ));
    assert!(
        !fx.db.search_lexical("кропива", 10).unwrap().is_empty(),
        "the premise fails if the text was never searchable"
    );

    fx.place_at(
        "notes/kropyva.txt",
        include_bytes!("../../mnema-extract/tests/fixtures/solid.png"),
        mtime_just_after(),
    );
    assert_eq!(
        fx.ingest("notes/kropyva.txt"),
        Ingested::Skipped {
            rule: SkipRule::NotText
        }
    );

    assert!(
        fx.db.search_lexical("кропива", 10).unwrap().is_empty(),
        "the old text still answers for a file that no longer contains it"
    );
}

/// D51. A file whose bytes never changed must not lose its document because a
/// later release classifies it differently. Measured by the data-loss harness
/// before this fix: touching the mtime — `touch`, `cp -p`, a restore from
/// backup, a sync client — is enough, because the first cheap arm compares
/// mtime and hands the file to a worker that now answers differently.
///
/// The sidecar carries the file's real sha256, and that is not a convenience:
/// a worker whose rule changed still *read* the bytes and still hashed them
/// before classifying (`worker.rs`, where the digest is taken before
/// `identify`). A stand-in that omitted it would be modelling a different
/// failure — an older worker — which this test is not about.
#[test]
fn a_file_whose_bytes_did_not_change_keeps_its_document() {
    let fx = Fixture::new();
    let prose = "Нотатка, яку ніхто не редагував.\n".repeat(50);
    fx.place_at("notes/untouched.txt", prose.as_bytes(), mtime());
    assert!(matches!(
        fx.ingest("notes/untouched.txt"),
        Ingested::Indexed { .. }
    ));
    assert!(
        !fx.db.search_lexical("редагував", 10).unwrap().is_empty(),
        "the premise fails if the text was never searchable"
    );

    // The same bytes, a later mtime — and a worker whose rule now refuses them.
    fx.place_at("notes/untouched.txt", prose.as_bytes(), mtime_just_after());
    let mut hasher = Sha256::new();
    hasher.update(prose.as_bytes());
    let sha256 = hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });
    let stricter = support::wrong_worker(
        fx.root.parent().unwrap(),
        &format!(
            r#"printf '{{"frame":"refused","rule":"not_text","reason":"the threshold moved","sha256":"{sha256}"}}\n'"#
        ),
    );
    let outcome = fx.ingest_with_worker("notes/untouched.txt", &stricter);

    assert!(
        matches!(outcome, Ingested::Skipped { .. }),
        "the file is refused under the new rule: {outcome:?}"
    );
    assert!(
        !fx.db.search_lexical("редагував", 10).unwrap().is_empty(),
        "the bytes are identical to what the index was built from, so the \
         document must survive a rule that changed under it"
    );
}

/// D51 §5. The other side of the line that test draws: a note whose append was
/// interrupted must not lose the prose it still has. Its tail is zeroed, so the
/// file is refused — but the earlier document stays searchable, because the
/// text on disk is still mostly the text the index holds.
#[test]
fn an_interrupted_append_does_not_delete_what_the_note_still_says() {
    let fx = Fixture::new();
    let prose = "Нотатка про засідання: ухвалили перенести терміни.\n".repeat(200);
    fx.place_at("notes/meeting.txt", prose.as_bytes(), mtime());
    assert!(matches!(
        fx.ingest("notes/meeting.txt"),
        Ingested::Indexed { .. }
    ));
    assert!(!fx.db.search_lexical("ухвалили", 10).unwrap().is_empty());

    let mut damaged = prose.into_bytes();
    damaged.extend_from_slice(&[0u8; 4096]);
    fx.place_at("notes/meeting.txt", &damaged, mtime_just_after());
    assert_eq!(
        fx.ingest("notes/meeting.txt"),
        Ingested::Skipped {
            rule: SkipRule::BinaryTail
        }
    );

    assert!(
        !fx.db.search_lexical("ухвалили", 10).unwrap().is_empty(),
        "the note's prose is still on disk, and deleting it would lose text \
         that is readable nowhere else"
    );
}

/// D51 §5, in the encoding the randomised harness cannot produce.
///
/// `an_interrupted_append_does_not_delete_what_the_note_still_says` covers a
/// UTF-8 note, and so does the harness — `interrupted_append_body` writes UTF-8
/// prose and nothing else. A UTF-16 note takes a different branch of `classify`
/// altogether, the one behind the byte-order mark, and the whole output of that
/// branch's tail arm was covered by nothing: a reviewer replaced it with
/// `Verdict::NotText` and every test in the workspace stayed green.
///
/// What that costs is a document, and it is the loss this cycle exists to
/// prevent: `NotText` on changed bytes displaces, so a UTF-16 note whose append
/// was interrupted loses the prose that is still on disk in front of the
/// damage.
#[test]
fn an_interrupted_utf16_note_does_not_delete_what_it_still_says() {
    let fx = Fixture::new();

    // A UTF-16LE note with a mark, invented outright: the mark is what tells
    // `classify` that the NUL bytes of every ASCII-range unit are half a code
    // unit rather than corruption.
    let mut note = vec![0xFF, 0xFE];
    for unit in "Протокол наради: розглянуто подання.\n"
        .repeat(60)
        .encode_utf16()
    {
        note.extend_from_slice(&unit.to_le_bytes());
    }
    fx.place_at("notes/utf16.txt", &note, mtime());
    assert!(matches!(
        fx.ingest("notes/utf16.txt"),
        Ingested::Indexed { .. }
    ));
    assert!(
        !fx.db.search_lexical("розглянуто", 10).unwrap().is_empty(),
        "the premise fails if UTF-16 prose was never searchable"
    );

    // The power goes out mid-append and the tail comes back zeroed.
    let mut damaged = note.clone();
    damaged.extend_from_slice(&[0u8; 4096]);
    fx.place_at("notes/utf16.txt", &damaged, mtime_just_after());
    assert_eq!(
        fx.ingest("notes/utf16.txt"),
        Ingested::Skipped {
            rule: SkipRule::BinaryTail
        },
        "a UTF-16 note that stops being text is a tail, not a photo"
    );

    assert!(
        !fx.db.search_lexical("розглянуто", 10).unwrap().is_empty(),
        "the note's prose is still on disk, and deleting it would lose text \
         that is readable nowhere else"
    );
}

/// The same question as `a_text_file_overwritten_by_a_photo_stops_answering`,
/// one rule over: a `.txt` overwritten by a format this product has no reader
/// for must also stop answering under its own name. The bytes are not the ones
/// the index was built from, so what it holds is a file that is gone.
///
/// This direction was never in doubt — `Unsupported` displaced
/// unconditionally. It is here because the condition added beside it has to be
/// pinned from both sides, and a one-sided assertion is satisfied by a rule
/// that never displaces at all.
#[test]
fn a_text_file_overwritten_by_a_format_with_no_reader_stops_answering() {
    let fx = Fixture::new();
    fx.place_at(
        "notes/protokol.txt",
        "засідання ухвалило перенести розгляд\n".as_bytes(),
        mtime(),
    );
    assert!(matches!(
        fx.ingest("notes/protokol.txt"),
        Ingested::Indexed { .. }
    ));
    assert!(
        !fx.db.search_lexical("розгляд", 10).unwrap().is_empty(),
        "the premise fails if the text was never searchable"
    );

    // A PDF header with nothing readable behind it. `identify` answers
    // `Reader::Pdf` on the magic alone, and no reader for that format is built
    // — task 6 shipped plain text and markdown — so the worker refuses the
    // file as `unsupported` after reading and hashing its bytes.
    fx.place_at(
        "notes/protokol.txt",
        b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n",
        mtime_just_after(),
    );
    assert_eq!(
        fx.ingest("notes/protokol.txt"),
        Ingested::Skipped {
            rule: SkipRule::Unsupported
        }
    );

    assert!(
        fx.db.search_lexical("розгляд", 10).unwrap().is_empty(),
        "the old text still answers for a file that no longer contains it"
    );
}

/// The other side of that line, and the inversion it corrects.
///
/// `Unsupported` is the *least* stable verdict this product gives: the worker's
/// own words for it are "no reader implemented yet"
/// (`crates/mnema-extract/src/bin/worker.rs`), which is exactly what a release
/// changes — where `NotText` promises the opposite, that "no release adds a
/// reader that makes them prose". Task 10 made the stable rule conditional on
/// the digest and left the unstable one deleting outright.
///
/// What that costs is one document per file, silently: a folder indexed by a
/// build that has a reader, walked once by a build that does not — a rollback,
/// a second machine, a partial install — loses every document in it, with the
/// bytes never having moved. The sidecar stands in for the build without the
/// reader, and carries the file's real digest because a worker that declines a
/// format still read and hashed the bytes before deciding.
#[test]
fn a_file_no_reader_can_take_keeps_its_document_when_only_the_rule_changed() {
    let fx = Fixture::new();
    let prose = "Довідка про стан робіт, підписана комісією.\n".repeat(50);
    fx.place_at("notes/dovidka.txt", prose.as_bytes(), mtime());
    assert!(matches!(
        fx.ingest("notes/dovidka.txt"),
        Ingested::Indexed { .. }
    ));
    assert!(
        !fx.db.search_lexical("комісією", 10).unwrap().is_empty(),
        "the premise fails if the text was never searchable"
    );

    // The same bytes, a later mtime — and a build that has no reader for them.
    fx.place_at("notes/dovidka.txt", prose.as_bytes(), mtime_just_after());
    let sha256 = sha256_of(prose.as_bytes());
    let without_the_reader = support::wrong_worker(
        fx.root.parent().unwrap(),
        &format!(
            r#"printf '{{"frame":"refused","rule":"unsupported","reason":"no reader implemented yet","sha256":"{sha256}"}}\n'"#
        ),
    );
    let outcome = fx.ingest_with_worker("notes/dovidka.txt", &without_the_reader);

    assert_eq!(
        outcome,
        Ingested::Skipped {
            rule: SkipRule::Unsupported
        },
        "the premise fails unless this build refuses the file for want of a reader"
    );
    assert!(
        !fx.db.search_lexical("комісією", 10).unwrap().is_empty(),
        "the bytes are identical to what the index was built from, so the \
         document must survive a build that lost the reader"
    );
}

/// The state neither `Unsupported` nor `Unreadable` could say: the file **is**
/// the format its magic claims, this product **has** a reader for it, and the
/// reader could not finish — a PDF that ends mid-object, a zip whose central
/// directory does not parse.
///
/// It lands on the same side of `displaces` as `Unsupported`, and for the same
/// argument rather than by analogy. What a reader survives is what a release
/// changes: a folder indexed by a build whose reader recovers from the damage
/// and walked again by one that does not — a rollback, a second machine, a
/// vendored library at a different version — would lose a document per file,
/// with the bytes never having moved. The stand-in worker is that other build,
/// and it carries the file's real digest because a reader that opened a file
/// and gave up part-way through still hashed the bytes it was handed.
///
/// **Both directions, and the second is what makes the first mean anything.**
/// "Does not displace when the digest matches" is satisfied by a rule that
/// never displaces at all — the one-sided shape this file has been caught in
/// before.
#[cfg(unix)]
#[test]
fn a_damaged_file_keeps_its_document_when_the_bytes_did_not_move() {
    let fx = Fixture::new();
    let prose = "Акт приймання робіт, складений у двох примірниках.\n".repeat(50);
    fx.place_at("акти/акт.txt", prose.as_bytes(), mtime());
    assert!(matches!(
        fx.ingest("акти/акт.txt"),
        Ingested::Indexed { .. }
    ));
    assert!(
        !fx.db.search_lexical("примірниках", 10).unwrap().is_empty(),
        "the premise fails if the text was never searchable"
    );

    // The same bytes, a later mtime — and a build whose reader gives up on
    // them.
    fx.place_at("акти/акт.txt", prose.as_bytes(), mtime_just_after());
    let unchanged = sha256_of(prose.as_bytes());
    let gives_up = support::wrong_worker(
        fx.root.parent().unwrap(),
        &format!(
            r#"printf '{{"frame":"refused","rule":"malformed","reason":"the document ends mid-object","sha256":"{unchanged}"}}\n'"#
        ),
    );
    assert_eq!(
        fx.ingest_with_worker("акти/акт.txt", &gives_up),
        Ingested::Skipped {
            rule: SkipRule::Malformed
        },
        "the premise fails unless the wire string arrives as this rule and no other"
    );
    assert!(
        !fx.db.search_lexical("примірниках", 10).unwrap().is_empty(),
        "the bytes are identical to what the index was built from, so a build \
         whose reader cannot finish them must not take the document away"
    );

    // The other direction, which is the whole point of the digest: the same
    // rule over bytes that are *not* the indexed ones is a file replaced by a
    // broken one, and the old text has to stop answering under a name it is no
    // longer in.
    let broken = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog\nendo".as_slice();
    fx.place_at(
        "акти/акт.txt",
        broken,
        mtime_just_after() + Duration::from_millis(250),
    );
    let replaced = sha256_of(broken);
    let gives_up_on_a_replacement = support::wrong_worker(
        fx.root.parent().unwrap(),
        &format!(
            r#"printf '{{"frame":"refused","rule":"malformed","reason":"the document ends mid-object","sha256":"{replaced}"}}\n'"#
        ),
    );
    assert_eq!(
        fx.ingest_with_worker("акти/акт.txt", &gives_up_on_a_replacement),
        Ingested::Skipped {
            rule: SkipRule::Malformed
        }
    );
    assert!(
        fx.db.search_lexical("примірниках", 10).unwrap().is_empty(),
        "the old text still answers for a file that no longer contains it"
    );
}

/// The other half of the pair, and a separate test rather than a second
/// assertion in the one above, because `displaces` gives the two rules
/// separate arms — a mutation of either has to have something of its own to
/// redden.
///
/// A password is not damage. The file is whole and the reader is the right
/// one; what is missing is a key, and the two are different things to the
/// person reading the skip list — one is fixed by supplying a password and the
/// other is not fixed at all. Same conditional displacement, same reason: a
/// build that learns to ask for passwords must not have deleted the documents
/// of every locked file the build before it met.
#[cfg(unix)]
#[test]
fn a_locked_file_keeps_its_document_when_the_bytes_did_not_move() {
    let fx = Fixture::new();
    let prose = "Довідка про доходи, видана на вимогу заявника.\n".repeat(50);
    fx.place_at("довідки/довідка.txt", prose.as_bytes(), mtime());
    assert!(matches!(
        fx.ingest("довідки/довідка.txt"),
        Ingested::Indexed { .. }
    ));
    assert!(
        !fx.db.search_lexical("заявника", 10).unwrap().is_empty(),
        "the premise fails if the text was never searchable"
    );

    fx.place_at("довідки/довідка.txt", prose.as_bytes(), mtime_just_after());
    let unchanged = sha256_of(prose.as_bytes());
    let asks_for_a_password = support::wrong_worker(
        fx.root.parent().unwrap(),
        &format!(
            r#"printf '{{"frame":"refused","rule":"encrypted","reason":"this document is password-protected","sha256":"{unchanged}"}}\n'"#
        ),
    );
    assert_eq!(
        fx.ingest_with_worker("довідки/довідка.txt", &asks_for_a_password),
        Ingested::Skipped {
            rule: SkipRule::Encrypted
        },
        "the premise fails unless the wire string arrives as this rule and no other"
    );
    assert!(
        !fx.db.search_lexical("заявника", 10).unwrap().is_empty(),
        "the bytes are identical to what the index was built from, so a build \
         that cannot open them must not take the document away"
    );

    let locked = b"%PDF-1.7\n1 0 obj\n<< /Encrypt 2 0 R >>\n".as_slice();
    fx.place_at(
        "довідки/довідка.txt",
        locked,
        mtime_just_after() + Duration::from_millis(250),
    );
    let replaced = sha256_of(locked);
    let asks_about_a_replacement = support::wrong_worker(
        fx.root.parent().unwrap(),
        &format!(
            r#"printf '{{"frame":"refused","rule":"encrypted","reason":"this document is password-protected","sha256":"{replaced}"}}\n'"#
        ),
    );
    assert_eq!(
        fx.ingest_with_worker("довідки/довідка.txt", &asks_about_a_replacement),
        Ingested::Skipped {
            rule: SkipRule::Encrypted
        }
    );
    assert!(
        fx.db.search_lexical("заявника", 10).unwrap().is_empty(),
        "the old text still answers for a file that no longer contains it"
    );
}

// ------------------------------------------------- markdown, and its pages

/// An invented handbook: content before the first heading, two sections, and a
/// fence inside the second.
const HANDBOOK: &str = "\
Вступні положення до збірника.

# Постачання обладнання

Комісія розглянула звернення щодо постачання лабораторного обладнання.

## Розрахунок

Формула наведена нижче.

```rust
let userName = \"равелла\";
```
";

/// The whole point of the page marker, in the database: a markdown file is
/// several pages, each named, with its own blocks and its own citations.
///
/// This is `a_txt_file_becomes_a_citation_that_can_be_highlighted`'s assertion
/// over more than one page — which is what makes it a different test rather
/// than a longer fixture. Every block of a multi-page document used to be
/// unrepresentable: the wire could not say which page a block belonged to, and
/// `ingest_file` refused the document rather than putting them all on page 1.
#[test]
fn a_markdown_file_reaches_the_database_as_several_pages() {
    let fx = Fixture::new();
    fx.place("довідники/збірник.md", HANDBOOK.as_bytes());

    let outcome = fx.ingest("довідники/збірник.md");
    let Ingested::Indexed { chunks, .. } = &outcome else {
        panic!("expected the file to be indexed, got {outcome:?}");
    };
    assert!(*chunks > 0);

    assert_eq!(fx.count("SELECT count(*) FROM page"), 3);
    assert_eq!(
        fx.count("SELECT count(*) FROM page WHERE text_source = 'native:md'"),
        3,
        "the page records which reader produced its text"
    );
    let titles: Vec<Option<String>> = fx
        .db
        .conn()
        .prepare("SELECT section_title FROM page ORDER BY page_no")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        titles,
        vec![
            None,
            Some("Постачання обладнання".to_string()),
            Some("Розрахунок".to_string()),
        ]
    );

    // Blocks land on the page they were sent under, not all on the first one.
    assert_eq!(
        fx.count(
            "SELECT count(*) FROM block b JOIN page p ON p.id = b.page_id WHERE p.page_no = 1"
        ),
        1,
        "only the run of text before the first heading"
    );
    assert!(
        fx.count(
            "SELECT count(*) FROM block b JOIN page p ON p.id = b.page_id \
             WHERE p.page_no = 3 AND b.type = 'code'"
        ) == 1,
        "the fence belongs to the section it sits under"
    );

    // …and a citation from a later page reads back with its own section name
    // and spans that locate themselves in the blocks they claim.
    let hits = fx.db.search_lexical("формула", 10).unwrap();
    let hit = *hits.first().expect("the indexed word must be findable");
    let citation = fx.db.citation(hit).unwrap().expect("the chunk exists");
    assert_eq!(citation.section_title.as_deref(), Some("Розрахунок"));
    assert_eq!(
        citation.relative_path.as_deref(),
        Some("довідники/збірник.md")
    );
    assert!(matches!(citation.coordinate, Coordinate::Line { .. }));
    assert!(!citation.spans.is_empty());
    for span in &citation.spans {
        let block = fx
            .db
            .block_text(span.block_id)
            .unwrap()
            .expect("every span must name a real block");
        let quoted = char_slice(&citation.text, span.start, span.end);
        assert!(
            chars_from(&block, span.block_start).starts_with(&quoted),
            "block {} from character {} does not begin with the {} characters the \
             citation says came from there.\n  block: {block:?}\n  quoted: {quoted:?}",
            span.block_id,
            span.block_start,
            quoted.chars().count(),
        );
    }
}

/// D41 where it is visible: the same identifier is findable by its parts
/// inside a fence and not outside one, in one file.
///
/// `prepare_for_search` appends split forms of run-together identifiers for
/// `SourceKind::Code` and for nothing else
/// (`crates/mnema-index/src/text_prep.rs`), so this is what a chunk's kind
/// buys. The document is `text/markdown` — a `Document` — so before D41 every
/// chunk of it took the document's kind and neither half got the split.
///
/// The negative half is the load-bearing one. Asserting only that the fence is
/// findable by `userName` would pass with every chunk in the file marked as
/// code, which is the other way to get this wrong — and is what "any code span
/// at all makes the chunk code" would do to a handbook full of one-line
/// commands.
///
/// Two sections, so the two chunks are two pages and the kinds are decided
/// separately: page 2 is a heading and a fence, which is code by a clear
/// majority of its characters, and page 1 is prose.
#[test]
fn a_code_chunk_is_searchable_by_its_camel_case_parts_and_prose_is_not() {
    let fx = Fixture::new();
    // The same identifier in both sections, so that what differs is the kind
    // of the chunk and not the text in it.
    fx.place(
        "довідники/api.md",
        "# Опис\n\nУ прикладі нижче readConfigValue повертає рядок конфігурації.\n\n\
         # Приклад\n\n\
         ```rust\nlet readConfigValue = завантажити();\nlet ціна = readConfigValue + 1;\n```\n"
            .as_bytes(),
    );
    fx.ingest("довідники/api.md");

    let kinds = chunk_kinds(&fx);
    assert_eq!(
        kinds.len(),
        2,
        "two sections, two pages, one chunk each: {kinds:?}"
    );
    let (prose_kind, prose_text) = &kinds[0];
    let (code_kind, code_text) = &kinds[1];
    assert_eq!(prose_kind, "document", "{prose_text:?}");
    assert_eq!(
        code_kind, "code",
        "a chunk that is mostly a fence is code, whatever its document is: {code_text:?}"
    );

    let by_whole = fx.db.search_lexical("readConfigValue", 10).unwrap();
    assert_eq!(
        by_whole.len(),
        2,
        "both sections hold the identifier and both must answer to it"
    );

    // `config` is a part, not a word: only the code chunk's prepared text has
    // it, because only that chunk was prepared as code.
    let by_part = fx.db.search_lexical("config", 10).unwrap();
    assert_eq!(
        by_part.len(),
        1,
        "exactly the fence answers to a split identifier"
    );
    assert!(
        fx.db
            .citation(by_part[0])
            .unwrap()
            .expect("the chunk exists")
            .text
            .contains("let readConfigValue"),
        "the chunk that answered must be the fence itself"
    );

    // …and the document is still what it is. A file is not code because part
    // of it is.
    let document_kind: String = fx
        .db
        .conn()
        .query_row("SELECT source_kind FROM document", [], |r| r.get(0))
        .unwrap();
    assert_eq!(document_kind, "document");
}

/// The price of majority typing, pinned rather than described.
///
/// A one-line command inside a page of prose is a minority of its chunk, so
/// the chunk is a `Document` and the identifier in the fence gets no split
/// forms — findable by `readConfigValue`, not by `config`. That is a real loss
/// of recall, and it is the side of the trade that was chosen deliberately:
/// the alternative marks a whole handbook as code and loses it from every
/// document-scoped query.
///
/// It is also what the withdrawn standalone rule was buying, at the price of
/// cutting this page into fragments — which is why the cost is recorded here
/// beside the test that measures the alternative
/// (`mnema_chunk`'s `a_page_of_prose_and_fences_does_not_become_all_fragments`).
#[test]
fn a_fence_that_is_a_minority_of_its_chunk_takes_the_prose_kind() {
    let fx = Fixture::new();
    fx.place(
        "довідники/крок.md",
        "# Крок перший\n\n\
         Комісія розглянула звернення щодо постачання лабораторного обладнання \
         та погодила строки приймання робіт за кожним етапом окремо, про що \
         складено акт за формою, наведеною нижче у цьому ж розділі довідника.\n\n\
         ```sh\nreadConfigValue\n```\n"
            .as_bytes(),
    );
    fx.ingest("довідники/крок.md");

    let kinds = chunk_kinds(&fx);
    assert_eq!(kinds.len(), 1, "one section, one chunk: {kinds:?}");
    assert_eq!(kinds[0].0, "document");
    assert!(
        kinds[0].1.contains("readConfigValue"),
        "the fence is in the chunk, it is simply not most of it"
    );

    assert_eq!(
        fx.db.search_lexical("readConfigValue", 10).unwrap().len(),
        1,
        "the identifier is still findable by its own spelling"
    );
    assert!(
        fx.db.search_lexical("config", 10).unwrap().is_empty(),
        "and not by its parts — this is what majority typing costs"
    );
}

/// The test task 10 could not write.
///
/// `ord` is unique per document, not per page, so the counter has to be
/// carried across pages **and** across the transaction slices the write loop
/// cuts them into. With one page there was nothing to carry to: replacing
/// `next_ord = chunk.ord + 1` with `next_ord = 0` left every test in the
/// repository green, which was measured at the time and recorded as a known
/// gap rather than assumed.
///
/// **It does not test the carry across transaction slices, and the name no
/// longer says it does.** `next_ord` is declared outside the
/// `pages.chunks(PAGES_PER_TRANSACTION)` loop, so slicing cannot affect it:
/// setting the constant to `usize::MAX` leaves the entire workspace green —
/// measured. Nothing else could catch that either, short of a fault-injection
/// seam that killed a job between two transactions, which is a shape the
/// product would carry for the tests' sake.
///
/// The fixture is still generated from `PAGES_PER_TRANSACTION + 2`, for a
/// different reason than the one it was written for. Crossing a slice boundary
/// is what makes the `slice_no == 0` guard reachable — the guard that decides
/// whether `insert_document` and `repoint` run once or once per transaction —
/// and mutating that guard to `slice_no == 0 || true` reddens **this test and
/// nothing else in the workspace**, on `UNIQUE constraint failed: document.id`.
/// Generating the fixture from the constant is what keeps that true if the
/// constant moves.
#[test]
fn ord_rises_across_pages() {
    let sections = mnema_ingest::PAGES_PER_TRANSACTION + 2;
    let mut source = String::new();
    for n in 1..=sections {
        source.push_str(&format!(
            "# Розділ {n}\n\nПоложення {n} про постачання обладнання та строки приймання робіт.\n\n"
        ));
    }

    let fx = Fixture::new();
    fx.place("довідники/довгий.md", source.as_bytes());
    let outcome = fx.ingest("довідники/довгий.md");
    let Ingested::Indexed { chunks, .. } = &outcome else {
        panic!("expected the file to be indexed, got {outcome:?}");
    };

    assert_eq!(
        fx.count("SELECT count(*) FROM page") as usize,
        sections,
        "one page per section, and more than one transaction's worth"
    );
    assert!(
        sections > mnema_ingest::PAGES_PER_TRANSACTION,
        "the fixture has to cross a slice boundary to reach the `slice_no == 0` guard"
    );

    // Strictly increasing, with no repeats — the collision `UNIQUE(document_id,
    // ord)` would raise is the loud half of this, and a counter that rose only
    // within a slice would be the quiet half.
    let ords: Vec<i64> = fx
        .db
        .conn()
        .prepare("SELECT ord FROM chunk ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(ords.len(), *chunks);
    assert_eq!(
        ords,
        (0..ords.len() as i64).collect::<Vec<_>>(),
        "ord is the document's own sequence, carried across pages and slices"
    );
}

// ------------------- a broken worker must not take the index with it
//
// `wrong_worker` itself now lives in `tests/support/mod.rs`, shared with
// `walk.rs` (see the `mod support;` note near the top of this file).

/// The whole index must survive a worker that is not the worker.
///
/// This is the branch's Critical finding, as it was measured. An `io::Error` on
/// the worker's stdout — which `read_line` raises for any byte sequence that is
/// not UTF-8 — becomes `Failure::Crash`, and `Crash` used to displace. So a
/// sidecar answering every request with raw bytes deleted a document per file,
/// returning `Ok(Skipped)` every time: the job did not stop, and the journal
/// read as "these files could not be read" rather than "your index is gone".
///
/// Three files rather than one, because one deletion is a bug and three is the
/// shape of the failure — it scales with the walk.
#[cfg(unix)]
#[test]
fn a_worker_that_is_not_the_worker_does_not_empty_the_index() {
    let fx = Fixture::new();
    // Three *different* documents, not three copies: content addressing would
    // make copies one document, and then one deletion would look like three.
    for name in ["a.txt", "b.txt", "c.txt"] {
        let text = format!("{CONTRACT}\nДодаток до {name}.\n");
        fx.place_at(name, text.as_bytes(), mtime());
        assert!(matches!(fx.ingest(name), Ingested::Indexed { .. }));
    }
    assert_eq!(fx.count("SELECT count(*) FROM document"), 3);
    assert_eq!(fx.count("SELECT count(*) FROM path"), 3);
    let chunks_before = fx.count("SELECT count(*) FROM chunk");

    // The sidecar is replaced by something that is not it. Every file is
    // touched, so the cheap arm cannot answer for any of them.
    let broken = support::wrong_worker(fx.root.parent().unwrap(), r"printf '\377\376\n'");
    for name in ["a.txt", "b.txt", "c.txt"] {
        set_mtime(&fx.root.join(name), mtime_just_after());
        assert_eq!(
            fx.ingest_with_worker(name, &broken),
            Ingested::Skipped {
                rule: SkipRule::Crash
            },
        );
    }

    assert_eq!(
        fx.count("SELECT count(*) FROM document"),
        3,
        "a worker that cannot speak to this parent deleted the index, one file at \
         a time, while every call returned Ok"
    );
    assert_eq!(fx.count("SELECT count(*) FROM path"), 3);
    assert_eq!(fx.count("SELECT count(*) FROM chunk"), chunks_before);
    assert!(!fx.db.search_lexical("Равелла", 10).unwrap().is_empty());
    assert_eq!(
        fx.db.skips_for_root(fx.root_id).unwrap().len(),
        3,
        "the skips are still journalled — kept is not the same as unnoticed"
    );
}

/// The same door, opened by the machine rather than by the binary.
///
/// A deadline that cannot be met is a fact about how loaded the machine is,
/// not about the file: the same document reads fine when it is quiet. If
/// `Timeout` displaced, a machine slow enough to miss every deadline would
/// empty the index exactly as a broken worker would.
///
/// The real worker, with a deadline no process can meet — which is also the
/// cheapest way to reach this rule without a stand-in.
#[test]
fn a_deadline_the_machine_could_not_meet_does_not_delete_the_document() {
    let fx = Fixture::new();
    let path = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the file to index")
    };

    set_mtime(&path, mtime_just_after());
    assert_eq!(
        fx.ingest_with_timeout("contracts/ravella.txt", Duration::from_nanos(1)),
        Ingested::Skipped {
            rule: SkipRule::Timeout
        }
    );

    assert_eq!(fx.count("SELECT count(*) FROM document"), 1);
    assert_eq!(fx.db.path_count(&document_id).unwrap(), 1);
    assert!(
        !fx.db.search_lexical("Равелла", 10).unwrap().is_empty(),
        "a slow machine deleted an indexed document"
    );
}

/// A worker from another release stops the job, and leaves the index exactly
/// as it was.
///
/// The strict unknown-rule arm in the pool is the only thing standing between
/// this and the deletion above through a different door: `Unsupported`
/// displaces, so a worker refusing under a rule this parent does not know
/// would remove the indexed content of every file it named. The pool's own
/// test pins the `Err`; this pins what the index looks like afterwards, which
/// is the half that matters here.
///
/// The rule string was `"encrypted"` until this build learned that word: a
/// stand-in for "unknown" that is only unknown for a while tests the opposite
/// of what it claims, so the replacement is deliberately one no roadmap
/// contains.
///
/// **This test reddened when that happened, and it does not name the rule
/// anywhere** — the `matches!` below asks only for a `Protocol` error. It went
/// red because a recognised rule makes the call return `Ok`, and it would have
/// gone red a second time over regardless: the stand-in worker sends a refusal
/// with no `sha256`, `Frame::Refused` gives that field `#[serde(default)]`
/// (`crates/mnema-core/src/wire.rs`), and `displaces` treats an unknown digest
/// as displacing — so the document these assertions expect to still be there
/// would have been deleted. What was missing was never redness; it was a red
/// that points at the premise instead of at the pool. Hence the `parse` check.
#[cfg(unix)]
#[test]
fn a_worker_from_another_release_stops_the_job_and_leaves_the_index_alone() {
    let fx = Fixture::new();
    let path = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the file to index")
    };

    assert_eq!(
        SkipRule::parse("rule_from_a_later_release"),
        None,
        "the stand-in for an unknown rule has become a rule this build knows"
    );
    let broken = support::wrong_worker(
        fx.root.parent().unwrap(),
        r#"printf '{"frame":"refused","rule":"rule_from_a_later_release","reason":"unnameable"}\n'"#,
    );
    set_mtime(&path, mtime_just_after());
    let outcome = fx.try_ingest_with_worker("contracts/ravella.txt", &broken);
    assert!(
        matches!(
            outcome,
            Err(mnema_ingest::IngestError::Pool(
                mnema_pool::PoolError::Protocol { .. }
            ))
        ),
        "a rule this parent does not know means the binaries do not match, which \
         must stop the job rather than be guessed at: {outcome:?}"
    );

    assert_eq!(fx.count("SELECT count(*) FROM document"), 1);
    assert_eq!(fx.db.path_count(&document_id).unwrap(), 1);
    assert!(!fx.db.search_lexical("Равелла", 10).unwrap().is_empty());
    assert_eq!(
        fx.db.skips_for_root(fx.root_id).unwrap().len(),
        0,
        "a protocol mismatch is not a per-file skip and must not be journalled as one"
    );
}

/// The checkpoint and the status land together or not at all.
///
/// They are written after the rows on purpose — a crash before them costs a
/// re-index rather than a lie, because the cheap arm finds no finished stage
/// and step 3 rebuilds. But that recovery is *reached* by finding no finished
/// stage, so a crash between the two statements is the one interruption it
/// cannot see: the stage says `done`, the status says `pending`, and every
/// future walk short-circuits on the stage. The document never comes back.
///
/// It is the same defect the stage check was added to close, one statement
/// further down. Measured with the second write forced to fail.
#[test]
fn the_checkpoint_and_the_status_cannot_disagree_after_an_interruption() {
    let fx = Fixture::new();
    fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());

    fx.break_writes_to("UPDATE", "document");
    let outcome = fx.try_ingest("contracts/ravella.txt");
    assert!(
        matches!(outcome, Err(mnema_ingest::IngestError::Index(_))),
        "expected the status write to fail, got {outcome:?}"
    );
    fx.unbreak_writes();

    let id: String = fx
        .db
        .conn()
        .query_row("SELECT id FROM document", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        fx.db.stage_status(&id, mnema_ingest::STAGE_CHUNK).unwrap(),
        None,
        "the stage was committed while the status was not, so the cheap arm will \
         answer Unchanged for ever and the document stays pending"
    );

    // …and because it did not, the next walk repairs the document instead of
    // stepping over it.
    let again = fx.ingest("contracts/ravella.txt");
    assert!(matches!(again, Ingested::Indexed { .. }), "{again:?}");
    assert_eq!(
        fx.db.document_status(&id).unwrap(),
        mnema_index::DocumentStatus::Indexed
    );
    assert_eq!(
        fx.db
            .stage_status(&id, mnema_ingest::STAGE_CHUNK)
            .unwrap()
            .as_deref(),
        Some(mnema_ingest::STATUS_DONE)
    );
}

/// A document with no pages is written as a document with no pages, not
/// skipped in silence.
///
/// `[T]::chunks` yields nothing for an empty slice, and slice 0 is where
/// `insert_document` and `repoint` live — so a reader reporting no pages
/// skipped the whole write while step 5 ran anyway. What came back was
/// `Indexed { chunks: 0 }` naming a `document_id` no row carried,
/// `set_document_status` updating nothing, an orphan `ingest_stage` row, and
/// the index still citing the file's previous text under its own name.
///
/// No reader in this product can produce it today: plain text always emits one
/// page and markdown starts from one. A worker of the wrong version can, and a
/// PDF whose every page was skipped for having no text layer will — which is
/// exactly when there has to be a document to hang those per-page `skipped`
/// rows on.
#[cfg(unix)]
#[test]
fn a_document_with_no_pages_is_still_written() {
    let fx = Fixture::new();
    let path = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the file to index")
    };

    // A worker that reports a document of nought pages: a header, then a
    // summary, and nothing between them.
    let empty = support::wrong_worker(
        fx.root.parent().unwrap(),
        &format!(
            "printf '{}\\n{}\\n'",
            // `reader`/`reader_version` are filled in with a reader this build
            // does not have, which is what a worker of another version would
            // send and costs this test nothing: its subject is the page count
            // of nought, and a header missing a required field would fail it
            // for the wrong reason — as a protocol error, before the empty
            // document was ever written.
            r#"{"frame":"header","sha256":"'"$(printf %064d 7)"'","mime":"application/pdf","source_kind":"document","reader":"pdf","reader_version":1,"pages":0}"#,
            r#"{"frame":"summary","skipped_pages":0,"text_source":"native:pdf"}"#
        ),
    );
    set_mtime(&path, mtime_just_after());
    let outcome = fx.ingest_with_worker("contracts/ravella.txt", &empty);

    let Ingested::Indexed {
        document_id: new_id,
        chunks,
    } = &outcome
    else {
        panic!("expected the empty document to be written, got {outcome:?}")
    };
    assert_eq!(*chunks, 0, "there were no pages, so there are no chunks");
    assert_ne!(
        new_id, &document_id,
        "a different sha256 is a different document"
    );

    // The row it named exists, and the path points at it.
    assert!(
        fx.db.document_exists(new_id).unwrap(),
        "Indexed named a document_id that no row carries"
    );
    assert_eq!(fx.db.path_count(new_id).unwrap(), 1);
    assert_eq!(
        fx.db.document_status(new_id).unwrap(),
        mnema_index::DocumentStatus::Indexed,
        "set_document_status updated nothing"
    );
    assert_eq!(
        fx.db
            .stage_status(new_id, mnema_ingest::STAGE_CHUNK)
            .unwrap()
            .as_deref(),
        Some(mnema_ingest::STATUS_DONE)
    );

    // And the previous version of the file is gone rather than still answering.
    assert!(
        fx.db.search_lexical("Равелла", 10).unwrap().is_empty(),
        "the write was skipped, so the old text is still in the index under this \
         file's own name"
    );
    assert_eq!(fx.count("SELECT count(*) FROM document"), 1);
}

/// Two files, two documents, and the content of one replaced by the content of
/// the other.
///
/// The uncovered line was the `displaced` argument on the `AlreadyIndexed`
/// path: the content is already indexed under the *other* file's name, so only
/// a path row is new — and without telling `repoint` what this path used to
/// hold, the document it held keeps every chunk with no path row at all.
/// Answering with text no file on disk contains, cited with `relative_path:
/// None`, for ever.
///
/// Only the first file is walked afterwards, which is the ordinary case: a
/// walk reaches one name before the other.
#[test]
fn replacing_one_files_content_with_anothers_leaves_no_document_unnamed() {
    let fx = Fixture::new();
    let x = CONTRACT.to_string();
    let y = CONTRACT.replace("Равелла", "Мурашка");
    let a = fx.place_at("a.txt", x.as_bytes(), mtime());
    fx.place_at("b.txt", y.as_bytes(), mtime());

    let Ingested::Indexed {
        document_id: doc_x, ..
    } = fx.ingest("a.txt")
    else {
        panic!("expected a.txt to index")
    };
    assert!(matches!(fx.ingest("b.txt"), Ingested::Indexed { .. }));
    assert_eq!(fx.count("SELECT count(*) FROM document"), 2);

    // The user saves b.txt's content over a.txt. The bytes are already indexed
    // under b.txt, so this is the `AlreadyIndexed` arm.
    set_bytes_and_mtime(&a, y.as_bytes(), mtime_just_after());
    assert!(matches!(
        fx.ingest("a.txt"),
        Ingested::AlreadyIndexed { .. }
    ));

    assert!(
        !fx.db.document_exists(&doc_x).unwrap(),
        "the document holding the replaced content kept its chunks with no path \
         naming it — every citation of it would name no file at all"
    );
    assert!(
        fx.db.search_lexical("Равелла", 10).unwrap().is_empty(),
        "text no file on disk contains is still answering"
    );
    assert_eq!(fx.count("SELECT count(*) FROM document"), 1);
    assert_eq!(fx.count("SELECT count(*) FROM path"), 2);
}

/// Contention is not a reason to end a walk.
///
/// The shape is ordinary: the window adds a folder while a walk is running, so
/// a second connection holds the write lock. `BUSY_TIMEOUT` is five seconds and
/// the walk waits all of them; measured, `ingest_file` came back after 5.19 s
/// with `SQLITE_BUSY`. By this crate's own contract every `IngestError` means
/// the job stops — which for the one error that means "wait and retry" is the
/// wrong answer, and would end a multi-hour walk because the user did something
/// perfectly reasonable.
///
/// No timing assertion: the wait is `BUSY_TIMEOUT`'s business, and a bound
/// tight enough to prove "fast" is one a loaded machine misses. What is
/// asserted is which error came back.
#[test]
fn a_walk_that_meets_the_window_holding_the_write_lock_is_told_to_retry() {
    let fx = Fixture::new();
    fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());

    // The window's connection, holding the write lock the way `open_index` +
    // a folder being added would.
    let window = open(&fx.index_path).unwrap();
    window.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
    window.insert_watched_root("/Volumes/Second").unwrap();

    let outcome = fx.try_ingest("contracts/ravella.txt");
    window.conn().execute_batch("COMMIT").unwrap();

    assert!(
        matches!(outcome, Err(mnema_ingest::IngestError::Busy(_))),
        "contention must be nameable as contention, or a walker cannot tell it \
         from a database it should give up on: {outcome:?}"
    );

    // …and the file really is still there to be retried.
    assert!(matches!(
        fx.ingest("contracts/ravella.txt"),
        Ingested::Indexed { .. }
    ));
}

/// The sequence the randomised harness found, written out so it stays found.
///
/// A document is dropped — here by its file being replaced with bytes no reader
/// takes, which displaces it — and then the same content comes back, which an
/// undo, a restore from backup or a file moved out and back all do. Because
/// `ingest_stage` is keyed on the *content* hash, a `done` stage left behind by
/// the dropped document was waiting for its replacement: `document_exists` is
/// false, a fresh row is inserted at `status = 'pending'`, and any interruption
/// before the new document's own checkpoint left `done` over `pending` for
/// good, with every later walk short-circuiting on the stage before it could
/// repair the status.
///
/// The harness reached this from seven of eight seed ranges, and will stop
/// reaching it now that it is fixed — which is why it is written out. A seed
/// that may not come up again is not a regression test.
#[test]
fn content_that_comes_back_finds_no_checkpoint_waiting_for_it() {
    let fx = Fixture::new();
    let path = fx.place_at("contracts/ravella.txt", CONTRACT.as_bytes(), mtime());
    let Ingested::Indexed { document_id, .. } = fx.ingest("contracts/ravella.txt") else {
        panic!("expected the file to index")
    };

    // Dropped: a PDF saved over it, which the worker reads and declines.
    set_bytes_and_mtime(
        &path,
        b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n",
        mtime_just_after(),
    );
    assert_eq!(
        fx.ingest("contracts/ravella.txt"),
        Ingested::Skipped {
            rule: SkipRule::Unsupported
        }
    );
    assert!(!fx.db.document_exists(&document_id).unwrap());
    assert_eq!(
        fx.db
            .stage_status(&document_id, mnema_ingest::STAGE_CHUNK)
            .unwrap(),
        None,
        "the checkpoint outlived the document, and the same content is about to \
         come back to it"
    );

    // …and back: an undo, or a restore. The bytes are identical, so this is the
    // same content hash the stage was keyed on.
    set_bytes_and_mtime(&path, CONTRACT.as_bytes(), mtime_just_after());

    // Interrupted before its own checkpoint, exactly as a kill would.
    fx.break_writes_to("INSERT", "ingest_stage");
    let outcome = fx.try_ingest("contracts/ravella.txt");
    assert!(
        matches!(outcome, Err(mnema_ingest::IngestError::Index(_))),
        "expected the checkpoint write to fail, got {outcome:?}"
    );
    fx.unbreak_writes();

    assert_eq!(
        fx.db
            .stage_status(&document_id, mnema_ingest::STAGE_CHUNK)
            .unwrap(),
        None,
        "a stage from the previous life of this content is standing over a \
         document that never finished: the cheap arm will answer Unchanged for \
         ever and nothing will re-examine it"
    );

    // …so the next walk repairs it rather than stepping over it.
    assert!(matches!(
        fx.ingest("contracts/ravella.txt"),
        Ingested::Indexed { .. }
    ));
    assert_eq!(
        fx.db.document_status(&document_id).unwrap(),
        mnema_index::DocumentStatus::Indexed
    );
    assert!(!fx.db.search_lexical("Равелла", 10).unwrap().is_empty());
}

// ------------------------------------------- a coordinate for every reader

// Every test below is `cfg(unix)` for one reason: the stand-in worker is a
// shell script (`support::wrong_worker`), and none of the five readers these
// coordinates belong to exists yet — pdf, html, docx, epub and xlsx arrive in
// later tasks of this cycle. Until they do, a worker that *states* it is one of
// them is the only way to reach `pages_of`'s new arms at all, and on Windows
// they are unreached. What holds them there is `mnema-chunk`'s own suite, which
// is platform-independent and covers the half that computes a range.

/// A stand-in worker that answers every request with exactly these frames,
/// whatever file it is handed.
///
/// The frames are typed [`Frame`] values put through the wire's own `to_line`
/// rather than JSON written out by hand: a literal would be a second copy of
/// the protocol, green until the day a field moves. A quoted heredoc carries
/// them into the script, so nothing inside them is expanded by the shell.
#[cfg(unix)]
fn worker_answering(fx: &Fixture, frames: &[Frame]) -> PathBuf {
    let lines: String = frames
        .iter()
        .map(|frame| to_line(frame).expect("a frame serialises"))
        .collect();
    support::wrong_worker(
        fx.root.parent().unwrap(),
        &format!("cat <<'MNEMA_FRAMES'\n{lines}MNEMA_FRAMES"),
    )
}

/// A block of a format that has no lines of its own — which is what every one
/// of these readers produces except xlsx.
#[cfg(unix)]
fn unlined_block(text: &str) -> Block {
    Block {
        block_type: BlockType::Paragraph,
        reading_order: 0,
        language: Some("uk".to_string()),
        text: text.to_string(),
        line_start: None,
        line_end: None,
    }
}

/// The coordinate of the one chunk a word is found in.
#[cfg(unix)]
fn coordinate_of(fx: &Fixture, word: &str) -> Coordinate {
    let hits = fx.db.search_lexical(word, 10).unwrap();
    assert_eq!(
        hits.len(),
        1,
        "the fixture is only readable if {word:?} sits in exactly one chunk"
    );
    fx.db
        .citation(hits[0])
        .unwrap()
        .expect("a chunk that was found has a citation")
        .coordinate
}

/// A PDF's citation names the page it came from.
///
/// The whole reason `pages_of` asks which reader made a document. A PDF block
/// carries no line numbers, and `PageContext::Lines` turns that into
/// `Coordinate::None` — every page of every PDF cited with no coordinate at
/// all, silently and without an error anywhere (spec §2.6).
///
/// **Two pages, numbered 7 and 9**, and neither number is the page's position
/// in the document. A coordinate taken from a counter instead of from the page
/// frame passes a one-page fixture and fails here, and so does one that copies
/// the first page's number onto the rest.
#[cfg(unix)]
#[test]
fn a_pdf_chunk_cites_its_page_not_nothing() {
    let fx = Fixture::new();
    let bytes = b"%PDF-1.7\ninvented, and never parsed: the worker below is a script\n";
    fx.place("zvity/richnyi.pdf", bytes);

    let worker = worker_answering(
        &fx,
        &[
            Frame::Header {
                sha256: sha256_of(bytes),
                mime: "application/pdf".to_string(),
                source_kind: SourceKind::Document,
                reader: "pdf".to_string(),
                reader_version: 1,
                pages: 2,
            },
            Frame::Page {
                page_no: 7,
                section_title: None,
            },
            Frame::Block(unlined_block(
                "Річний звіт про виконання кошторису за минулий період.",
            )),
            Frame::Page {
                page_no: 9,
                section_title: None,
            },
            Frame::Block(unlined_block(
                "Додаток до звіту: перелік придбаного обладнання для філії.",
            )),
            Frame::Summary {
                skipped_pages: 0,
                text_source: "native:pdf".to_string(),
            },
        ],
    );

    let outcome = fx.ingest_with_worker("zvity/richnyi.pdf", &worker);
    let Ingested::Indexed { chunks, .. } = &outcome else {
        panic!("expected the pdf to index, got {outcome:?}")
    };
    assert_eq!(*chunks, 2, "one chunk per page");

    assert_eq!(
        coordinate_of(&fx, "кошторису"),
        Coordinate::Page { number: 7 },
        "a PDF block has no line numbers, and `Lines` turns that into \
         Coordinate::None"
    );
    assert_eq!(
        coordinate_of(&fx, "обладнання"),
        Coordinate::Page { number: 9 },
        "the second page's coordinate must come from its own page frame"
    );
}

/// html, docx and epub have sections where a PDF has pages, and none of the
/// three has a line number to fall back on.
///
/// One title per reader, all three different: a coordinate hard-coded to any
/// one of them, or taken from the first page indexed, fails on the other two.
#[cfg(unix)]
#[test]
fn html_docx_and_epub_cite_the_section_their_page_names() {
    let fx = Fixture::new();
    // The reader, the file it made, the section its page names, a sentence to
    // find it by, and the word to search — one word per document, or
    // `coordinate_of` cannot tell the three apart.
    let readers = [
        (
            "html",
            "dovidky/umovy.html",
            "Розділ 3. Умови постачання",
            "Комісія розглянула звернення щодо умов постачання.",
            "звернення",
        ),
        (
            "docx",
            "dovidky/koshtorys.docx",
            "Додаток Б. Кошторис робіт",
            "Кошторис складено за формою, узгодженою сторонами.",
            "формою",
        ),
        (
            "epub",
            "dovidky/pryimannia.epub",
            "Глава друга. Приймання",
            "Приймання виконаних робіт підтверджується актом.",
            "актом",
        ),
    ];

    for (reader, relative, title, prose, word) in readers {
        let bytes = format!("invented bytes for {reader}, never parsed\n").into_bytes();
        fx.place(relative, &bytes);
        let worker = worker_answering(
            &fx,
            &[
                Frame::Header {
                    sha256: sha256_of(&bytes),
                    mime: format!("application/x-invented-{reader}"),
                    source_kind: SourceKind::Document,
                    reader: reader.to_string(),
                    reader_version: 1,
                    pages: 1,
                },
                Frame::Page {
                    page_no: 1,
                    section_title: Some(title.to_string()),
                },
                Frame::Block(unlined_block(prose)),
                Frame::Summary {
                    skipped_pages: 0,
                    text_source: format!("native:{reader}"),
                },
            ],
        );

        let outcome = fx.ingest_with_worker(relative, &worker);
        assert!(
            matches!(outcome, Ingested::Indexed { .. }),
            "expected {reader} to index, got {outcome:?}"
        );
        assert_eq!(
            coordinate_of(&fx, word),
            Coordinate::Section {
                title: title.to_string()
            },
            "{reader} must cite the section its own page names"
        );
    }
}

/// A page of one of those three formats that names **no** section carries an
/// empty one, not `Coordinate::None` — and that is a decision rather than a
/// leftover.
///
/// The two render identically: `Coordinate::Section { title: "" }` and
/// `Coordinate::None` are both the empty string, so on screen this is not a
/// choice at all. In the database it is. A section with no title says "this
/// document has sections and this page was not given one", which is a reader
/// with a hole in it; `None` says "this format has no coordinate to give",
/// which is what a bare text file is. Collapsing the first into the second
/// makes a defect indistinguishable from a design.
///
/// Naming a section is therefore the reader's obligation (spec §6, invariant
/// 1), not this loop's to paper over — and this test is where that obligation
/// is written down, so that whoever writes `html.rs` meets it here rather than
/// in a citation.
#[cfg(unix)]
#[test]
fn a_page_that_names_no_section_carries_an_empty_one_rather_than_none() {
    let fx = Fixture::new();
    let bytes = b"invented html, never parsed\n";
    fx.place("dovidky/bez-zagolovka.html", bytes);

    let worker = worker_answering(
        &fx,
        &[
            Frame::Header {
                sha256: sha256_of(bytes),
                mime: "text/html".to_string(),
                source_kind: SourceKind::Document,
                reader: "html".to_string(),
                reader_version: 1,
                pages: 1,
            },
            // What a page with no heading above it looks like on the wire.
            Frame::Page {
                page_no: 1,
                section_title: None,
            },
            Frame::Block(unlined_block(
                "Текст, який не має над собою жодного заголовка.",
            )),
            Frame::Summary {
                skipped_pages: 0,
                text_source: "native:html".to_string(),
            },
        ],
    );

    let outcome = fx.ingest_with_worker("dovidky/bez-zagolovka.html", &worker);
    assert!(
        matches!(outcome, Ingested::Indexed { .. }),
        "expected the untitled page to index, got {outcome:?}"
    );
    assert_eq!(
        coordinate_of(&fx, "заголовка"),
        Coordinate::Section {
            title: String::new()
        },
    );
}

/// A spreadsheet chunk cites the rows it covers, not the sheet it sits on.
///
/// This is the one the other tests here cannot stand in for, and the reason
/// `PageContext::Rows` exists at all. A sheet is one page, so a *fixed*
/// coordinate would give every chunk of this document the same answer — "аркуш
/// Кошторис, рядки 1–480" — for a chunk that covers forty of them. Non-empty,
/// plausible, and pointing at twelve times too much: an assertion that the
/// coordinate merely *exists* is satisfied by exactly that defect, so the
/// assertions below are that each chunk's range equals the rows of the blocks
/// it actually names, and that not every chunk carries the same one.
///
/// The expected range is read back out of the `block` table rather than
/// computed from the fixture, so a chunk whose spans name blocks other than the
/// ones its coordinate was built from fails here too.
#[cfg(unix)]
#[test]
fn an_xlsx_chunk_cites_the_rows_it_covers_not_the_whole_sheet() {
    let fx = Fixture::new();
    let bytes = b"PK\x03\x04 invented, and never unzipped\n";
    fx.place("koshtorysy/biudzhet.xlsx", bytes);

    // Twelve blocks of forty sheet rows each: one sheet spanning rows 1–480.
    let mut frames = vec![
        Frame::Header {
            sha256: sha256_of(bytes),
            mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
            source_kind: SourceKind::Data,
            reader: "xlsx".to_string(),
            reader_version: 1,
            pages: 1,
        },
        Frame::Page {
            page_no: 1,
            section_title: Some("Кошторис".to_string()),
        },
    ];
    frames.extend((0..12u32).map(|i| {
        Frame::Block(Block {
            block_type: BlockType::Table,
            reading_order: i64::from(i),
            language: Some("uk".to_string()),
            text: (0..3)
                .map(|row| {
                    format!(
                        "Позиція {i}.{row}: закупівля лабораторного обладнання для філії, \
                         сума узгоджена комісією.\n"
                    )
                })
                .collect(),
            // The rows this block occupies in the sheet, which is what the
            // xlsx reader owes every block it emits (spec §2.6).
            line_start: Some(i * 40 + 1),
            line_end: Some(i * 40 + 40),
        })
    }));
    frames.push(Frame::Summary {
        skipped_pages: 0,
        text_source: "native:xlsx".to_string(),
    });

    let worker = worker_answering(&fx, &frames);
    let outcome = fx.ingest_with_worker("koshtorysy/biudzhet.xlsx", &worker);
    let Ingested::Indexed {
        document_id,
        chunks,
    } = &outcome
    else {
        panic!("expected the spreadsheet to index, got {outcome:?}")
    };
    assert!(
        *chunks >= 3,
        "a sheet that becomes one chunk cannot tell a narrowed coordinate from \
         a fixed one, and this fixture produced {chunks}"
    );

    // The premise: the sheet really does span 480 rows, so a coordinate naming
    // all of them is a visibly different answer from one naming a chunk's.
    let sheet: (i64, i64) = fx
        .db
        .conn()
        .query_row(
            "SELECT min(line_start), max(line_end) FROM block",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(sheet, (1, 480), "the fixture's own rows");

    let mut ranges = Vec::new();
    for id in fx.chunk_ids(document_id) {
        let citation = fx
            .db
            .citation(id)
            .unwrap()
            .expect("a chunk that was written has a citation");
        let Coordinate::SheetRows {
            sheet: name,
            start,
            end,
        } = citation.coordinate
        else {
            panic!("chunk {id} carries {:?}", citation.coordinate)
        };
        assert_eq!(name, "Кошторис", "the sheet's name, from its page");

        let covered: Vec<i64> = citation.spans.iter().map(|s| s.block_id).collect();
        assert_eq!(
            (start, end),
            rows_of(&fx, &covered),
            "chunk {id} cites rows {start}–{end} while covering blocks {covered:?}"
        );
        assert_ne!(
            (start, end),
            (1, 480),
            "chunk {id} cites the whole sheet — which is what `Fixed` would give \
             it, and what this variant exists to prevent"
        );
        ranges.push((start, end));
    }
    ranges.dedup();
    assert!(
        ranges.len() > 1,
        "every chunk carries the same range, so nothing here distinguishes a \
         narrowed coordinate from a fixed one: {ranges:?}"
    );
}

/// The row range of a set of blocks, read back out of the database.
#[cfg(unix)]
fn rows_of(fx: &Fixture, block_ids: &[i64]) -> (u32, u32) {
    let mut start = u32::MAX;
    let mut end = 0;
    for id in block_ids {
        let rows: (Option<i64>, Option<i64>) = fx
            .db
            .conn()
            .query_row(
                "SELECT line_start, line_end FROM block WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let (a, b) = (
            rows.0.expect("the fixture gives every block its rows"),
            rows.1.expect("the fixture gives every block its rows"),
        );
        start = start.min(a as u32);
        end = end.max(b as u32);
    }
    (start, end)
}
