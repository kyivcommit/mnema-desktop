//! The chunker's whole defence.
//!
//! The gold archive cannot judge this crate: the server chunks to different
//! constants (320 tokens × 3.2 ≈ 1024/1536 characters against our 900/1850) and
//! its `char_span` is measurably wrong on two counts — the overlap is attributed
//! to the wrong block (`app/index/chunking.py:233-241,266-267`) and the tail is
//! merged back onto the chunk it was taken from (`app/index/chunking.py:170-196`).
//! So there is no reference output to diff against, and these properties are the
//! only thing between a correct chunker and a plausible one.
//!
//! Every fixture is synthetic. No real personal data reaches a test here.

use mnema_chunk::{
    Chunk, JOIN, MAX_CHARS, MIN_CHARS, OVERLAP_RATIO, PageContext, TARGET_CHARS, chunk_blocks,
    chunker_hash,
};
use mnema_core::{Block, BlockType, Coordinate};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Synthetic Ukrainian clauses. Deliberately invented; no real person, company
/// or contract is named here.
const UK: [&str; 8] = [
    "Комісія розглянула звернення щодо постачання лабораторного обладнання",
    "Строк виконання робіт становить дев'яносто календарних днів від дати підписання",
    "Виконавець зобов'язаний передати замовникові повний комплект документації",
    "Оплата здійснюється двома рівними частинами після приймання кожного етапу",
    "Сторони погодили, що зміни до цього додатка оформлюються письмово",
    "Гарантійний строк на змонтоване обладнання дорівнює двадцяти чотирьом місяцям",
    "Приймання виконаних робіт підтверджується актом за формою, наведеною нижче",
    "У разі прострочення нараховується пеня в розмірі облікової ставки",
];

const LATIN: [&str; 4] = [
    "The commission reviewed the request concerning laboratory equipment",
    "Delivery is due within ninety calendar days of the signing date",
    "The contractor hands over the full set of documentation on acceptance",
    "Payment falls due in two equal parts after each stage is accepted",
];

/// Exactly `n` characters of synthetic prose, sentences separated by `sep`.
fn prose(n: usize, seed: usize, sep: &str) -> String {
    let mut out = String::new();
    let mut i = seed;
    while out.chars().count() < n + 2 {
        out.push_str(UK[i % UK.len()]);
        out.push('.');
        out.push_str(sep);
        i += 1;
    }
    out.chars().take(n).collect()
}

fn latin(n: usize, seed: usize) -> String {
    let mut out = String::new();
    let mut i = seed;
    while out.chars().count() < n + 2 {
        out.push_str(LATIN[i % LATIN.len()]);
        out.push_str(". ");
        i += 1;
    }
    out.chars().take(n).collect()
}

/// Exactly `MAX_CHARS` characters whose last 15% holds a single space, five
/// characters from the end. The carry is computed over 277 characters and
/// `snap` then throws away all but 7 of them, so the next chunk starts far
/// smaller than the ratio suggests.
fn one_late_space() -> String {
    let mut s: String = "слово ".repeat(300).chars().take(1570).collect();
    s.push_str(&"ц".repeat(272));
    s.push(' ');
    s.push_str(&"я".repeat(7));
    assert_eq!(s.chars().count(), MAX_CHARS);
    s
}

/// Words, then a run of 120 spaces straddling `MAX_CHARS`, then more words —
/// what a page of code followed by a stretch of blank lines looks like.
///
/// **No sentence ends anywhere**, and that is the whole point of the fixture
/// rather than a detail: `units::boundary` returns at the first run preceded by
/// a `SENTENCE_END`, so any prose here would be cut long before the run and the
/// case would never be reached. Deliberately not built from `prose`, which adds
/// a full stop to every clause.
fn straddling_run() -> String {
    let words: String = "слово ".repeat(400).chars().take(1800).collect();
    let tail: String = "слово ".repeat(10).chars().take(36).collect();
    let run = " ".repeat(120);

    // The run has to start under the ceiling and end over it, or there is
    // nothing straddling and the fixture is asserting an ordinary split.
    // Measured off the strings themselves and against `MAX_CHARS`, so that
    // moving the constant reddens this rather than quietly retiring the case.
    let run_start = words.chars().count();
    let run_end = run_start + run.chars().count();
    assert!(
        run_start < MAX_CHARS && run_end > MAX_CHARS,
        "the run from {run_start} to {run_end} must straddle {MAX_CHARS}"
    );
    format!("{words}{run}{tail}")
}

/// `n` characters with no whitespace and no sentence end anywhere — a minified
/// line or a base64 blob. Only the hard cut at `MAX_CHARS` terminates this.
fn unbroken(n: usize) -> String {
    "aGVsbG8Xd29ybGQ7c3ludGhldGljOw"
        .repeat(n.div_ceil(30))
        .chars()
        .take(n)
        .collect()
}

fn block(text: String, line_start: Option<u32>, line_end: Option<u32>) -> Block {
    Block {
        block_type: BlockType::Paragraph,
        reading_order: 0,
        language: Some("uk".into()),
        text,
        line_start,
        line_end,
    }
}

/// A fenced or indented code block — the one block type that is chunked on its
/// own (D41).
fn code(text: String, line_start: Option<u32>, line_end: Option<u32>) -> Block {
    Block {
        block_type: BlockType::Code,
        reading_order: 0,
        language: None,
        text,
        line_start,
        line_end,
    }
}

