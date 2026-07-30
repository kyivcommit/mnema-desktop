use mnema_core::Block;

/// A block's text with its character boundaries precomputed.
///
/// Every offset this crate emits is a **character** offset, and `&str` is
/// indexed by bytes. This table is the only place the two meet: a byte-offset
/// implementation passes every test written over ASCII and then shows itself as
/// a citation quoting the wrong slice of the first Ukrainian chunk (the
/// argument on `mnema_index`'s `validate_locator`).
pub(crate) struct View<'a> {
    /// The rowid `mnema_index::Db::insert_block` returned for this block. The
    /// chunker cannot invent it — `Block` carries no id, by design
    /// (`crates/mnema-core/src/block.rs:45-46`).
    pub(crate) id: i64,
    text: &'a str,
    /// Byte offset of every character, plus the text's length.
    ///
    /// `u32`, and this is the whole index: a `View` exists for every block of
    /// the page at once, and "a page" of a text file is the entire file
    /// (`crates/mnema-extract/src/text.rs:38` returns one flat `Vec<Block>`),
    /// bounded only by the pool's 64 MiB ceiling (the default of
    /// `mnema_pool::PoolConfig::max_bytes`). At 4 bytes per character that is
    /// ~270 MB in the worst case; at the 12 bytes a `Vec<usize>` plus a
    /// `Vec<char>` cost, it was ~800 MB — an out-of-memory on a desktop machine
    /// indexing someone's folder, invisible in every test. A block cannot
    /// overflow this: `Segment.block_start` is already `u32`, so a block over
    /// 4 GiB is unrepresentable downstream regardless.
    offsets: Vec<u32>,
    pub(crate) line_start: Option<u32>,
    pub(crate) line_end: Option<u32>,
}

impl<'a> View<'a> {
    pub(crate) fn new(id: i64, block: &'a Block) -> Self {
        let mut offsets: Vec<u32> = block
            .text
            .char_indices()
            .map(|(i, _)| i as u32)
            .chain(std::iter::once(block.text.len() as u32))
            .collect();
        offsets.shrink_to_fit();
        View {
            id,
            text: &block.text,
            offsets,
            line_start: block.line_start,
            line_end: block.line_end,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    /// The text between two character offsets — a slice of the block, never a
    /// string rebuilt from parts. Rebuilding is how the server turns a newline
    /// into a space (`app/index/chunking.py:210,212`) and loses the substring
    /// property every citation depends on.
    pub(crate) fn slice(&self, from: usize, to: usize) -> &'a str {
        &self.text[self.offsets[from] as usize..self.offsets[to] as usize]
    }

    /// Decoded from the text rather than held in a `Vec<char>`: the offset
    /// table already says where the character starts, so keeping a second copy
    /// of the block costs 4 bytes per character to save one UTF-8 decode.
    pub(crate) fn char_at(&self, at: usize) -> char {
        self.text[self.offsets[at] as usize..]
            .chars()
            .next()
            .expect("an offset in the table always starts a character")
    }
}
