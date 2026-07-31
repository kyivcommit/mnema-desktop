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
}

#[derive(Debug, Default)]
pub struct Walked {
    pub found: Vec<Found>,
    /// How many entries the rules removed. A count, not a list: the list is the
    /// whole excluded tree and can be larger than the kept one by three orders
    /// of magnitude (measured: 411 kept against 384,275 unfiltered).
    pub excluded: u64,
    pub skipped: Vec<PreSkip>,
}

pub fn enumerate(root: &Path, rules: &WalkRules) -> Walked {
    let mut walked = Walked::default();
    let walk = rules.builder(root).build();

    for entry in walk {
        let Ok(entry) = entry else {
            // An entry the walker itself could not read. Counted as excluded
            // rather than invented into a path: there is nothing to record.
            walked.excluded += 1;
            continue;
        };
        if entry.depth() == 0 {
            continue; // the root itself
        }
        let Ok(meta) = entry.metadata() else {
            walked.excluded += 1;
            continue;
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
            walked.excluded += 1;
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
