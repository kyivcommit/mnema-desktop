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
    /// Lossy, and only ever used for display and for the journal: the point of
    /// this variant is that the exact name is NOT representable as text.
    pub display_path: String,
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
    /// directory that vanished mid-walk, or a size that does not fit `i64`.
    /// A directory refused this way takes its whole subtree down with it —
    /// nothing under it becomes `found`, and nothing else names it — so the
    /// path travels here rather than being folded into a bare count
    /// (`Walked::unreadable`). See `Walked::complete`.
    Unreadable,
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
    let walk = rules.builder(root).build();

    for entry in walk {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                // The walker's own error: it could not even list this entry,
                // so a directory here loses its whole subtree, not just
                // itself. `ignore::Error` has no public path accessor, so
                // `error_path` peels the wrapper variants that carry one.
                walked.unreadable += 1;
                walked.complete = false;
                walked.skipped.push(PreSkip {
                    display_path: error_path(&err)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| err.to_string()),
                    rule: PreSkipRule::Unreadable,
                });
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
                    display_path: entry.path().to_string_lossy().into_owned(),
                    rule: PreSkipRule::Unreadable,
                });
                continue;
            }
        };
        if !meta.is_file() {
            continue;
        }
        let absolute = entry.into_path();
        let Ok(rel) = absolute.strip_prefix(root) else {
            continue;
        };
        let Some(relative) = relative_string(rel) else {
            walked.skipped.push(PreSkip {
                display_path: rel.to_string_lossy().into_owned(),
                rule: PreSkipRule::UnrepresentableName,
            });
            continue;
        };
        let Some(on_disk) = on_disk_of(&meta) else {
            walked.unreadable += 1;
            walked.complete = false;
            walked.skipped.push(PreSkip {
                display_path: relative,
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

/// The path a walker error names, if it names one at all. `ignore::Error`
/// exposes `.depth()` but no `.path()`; the path lives inside the
/// `WithPath` variant, reached by peeling whatever wrapper variants
/// (`WithDepth`, `WithLineNumber`, `Partial`) sit above it.
fn error_path(err: &ignore::Error) -> Option<&Path> {
    match err {
        ignore::Error::WithPath { path, .. } => Some(path),
        ignore::Error::WithDepth { err, .. } => error_path(err),
        ignore::Error::WithLineNumber { err, .. } => error_path(err),
        ignore::Error::Partial(errs) => errs.first().and_then(error_path),
        _ => None,
    }
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
