//! Types shared by every other crate, with no I/O of its own.

mod block;
mod locator;
pub mod manifest;
pub mod nfc;
mod source_kind;
pub mod wire;

pub use block::{Block, BlockType};
pub use locator::{Coordinate, Locator, Segment};
pub use source_kind::SourceKind;

/// A file's identity for the cheap arm: the two numbers `path` records.
///
/// Here rather than in `mnema-walk` because three crates read it — the walk
/// produces it, `mnema-ingest` compares it, `mnema-index` stores it on a skip
/// row — and this is the crate that depends on nothing of ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnDisk {
    pub size_bytes: i64,
    /// Nanoseconds since the epoch, negative before it. Nanoseconds rather than
    /// whole seconds is the whole value of the cheap arm: at second granularity
    /// a file edited twice within one second, to the same length, is
    /// indistinguishable from an untouched one and is never re-indexed.
    pub mtime: i64,
}
