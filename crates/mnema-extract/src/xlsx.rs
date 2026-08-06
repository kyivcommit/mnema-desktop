//! The XLSX reader: a sheet is a page, a row is a block, and the row number is
//! the coordinate.
//!
//! **This is the only reader whose citation coordinate is a range the chunker
//! computes.** `pages_of` gives it `PageContext::Rows { sheet }`
//! (`crates/mnema-ingest/src/lib.rs:1403`), and the chunker turns the rows of
//! the blocks a chunk actually covers into `Coordinate::SheetRows`. The failure
//! that arrangement exists to prevent is a fixed coordinate repeating the
//! sheet's whole extent onto every chunk of it — a chunk of rows 10–20 citing
//! "аркуш Дані, рядки 1–500". So every block this reader emits owes its rows,
//! and one block without them makes the whole range `Coordinate::None`.
//!
//! **A row is one block and its cells are joined by tabs — a decision, not a
//! default, and `html.rs` answered the same question the other way.** Task 10
//! made an HTML table cell its own block, because joining cells needs a
//! separator no cell contains and inventing characters is what D32/D38 forbid.
//! The trade is different here, and it is the coordinate that makes it
//! different: an HTML chunk is cited by its section, so joining a `<tr>` buys no
//! coordinate and pays the separator for nothing, while an xlsx chunk is cited
//! by a **row range** — a block per cell would make `line_start`/`line_end`
//! describe a unit the citation cannot name, and a row of six cells would reach
//! the index as six blocks joined by blank lines, which is not what a
//! spreadsheet row is. The server joins with tabs for the same reason
//! (`app/textdoc/office.py:254`).
//!
//! What the tab costs, stated where it will be read:
//!
//! - **a cell may contain one.** Measured, not assumed: calamine hands back
//!   `"з\tтабом"` intact for a shared string holding a tab, so the separator is
//!   not unambiguous and nothing downstream can tell a cell boundary from a tab
//!   inside a cell. `tests/xlsx.rs` states this in the verbatim test rather than
//!   hiding it;
//! - **`line_start`/`line_end` number *sheet rows*, not lines of text.** A block
//!   here holds no newline at all, so the two coincide by construction — but the
//!   field means something different for this reader than for `text.rs`, and
//!   `PageContext::Rows` is exactly what renames it at the citation.
//!
//! A leading gap is kept: a row whose first value sits in column C begins with
//! two tabs, because that is where the value is. The server's `.strip()` is not
//! ported — it would move a value out of its column and trim a cell's own
//! leading space, and block text is verbatim after NFC (D32, D38).
//!
//! **The header row is an ordinary block of its sheet.** The server copies it
//! onto every continuation page (`office.py:272-279`); that is not ported,
//! because a copied header is text at a row it is not on, and this reader's
//! whole coordinate is which rows a chunk covers.
//!
//! # What the parse can lose, each answered by a run
//!
//! In xlsx the text is not in the cell: a cell of type `s` carries an **index**
//! into `xl/sharedStrings.xml`. Every line below is a measurement against
//! calamine 0.36, not a reading of the specification.
//!
//! - **`worksheets()` — the obvious API — silently drops a sheet it cannot
//!   read.** It is `filter_map(|n| self.worksheet_range(&n).ok()?)`
//!   (`calamine-0.36.0/src/xlsx/mod.rs:2628`), so a sheet whose XML stops in the
//!   middle of an element comes back as *no sheet at all*: measured, a workbook
//!   of one truncated sheet answered `[]`, with no error anywhere. This reader
//!   asks for each sheet by name and handles the error.
//! - **`worksheet_range` turns a 1 443-byte file into a dead process.**
//!   `Range::from_sparse` allocates `rows × columns` cells densely
//!   (`lib.rs:958-961`), so an archive holding two cells — `A1` and
//!   `XFD1048576` — asks for 16 384 × 1 048 576 of them. Measured: 6.99 GB
//!   resident, 200 GB peak footprint, killed after 50 s. The same file through
//!   [`Xlsx::worksheet_cells_reader`] returns two cells instantly, which is why
//!   this reader streams cells and never builds a `Range`.
//! - **A shared-string index past the end of the table stops the sheet.**
//!   `next_cell` answers `Unexpected("Cell string index not found in shared
//!   strings table")` at that cell, and so does every cell of type `s` in a
//!   workbook with no `xl/sharedStrings.xml` at all. Neither refuses the file:
//!   the sheet is skipped by number and the rest of the workbook is read.
//! - **A sheet the workbook declares and the archive does not hold** answers
//!   `WorksheetNotFound`, and a **chartsheet** answers `NotAWorksheet`. Both are
//!   skipped by number — refusing the file over one of them would take a whole
//!   workbook out of the index over a chart, which is the argument `epub.rs`
//!   makes for a chapter the archive does not hold.
//! - **Two sheets with the same name: the second is unreachable.**
//!   `worksheet_cells_reader` resolves a name by `find` (`xlsx/mod.rs:2521`), so
//!   asking twice reads the *first* sheet twice — measured, both reads returned
//!   `"перший"`. Read by name once each: a repeated name is skipped by number,
//!   which loses that sheet and is the only outcome that does not also store
//!   another sheet's text twice under it.
//! - **A rich-text shared string is joined for us**: `<r>` runs came back as
//!   `"Разом до сплати"`, and an inline `<is>` of two runs as one value. Nothing
//!   here has to reassemble them.
//! - **A formula's cached value is read and its source is not.** `<f>` with a
//!   `<v>` beside it gives `Float(42)`; `<f>` alone gives `Data::Empty`, so a
//!   workbook written by a tool that caches nothing loses its computed cells.
//!   The server falls back to the formula *text* (`office.py:250`); that is
//!   deliberately not ported, for the reason `docx.rs` gives `<w:instrText>` —
//!   it is code, it is never painted, and a citation quoting `=SUM(B1:C1)` for a
//!   cell the person sees as `42` is the sharper failure. A row of nothing but
//!   uncached formulas is therefore an empty row and is dropped.
//! - **One cell reference named twice keeps the last value, silently.** Measured
//!   after a review asked which of the two wins, because the answer was neither
//!   written down nor tested: `<c r="A1">…0…</c><c r="A1">…1…</c>` stores the
//!   second. Either would be an arbitrary choice — what was missing is that it
//!   is *a* choice, so `a_malformed_row_is_read_the_way_a_spreadsheet_would`
//!   pins it. Excel does not write such a file; a crafted one does.
//!
//! **No number format is applied, and that is the largest thing this reader
//! knowingly gets wrong.** A cell in xlsx is a raw value plus a `numFmt` in
//! `xl/styles.xml` that says how it is painted; calamine hands back the value,
//! and `Display` prints it. So what reaches the index is never what the sheet
//! shows for any formatted cell:
//!
//! | cell | the sheet shows | the index gets |
//! |---|---|---|
//! | `0.15`, numFmt 9 | `15%` | `0.15` |
//! | `1500.5`, numFmt 44 | `1 500,50 ₴` | `1500.5` |
//! | `46000`, numFmt 14 | `06.08.2026` | `46000` |
//! | `1e15` | `1E+15` | `1000000000000000` |
//! | `t="b"`, `1` | `TRUE` | `true` |
//!
//! A search for `15%` fails for exactly the reason a search for `06.08.2026`
//! fails, and **this was written as a fact about dates until a review measured
//! the rest of the column.** The date is the sharpest instance because a whole
//! column of them is unreadable rather than merely unformatted, not because it
//! is a separate defect.
//!
//! It is not a coding error either: the server reads `openpyxl`'s `cell.value`
//! and lands in the same place. Closing it needs calamine's `dates` feature —
//! which is `chrono` — *and* a rule for which of the several hundred format
//! codes render how, in whose locale; a dependency and a display policy, neither
//! of them a line in this file.
//! `no_number_format_is_applied_so_a_cell_is_indexed_as_its_raw_value` pins the
//! class rather than one member of it.
//!
//! **A hidden sheet is read.** `sheets_metadata()` reports `Hidden` and
//! `VeryHidden` and calamine reads all three alike; openpyxl's `wb.worksheets`
//! does too, so the server indexes them. A hidden sheet is present content the
//! *view* hides, not content that was removed — which is what separates it from
//! `<w:delText>`. The cost is that a citation can name a sheet the person does
//! not see on opening the file, and `Sheet::visible` is where a later decision
//! would hook in.
//!
//! **The cap, and why it is not `zip_part::read_member`.** calamine has none of
//! its own and inflates eagerly: measured, a **409 KB** archive made
//! `Xlsx::new` build a 400 MiB shared string before a single cell was read,
//! peaking at 1.68 GB. So every member is inflated against
//! [`zip_part::MEMBER_MAX_BYTES`] and one [`WORKBOOK_MAX_BYTES`] budget *before*
//! calamine opens the file. It is measured rather than read, because this reader
//! does not consume the member it measures — calamine does — and the shared
//! helper's `Vec<u8>` would be sixteen megabytes allocated to be thrown away.
//! There is deliberately **no cap on the number of members**: an entry-count
//! walk was the obvious second guard and the run says it is not needed — 100 000
//! members cost 20 ms to open and 69 ms to walk, so the largest archive the
//! request ceiling admits is a fraction of a second.

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read};

