import MnemaMLX
import XCTest

/// `withoutThinking`: what reaches the client after a search is the answer, not E4B's thinking (D188).
final class ThinkingTests: XCTestCase {
    func test_a_closed_thought_block_is_removed_and_the_answer_kept() {
        let raw = "<|channel>thought\nThinking Process:\n1. Analyze <c>3</c>\n<channel|>Змінна `rprompt` <c>1</c>."
        XCTAssertEqual(withoutThinking(raw), "Змінна `rprompt` <c>1</c>.")
    }

    func test_an_answer_without_thinking_is_unchanged() {
        XCTAssertEqual(withoutThinking("Відповідь <c>2</c>."), "Відповідь <c>2</c>.")
    }

    func test_a_block_cut_off_by_the_limit_leaves_no_thinking() {
        XCTAssertEqual(withoutThinking("<|channel>thought\nThinking Process: 1. The user"), "")
        XCTAssertEqual(withoutThinking("Так. <|channel>thought\nmore"), "Так.")
    }

    func test_every_block_goes() {
        XCTAssertEqual(withoutThinking("<|channel>thought a<channel|>X<|channel>thought b<channel|>Y"), "XY")
    }
}
