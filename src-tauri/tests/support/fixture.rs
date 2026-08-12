//! The application fixture: a mock provider, a temporary index, and a
//! credential reference nobody else uses.
//!
//! **Beside `mod.rs`, not inside it**, and included with `#[path]` by the one
//! test binary that wants it. `mod.rs` is declared by every binary under
//! `tests/`, and Cargo compiles a shared module separately into each of them —
//! so anything here that `commands.rs` does not use would be dead code *there*,
//! and the only ways to silence that are a blanket `allow` or an unused import
//! of every item. A file a binary has to ask for is checked in the binary that
//! asks, and nowhere else.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_desktop::models::ExistingVectors;
use mnema_desktop::state::AppState;
use mnema_index::DocumentStatus;
use mnema_mock_provider::{MockServer, Reply};
use tauri::Manager as _;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

use crate::support::worker;

/// The default model this product offers, and how wide its vectors are.
///
/// Named here rather than inside one test because more than one of them needs
/// an index that has been configured at all, and "configured" is not a state a
/// test should spell differently each time it wants one.
const DEFAULT_MODEL: &str = "baai/bge-m3";
const DEFAULT_DIM: i64 = 1024;

/// A credits body a key check can read: ten bought, one spent.
const CREDITS: &str = r#"{"data":{"total_credits":10.0,"total_usage":1.0}}"#;

/// A running application state pointed at a mock provider and at an in-memory
/// credential store, with a reference nobody else uses.
///
/// `MockServer` comes from `mnema-mock-provider`, the same crate the provider's
/// own tests use — one server, not two copies that drift.
///
/// **Two separate guards keep this off a real machine's credential store, and
/// they are not interchangeable.**
///
/// The first is the store. `mnema-secrets` keeps the platform store out of reach
/// only under its **own** `cfg(test)` — the `#[cfg(test)]` arm inside
/// `platform_store`, `crates/mnema-secrets/src/lib.rs:313,320` — and an
/// integration test of another crate compiles it without that flag. Left alone,
/// everything below would reach the developer's real login keychain, and would
/// fail outright on a runner that has neither an unlocked keychain nor a Secret
/// Service session. `mnema_secrets::test_store::register` puts an in-memory
/// store in front of it, so the shipped `store`/`load`/`forget` run unchanged
/// on any platform and reach nothing that outlives the process.
///
/// The second is the reference, and it is still needed with the store swapped:
/// the fixtures in a binary share that one store, so two of them under one name
/// would cross one test's secret into another and one `Drop` would clear the
/// other's entry. It is also what keeps the production name out of a test
/// binary at all — the guard that still matters for any future test that
/// forgets to register a store.
///
/// Hence a reference unique per process, per thread and per fixture, and a
/// `Drop` that removes it.
pub struct Fixture {
    app: tauri::App<MockRuntime>,
    dir: tempfile::TempDir,
    credential_ref: String,
    server: MockServer,
}