/// Synthetic source, `n` characters of it. Deliberately camel-cased: the
/// identifier split that `SourceKind::Code` turns on downstream is the reason
/// a code chunk must not be mixed with prose in the first place.
fn source(n: usize) -> String {
    let mut out = String::new();
    let mut i = 0;
    while out.chars().count() < n {
        out.push_str(&format!(
            "let userName{i} = readConfigValue(\"обладнання\");\n"
        ));
        i += 1;
    }
    out.chars().take(n).collect()
}

/// Blocks paired with the rowids the caller got back from `insert_block`.
fn ids(blocks: &[Block]) -> Vec<(i64, &Block)> {
    blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (100 + i as i64, b))
        .collect()
}

struct Case {
    name: &'static str,
    blocks: Vec<Block>,
    page: PageContext,
}

/// Every case the general properties (1, 3, 4, 5) are asserted over. A property
/// asserted on one happy fixture is a property that has never been tested.
fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "prose 800/100/900 — the carry reaches back over a join",
            blocks: vec![
                block(prose(800, 0, " "), Some(1), Some(9)),
                block(prose(100, 3, " "), Some(11), Some(12)),
                block(prose(900, 5, " "), Some(14), Some(30)),
            ],
            page: PageContext::Lines,
        },
        Case {
            name: "one block far over the ceiling",
            blocks: vec![block(prose(5000, 1, " "), Some(1), Some(64))],
            page: PageContext::Lines,
        },
        Case {
            name: "sentences separated by newlines",
            blocks: vec![block(prose(3000, 2, "\n"), Some(1), Some(40))],
            page: PageContext::Fixed(Coordinate::Page { number: 7 }),
        },
        Case {
            name: "no whitespace at all",
            blocks: vec![block(unbroken(4000), None, None)],
            page: PageContext::Fixed(Coordinate::None),
        },
        Case {
            // A run of whitespace straddling the ceiling, which is what a page
            // of code followed by blank lines looks like. It is the third path
            // to a chunk over MAX_CHARS, and the only one no fixture reached:
            // `units::boundary` refuses a run that spills past the limit —
            // cutting inside one would split whitespace across two pieces and
            // return a boundary beyond `from + MAX_CHARS`. Removing that guard
            // left the whole workspace green and produced a 1 920-character
            // chunk here.
            //
            // A piece over MAX_CHARS also falsifies what `pack.rs` states in
            // words and relies on for its carry argument, so this fixture
            // stands behind two claims, not one.
            name: "a run of whitespace straddling the ceiling",
            blocks: vec![block(straddling_run(), Some(1), Some(24))],
            page: PageContext::Lines,
        },
        Case {
            name: "whitespace but no sentence end",
            blocks: vec![block(
                "слово ".repeat(500).chars().take(2600).collect(),
                Some(3),
                Some(3),
            )],
            page: PageContext::Lines,
        },
        Case {
            name: "many small blocks",
            blocks: (0..14)
                .map(|i| {
                    block(
                        prose(80, i, " "),
                        Some(i as u32 * 2 + 1),
                        Some(i as u32 * 2 + 2),
                    )
                })
                .collect(),
            page: PageContext::Fixed(Coordinate::Page { number: 3 }),
        },
        Case {
            name: "short tail after three full blocks",
            blocks: vec![
                block(prose(900, 0, " "), Some(1), Some(12)),
                block(prose(900, 2, " "), Some(13), Some(24)),
                block(prose(900, 4, " "), Some(25), Some(36)),
                block(prose(40, 6, " "), Some(37), Some(37)),
            ],
            page: PageContext::Lines,
        },
        Case {
            name: "one tiny block",
            blocks: vec![block("Додаток 3.".into(), Some(4), Some(4))],
            page: PageContext::Lines,
        },
        Case {
            name: "blank blocks between real ones",
            blocks: vec![
                block(prose(300, 0, " "), Some(1), Some(4)),
                block("   \n\t ".into(), Some(5), Some(5)),
                block(prose(300, 4, " "), Some(6), Some(9)),
            ],
            page: PageContext::Lines,
        },
        Case {
            name: "latin prose",
            blocks: vec![
                block(latin(1200, 0), Some(1), Some(16)),
                block(latin(700, 1), Some(17), Some(26)),
            ],
            page: PageContext::Lines,
        },
        Case {
            name: "a block exactly at the ceiling",
            blocks: vec![block(prose(MAX_CHARS, 0, " "), Some(1), Some(24))],
            page: PageContext::Lines,
        },
        Case {
            // The only shape that reaches the "drop the carry" branch: a piece
            // of exactly MAX_CHARS arriving after a flush leaves no room for
            // the carry beside it, whatever the carry's size.
            name: "a short block then one exactly at the ceiling",
            blocks: vec![
                block(prose(300, 0, " "), Some(1), Some(4)),
                block(prose(MAX_CHARS, 3, " "), Some(5), Some(28)),
            ],
            page: PageContext::Lines,
        },
        Case {
            // An oversized block whose last piece is a 25-character remainder:
            // the fold fires and the merged chunk still fits. The only fixture
            // that reaches the fold *and* stays under the ceiling, which is the
            // one shape where the carry exclusion in `new_pieces` shows.
            // `the_fold_leaves_the_carry_behind` below asserts the shape, so
            // this stops being silent if the prose ever changes length.
            name: "an oversized block with a remainder that folds back",
            blocks: vec![block(prose(1950, 0, " "), None, None)],
            page: PageContext::Fixed(Coordinate::None),
        },
        Case {
            // The fold's own ceiling guard. `snap` can shrink a carry far below
            // 15% when the only whitespace in the carry region sits near its
            // end — here the carry is 5 characters, not 277, so the remainder
            // is under MIN_CHARS while the previous chunk is still full.
            name: "a carry that snapping shrinks to nothing much",
            blocks: vec![
                block(one_late_space(), Some(1), Some(1)),
                block(prose(150, 2, " "), Some(2), Some(3)),
            ],
            page: PageContext::Lines,
        },
        Case {
            // Two characters decide this one: 400 + 1450 is exactly MAX_CHARS,
            // so the ceiling holds only if the JOIN between them is counted.
            name: "two blocks that fit only without the join",
            blocks: vec![
                block(prose(400, 1, " "), Some(1), Some(6)),
                block(prose(1450, 4, " "), Some(7), Some(26)),
            ],
            page: PageContext::Lines,
        },
        Case {
            // A fence between two paragraphs: the shape a markdown section
            // has, and the one the standalone rule exists for.
            name: "a fence between two paragraphs",
            blocks: vec![
                block(prose(500, 0, " "), Some(1), Some(7)),
                code(source(300), Some(9), Some(14)),
                block(prose(500, 2, " "), Some(16), Some(23)),
            ],
            page: PageContext::Lines,
        },
        Case {
            // A fence far over the ceiling, so that the pieces of one code
            // block have to pack with each other — which they may, unlike
            // pieces of two different blocks.
            name: "a fence over the ceiling between two paragraphs",
            blocks: vec![
                block(prose(400, 1, " "), Some(1), Some(6)),
                code(source(4000), Some(8), Some(90)),
                block(prose(400, 3, " "), Some(92), Some(98)),
            ],
            page: PageContext::Lines,
        },
        Case {
            // A fence too short to stand on its own, at the end of a page.
            // This is the shape the fold reaches for: without a kind check
            // there, a one-line fence is merged into the paragraph above it
            // and the chunk is both kinds at once.
            name: "a fence under the minimum after a full paragraph",
            blocks: vec![
                block(prose(900, 0, " "), Some(1), Some(12)),
                code("let userName = 1;".into(), Some(14), Some(14)),
            ],
            page: PageContext::Lines,
        },
        Case {
            // Two fences with nothing between them: adjacent standalone blocks
            // are not each other's company either.
            name: "two fences in a row",
            blocks: vec![
                code(source(200), Some(1), Some(4)),
                code(source(200), Some(6), Some(9)),
            ],
            page: PageContext::Lines,
        },
        Case {
            // A one-character chunk with its own row, embedding and citation.
            // Nothing can be joined to a block that fills the ceiling by
            // itself, so this is what the ceiling costs at its worst.
            name: "one character before a block that fills the ceiling",
            blocks: vec![
                block("Я".into(), Some(1), Some(1)),
                block(prose(MAX_CHARS, 5, " "), Some(2), Some(25)),
            ],
            page: PageContext::Lines,
        },
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn char_slice(s: &str, start: usize, end: usize) -> String {
    s.chars().skip(start).take(end - start).collect()
}

