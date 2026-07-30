//! Packing pieces into chunks, and the overlap carry — the part the server
//! gets wrong twice.

use crate::units::Piece;
use crate::view::View;
use crate::{JOIN, MAX_CHARS, MIN_CHARS, OVERLAP_RATIO, TARGET_CHARS};

/// One piece of a chunk under construction. Becomes a `mnema_core::Segment`
/// once the chunk is finished and its rowid can be attached.
#[derive(Debug, Clone)]
pub(crate) struct Seg {
    pub(crate) block: usize,
    pub(crate) block_start: usize,
    pub(crate) len: usize,
    /// Where this piece starts inside the chunk's own text.
    pub(crate) chunk_start: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Building {
    pub(crate) text: String,
    pub(crate) segs: Vec<Seg>,
    /// Characters, not bytes — `text.len()` is the wrong number everywhere.
    pub(crate) len: usize,
    /// How many leading characters came from the previous chunk's tail. They
    /// are already indexed there, so telling them from new content is what
    /// keeps the same text out of the index twice.
    carry_len: usize,
}

fn join_chars() -> usize {
    JOIN.chars().count()
}

impl Building {
    fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    fn has_new_content(&self) -> bool {
        self.len > self.carry_len
    }

    /// True when this piece continues the last segment exactly. Real case: the
    /// carry from block B followed by the next piece of block B. Inserting a
    /// `JOIN` there would put characters into the chunk that are in no block,
    /// and the chunk would stop being rebuildable from its own locator.
    fn continues(&self, piece: &Piece) -> bool {
        self.segs
            .last()
            .is_some_and(|s| s.block == piece.block && s.block_start + s.len == piece.start)
    }

    /// How many characters appending this piece would add, join included.
    fn added(&self, piece: &Piece) -> usize {
        if self.is_empty() || self.continues(piece) {
            piece.len
        } else {
            join_chars() + piece.len
        }
    }

    fn append(&mut self, views: &[View], piece: &Piece) {
        let extend = self.continues(piece);
        if !extend && !self.is_empty() {
            self.text.push_str(JOIN);
            self.len += join_chars();
        }
        self.text
            .push_str(views[piece.block].slice(piece.start, piece.start + piece.len));
        if extend {
            self.segs.last_mut().expect("continues() saw a segment").len += piece.len;
        } else {
            self.segs.push(Seg {
                block: piece.block,
                block_start: piece.start,
                len: piece.len,
                chunk_start: self.len,
            });
        }
        self.len += piece.len;
    }

    fn clear(&mut self) {
        *self = Building::default();
    }

    /// The pieces of this chunk that are *not* the carry.
    fn new_pieces(&self) -> Vec<Piece> {
        self.segs
            .iter()
            .filter(|s| s.chunk_start + s.len > self.carry_len)
            .map(|s| {
                let cut = self.carry_len.saturating_sub(s.chunk_start);
                Piece {
                    block: s.block,
                    start: s.block_start + cut,
                    len: s.len - cut,
                }
            })
            .collect()
    }