impl Fixture {
    /// A provider that refuses every key it is shown.
    pub fn with_provider_rejecting_the_key() -> Self {
        Self::new(vec![Reply::status(401, r#"{"error":{"message":"nope"}}"#)])
    }

    /// A provider that answers a credit check and then an embedding check —
    /// the order `set_key` and then `set_embedding_model` ask them in, which is
    /// the sequence [`Fixture::adopt_default_model`] makes.
    ///
    /// `MockServer` answers its replies strictly in order, one per connection,
    /// and pays no attention to which path was asked for. So this is a
    /// *sequence* and not a set of behaviours, and a test that calls these two
    /// in the other order gets the wrong body rather than an error saying so.
    pub fn with_provider_accepting_everything() -> Self {
        Self::with_provider_answering_with_dimension(DEFAULT_DIM as usize)
    }

    /// The same two answers, with the embedding check answering vectors `width`
    /// components wide.
    ///
    /// Named for the width because that is the only thing about the provider's
    /// answer `set_embedding_model` may take a dimension from: the model list
    /// states none, and the same model name answers 1536 or 1024 depending on a
    /// parameter (spec §2.4).
    pub fn with_provider_answering_with_dimension(width: usize) -> Self {
        Self::with_provider_answering_embedding_checks(width, 1)
    }

    /// A credit check, then `checks` embedding checks at `width` — the sequence
    /// a test that sets the key once and then chooses a model `checks` times
    /// makes. One reply short and the extra call gets the mock's `599`
    /// sentinel, which fails the test loudly rather than hanging.
    pub fn with_provider_answering_embedding_checks(width: usize, checks: usize) -> Self {
        let mut replies = vec![Reply::ok(CREDITS)];
        let vectors = mnema_mock_provider::two_vectors(width);
        replies.extend((0..checks).map(|_| Reply::ok(&vectors)));
        Self::new(replies)
    }

    /// A credit check, one embedding check at [`DEFAULT_DIM`], and then
    /// whatever `run` says — the sequence a test that sets the key, adopts the
    /// default model and *then* starts an embedding job makes.
    ///
    /// The replies for the run are the caller's because there is no one shape:
    /// how many calls a pass makes depends on how many chunks are queued, how
    /// wide the batch is, and whether a refusal sends it back one text at a
    /// time. `MockServer` answers strictly in order and ignores the path, so a
    /// test that miscounts gets the wrong body rather than a message saying so
    /// — and one reply short gets the mock's `599` sentinel, which fails
    /// loudly.
    pub fn with_provider_answering_a_run(run: Vec<Reply>) -> Self {
        let mut replies = vec![
            Reply::ok(CREDITS),
            Reply::ok(&mnema_mock_provider::two_vectors(DEFAULT_DIM as usize)),
        ];
        replies.extend(run);
        Self::new(replies)
    }

    /// A provider whose model list answers `200` with exactly `body`.
    pub fn with_provider_listing(body: &str) -> Self {
        Self::new(vec![Reply::ok(body)])
    }

    /// A provider that refuses the key and repeats it back inside the refusal.
    /// Not a hypothetical shape: `mnema-provider` runs this same body against
    /// its own failure paths for the same reason
    /// (`crates/mnema-provider/tests/probe.rs:433`), because a provider handed
    /// a malformed credential commonly echoes it back.
    pub fn with_provider_echoing(key: &str) -> Self {
        Self::new(vec![Reply::status(
            401,
            &format!(r#"{{"error":{{"message":"invalid key {key}"}}}}"#),
        )])
    }

    /// A provider whose credit check answers `200` with exactly `body`, for the
    /// tests that care what a balance this build cannot read carries with it.
    pub fn with_provider_stating_credits(body: &str) -> Self {
        Self::new(vec![Reply::ok(body)])
    }

    /// Nothing listening. Port 1 refuses the connection at once, so this is
    /// "nobody answered" reached in microseconds rather than by waiting out a
    /// timeout — a different fact from any answer a provider can give, and the
    /// one this layer has to keep apart from a refusal.
    pub fn with_no_provider_listening() -> Self {
        Self::pointed_at(vec![], Some("http://127.0.0.1:1"))
    }

    /// An application whose credential store cannot be reached at all.
    ///
    /// The reference is empty, and `mnema_secrets::entry` refuses an empty one
    /// **before** it installs or consults any store, so this is a store failure
    /// reached in microseconds and touching nothing — the same trick
    /// `tests/commands.rs` documents at length for `NO_CREDENTIAL`, used here
    /// deliberately rather than incidentally.
    ///
    /// It stands in for the states that make `mnema_secrets::Error` a fact about
    /// the machine rather than a defect: a locked keychain on macOS, an absent
    /// Secret Service session on Linux. Neither can be arranged from a test, and
    /// both arrive at this layer the same way this does — as an `Err` from
    /// `load`.
    pub fn with_a_credential_store_that_will_not_answer() -> Self {
        Self::pointed_at_with_reference(vec![Reply::ok(CREDITS)], None, String::new())
    }

    fn new(replies: Vec<Reply>) -> Self {
        Self::pointed_at(replies, None)
    }

    fn pointed_at(replies: Vec<Reply>, base: Option<&str>) -> Self {
        Self::pointed_at_with_reference(replies, base, unique_reference())
    }

    fn pointed_at_with_reference(
        replies: Vec<Reply>,
        base: Option<&str>,
        credential_ref: String,
    ) -> Self {
        // Before anything can ask for a credential. `ensure_default_store`
        // accepts whoever registered first, so this has to precede the first
        // `store`/`load`/`forget` in the binary rather than sit beside the
        // assertion that needs it.
        mnema_secrets::test_store::register();
        let server = MockServer::new(replies);
        let dir = tempfile::tempdir().expect("a temporary directory");
        let app = mock_builder()
            .manage(AppState::new(
                dir.path().to_path_buf(),
                worker().to_path_buf(),
                base.unwrap_or(server.base()).to_string(),
                credential_ref.clone(),
            ))
            .invoke_handler(mnema_desktop::invoke_handler())
            .build(mock_context(noop_assets()))
            .expect("failed to build the mock application");
        Self {
            app,
            dir,
            credential_ref,
            server,
        }
    }

    pub fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    /// A request the mock provider has already received, or `None` — for a
    /// test whose claim is that a command never called out at all. See
    /// [`MockServer::request_if_any`] for why it does not wait and what makes
    /// the answer sound.
    pub fn provider_request(&self) -> Option<String> {
        self.server.request_if_any()
    }

    pub fn credential_ref(&self) -> &str {
        &self.credential_ref
    }

    pub fn index_path(&self) -> PathBuf {
        self.dir.path().join("index.sqlite")
    }

    pub fn open_index(&self) {
        self.state().open_index().expect("the index opens");
    }

    /// Records this installation's model configuration in the index, through
    /// the command the application uses.
    ///
    /// That row is the one thing the design deliberately does put in the
    /// database in the key's place: `model_config.credential_ref` holds the
    /// NAME the credential is filed under, never the secret. A test scanning
    /// the database for the key needs it there, or the scan proves only that it
    /// was reading an empty file.
    ///
    /// It called `Db::adopt_embedding_model` directly until `set_embedding_model`
    /// existed, and the part of the two that could have diverged in silence was
    /// exactly `credential_ref`: a width that disagreed raises
    /// `SpaceDimMismatch`, a model name that disagreed mints a second space, and
    /// a reference that disagreed writes the wrong name and fails nothing. What
    /// stops it now is not that they agree — it is that there is no second value
    /// to disagree with. The command takes the reference from
    /// `AppState::credential_ref`, this fixture put it there, and this function
    /// passes none.
    ///
    /// It needs a key in the store and a provider answering an embedding check
    /// with `DEFAULT_DIM`-wide vectors, which is
    /// [`Fixture::with_provider_accepting_everything`] with `set_key` called
    /// first.
    /// It adopts with [`ExistingVectors::Keep`], the value that changes nothing
    /// about a fresh index and would refuse rather than destroy on one that is
    /// not — so a test reaching this helper for "an index that is configured"
    /// cannot lose vectors to it by accident.
    pub fn adopt_default_model(&self) {
        let adopted = mnema_desktop::models::set_embedding_model(
            self.state(),
            DEFAULT_MODEL.into(),
            ExistingVectors::Keep,
        )
        .expect("the default model is adopted");
        // Not a restatement of the command's own test. It says the provider
        // this fixture built answered the width this fixture names, so that a
        // caller relying on `DEFAULT_DIM` is relying on something checked
        // rather than on two constants that happen to match.
        assert_eq!(
            adopted.dim, DEFAULT_DIM,
            "the fixture's provider answered a width other than the one this fixture names"
        );
    }

    /// Puts `count` embeddings into the active space, so a model change has
    /// something to be refused over — and something a confirmed one can throw
    /// away.
    ///
    /// The chunk ids are `1..=count` and no `chunk` row exists for any of them.
    /// That is not a shortcut around a foreign key but the shape the schema
    /// actually permits: a `vec0` table is the target of none, which is the
    /// whole reason `Db::delete_vectors_for_document` has to sweep by hand, and
    /// `embedded_chunk_count` counts these rows exactly as it counts any others.
    ///
    /// It returns the space the vectors went into, read back from the index
    /// rather than taken from the adoption that put it there — a test asserting
    /// that a space disappeared must be naming a space the database agrees
    /// exists.
    pub fn embed_chunks_in_the_active_space(&self, count: i64) -> i64 {
        let space = self
            .active_space()
            .expect("a model has to be adopted before anything can be embedded");
        self.state()
            .with_index(|db| {
                for chunk_id in 1..=count {
                    // Finite and far from a zero norm, which is all
                    // `check_rankable` asks of a stored vector; the components
                    // differ per chunk so that two of these are not literally
                    // the same vector under a different key.
                    let width = DEFAULT_DIM as usize;
                    let mut v = vec![0.0f32; width];
                    v[0] = 1.0;
                    v[chunk_id as usize % width] += 0.5;
                    db.insert_vector(space, chunk_id, &v)?;
                }
                Ok(())
            })
            .expect("the vectors are written");
        assert_eq!(
            self.embedded_chunks_in(space),
            count,
            "the index did not record the embeddings this fixture claims to have written, so \
             every assertion about losing them is about nothing"
        );
        space
    }

    /// One `indexed` document holding `count` chunks, each with its own text —
    /// the state an embedding pass actually meets, as opposed to
    /// [`Fixture::embed_chunks_in_the_active_space`], which writes vectors for
    /// chunk ids no `chunk` row exists for.
    ///
    /// It goes through `Db`'s own writers rather than raw SQL for the reason
    /// `adopt_default_model` does: a fixture that writes rows its own way is a
    /// fixture that can build a database the product cannot. `status` is
    /// advanced to `Indexed` last, because the embedding queue joins on it —
    /// `Db::chunks_needing_embedding` ignores a document that is still
    /// `pending`, which is the ordinary state of one being rebuilt.
    ///
    /// Returns the chunk ids, so a test can name a chunk rather than assume
    /// what the ids came out as.
    pub fn write_indexed_chunks(&self, count: usize) -> Vec<i64> {
        self.state()
            .with_index(|db| {
                let doc =
                    db.insert_document(&"a".repeat(64), "text/plain", 64, SourceKind::Document)?;
                let mut ids = Vec::new();
                for ord in 0..count {
                    // Distinct text per chunk: `upsert_vector_for_text` binds a
                    // vector to the text it was made from, and chunks that all
                    // said the same thing would hide a pass that bound one
                    // chunk's answer to another's row.
                    let text = format!("chunk number {ord}");
                    let page = db.insert_page(&doc, ord as i64 + 1, "native:txt", None)?;
                    let block = db.insert_block(
                        page,
                        &Block {
                            block_type: BlockType::Paragraph,
                            reading_order: 0,
                            language: Some("uk".into()),
                            text: text.clone(),
                            line_start: None,
                            line_end: None,
                        },
                    )?;
                    ids.push(db.insert_chunk(
                        &doc,
                        ord as i64,
                        &text,
                        &Locator {
                            spans: vec![Segment {
                                block_id: block,
                                start: 0,
                                end: text.chars().count() as u32,
                                block_start: 0,
                            }],
                            coordinate: Coordinate::None,
                        },
                        SourceKind::Document,
                    )?);
                }
                db.set_document_status(&doc, DocumentStatus::Indexed)?;
                Ok(ids)
            })
            .expect("the chunks are written")
    }

    /// Which space the index is working with, straight from `meta.active_space`.
    pub fn active_space(&self) -> Option<i64> {
        self.state()
            .with_index(|db| db.active_space())
            .expect("the index answers")
    }

    /// Every space the index holds a row for, in id order.
    pub fn space_ids(&self) -> Vec<i64> {
        self.state()
            .with_index(|db| {
                let mut stmt = db
                    .conn()
                    .prepare("SELECT id FROM embedding_space ORDER BY id")?;
                let ids = stmt
                    .query_map([], |r| r.get(0))?
                    .collect::<Result<Vec<i64>, _>>()?;
                Ok(ids)
            })
            .expect("the index answers")
    }

    pub fn embedded_chunks_in(&self, space_id: i64) -> i64 {
        self.state()
            .with_index(|db| db.embedded_chunk_count(space_id))
            .expect("the index answers")
    }

    /// Every table SQLite holds for one space: the `vec0` table itself and the
    /// four shadow tables vec0 brings with it.
    ///
    /// **Matched exactly rather than with `LIKE 'vec_emb_1%'`**, which also
    /// matches space 10 — an assertion that a table is gone must not be
    /// satisfied, or contradicted, by a neighbour's name. The shadow tables are
    /// counted rather than skipped because they are the disk this task exists to
    /// free: a row deleted with four shadow tables left behind is the silent
    /// leak, and it is invisible to any check that looks only for `vec_emb_<id>`.
    pub fn tables_of_space(&self, space_id: i64) -> Vec<String> {
        let own = format!("vec_emb_{space_id}");
        let shadow = format!("{own}_");
        self.state()
            .with_index(|db| {
                let mut stmt = db.conn().prepare(
                    "SELECT name FROM sqlite_master WHERE name LIKE 'vec\\_emb\\_%' ESCAPE '\\' \
                     ORDER BY name",
                )?;
                let names = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                Ok(names)
            })
            .expect("the index answers")
            .into_iter()
            .filter(|name| *name == own || name.starts_with(&shadow))
            .collect()
    }

    /// Every file in the data directory, read as bytes.
    ///
    /// It enumerates rather than naming `index.sqlite`, `-wal` and `-shm`. The
    /// index runs in WAL mode (`crates/mnema-index/src/open.rs:115`), so which
    /// of those exists — and which one a value written moments ago is actually
    /// in — depends on when SQLite last checkpointed. A scan that names the
    /// files it expects is a scan that misses the one it did not think of, and
    /// an assertion of absence checked against the wrong file passes while the
    /// thing it forbids sits on disk one filename over.
    ///
    /// **Where it stops**, written down rather than left to be re-derived. It is
    /// the directory tree — `collect_files` recurses, so a subdirectory is
    /// scanned, not skipped — at one instant, with the connection open, and the
    /// directory is the one the product writes to: `index_path` is
    /// `data_dir.join("index.sqlite")` on both sides
    /// (`src-tauri/src/paths.rs:8-10`). Outside it: SQLite's own temporary
    /// files, which go to `TMPDIR`/`SQLITE_TMPDIR` rather than here (this path
    /// writes too little to spill, so there are none — but a scan would not see
    /// them if there were); anything written after the assertion runs; and the
    /// webview's own storage, where a typed key exists before it ever reaches
    /// this layer.
    pub fn files_on_disk(&self) -> Vec<ScannedFile> {
        let mut found = Vec::new();
        collect_files(self.dir.path(), &mut found);
        found.sort_by(|a, b| a.path.cmp(&b.path));
        found
    }
}

impl Drop for Fixture {
    /// The credential is in the registered store, not in the temporary
    /// directory, so dropping the directory does not remove it — and the store
    /// outlives this fixture, since it is the binary's process-global default.
    /// Not a panic on failure: panicking in a `Drop` during an unwinding
    /// assertion aborts the process and hides the real failure.
    ///
    /// An empty reference is skipped rather than attempted. That is not a
    /// special case bolted on: this function removes the credential its fixture
    /// created, and `with_a_credential_store_that_will_not_answer` cannot have
    /// created one — `forget("")` would fail for the same reason `load("")`
    /// does, and print a hand-wringing line about a credential that never
    /// existed on every passing run.
    fn drop(&mut self) {
        if self.credential_ref.is_empty() {
            return;
        }
        if let Err(e) = mnema_secrets::forget(&self.credential_ref) {
            eprintln!(
                "could not remove the test credential `{}` — delete it by hand: {e}",
                self.credential_ref
            );
        }
    }
}

/// What the provider answers a batch of `count` texts with: `count` vectors
/// [`DEFAULT_DIM`] wide, each naming its own position.
///
/// `mnema_mock_provider::two_vectors` answers exactly two, which is what a
/// model *check* asks for; a run asks for as many as its batch holds. Every
/// vector is a different basis vector, so a pass that bound one chunk's answer
/// to another chunk's row leaves two rows this test can tell apart rather than
/// two copies of one number.
pub fn vectors_for(count: usize) -> String {
    let width = DEFAULT_DIM as usize;
    let rows: Vec<String> = (0..count)
        .map(|i| {
            let hot = i % width;
            let components = (0..width)
                .map(|c| if c == hot { "1.0" } else { "0.0" })
                .collect::<Vec<_>>()
                .join(",");
            format!(r#"{{"embedding":[{components}],"index":{i}}}"#)
        })
        .collect();
    format!(r#"{{"data":[{}]}}"#, rows.join(","))
}

/// One file on disk, with its contents.
pub struct ScannedFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

impl ScannedFile {
    /// Whether `needle` appears anywhere in this file.
    ///
    /// An empty needle is refused rather than answered `true`: it is what a
    /// caller that lost the string it meant to search for passes, and "yes, the
    /// empty string is in here" is the least useful true answer available.
    pub fn holds(&self, needle: &[u8]) -> bool {
        assert!(!needle.is_empty(), "searching for nothing answers nothing");
        self.bytes.len() >= needle.len() && self.bytes.windows(needle.len()).any(|w| w == needle)
    }
}

fn collect_files(dir: &Path, into: &mut Vec<ScannedFile>) {
    for entry in std::fs::read_dir(dir).expect("the data directory can be listed") {
        let path = entry.expect("a directory entry can be read").path();
        if path.is_dir() {
            collect_files(&path, into);
        } else {
            // Named, because the alternative is a bare io error on a path
            // nobody printed. Anything here that is not a readable regular file
            // — a socket, a fifo — is a fact about the data directory worth
            // seeing, not noise to skip past: skipping it would silently shrink
            // the scanned set, which is the one thing this scan must not do.
            let bytes = std::fs::read(&path).unwrap_or_else(|e| {
                panic!(
                    "{} is in the data directory and could not be read, so the scan below \
                     covers less than it says: {e}",
                    path.display()
                )
            });
            into.push(ScannedFile { bytes, path });
        }
    }
}

/// A credential reference no other fixture can collide with.
///
/// Three parts, and each closes a different collision. The process id keeps two
/// concurrent `cargo test` runs apart; the thread id keeps the test binary's own
/// parallel tests apart; the counter keeps two fixtures built on the SAME thread
/// apart, which is the one a test with two providers in it would otherwise hit —
/// two fixtures sharing a reference means one `Drop` deleting the other's
/// credential while it is still in use.
fn unique_reference() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "mnema-test-{}-{:?}-{}",
        std::process::id(),
        std::thread::current().id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