fn n_chars(s: &str) -> usize {
    s.chars().count()
}

fn find_block<'a>(blocks: &'a [(i64, &'a Block)], id: i64) -> &'a Block {
    blocks
        .iter()
        .find(|(bid, _)| *bid == id)
        .unwrap_or_else(|| panic!("segment names block {id}, which was not passed in"))
        .1
}

/// Rebuilds a chunk's text from its locator alone — reading each segment out of
/// its own block — which is exactly what a citation renderer has to do. If this
/// disagrees with `chunk.text`, every highlight this product ever draws is a
/// guess.
fn rebuild(chunk: &Chunk, blocks: &[(i64, &Block)]) -> String {
    let mut out = String::new();
    let mut prev: Option<&mnema_core::Segment> = None;
    for seg in &chunk.locator.spans {
        let b = find_block(blocks, seg.block_id);
        let len = (seg.end - seg.start) as usize;
        if let Some(p) = prev {
            let contiguous = p.block_id == seg.block_id
                && p.block_start as usize + (p.end - p.start) as usize == seg.block_start as usize;
            if !contiguous {
                out.push_str(JOIN);
            }
        }
        let from = seg.block_start as usize;
        out.push_str(&char_slice(&b.text, from, from + len));
        prev = Some(seg);
    }
    out
}

/// The source range each segment of a chunk claims, as `(block_id, start, end)`
/// in the block's own character offsets.
fn covered(chunk: &Chunk) -> Vec<(i64, usize, usize)> {
    chunk
        .locator
        .spans
        .iter()
        .map(|s| {
            let start = s.block_start as usize;
            (s.block_id, start, start + (s.end - s.start) as usize)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 1. The headline property
// ---------------------------------------------------------------------------

#[test]
fn a_chunk_can_be_rebuilt_from_its_locator_alone() {
    for case in cases() {
        let blocks = ids(&case.blocks);
        let chunks = chunk_blocks(&blocks, 0, &case.page);
        assert!(!chunks.is_empty(), "{}: produced no chunks", case.name);
        for chunk in &chunks {
            assert_eq!(
                rebuild(chunk, &blocks),
                chunk.text,
                "{}: chunk {} does not read back out of its own blocks",
                case.name,
                chunk.ord
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 2. The overlap
// ---------------------------------------------------------------------------

#[test]
fn the_overlap_is_attributed_to_the_block_it_came_from() {
    // 800/100/900 rather than the server's measured 1000/40/900: under our
    // constants a 1000-character block flushes on its own, so the carry would
    // never cross a join and the property would go untested. 800 + JOIN + 100
    // is the first flush, so the 15% carry reaches back past the join into the
    // first block. The server attributes the whole carry to `cur[-1][0]`, the
    // last block (`app/index/chunking.py:266-267`) — 112 of 152 characters to
    // the wrong block on its own measured run.
    let blocks = vec![
        block(prose(800, 0, " "), Some(1), Some(9)),
        block(prose(100, 3, " "), Some(11), Some(12)),
        block(prose(900, 5, " "), Some(14), Some(30)),
    ];
    let with_ids = ids(&blocks);
    let chunks = chunk_blocks(&with_ids, 0, &PageContext::Lines);
    assert!(
        chunks.len() >= 2,
        "expected the fixture to flush at least once"
    );

    let second = &chunks[1];
    let named: Vec<i64> = second.locator.spans.iter().map(|s| s.block_id).collect();
    assert_eq!(
        named,
        vec![100, 101, 102],
        "the carry crossed a join, so it must keep one segment per block it came \
         from — not one segment attributed to the last block"
    );

    // and every carried character reads out of the block its segment names
    for seg in &second.locator.spans {
        let b = find_block(&with_ids, seg.block_id);
        let from = seg.block_start as usize;
        let len = (seg.end - seg.start) as usize;
        assert_eq!(
            char_slice(&b.text, from, from + len),
            char_slice(&second.text, seg.start as usize, seg.end as usize),
            "segment of block {} does not read back",
            seg.block_id
        );
    }

    // the carried part is a real suffix of the chunk it came from
    let carried = char_slice(&second.text, 0, second.locator.spans[1].end as usize);
    assert!(
        chunks[0].text.ends_with(&carried),
        "the carry must be the tail of the previous chunk, verbatim"
    );
}

#[test]
fn the_carry_starts_at_a_word_boundary() {
    // 15% of a chunk lands mid-word. A chunk that opens on "ання" embeds a
    // fragment and cites one, so the carry is snapped forward to the next word.
    for case in cases() {
        let blocks = ids(&case.blocks);
        let chunks = chunk_blocks(&blocks, 0, &case.page);
        for pair in chunks.windows(2) {
            let first = pair[1].locator.spans[0];
            let source: Vec<char> = find_block(&blocks, first.block_id).text.chars().collect();
            let at = first.block_start as usize;
            // Only meaningful where the chunk before it ended mid-block: a
            // carry that begins where a piece begins is already at a boundary.
            if at == 0 || !source[at..].iter().any(|c| c.is_whitespace()) {
                continue;
            }
            assert!(
                source[at - 1].is_whitespace(),
                "{}: chunk {} opens mid-word at {at}: {:?}",
                case.name,
                pair[1].ord,
                source[at.saturating_sub(6)..(at + 6).min(source.len())]
                    .iter()
                    .collect::<String>()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 3, 4, 5, 6. Structural properties over every fixture
// ---------------------------------------------------------------------------

#[test]
fn no_source_character_appears_twice_in_one_chunk() {
    for case in cases() {
        let blocks = ids(&case.blocks);
        for chunk in chunk_blocks(&blocks, 0, &case.page) {
            let ranges = covered(&chunk);
            for (i, a) in ranges.iter().enumerate() {
                for b in &ranges[i + 1..] {
                    if a.0 == b.0 {
                        assert!(
                            a.2 <= b.1 || b.2 <= a.1,
                            "{}: chunk {} names block {} at [{}, {}) and [{}, {}) — \
                             the same source characters twice",
                            case.name,
                            chunk.ord,
                            a.0,
                            a.1,
                            a.2,
                            b.1,
                            b.2
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn spans_are_ordered_and_never_overlap() {
    // These are exactly the conditions `mnema_index::write::validate_locator`
    // enforces at insert time (`crates/mnema-index/src/write.rs:222-253`).
    // Asserted here so a failure names the chunker rather than the database.
    let join_len = n_chars(JOIN) as u32;
    for case in cases() {
        let blocks = ids(&case.blocks);
        for chunk in chunk_blocks(&blocks, 0, &case.page) {
            let spans = &chunk.locator.spans;
            assert!(!spans.is_empty(), "{}: chunk with no span", case.name);
            assert_eq!(
                spans[0].start, 0,
                "{}: chunk starts inside no span",
                case.name
            );
            let mut prev_end = None;
            for seg in spans {
                assert!(seg.start <= seg.end, "{}: inverted span", case.name);
                if let Some(prev_end) = prev_end {
                    assert!(
                        seg.start >= prev_end,
                        "{}: span at {} overlaps the one ending at {prev_end}",
                        case.name,
                        seg.start
                    );
                    // the only thing allowed between two segments is the JOIN
                    let gap = seg.start - prev_end;
                    assert!(
                        gap == 0 || gap == join_len,
                        "{}: {gap} characters between segments belong to no block",
                        case.name
                    );
                    if gap != 0 {
                        assert_eq!(
                            char_slice(&chunk.text, prev_end as usize, seg.start as usize),
                            JOIN,
                            "{}: the gap between segments is not the join",
                            case.name
                        );
                    }
                }
                prev_end = Some(seg.end);
            }
            assert_eq!(
                prev_end.unwrap() as usize,
                n_chars(&chunk.text),
                "{}: the chunk's last characters belong to no span",
                case.name
            );
        }
    }
}

#[test]
fn no_chunk_exceeds_the_ceiling() {
    for case in cases() {
        let blocks = ids(&case.blocks);
        for chunk in chunk_blocks(&blocks, 0, &case.page) {
            assert!(
                n_chars(&chunk.text) <= MAX_CHARS,
                "{}: chunk {} is {} characters, ceiling is {MAX_CHARS}",
                case.name,
                chunk.ord,
                n_chars(&chunk.text)
            );
        }
    }
}

#[test]
fn a_chunk_falls_below_the_minimum_only_when_the_ceiling_forced_it() {
    // The plain reading — "every chunk but the last meets MIN_CHARS" — is not
    // true and cannot be made true here. Blocks ["Я", <1850 characters>] give a
    // one-character non-last chunk with a row, an embedding and a citation of
    // its own, because nothing can be joined to a block that fills the ceiling
    // by itself. The alternatives are breaching the ceiling or splitting a
    // block already under it, both worse; whether a chunk that short should be
    // embedded and cited at all belongs to the indexing and search specs.
    //
    // So this asserts what is actually true, with the exception named: a chunk
    // is short only when the next one could not have been joined to it. A
    // short chunk that had room beside it is a packing bug and reddens here.
    //
    // It briefly had a second exemption, for a pair of chunks belonging to
    // different code blocks, when a code block was chunked on its own. The
    // exemption was true — such a pair could not have been joined however much
    // room there was — and it was also the case that hid the cost of the rule
    // that made it true, since after it the crate said nothing at all about
    // how short a code-adjacent chunk may be. The rule is gone and so is the
    // exemption: the ceiling is once again the only thing that may produce a
    // short chunk.
    let join = n_chars(JOIN);
    let mut short_seen = 0;
    for case in cases() {
        let blocks = ids(&case.blocks);
        let chunks = chunk_blocks(&blocks, 0, &case.page);
        for pair in chunks.windows(2) {
            let len = n_chars(&pair[0].text);
            if len >= MIN_CHARS {
                continue;
            }
            short_seen += 1;
            assert!(
                len + join + n_chars(&pair[1].text) > MAX_CHARS,
                "{}: chunk {} is {len} characters and chunk {} is {}, which would \
                 have fitted in one — the flush was not forced by the ceiling",
                case.name,
                pair[0].ord,
                pair[1].ord,
                n_chars(&pair[1].text)
            );
        }
    }
    assert!(
        short_seen > 0,
        "no fixture produces a short chunk, so this test asserted nothing"
    );
}

/// The same guarantee for the shape a documentation folder is actually made
/// of, and the test that would have priced the rule this chunker briefly had.
///
/// A README is a run of `## step` · paragraph · one-line command. Between
/// 2026-07-27 and the review of this branch, a code block was chunked strictly
/// on its own, which sounds like a rule about fences and is not: it cuts the
/// **prose** stream at every fence too. Measured on this exact fixture, the
/// rule turned 2 chunks of 985 and 515 characters into 16 chunks of 135 and 31
/// — every one of them under `MIN_CHARS`, which is the size this crate's own
/// comment calls "a chunk with no context: searchable, citable and useless".
///
/// The median rather than every chunk, because the last chunk of a page is
/// legitimately a remainder and a fixture that forbids one would be asserting
/// something else.
#[test]
fn a_page_of_prose_and_fences_does_not_become_all_fragments() {
    let mut blocks = Vec::new();
    for step in 1..=8 {
        blocks.push(block(
            format!("## Крок {step}"),
            Some(step * 6),
            Some(step * 6),
        ));
        blocks.push(block(prose(125, step as usize, " "), None, None));
        blocks.push(code(
            format!("mnema index --root ./документи{step}"),
            None,
            None,
        ));
    }
    let with_ids = ids(&blocks);
    let chunks = chunk_blocks(&with_ids, 0, &PageContext::Lines);

    let mut lengths: Vec<usize> = chunks.iter().map(|c| n_chars(&c.text)).collect();
    assert!(!lengths.is_empty(), "the fixture must produce chunks");
    lengths.sort_unstable();
    let median = lengths[lengths.len() / 2];
    assert!(
        median >= MIN_CHARS,
        "the median chunk is {median} characters over {} chunks — a page of ordinary \
         documentation has been cut into fragments: {lengths:?}",
        lengths.len()
    );
}

#[test]
fn a_page_of_ordinary_prose_has_no_short_chunk_at_all() {
    let blocks = vec![
        block(prose(900, 0, " "), Some(1), Some(12)),
        block(prose(900, 2, " "), Some(13), Some(24)),
        block(prose(900, 4, " "), Some(25), Some(36)),
        block(prose(40, 6, " "), Some(37), Some(37)),
    ];
    let with_ids = ids(&blocks);
    let chunks = chunk_blocks(&with_ids, 0, &PageContext::Lines);
    assert!(chunks.len() >= 2, "the fixture must produce several chunks");
    for chunk in &chunks[..chunks.len() - 1] {
        assert!(
            n_chars(&chunk.text) >= MIN_CHARS,
            "chunk {} is {} characters",
            chunk.ord,
            n_chars(&chunk.text)
        );
    }
}

#[test]
fn a_short_tail_is_folded_into_the_previous_chunk() {
    // The 40-character block cannot stand alone: it is under MIN_CHARS, so its
    // pieces join the previous chunk (§3.6). Emitting it as its own chunk would
    // put a fragment with no context into the index.
    let blocks = vec![
        block(prose(900, 0, " "), Some(1), Some(12)),
        block(prose(900, 2, " "), Some(13), Some(24)),
        block(prose(900, 4, " "), Some(25), Some(36)),
        block("Додаток 3 до цього договору.".into(), Some(37), Some(37)),
    ];
    let with_ids = ids(&blocks);
    let chunks = chunk_blocks(&with_ids, 0, &PageContext::Lines);
    let last = chunks.last().unwrap();
    assert!(
        last.text.ends_with("Додаток 3 до цього договору."),
        "the tail must end the last chunk, got: {:?}",
        last.text.chars().rev().take(40).collect::<String>()
    );
    assert!(
        last.locator.spans.iter().any(|s| s.block_id == 103),
        "the folded tail must bring its own segment"
    );
    assert!(
        n_chars(&last.text) >= MIN_CHARS,
        "the fold exists so the tail does not become a {}-character chunk",
        n_chars(&last.text)
    );
}

/// Renamed from `the_fold_leaves_the_carry_behind`, which claimed more than it
/// checks: this is one fixture at the fold, and the guard that holds the
/// property in general — that no chunk repeats only what the one before it
/// already had — is `every_chunk_carries_source_the_previous_one_did_not`.
#[test]
fn the_folded_remainder_adds_only_its_own_characters() {
    // The direct counter to the server's `_merge_tail`
    // (`app/index/chunking.py:170-196`), which appends the carry back onto the
    // very chunk it was taken from — 155 characters indexed twice on its
    // measured run, 149 on ours.
    //
    // One block of 1 950 characters splits into three pieces, the last of them
    // 25 characters long. That remainder is under MIN_CHARS, so it is folded
    // into the chunk before it — and only its 25 new characters go, not the
    // ~160 of carry sitting in front of them.
    let blocks = vec![block(prose(1950, 0, " "), None, None)];
    let with_ids = ids(&blocks);
    let chunks = chunk_blocks(&with_ids, 0, &PageContext::Fixed(Coordinate::None));

    // If this is ever 3, the fixture stopped reaching the fold and the rest of
    // the test is asserting nothing — retune the length rather than delete it.
    assert_eq!(
        chunks.len(),
        2,
        "the fixture must reach the fold: {:?}",
        chunks.iter().map(|c| n_chars(&c.text)).collect::<Vec<_>>()
    );

    let last = chunks.last().unwrap();
    assert_eq!(
        last.locator.spans.len(),
        1,
        "the fold appended text contiguous with what was already there, so the \
         chunk is still one slice of one block: {:?}",
        last.locator.spans
    );
    assert!(
        !last.text.contains(JOIN),
        "a JOIN in a chunk drawn from one contiguous run means the carry was \
         folded back in beside itself"
    );
    let span = last.locator.spans[0];
    assert_eq!(
        span.block_start as usize + (span.end - span.start) as usize,
        n_chars(&blocks[0].text),
        "the folded remainder must reach the end of the block"
    );
}

#[test]
fn every_chunk_carries_source_the_previous_one_did_not() {
    // The server's `_merge_tail` (`app/index/chunking.py:170-196`) appends the
    // carry back onto the very chunk it was taken from — 155 characters indexed
    // twice on its measured run. The same shape appears mid-loop when the next
    // piece cannot fit beside the carry. A chunk that is nothing but the tail of
    // its predecessor is duplicated text with a citation pointing at it.
    for case in cases() {
        let blocks = ids(&case.blocks);
        let chunks = chunk_blocks(&blocks, 0, &case.page);
        for pair in chunks.windows(2) {
            let before = covered(&pair[0]);
            let new_source = covered(&pair[1]).into_iter().any(|(id, s, e)| {
                !before
                    .iter()
                    .any(|(bid, bs, be)| *bid == id && *bs <= s && e <= *be)
            });
            assert!(
                new_source,
                "{}: chunk {} is entirely contained in chunk {}",
                case.name, pair[1].ord, pair[0].ord
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 7. Ordinals
// ---------------------------------------------------------------------------

#[test]
fn ord_continues_across_slices() {
    let first = vec![block(prose(2000, 0, " "), Some(1), Some(26))];
    let second = vec![block(prose(2000, 3, " "), Some(27), Some(52))];
    let a = chunk_blocks(&ids(&first), 0, &PageContext::Lines);
    let last = a.last().unwrap().ord;
    let b = chunk_blocks(&ids(&second), last + 1, &PageContext::Lines);

    assert_eq!(a[0].ord, 0);
    for (i, c) in a.iter().enumerate() {
        assert_eq!(c.ord, i as i64, "ords within a call are consecutive");
    }
    assert!(
        b[0].ord > last,
        "`UNIQUE(document_id, ord)` collides otherwise: {} !> {last}",
        b[0].ord
    );
    assert_eq!(b[0].ord, last + 1);
}

// ---------------------------------------------------------------------------
// 8. Splitting replaces nothing
// ---------------------------------------------------------------------------

#[test]
fn splitting_a_long_block_replaces_no_character() {
    // `_split_oversized` rebuilds the pieces with `sep = " "`
    // (`app/index/chunking.py:210,212`), so a block whose sentences are
    // separated by newlines comes back out with spaces — and the substring
    // property test 1 rests on is gone.
    let blocks = vec![block(prose(3000, 2, "\n"), Some(1), Some(40))];
    assert!(
        blocks[0].text.contains('\n'),
        "the fixture must contain newlines"
    );
    let with_ids = ids(&blocks);
    let chunks = chunk_blocks(&with_ids, 0, &PageContext::Lines);
    assert!(chunks.len() > 1, "3000 characters must split");

    let source: Vec<char> = blocks[0].text.chars().collect();
    let mut rebuilt: Vec<Option<char>> = vec![None; source.len()];
    for chunk in &chunks {
        let text: Vec<char> = chunk.text.chars().collect();
        for seg in &chunk.locator.spans {
            assert_eq!(seg.block_id, 100);
            for (k, ch) in text[seg.start as usize..seg.end as usize]
                .iter()
                .enumerate()
            {
                let at = seg.block_start as usize + k;
                if let Some(seen) = rebuilt[at] {
                    assert_eq!(seen, *ch, "two chunks disagree about character {at}");
                }
                rebuilt[at] = Some(*ch);
            }
        }
    }
    let missing = rebuilt.iter().position(Option::is_none);
    assert_eq!(
        missing, None,
        "character {missing:?} of the block is in no chunk"
    );
    let joined: String = rebuilt.into_iter().map(Option::unwrap).collect();
    assert_eq!(joined, blocks[0].text, "the pieces are not the block");
}

// ---------------------------------------------------------------------------
// 9. Characters, not bytes
// ---------------------------------------------------------------------------

#[test]
fn offsets_are_characters_not_bytes() {
    // Ukrainian throughout: every character is two bytes, so a byte-offset
    // implementation passes tests 1–8 on ASCII and quotes the wrong slice here.
    let blocks = vec![
        block(prose(1400, 0, " "), Some(1), Some(18)),
        block(prose(1400, 4, " "), Some(19), Some(36)),
    ];
    let with_ids = ids(&blocks);
    let chunks = chunk_blocks(&with_ids, 0, &PageContext::Lines);
    assert!(chunks.len() > 1);

    for chunk in &chunks {
        assert!(
            chunk.text.len() > n_chars(&chunk.text),
            "the fixture must be non-ASCII for this test to mean anything"
        );
        let last = chunk.locator.spans.last().unwrap();
        assert_eq!(
            last.end as usize,
            n_chars(&chunk.text),
            "the last offset counts characters, not the {} bytes",
            chunk.text.len()
        );
        assert_eq!(rebuild(chunk, &with_ids), chunk.text);
        for seg in &chunk.locator.spans {
            let b = find_block(&with_ids, seg.block_id);
            let from = seg.block_start as usize;
            let len = (seg.end - seg.start) as usize;
            assert!(
                from + len <= n_chars(&b.text),
                "block_start {from} + {len} runs past the block's {} characters",
                n_chars(&b.text)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 10. Empty blocks
// ---------------------------------------------------------------------------

#[test]
fn an_empty_block_contributes_neither_text_nor_a_segment() {
    let real = [
        block(prose(300, 0, " "), Some(1), Some(4)),
        block(prose(300, 4, " "), Some(6), Some(9)),
    ];
    let padded = [
        block(prose(300, 0, " "), Some(1), Some(4)),
        block("   \n\t ".into(), Some(5), Some(5)),
        block(prose(300, 4, " "), Some(6), Some(9)),
    ];
    // the blank block keeps its own rowid; the two runs must still agree
    let without: Vec<(i64, &Block)> = vec![(100, &real[0]), (102, &real[1])];
    let with: Vec<(i64, &Block)> = vec![(100, &padded[0]), (101, &padded[1]), (102, &padded[2])];

    let a = chunk_blocks(&without, 0, &PageContext::Lines);
    let b = chunk_blocks(&with, 0, &PageContext::Lines);
    assert_eq!(a, b, "a blank block changed the output");
    for chunk in &b {
        assert!(
            chunk.locator.spans.iter().all(|s| s.block_id != 101),
            "the blank block got a segment"
        );
    }
}

// ---------------------------------------------------------------------------
// 11. Coordinates
// ---------------------------------------------------------------------------

#[test]
fn a_line_coordinate_covers_every_block_the_chunk_touches() {
    let blocks = vec![
        block(prose(300, 0, " "), Some(1), Some(10)),
        block(prose(50, 3, " "), Some(11), Some(14)),
        block(prose(300, 5, " "), Some(15), Some(40)),
    ];
    let with_ids = ids(&blocks);
    let chunks = chunk_blocks(&with_ids, 0, &PageContext::Lines);
    assert_eq!(chunks.len(), 1, "654 characters is one chunk");
    assert_eq!(
        chunks[0].locator.coordinate,
        Coordinate::Line { start: 1, end: 40 },
        "the range must span every block the chunk touches"
    );
}

#[test]
fn a_block_without_line_numbers_leaves_the_chunk_uncoordinated() {
    let blocks = vec![
        block(prose(300, 0, " "), Some(1), Some(10)),
        block(prose(50, 3, " "), None, None),
        block(prose(300, 5, " "), Some(15), Some(40)),
    ];
    let with_ids = ids(&blocks);
    let chunks = chunk_blocks(&with_ids, 0, &PageContext::Lines);
    assert_eq!(
        chunks[0].locator.coordinate,
        Coordinate::None,
        "render nothing rather than invent a line number"
    );
}

#[test]
fn a_fixed_coordinate_is_copied_onto_every_chunk() {
    let blocks = vec![block(prose(4000, 0, " "), None, None)];
    let with_ids = ids(&blocks);
    let page = PageContext::Fixed(Coordinate::Page { number: 7 });
    let chunks = chunk_blocks(&with_ids, 0, &page);
    assert!(chunks.len() > 2);
    for chunk in &chunks {
        assert_eq!(chunk.locator.coordinate, Coordinate::Page { number: 7 });
    }
}

// ---------------------------------------------------------------------------
// 12. The hash
// ---------------------------------------------------------------------------

/// A block bigger than one chunk overlaps with itself.
///
/// Written for a rule that no longer exists — a code block chunked strictly on
/// its own — and kept because the crate had no test that the overlap *happens*
/// at all: `every_chunk_carries_source_the_previous_one_did_not` asserts the
/// opposite direction, that a chunk is not made only of carry. The packing is
/// blind to `block_type`, so the fence here is just a block.
///
/// 2 000 characters, deliberately: an oversized block is cut into pieces of
/// nearly `MAX_CHARS` each, and a carry never fits beside a piece that size —
/// the second `> MAX_CHARS` check in `pack` drops it. So a much longer fixture
/// would pass this test vacuously. This one is a full piece and a small
/// remainder, where the carry does fit and its absence would show.
#[test]
fn a_block_bigger_than_one_chunk_overlaps_with_itself() {
    let blocks = vec![code(source(2000), Some(1), Some(45))];
    let with_ids = ids(&blocks);
    let chunks = chunk_blocks(&with_ids, 0, &PageContext::Lines);
    assert_eq!(chunks.len(), 2, "one full piece and a remainder");

    let previous = chunks[0].locator.spans.last().expect("a chunk has spans");
    let next = chunks[1].locator.spans.first().expect("a chunk has spans");
    let previous_end = previous.block_start + (previous.end - previous.start);
    assert!(
        next.block_start < previous_end,
        "chunk 1 starts at {} in the block, after chunk 0 ended at {previous_end} — \
         there is no overlap between them",
        next.block_start,
    );
}

/// A fence and the prose around it belong in one chunk, which is the whole of
/// what the withdrawn standalone rule forbade.
///
/// Asserted rather than merely no longer forbidden, because `chunk_kind`'s
/// majority typing only makes sense over chunks that can hold both — if the
/// chunker ever separated them again, the majority rule would become an
/// elaborate way of saying "all or none" and nothing would say so.
#[test]
fn a_fence_and_the_prose_around_it_can_share_a_chunk() {
    let blocks = vec![
        block(prose(300, 0, " "), Some(1), Some(4)),
        code("mnema index --root ./документи".into(), Some(6), Some(6)),
        block(prose(300, 2, " "), Some(8), Some(11)),
    ];
    let with_ids = ids(&blocks);
    let chunks = chunk_blocks(&with_ids, 0, &PageContext::Lines);
    assert_eq!(chunks.len(), 1, "630 characters is one chunk: {chunks:?}");
    let named: Vec<i64> = chunks[0].locator.spans.iter().map(|s| s.block_id).collect();
    assert_eq!(named, vec![with_ids[0].0, with_ids[1].0, with_ids[2].0]);
}

#[test]
fn the_chunker_hash_names_the_constants_it_was_built_from() {
    // `embedding_space.chunker_hash` is NOT NULL inside a UNIQUE key
    // (`crates/mnema-index/src/schema.sql:271,280`). Its whole job is to change
    // when the chunking changes, so this string is pinned: touching a constant
    // reddens this test and forces a deliberate decision about the vectors
    // already in the database.
    assert_eq!(
        chunker_hash(),
        // Still rev=1, and that is a decision rather than an omission: task 11
        // bumped it to 2 for a standalone-code rule and withdrew the rule
        // after measuring what it did to ordinary documentation. The packing
        // that came back is byte-identical to the packing that left, so the
        // hash must say so — see `REV`.
        "chars/target=900/max=1850/overlap=0.15/min=200/join=%0A%0A/rev=1"
    );
    assert_eq!(TARGET_CHARS, 900);
    assert_eq!(MAX_CHARS, 1850);
    assert_eq!(MIN_CHARS, 200);
    assert_eq!(JOIN, "\n\n");
    // Belt and braces, and the two catch different mistakes: the assertion
    // above catches a ratio the format string rounds away, this one catches a
    // ratio that changes the hash but that nobody meant to change. 0.154 used
    // to pass both the hash and every other test here while producing a
    // measurably different chunking over 3 000 pages.
    assert_eq!(OVERLAP_RATIO, 0.15);
    assert!(
        chunker_hash().contains("overlap=0.15/"),
        "the hash must carry the ratio it chunked with: {}",
        chunker_hash()
    );
}

// ---------------------------------------------------------------------------
// Degenerate input
// ---------------------------------------------------------------------------

#[test]
fn nothing_usable_yields_no_chunks() {
    let blocks = vec![block("  \n ".into(), Some(1), Some(2))];
    assert!(chunk_blocks(&ids(&blocks), 0, &PageContext::Lines).is_empty());
    assert!(chunk_blocks(&[], 0, &PageContext::Lines).is_empty());
}
