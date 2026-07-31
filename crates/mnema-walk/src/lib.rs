//! What is under a watched root after the rules.
//!
//! This crate is the ONLY place that looks at the disk during a walk. Size and
//! mtime are taken here once and travel to `ingest_file`; taking them again
//! there would compare a file against a stat of a moment that has passed (§5).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod rules;
pub use rules::WalkRules;

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
    /// Not a regular file and not a directory: a symlink (`follow_links` is
    /// off, so `metadata()` is an lstat and never resolves one), a FIFO, a
    /// socket, a device. Naming it here is the whole fix — not following a
    /// symlink is a decision, not a failure, so this does NOT clear
    /// `Walked::complete`.
    NotAFile,
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
}

impl Default for Walked {
    fn default() -> Self {
        Self {
            found: Vec::new(),
            unreadable: 0,
            skipped: Vec::new(),
            complete: true,
        }
    }
}

pub fn enumerate(root: &Path, rules: &WalkRules) -> Walked {
    let mut walked = Walked::default();

    if !root.is_dir() {
        // Not a directory at all: does not exist, or is a regular file (or a
        // symlink to one). Nothing below names the root's own entry — the
        // `entry.depth() == 0` case further down is skipped in silence on
        // purpose — so without this check `found` comes back empty and
        // `complete` stays true, indistinguishable from an empty, real
        // folder.
        walked.unreadable += 1;
        walked.complete = false;
        walked.skipped.push(PreSkip {
            relative: None,
            detail: root.to_string_lossy().into_owned(),
            rule: PreSkipRule::Unreadable,
        });
        return walked;
    }

    let walk = rules.builder(root).build();

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
            continue; // the root itself
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
            // a failure — see `PreSkipRule::NotAFile`.
            walked.skipped.push(PreSkip {
                relative: relative_of(root, entry.path()),
                detail: entry.path().to_string_lossy().into_owned(),
                rule: PreSkipRule::NotAFile,
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

/// `/`-separated and valid UTF-8, or nothing at all.
fn relative_string(rel: &Path) -> Option<String> {
    let mut out = String::new();
    for (i, part) in rel.components().enumerate() {
        let part = part.as_os_str().to_str()?;
        if i > 0 {
            out.push('/');
        }
        out.push_str(part);
    }
    Some(out)
}

pub fn stat(path: &Path) -> Option<OnDisk> {
    on_disk_of(&std::fs::metadata(path).ok()?)
}

fn on_disk_of(meta: &std::fs::Metadata) -> Option<OnDisk> {
    Some(OnDisk {
        size_bytes: i64::try_from(meta.len()).ok()?,
        mtime: mtime_nanos(meta.modified().ok()?)?,
    })
}

/// Nanoseconds since the epoch, negative before it. Moved here from
/// `mnema-ingest` unchanged: at second granularity a file edited twice within
/// one second to the same length is indistinguishable from an untouched one.
fn mtime_nanos(modified: SystemTime) -> Option<i64> {
    match modified.duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_nanos()).ok(),
        Err(before) => i64::try_from(before.duration().as_nanos())
            .ok()
            .map(|nanos| -nanos),
    }
}
