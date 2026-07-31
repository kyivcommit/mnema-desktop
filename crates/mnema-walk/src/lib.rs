//! What is under a watched root after the rules.
//!
//! This crate is the ONLY place that looks at the disk during a walk. Size and
//! mtime are taken here once and travel to `ingest_file`; taking them again
//! there would compare a file against a stat of a moment that has passed (§5).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod rules;
// `RulesError` has to be nameable outside this crate: `WalkRules::new`
// returns it, and a caller with a save dialog in front of a person needs to
// match on it or put it in a signature, not just observe that `new` failed
// (review fix round 2, Critical finding — this line was the whole gap).
pub use rules::{RulesError, WalkRules};

// Defined in the shared-types crate, re-exported here so a caller that only
// deals with the walk can name it without a second dependency.
pub use mnema_core::OnDisk;

/// One file the walk kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Relative to the watched root, with `/` as the separator on every
    /// platform, because it is stored and compared as text (`path.relative_path`).
    pub relative: String,
    pub absolute: PathBuf,
    pub on_disk: OnDisk,
}

/// A file the walk refused before any worker was asked, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreSkip {
    /// Relative to the watched root, `/`-separated — the same form as
    /// `Found::relative`, because the reconciliation keys the skip journal on
    /// it and compares it against what the walk found. `None` only when the
    /// failure carried no path we can express in that form at all: no path
    /// (a walker error with nothing to peel), or a path that is not valid
    /// UTF-8 (`UnrepresentableName` — the whole point of that variant is
    /// that no `String` can name it).
    pub relative: Option<String>,
    /// For a human and for the journal's reason column: the lossy name, or
    /// the error text when there is no path at all. Never compared against
    /// anything.
    pub detail: String,
    pub rule: PreSkipRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreSkipRule {
    /// The name is not valid UTF-8. Storing it lossily would store a string
    /// that no longer opens the file (§5).
    UnrepresentableName,
    /// A cloud placeholder: present in the listing, absent from the disk, and
    /// reading it would download it (§5).
    NotMaterialised,
    /// The walker could not read this entry at all: permission denied, a
    /// directory that vanished mid-walk, a size that does not fit `i64`, or
    /// the watched root itself not being a directory. A directory refused
    /// this way takes its whole subtree down with it — nothing under it
    /// becomes `found`, and nothing else names it — so the path travels here
    /// rather than being folded into a bare count (`Walked::unreadable`).
    /// See `Walked::complete`.
    Unreadable,
    /// Not a regular file and not a directory, and following it once (the
    /// one look this crate takes past an lstat) resolves to something other
    /// than a directory: a symlink to a file, a broken/dangling symlink, a
    /// FIFO, a socket, a device. Naming it here is the whole fix — not
    /// following a symlink is a decision, not a failure, so this does NOT
    /// clear `Walked::complete`.
    NotAFile,
    /// Not a regular file and not a directory (same as `NotAFile`), but
    /// following it once resolves to a directory: a symlink to a directory.
    /// Distinguished from `NotAFile` because losing this one is not losing
    /// one entry, it is losing a whole subtree in a single step — nothing
    /// under it was ever visited, so nothing under it appears anywhere else
    /// either. `Walked::complete` still stays true: not following a symlink
    /// is a decision made once, in `WalkRules::builder`, not a read failure
    /// discovered per-entry. What a later reconciliation does with a
    /// subtree-shaped skip — treat `relative` as a prefix, or not — is that
    /// reconciliation's call, not this crate's.
    NotAFileSubtree,
}

