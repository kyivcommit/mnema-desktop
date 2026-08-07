//! Turns a page's blocks into chunks — the units that get embedded, searched
//! and cited.
//!
//! A pure function: no I/O, no database, no filesystem, no network. The caller
//! has already written the page and its blocks and holds the rowid of each, so
//! it passes them in; `Block` itself carries no id
//! (`crates/mnema-core/src/block.rs:45-46`), and the chunker must not invent
//! one. The output goes to `mnema_index::Db::insert_chunk`.
//!
//! `chunk_blocks` is called **once per page**: a chunk may never cross a page,
//! and the schema enforces it (trigger `chunk_span_blocks_bi`), so a violation
//! surfaces as an insert error rather than as a wrong citation.
//!
//! Two rules run through everything here:
//!
//! * **Characters, never bytes.** A byte-offset implementation passes every
//!   test written over ASCII and shows itself only as a citation quoting the
//!   wrong slice of the first Ukrainian chunk.
//! * **No character is ever modified.** Nothing is trimmed, collapsed,
//!   re-normalised or re-separated: every piece of text emitted is a slice of
//!   some block identified by an offset. The server strips block text before
//!   splitting (`app/index/chunking.py:248`), which shifts every offset
//!   relative to the block it claims to point into.

mod pack;
mod units;
mod view;

use mnema_core::{Block, Coordinate, Locator, Segment};
use pack::Seg;
use view::View;

/// Where a chunk stops growing by preference (D31).
pub const TARGET_CHARS: usize = 900;
/// Where it stops growing absolutely.
pub const MAX_CHARS: usize = 1850;
/// Below this, a trailing chunk is folded into the one before it instead.
pub const MIN_CHARS: usize = 200;
/// The share of a finished chunk re-seeded into the next one.
pub const OVERLAP_RATIO: f64 = 0.15;
/// What separates two pieces that are not adjacent in the source.
pub const JOIN: &str = "\n\n";

/// Bumped for a change in the chunking that the constants above do not capture
/// — the splitting rule, the carry, page-furniture skipping when PDF lands.
///
/// **It went to 2 and came back**, and it is 1 rather than 3 on purpose. Task
/// 11 made a code block standalone, bumped this, and the rule was withdrawn
/// after measurement (see `chunk_blocks`); the packing that came back is
/// byte-identical to the packing that left. This number answers "were two
/// databases chunked the same way?", so it must not claim a change that did
/// not survive — a bump is a re-index of every document in an embedding space,
/// and one for a round trip would be a re-index that buys nothing.
const REV: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub ord: i64,
    pub text: String,
    pub locator: Locator,
}

/// What the caller knows about this page that the chunker cannot see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageContext {
    /// Formats that carry line numbers (txt, md, code): the coordinate is
    /// computed from the blocks each chunk actually covers.
    Lines,
    /// The page's own coordinate, identical for every chunk on it
    /// (`Coordinate::Page` for a PDF, `Coordinate::None` when there is none).
    Fixed(Coordinate),
    /// A spreadsheet sheet: the row range is computed from the blocks each
    /// chunk actually covers, exactly as `Lines` does, and only the rendering
    /// differs — `Coordinate::SheetRows` instead of `Coordinate::Line`.
    ///
    /// **`Fixed` would be wrong here, and not by a little.** A sheet is one
    /// page, so a fixed coordinate is the sheet's whole extent repeated onto
    /// every chunk of it: a chunk of rows 10–20 citing "аркуш Дані, рядки
    /// 1–500", which is non-empty, plausible, and points at fifty times too
    /// much. That is the defect this variant exists to prevent rather than a
    /// nicety — the coordinate is what makes an answer checkable.
    ///
    /// The reader owes every block it emits the sheet rows that block occupies;
    /// one block without them makes the whole range a guess, and `coordinate`
    /// then answers `Coordinate::None` rather than inventing a sheet range.
    Rows { sheet: String },
}

