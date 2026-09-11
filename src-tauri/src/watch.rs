//! The folder watcher: `notify` events in, one `scan_job::start` out.
//!
//! Spec: docs 2026-09-10 (private). The walk is the source of truth; this
//! module only decides WHEN to run it again. Nothing here touches the index.
use std::path::{Path, PathBuf};
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
// Driven by the trigger thread from Task 4; the allow leaves with it.
#[allow(dead_code)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pending {
    pub first: Option<Instant>,
    pub last: Option<Instant>,
}

// Driven by the trigger thread from Task 4; the allow leaves with it.
#[allow(dead_code)]
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
/// through its parent), then on Windows strips the verbatim prefix
/// `canonicalize` adds and folds case. Measured: FSEvents on macOS delivers
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

/// Whether an event from `notify` is a reason to run the scan again.
///
/// Three rules, in this order, and the order is the contract:
/// 1. `Rescan` (queue overflow on Linux, `MustScanSubDirs` on macOS) wakes
///    unconditionally — it carries no paths and means "something changed,
///    I do not know what", which is exactly what a full scan answers.
/// 2. `Access` never wakes. The inotify backend subscribes to `IN_OPEN`
///    (notify 8.2.0 `inotify.rs:427`), so the walk's own directory reads and
///    the ingest's own file reads would otherwise re-trigger the scan
///    they belong to.
/// 3. Everything else wakes unless EVERY path is under `private_dir` — the
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
}
