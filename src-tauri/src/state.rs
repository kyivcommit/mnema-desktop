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
    /// What the start-up open answered, on the runs where what it answered was
    /// a failure.
    ///
    /// `db` is `None` both before anything has opened the index and after an
    /// open that failed — a failed `open_index` returns before it assigns — so
    /// `with_index` says `IndexNotOpen` in either case and
    /// [`crate::models::UnreadableCause`] folds the two into one value, which
    /// is what its own doc records. This field is the half that was missing.
    ///
    /// Only the boot's answer, and that is the whole distinction: every other
    /// caller of [`AppState::open_index`] is a command, and a command hands its
    /// rejection back to whoever asked. The boot has nobody to hand it to, so
    /// an error logged there is an error dropped, and the settings screen then
    /// draws a person whose index is broken the sentence written for the
    /// ordinary state at start-up.
    boot_open_error: Mutex<Option<String>>,
    /// Whether the single job slot is taken. Separate from `cancel`, which is
    /// about a job that is already running.
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    /// The interface locale (§D129): the persisted choice and what it resolves
    /// to. Set once at start-up by `resolve_effective` (Task 6) and again by
    /// `locale::apply_choice` on every change; read by `get_locale` and by
    /// tray/menu construction so both agree with what was last written.
    locale: Mutex<crate::locale::LocaleState>,
    /// The global shortcut, as the operating system last answered about it.
    ///
    /// Set once at start-up by [`crate::prefs::install_hotkey`] and again by
    /// the `set_hotkey` command. Not `Copy` — it carries a `String` and a
    /// failure's sentence — so the getter clones, which is the same trade every
    /// other getter here makes for the same reason: no caller holds this lock
    /// for the length of a command.
    hotkey: Mutex<crate::prefs::HotkeyState>,
    /// The two operating-system services, defaulted to inert.
    ///
    /// 🔴 **Installed rather than constructed** ([`AppState::install_os_services`]),
    /// and that is what keeps `new`'s four arguments at four: it has eight call
    /// sites and seven of them are tests that do not care which registrar is in
    /// place. It is also what makes "nothing under `cargo test` touches the real
    /// plugins" structural — see [`crate::os_services`]'s own header.
    shortcuts: Mutex<Box<dyn crate::os_services::ShortcutRegistrar>>,
    autolaunch: Mutex<Box<dyn crate::os_services::Autolaunch>>,
    /// Serialises a whole hotkey change against another one.
    ///
    /// Separate from `hotkey` above, and that is the point: `hotkey` is held for
    /// one read or one write, while this is held across the read, both
    /// operating-system calls, the store and the persist. Merging them would
    /// mean holding the state's own lock for the length of a command, which is
    /// what every other getter here exists to avoid.
    hotkey_change: Mutex<()>,
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
            boot_open_error: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            // Safe default; overwritten at startup by `resolve_effective`
            // before any window draws (Task 6).
            locale: Mutex::new(crate::locale::LocaleState {
                choice: crate::locale::LocaleChoice::Auto,
                effective: crate::locale::Lang::En,
            }),
            // 🔴 A STATED default, written here so that no test has to infer it
            // from another test. `.setup` calls `prefs::install_hotkey` before
            // any window exists, so the shipped application never shows this
            // value — the state a person can read is always the one an actual
            // registration produced. It is visible to `tests/commands.rs`,
            // where `app_in` never runs `.setup`, and four fixtures assert
            // against it. Changing this line changes what they assert.
            hotkey: Mutex::new(crate::prefs::HotkeyState {
                shortcut: crate::prefs::DEFAULT_HOTKEY.to_string(),
                status: crate::prefs::HotkeyStatus::Unavailable {
                    reason: "the shortcut has not been registered yet".to_string(),
                },
            }),
            shortcuts: Mutex::new(Box::new(crate::os_services::NoOsServices)),
            autolaunch: Mutex::new(Box::new(crate::os_services::NoOsServices)),
            hotkey_change: Mutex::new(()),
        }
    }

    /// Replaces the inert defaults with services that reach the operating
    /// system. Called once from `.setup` with the real plugin wrappers, and
    /// from the tests that want recording fakes.
    pub fn install_os_services(
        &self,
        shortcuts: Box<dyn crate::os_services::ShortcutRegistrar>,
        autolaunch: Box<dyn crate::os_services::Autolaunch>,
    ) {
        *self
            .shortcuts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = shortcuts;
        *self
            .autolaunch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = autolaunch;
    }

    /// Runs `f` against the installed shortcut registrar.
    ///
    /// A closure rather than a getter for the reason [`AppState::with_index`]
    /// gives: the value is behind a lock, and handing out a guard would let a
    /// caller hold it for a whole command.
    ///
    /// 🔴 **Why holding this lock across the blocking `register` is safe, stated
    /// as the argument that is actually true.** The real registrar's `register`
    /// blocks until the main thread services it, so a main thread waiting on
    /// this lock while a worker holds it and waits on the main thread would
    /// deadlock. It cannot happen today because **the only main-thread
    /// acquisitions are in `.setup`** — `install_os_services` and
    /// `install_hotkey`'s call to this method — and `.setup` runs before the
    /// event loop can dispatch any command, so no worker can be holding the
    /// lock at that moment. (An earlier draft of this comment said "the main
    /// thread never takes this lock", which the same commit contradicted twice.)
    ///
    /// ⚠️ **The obligation that follows: nothing on the main thread may take
    /// this lock after start-up.** A menu item, a tray callback or a window
    /// event that reaches `with_shortcuts` would be exactly the deadlock above,
    /// and it would be built under a comment saying it was impossible.
    pub fn with_shortcuts<T>(
        &self,
        f: impl FnOnce(&dyn crate::os_services::ShortcutRegistrar) -> T,
    ) -> T {
        let guard = self
            .shortcuts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(guard.as_ref())
    }

    /// Runs `f` against the installed autolaunch. Same shape and same reasoning
    /// as [`AppState::with_shortcuts`].
    pub fn with_autolaunch<T>(
        &self,
        f: impl FnOnce(&dyn crate::os_services::Autolaunch) -> T,
    ) -> T {
        let guard = self
            .autolaunch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(guard.as_ref())
    }

    /// A copy of the hotkey state. Same poison-recovery trade as
    /// [`AppState::locale`]: behind the lock is a struct of owned values with
    /// no invariant a panicking holder could have left half-built, and one
    /// wrong label is a smaller failure than losing the window over it.
    pub fn hotkey(&self) -> crate::prefs::HotkeyState {
        self.hotkey
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Takes the hotkey-change lock, held for a whole `set_hotkey` rather than
    /// for one read or one write. A guard rather than a closure because what it
    /// spans is the length of a command — which is the one thing every other
    /// lock here refuses to do, and the one thing this lock is for.
    ///
    /// Poison is recovered rather than propagated, the trade [`AppState::locale`]
    /// spells out: this guards no value at all, only an ordering, and a caller
    /// that panicked half-way through a change has left the state and the file
    /// exactly as consistent as they were — the state is written before the
    /// persist and each is one operation.
    pub fn lock_hotkey_change(&self) -> std::sync::MutexGuard<'_, ()> {
        self.hotkey_change
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Overwrites the hotkey state — from [`crate::prefs::install_hotkey`] at
    /// start-up and from `set_hotkey` on every change.
    pub fn set_hotkey_state(&self, s: crate::prefs::HotkeyState) {
        *self
            .hotkey
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = s;
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// A copy of the current locale state — `LocaleState` is `Copy`, so this
    /// hands back a value rather than a guard, the same reasoning as every
    /// other getter here: no caller holds this lock across an await point or a
    /// whole command.
    ///
    /// Recovers a poisoned lock instead of panicking (`into_inner` on the
    /// `PoisonError`, not `expect`): the guarded value is a plain `Copy`
    /// struct with no invariant a panicking holder could have left broken, and
    /// a wrong menu label is a smaller failure than losing the window over it
    /// — the same trade [`crate::error::Error::StatePoisoned`] documents for
    /// `db`, made explicit here instead of typed, because this getter's
    /// signature (matching the brief) returns `LocaleState`, not a `Result`.
    pub fn locale(&self) -> crate::locale::LocaleState {
        *self
            .locale
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Overwrites the locale state — called by `locale::apply_choice` after a
    /// successful write to prefs, and once at start-up by `resolve_effective`
    /// (Task 6). Same poison-recovery reasoning as [`AppState::locale`].
    pub fn set_locale_state(&self, s: crate::locale::LocaleState) {
        *self
            .locale
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = s;
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

    /// Records what the start-up open answered: the failure's own sentence, or
    /// `None` for a success.
    ///
    /// **Total, not conditional** — it is called with `outcome.err().map(...)`
    /// (`lib.rs:169`) rather than only from an `if let Err(e)`, so it always
    /// writes a definite answer, success included, instead of writing only on
    /// failure and leaving a caller to remember to clear the field on the other
    /// path. `boot_index` (`lib.rs:169`) is this setter's only caller today, run
    /// once per process, so nothing here actually recovers from an earlier
    /// failure — what the total shape buys is that a future second caller (a
    /// retry, a manual re-open exposed from the settings screen) cannot leave a
    /// stale failure from an earlier call standing after a later one succeeds,
    /// without that caller having to know to clear this field itself.
    ///
    /// Poison recovery instead of `expect`, the trade [`AppState::locale`]
    /// spells out: behind this lock is a `String` with no invariant a panicking
    /// holder could have left half-built, and losing the window over it is a
    /// larger failure than one wrong sentence on one screen.
    pub fn set_boot_open_error(&self, reason: Option<String>) {
        *self
            .boot_open_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = reason;
    }

    /// Why the start-up open failed, if it did. A clone rather than a guard,
    /// for the reason every other getter here hands back a value: no caller
    /// holds this lock for the length of a command.
    pub fn boot_open_error(&self) -> Option<String> {
        self.boot_open_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
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
    ///
    /// ⚠️ **Three ways out, and something downstream classifies them.**
    /// `StatePoisoned`, `IndexNotOpen`, and whatever `f` returns as
    /// `Error::Index(_)`. `models::UnreadableCause::of` sorts those three into
    /// what a settings window draws — "no index is open" against "a read
    /// failed, which is a bug report" — and it cannot be made to fail
    /// compilation when this list grows, because no `match` can express "the
    /// errors `with_index` produces". So the obligation sits here, on the
    /// function that can break it: **a fourth way out of this function owes
    /// that classification a decision.** Left unmade, a new failure is drawn to
    /// the user as a defect report whatever it actually is.
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