/// Chunks one page's blocks, each paired with the rowid `insert_block`
/// returned for it.
///
/// `first_ord` is how `ord` keeps rising across pages — `UNIQUE(document_id,
/// ord)` collides otherwise. The blocks are used in the order given, which is
/// reading order.
pub fn chunk_blocks(blocks: &[(i64, &Block)], first_ord: i64, page: &PageContext) -> Vec<Chunk> {
    // A block that is nothing but whitespace would contribute a separator and
    // no content. Nothing else is skipped: the server's page-furniture
    // segmentation (`app/config.py:177-182`) exists for PDF headers and
    // footers, which this slice cannot produce — whoever adds PDF revisits it
    // and bumps REV.
    //
    // **And nothing else is segmented, either — that half was tried and
    // withdrawn.** The same server config keeps a standalone block out of its
    // neighbours' chunks, and D41 was read as asking for it once markdown
    // could produce a `BlockType::Code`. Implemented, it did not isolate
    // fences: the rule is symmetric, so a sealed fence cuts the *prose* stream
    // at every fence too. Measured on a README shape — eight `## step` ·
    // paragraph · one-line command triples — it turned 2 chunks of 985 and 515
    // characters into 16 of 136 and 31, every one under `MIN_CHARS`.
    // `tests/invariants.rs::a_page_of_prose_and_fences_does_not_become_all_fragments`
    // is that measurement, kept as a test. Whoever brings the rule back for
    // PDF page furniture needs an answer to it — dropping a header is not the
    // same operation as refusing to pack one.
    //
    // What D41 actually asks for is that a chunk's `source_kind` may differ
    // from its document's, and that does not need segmentation at all:
    // `mnema_ingest::chunk_kind` types a chunk by which kind holds most of its
    // characters.
    let views: Vec<View> = blocks
        .iter()
        .filter(|(_, b)| !b.text.trim().is_empty())
        .map(|(id, b)| View::new(*id, b))
        .collect();

    let pieces: Vec<units::Piece> = views
        .iter()
        .enumerate()
        .flat_map(|(ix, v)| units::pieces_of(ix, v))
        .collect();

    pack::pack(&views, &pieces)
        .into_iter()
        .enumerate()
        .map(|(i, built)| Chunk {
            ord: first_ord + i as i64,
            locator: Locator {
                spans: spans(&built.segs, &views),
                coordinate: coordinate(page, &built.segs, &views),
            },
            text: built.text,
        })
        .collect()
}

fn spans(segs: &[Seg], views: &[View]) -> Vec<Segment> {
    segs.iter()
        .map(|s| Segment {
            block_id: views[s.block].id,
            start: s.chunk_start as u32,
            end: (s.chunk_start + s.len) as u32,
            block_start: s.block_start as u32,
        })
        .collect()
}

fn coordinate(page: &PageContext, segs: &[Seg], views: &[View]) -> Coordinate {
    match page {
        PageContext::Fixed(c) => c.clone(),
        PageContext::Lines => line_range(segs, views),
        // The same computation as `Lines`, wrapped in the sheet's name: the
        // rows a chunk covers, never the sheet's own extent.
        PageContext::Rows { sheet } => match line_range(segs, views) {
            Coordinate::Line { start, end } => Coordinate::SheetRows {
                sheet: sheet.clone(),
                start,
                end,
            },
            // `line_range` has exactly one other answer — `Coordinate::None`,
            // for a block with no rows and for a chunk with no segments — and
            // it is passed through rather than dressed up as a sheet range
            // starting at zero.
            other => other,
        },
    }
}

/// The line range of the blocks a chunk actually names.
fn line_range(segs: &[Seg], views: &[View]) -> Coordinate {
    // Block-granular on purpose: nothing here tracks which line inside a block
    // a character sits on, so a chunk covering half a block still names that
    // block's whole line range. A knowing approximation, not an oversight —
    // narrowing it needs line offsets the reader does not currently emit.
    let mut start = u32::MAX;
    let mut end = 0;
    for seg in segs {
        let v = &views[seg.block];
        let (Some(a), Some(b)) = (v.line_start, v.line_end) else {
            // One block without line numbers makes the whole range a guess.
            // Render nothing rather than invent one
            // (`crates/mnema-core/src/locator.rs:37-39`).
            return Coordinate::None;
        };
        start = start.min(a);
        end = end.max(b);
    }
    if segs.is_empty() {
        return Coordinate::None;
    }
    Coordinate::Line { start, end }
}

/// The value `embedding_space.chunker_hash` needs: what this chunker does, in
/// the form of the constants it does it with.
///
/// A function rather than a hand-maintained constant, because its whole job is
/// to change when the chunking changes — a constant is a value someone forgets
/// to bump, while a string formatted from the constants cannot disagree with
/// them. A test pins the exact output, so touching a constant reddens it and
/// forces a deliberate decision about the vectors already in the database
/// (`crates/mnema-index/src/schema.sql:271,280`). Readable in the database
/// beats an opaque digest.
///
/// Every field is formatted losslessly. The ratio was once written `{:.2}`,
/// which rounded: any value in `[0.145, 0.155)` chunked differently under a
/// byte-identical hash, and two different chunkings would then be written into
/// one embedding space — the exact failure this function exists to prevent.
/// `{}` on an `f64` prints the shortest string that reads back as the same
/// number, so `0.151` cannot hide behind `0.15`.
pub fn chunker_hash() -> String {
    format!(
        "chars/target={TARGET_CHARS}/max={MAX_CHARS}/overlap={OVERLAP_RATIO}/min={MIN_CHARS}/join={}/rev={REV}",
        escape(JOIN)
    )
}

/// Percent-encoding, so a separator made of control characters stays one
/// readable field of the hash.
fn escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || "-._~".contains(ch) {
            out.push(ch);
        } else {
            let mut buf = [0u8; 4];
            for b in ch.encode_utf8(&mut buf).as_bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}