    /// The seed of the next chunk: this one's tail, **with every carried piece
    /// keeping its own segment and its own `block_start`**.
    ///
    /// The server instead slices the joined string and attributes the whole
    /// carry to `cur[-1][0]`, the last block
    /// (`app/index/chunking.py:233-241,266-267`). Measured on a three-block
    /// run, 112 of 152 carried characters end up named by the wrong block, and
    /// a highlight then points at text that is not there.
    fn carry(&self, views: &[View]) -> Building {
        let want = (self.len as f64 * OVERLAP_RATIO) as usize;
        let mut next = Building::default();
        if want == 0 {
            return next;
        }
        // The carry is the tail of the chunk, so walk the segments and keep
        // whatever lies at or after this position.
        let from = self.len - want;
        for seg in &self.segs {
            if seg.chunk_start + seg.len <= from {
                continue;
            }
            let mut block_start = seg.block_start;
            let mut len = seg.len;
            // At most one segment straddles `from`; every later one starts
            // after it, so this trims exactly the first kept piece. There is no
            // "have I trimmed yet" flag because there is nothing it could
            // guard — an earlier draft carried one and it was inert.
            if seg.chunk_start < from {
                let cut = from - seg.chunk_start;
                block_start += cut;
                len -= cut;
                match snap(views[seg.block].slice(block_start, block_start + len)) {
                    Snap::Whole => {}
                    Snap::After(k) => {
                        block_start += k;
                        len -= k;
                    }
                    // Nothing but whitespace after the first word break: drop
                    // this piece and let the carry start at the next segment.
                    Snap::Nothing => continue,
                }
            }
            next.append(
                views,
                &Piece {
                    block: seg.block,
                    start: block_start,
                    len,
                },
            );
        }
        next.carry_len = next.len;
        next
    }
}

enum Snap {
    /// No whitespace at all: a carry starting mid-word beats no carry.
    Whole,
    After(usize),
    /// Only whitespace after the first run — nothing worth carrying.
    Nothing,
}

/// A carry cut at 15% of a chunk lands mid-word. Move it forward to the start
/// of the next word so the next chunk does not open on a fragment.
///
/// The offset returned counts characters, like everything else here — which is
/// why this walks the slice rather than indexing it.
fn snap(text: &str) -> Snap {
    let mut chars = text
        .chars()
        .enumerate()
        .skip_while(|(_, c)| !c.is_whitespace());
    if chars.next().is_none() {
        return Snap::Whole;
    }
    match chars.find(|(_, c)| !c.is_whitespace()) {
        Some((at, _)) => Snap::After(at),
        None => Snap::Nothing,
    }
}

/// Packs every piece of a page into chunks, in order.
pub(crate) fn pack(views: &[View], pieces: &[Piece]) -> Vec<Building> {
    let mut out: Vec<Building> = Vec::new();
    let mut cur = Building::default();

    for piece in pieces {
        // The ceiling is checked again *after* appending, below. The server
        // checks it only before, then appends unconditionally
        // (`app/index/chunking.py:271-278`) — measured: 24% of the chunks from
        // its oversized path are over the ceiling.
        if !cur.is_empty() && cur.len + cur.added(piece) > MAX_CHARS {
            flush(&mut out, &mut cur, views);
        }
        if !cur.is_empty() && cur.len + cur.added(piece) > MAX_CHARS {
            // Only the carry can still be in the way, and dropping it always
            // makes room: no piece is longer than MAX_CHARS.
            cur.clear();
        }
        cur.append(views, piece);
        if cur.len >= TARGET_CHARS {
            flush(&mut out, &mut cur, views);
        }
    }

    finish(&mut out, cur, views);
    out
}

fn flush(out: &mut Vec<Building>, cur: &mut Building, views: &[View]) {
    if !cur.has_new_content() {
        // Nothing here but the tail of the chunk before it. Emitting it would
        // index the same characters a second time under a citation of their
        // own — the server's `_merge_tail` defect
        // (`app/index/chunking.py:170-196`) reached from the loop rather than
        // from the end of the page.
        cur.clear();
        return;
    }
    let next = cur.carry(views);
    out.push(std::mem::replace(cur, next));
}

/// The trailing remainder of a page.
fn finish(out: &mut Vec<Building>, cur: Building, views: &[View]) {
    if cur.is_empty() || !cur.has_new_content() {
        return;
    }
    if cur.len < MIN_CHARS && !out.is_empty() {
        // A fragment on its own is a chunk with no context — searchable,
        // citable and useless. Fold its new pieces into the chunk before it,
        // carry excluded: the carry is already that chunk's own tail.
        let mut merged = out.last().expect("checked non-empty").clone();
        for piece in cur.new_pieces() {
            merged.append(views, &piece);
        }
        if merged.len <= MAX_CHARS {
            *out.last_mut().expect("checked non-empty") = merged;
            return;
        }
    }
    out.push(cur);
}
