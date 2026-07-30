//! The four tests task-6-brief.md specifies for the txt reader and the NDJSON
//! wire, verbatim from the brief.

use mnema_core::{Block, BlockType};
use mnema_extract::extract_text;
use mnema_extract::wire::{self, Frame};

fn sample_block() -> Block {
    Block {
        block_type: BlockType::Paragraph,
        reading_order: 0,
        language: None,
        text: "приклад".to_string(),
        line_start: Some(1),
        line_end: Some(1),
    }
}

#[test]
fn a_paragraph_keeps_its_own_line_numbers() {
    let src = "перший рядок\nдругий рядок\n\nновий абзац\n";
    let blocks = extract_text(src.as_bytes());
    assert_eq!(blocks.len(), 2);
    assert_eq!(
        (blocks[0].line_start, blocks[0].line_end),
        (Some(1), Some(2))
    );
    assert_eq!(
        (blocks[1].line_start, blocks[1].line_end),
        (Some(4), Some(4))
    );
}

#[test]
fn indentation_and_tabs_survive_verbatim() {
    let src = "line one\n   line two\ttabbed\u{a0}nbsp\n";
    let blocks = extract_text(src.as_bytes());
    assert!(
        blocks[0].text.contains('\t'),
        "the server collapses this; we must not"
    );
    assert!(blocks[0].text.contains('\u{a0}'));
}

#[test]
fn text_is_nfc_before_it_leaves_the_reader() {
    let decomposed = "\u{0438}\u{0306}од";
    let blocks = extract_text(decomposed.as_bytes());
    assert_eq!(
        blocks[0].text, "йод",
        "NFC happens before offsets and hashes (D32)"
    );
}

#[test]
fn each_frame_is_one_line_of_json() {
    let line = wire::to_line(&Frame::Block(sample_block())).unwrap();
    assert_eq!(
        line.matches('\n').count(),
        1,
        "NDJSON: exactly one trailing newline"
    );
}
