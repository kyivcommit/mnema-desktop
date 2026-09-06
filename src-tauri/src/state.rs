use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mnema_index::Db;

use crate::error::Error;

/// That the job slot has changed hands — a signal to go and look, carrying
/// NOTHING about which way it went. Named, rather than written out at each of
/// the three places it appears, because those three have to agree — the field,
/// the setter's argument and [`JobSlot`]'s own copy.
///
/// 🔴 **It carried a `bool` and could not.** [`JobSlot::drop`] writes the
/// ending before it announces, so an incoming job can claim the slot in between
/// and announce its own start first: the two announcements then arrive `true`,
/// `false` while the slot is HELD, and a consumer replaying the last edge
/// disables «Зупинити сканування» for the whole of a job that is running. The
/// walk → embed handoff is exactly that sequence. Every consumer must therefore
/// read [`AppState::scan_state`] at the moment it acts, which is what
/// [`crate::tray::refresh_tray`] already does for the same reason and what
/// `state::tests::an_announcement_is_read_as_the_fact_not_replayed_as_the_edge`
/// pins. With no boolean to replay, the ordering of two announcements cannot
/// decide what the tray ends up saying.
pub type JobObserver = dyn Fn() + Send + Sync;

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
    /// What the application is doing, and what the jobs before it left behind.
    ///
    /// The single job slot is the `Running` arm of
    /// [`crate::scan_state::ScanSnapshot`] and not a boolean beside it, which is
    /// what makes "the slot is taken" and "this is what it is taken for" one
    /// fact rather than two that can disagree. An `Arc` because [`JobSlot`]
    /// holds a clone for the life of the job and holds no reference back to this
    /// struct.
    ///
    /// Separate from `cancel`, which is about a job that is already running and
    /// stays an atomic: `mnema_ingest::walk_root` takes a `&AtomicBool`, and a
    /// job asks it in a tight loop where a mutex has nothing to offer.
    scan: Arc<Mutex<crate::scan_state::ScanState>>,
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
    /// Who to tell when the job slot changes hands, if anybody asked.
    ///
    /// An `Arc` rather than the `Box` [`AppState::set_job_observer`] takes,
    /// because [`JobSlot`] carries its own clone for the life of the job:
    /// `Box<dyn Fn(…)>` is not `Clone`, and looking the observer up again at
    /// drop time would need a reference back to this struct that `JobSlot`
    /// deliberately does not hold. `None` is the default and stays the default
    /// under `cargo test` — nothing but `.setup` installs one.
    job_observer: Mutex<Option<Arc<JobObserver>>>,
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
            scan: Arc::new(Mutex::new(crate::scan_state::ScanState::default())),
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
            job_observer: Mutex::new(None),
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
    /// (`lib.rs:198`) rather than only from an `if let Err(e)`, so it always
    /// writes a definite answer, success included, instead of writing only on
    /// failure and leaving a caller to remember to clear the field on the other
    /// path. `boot_index` (`lib.rs:198`) is this setter's only caller today, run
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

    /// Takes the single job slot for `initial`, or reports that it is already
    /// taken.
    ///
    /// One job at a time is the model. PDF extraction is serialised within the
    /// process (D35) and the index takes one writer, so a second job would spend
    /// its life waiting on both.
    ///
    /// The phase is an argument rather than something the job sets afterwards
    /// because the announcement fires before this returns: an observer that
    /// looked between a claim and a first `update` would find a job running with
    /// nothing to say about it, and the strip it draws would flicker through a
    /// state no job was ever in.
    ///
    /// `cancellable` is fixed here for the life of the job — see
    /// [`crate::scan_state::ScanSnapshot::Running`]'s own field for why it sits
    /// beside the phase rather than inside it.
    pub fn claim_job(
        &self,
        initial: crate::scan_state::Phase,
        cancellable: bool,
    ) -> Result<JobSlot, Error> {
        {
            let mut scan = self
                .scan
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // Check and write under ONE lock. Two — a `job_is_running()` and
            // then a write — is check-then-act, and two callers can both pass
            // the check.
            if matches!(
                scan.snapshot,
                crate::scan_state::ScanSnapshot::Running { .. }
            ) {
                return Err(Error::JobAlreadyRunning);
            }
            scan.snapshot = crate::scan_state::ScanSnapshot::Running {
                phase: initial,
                cancellable,
            };
            scan.revision += 1;
            // `files`, `read_seq` and `last_reading` are deliberately untouched:
            // they are what the jobs before this one left behind, and a claim
            // that cleared them would blank a reopened window's only account of
            // the scan that just ran.
        }
        // Cleared only once the slot is ours: doing it earlier would clear a
        // cancellation aimed at the job that is still running.
        self.cancel.store(false, Ordering::SeqCst);
        // After the early return above, so a refused claim announces nothing:
        // the caller that lost never held the slot, and an announcement from it
        // would enable a control on behalf of a job that does not exist. Cloned
        // out from under the lock before the call, so an observer is never run
        // while this mutex is held.
        let observer = self.observer();
        // The slot exists BEFORE anything is told the slot is taken. An observer
        // is free to act on the state the moment it hears — including claiming
        // and releasing it again — and the struct that gives the slot back must
        // be built by then, not after the announcement has returned.
        let slot = JobSlot {
            scan: self.scan.clone(),
            cancel: self.cancel.clone(),
            observer,
            cancellable,
            finished: false,
        };
        if let Some(f) = &slot.observer {
            f();
        }
        Ok(slot)
    }

    /// The whole of what the application is doing, copied out.
    ///
    /// By value rather than behind a guard, for the reason [`AppState::
    /// with_index`] gives about the index: a caller holding this lock across a
    /// command would be holding it against the job that writes to it.
    pub fn scan_state(&self) -> crate::scan_state::ScanState {
        self.scan
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Records how many files the index holds, without a job.
    ///
    /// The boot's way in: a window opened before anything has run still has to
    /// be told what the index already holds, and there is no
    /// [`JobSlot::finish`] to carry the number for it.
    pub fn set_files(&self, files: i64) {
        {
            let mut scan = self
                .scan
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            scan.files = files;
            scan.revision += 1;
        }
        self.announce();
    }

    /// The observer as it stands, cloned out from under its own lock so that
    /// nothing calls it while any lock here is held.
    fn observer(&self) -> Option<Arc<JobObserver>> {
        self.job_observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Tells whoever asked to go and look. Call sites must already have released
    /// `scan`: an observer reads [`AppState::scan_state`], which takes that same
    /// lock.
    fn announce(&self) {
        if let Some(f) = self.observer() {
            f();
        }
    }

    /// Registers who to tell when the job slot changes hands. Installed once,
    /// from `.setup`, so that the tray can offer «Зупинити сканування» only
    /// while there is something to stop.
    ///
    /// Takes a `Box` and stores an `Arc`: the slot hands its holder a clone,
    /// and a `Box` cannot be cloned. Replaces any previous observer rather than
    /// accumulating a list, because there is exactly one tray.
    pub fn set_job_observer(&self, f: Box<JobObserver>) {
        *self
            .job_observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::from(f));
    }

    /// Asks the running job to stop. A no-op when nothing is running.
    pub fn cancel_job(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Whether the job slot is taken.
    ///
    /// The narrow question, for the callers that only have a narrow thing to do
    /// with the answer — the tray's Stop item is either enabled or it is not.
    /// The window is handed the whole of [`AppState::scan_state`] through the
    /// `job_status` command instead, because "a job is running" is not enough to
    /// draw one.
    pub fn job_is_running(&self) -> bool {
        matches!(
            self.scan
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .snapshot,
            crate::scan_state::ScanSnapshot::Running { .. }
        )
    }
}

/// Proof that the caller holds the job slot, and the thing that gives it back.
///
/// Released on drop rather than by an explicit call, so a job that panics
/// half-way through still frees the slot instead of locking the application out
/// of indexing until it is restarted.
pub struct JobSlot {
    /// The same `Arc` [`AppState`] holds, so a write through this slot IS a
    /// write to the application's state and not a copy of it that has to be
    /// reconciled later.
    scan: Arc<Mutex<crate::scan_state::ScanState>>,
    cancel: Arc<AtomicBool>,
    /// The observer as it stood when this slot was claimed, carried rather than
    /// looked up: this struct holds no reference to [`AppState`], and giving it
    /// one to reach a field at drop time would be a larger change than a clone
    /// of an `Arc`. `None` when nobody registered, which is what keeps every
    /// existing caller silent.
    observer: Option<Arc<JobObserver>>,
    /// Carried rather than re-read out of the snapshot on every `update`. It
    /// cannot change for the life of the job, and re-reading it would need an
    /// arm for "the snapshot is not `Running`" — a state this slot's own
    /// existence rules out, and so an arm no test could ever reach.
    cancellable: bool,
    /// Whether [`JobSlot::finish`] has already written an ending. It is what
    /// stops [`JobSlot::drop`] from overwriting the report the job just wrote
    /// with the one nobody wrote.
    finished: bool,
}

impl JobSlot {
    pub fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }

    /// Replaces what the running job says it is doing.
    ///
    /// Touches neither `read_seq` nor `last_reading`: a progress tick is not a
    /// reading pass ending, and a consumer watching for the second must not be
    /// woken by the first.
    pub fn update(&self, phase: crate::scan_state::Phase) {
        {
            let mut scan = self
                .scan
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            scan.snapshot = crate::scan_state::ScanSnapshot::Running {
                phase,
                cancellable: self.cancellable,
            };
            scan.revision += 1;
        }
        self.announce();
    }

    /// Records that a reading pass has ended, and what it concluded.
    ///
    /// Called once per reading pass, by the pass, as it ends — not by
    /// [`JobSlot::finish`], because a job may read several folders and each of
    /// them is a pass that a consumer has to be able to act on before the job is
    /// over.
    ///
    /// 🔴 **One write, under one lock.** `last_reading` and `read_seq` are two
    /// halves of one fact: a consumer that saw the counter move and then read a
    /// `last_reading` belonging to the pass before it would attribute one pass's
    /// conclusions to another. Splitting this into two locked writes is what
    /// would make that window exist.
    pub fn mark_reading_done(&self, outcome: crate::scan_state::ReadingOutcome) {
        {
            let mut scan = self
                .scan
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            scan.last_reading = Some(outcome);
            scan.read_seq += 1;
            scan.revision += 1;
        }
        self.announce();
    }

    /// Gives the slot back with something to say for the job.
    ///
    /// Takes `self`, so the slot cannot be used afterwards and the ending cannot
    /// be written twice. `files` is `Option` because only a job that counted
    /// them has a number: `None` leaves the last count standing, which is what
    /// a probe and a cancelled pass owe the window.
    pub fn finish(mut self, terminal: crate::scan_state::Terminal, files: Option<i64>) {
        {
            let mut scan = self
                .scan
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            scan.snapshot = match terminal {
                crate::scan_state::Terminal::Idle => crate::scan_state::ScanSnapshot::Idle,
                crate::scan_state::Terminal::Ended { report } => {
                    crate::scan_state::ScanSnapshot::Ended { report }
                }
            };
            if let Some(files) = files {
                scan.files = files;
            }
            scan.revision += 1;
        }
        // BEFORE the announcement, not after. An observer may claim the slot the
        // moment it hears — the walk → embed handoff does exactly that — and
        // this `self` is dropped after the announcement returns. A `Drop` that
        // still thought it owed an ending would then write one over the job that
        // had already started.
        self.finished = true;
        self.announce();
    }

    /// Tells whoever asked to go and look. Call sites must already have released
    /// `scan`, for the reason [`AppState::announce`] gives.
    fn announce(&self) {
        if let Some(f) = &self.observer {
            f();
        }
    }
}