use calamine::{Data, Reader, Xlsx};
use mnema_core::{Block, BlockType, nfc};

use crate::markdown::bound_section_title;
use crate::zip_part::MEMBER_MAX_BYTES;

/// What separates two cells of one row.
///
/// A constant rather than a literal because it is the whole of this reader's
/// answer to the question `html.rs` answered differently — see the module doc
/// for the trade and what it costs.
const CELL_SEPARATOR: char = '\t';

/// What everything inside one workbook may inflate to, in total.
///
/// The same shape and the same number as [`crate::BOOK_MAX_BYTES`], and for the
/// same reason: [`MEMBER_MAX_BYTES`] bounds one member, and a package of N
/// members each just under it is the same attack with more entries. A workbook
/// opens at least four (`_rels/.rels`, `xl/workbook.xml`, its relationships and
/// one worksheet) and one per sheet after that.
pub const WORKBOOK_MAX_BYTES: usize = 256 << 20;

/// One sheet of a workbook: one page of the document it becomes.
///
/// Deliberately not [`crate::DocxSection`] or [`crate::HtmlPage`], although the
/// three carry the same three things — a reader's page is not the pool's, and
/// sharing one type would put the pool's into the crate that links Pdfium (D40).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XlsxSheet {
    /// The sheet's own 1-based position in the workbook, **not** its position
    /// among the sheets that came back. Sheets 1 and 3 arriving with 2 skipped
    /// is the intended shape.
    pub page_no: u32,
    /// The sheet's name, flattened onto one line and bounded by
    /// [`bound_section_title`] — and this is the **only** place it is bounded.
    /// `pages_of` copies this exact string into `Coordinate::SheetRows { sheet }`
    /// (`crates/mnema-ingest/src/lib.rs:1403`), so a second bounding anywhere
    /// would let the citation and the coordinate show two different names,
    /// silently, with every test green.
    pub section_title: Option<String>,
    pub blocks: Vec<Block>,
}

