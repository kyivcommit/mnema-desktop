use serde::{Deserialize, Serialize};

/// One piece of a chunk's text, drawn from a single source block. A chunk
/// spanning several blocks carries one `Segment` per block, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub block_id: i64,
    /// Offsets into the chunk's own text.
    pub start: u32,
    pub end: u32,
    /// Where this piece begins inside its block's text. The server has no
    /// equivalent and must locate the quote by substring search, giving up
    /// silently on zero or multiple hits (app/index/highlight.py:50-57).
    pub block_start: u32,
}

/// The natural coordinate of a format, stored structurally rather than as a
/// pre-rendered string, so it can be sorted and localised. G7.0 §5.2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Coordinate {
    Page {
        number: u32,
    },
    Line {
        start: u32,
        end: u32,
    },
    SheetRows {
        sheet: String,
        start: u32,
        end: u32,
    },
    Section {
        title: String,
    },
    /// The document has no verifiable coordinate. Render nothing rather than
    /// inventing a page number.
    None,
}

impl Coordinate {
    pub fn render(&self) -> String {
        match self {
            Coordinate::Page { number } => format!("с. {number}"),
            Coordinate::Line { start, end } if start == end => format!("рядок {start}"),
            Coordinate::Line { start, end } => format!("рядки {start}–{end}"),
            Coordinate::SheetRows { sheet, start, end } if start == end => {
                format!("аркуш {sheet}, рядок {start}")
            }
            Coordinate::SheetRows { sheet, start, end } => {
                format!("аркуш {sheet}, рядки {start}–{end}")
            }
            Coordinate::Section { title } => title.clone(),
            Coordinate::None => String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locator {
    pub spans: Vec<Segment>,
    pub coordinate: Coordinate,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_coordinate_renders_as_a_range() {
        let loc = Locator {
            spans: vec![Segment {
                block_id: 1,
                start: 0,
                end: 42,
                block_start: 0,
            }],
            coordinate: Coordinate::Line {
                start: 412,
                end: 427,
            },
        };
        assert_eq!(loc.coordinate.render(), "рядки 412–427");
    }

    #[test]
    fn a_single_line_renders_without_a_range() {
        let c = Coordinate::Line { start: 7, end: 7 };
        assert_eq!(c.render(), "рядок 7");
    }

    #[test]
    fn a_single_sheet_row_renders_without_a_range() {
        let c = Coordinate::SheetRows {
            sheet: "Кошторис".into(),
            start: 14,
            end: 14,
        };
        assert_eq!(c.render(), "аркуш Кошторис, рядок 14");
    }

    #[test]
    fn an_invented_page_number_is_not_a_coordinate() {
        // Text formats have no pages; the server invents them. A chunk from a .txt
        // carries Section or None, never Page — see G7.0 §5.2.
        let c = Coordinate::Section {
            title: "Розділ 3. Умови постачання".into(),
        };
        assert_eq!(c.render(), "Розділ 3. Умови постачання");
    }

    #[test]
    fn a_locator_round_trips_several_segments() {
        let loc = Locator {
            spans: vec![
                Segment {
                    block_id: 41,
                    start: 0,
                    end: 152,
                    block_start: 88,
                },
                Segment {
                    block_id: 42,
                    start: 154,
                    end: 1053,
                    block_start: 0,
                },
            ],
            coordinate: Coordinate::Line { start: 12, end: 40 },
        };
        let json = serde_json::to_string(&loc.spans).unwrap();
        let back: Vec<Segment> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, loc.spans);
        assert_eq!(
            back[0].block_start, 88,
            "block_start is what a highlight needs"
        );
    }

    #[test]
    fn a_sheet_coordinate_names_a_row_range_not_a_cell() {
        let c = Coordinate::SheetRows {
            sheet: "Кошторис".into(),
            start: 14,
            end: 19,
        };
        let rendered = c.render();
        assert!(rendered.contains("14"), "got {rendered}");
        assert!(
            rendered.contains("19"),
            "a range must show both ends: {rendered}"
        );
    }
}