impl Drop for JobSlot {
    /// 🔴 **A job that vanished is not a job that finished.**
    ///
    /// A panic, an early return, or a thread that simply ends leaves the slot to
    /// this, and this has no report to write. Writing `Idle` for a scan would
    /// tell the user the folder was read to the end when it was not: an idle
    /// window over an index missing whatever the job never reached, and nothing
    /// left to correct it. So the two phases that owe a report get one saying
    /// exactly what happened — that nobody wrote it — and the phases that owe
    /// none go back to idle.
    ///
    /// The ending is written BEFORE the announcement, not after: an observer
    /// reads the state for itself and must find the truth this drop has already
    /// made. It is also what lets an incoming job claim the slot between the
    /// write and the announcement — the handoff whose ordering is the reason
    /// nothing is passed to an observer (see [`JobObserver`]). This runs on the
    /// job's own thread, which is why the installed observer hands the work to
    /// the main thread and returns rather than blocking here — see
    /// `tray::refresh_tray`.
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Which phase it vanished from, not merely whether it owed anything:
        // the report says where the job was, and the two phases are resumed
        // from different places — see `scan_job::resume_for`. The note sits
        // above the block rather than beside the binding it is about, because
        // `scripts/mutations/pr9-shell.sh` quotes the lock and that binding as
        // one adjacent block; a comment between them leaves the case matching
        // nothing, and a case that matches nothing proves nothing while still
        // reporting green.
        {
            let mut scan = self
                .scan
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let owed_a_report = match scan.snapshot {
                crate::scan_state::ScanSnapshot::Running {
                    phase: crate::scan_state::Phase::Reading { .. },
                    ..
                } => Some(crate::scan_state::EndedIn::Reading),
                crate::scan_state::ScanSnapshot::Running {
                    phase: crate::scan_state::Phase::Embedding { .. },
                    ..
                } => Some(crate::scan_state::EndedIn::Embedding),
                _ => None,
            };
            scan.snapshot = if let Some(ended_in) = owed_a_report {
                crate::scan_state::ScanSnapshot::Ended {
                    report: crate::scan_state::ScanReport {
                        // ⚠️ `NotReached` even for a job that vanished DURING
                        // the embedding phase, and that is the conservative
                        // direction rather than an accurate one: nothing here
                        // knows how far that pass got, and the two ways to be
                        // wrong are not symmetric. Under-claiming leaves a
                        // person told nothing was embedded when some of it was,
                        // and `resume` below sends them back to finish it;
                        // over-claiming would tell them their archive is
                        // searchable when it is not.
                        embedding: crate::scan_state::EmbedOutcome::NotReached,
                        ended_in,
                        reason: crate::job::EndReason::Failed,
                        // English, and here rather than in `locale.rs`, for the
                        // reason every sentence in `error.rs` is: it is a
                        // diagnostic about a defect, not a sentence written for
                        // a person to act on.
                        message: Some("the job ended without a report".to_string()),
                        resume: crate::scan_job::resume_for(
                            crate::job::EndReason::Failed,
                            ended_in,
                        ),
                    },
                }
            } else {
                crate::scan_state::ScanSnapshot::Idle
            };
            scan.revision += 1;
        }
        self.announce();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::job::{EndReason, Progress};
    use crate::scan_state::{
        EmbedOutcome, EndedIn, OtherJob, Phase, ReadingOutcome, ScanReport, ScanSnapshot,
        ScanState, Terminal,
    };

