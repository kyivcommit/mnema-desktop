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

    /// Milliseconds from the start of the last `chat` generation to its first token.
    public private(set) var lastFirstTokenMs: Int?
    /// Same for the generation `load` runs to warm the chat model; kept apart so a warm-up never looks like a request.
    public private(set) var warmupFirstTokenMs: Int?

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
            // The first embed in a process pays Metal kernel compilation (1.3 s measured on a never-run binary);
            // a failed warm-up must not leave a model that reports itself loaded.
            do { _ = try await embed(["warm-up"]) } catch { embedder = nil; throw error }
        }
        if models.contains("chat") && llm == nil {
            llm = try await LLMModelFactory.shared.loadContainer(from: chatDir, using: #huggingFaceTokenizerLoader())
            // Same for the first generation (2.7 s measured): pay it here, in the background load.
            do {
                _ = try await generate([Turn(role: "user", content: "Hi")], tools: nil, maxTokens: 1, report: false)
            } catch { llm = nil; throw error }
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
        try await generate(messages.map { Turn(role: $0["role"] ?? "user", content: $0["content"] ?? "") },
                           tools: nil, maxTokens: 1024, report: true).text
    }

    /// A turn that may end in tool calls: with `tools`, the model's calls come back parsed (D188).
    public func chat(_ turns: [Turn], tools: [ToolSpec]?) async throws -> ChatOutput {
        let (text, calls) = try await generate(turns, tools: tools, maxTokens: 1024, report: true)
        return calls.isEmpty ? .text(text) : .toolCalls(calls)
    }

    /// `report: false` is the warm-up: the time goes to `warmupFirstTokenMs`, nothing to stderr.
    private func generate(_ turns: [Turn], tools: [ToolSpec]?, maxTokens: Int, report: Bool) async throws
        -> (text: String, calls: [ToolCall])
    {
        guard let llm else { throw EngineError.notLoaded }
        return try await llm.perform { ctx -> (text: String, calls: [ToolCall]) in
            let names = toolNames(turns)
            let chat: [Chat.Message] = zip(turns, names).map { turn, name in
                switch turn.role {
                case "system": return .system(turn.content)
                case "assistant": return .assistant(turn.content, toolCalls: turn.toolCalls.isEmpty ? nil : turn.toolCalls)
                case "tool": return .tool(turn.content, id: turn.toolCallId, name: name)
                default: return .user(turn.content)
                }
            }
            // The processor's own prepare() matches the Python template token for token; ChatSession did not.
            let input = try await ctx.processor.prepare(
                input: UserInput(chat: chat, tools: tools, additionalContext: ["enable_thinking": false]))
            let start = Date()
            var first: Date?
            var out = ""
            var calls: [ToolCall] = []
            for await g in try MLXLMCommon.generate(
                input: input, parameters: .init(maxTokens: maxTokens, temperature: 0), context: ctx, tools: tools)
            {
                if case .toolCall(let call) = g { calls.append(call) }
                if case .chunk(let x) = g {
                    if first == nil {
                        first = Date()
                        let ms = Int(first!.timeIntervalSince(start) * 1000)
                        if report {
                            self.lastFirstTokenMs = ms
                            FileHandle.standardError.write(Data("first_token_ms=\(ms)\n".utf8))
                        } else {
                            self.warmupFirstTokenMs = ms
                        }
                    }
                    out += x
                }
            }
            return (withoutThinking(out), calls)
        }
    }
}

/// One chat message as the wire gives it. `toolCalls`: an assistant turn that asked for tools;
/// role `tool`: what a tool returned.
/// The tool's name for each `tool` turn (nil elsewhere): a tool result names its call by id only, and the
/// template wants the name too. Ids restart with every response (`call_0`…), so an id means the call of the
/// latest assistant turn before it that used it.
public func toolNames(_ turns: [Turn]) -> [String?] {
    var seen: [String: String] = [:]
    return turns.map { turn in
        for c in turn.toolCalls { if let id = c.id { seen[id] = c.function.name } }
        return turn.role == "tool" ? turn.toolCallId.flatMap { seen[$0] } : nil
    }
}

public struct Turn: Sendable {
    public let role: String
    public let content: String
    public let toolCalls: [ToolCall]
    public let toolCallId: String?

    public init(role: String, content: String, toolCalls: [ToolCall] = [], toolCallId: String? = nil) {
        self.role = role
        self.content = content
        self.toolCalls = toolCalls
        self.toolCallId = toolCallId
    }
}

public enum ChatOutput: Equatable, Sendable {
    case text(String)
    case toolCalls([ToolCall])
}

public enum EngineError: Error {
    case notLoaded
}

/// The answer without Gemma 4's thinking channel. After a tool result the template continues the model's
/// own turn and writes no empty thought block, so E4B thinks on its own (measured 2026-11-03: 9 of 24
/// answers after a search); llama.cpp moves the block out of the content, and so does this. A block cut
/// off by the token limit goes to the end: what follows its opening is thinking, never the answer.
public func withoutThinking(_ text: String) -> String {
    var out = text
    while let open = out.range(of: "<|channel>thought") {
        if let close = out.range(of: "<channel|>", range: open.upperBound ..< out.endIndex) {
            out.removeSubrange(open.lowerBound ..< close.upperBound)
        } else {
            out.removeSubrange(open.lowerBound ..< out.endIndex)
        }
    }
    return out.trimmingCharacters(in: .whitespacesAndNewlines)
}
