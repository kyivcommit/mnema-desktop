//! Types shared by every other crate, with no I/O of its own.

mod block;
mod locator;
pub mod nfc;
mod source_kind;
pub mod wire;

pub use block::{Block, BlockType};
pub use locator::{Coordinate, Locator, Segment};
pub use source_kind::SourceKind;