    /// An `AppState` with nothing but paths — the observer contract touches no
    /// index, no provider and no credential store, so the four arguments are
    /// only there to satisfy the constructor.
    ///
    /// In an `Arc` because an observer that records what it READ has to be able
    /// to ask this state; it holds a `Weak`, so the state does not own an
    /// observer that owns the state back.
    fn state() -> Arc<AppState> {
        Arc::new(AppState::new(
            PathBuf::from("/nonexistent/mnema-observer-test"),
            PathBuf::from("/nonexistent/mnema-observer-worker"),
            "http://127.0.0.1:0".to_string(),
            "mnema-desktop-state-observer-test".to_string(),
        ))
    }

    /// A phase for the tests whose subject is the slot rather than the work:
    /// [`Phase::Other`] is the one phase whose ending owes the user no report,
    /// so a test that uses it is asserting about the protocol and not about the
    /// Drop policy.
    fn probe() -> Phase {
        Phase::Other {
            job: OtherJob::Probe,
        }
    }

    /// A reading phase with nothing read yet, which is what
    /// [`AppState::claim_job`]'s callers pass at the moment they claim.
    fn reading() -> Phase {
        Phase::Reading {
            root_index: 0,
            root_count: 1,
            root_path: "/nonexistent/mnema-reading-root".to_string(),
            counts: Progress::default(),
        }
    }