/// What an XLSX read produced: the sheets that had text, and the numbers of the
/// sheets that did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XlsxWorkbook {
    pub sheets: Vec<XlsxSheet>,
    /// Ascending, 1-based, and disjoint from `sheets` — every sheet the workbook
    /// declares appears in exactly one of the two. The disjointness is not
    /// politeness: `mnema-pool` stops the entire job when one number is in both
    /// lists (`crates/mnema-pool/src/lib.rs:1338`), because a page that was read
    /// and reported skipped is a journal row telling someone a sheet is missing
    /// while the index holds it.
    ///
    /// **A spreadsheet really can skip one**, which is what separates this
    /// reader from `docx.rs` and `html.rs` — see the module doc for the five
    /// measured ways a sheet fails while the rest of the workbook reads.
    pub skipped: Vec<u32>,
}

/// Why a workbook could not be read at all. Three variants, three refusal
/// rules, and every one of them reachable — a variant nothing produces is a
/// branch in the worker that no test can ever redden.
#[derive(Debug, Clone, thiserror::Error)]
pub enum XlsxError {
    /// The archive parses as a zip and what makes it a workbook does not: no
    /// package relationships, no `xl/workbook.xml` where they point, or XML that
    /// does not parse.
    ///
    /// **Not the verdict for a sheet.** A sheet that is missing, damaged or not
    /// a worksheet is skipped by number; this names the structure that says
    /// which sheets there are, and without it there is nothing to skip from.
    #[error("this workbook is damaged: {0}")]
    Malformed(String),

    /// A member inflated past [`MEMBER_MAX_BYTES`], or the package as a whole
    /// past [`WORKBOOK_MAX_BYTES`].
    ///
    /// Decided on what came out of the stream, never on the size the archive
    /// declares — see `zip_part`'s module doc for the forged-size case that
    /// makes the distinction load-bearing.
    ///
    /// **The message names both, and it used to name only the first.** Since
    /// this task the string is what a person is shown (`reason: e.to_string()`
    /// in the worker), and a workbook of a thousand small members refused by the
    /// total would have read "past the cap on one member" — a true-sounding
    /// sentence about the wrong rule. `epub.rs:153` still has the narrow
    /// wording for the same two-cap arrangement; it is another reader's
    /// user-visible string and is left alone rather than changed from here.
    #[error("this workbook decompresses past the cap on what one file may inflate to")]
    TooLarge,

    /// The workbook declares sheets and not one of them produced a row with a
    /// word in it: a workbook of charts, a workbook whose every value is an
    /// uncached formula, a new and empty workbook.
    ///
    /// The same answer `pdf.rs` gives a document whose every page is a scan and
    /// `epub.rs` a book of plates. A file with no text in it is a fact worth
    /// telling someone, and storing it as a document with zero blocks tells
    /// them nothing.
    #[error("no sheet of this workbook carries any text")]
    NoText,
}

/// Reads an XLSX into sheets, one page each.
///
/// Takes bytes rather than a path for the reason every reader in this crate
/// does: `handle_request` reads the file once and hashes the same `Vec<u8>` it
/// hands here, so nothing can change between the digest and the reading.
pub fn extract_xlsx(bytes: &[u8]) -> Result<XlsxWorkbook, XlsxError> {
    extract(bytes, WORKBOOK_MAX_BYTES)
}

