import Foundation
import MLX
import MLXEmbedders
import MLXHuggingFace
import MLXLLM
import MLXLMCommon
import Tokenizers

/// bge-m3 (embeddings) and Gemma (chat) in this process. Not thread-safe: the server calls it from its
/// single work queue, one job at a time.
public final class Engine {
    private var embedder: EmbedderModelContainer?
    private var llm: ModelContainer?

    public init() {}

    /// What is resident now, by the names `load` and `unload` take.
    public var loadedModels: [String] {
        (embedder == nil ? [] : ["embed"]) + (llm == nil ? [] : ["chat"])
    }

    /// Loads the named models ("embed", "chat") that are not loaded yet; the rest stay as they are.
    public func load(embedDir: URL, chatDir: URL, models: [String] = ["embed", "chat"]) async throws {
        // Every embed batch has a new shape and MLX keeps freed buffers for reuse; unbounded, that cache
        // grew to a 24 GB footprint over a library re-embed.
        Memory.cacheLimit = 256 * 1024 * 1024
        if models.contains("embed") && embedder == nil {
            embedder = try await EmbedderModelFactory.shared.loadContainer(
                from: embedDir, using: #huggingFaceTokenizerLoader())
        }
        if models.contains("chat") && llm == nil {
            llm = try await LLMModelFactory.shared.loadContainer(from: chatDir, using: #huggingFaceTokenizerLoader())
        }
    }

    public func unload(_ models: [String] = ["embed", "chat"]) {
        if models.contains("embed") { embedder = nil }
        if models.contains("chat") { llm = nil }
        Memory.clearCache()
    }

    public func embed(_ texts: [String]) async throws -> [[Float]] {
        guard let embedder else { throw EngineError.notLoaded }
        var out: [[Float]] = []
        var i = 0
        while i < texts.count {
            // Sub-batches of ≤ 16 texts or ≤ 48,000 characters: memory does not depend on what the client sent.
            var j = i
            var chars = 0
            while j < texts.count && j - i < 16 && (j == i || chars + texts[j].count <= 48_000) {
                chars += texts[j].count
                j += 1
            }
            let batch = Array(texts[i ..< j])
            out += await embedder.perform { ctx -> [[Float]] in
                let ids = batch.map { Array(ctx.tokenizer.encode(text: $0, addSpecialTokens: true).prefix(8192)) }
                let len = ids.map(\.count).max()!
                let pad = 1  // XLM-RoBERTa <pad>
                let padded = stacked(ids.map { MLXArray($0 + Array(repeating: pad, count: len - $0.count)) })
                let mask = stacked(ids.map {
                    MLXArray(Array(repeating: Int32(1), count: $0.count) + Array(repeating: Int32(0), count: len - $0.count))
                })
                let h = ctx.model(padded, positionIds: nil, tokenTypeIds: MLXArray.zeros(like: padded), attentionMask: mask)
                    .hiddenStates!
                // bge-m3 dense = CLS (first token) + L2 norm, by hand: the library's .cls is the BERT pooler.
                var cls = h[0..., 0, 0...]
                cls = cls / sqrt((cls * cls).sum(axis: -1, keepDims: true))
                let flat = cls.asType(.float32).asArray(Float.self)
                let dim = flat.count / batch.count
                return (0 ..< batch.count).map { Array(flat[$0 * dim ..< ($0 + 1) * dim]) }
            }
            i = j
        }
        return out
    }

    /// Greedy, no streaming. Prints `first_token_ms=<n>` to stderr: from the start of `generate` to the
    /// first token, so the 0.5 s gate measures the model, not the wire.
    public func chat(_ messages: [[String: String]]) async throws -> String {
        guard let llm else { throw EngineError.notLoaded }
        return try await llm.perform { ctx -> String in
            let chat: [Chat.Message] = messages.map {
                let text = $0["content"] ?? ""
                switch $0["role"] {
                case "system": return .system(text)
                case "assistant": return .assistant(text)
                default: return .user(text)
                }
            }
            // The processor's own prepare() matches the Python template token for token; ChatSession did not.
            let input = try await ctx.processor.prepare(
                input: UserInput(chat: chat, additionalContext: ["enable_thinking": false]))
            let start = Date()
            var first: Date?
            var out = ""
            for await g in try MLXLMCommon.generate(
                input: input, parameters: .init(maxTokens: 1024, temperature: 0), context: ctx)
            {
                if case .chunk(let x) = g {
                    if first == nil {
                        first = Date()
                        let ms = Int(first!.timeIntervalSince(start) * 1000)
                        FileHandle.standardError.write(Data("first_token_ms=\(ms)\n".utf8))
                    }
                    out += x
                }
            }
            return out
        }
    }
}

public enum EngineError: Error {
    case notLoaded
}
