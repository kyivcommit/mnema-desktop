use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mnema_index::Db;

use crate::error::Error;

/// The core owns the truth; the window only draws it. A reload of the webview
/// must not lose or contradict anything, so nothing lives on the JavaScript side
/// that is not also here. G7.0 §4.
pub struct AppState {
    /// Resolved once, at start-up, rather than re-derived inside each command.
    /// Which directory holds the index is a decision, and a decision taken in
    /// four places is four decisions.
    data_dir: PathBuf,
    /// Where a walk job's `Pool` finds the extraction worker. Resolved once
    /// for the same reason `data_dir` is — see [`crate::paths::worker_path`]
    /// for what this path is good for today and what it is not.
    worker: PathBuf,
    /// Where the provider lives. A field rather than a constant because the
    /// tests point it at a local server; production passes
    /// `mnema_provider::OPENROUTER_BASE` in `lib.rs`.
    provider_base: String,
    /// Which entry in the credential store this installation uses. Never the
    /// secret — the name it is filed under.
    ///
    /// A field rather than a constant for a sharper reason than the one above.
    /// `mnema-secrets` keeps the platform store out of reach only under its
    /// **own** `cfg(test)` — the `#[cfg(test)]` arm inside `platform_store`
    /// (`crates/mnema-secrets/src/lib.rs:313,320`) — and an integration test of
    /// *this* crate compiles that one without the flag, so a test here reaches
    /// whatever store the process has. Tests register an in-memory one and give
    /// each fixture its own reference inside it; a shared reference would cross
    /// one test's secret into another, and the production name in a test binary
    /// would put a test's value where the application looks for the user's.
    credential_ref: String,
    /// `None` until the first `open_index`. The window opens before the database
    /// does, because a failure to open must be something the user can read
    /// rather than a process that never draws.
    db: Mutex<Option<Db>>,
    /// Whether the single job slot is taken. Separate from `cancel`, which is
    /// about a job that is already running.
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(
        data_dir: PathBuf,
        worker: PathBuf,
        provider_base: String,
        credential_ref: String,
    ) -> Self {
        Self {
            data_dir,
            worker,
            provider_base,
            credential_ref,
            db: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn worker_path(&self) -> &Path {
        &self.worker
    }

    pub fn provider_base(&self) -> &str {
        &self.provider_base
    }

    pub fn credential_ref(&self) -> &str {
        &self.credential_ref
    }

    /// Opens the index, creating the directory and the database if needed, and
    /// keeps the connection. Returns where it is and what schema version it
    /// reached — the version because the window displays it, so that someone
    /// reporting a problem can say which schema they are on.
    ///
    /// It is NOT what lets the window explain a database written by a newer
    /// Mnema, which is what this comment claimed until the branch review. In
    /// that case migration fails, `open` returns `Err`, and no version is ever
    /// produced to say anything specific with. What reaches the page today is
    /// the underlying string: `rusqlite_migration error in migrations
    /// definition: Attempt to migrate a database with a migration number that is
    /// too high`. Turning that into a typed "your index is newer than this
    /// application" needs an error variant rather than a number, and belongs to
    /// the interface spec.
    ///
    /// Calling it again re-opens: the previous connection is dropped, which is
    /// what makes a failed open recoverable without restarting the process.
    pub fn open_index(&self) -> Result<(PathBuf, i64), Error> {
        std::fs::create_dir_all(&self.data_dir).map_err(|source| Error::DataDir {
            path: self.data_dir.display().to_string(),
            source,
        })?;
        let path = crate::paths::index_path(&self.data_dir);

        let db = mnema_index::open(&path)?;
        let version = db.schema_version()?;
        *self.db.lock().map_err(|_| Error::StatePoisoned)? = Some(db);
        Ok((path, version))
    }

    /// A second connection to the same index, for a job that must not hold the
    /// window's.
    ///
    /// Everything the window reads goes through [`AppState::with_index`], which
    /// takes the lock around `db` for the length of the call. An indexing job
    /// is a sequence of writes that runs for hours; on that connection it would
    /// hold the lock for the length of each one, and every search the user
    /// typed meanwhile would queue behind it.
    ///
    /// A second connection is not a workaround for the lock, it is what SQLite
    /// is set up for here: WAL is on and a busy timeout is set
    /// (`crates/mnema-index/src/open.rs:114-115`), so a writer on its own
    /// connection does not block readers on another at all, and a reader sees
    /// the last committed state rather than waiting for the write in flight.
    ///
    /// Returned by value rather than stored: the job owns it for exactly as
    /// long as the job runs, and dropping it closes the connection. Nothing
    /// about `with_index`'s contract changes.
    pub fn open_job_index(&self) -> Result<Db, Error> {
        Ok(mnema_index::open(&crate::paths::index_path(
            &self.data_dir,
        ))?)
    }

    /// Runs `f` against the open index.
    ///
    /// A closure rather than a getter because the connection is behind a lock:
    /// handing out a guard would let a caller hold it across an await point or a
    /// whole command, and the indexing job is on the other side of that lock.
    pub fn with_index<T>(
        &self,
        f: impl FnOnce(&Db) -> Result<T, mnema_index::Error>,
    ) -> Result<T, Error> {
        let guard = self.db.lock().map_err(|_| Error::StatePoisoned)?;
        let db = guard.as_ref().ok_or(Error::IndexNotOpen)?;
        Ok(f(db)?)
    }

    /// Takes the single job slot, or reports that it is already taken.
    ///
    /// One job at a time is the model. PDF extraction is serialised within the
    /// process (D35) and the index takes one writer, so a second job would spend
    /// its life waiting on both.
    pub fn claim_job(&self) -> Result<JobSlot, Error> {
        self.running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::JobAlreadyRunning)?;
        // Cleared only once the slot is ours: doing it earlier would clear a
        // cancellation aimed at the job that is still running.
        self.cancel.store(false, Ordering::SeqCst);
        Ok(JobSlot {
            running: self.running.clone(),
            cancel: self.cancel.clone(),
        })
    }

    /// Asks the running job to stop. A no-op when nothing is running.
    pub fn cancel_job(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Whether the job slot is taken.
    ///
    /// Reaches the window through the `job_status` command, which is what a page
    /// that reloaded mid-job asks to find out whether it should be drawing one.
    pub fn job_is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
}

/// Proof that the caller holds the job slot, and the thing that gives it back.
///
/// Released on drop rather than by an explicit call, so a job that panics
/// half-way through still frees the slot instead of locking the application out
/// of indexing until it is restarted.
pub struct JobSlot {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

impl JobSlot {
    pub fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }
}

impl Drop for JobSlot {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
    }
}