/// The read, against a stated budget.
///
/// Split out for the reason `epub::extract` is: [`WORKBOOK_MAX_BYTES`] is a
/// quarter of a gigabyte, and a test that reached it would have to inflate a
/// quarter of a gigabyte. The rule these tests exist to check is not "256 MiB";
/// it is that **every** member draws against one total and that the cap stands
/// before calamine opens anything, which a budget of a few kilobytes states just
/// as well.
fn extract(bytes: &[u8], budget: usize) -> Result<XlsxWorkbook, XlsxError> {
    // Before `Xlsx::new`, and that order is the whole guard: calamine reads
    // `xl/sharedStrings.xml` eagerly inside the constructor, so a cap applied
    // afterwards would be measuring memory it has already allocated.
    measure_package(bytes, budget)?;

    let mut workbook: Xlsx<_> = Xlsx::new(Cursor::new(bytes))
        .map_err(|e| XlsxError::Malformed(format!("this workbook does not open: {e}")))?;

    let mut sheets = Vec::new();
    let mut skipped = Vec::new();
    // Set by a sheet whose cells would not parse, as opposed to one that is
    // simply absent or simply empty. It only matters when nothing at all was
    // read: a workbook that produced nothing because its XML is cut should say
    // so, rather than report the same "no text in this workbook" as a workbook
    // of charts.
    let mut saw_damage = false;
    // **Names already asked for, because a name is how a sheet is addressed and
    // two sheets may share one.** Measured: `worksheet_cells_reader` resolves a
    // name with `find`, so a second request for a repeated name reads the first
    // sheet again. Without this set the workbook would hold one sheet's rows
    // twice under two page numbers, and the other sheet not at all.
    let mut asked: HashSet<String> = HashSet::new();

    for (index, name) in workbook.sheet_names().into_iter().enumerate() {
        // 1-based, and the position in the workbook rather than in `sheets`:
        // this is the number that goes into `skipped_pages` when nothing comes
        // back, and it has to mean the same thing in both lists.
        let page_no = index as u32 + 1;

        if !asked.insert(name.clone()) {
            skipped.push(page_no);
            continue;
        }

        let rows = match read_sheet(&mut workbook, &name) {
            SheetRead::Rows(rows) => rows,
            SheetRead::Damaged => {
                saw_damage = true;
                skipped.push(page_no);
                continue;
            }
            SheetRead::Absent => {
                skipped.push(page_no);
                continue;
            }
        };

        let blocks = blocks_of(rows);
        if blocks.is_empty() {
            // A sheet that is there and says nothing: a new sheet, a sheet of
            // charts, a sheet of uncached formulas. Named as skipped rather than
            // stored as an empty page, so that the journal can say which sheet
            // of the workbook this reader got nothing out of.
            skipped.push(page_no);
            continue;
        }

        sheets.push(XlsxSheet {
            page_no,
            section_title: sheet_title(&name),
            blocks,
        });
    }

    if sheets.is_empty() {
        return Err(if saw_damage {
            XlsxError::Malformed("no sheet of this workbook could be read".to_string())
        } else {
            XlsxError::NoText
        });
    }

    Ok(XlsxWorkbook { sheets, skipped })
}

