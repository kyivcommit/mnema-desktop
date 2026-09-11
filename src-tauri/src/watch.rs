//! The folder watcher: `notify` events in, one `scan_job::start` out.
//!
//! Spec: docs 2026-09-10 (private). The walk is the source of truth; this
//! module only decides WHEN to run it again. Nothing here touches the index.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;
use std::time::Instant;

pub const QUIET: Duration = Duration::from_secs(2);
pub const MAX_WAIT: Duration = Duration::from_secs(30);
pub const POLL: Duration = Duration::from_secs(5);
pub const REWATCH: Duration = Duration::from_secs(60);

/// What the trigger thread waits on: the first and the last wake since the
/// last trigger. Two instants and nothing else — spec review P2-4 asked for
/// a bounded pending state instead of an unbounded queue, and P2-3 for a
/// cap so that a stream of wakes cannot starve the scan.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pending {
    pub first: Option<Instant>,
    pub last: Option<Instant>,
}

impl Pending {
    pub(crate) fn wake(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// `None`: nothing pending. `Some(ZERO)`: fire now. `Some(d)`: wait `d`.
    /// Fires when `QUIET` has passed since the last wake OR `MAX_WAIT` since
    /// the first; the sooner of the two.
    pub(crate) fn due(&self, now: Instant) -> Option<Duration> {
        let (first, last) = (self.first?, self.last?);
        let fire_at = (last + QUIET).min(first + MAX_WAIT);
        Some(fire_at.saturating_duration_since(now))
    }

    pub(crate) fn take(&mut self) -> Option<Instant> {
        let last = self.last;
        *self = Self::default();
        last
    }
}

/// One form for every path this module compares. Resolves symlinks in the
/// longest EXISTING prefix (so a path that was just deleted still resolves
/// through its parent), then on Windows strips the `\\?\C:` (verbatim disk)
/// prefix `canonicalize` adds and folds case. Measured: FSEvents on macOS delivers
/// `/private/var/…` for a root handed over as `/var/…`; Windows
/// `canonicalize` answers `\\?\C:\…` while notify builds event paths from
/// the plain `C:\…` it was given.
pub(crate) fn plain(p: &Path) -> PathBuf {
    let resolved = resolve_existing_prefix(p);
    #[cfg(not(windows))]
    {
        resolved
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut out = PathBuf::new();
        for c in resolved.components() {
            match c {
                Component::Prefix(pre) => match pre.kind() {
                    Prefix::VerbatimDisk(d) | Prefix::Disk(d) => {
                        out.push(format!("{}:\\", (d as char).to_ascii_lowercase()))
                    }
                    Prefix::VerbatimUNC(s, h) | Prefix::UNC(s, h) => out.push(format!(
                        r"\\{}\{}",
                        s.to_string_lossy().to_lowercase(),
                        h.to_string_lossy().to_lowercase()
                    )),
                    Prefix::Verbatim(s) | Prefix::DeviceNS(s) => {
                        out.push(format!(r"\\?\{}", s.to_string_lossy().to_lowercase()))
                    }
                },
                Component::RootDir => {}
                other => out.push(other.as_os_str().to_string_lossy().to_lowercase()),
            }
        }
        out
    }
}

/// `canonicalize` of the longest prefix that exists, with the rest appended
/// as given. A path with no existing prefix at all comes back unchanged.
fn resolve_existing_prefix(p: &Path) -> PathBuf {
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(c) = cur.canonicalize() {
            let mut out = c;
            for seg in rest.iter().rev() {
                out.push(seg);
            }
            return out;
        }
        match (
            cur.file_name().map(|f| f.to_os_string()),
            cur.parent().map(Path::to_path_buf),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                cur = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// What the trigger needs from the application — a trait so the busy and
/// Stop paths can be driven from a script instead of a real scan.
pub(crate) trait Slot {
    fn start(&self) -> Result<(), crate::error::Error>;
    fn snapshot(&self) -> crate::scan_state::ScanSnapshot;
    fn stopped_at(&self) -> Option<Instant>;
}

impl Slot for crate::state::AppState {
    fn start(&self) -> Result<(), crate::error::Error> {
        crate::scan_job::start(self, crate::scan_state::Entry::Full)
    }
    fn snapshot(&self) -> crate::scan_state::ScanSnapshot {
        self.scan_state().snapshot
    }
    fn stopped_at(&self) -> Option<Instant> {
        crate::state::AppState::stopped_at(self)
    }
}

/// One trigger. Looks at the slot BEFORE every claim (plan review P1-1:
/// `claim_job` overwrites a Cancelled report with Running and resets the
/// cancel flag, so a `start()`-first design never sees the Stop). Carries
/// `newest`, the latest wake it has accepted, across every retry (P2-6).
///
/// Answers the wake it started on, or `None` when it dropped the trigger.
/// The one residual race — Stop pressed on an IDLE slot between the look
/// and the claim — is a Stop with nothing to stop, and `cancel_job` on an
/// idle slot is a no-op by its own doc.
pub(crate) fn trigger(
    slot: &dyn Slot,
    pending: &Mutex<Pending>,
    seen_last: Option<Instant>,
    sleep: &mut dyn FnMut(Duration),
) -> Option<Instant> {
    use crate::job::EndReason;
    use crate::scan_state::ScanSnapshot;
    let mut newest = seen_last;
    loop {
        match slot.snapshot() {
            ScanSnapshot::Running { .. } => {
                sleep(POLL);
                continue;
            }
            ScanSnapshot::Ended { report } if report.reason == EndReason::Cancelled => {
                let mut p = pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                newest = newest.max(p.last);
                p.take();
            }
            // Idle, or ended for a reason other than Cancelled: our change
            // may have been missed by whatever ran, so the Stop rule below
            // still applies here too — a wake that arrived before the press
            // must not restart the scan the press just stopped, even when
            // whatever took the slot in between did not itself end
            // Cancelled. `pending` is left alone in this arm either way; a
            // wake that arrives meanwhile is the next scan's.
            _ => {}
        }
        // The Stop rule, read once per look, for every branch above: a wake
        // `newest` has accepted is good to start on only if it postdates the
        // last Stop. Either side missing — Stop never pressed, or nothing
        // accepted yet — has nothing to forbid, so it defaults to starting.
        let after_stop = match (newest, slot.stopped_at()) {
            (Some(n), Some(s)) => n > s,
            _ => true,
        };
        if !after_stop {
            return None;
        }
        match slot.start() {
            Ok(()) => return newest,
            Err(crate::error::Error::JobAlreadyRunning) => continue,
            Err(e) => {
                eprintln!("mnema: the watcher did not start a scan: {e}");
                return None;
            }
        }
    }
}

/// Whether an event from `notify` is a reason to run the scan again.
///
/// Four rules, in this order, and the order is the contract:
/// 1. `Err` always wakes — the watcher itself is in trouble (queue overflow,
///    a backend error), which is not something a missed scan can wait out.
/// 2. `Rescan` (queue overflow on Linux, `MustScanSubDirs` on macOS) wakes
///    unconditionally — it carries no paths and means "something changed,
///    I do not know what", which is exactly what a full scan answers.
/// 3. `Access` never wakes. The inotify backend subscribes to `IN_OPEN`
///    (notify 8.2.0 `inotify.rs:427`), so the walk's own directory reads and
///    the ingest's own file reads would otherwise re-trigger the scan
///    they belong to.
/// 4. Everything else wakes unless EVERY path is under `private_dir` — the
///    whole app-data directory, not the index file: the WAL and SHM
///    sidecars and `prefs.json.<pid>.<n>.tmp` live beside it, and a scan
///    writes `SCAN_INCOMPLETE` before it walks anything. One private path
///    beside one outside path is an outside change and passes. No paths
///    and no rescan flag is nothing to act on.
///
/// `private_dir` is already in `plain` form; event paths are folded here.
pub(crate) fn classify(event: &Result<notify::Event, notify::Error>, private_dir: &Path) -> bool {
    let event = match event {
        Err(_) => return true,
        Ok(event) => event,
    };
    if event.flag() == Some(notify::event::Flag::Rescan) {
        return true;
    }
    if matches!(event.kind, notify::EventKind::Access(_)) {
        return false;
    }
    !event
        .paths
        .iter()
        .all(|p| plain(p).starts_with(private_dir))
}

pub(crate) trait Subscriptions {
    fn watch(&mut self, root: &Path) -> Result<(), String>;
    fn unwatch(&mut self, root: &Path) -> Result<(), String>;
}

impl Subscriptions for notify::RecommendedWatcher {
    fn watch(&mut self, root: &Path) -> Result<(), String> {
        notify::Watcher::watch(self, root, notify::RecursiveMode::Recursive)
            .map_err(|e| e.to_string())
    }
    fn unwatch(&mut self, root: &Path) -> Result<(), String> {
        notify::Watcher::unwatch(self, root).map_err(|e| e.to_string())
    }
}

/// The outermost roots. A nested root is covered by its ancestor's recursive
/// subscription and is never subscribed on its own: on Linux `unwatch` of a
/// parent removes every watch under it (notify 8.2.0 `inotify.rs:486-494`)
/// and `unwatch` of a separately-subscribed child punches a hole in the
/// parent's tree. Subscribing only the cover makes both impossible.
pub(crate) fn cover(roots: &HashSet<PathBuf>) -> HashSet<PathBuf> {
    roots
        .iter()
        .filter(|r| !roots.iter().any(|o| o != *r && r.starts_with(o)))
        .cloned()
        .collect()
}

/// Bring `watched` in line with `cover(roots)` through `subs`: unwatch what
/// is gone FIRST, then watch what is new, touch nothing else (plan review
/// P2-5, P2-7). A refusal to `watch` is logged and left out of `watched`, so
/// the next call retries it. A refusal to `unwatch` (review round 1, item 5)
/// is logged and left IN `watched` — the OS subscription is still there, and
/// forgetting it would let a later call re-`watch` the same root while it is
/// already subscribed, or think it is free of it when it is not. Answers
/// whether the whole cover is watched.
pub(crate) fn reconcile(
    subs: &mut dyn Subscriptions,
    watched: &mut HashSet<PathBuf>,
    roots: &HashSet<PathBuf>,
) -> bool {
    let want = cover(roots);
    let gone: Vec<PathBuf> = watched.difference(&want).cloned().collect();
    let mut all_gone = true;
    for path in &gone {
        match subs.unwatch(path) {
            Ok(()) => {
                watched.remove(path);
            }
            Err(e) => {
                eprintln!("mnema: could not stop watching {}: {e}", path.display());
                all_gone = false;
            }
        }
    }
    let missing: Vec<PathBuf> = want.difference(watched).cloned().collect();
    for path in &missing {
        match subs.watch(path) {
            Ok(()) => {
                watched.insert(path.clone());
            }
            Err(e) => eprintln!("mnema: not watching {}: {e}", path.display()),
        }
    }
    all_gone && watched.len() == want.len()
}

type RootsReader = Box<dyn Fn() -> Result<HashSet<PathBuf>, String> + Send + Sync>;

/// Everything the `notify` callback, the trigger thread and the folder
/// commands share. The THREAD is the only writer of `watched` and the only
/// caller of `reconcile` (plan review P2-4, P2-5); commands mark `dirty`.
pub struct Shared {
    pub(crate) pending: Mutex<Pending>,
    pub(crate) cv: Condvar,
    watched: Mutex<HashSet<PathBuf>>,
    /// One entry per removed DIRECTORY the callback has seen but has not
    /// yet folded into `watched` (review round 2; reworded, final review —
    /// a plain file removal never reaches this queue at all, `on_event`'s
    /// own doc has why). The callback runs on the notify backend's own
    /// thread — the one `stop()`/`join()` waits on (FSEvents
    /// `watch_inner`, `fsevent.rs:308-346`) or that blocks in `rx.recv()`
    /// (inotify `inotify.rs:560,576`) — and `rewatch` holds `watched`
    /// while it calls into that same backend to `watch`/`unwatch`, so the
    /// callback may never wait on `watched`'s lock: it would risk hanging
    /// the very thread `rewatch`'s OS calls (and `close()`'s watcher drop)
    /// depend on. It pushes `plain` paths here instead (computed BEFORE
    /// this lock is taken — `plain` calls `canonicalize`, an OS call of
    /// its own, and never runs across this lock either) and marks
    /// `dirty`; the owner thread — `rewatch`, the only reader — drains
    /// this under its own short lock, held only for the drain and never
    /// across any OS call, before it reconciles.
    ///
    /// The drain follows the CURRENT `trigger` call, never happens during
    /// one: `run`'s loop only re-checks `dirty` (and so only calls
    /// `rewatch`) between one `trigger` call and the next, so a removal
    /// queued while the owner thread is asleep INSIDE `trigger` (its
    /// `POLL` retry, or the scan it is waiting on) sits in this `Vec`
    /// until that `trigger` call returns.
    ///
    /// No cap: truncating it could drop the one entry that names the
    /// root actually being removed among whatever else queued alongside
    /// it, and a `reconcile` that never sees that removal answers
    /// `all == true` — fully covered — with nothing left to make it
    /// retry.
    forget: Mutex<Vec<PathBuf>>,
    watcher: Mutex<Option<Box<dyn Subscriptions + Send>>>,
    roots: Mutex<Option<RootsReader>>,
    private_dir: Mutex<PathBuf>,
    dirty: AtomicBool,
    closed: AtomicBool,
    tick: AtomicBool,      // test hook: stands in for the REWATCH timeout
    generation: AtomicU64, // bumped after every reconcile, for tests to wait on
    exited: AtomicBool,
}

impl Shared {
    pub(crate) fn new() -> Self {
        Self {
            pending: Mutex::new(Pending::default()),
            cv: Condvar::new(),
            watched: Mutex::new(HashSet::new()),
            forget: Mutex::new(Vec::new()),
            watcher: Mutex::new(None),
            roots: Mutex::new(None),
            private_dir: Mutex::new(PathBuf::new()),
            dirty: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            tick: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            exited: AtomicBool::new(false),
        }
    }
    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Stores `true` into `flag` while holding `pending`'s lock, and
    /// notifies before releasing it. Every writer of `dirty`, `closed` and
    /// `tick` goes through this (review round 1, item 2): `run`'s wait loop
    /// checks those same flags while holding this same lock, immediately
    /// before calling `cv.wait`/`wait_timeout` — a store made without it can
    /// land in the gap between that check and the wait, and the wakeup is
    /// then lost until an unrelated event happens to arrive.
    fn raise(&self, flag: &AtomicBool, notify_all: bool) {
        let _p = Self::lock(&self.pending);
        flag.store(true, Ordering::SeqCst);
        if notify_all {
            self.cv.notify_all();
        } else {
            self.cv.notify_one();
        }
    }

    pub(crate) fn wake(&self) {
        Self::lock(&self.pending).wake(Instant::now());
        self.cv.notify_one();
    }
    /// Commands call this and nothing else: mark the desired set dirty and
    /// nudge the thread. Never wakes `pending`.
    pub fn request_rewatch(&self) {
        self.raise(&self.dirty, false);
    }
    pub fn watched(&self) -> HashSet<PathBuf> {
        Self::lock(&self.watched).clone()
    }
    /// Stop the thread and drop the OS watches. Idempotent. Also clears
    /// `watched` (review round 1, item 6): once the watcher is dropped, no
    /// OS subscription it named still exists, so `watched()` must not keep
    /// reporting one.
    pub fn close(&self) {
        self.raise(&self.closed, true);
        *Self::lock(&self.watcher) = None; // drops the OS watches
        *Self::lock(&self.watched) = HashSet::new();
    }

    /// Builds the OS watcher. The callback holds a `Weak` (no cycle: `Shared`
    /// → watcher → callback → `Weak<Shared>`), so dropping the last `Arc`
    /// drops the watcher with it.
    pub(crate) fn open(self: &Arc<Self>, private_dir: PathBuf, roots: RootsReader) {
        *Self::lock(&self.private_dir) = private_dir;
        *Self::lock(&self.roots) = Some(roots);
        let me: Weak<Shared> = Arc::downgrade(self);
        match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Some(shared) = me.upgrade() {
                shared.on_event(&res);
            }
        }) {
            Ok(w) => *Self::lock(&self.watcher) = Some(Box::new(w)),
            Err(e) => eprintln!("mnema: the folder watcher could not be created: {e}"),
        }
    }

    fn on_event(&self, res: &Result<notify::Event, notify::Error>) {
        if let Ok(ev) = res
            && matches!(
                ev.kind,
                notify::EventKind::Remove(
                    notify::event::RemoveKind::Folder
                        | notify::event::RemoveKind::Other
                        | notify::event::RemoveKind::Any
                )
            )
        {
            // A root removed from under us: inotify drops the watch itself
            // (`inotify.rs:305-315`). This callback must never take
            // `watched`'s lock (review round 2 — see `forget`'s own doc for
            // why), so the removal travels through `forget` instead of
            // being applied here; `rewatch` folds it in before it next
            // reconciles. `plain` calls `canonicalize`, an OS call of its
            // own — computed here, BEFORE `forget`'s lock is taken, never
            // across it. Only a directory kind reaches this branch at all
            // (final review): roots are directories, and a `Remove(File)`
            // event — the bulk of an ordinary mass delete — has nothing to
            // do with the watched-root list `forget` exists to patch.
            let paths: Vec<PathBuf> = ev.paths.iter().map(|p| plain(p)).collect();
            {
                let mut forget = Self::lock(&self.forget);
                forget.extend(paths);
            }
            self.request_rewatch();
        }
        if classify(res, &Self::lock(&self.private_dir)) {
            self.wake();
        }
    }

    /// The thread's only subscription step. Reads the desired roots and
    /// reconciles; answers whether the cover is fully watched.
    fn rewatch(&self) -> bool {
        self.dirty.store(false, Ordering::SeqCst);
        let roots = match Self::lock(&self.roots).as_ref() {
            Some(read) => read(),
            None => return true,
        };
        let roots: HashSet<PathBuf> = match roots {
            Ok(r) => r.iter().map(|p| plain(p)).collect(),
            Err(e) => {
                eprintln!("mnema: the watcher could not read the watched folders: {e}");
                // Unknown, not "fully covered" (review round 1, item 3): a
                // roots-reader error that answered `true` would let the loop
                // wait with no timeout, and an index still closed at startup
                // means nothing is ever retried.
                return false;
            }
        };
        let all = {
            let mut watcher = Self::lock(&self.watcher);
            let mut watched = Self::lock(&self.watched);
            // Fold in whatever the callback queued in `forget` (its own doc
            // explains why a removal cannot land in `watched` directly)
            // before reconciling, so a root the kernel already dropped is
            // not treated as still subscribed and re-`unwatch`ed for
            // nothing, and a root that reappears gets re-subscribed. Locked
            // only for the drain itself, never across the `reconcile` call
            // below — lock order here is `watcher` → `watched` → `forget`.
            let forgotten: Vec<PathBuf> = {
                let mut forget = Self::lock(&self.forget);
                std::mem::take(&mut *forget)
            };
            for p in forgotten {
                watched.remove(&p);
            }
            match watcher.as_mut() {
                Some(w) => reconcile(w.as_mut(), &mut watched, &roots),
                // No watcher means nothing is actually subscribed, whatever
                // `roots` says (review round 1, item 3) — `false`, so the
                // loop retries on the `REWATCH` timeout instead of waiting
                // forever.
                None => false,
            }
        };
        self.generation.fetch_add(1, Ordering::SeqCst);
        all
    }

    /// The thread body. `slot` is the application in production and a
    /// counting stub in tests.
    fn run(self: Arc<Self>, slot: Arc<dyn Slot + Send + Sync>) {
        let mut sleep = |d: Duration| std::thread::sleep(d);
        let mut all = self.rewatch();
        let mut seen_last = trigger(&*slot, &self.pending, None, &mut sleep);
        while !self.closed.load(Ordering::SeqCst) {
            let taken = {
                let mut p = Self::lock(&self.pending);
                loop {
                    if self.closed.load(Ordering::SeqCst) {
                        break None;
                    }
                    if self.dirty.swap(false, Ordering::SeqCst)
                        || self.tick.swap(false, Ordering::SeqCst)
                    {
                        drop(p);
                        all = self.rewatch();
                        p = Self::lock(&self.pending);
                        continue;
                    }
                    match p.due(Instant::now()) {
                        Some(d) if d.is_zero() => break p.take(),
                        Some(d) => {
                            p = self
                                .cv
                                .wait_timeout(p, d)
                                .unwrap_or_else(|e| e.into_inner())
                                .0
                        }
                        None if all => p = self.cv.wait(p).unwrap_or_else(|e| e.into_inner()),
                        None => {
                            let (g, r) = self
                                .cv
                                .wait_timeout(p, REWATCH)
                                .unwrap_or_else(|e| e.into_inner());
                            p = g;
                            if r.timed_out() {
                                self.tick.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                }
            };
            if self.closed.load(Ordering::SeqCst) {
                break;
            }
            all = self.rewatch();
            seen_last =
                trigger(&*slot, &self.pending, taken.max(seen_last), &mut sleep).or(seen_last);
        }
        self.exited.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn spawn_for_test(self: &Arc<Self>) -> Arc<CountingSlot> {
        let slot = Arc::new(CountingSlot::default());
        let me = Arc::clone(self);
        let s: Arc<dyn Slot + Send + Sync> = slot.clone();
        std::thread::spawn(move || me.run(s));
        slot
    }
    #[cfg(test)]
    pub(crate) fn tick_for_test(&self) {
        self.raise(&self.tick, false);
    }
    #[cfg(test)]
    pub(crate) fn rewatch_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    #[cfg(test)]
    pub(crate) fn thread_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }
}

/// `.setup`'s last line. The thread begins with one direct trigger (owner
/// decision 1: a scan at launch).
pub fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let state = app.state::<crate::state::AppState>();
    let shared = Arc::clone(state.watch());
    let reader_handle = app.clone();
    shared.open(
        plain(state.data_dir()),
        Box::new(move || {
            let st = reader_handle.state::<crate::state::AppState>();
            st.with_index(|db| {
                Ok(db
                    .list_watched_roots()?
                    .into_iter()
                    .map(|r| PathBuf::from(r.absolute_path))
                    .collect())
            })
            .map_err(|e| e.to_string())
        }),
    );
    let slot_handle = app.clone();
    std::thread::Builder::new()
        .name("mnema-watch".into())
        .spawn(move || shared.run(Arc::new(HandleSlot(slot_handle))))
        .expect("spawning the watcher thread");
}

/// `Slot` over an `AppHandle`, so the thread holds the handle and not a reference into state.
struct HandleSlot<R: tauri::Runtime>(tauri::AppHandle<R>);
impl<R: tauri::Runtime> Slot for HandleSlot<R> {
    fn start(&self) -> Result<(), crate::error::Error> {
        use tauri::Manager;
        Slot::start(&*self.0.state::<crate::state::AppState>())
    }
    fn snapshot(&self) -> crate::scan_state::ScanSnapshot {
        use tauri::Manager;
        Slot::snapshot(&*self.0.state::<crate::state::AppState>())
    }
    fn stopped_at(&self) -> Option<Instant> {
        use tauri::Manager;
        Slot::stopped_at(&*self.0.state::<crate::state::AppState>())
    }
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct CountingSlot {
    pub starts: AtomicU64,
}
#[cfg(test)]
impl Slot for CountingSlot {
    fn start(&self) -> Result<(), crate::error::Error> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn snapshot(&self) -> crate::scan_state::ScanSnapshot {
        crate::scan_state::ScanSnapshot::Idle
    }
    fn stopped_at(&self) -> Option<Instant> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, AccessMode, CreateKind, Flag, ModifyKind};
    use notify::{Event, EventKind};

    fn private() -> PathBuf {
        plain(Path::new("/data/mnema"))
    }
    fn ev(kind: EventKind, paths: &[&str]) -> Result<Event, notify::Error> {
        let mut e = Event::new(kind);
        for p in paths {
            e = e.add_path(PathBuf::from(p));
        }
        Ok(e)
    }

    #[test]
    fn plain_resolves_the_existing_prefix_and_keeps_the_rest() {
        // Measured in the self-review probe: FSEvents delivers
        // `/private/var/…` for a root handed over as `/var/…`.
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().canonicalize().unwrap();
        let link = dir.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(unix)]
        assert_eq!(
            plain(&link.join("gone").join("deeper.txt")),
            real.join("gone").join("deeper.txt"),
            "a deleted tail resolves through its existing parent"
        );
        assert_eq!(
            plain(Path::new("/no/such/prefix/at/all")),
            PathBuf::from("/no/such/prefix/at/all")
        );
        #[cfg(windows)]
        {
            // Plan review P1-2: `canonicalize` answers `\\?\C:\…`; notify builds event paths from the plain `C:\…` it was handed.
            assert_eq!(
                plain(Path::new(r"\\?\C:\Windows")),
                PathBuf::from(r"c:\windows")
            );
            assert_eq!(
                plain(Path::new(r"C:\WINDOWS")),
                PathBuf::from(r"c:\windows")
            );
        }
    }

    #[test]
    fn a_read_is_not_a_change() {
        assert!(!classify(
            &ev(
                EventKind::Access(AccessKind::Open(AccessMode::Any)),
                &["/docs/a.txt"]
            ),
            &private()
        ));
        assert!(classify(
            &ev(EventKind::Modify(ModifyKind::Any), &["/docs/a.txt"]),
            &private()
        ));
    }

    #[test]
    fn rescan_wakes_without_paths_and_regardless_of_kind() {
        let e = Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan));
        assert!(classify(&e, &private()));
        let e = Ok(Event::new(EventKind::Access(AccessKind::Any)).set_flag(Flag::Rescan));
        assert!(classify(&e, &private()));
    }