    /// A reading outcome with something in every field the slot has to carry
    /// unchanged, so that a slot storing a blank one — or the one from the pass
    /// before — is a failure rather than an equality between two defaults.
    fn a_pass_that_read_one_folder() -> ReadingOutcome {
        ReadingOutcome {
            reason: EndReason::VolumeMissing,
            complete: false,
            roots_read: 1,
            root_count: 3,
            done: 4,
            total: 5,
            indexed: 2,
            unchanged: 1,
            skipped: 1,
            removed: 6,
            contended: 1,
            roots: Vec::new(),
        }
    }

    /// What [`JobSlot::drop`] writes for a phase that owed a report and never
    /// produced one. Written out here so that the Drop tests compare against
    /// one spelling rather than several.
    ///
    /// Takes the phase it vanished from, because the report names it: a job
    /// that disappeared out of the reading pass and one that disappeared out of
    /// the embedding pass are resumed from different places, and a fixture that
    /// spelled one answer for both would let the two swap without a test
    /// noticing.
    fn report_nobody_wrote(ended_in: EndedIn) -> ScanReport {
        ScanReport {
            embedding: EmbedOutcome::NotReached,
            ended_in,
            reason: EndReason::Failed,
            message: Some("the job ended without a report".to_string()),
            resume: crate::scan_job::resume_for(EndReason::Failed, ended_in),
        }
    }

    /// What the observer FOUND when it looked, once per announcement and in
    /// order, so a test can say what was there AND that nothing else was
    /// announced.
    ///
    /// It records a read rather than an argument because there is no argument:
    /// see [`JobObserver`] for the handoff that took it away. It records the
    /// whole [`ScanState`] rather than only the snapshot because `files` and
    /// `revision` change without the snapshot changing at all, and an
    /// announcement about one of those is still an announcement.
    type Log = Arc<Mutex<Vec<ScanState>>>;

    /// Poison-recovering rather than `unwrap`, and not for tidiness: a failed
    /// assertion here poisons the log while `JobSlot` is still alive, and the
    /// notification that `Drop` then fires as the stack unwinds would panic a
    /// second time inside a panic. The test would still fail — with a
    /// double-panic abort instead of the assertion that found the defect.
    fn recorder(log: &Log, state: &Arc<AppState>) -> Box<JobObserver> {
        let sink = log.clone();
        let weak = Arc::downgrade(state);
        Box::new(move || {
            let seen = weak
                .upgrade()
                .expect("the state outlives its own observer")
                .scan_state();
            sink.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(seen)
        })
    }