#[derive(Debug)]
pub struct Walked {
    pub found: Vec<Found>,
    /// How many entries the walk could not read: the walker's own error, a
    /// failed `metadata()`, or a size that does not fit `i64`. A count, not
    /// only a list, for the same reason the old doc comment on this field
    /// was wrong: rule removals are NOT counted here — the three rule layers
    /// simply never produce these entries as walk candidates in the first
    /// place, so there is nothing for this field to see. `skipped` carries
    /// the path of every one of these; this is only the tally.
    pub unreadable: u64,
    pub skipped: Vec<PreSkip>,
    /// False if any entry was left out because the walk could not read it.
    /// An unreadable subdirectory is indistinguishable from an empty one by
    /// its absence from `found` alone — the same shape a deleted directory
    /// would have — so a reconciliation that deletes rows for paths absent
    /// from `found` must refuse to run when this is false.
    pub complete: bool,
    /// False only when the override-based rule layers — the unconditional
    /// built-in list, user prefixes, `.DS_Store` — failed to combine into a
    /// working pattern set for this walk (`WalkRules::builder`). Past a few
    /// thousand user prefixes, or one long enough on its own, the matching
    /// engine can refuse the combined set even though `WalkRules::new`
    /// already refused any prefix that fails to compile alone — this is the
    /// aggregate failure that only shows up once every prefix is combined
    /// with the rest. When it is false, `found` may hold files the user
    /// excluded on purpose: every override-based exclusion silently stopped
    /// applying for this one walk.
    ///
    /// This is a different KIND of fact from `complete`, not a variant of
    /// it. `complete == false` means "the walk could not read everything
    /// under the root, so do not trust an absence enough to delete for it."
    /// `rules_applied == false` means "the walk may have kept files the
    /// user asked to exclude, so do not index or send them anywhere."
    /// Nothing in this crate reads this field yet — Task 7 does.
    pub rules_applied: bool,
}

impl Default for Walked {
    fn default() -> Self {
        Self {
            found: Vec::new(),
            unreadable: 0,
            skipped: Vec::new(),
            complete: true,
            rules_applied: true,
        }
    }
}