/// Every member of the package, inflated against one member cap and one budget
/// and kept nowhere.
///
/// **Only `TooLarge` comes out of here**, and that narrowness is deliberate: the
/// job of this pass is the cap, not damage detection. A member that will not
/// decompress is left for calamine to meet — `docProps/app.xml` being corrupt in
/// a workbook whose sheets are fine is not a reason to refuse the workbook, and
/// this function cannot tell which members calamine will actually open.
///
/// **A member that breaks is not a member that was free**, and the first version
/// of this function had it the other way round. `io::copy` reports what it moved
/// only when it *succeeds*, and a broken deflate stream has already inflated
/// everything before the break — measured: `Err("Invalid checksum")` after
/// 4 194 304 bytes reached the sink. Skipping the charge on that arm made a
/// 417 KB archive of 100 members inflate **400 MiB** and answer `Ok`, a third
/// again over a budget that never fired. It never leaked — `take` and the
/// counter bound each member — but the pool's 120 s deadline
/// (`crates/mnema-pool/src/lib.rs:552`) turns that into a worker slot eaten
/// whole and a journal row saying `Timeout`, with nothing naming the cause.
/// So the bytes are counted **on the way past** and charged whichever way the
/// copy ends.
///
/// Counted rather than charged at `cap`, which was the other candidate: a member
/// that broke after a hundred bytes would then be billed sixteen megabytes, and
/// a mildly damaged workbook would be refused for damage under a rule about
/// size. The budget falls from work done, and this is what work done is.
///
/// **Every member, not only the ones that look like XML.** A worksheet's path
/// comes out of `xl/_rels/workbook.xml.rels` and can be anything, so a rule
/// keyed on the extension would leave a bomb parked at `xl/worksheets/sheet1.dat`
/// unmeasured and calamine would read it anyway. The cost of the wider rule is
/// stated rather than hidden: a workbook holding one embedded image over
/// [`MEMBER_MAX_BYTES`] is refused, though nothing here would ever have read it.
fn measure_package(bytes: &[u8], budget: usize) -> Result<(), XlsxError> {
    let mut budget = budget;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| XlsxError::Malformed("this file is not a zip archive".to_string()))?;

    for index in 0..archive.len() {
        let Ok(mut member) = archive.by_index(index) else {
            continue;
        };
        let cap = MEMBER_MAX_BYTES.min(budget) as u64;
        // `cap + 1` so that "exactly at the cap" and "over it" are told apart by
        // what came out of the stream, never by the size the central directory
        // declares — the forged-size case `zip_part`'s module doc measures.
        //
        // The result is deliberately discarded: [`Inflated`] has the count
        // either way, and the whole point is that a stream which breaks
        // half-way still inflated everything up to the break. Nothing is kept —
        // this pass counts and drops, so a `Vec<u8>` here would be sixteen
        // megabytes allocated in order to be thrown away.
        let mut inflated = Inflated(0);
        let _ = std::io::copy(&mut Read::take(&mut member, cap + 1), &mut inflated);
        let inflated = inflated.0;
        if inflated > cap {
            return Err(XlsxError::TooLarge);
        }
        // `saturating_sub` for what the failure would look like rather than for
        // whether it can happen: `inflated <= cap <= budget` holds above, but a
        // plain `-=` would panic in the worker under debug and wrap to near
        // `usize::MAX` under release — after which the budget silently stops
        // bounding anything, which is the one outcome this cap exists to
        // prevent, arriving through the cap itself.
        budget = budget.saturating_sub(inflated as usize);
    }

    Ok(())
}

/// A sink that counts what passes through it.
///
/// `io::sink()` would do the discarding and `io::copy`'s return value would do
/// the counting — but only on the arm where the copy succeeds, which is the one
/// arm where the count does not matter. This carries it out of the failing arm
/// too; see [`measure_package`] for what that was worth.
struct Inflated(u64);

