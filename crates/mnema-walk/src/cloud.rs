//! Whether a file's bytes are actually on this disk.
//!
//! Most branches here are a measurement, recorded in §10 of the design with
//! the command that produced it. A platform with no measurement returns
//! `true`: treating an ordinary file as a placeholder would silently stop
//! indexing it, which is a worse failure than the one this module prevents.
//! One branch is not a measurement but a deliberate judgement made in its
//! absence — see the doc comment on `OFFLINE` below for which one and what
//! it costs.
//!
//! Each platform's decision is a pure function of the word the kernel
//! states, and neither predicate below touches a platform API — so both are
//! compiled and tested on every machine this crate builds on, not only the
//! one whose `is_materialised` calls it. A placeholder cannot be created in
//! a temporary directory, so the measured numbers themselves are the test
//! fixtures, and this is what makes a Windows measurement actually checked
//! on macOS and back, rather than merely checkable in principle.

/// `SF_DATALESS` — the kernel states that this file's contents are not on
/// this disk and that reading it will fetch them.
// Unused outside tests on every platform but this one — but the allow is
// narrowed to those platforms on purpose. A blanket `allow` here silences
// the one signal that catches the macOS arm of `is_materialised` being
// gutted: with the body replaced by `true` the tests still pass (they
// exercise the pure predicate), and only "never used" says the protection
// is no longer wired to anything.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const SF_DATALESS: u32 = 0x4000_0000;

/// Measured on 914 real iCloud files, and this is NOT the obvious test.
/// "No allocated blocks" agrees with the flag 908 times and disagrees 6 —
/// every disagreement a dataless file that HAS blocks, which the block test
/// calls local, which makes the walk read it, which downloads it. The flag is
/// what the kernel actually states; blocks are a proxy for it that is wrong in
/// exactly the direction this function exists to prevent.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn materialised_from_st_flags(st_flags: u32) -> bool {
    st_flags & SF_DATALESS == 0
}

/// `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` — opening the file for read blocks
/// on a download.
#[cfg_attr(not(windows), allow(dead_code))]
const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
/// `FILE_ATTRIBUTE_OFFLINE` — the older bit meaning the same thing; some
/// providers set only this one.
///
/// Unlike `RECALL_ON_DATA_ACCESS`, refusing on THIS bit alone is not backed
/// by a measurement: the real OneDrive placeholder on the stand carried it
/// together with `RECALL_ON_DATA_ACCESS` (`0x00401620`, spec §10.1), never by
/// itself. Treating it as sufficient on its own is a judgement made in the
/// absence of a measurement, in the direction the module doc above already
/// calls the safer one — but the cost is real, not hypothetical: a local
/// file some other system (an HSM, a backup tool) sets this flag on for its
/// own reasons is never indexed by this walk, and per the obligation on
/// `PreSkipRule::NotMaterialised` a reconciliation must read it as present,
/// not absent, so it is never deleted for being missing either. It simply
/// never enters the index, silently, for as long as the flag stays set.
#[cfg_attr(not(windows), allow(dead_code))]
const OFFLINE: u32 = 0x0000_1000;

/// Deliberately not keyed on `FILE_ATTRIBUTE_REPARSE_POINT`: measured on the
/// stand, a *downloaded* OneDrive file carries it (Files-On-Demand uses it for
/// everything it manages) and so does every rustup proxy. It would mean the
/// walk refuses files that are entirely local.
#[cfg_attr(not(windows), allow(dead_code))]
fn materialised_from_file_attributes(attrs: u32) -> bool {
    attrs & (RECALL_ON_DATA_ACCESS | OFFLINE) == 0
}

/// Reads no bytes: every branch looks only at metadata already in hand,
/// because reading a placeholder is precisely what this prevents.
#[cfg(target_os = "macos")]
pub fn is_materialised(meta: &std::fs::Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    materialised_from_st_flags(meta.st_flags())
}

#[cfg(windows)]
pub fn is_materialised(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    materialised_from_file_attributes(meta.file_attributes())
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn is_materialised(_meta: &std::fs::Metadata) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact `st_flags` of a real iCloud placeholder, spec §10.1:
    /// `SF_DATALESS` | `UF_COMPRESSED` | `UF_TRACKED`.
    #[test]
    fn the_measured_icloud_placeholder_is_not_materialised() {
        assert!(!materialised_from_st_flags(0x4000_0060));
    }

    /// `UF_COMPRESSED | UF_TRACKED` without `SF_DATALESS`. Measured as the
    /// confound: compression alone does not mean the bytes are elsewhere.
    #[test]
    fn compression_flags_alone_do_not_make_a_placeholder() {
        assert!(materialised_from_st_flags(0x0000_0060));
    }

    #[test]
    fn an_unflagged_file_is_materialised() {
        assert!(materialised_from_st_flags(0));
    }

    /// The exact attribute word of `Getting started with OneDrive.pdf`,
    /// spec §10.1: `RECALL_ON_DATA_ACCESS` and `OFFLINE` together.
    #[test]
    fn the_measured_onedrive_placeholder_is_not_materialised() {
        assert!(!materialised_from_file_attributes(0x0040_1620));
    }

    /// Measured twice on the same machine and the reason this is not a
    /// reparse-point test: `0x420` is `REPARSE_POINT | ARCHIVE`, and it is
    /// what a DOWNLOADED OneDrive file carries — and every rustup proxy
    /// (`cargo.exe`, `rust-analyzer.exe`) as well. A predicate keyed on the
    /// reparse point calls all of those placeholders and stops indexing them.
    #[test]
    fn a_downloaded_onedrive_file_is_materialised() {
        assert!(materialised_from_file_attributes(0x0000_0420));
    }

    /// `RECALL_ON_DATA_ACCESS` alone, without `OFFLINE`: this is the bit that
    /// actually blocks a read on a download. A mask that only recognised
    /// `OFFLINE` would call this materialised, and the walk would read — and
    /// so download — the placeholder, exactly the harm this module exists to
    /// prevent. Catches that specific mutation.
    #[test]
    fn the_recall_bit_alone_is_enough_to_refuse() {
        assert!(!materialised_from_file_attributes(0x0040_0000));
    }

    /// `OFFLINE` on its own is enough to refuse. Not a measurement, unlike
    /// the test above: see the doc comment on `OFFLINE` for what backs this
    /// one instead, and what refusing on it alone costs.
    #[test]
    fn the_offline_bit_alone_is_enough_to_refuse() {
        assert!(!materialised_from_file_attributes(0x0000_1000));
    }

    /// The control that ties the pure functions to real metadata: an
    /// ordinary local file must come out materialised on whatever platform
    /// this is running on, or the walk indexes nothing at all.
    #[test]
    fn an_ordinary_local_file_is_materialised() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("ordinary.txt");
        std::fs::write(&path, vec![b'x'; 4096]).unwrap();
        assert!(is_materialised(&std::fs::metadata(&path).unwrap()));
    }
}