pub fn enumerate(root: &Path, rules: &WalkRules) -> Walked {
    let mut walked = Walked::default();

    if !root.is_dir() {
        // Not a directory at all, right now: does not exist, or is a regular
        // file (or a symlink to one). This and the walk below are two
        // separate syscalls — the `entry.depth() == 0` arm inside the loop
        // carries the same check for the window between them — but without
        // this one, the common case (root was never a directory to begin
        // with) would have to build a `Walk` and iterate it just to learn
        // that, instead of returning immediately.
        walked.unreadable += 1;
        walked.complete = false;
        walked.skipped.push(PreSkip {
            relative: None,
            detail: root.to_string_lossy().into_owned(),
            rule: PreSkipRule::Unreadable,
        });
        return walked;
    }

    let (builder, rules_applied) = rules.builder(root);
    walked.rules_applied = rules_applied;
    let walk = builder.build();

    for entry in walk {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                // The walker's own error: it could not even list this entry,
                // so a directory here loses its whole subtree, not just
                // itself. `Error::Partial` bundles several independent
                // failures into one `Err`, so flatten before recording —
                // otherwise only the first of them would ever be named.
                for err in flatten_partial(&err) {
                    walked.unreadable += 1;
                    walked.complete = false;
                    let path = error_path(err);
                    walked.skipped.push(PreSkip {
                        relative: path.and_then(|p| relative_of(root, p)),
                        detail: path
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_else(|| err.to_string()),
                        rule: PreSkipRule::Unreadable,
                    });
                }
                continue;
            }
        };
        if entry.depth() == 0 {
            // The root itself — usually just skipped. But `root.is_dir()`
            // above and this walk are two separate syscalls: if the root
            // stopped being a directory in between (replaced by a file,
            // deleted), the walker yields it here as an ordinary entry, and
            // silently `continue`-ing would reproduce the exact state the
            // `!root.is_dir()` guard above exists to prevent — empty
            // `found`, `complete` left true.
            if !root_entry_is_walkable(entry.path()) {
                walked.unreadable += 1;
                walked.complete = false;
                walked.skipped.push(PreSkip {
                    relative: None,
                    detail: entry.path().to_string_lossy().into_owned(),
                    rule: PreSkipRule::Unreadable,
                });
            }
            continue;
        }
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(_) => {
                walked.unreadable += 1;
                walked.complete = false;
                walked.skipped.push(PreSkip {
                    relative: relative_of(root, entry.path()),
                    detail: entry.path().to_string_lossy().into_owned(),
                    rule: PreSkipRule::Unreadable,
                });
                continue;
            }
        };
        if meta.is_dir() {
            continue;
        }
        if !meta.is_file() {
            // A symlink (`follow_links(false)` makes this an lstat, so a
            // symlink is neither `is_file` nor `is_dir` here, regardless of
            // what it points at), or a FIFO, socket, device. Named, but not
            // a failure. One extra, FOLLOWING stat — only reached for these
            // rare entries — tells a symlink to a directory (a whole subtree
            // the walk never entered) apart from everything else: a symlink
            // to a file, a dangling symlink (the follow fails; that is not a
            // read error, it is still `NotAFile`), a FIFO, a socket, a
            // device.
            let rule = match std::fs::metadata(entry.path()) {
                Ok(followed) if followed.is_dir() => PreSkipRule::NotAFileSubtree,
                _ => PreSkipRule::NotAFile,
            };
            walked.skipped.push(PreSkip {
                relative: relative_of(root, entry.path()),
                detail: entry.path().to_string_lossy().into_owned(),
                rule,
            });
            continue;
        }
        let absolute = entry.into_path();
        let Ok(rel) = absolute.strip_prefix(root) else {
            continue;
        };
        let Some(relative) = relative_string(rel) else {
            walked.skipped.push(PreSkip {
                relative: None,
                detail: rel.to_string_lossy().into_owned(),
                rule: PreSkipRule::UnrepresentableName,
            });
            continue;
        };
        let Some(on_disk) = on_disk_of(&meta) else {
            walked.unreadable += 1;
            walked.complete = false;
            walked.skipped.push(PreSkip {
                relative: Some(relative.clone()),
                detail: relative,
                rule: PreSkipRule::Unreadable,
            });
            continue;
        };
        walked.found.push(Found {
            relative,
            absolute,
            on_disk,
        });
    }
    // `WalkBuilder::sort_by_file_path` sorts siblings within one directory by
    // `DirEntry::path()` (`ignore-0.4.31/src/walk.rs:621-625`), which is a
    // per-directory comparison, not a comparison of the flattened relative
    // string across the whole tree. A directory and a same-named-prefix file
    // (`middle/` and `middle.txt`) are siblings whose *names* compare
    // `"middle" < "middle.txt"`, so the walker descends into `middle/`
    // before it reaches `middle.txt` — but as relative-path strings
    // `"middle.txt" < "middle/inner.txt"` (`.` is 0x2E, `/` is 0x2F). The
    // builder-level sort is kept for locality; this sort is what actually
    // makes the order the same on two machines.
    walked.found.sort_by(|a, b| a.relative.cmp(&b.relative));
    walked
}

