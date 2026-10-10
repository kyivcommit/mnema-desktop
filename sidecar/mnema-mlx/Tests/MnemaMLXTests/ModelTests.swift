import Foundation
import MLX
import MnemaMLX
import XCTest

/// Real-model tests. They need both models on disk and run only with MNEMA_MLX_MODELS set (under
/// `xcodebuild test`, as TEST_RUNNER_MNEMA_MLX_MODELS): bge-m3-mlx-8bit and Gemma4-E2B-IT-Text-int4-flat.
final class ModelTests: XCTestCase {
    struct Reference: Decodable {
        struct Sentence: Decodable { let text: String; let embedding: [Float] }
        let sentences: [Sentence]
    }

    func models() throws -> URL {
        guard let dir = ProcessInfo.processInfo.environment["MNEMA_MLX_MODELS"] else {
            throw XCTSkip("needs MNEMA_MLX_MODELS")
        }
        return URL(fileURLWithPath: dir)
    }

    func loaded(chat: String = "Gemma4-E2B-IT-Text-int4-flat") async throws -> Engine {
        let dir = try models()
        let e = Engine()
        try await e.load(embedDir: dir.appendingPathComponent("bge-m3-mlx-8bit"),
                         chatDir: dir.appendingPathComponent(chat))
        return e
    }

    func reference() throws -> Reference {
        let url = Bundle.module.url(forResource: "bge-m3-reference", withExtension: "json", subdirectory: "Fixtures")!
        return try JSONDecoder().decode(Reference.self, from: Data(contentsOf: url))
    }

    func norm(_ v: [Float]) -> Float { v.reduce(0) { $0 + $1 * $1 }.squareRoot() }

    func test_embedding_is_unit_length() async throws {
        let e = try await loaded()
        defer { e.unload() }
        let ref = try reference()
        let vecs = try await e.embed(ref.sentences.map(\.text))
        XCTAssertEqual(vecs.count, 5)
        for (s, v) in zip(ref.sentences, vecs) {
            XCTAssertLessThan(abs(norm(v) - 1), 1e-3, "\(s.text): norm \(norm(v))")
        }
    }

    func test_embedding_matches_reference() async throws {
        let e = try await loaded()
        defer { e.unload() }
        let ref = try reference()
        let vecs = try await e.embed(ref.sentences.map(\.text))
        XCTAssertEqual(vecs.count, 5)
        for (s, v) in zip(ref.sentences, vecs) {
            let cos = zip(s.embedding, v).reduce(0) { $0 + $1.0 * $1.1 } / (norm(s.embedding) * norm(v))
            XCTAssertGreaterThanOrEqual(cos, 0.98, s.text)
            print("cos \(cos) \(s.text)")
        }
    }

    /// The product's prompt (auto language), byte for byte as mnema-rag builds it.
    static let capitalQuestion: [[String: String]] = [
        ["role": "system", "content": "You are a careful research assistant answering questions about a collection of documents. You are given numbered SOURCE passages.\nRules (follow strictly):\n1. Answer ONLY using facts found in the numbered sources; do not use outside knowledge. Expanding an abbreviation or short form to its full word, and matching an inflected or declined word form, is reading comprehension — not outside knowledge. For example, a source that writes apt. for apartment still states the apartment; expand such short forms to their full word. Match on meaning, not on exact characters.\n2. After every factual statement, cite the source(s) it came from with the inline anchor <c>N</c>, where N is the source number. Cite multiple sources as <c>1</c><c>3</c>.\n3. Never invent or guess a source number — cite only numbers that appear in the list.\n4. Say information is missing ONLY when the fact is genuinely absent, never merely because the wording or abbreviation differs from the question; add no citation anchors when you do.\n5. OUTPUT LANGUAGE — THIS IS THE MOST IMPORTANT RULE, AND IT OVERRIDES THE SOURCE LANGUAGE: write the ENTIRE answer in the same language as the question. Detect the question's language and use exactly that language (for example, a Ukrainian question gets a Ukrainian answer). The SOURCE passages may be in another language; translate any facts or quotes you cite into the question's language. NEVER answer in English unless the question itself is in English. Do NOT state or mention which language you are using — just give the answer."],
        ["role": "user", "content": "Sources:\n\n[1]\nСтолиця Франції — Париж.\n\n" + filler
            + "Question: Яка столиця Франції?\n\nWrite your entire answer in the same language as the Question above."],
    ]

