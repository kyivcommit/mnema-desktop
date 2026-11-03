import MLXLMCommon
import MnemaMLX
import XCTest

/// `toolNames`: a tool result takes the name of the call it answers, though ids restart per response (D188).
final class ToolNamesTests: XCTestCase {
    private func call(_ name: String, _ id: String) -> ToolCall {
        ToolCall(function: .init(name: name, arguments: [:]), id: id)
    }

    func test_a_reused_id_names_the_latest_call_before_it() {
        let turns = [
            Turn(role: "user", content: "q1"),
            Turn(role: "assistant", content: "", toolCalls: [call("search", "call_0")]),
            Turn(role: "tool", content: "r1", toolCallId: "call_0"),
            Turn(role: "assistant", content: "a1"),
            Turn(role: "user", content: "q2"),
            Turn(role: "assistant", content: "", toolCalls: [call("open", "call_0")]),
            Turn(role: "tool", content: "r2", toolCallId: "call_0"),
        ]
        XCTAssertEqual(toolNames(turns), [nil, nil, "search", nil, nil, nil, "open"])
    }

    func test_an_unknown_id_has_no_name() {
        XCTAssertEqual(toolNames([Turn(role: "tool", content: "r", toolCallId: "call_9")]), [nil])
    }
}
