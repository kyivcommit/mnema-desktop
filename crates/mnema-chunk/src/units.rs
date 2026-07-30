//! Cutting one block into the largest slices that are still smaller than a
//! chunk. Boundaries are positions, never edits: the concatenation of a block's
//! pieces is the block, character for character.

use crate::view::View;
use crate::{MAX_CHARS, TARGET_CHARS};

/// A contiguous slice of one block, addressed in that block's own character
/// offsets.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Piece {
    /// Index into the `View` list, not a rowid — the rowid is attached only
    /// when the segment is emitted.
    pub(crate) block: usize,
    pub(crate) start: usize,
    pub(crate) len: usize,
}

/// Characters that end a sentence when whitespace follows.
const SENTENCE_END: [char; 4] = ['.', '!', '?', '…'];

/// The pieces of one block, in order, together covering all of it.
pub(crate) fn pieces_of(block: usize, view: &View) -> Vec<Piece> {
    if view.len() == 0 {
        return Vec::new();
    }
    // A block that already fits is one piece — itself. Splitting it would gain
    // nothing and cost a join that the source does not have.
    if view.len() <= MAX_CHARS {
        return vec![Piece {
            block,
            start: 0,
            len: view.len(),
        }];
    }

    let mut out = Vec::new();
    let mut start = 0;
    let mut len = 0;
    for (a, b) in units(view) {
        let unit = b - a;
        if len > 0 && len + unit > MAX_CHARS {
            out.push(Piece { block, start, len });
            len = 0;
        }
        if len == 0 {
            start = a;
        }
        len += unit;
        if len >= TARGET_CHARS {
            out.push(Piece { block, start, len });
            len = 0;
        }
    }
    if len > 0 {
        out.push(Piece { block, start, len });
    }
    out
}

/// The block's text as `[start, end)` ranges whose concatenation is the whole
/// text. Each unit carries its own trailing whitespace, so no separator has to
/// be re-invented when they are put back together.
fn units(view: &View) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < view.len() {
        let end = boundary(view, at);
        out.push((at, end));
        at = end;
    }
    out
}

/// Where the unit starting at `from` ends, in order of preference: after a
/// sentence end, else after the last word boundary that still fits, else a hard
/// cut at `MAX_CHARS`.
///
/// The hard cut is not a nicety: a minified line or a base64 blob has no
/// whitespace anywhere, and without it this loop would never terminate.
fn boundary(view: &View, from: usize) -> usize {
    let n = view.len();
    let limit = (from + MAX_CHARS).min(n);
    let mut last_word = None;
    let mut at = from;
    while at < limit {
        if !view.char_at(at).is_whitespace() {
            at += 1;
            continue;
        }
        let run_start = at;
        let mut run_end = at;
        while run_end < n && view.char_at(run_end).is_whitespace() {
            run_end += 1;
        }
        // A run that spills past the limit is not a boundary this unit can use:
        // cutting inside it would split whitespace across two pieces.
        if run_end <= limit {
            if run_start > from && SENTENCE_END.contains(&view.char_at(run_start - 1)) {
                return run_end;
            }
            last_word = Some(run_end);
        }
        at = run_end;
    }
    last_word.unwrap_or(limit)
}