impl std::io::Write for Inflated {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // `saturating_add` for the same reason the budget's subtraction uses
        // it: `take` bounds this well below `u64::MAX`, and a counter that
        // wrapped would report a bomb as a small member.
        self.0 = self.0.saturating_add(buf.len() as u64);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// What asking a workbook for one sheet answered.
enum SheetRead {
    /// Row index (0-based, as calamine reports it) to column index to the text
    /// of that cell. A map rather than a stream because a sheet is free to list
    /// its cells in any order and to skip rows entirely, and because a cell
    /// reference repeated twice must not become two cells.
    Rows(BTreeMap<u32, BTreeMap<u32, String>>),
    /// The workbook names this sheet and there is nothing to read: no such
    /// member, or a chartsheet.
    Absent,
    /// The sheet exists and its cells stop making sense part way through.
    Damaged,
}

/// Streams one sheet's cells.
///
/// **The partial read is thrown away, and that is the point.** A sheet whose XML
/// stops inside an element hands back every cell before the cut and then errors
/// — measured, `next_cell` returned `"початок"` and then
/// `IllFormed(MissingEndTag("v"))`. Keeping the prefix would store a truncated
/// sheet as though it were whole, which is the shape `docx.rs`'s `depth != 0`
/// check exists to refuse, and the shape `epub.rs` refuses by skipping a chapter
/// whose stream is corrupt rather than storing what came before the damage.
fn read_sheet<RS: Read + std::io::Seek>(workbook: &mut Xlsx<RS>, name: &str) -> SheetRead {
    let Ok(mut cells) = workbook.worksheet_cells_reader(name) else {
        return SheetRead::Absent;
    };

    let mut rows: BTreeMap<u32, BTreeMap<u32, String>> = BTreeMap::new();
    loop {
        match cells.next_cell() {
            Ok(Some(cell)) => {
                let text = Data::from(cell.get_value().clone()).to_string();
                if text.is_empty() {
                    // An empty cell must not widen its row: the join below runs
                    // from column 0 to the last cell that has text, so an empty
                    // trailing cell would otherwise add a tab to the end of the
                    // block — the server's `.rstrip("\t")`, obtained by not
                    // putting it there.
                    continue;
                }
                let (row, column) = cell.get_position();
                rows.entry(row).or_default().insert(column, text);
            }
            Ok(None) => return SheetRead::Rows(rows),
            Err(_) => return SheetRead::Damaged,
        }
    }
}

/// One block per row that has a word in it, in ascending row order.
fn blocks_of(rows: BTreeMap<u32, BTreeMap<u32, String>>) -> Vec<Block> {
    let mut blocks = Vec::new();
    for (row, cells) in rows {
        let Some(last) = cells.keys().next_back().copied() else {
            continue;
        };
        let mut text = String::new();
        for column in 0..=last {
            if column > 0 {
                text.push(CELL_SEPARATOR);
            }
            if let Some(cell) = cells.get(&column) {
                text.push_str(cell);
            }
        }

        // Once per block, over the text the parse produced, and before anything
        // downstream takes an offset or a hash from it (D32, D38). Nothing else
        // touches it: no folding, no reflow, no trimming. Running it on the
        // joined row rather than per cell is the same answer — a tab separates
        // every pair, so no combining mark can compose across a cell boundary.
        let text = nfc::normalise(&text).into_owned();

        // A row of nothing but separators and spaces produces no block: it is
        // searchable, citable and empty of content, which is `markdown.rs`'s
        // argument for dropping a thematic break — and `chunk_blocks` would
        // filter it out anyway, leaving a `block` row nothing can ever cite.
        if text.trim().is_empty() {
            continue;
        }

        blocks.push(Block {
            block_type: BlockType::Table,
            // Restarts on every sheet: the schema's uniqueness is on
            // `(page_id, reading_order)`, because reading order is what
            // reconstructs a page rather than a document.
            reading_order: blocks.len() as i64,
            // Nothing here detects language; a per-block guess is the extraction
            // spec's subject, as in every other reader.
            language: None,
            text,
            // **The rows this block occupies, which is the whole reason
            // `PageContext::Rows` exists.** One row per block, so the two ends
            // are the same number — and they are the *sheet's* row, 1-based,
            // while calamine counts from 0.
            //
            // `saturating_add` for what the failure would look like rather than
            // for whether it can happen: a wrap would put row 0 into a 1-based
            // field and cite "рядок 0" of a sheet that has none.
            line_start: Some(row.saturating_add(1)),
            line_end: Some(row.saturating_add(1)),
        });
    }
    blocks
}

/// The name the sheet gives itself, in the one form that reaches both the
/// citation and the coordinate.
///
/// Flattened onto one line and bounded, exactly as a markdown heading is and for
/// the same reason: no offset is ever measured into a title, so unlike
/// `block.text` it is display metadata rather than evidence. **After NFC**,
/// because normalisation changes the character count and bounding first would
/// cut in the wrong place.
///
/// `None` for a name that is nothing but whitespace, which Excel cannot produce
/// and a crafted file can: `pages_of` then renders an empty sheet name, the same
/// answer it gives an untitled HTML page, rather than this reader inventing one.
fn sheet_title(name: &str) -> Option<String> {
    let flattened = nfc::normalise(name)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    bound_section_title(flattened)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// An xlsx of the given members, each Deflated.
    ///
    /// Built in code rather than checked in as a fixture, the choice Task 11
    /// made deliberately: a binary fixture is unreadable in a diff, nobody can
    /// reproduce it, and a mistake inside one looks exactly like a mistake in
    /// the reader.
    fn archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let deflated: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in members {
                w.start_file(*name, deflated).unwrap();
                w.write_all(body).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    const PACKAGE_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;

    /// A workbook of one sheet called `Дані`, with the given `<sheetData>` rows
    /// and shared strings, plus whatever extra members the case needs.
    fn one_sheet(rows: &str, shared: &str, extra: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut members = vec![
            ("_rels/.rels", PACKAGE_RELS.as_bytes().to_vec()),
            (
                "xl/workbook.xml",
                format!(
                    r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="{}" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
                    "Дані"
                )
                .into_bytes(),
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.to_vec(),
            ),
            (
                "xl/sharedStrings.xml",
                format!(
                    r#"<?xml version="1.0"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{shared}</sst>"#
                )
                .into_bytes(),
            ),
            (
                "xl/worksheets/sheet1.xml",
                format!(
                    r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{rows}</sheetData></worksheet>"#
                )
                .into_bytes(),
            ),
        ];
        members.extend(extra.iter().cloned());
        archive(&members)
    }

    /// Flips the stored CRC-32 of every member whose name starts with `prefix`,
    /// so that member inflates in full and then fails at the end of its stream.
    ///
    /// The CRC sits 14 bytes into a local file header and 16 into a central
    /// directory entry; both are patched, because which one the reader trusts is
    /// the library's business. Matching on the name is what keeps the workbook's
    /// own parts intact — corrupting those would test something else entirely.
    fn break_streams(bytes: &mut [u8], prefix: &str) -> usize {
        // (signature, crc offset, name-length offset, name offset)
        const HEADERS: [(&[u8; 4], usize, usize, usize); 2] =
            [(b"PK\x03\x04", 14, 26, 30), (b"PK\x01\x02", 16, 28, 46)];
        let mut broken = 0;
        for i in 0..bytes.len().saturating_sub(46) {
            for (signature, crc, len_at, name_at) in HEADERS {
                if &bytes[i..i + 4] != signature {
                    continue;
                }
                let len = u16::from_le_bytes([bytes[i + len_at], bytes[i + len_at + 1]]) as usize;
                let Some(name) = bytes.get(i + name_at..i + name_at + len) else {
                    continue;
                };
                if name.starts_with(prefix.as_bytes()) {
                    bytes[i + crc] ^= 0xff;
                    broken += 1;
                }
            }
        }
        broken
    }

    /// **`extract_xlsx` spends [`WORKBOOK_MAX_BYTES`], and nothing said so until
    /// this test.**
    ///
    /// Measured by a review: replacing the budget in [`extract_xlsx`] with
    /// `usize::MAX` left **all 244 tests of this crate green**. Every other cap
    /// test goes through the private `extract`, so the mechanism was covered and
    /// the wiring was not — and the wiring is the half a later edit touches.
    ///
    /// It is the one assertion in this file that cannot use a budget of a few
    /// kilobytes, because the number under test is a quarter of a gigabyte and
    /// the only way to exceed it is to inflate it. The member is therefore
    /// deflated **once** and copied compressed seventeen times
    /// (`raw_copy_file_rename`): building it the obvious way cost 6,6 s of the
    /// suite, and all but a fraction of that was re-deflating the same sixteen
    /// megabytes over and over. `zip` deflates a run of one byte at about
    /// 1000:1, so the archive itself stays a few hundred kilobytes.
    ///
    /// Both directions, and the second is what makes the first mean anything:
    /// the same bytes with the budget lifted are **not** refused for size, so
    /// the refusal above came from the budget rather than from anything else
    /// about a 17-member archive.
    #[test]
    fn the_public_entry_spends_the_workbook_budget() {
        // Seventeen members of exactly `MEMBER_MAX_BYTES`: none is over the
        // per-member cap on its own, and together they are 272 MiB against a
        // 256 MiB total. The case `zip_part`'s own doc names — "N members each
        // just under this cap is the same attack with more entries".
        let members = WORKBOOK_MAX_BYTES / MEMBER_MAX_BYTES + 1;
        let one = archive(&[("xl/big.xml", vec![b'a'; MEMBER_MAX_BYTES])]);

        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            for i in 0..members {
                let mut source = zip::ZipArchive::new(Cursor::new(&one)).unwrap();
                w.raw_copy_file_rename(source.by_index_raw(0).unwrap(), format!("xl/big{i}.xml"))
                    .unwrap();
            }
            w.finish().unwrap();
        }
        let bytes = buf.into_inner();

        assert!(matches!(extract_xlsx(&bytes), Err(XlsxError::TooLarge)));

        // With the budget lifted, the per-member cap alone lets all seventeen
        // through and the read fails later, on not being a workbook at all.
        assert!(matches!(
            extract(&bytes, usize::MAX),
            Err(XlsxError::Malformed(_))
        ));
    }

    /// **A member whose stream breaks has already inflated, and it is charged
    /// for it.**
    ///
    /// The defect this replaces: `io::copy` reports what it moved only on the
    /// arm where it succeeds, so skipping the charge on the failing arm made a
    /// broken member free. Measured before the fix — a 417 KB archive of 100
    /// members inflated 400 MiB and answered `Ok` against a 256 MiB budget.
    ///
    /// Four assertions, and the first is a **premise about the fixture**: it
    /// asserts that these members really do inflate and then error, so that a
    /// `zip` release which stopped validating checksums would redden this test
    /// instead of quietly turning it into a test of nothing.
    #[test]
    fn a_member_whose_stream_breaks_is_still_charged_to_the_budget() {
        let filler: Vec<(&str, Vec<u8>)> = vec![
            ("xl/broken-a.xml", vec![b'a'; 2000]),
            ("xl/broken-b.xml", vec![b'b'; 2000]),
            ("xl/broken-c.xml", vec![b'c'; 2000]),
        ];
        let mut bytes = one_sheet(
            r#"<row r="1"><c r="A1" t="s"><v>0</v></c></row>"#,
            "<si><t>рядок</t></si>",
            &filler,
        );
        assert_eq!(
            break_streams(&mut bytes, "xl/broken-"),
            6,
            "three members, two headers each"
        );

        // The premise: this member inflates in full and *then* fails.
        let mut zipped = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let mut member = zipped.by_name("xl/broken-a.xml").unwrap();
        let mut counted = Inflated(0);
        let outcome = std::io::copy(&mut member, &mut counted);
        assert!(
            outcome.is_err(),
            "the fixture is not broken, so this test measures nothing"
        );
        assert_eq!(
            counted.0, 2000,
            "a broken stream still emitted its whole member"
        );

        // Three broken members of 2 000 bytes against a budget of 3 000: the
        // first is charged, and the second must not fit. Uncharged, every one of
        // them measures 2 000 against an unspent 3 000 for ever.
        assert!(matches!(extract(&bytes, 3000), Err(XlsxError::TooLarge)));

        // And the other direction, so the assertion above is not satisfied by a
        // reader that refuses any damaged archive: the same bytes under a real
        // budget are read, broken members and all.
        let workbook = extract(&bytes, WORKBOOK_MAX_BYTES).expect("a broken filler is not damage");
        assert_eq!(workbook.sheets[0].blocks[0].text, "рядок");
    }

    /// **The cap stands before calamine opens the file, and it stands on the
    /// stream.**
    ///
    /// Two assertions doing two different jobs. The first is the bomb this pass
    /// exists for: a member that is a few hundred bytes in the archive and far
    /// more than the cap once inflated. The second is the direction that keeps
    /// the first from being satisfied by a reader that refuses everything — the
    /// *same* archive under a cap large enough for it is read.
    #[test]
    fn a_member_over_the_cap_refuses_the_workbook_and_one_under_it_does_not() {
        let padding = vec![b'a'; 8192];
        let bomb = one_sheet(
            r#"<row r="1"><c r="A1" t="s"><v>0</v></c></row>"#,
            &format!(
                "<si><t>{}</t></si>",
                String::from_utf8(padding).expect("ascii")
            ),
            &[],
        );
        assert!(
            bomb.len() < 4096,
            "the fixture must be small in the archive and large out of it, and it is {} bytes",
            bomb.len()
        );

        assert!(matches!(extract(&bomb, 1024), Err(XlsxError::TooLarge)));

        // The same bytes, read: so the refusal above is the cap and not the
        // fixture.
        let workbook = extract(&bomb, WORKBOOK_MAX_BYTES).expect("under a real budget it reads");
        assert_eq!(workbook.sheets.len(), 1);
        assert_eq!(workbook.sheets[0].blocks[0].text.chars().count(), 8192);
    }

    /// The budget is a **total**, not a per-member allowance.
    ///
    /// Every member is well under the per-member share of it and the package as
    /// a whole is not, which is the case a per-member cap alone cannot see —
    /// `zip_part`'s own doc names it ("N members each just under this cap is the
    /// same attack with more entries").
    #[test]
    fn many_small_members_exhaust_one_budget_between_them() {
        let filler: Vec<(&str, Vec<u8>)> = vec![
            ("xl/a.xml", vec![b'a'; 400]),
            ("xl/b.xml", vec![b'b'; 400]),
            ("xl/c.xml", vec![b'c'; 400]),
        ];
        let spread = one_sheet(
            r#"<row r="1"><c r="A1" t="s"><v>0</v></c></row>"#,
            "<si><t>рядок</t></si>",
            &filler,
        );

        // No single member is over 500 bytes…
        assert!(matches!(extract(&spread, 500), Err(XlsxError::TooLarge)));
        // …and the sheet reads once the total is generous enough.
        assert!(extract(&spread, WORKBOOK_MAX_BYTES).is_ok());
    }

    /// A sheet name is bounded here and nowhere else.
    ///
    /// Both directions: a name at the limit is passed through whole, and one
    /// past it is cut and says so.
    #[test]
    fn a_sheet_name_is_flattened_and_bounded_once() {
        let exact = "Д".repeat(crate::SECTION_TITLE_MAX_CHARS);
        assert_eq!(sheet_title(&exact), Some(exact.clone()));

        let long = "Д".repeat(crate::SECTION_TITLE_MAX_CHARS + 1);
        let cut = sheet_title(&long).expect("a long name is still a name");
        assert_eq!(cut.chars().count(), crate::SECTION_TITLE_MAX_CHARS);
        assert!(cut.ends_with('…'), "a cut title must say it was cut: {cut}");

        // Flattened, not trimmed away: the whitespace inside a name is display
        // metadata and nothing measures an offset into it.
        assert_eq!(
            sheet_title("  Кошторис\t 2026 "),
            Some("Кошторис 2026".into())
        );
        // And a name that is only whitespace names nothing rather than naming
        // the empty string.
        assert_eq!(sheet_title("   "), None);

        // **NFC runs, and it runs before the bound.** macOS hands over
        // decomposed text, so a sheet named `Дані` on one machine and on another
        // are two different strings unless this happens — and the order matters
        // as well as the fact: normalisation changes the character count, so
        // bounding first would cut a name in the wrong place.
        assert_eq!(
            sheet_title("Знаи\u{0306}дене"),
            Some("Знайдене".to_string())
        );
        let decomposed = "и\u{0306}".repeat(crate::SECTION_TITLE_MAX_CHARS);
        let composed = sheet_title(&decomposed).expect("a named sheet");
        assert_eq!(
            composed.chars().count(),
            crate::SECTION_TITLE_MAX_CHARS,
            "bounding ran before NFC and cut a name that composes to exactly the bound: {composed}"
        );
        assert!(!composed.ends_with('…'), "{composed}");
    }
}
