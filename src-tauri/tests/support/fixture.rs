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

use mnema_desktop::state::AppState;
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
    _server: MockServer,
}

impl Fixture {
    /// A provider that refuses every key it is shown.
    pub fn with_provider_rejecting_the_key() -> Self {
        Self::new(vec![Reply::status(401, r#"{"error":{"message":"nope"}}"#)])
    }

    /// A provider that answers a credit check, twice — once for the call under
    /// test and once spare, so a test that sets a key again does not run the
    /// server out of answers and fail for that instead.
    pub fn with_provider_accepting_everything() -> Self {
        Self::new(vec![Reply::ok(CREDITS), Reply::ok(CREDITS)])
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

    fn new(replies: Vec<Reply>) -> Self {
        Self::pointed_at(replies, None)
    }

    fn pointed_at(replies: Vec<Reply>, base: Option<&str>) -> Self {
        // Before anything can ask for a credential. `ensure_default_store`
        // accepts whoever registered first, so this has to precede the first
        // `store`/`load`/`forget` in the binary rather than sit beside the
        // assertion that needs it.
        mnema_secrets::test_store::register();
        let server = MockServer::new(replies);
        let dir = tempfile::tempdir().expect("a temporary directory");
        let credential_ref = unique_reference();
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
            _server: server,
        }
    }

    pub fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
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

    /// Records this installation's model configuration in the index.
    ///
    /// That row is the one thing the design deliberately does put in the
    /// database in the key's place: `model_config.credential_ref` holds the
    /// NAME the credential is filed under, never the secret. A test scanning
    /// the database for the key needs it there, or the scan proves only that it
    /// was reading an empty file.
    ///
    /// It goes through the index's own API rather than through a command
    /// because the command that will choose a model has to ask the provider how
    /// wide it is first, and that question is not this file's subject. The row
    /// written here is the row it will write.
    pub fn adopt_default_model(&self) {
        self.state()
            .with_index(|db| {
                db.adopt_embedding_model(
                    DEFAULT_MODEL,
                    DEFAULT_DIM,
                    &self.credential_ref,
                    &mnema_chunk::chunker_hash(),
                )
            })
            .expect("the default model is adopted");
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
    fn drop(&mut self) {
        if let Err(e) = mnema_secrets::forget(&self.credential_ref) {
            eprintln!(
                "could not remove the test credential `{}` — delete it by hand: {e}",
                self.credential_ref
            );
        }
    }
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