    #[test]
    fn an_error_wakes() {
        let e: Result<Event, notify::Error> = Err(notify::Error::generic("queue overflow"));
        assert!(classify(&e, &private()));
    }

    #[test]
    fn the_private_directory_is_dropped_only_when_every_path_is_inside_it() {
        assert!(!classify(
            &ev(
                EventKind::Modify(ModifyKind::Any),
                &["/data/mnema/index.sqlite-wal"]
            ),
            &private()
        ));
        assert!(!classify(
            &ev(
                EventKind::Create(CreateKind::File),
                &["/data/mnema/prefs.json.123.4.tmp", "/data/mnema/prefs.json"]
            ),
            &private()
        ));
        assert!(classify(
            &ev(
                EventKind::Modify(ModifyKind::Any),
                &["/data/mnema/index.sqlite", "/docs/b.txt"]
            ),
            &private()
        ));
        assert!(
            !classify(&ev(EventKind::Modify(ModifyKind::Any), &[]), &private()),
            "no paths, no rescan flag: nothing to act on"
        );
    }

    /// `private()` above is a directory that never exists, so every test that
    /// uses it takes `plain`'s "no such prefix" branch: none of them exercise
    /// `plain` + `classify` together over a root whose real path differs from
    /// the raw one, which is the failure mode §2's amendment exists for
    /// (measured: FSEvents delivers `/private/var/…` for a root handed over
    /// as `/var/…`). This builds a private dir reached through a real
    /// symlink and classifies events already given in the *resolved* form —
    /// the form FSEvents actually delivers — against it.
    #[cfg(unix)]
    #[test]
    fn the_private_directory_is_dropped_when_it_is_reached_through_a_symlink() {
        // Final review: this whole test is unix-only (a real symlink), and
        // the `#[cfg(unix)]` used to sit on an inner block rather than the
        // function — vacuously green on Windows, asserting nothing there.
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(real.join("data/mnema")).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(real.join("data"), &link).unwrap();
        let private_dir = plain(&link.join("mnema"));

        assert!(
            !classify(
                &ev(
                    EventKind::Modify(ModifyKind::Any),
                    &[real.join("data/mnema/index.sqlite").to_str().unwrap()]
                ),
                &private_dir
            ),
            "a resolved path under the symlinked private dir is still recognised as private"
        );
        assert!(
            classify(
                &ev(
                    EventKind::Modify(ModifyKind::Any),
                    &[real.join("data/other/file.txt").to_str().unwrap()]
                ),
                &private_dir
            ),
            "positive control: a resolved path outside the private dir still wakes"
        );
    }