    fn snapshots(log: &Log) -> Vec<ScanSnapshot> {
        log.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|state| state.snapshot.clone())
            .collect()
    }

    /// Every announcement in order, whole. `snapshots` and `announced_files`
    /// below are projections of this; a test that has to say "and nothing was
    /// announced between these two" needs the states themselves.
    fn announced(log: &Log) -> Vec<ScanState> {
        log.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn announced_files(log: &Log) -> Vec<i64> {
        log.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|state| state.files)
            .collect()
    }

    /// The whole contract in the order a job lives it: a look after the claim
    /// finds the slot taken AND finds what it was taken for, and a look after
    /// it is given back finds it free. Both announcements, because an
    /// implementation that fired only the first would leave the tray offering
    /// to stop a job that has finished — which is the failure this observer
    /// exists to prevent, not a hypothetical.
    ///
    /// The pair of states it separates is "the snapshot was written, then the
    /// observer was told" against "the observer was told, then the snapshot was
    /// written": the observer READS `scan_state()` rather than being handed
    /// anything, so an announcement that fires before the write records the
    /// previous snapshot and the assertion below fails on the value.
    #[test]
    fn the_observer_hears_a_job_start_and_finish() {
        let log = Log::default();
        let state = state();
        state.set_job_observer(recorder(&log, &state));

        let slot = state.claim_job(probe(), true).expect("the slot is free");
        assert_eq!(
            snapshots(&log),
            [ScanSnapshot::Running {
                phase: probe(),
                cancellable: true,
            }],
            "claiming must announce itself, and the announcement must find the \
             phase the job was claimed for"
        );

        slot.finish(Terminal::Idle, None);
        assert_eq!(
            snapshots(&log),
            [
                ScanSnapshot::Running {
                    phase: probe(),
                    cancellable: true,
                },
                ScanSnapshot::Idle
            ],
            "releasing must announce itself too"
        );
    }

    /// A refused claim announces NOTHING and changes nothing. This is the
    /// assertion that separates "notify when the slot changes hands" from
    /// "notify on every call": the second caller never held the slot, so an
    /// announcement from it would enable a control on behalf of a job that does
    /// not exist, and the first job's own `Drop` would then be the only thing
    /// left to disable it.
    ///
    /// `revision` is asserted in both directions — it grows on the claim that
    /// was accepted and stands still on the one that was refused — because a
    /// `revision` that never moved at all would satisfy the second half on its
    /// own while making every consumer of the snapshot blind.
    #[test]
    fn a_refused_claim_announces_nothing() {
        let log = Log::default();
        let state = state();
        state.set_job_observer(recorder(&log, &state));

        let idle = state.scan_state().revision;
        let held = state.claim_job(probe(), true).expect("the slot is free");
        let claimed = state.scan_state().revision;
        assert!(
            claimed > idle,
            "an accepted claim is a change, and a consumer polling `revision` \
             has to be able to see it"
        );

        assert!(
            state.claim_job(reading(), true).is_err(),
            "a second claim must be refused while the first is held"
        );
        assert_eq!(
            state.scan_state().revision,
            claimed,
            "the refusal changed nothing, so it must not look like a change"
        );
        assert_eq!(
            state.scan_state().snapshot,
            ScanSnapshot::Running {
                phase: probe(),
                cancellable: true,
            },
            "the refused claim must not have overwritten the running job's phase"
        );
        assert_eq!(
            snapshots(&log),
            [ScanSnapshot::Running {
                phase: probe(),
                cancellable: true,
            }],
            "the refusal must not have announced anything"
        );

        // And the still-held slot is the one that speaks on the way out: one
        // announcement, not two.
        drop(held);
        assert_eq!(
            snapshots(&log),
            [
                ScanSnapshot::Running {
                    phase: probe(),
                    cancellable: true,
                },
                ScanSnapshot::Idle
            ]
        );
    }

    /// 🔴 **The walk → embed handoff, with the outgoing job announcing LAST.**
    ///
    /// `JobSlot::drop` writes the ending and only then announces, so between
    /// those two an incoming job can claim the slot and announce its own start.
    /// The announcements then arrive `running`, `running`, `idle` while the slot
    /// is HELD, and a consumer that replays the last edge disables «Зупинити
    /// сканування» for the whole of a job that is running — the tray then offers
    /// no way to stop a scan until the next job boundary.
    ///
    /// The interleaving is driven from inside the second announcement rather
    /// than from a second thread on purpose: what makes the window reachable is
    /// the ORDER of the two announcements, and the write that precedes them is
    /// what lets the incoming claim win. A thread would add a schedule to wait
    /// on and prove the same thing.
    ///
    /// What the fake records is what it READ, never what it was passed. That is
    /// the whole assertion: an announcement is a signal to go and look, and the
    /// fact a look finds after the handoff is that a job is running.
    #[test]
    fn an_announcement_is_read_as_the_fact_not_replayed_as_the_edge() {
        let log: Arc<Mutex<Vec<bool>>> = Arc::default();
        let state = Arc::new(state());
        // Where the incoming job's slot is parked so that it is still HELD when
        // the outgoing announcement is read. Dropping it inside the observer
        // would put the fact back to `false` and the fixture would pass for the
        // wrong reason.
        let incoming: Arc<Mutex<Option<JobSlot>>> = Arc::default();
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sink = log.clone();
        let held = incoming.clone();
        let counter = seen.clone();
        // A `Weak`, so the observer the state owns does not own the state back.
        let weak = Arc::downgrade(&state);
        state.set_job_observer(Box::new(move || {
            let st = weak.upgrade().expect("the state outlives its own observer");
            // Announcement 1 (0-based) is the outgoing job's `Drop`: the slot is
            // already free here, which is what lets the incoming claim win.
            if counter.fetch_add(1, Ordering::SeqCst) == 1 {
                *held
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(
                    st.claim_job(probe(), true)
                        .expect("the outgoing slot is free before it announces"),
                );
            }
            sink.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(st.job_is_running());
        }));

        let outgoing = state.claim_job(probe(), true).expect("the slot is free");
        drop(outgoing);

        assert_eq!(
            *log.lock().unwrap(),
            [true, true, true],
            "the outgoing job announced last, and what a look finds then is the \
             incoming job: replaying the edge disables Stop for a running job"
        );
        assert!(
            state.job_is_running(),
            "the incoming job still holds the slot"
        );

        // The other direction, and the reason the incoming slot was parked: when
        // it is finally given back the fact is false, and an announcement that
        // said nothing but `true` would be no better than one that said nothing
        // but `false`.
        drop(
            incoming
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take(),
        );
        assert_eq!(*log.lock().unwrap(), [true, true, true, false]);
        assert!(!state.job_is_running());
    }

    /// Silence is the default, and it has to be: seven of `AppState::new`'s
    /// call sites are tests that install no observer, and the shipped
    /// application runs without one until `.setup` reaches the tray.
    #[test]
    fn a_state_with_no_observer_claims_and_releases_as_before() {
        let state = state();
        let slot = state.claim_job(probe(), true).expect("the slot is free");
        assert!(state.job_is_running());
        assert!(state.claim_job(probe(), true).is_err());
        drop(slot);
        assert!(!state.job_is_running());
        state
            .claim_job(probe(), true)
            .expect("the slot is free again");
    }

    /// 🔴 **A reading job that vanished against a reading job that reported.**
    ///
    /// A panic, a `?` on the way out, or a thread that simply ends leaves the
    /// slot to `Drop`, and `Drop` has nothing to report. Writing `Idle` there
    /// would say the scan finished — the user would be shown an idle window
    /// over a folder that was never read to the end. The ending is written
    /// instead, as the failure it is, and only for the two phases that owe a
    /// report.
    ///
    /// The other direction is the same fixture with `finish` called: the report
    /// the job wrote survives, which is what says `Drop` did not overwrite it
    /// afterwards.
    #[test]
    fn a_reading_job_that_vanished_ends_with_the_report_nobody_wrote() {
        let state = state();
        drop(state.claim_job(reading(), true).expect("the slot is free"));
        assert_eq!(
            state.scan_state().snapshot,
            ScanSnapshot::Ended {
                report: report_nobody_wrote(EndedIn::Reading),
            },
            "a reading job that ended without a report is a failure, not an idle \
             application"
        );

        let slot = state.claim_job(reading(), true).expect("the slot is free");
        slot.finish(
            Terminal::Ended {
                report: ScanReport::default(),
            },
            None,
        );
        assert_eq!(
            state.scan_state().snapshot,
            ScanSnapshot::Ended {
                report: ScanReport::default(),
            },
            "the report the job wrote must survive its own slot being dropped"
        );
    }

    /// The embedding half of the pair above: the phase differs, the obligation
    /// does not. An embedding pass that vanished has left chunks unembedded,
    /// and vector search answers for fewer of them than the window shows.
    #[test]
    fn an_embedding_job_that_vanished_ends_with_the_report_nobody_wrote() {
        let state = state();
        drop(
            state
                .claim_job(
                    Phase::Embedding {
                        counts: Progress::default(),
                    },
                    true,
                )
                .expect("the slot is free"),
        );
        assert_eq!(
            state.scan_state().snapshot,
            ScanSnapshot::Ended {
                report: report_nobody_wrote(EndedIn::Embedding),
            }
        );
        state
            .claim_job(probe(), true)
            .expect("the slot is free again");
    }

    /// The other side of the Drop policy, and the reason it is a policy rather
    /// than one branch: a probe and a model adoption owe the user no report at
    /// all, so an ending written for them would put a failure on screen that
    /// belongs to nothing the user asked for.
    ///
    /// Task 11a (debt sweep). `claim_job` and `drop` used to be checked only
    /// through the state AFTER the drop, dropped in the very statement that
    /// claimed it (`drop(state.claim_job(...).expect(...))`) — so this test
    /// never looked at what the slot held while it was still alive. A `Drop`
    /// that did nothing at all, from a phase `claim_job` never actually wrote
    /// as `Running` in the first place, would leave the same `Idle` this test
    /// checks for: it would be proving Drop's policy by leaning on a claim it
    /// never itself confirmed. Each phase is now claimed into a binding, its
    /// running state asserted, and only then dropped — so this test stands on
    /// its own precondition instead of a neighbour's.
    #[test]
    fn a_job_with_nothing_to_report_goes_back_to_idle_when_it_vanishes() {
        let state = state();
        let probe_slot = state.claim_job(probe(), true).expect("the slot is free");
        assert!(
            state.job_is_running(),
            "the probe must actually be running before it vanishes, or dropping it proves \
             nothing about the Drop policy"
        );
        drop(probe_slot);
        assert_eq!(
            state.scan_state().snapshot,
            ScanSnapshot::Idle,
            "a probe that vanished is not a scan that failed"
        );

        let adoption_slot = state
            .claim_job(
                Phase::Other {
                    job: OtherJob::ModelAdoption,
                },
                false,
            )
            .expect("the slot is free again");
        assert!(
            state.job_is_running(),
            "the model adoption must actually be running before it vanishes, or dropping it \
             proves nothing about the Drop policy"
        );
        drop(adoption_slot);
        assert_eq!(
            state.scan_state().snapshot,
            ScanSnapshot::Idle,
            "a model adoption that vanished is not a scan that failed"
        );
    }

    /// Removal is the third phase with nothing to report: it is not a scan, and
    /// a folder taken out of the index that fails half-way is reported by the
    /// command that asked for it, which has a caller to answer.
    ///
    /// Task 11a (debt sweep): the same fix as the pair above, and for the same
    /// reason — the slot is claimed into a binding and its running state
    /// asserted before it is dropped, so the `Idle` this test checks for is
    /// proven to be `Drop`'s own transition rather than a state the removal
    /// was never actually claimed into.
    #[test]
    fn a_removal_that_vanished_goes_back_to_idle() {
        let state = state();
        let slot = state
            .claim_job(
                Phase::Removing {
                    root_path: "/nonexistent/mnema-removed-root".to_string(),
                },
                false,
            )
            .expect("the slot is free");
        assert!(
            state.job_is_running(),
            "the removal must actually be running before it vanishes, or dropping it proves \
             nothing about the Drop policy"
        );
        drop(slot);
        assert_eq!(
            state.scan_state().snapshot,
            ScanSnapshot::Idle,
            "a removal that vanished is not a scan that failed"
        );
        state
            .claim_job(probe(), true)
            .expect("the slot is free again");
    }

    /// 🔴 **`read_seq` counts reading passes that ENDED, and nothing else.**
    ///
    /// The pair it separates is "a reading pass has finished since I last
    /// looked" against "something about the job changed": a consumer that
    /// re-reads the index when `read_seq` moves would re-read it on every
    /// progress tick if `update` moved it too, and would never re-read it at
    /// all if `mark_reading_done` did not.
    ///
    /// The second pair, asserted after `finish`: `last_reading` survives the job
    /// that recorded it and the next claim. A `claim_job` that cleared it leaves
    /// a reopened window with no answer about the scan that just ran.
    ///
    /// The outcome recorded is a filled one rather than the default, so the
    /// transition pinned is not merely `None` to `Some`: a slot that stored a
    /// blank outcome, or the one from a pass before it, fails here too.
    #[test]
    fn only_a_finished_reading_pass_moves_read_seq() {
        let state = state();
        assert_eq!(state.scan_state().read_seq, 0);
        assert_eq!(state.scan_state().last_reading, None);

        let slot = state.claim_job(reading(), true).expect("the slot is free");
        let claimed = state.scan_state();
        assert_eq!(
            claimed.read_seq, 0,
            "claiming the slot is not a reading pass ending"
        );
        assert_eq!(claimed.last_reading, None);

        slot.update(reading());
        assert_eq!(
            state.scan_state().read_seq,
            0,
            "progress inside a reading pass is not the pass ending"
        );

        slot.mark_reading_done(a_pass_that_read_one_folder());
        let done = state.scan_state();
        assert_eq!(
            done.read_seq, 1,
            "the pass ended, which is the one thing that moves this"
        );
        assert_eq!(done.last_reading, Some(a_pass_that_read_one_folder()));
        assert!(
            done.revision > claimed.revision,
            "a reading pass ending is a change to the snapshot too"
        );

        slot.finish(Terminal::Idle, None);
        state.set_files(9);
        let later = state.scan_state();
        assert_eq!(
            later.read_seq, 1,
            "neither the job ending nor a file count is a reading pass"
        );
        assert_eq!(later.last_reading, Some(a_pass_that_read_one_folder()));

        let next = state.claim_job(reading(), true).expect("the slot is free");
        let after_next_claim = state.scan_state();
        assert_eq!(
            after_next_claim.read_seq, 1,
            "a new job does not un-read the pass before it"
        );
        assert_eq!(
            after_next_claim.last_reading,
            Some(a_pass_that_read_one_folder()),
            "a claim that cleared this leaves a reopened window with no answer \
             about the scan that just ran"
        );
        drop(next);
    }

    /// How many files the index holds is a fact about the index, not about the
    /// job that last counted them: it must outlive the job, and the next job's
    /// claim must not blank it. The pair separated is "no job has run yet" —
    /// where zero is the truth — from "a job ran and the count it left is still
    /// the truth", which a claim that reset the field would make
    /// indistinguishable.
    ///
    /// 🔴 **An ending moves `revision`, once, and is announced only after it is
    /// written.**
    ///
    /// The pair this separates is "the job ended" from "any surface can tell
    /// that it ended". `revision` is the ONLY thing that says one read of this
    /// state is newer than another (see the field's own doc), and
    /// `ui/src/settings/jobs.ts`'s `apply` keeps the higher one and DROPS
    /// everything else. So an ending written under the revision the last
    /// progress tick already carried is an ending the window throws away: the
    /// strip goes on drawing a running pass, Stop live, over a slot that is
    /// free — and nothing arrives later to correct it, because the job that
    /// would have announced again has gone.
    ///
    /// Nothing else in this file could see that. The snapshot really does
    /// change here, so every assertion phrased on `snapshot` alone — which is
    /// what the observer tests are — passes against an ending that never moved
    /// the counter. The neighbouring tests pin the bump on `claim_job`
    /// (`a_refused_claim_announces_nothing`), on `mark_reading_done`
    /// (`only_a_finished_reading_pass_moves_read_seq`) and on `set_files`
    /// (below); `finish` was the one write on this type that had none.
    ///
    /// **All four shapes `finish` has**, because the bump sits under one of
    /// them and above the other: `Terminal::Idle` and `Terminal::Ended`, each
    /// with a file count and without one. A bump written inside the `if let
    /// Some(files)` arm answers only half of them, and a fixture that always
    /// passed a count would call that correct.
    ///
    /// "Exactly one announcement" and "after the write" come from the same
    /// place, and that is why `Log` records the whole [`ScanState`] rather than
    /// its snapshot: one new entry per `finish` says it announced once, and
    /// that entry carrying the NEW revision says the write came first. An
    /// announcement fired ahead of the write would record the old number while
    /// leaving the final state correct.
    #[test]
    fn an_ending_moves_the_revision_once_and_is_announced_after_it_is_written() {
        for (which, terminal, files) in [
            ("idle, no count", Terminal::Idle, None),
            ("idle, with a count", Terminal::Idle, Some(11)),
            (
                "ended, no count",
                Terminal::Ended {
                    report: ScanReport::default(),
                },
                None,
            ),
            (
                "ended, with a count",
                Terminal::Ended {
                    report: ScanReport::default(),
                },
                Some(13),
            ),
        ] {
            let log = Log::default();
            let state = state();
            state.set_job_observer(recorder(&log, &state));

            let slot = state.claim_job(probe(), true).expect("the slot is free");
            let before = state.scan_state().revision;
            let already_announced = announced(&log).len();

            slot.finish(terminal, files);

            let settled = state.scan_state();
            assert_eq!(
                settled.revision,
                before + 1,
                "({which}) the ending did not move `revision`, so `jobs.ts`'s \
                 `apply` would drop it and the window would go on drawing the \
                 run that has just finished"
            );

            let since = announced(&log).split_off(already_announced);
            assert_eq!(
                since.len(),
                1,
                "({which}) an ending announces exactly once: {since:?}"
            );
            assert_eq!(
                since[0].revision, settled.revision,
                "({which}) the observer looked and found the revision the \
                 ending had not written yet, so it was told before the write"
            );
            assert_eq!(
                since[0].snapshot, settled.snapshot,
                "({which}) the observer found a snapshot the ending had not \
                 written yet"
            );
            assert_eq!(
                since[0].files, settled.files,
                "({which}) the observer found a file count the ending had not \
                 written yet"
            );
        }
    }

    /// `set_files` is the boot's way in, and it announces: a window opened
    /// before any job runs still has to be told what the index holds.
    #[test]
    fn the_file_count_outlives_the_job_that_counted_it() {
        let log = Log::default();
        let state = state();
        state.set_job_observer(recorder(&log, &state));
        assert_eq!(state.scan_state().files, 0);

        let slot = state.claim_job(reading(), true).expect("the slot is free");
        slot.finish(
            Terminal::Ended {
                report: ScanReport::default(),
            },
            Some(42),
        );
        assert_eq!(state.scan_state().files, 42);

        let next = state.claim_job(probe(), true).expect("the slot is free");
        assert_eq!(
            state.scan_state().files,
            42,
            "a new job does not blank the count the last one left"
        );
        next.finish(Terminal::Idle, None);
        assert_eq!(
            state.scan_state().files,
            42,
            "an ending that counted nothing does not blank it either"
        );

        let before = state.scan_state().revision;
        state.set_files(7);
        assert_eq!(state.scan_state().files, 7);
        assert!(
            state.scan_state().revision > before,
            "a count nobody can see changed is a count nobody re-reads"
        );
        assert_eq!(
            announced_files(&log),
            [0, 42, 42, 42, 7],
            "every write to the snapshot announces itself, `set_files` included"
        );
    }
}
