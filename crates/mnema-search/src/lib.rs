//! The two arms of search, and the rule that puts their answers together.
//!
//! Depends on both `mnema-index` and `mnema-provider`, the same shape as
//! `mnema-embed`: the index cannot reach a network, and the provider knows
//! nothing about chunks.

mod fuse;

pub use fuse::{CANDIDATES, FusionRule, RRF_K, fuse};