    /// Unrelated sources [2]…[13] put source [1] well beyond Gemma's 512-token sliding window, where the
    /// global-attention layers have to carry it; a short prompt cannot tell a misread config apart.
    static let filler = (2 ... 13).map { n in
        "[\(n)]\n" + Array(repeating: "Склад номер \(n) має \(n * 7) полиць, на кожній по \(n + 3) коробок з деталями; "
            + "його відкривають о \(n % 12 + 1)-й годині, а ключі зберігає черговий.", count: 3).joined(separator: " ") + "\n\n"
    }.joined()

    /// Under xctest the host process's Metal cache is already warm, so the time test below cannot go red here;
    /// this one fails when `load` runs no warm-up generation (and when the warm-up is reported as a request).
    func test_load_runs_a_silent_warmup() async throws {
        let e = try await loaded()
        defer { e.unload() }
        XCTAssertNotNil(e.warmupFirstTokenMs, "load ran no warm-up generation")
        XCTAssertNil(e.lastFirstTokenMs, "the warm-up must not look like a request")
    }

    func test_first_chat_after_load_is_warm() async throws {
        let e = try await loaded()
        defer { e.unload() }
        _ = try await e.chat(Self.capitalQuestion)
        let ms = try XCTUnwrap(e.lastFirstTokenMs)
        _ = try await e.chat(Self.capitalQuestion)
        let second = try XCTUnwrap(e.lastFirstTokenMs)
        print("first_chat_after_load_ms \(ms) second \(second)")
        XCTAssertLessThan(ms, 1000, "first token \(ms) ms after load")
    }

    func test_chat_does_not_think() async throws {
        let e = try await loaded()
        defer { e.unload() }
        let content = try await e.chat(Self.capitalQuestion)
        print("answer: \(content)")
        XCTAssertFalse(content.isEmpty)
        XCTAssertFalse(content.contains("<think"), content)
        XCTAssertFalse(content.contains("<|channel"), content)
        XCTAssertFalse(content.hasPrefix("The user"), content)
    }

    /// The upstream nested config.json loads too, but the model then never cites (spike: 0/30).
    func test_gemma_reads_our_flat_config() async throws {
        let e = try await loaded()
        defer { e.unload() }
        let content = try await e.chat(Self.capitalQuestion)
        print("answer: \(content)")
        XCTAssertTrue(content.contains("<c>1</c>"), content)
        XCTAssertTrue(content.contains("Париж"), content)
    }

    func test_cache_limit_is_256_mb() async throws {
        Memory.cacheLimit = 0
        let e = try await loaded()
        defer { e.unload() }
        XCTAssertEqual(Memory.cacheLimit, 256 * 1024 * 1024)
    }

    func test_unload_frees_gpu_memory() async throws {
        let e = try await loaded()
        _ = try await e.embed(["Кіт спить на теплому підвіконні."])
        _ = try await e.chat(Self.capitalQuestion)
        XCTAssertGreaterThan(Memory.activeMemory, 0)
        e.unload()
        // Whole megabytes, as measured on both machines: ~13 KB of small arrays outlive the models
        // (13,734 bytes here), the weights do not.
        XCTAssertEqual(Memory.activeMemory / 1_048_576, 0, "\(Memory.activeMemory) bytes")
    }

    func test_long_input_is_truncated_not_fatal() async throws {
        let e = try await loaded()
        defer { e.unload() }
        let long = String(repeating: "Кіт спить на теплому підвіконні, а потяг іде до Львова. ", count: 1200)  // ~20k tokens
        let batch = (0 ..< 40).map { "Речення номер \($0) про склад і полиці." }
        for input in [[long], batch] {
            let vecs = try await e.embed(input)
            XCTAssertEqual(vecs.count, input.count)
            for v in vecs { XCTAssertLessThan(abs(norm(v) - 1), 1e-3, "norm \(norm(v))") }
        }
    }

    /// Embed loads, chat then fails: /v1/models must report the embedder that is resident, not "nothing".
    func test_models_report_what_a_failed_load_left_loaded() throws {
        let dir = try models()
        let (p, line) = try startStubRaw(args: ["--embed", dir.appendingPathComponent("bge-m3-mlx-8bit").path,
                                                "--chat", dir.appendingPathComponent("no-such-model").path])
        defer { p.terminate() }
        let port = Int(line.dropFirst(5)) ?? 0
        XCTAssertEqual(http(port, "/mnema/load", method: "POST", timeout: 60).status, 500)
        XCTAssertEqual(try MnemaMLXTests.loaded(port), ["baai/bge-m3": true, "gemma-4-e2b-it": false])
    }
}