    use std::time::Instant;
    fn t(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    #[test]
    fn five_wakes_in_a_burst_fire_once_after_quiet() {
        let base = Instant::now();
        let mut p = Pending::default();
        for ms in [0, 20, 40, 60, 80] {
            p.wake(base + Duration::from_millis(ms));
        }
        assert_eq!(
            p.due(base + Duration::from_millis(100)),
            Some(QUIET - Duration::from_millis(20))
        );
        assert_eq!(
            p.due(base + Duration::from_millis(80) + QUIET),
            Some(Duration::ZERO)
        );
        assert_eq!(p.take(), Some(base + Duration::from_millis(80)));
        assert_eq!(
            p.due(base + Duration::from_secs(10)),
            None,
            "taken means nothing pending"
        );
    }

    #[test]
    fn a_wake_every_second_still_fires_by_max_wait() {
        // Spec review P2-3: trailing-only debounce under a one-per-second stream never fires.
        let base = Instant::now();
        let mut p = Pending::default();
        let mut fired_at = None;
        for s in 0..61u64 {
            let now = t(base, s);
            p.wake(now);
            if p.due(now + Duration::from_millis(500)) == Some(Duration::ZERO) {
                fired_at = Some(s);
                break;
            }
        }
        assert_eq!(
            fired_at,
            Some(30),
            "the cap from the first wake must fire at MAX_WAIT, not never"
        );
    }

    #[test]
    fn due_never_exceeds_the_cap() {
        let base = Instant::now();
        let mut p = Pending::default();
        p.wake(base);
        p.wake(t(base, 29));
        assert_eq!(
            p.due(t(base, 29)),
            Some(Duration::from_secs(1)),
            "one second to the cap, not two of quiet"
        );
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_a_verbatim_private_dir_still_matches_a_plain_event_path() {
        let private = plain(Path::new(r"\\?\C:\Users\U\AppData\Local\Mnema"));
        assert!(!classify(
            &ev(
                EventKind::Modify(ModifyKind::Any),
                &[r"C:\Users\U\AppData\Local\Mnema\index.sqlite-wal"]
            ),
            &private
        ));
        assert!(
            classify(
                &ev(
                    EventKind::Modify(ModifyKind::Any),
                    &[r"C:\Users\U\Documents\a.txt"]
                ),
                &private
            ),
            "positive control"
        );
    }

    use crate::job::EndReason;
    use crate::scan_state::{ScanReport, ScanSnapshot};
    use std::cell::RefCell;

    /// A scripted slot. `snapshot()` answers `snaps` in order and repeats the
    /// last one; `start()` answers `starts` in order.
    struct Script {
        starts: RefCell<Vec<Result<(), crate::error::Error>>>,
        snaps: RefCell<Vec<ScanSnapshot>>,
        stopped: Option<Instant>,
        started: RefCell<usize>,
    }
    impl Slot for Script {
        fn start(&self) -> Result<(), crate::error::Error> {
            *self.started.borrow_mut() += 1;
            self.starts.borrow_mut().remove(0)
        }
        fn snapshot(&self) -> ScanSnapshot {
            let mut s = self.snaps.borrow_mut();
            if s.len() > 1 {
                s.remove(0)
            } else {
                s[0].clone()
            }
        }
        fn stopped_at(&self) -> Option<Instant> {
            self.stopped
        }
    }
    fn ended(reason: EndReason) -> ScanSnapshot {
        ScanSnapshot::Ended {
            report: ScanReport {
                reason,
                ..ScanReport::default()
            },
        }
    }
    fn running() -> ScanSnapshot {
        ScanSnapshot::Running {
            phase: crate::scan_state::Phase::Other {
                job: crate::scan_state::OtherJob::Probe,
            },
            cancellable: true,
        }
    }
    fn busy() -> crate::error::Error {
        crate::error::Error::JobAlreadyRunning
    }
    fn script(
        starts: Vec<Result<(), crate::error::Error>>,
        snaps: Vec<ScanSnapshot>,
        stopped: Option<Instant>,
    ) -> Script {
        Script {
            starts: RefCell::new(starts),
            snaps: RefCell::new(snaps),
            stopped,
            started: RefCell::new(0),
        }
    }
    fn no_sleep() -> impl FnMut(Duration) {
        |_| {}
    }

    #[test]
    fn a_cancelled_scan_already_ended_before_the_first_attempt_is_not_restarted() {
        // Plan review P1-1: the slot is FREE; a `start()`-first trigger would
        // claim it and never read the Stop rule.
        let base = Instant::now();
        let s = script(
            vec![Ok(())],
            vec![ended(EndReason::Cancelled)],
            Some(base + Duration::from_secs(5)),
        );
        let pending = Mutex::new(Pending::default());
        let out = trigger(
            &s,
            &pending,
            Some(base + Duration::from_secs(3)),
            &mut no_sleep(),
        );
        assert_eq!(
            *s.started.borrow(),
            0,
            "a change before Stop must not claim the freed slot"
        );
        assert_eq!(out, None);
        assert_eq!(*pending.lock().unwrap(), Pending::default());
    }

    #[test]
    fn a_change_after_stop_starts_even_though_the_slot_shows_cancelled() {
        let base = Instant::now();
        let s = script(
            vec![Ok(())],
            vec![ended(EndReason::Cancelled)],
            Some(base + Duration::from_secs(5)),
        );
        let pending = Mutex::new(Pending::default());
        let out = trigger(
            &s,
            &pending,
            Some(base + Duration::from_secs(7)),
            &mut no_sleep(),
        );
        assert_eq!(
            *s.started.borrow(),
            1,
            "positive control: a change after Stop scans"
        );
        assert_eq!(out, Some(base + Duration::from_secs(7)));
    }

    #[test]
    fn a_change_before_stop_while_running_does_not_restart_once_cancelled() {
        let base = Instant::now();
        let s = script(
            vec![],
            vec![running(), ended(EndReason::Cancelled)],
            Some(base + Duration::from_secs(5)),
        );
        let pending = Mutex::new(Pending::default());
        trigger(
            &s,
            &pending,
            Some(base + Duration::from_secs(3)),
            &mut no_sleep(),
        );
        assert_eq!(*s.started.borrow(), 0);
    }

    #[test]
    fn a_wake_during_the_wait_that_postdates_stop_restarts() {
        let base = Instant::now();
        let s = script(
            vec![Ok(())],
            vec![running(), ended(EndReason::Cancelled)],
            Some(base + Duration::from_secs(5)),
        );
        let pending = Mutex::new(Pending::default());
        pending.lock().unwrap().wake(base + Duration::from_secs(7));
        let out = trigger(
            &s,
            &pending,
            Some(base + Duration::from_secs(3)),
            &mut no_sleep(),
        );
        assert_eq!(*s.started.borrow(), 1);
        assert_eq!(out, Some(base + Duration::from_secs(7)));
        assert_eq!(
            *pending.lock().unwrap(),
            Pending::default(),
            "the restart consumed it"
        );
    }

    #[test]
    fn newest_survives_a_second_busy_that_restores_the_cancelled_report() {
        // Plan review P2-6: after the restart was allowed, an `Other` job
        // takes the slot and, on ending, restores the previous Cancelled
        // report (`state.rs:538-541`). The trigger must remember the
        // post-Stop wake it already accepted.
        let base = Instant::now();
        let s = script(
            vec![Err(busy()), Ok(())],
            vec![
                running(),
                ended(EndReason::Cancelled),
                running(),
                ended(EndReason::Cancelled),
            ],
            Some(base + Duration::from_secs(5)),
        );
        let pending = Mutex::new(Pending::default());
        pending.lock().unwrap().wake(base + Duration::from_secs(7));
        let out = trigger(
            &s,
            &pending,
            Some(base + Duration::from_secs(3)),
            &mut no_sleep(),
        );
        assert_eq!(
            *s.started.borrow(),
            2,
            "the third look at the slot must still start"
        );
        assert_eq!(out, Some(base + Duration::from_secs(7)));
    }

    #[test]
    fn a_completed_foreign_scan_is_followed_by_ours_and_pending_is_kept() {
        let base = Instant::now();
        let s = script(
            vec![Ok(())],
            vec![running(), ended(EndReason::Completed)],
            None,
        );
        let pending = Mutex::new(Pending::default());
        pending.lock().unwrap().wake(base + Duration::from_secs(1));
        trigger(&s, &pending, Some(base), &mut no_sleep());
        assert_eq!(*s.started.borrow(), 1);
        assert!(
            pending.lock().unwrap().first.is_some(),
            "a wake during the wait is the next scan's"
        );
    }

    #[test]
    fn a_second_busy_answer_keeps_waiting() {
        let s = script(
            vec![Err(busy()), Err(busy()), Ok(())],
            vec![ScanSnapshot::Idle],
            None,
        );
        let pending = Mutex::new(Pending::default());
        trigger(&s, &pending, None, &mut no_sleep());
        assert_eq!(*s.started.borrow(), 3);
    }

    #[test]
    fn any_other_refusal_is_logged_and_dropped() {
        let s = script(
            vec![Err(crate::error::Error::IndexNotOpen)],
            vec![ScanSnapshot::Idle],
            None,
        );
        let pending = Mutex::new(Pending::default());
        assert_eq!(trigger(&s, &pending, None, &mut no_sleep()), None);
        assert_eq!(*s.started.borrow(), 1);
    }

    #[test]
    fn a_change_before_stop_is_not_restarted_by_a_foreign_completed_scan() {
        // Review round 1: the Stop rule guarded only the Cancelled arm; a
        // pre-Stop wake read against a foreign job that ended Completed (not
        // Cancelled, so `claim_job`'s restore condition at `state.rs:538-541`
        // never applies) fell through to the `_` arm and restarted the scan
        // Stop had just stopped.
        let base = Instant::now();
        let t = base + Duration::from_secs(10);
        let s = script(vec![], vec![ended(EndReason::Completed)], Some(t));
        let pending = Mutex::new(Pending::default());
        let out = trigger(
            &s,
            &pending,
            Some(t - Duration::from_secs(1)),
            &mut no_sleep(),
        );
        assert_eq!(
            *s.started.borrow(),
            0,
            "pre-Stop wake after a foreign Completed: dropped"
        );
        assert_eq!(out, None);

        // Positive control: the identical shape, but the wake postdates Stop.
        let s = script(vec![Ok(())], vec![ended(EndReason::Completed)], Some(t));
        let pending = Mutex::new(Pending::default());
        let out = trigger(
            &s,
            &pending,
            Some(t + Duration::from_secs(1)),
            &mut no_sleep(),
        );
        assert_eq!(
            *s.started.borrow(),
            1,
            "positive control: a post-Stop wake after a foreign Completed still starts"
        );
        assert_eq!(out, Some(t + Duration::from_secs(1)));
    }

    use std::collections::HashSet;
    use std::sync::Arc;

    /// Records every call; `watch` refuses paths in `refuse`.
    #[derive(Default)]
    struct Spy {
        calls: Vec<(&'static str, PathBuf)>,
        refuse: HashSet<PathBuf>,
    }
    impl Subscriptions for Spy {
        fn watch(&mut self, root: &Path) -> Result<(), String> {
            self.calls.push(("watch", root.to_path_buf()));
            if self.refuse.contains(root) {
                Err("refused".into())
            } else {
                Ok(())
            }
        }
        fn unwatch(&mut self, root: &Path) -> Result<(), String> {
            self.calls.push(("unwatch", root.to_path_buf()));
            Ok(())
        }
    }
    fn set(paths: &[&str]) -> HashSet<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn cover_keeps_only_roots_without_a_watched_ancestor() {
        assert_eq!(
            cover(&set(&["/docs", "/docs/sub", "/other", "/docs/sub/deep"])),
            set(&["/docs", "/other"])
        );
        assert_eq!(
            cover(&set(&["/docs-x", "/docs"])),
            set(&["/docs-x", "/docs"]),
            "a sibling with a shared prefix is not an ancestor"
        );
    }

    #[test]
    fn reconcile_touches_only_what_changed() {
        // Plan review P2-7: the rebuild mutant re-subscribes A and the
        // end state is the same — only the calls tell them apart.
        let mut spy = Spy::default();
        let mut watched = set(&["/a", "/b"]);
        assert!(reconcile(&mut spy, &mut watched, &set(&["/a"])));
        assert_eq!(spy.calls, vec![("unwatch", PathBuf::from("/b"))]);
        assert_eq!(watched, set(&["/a"]));
    }

    #[test]
    fn removing_a_parent_root_resubscribes_the_child_it_covered() {
        // Plan review P2-3: Linux unwatch(parent) removes the child's watch too.
        let mut spy = Spy::default();
        let mut watched = HashSet::new();
        reconcile(&mut spy, &mut watched, &set(&["/docs", "/docs/sub"]));
        assert_eq!(
            watched,
            set(&["/docs"]),
            "the child is covered by the parent, never subscribed on its own"
        );
        spy.calls.clear();
        reconcile(&mut spy, &mut watched, &set(&["/docs/sub"]));
        assert_eq!(
            spy.calls,
            vec![
                ("unwatch", PathBuf::from("/docs")),
                ("watch", PathBuf::from("/docs/sub"))
            ],
            "unwatch before watch, so the new subscription is not swept away"
        );
        assert_eq!(watched, set(&["/docs/sub"]));
    }

    #[test]
    fn removing_a_child_root_leaves_the_parent_alone() {
        let mut spy = Spy::default();
        let mut watched = set(&["/docs"]);
        reconcile(&mut spy, &mut watched, &set(&["/docs"]));
        assert!(
            spy.calls.is_empty(),
            "removing a covered child must not unwatch anything — that would punch a hole in the parent"
        );
    }

    #[test]
    fn a_refused_root_is_reported_and_retried_next_time() {
        let mut spy = Spy {
            refuse: set(&["/gone"]),
            ..Default::default()
        };
        let mut watched = HashSet::new();
        assert!(!reconcile(&mut spy, &mut watched, &set(&["/a", "/gone"])));
        assert_eq!(watched, set(&["/a"]));
        spy.refuse.clear();
        assert!(reconcile(&mut spy, &mut watched, &set(&["/a", "/gone"])));
        assert_eq!(watched, set(&["/a", "/gone"]));
    }

    fn shared_for(
        roots: Vec<PathBuf>,
    ) -> (Arc<Shared>, Arc<Mutex<Vec<PathBuf>>>, Arc<CountingSlot>) {
        let desired = Arc::new(Mutex::new(roots));
        let shared = Arc::new(Shared::new());
        let reader = Arc::clone(&desired);
        shared.open(
            plain(Path::new("/nonexistent-private")),
            Box::new(move || Ok(reader.lock().unwrap().iter().cloned().collect())),
        );
        let slot = shared.spawn_for_test(); // the thread with `Slot` = `CountingSlot`
        shared.request_rewatch();
        assert!(wait_for(|| shared.rewatch_generation() >= 1));
        (shared, desired, slot)
    }
    fn wait_for(mut f: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        f()
    }

    #[test]
    fn a_write_under_a_watched_root_wakes_and_close_stops_the_thread() {
        let root = tempfile::tempdir().unwrap();
        let (shared, _, slot) = shared_for(vec![root.path().to_path_buf()]);
        std::fs::write(root.path().join("a.txt"), "x").unwrap();
        // The thread may already have consumed the wake into a second start.
        assert!(
            wait_for(|| shared.pending.lock().unwrap().first.is_some()
                || slot.starts.load(Ordering::SeqCst) >= 2),
            "no wake within 5 s"
        );
        shared.close();
        assert!(
            wait_for(|| shared.thread_exited()),
            "close must end the thread"
        );
        // The thread's own `Arc<Self>` is released only after `run` returns,
        // just after `exited` is set — so the count can still be 2 for a
        // moment: wait rather than assert immediately.
        assert!(
            wait_for(|| Arc::strong_count(&shared) == 1),
            "the callback holds only a Weak — no cycle keeps Shared alive: \
             expected the thread's Arc to be released, found it still held"
        );
    }

    #[test]
    fn a_failed_new_root_is_retried_on_the_tick_after_it_appears() {
        // Plan review P2-4: the thread, not the command, owns the retry.
        let parent = tempfile::tempdir().unwrap();
        let a = parent.path().join("a");
        let later = parent.path().join("later");
        std::fs::create_dir(&a).unwrap();
        let (shared, desired, _slot) = shared_for(vec![a.clone()]);
        desired.lock().unwrap().push(later.clone());
        shared.request_rewatch();
        assert!(wait_for(|| shared.rewatch_generation() >= 2));
        assert_eq!(
            shared.watched(),
            HashSet::from([plain(&a)]),
            "an absent root cannot be subscribed; keys are in plain form"
        );
        std::fs::create_dir(&later).unwrap();
        shared.tick_for_test(); // stands in for the 60 s REWATCH timeout
        assert!(wait_for(|| shared.watched().contains(&plain(&later))));
        std::fs::write(later.join("x.txt"), "x").unwrap();
        assert!(wait_for(|| shared.pending.lock().unwrap().first.is_some()));
        shared.close();
    }

    #[test]
    fn a_removed_root_leaves_the_watched_set() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("gone");
        std::fs::create_dir(&root).unwrap();
        let (shared, _, _slot) = shared_for(vec![root.clone()]);
        // Positive control (review round 1, item 4): `watched().is_empty()`
        // after the removal would also pass if the root was never
        // subscribed in the first place.
        assert!(
            wait_for(|| shared.watched().contains(&plain(&root))),
            "the root must be subscribed before the test removes it"
        );
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            wait_for(|| shared.watched().is_empty()),
            "the root's own Remove must drop it from `watched`"
        );
        shared.close();
    }
}
