//! The folder watcher: `notify` events in, one `scan_job::start` out.
//!
//! Spec: docs 2026-09-10 (private). The walk is the source of truth; this
//! module only decides WHEN to run it again. Nothing here touches the index.
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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
    // `trigger` (Task 4) only ever calls `take`; `wake` and `due` are driven
    // by the watcher thread once Task 5 subscribes — the allow leaves with it.
    #[allow(dead_code)]
    pub(crate) fn wake(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// `None`: nothing pending. `Some(ZERO)`: fire now. `Some(d)`: wait `d`.
    /// Fires when `QUIET` has passed since the last wake OR `MAX_WAIT` since
    /// the first; the sooner of the two.
    // Driven by the watcher thread once Task 5 subscribes; the allow leaves with it.
    #[allow(dead_code)]
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
// Called from the watcher thread once Task 5 subscribes; the allow leaves with it.
#[allow(dead_code)]
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
// Called from the watcher thread once Task 5 subscribes; the allow leaves with it.
#[allow(dead_code)]
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
///
/// Called from the watcher thread once Task 5 subscribes; the allow leaves with it.
#[allow(dead_code)]
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
// Called from the watcher thread once Task 5 subscribes; the allow leaves with it.
#[allow(dead_code)]
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
                let after_stop = matches!((newest, slot.stopped_at()), (Some(n), Some(s)) if n > s);
                if !after_stop {
                    p.take();
                    return None;
                }
                p.take();
            }
            // Idle, or ended for another reason: our change may have been
            // missed by whatever ran — claim. `pending` is left alone; a
            // wake that arrived meanwhile is the next scan's.
            _ => {}
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
// Called from the watcher thread once Task 5 subscribes; the allow leaves with it.
#[allow(dead_code)]
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
    #[test]
    fn the_private_directory_is_dropped_when_it_is_reached_through_a_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(real.join("data/mnema")).unwrap();
        #[cfg(unix)]
        {
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
    use std::sync::Mutex;

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
        // report (`state.rs:805-815`). The trigger must remember the
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
}