/// Whether the depth-0 entry — the root itself, at the moment the walker
/// reached it — is still something to descend into. Deliberately
/// `std::fs::metadata` (follows symlinks), NOT an lstat: a root that is
/// itself a symlink to a directory is walkable and the walker already
/// recurses into it correctly (confirmed by probe), so an lstat-based check
/// would wrongly reject every symlinked root. `root.is_dir()`, used before
/// the walk is even built, follows symlinks the same way; this matches it.
/// `false` for a dangling symlink, a symlink to a regular file, a plain
/// regular file, or anything `metadata` cannot stat at all — the four
/// non-directory root shapes this crate has to tell apart from "still a
/// directory" (fix round 4, Important finding: this used to be inline and
/// pinned by nothing).
fn root_entry_is_walkable(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

/// `ignore::Error::Partial` bundles several independent failures into one
/// `Err` — a caller that unwraps only the first one silently drops the rest.
/// Flattens every leaf error out, so each gets its own `PreSkip`.
fn flatten_partial(err: &ignore::Error) -> Vec<&ignore::Error> {
    match err {
        ignore::Error::Partial(errs) => errs.iter().flat_map(flatten_partial).collect(),
        other => vec![other],
    }
}

/// The path a walker error names, if it names one at all. `ignore::Error`
/// exposes `.depth()` but no `.path()`; the path lives inside the
/// `WithPath` variant, reached by peeling whatever wrapper variants
/// (`WithDepth`, `WithLineNumber`) sit above it. Callers pass leaf errors
/// already flattened by `flatten_partial`, so `Partial` needs no case here.
fn error_path(err: &ignore::Error) -> Option<&Path> {
    match err {
        ignore::Error::WithPath { path, .. } => Some(path),
        ignore::Error::WithDepth { err, .. } => error_path(err),
        ignore::Error::WithLineNumber { err, .. } => error_path(err),
        _ => None,
    }
}

/// A `PreSkip::relative` from an absolute path: root-stripped and
/// `/`-separated, or `None` when the path is outside the root or not valid
/// UTF-8 once relative — the skip is still recorded, just without a key a
/// reconciliation can compare against `Found::relative`.
fn relative_of(root: &Path, absolute: &Path) -> Option<String> {
    let rel = absolute.strip_prefix(root).ok()?;
    relative_string(rel)
}

/// `/`-separated and valid UTF-8, or nothing at all. Also `None` for an
/// empty component list (`rel == root`): `Some("")` would be a `PreSkip`
/// key that can never equal a real `Found::relative` — shaped like a path,
/// naming nothing — which is worse than admitting there is no key at all
/// (fix round 3, Important finding: this is how a `chmod 000` ROOT, as
/// opposed to a `chmod 000` subdirectory, used to end up in the journal).
fn relative_string(rel: &Path) -> Option<String> {
    let mut out = String::new();
    for (i, part) in rel.components().enumerate() {
        let part = part.as_os_str().to_str()?;
        if i > 0 {
            out.push('/');
        }
        out.push_str(part);
    }
    if out.is_empty() { None } else { Some(out) }
}

pub fn stat(path: &Path) -> Option<OnDisk> {
    on_disk_of(&std::fs::metadata(path).ok()?)
}

fn on_disk_of(meta: &std::fs::Metadata) -> Option<OnDisk> {
    Some(OnDisk {
        size_bytes: i64::try_from(meta.len()).ok()?,
        mtime: mtime_nanos(meta.modified().ok()?),
    })
}

/// Nanoseconds since the epoch, negative before it: at second granularity a
/// file edited twice within one second to the same length is
/// indistinguishable from an untouched one.
///
/// COPIED from `mnema-ingest` (`crates/mnema-ingest/src/lib.rs:715`), not
/// moved — and deliberately no longer identical to it. That copy still
/// returns `Option<i64>` and still refuses (`None`) past roughly year 2262,
/// because ITS caller falls back to re-reading the one file involved when
/// the cheap arm has nothing to compare against — a small, per-file cost.
/// This one cannot afford that answer: `None` here would propagate through
/// `on_disk_of` into `enumerate` and clear `Walked::complete` for the WHOLE
/// watched root over a single file, not just decline to compare that one
/// file (see the paragraph below). Task 5 of the plan retires the
/// `mnema-ingest` copy once this walk becomes the single source of the
/// metadata `ingest_file` compares against; until then the two versions
/// disagree on purpose, not by drift.
///
/// SATURATES rather than refusing when a value does not fit `i64`: `i64::MAX`
/// past roughly year 2262, `i64::MIN` symmetrically before it. This used to
/// return `None`, which made `on_disk_of` return `None`, which made
/// `enumerate` treat the whole file as unreadable and clear
/// `Walked::complete` — so ONE file with a bogus far-future timestamp would
/// permanently forbid deletion under the entire watched root, the exact
/// hazard `PreSkipRule::NotAFile` exists to prevent, reached through another
/// door. macOS clamps `SystemTime` internally, so this host cannot produce
/// such a value, but ext4 (to year 2446) and Windows `FILETIME` (to year
/// 30828) can represent one, and archive extraction or a wrong clock can
/// write one. A file whose timestamp cannot be represented exactly is still
/// a file that can be read and indexed; saturating only costs the cheap arm
/// the ability to notice a LATER edit that changes nothing but an
/// already-saturated mtime — strictly less bad than never deleting again.
///
/// The size arm (`on_disk_of`'s `i64::try_from(meta.len())`) is deliberately
/// NOT changed the same way: on macOS and Linux, `off_t` — the size type
/// behind `stat()` — is itself a signed 64-bit integer, so a size past
/// `i64::MAX` cannot be reported in the first place (confirmed by probe; see
/// the Task 1 report). Windows has no `off_t`; `Metadata::len()` there comes
/// from `nFileSizeHigh`/`nFileSizeLow`, and what actually bounds it is NTFS's
/// own maximum file size (about 8 PB) or ReFS's (about 35 PB), both still
/// far under `i64::MAX` (about 8 EiB). On every platform this ships to, that
/// branch is a guard against something that cannot happen, not a live path —
/// unlike this one.
fn mtime_nanos(modified: SystemTime) -> i64 {
    match modified.duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_nanos()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_nanos())
            .map(|nanos| -nanos)
            .unwrap_or(i64::MIN),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A timestamp `i64` cannot represent exactly must not make the file
    /// unreadable (see the doc comment on `mtime_nanos`) — it saturates
    /// instead of refusing (fix round 3, Important finding).
    #[test]
    fn mtime_nanos_saturates_past_the_representable_range() {
        let far_future = UNIX_EPOCH
            .checked_add(Duration::from_secs(10_000_000_000_000))
            .expect("constructing a far-future SystemTime for this test");

        assert_eq!(mtime_nanos(far_future), i64::MAX);
    }

    #[test]
    fn mtime_nanos_saturates_before_the_representable_range() {
        let far_past = UNIX_EPOCH
            .checked_sub(Duration::from_secs(10_000_000_000_000))
            .expect("constructing a far-past SystemTime for this test");

        assert_eq!(mtime_nanos(far_past), i64::MIN);
    }

    /// The decision the `entry.depth() == 0` arm makes, pinned directly
    /// rather than only through the whole-`enumerate` race, which cannot be
    /// tested deterministically (fix round 4, Important finding: this arm
    /// had no coverage at all — reducing it to a bare `continue` left every
    /// test in `enumerate.rs` green). The four non-directory root shapes
    /// this crate has to tell apart from "still walkable", plus the
    /// ordinary case as a baseline.
    #[test]
    #[cfg(unix)]
    fn root_entry_is_walkable_tells_apart_the_root_shapes_that_matter() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();

        let real_dir = dir.path().join("real_dir");
        std::fs::create_dir(&real_dir).unwrap();
        assert!(
            root_entry_is_walkable(&real_dir),
            "a plain directory is walkable"
        );

        let dir_link = dir.path().join("dir_link");
        symlink(&real_dir, &dir_link).unwrap();
        assert!(
            root_entry_is_walkable(&dir_link),
            "a symlink to a directory is walkable"
        );

        let broken_link = dir.path().join("broken_link");
        symlink(dir.path().join("does_not_exist"), &broken_link).unwrap();
        assert!(
            !root_entry_is_walkable(&broken_link),
            "a dangling symlink is not walkable"
        );

        let real_file = dir.path().join("real_file.txt");
        std::fs::write(&real_file, b"x").unwrap();
        let file_link = dir.path().join("file_link");
        symlink(&real_file, &file_link).unwrap();
        assert!(
            !root_entry_is_walkable(&file_link),
            "a symlink to a regular file is not walkable"
        );

        assert!(
            !root_entry_is_walkable(&real_file),
            "a plain regular file is not walkable"
        );
    }
}
