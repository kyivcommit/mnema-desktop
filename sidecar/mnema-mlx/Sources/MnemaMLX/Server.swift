import Foundation
import Network

struct Request {
    let method: String
    let path: String
    let headers: [String: String]  // lower-cased names
    let body: Data
}

struct Response {
    let status: Int
    let body: Data

    init(_ status: Int, _ json: Any? = nil) {
        self.status = status
        body = json.flatMap { try? JSONSerialization.data(withJSONObject: $0) } ?? Data()
    }
}

/// One worker thread; `urgent` jobs run before the rest, a running job is never interrupted.
final class WorkQueue {
    private let cond = NSCondition()
    private var urgent: [() -> Void] = []
    private var normal: [() -> Void] = []

    init() {
        let t = Thread { [self] in
            while true {
                cond.lock()
                while urgent.isEmpty && normal.isEmpty { cond.wait() }
                let job = urgent.isEmpty ? normal.removeFirst() : urgent.removeFirst()
                cond.unlock()
                job()
            }
        }
        t.start()
    }

    func submit(urgent isUrgent: Bool, _ job: @escaping () -> Void) {
        cond.lock()
        if isUrgent { urgent.append(job) } else { normal.append(job) }
        cond.signal()
        cond.unlock()
    }
}

public final class Server {
    private let listener: NWListener
    private let queue = DispatchQueue(label: "mnema-mlx.server")
    private let work = WorkQueue()  // one model at a time
    private let token: String
    private let embedDelayMs: Int
    private let lock = NSLock()
    private var loaded = ["chat": false, "embed": false]
    /// nil: `--stub`.
    private let models: (engine: Engine, embedDir: URL, chatDir: URL)?

    public init(token: String, embedDelayMs: Int = 0) throws {
        self.token = token
        self.embedDelayMs = embedDelayMs
        models = nil
        listener = try Self.listen()
    }

    public init(token: String, embedDir: URL, chatDir: URL) throws {
        self.token = token
        embedDelayMs = 0
        models = (Engine(), embedDir, chatDir)
        listener = try Self.listen()
    }

    private static func listen() throws -> NWListener {
        let params = NWParameters.tcp
        params.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: 0)
        return try NWListener(using: params)
    }

    /// Starts listening and returns the port the OS picked.
    public func start() throws -> UInt16 {
        let ready = DispatchSemaphore(value: 0)
        listener.stateUpdateHandler = { if case .ready = $0 { ready.signal() } }
        listener.newConnectionHandler = { [self] conn in
            conn.start(queue: queue)
            receive(conn, Data())
        }
        listener.start(queue: queue)
        ready.wait()
        return listener.port!.rawValue
    }

    private func receive(_ conn: NWConnection, _ buf: Data) {
        conn.receive(minimumIncompleteLength: 1, maximumLength: 1 << 20) { [self] data, _, done, err in
            var buf = buf
            buf.append(data ?? Data())
            if let req = parse(buf) {
                handle(req) { resp in
                    var head = "HTTP/1.1 \(resp.status) X\r\nContent-Length: \(resp.body.count)\r\n"
                    head += "Content-Type: application/json\r\nConnection: close\r\n\r\n"
                    conn.send(content: Data(head.utf8) + resp.body, completion: .contentProcessed { _ in conn.cancel() })
                }
            } else if done || err != nil {
                conn.cancel()
            } else {
                receive(conn, buf)
            }
        }
    }

    /// Returns nil until the whole request (headers and Content-Length body) has arrived.
    private func parse(_ buf: Data) -> Request? {
        guard let end = buf.range(of: Data("\r\n\r\n".utf8)) else { return nil }
        let lines = String(decoding: buf[..<end.lowerBound], as: UTF8.self).components(separatedBy: "\r\n")
        let start = lines[0].split(separator: " ")
        guard start.count >= 2 else { return nil }
        var headers: [String: String] = [:]
        for l in lines.dropFirst() {
            if let c = l.firstIndex(of: ":") {
                headers[l[..<c].lowercased()] = l[l.index(after: c)...].trimmingCharacters(in: .whitespaces)
            }
        }
        let n = Int(headers["content-length"] ?? "0") ?? 0
        let body = buf[end.upperBound...]
        guard body.count >= n else { return nil }
        return Request(method: String(start[0]), path: String(start[1]), headers: headers, body: Data(body.prefix(n)))
    }

    private func handle(_ req: Request, reply: @escaping (Response) -> Void) {
        guard req.headers["authorization"] == "Bearer \(token)" else {
            return reply(Response(401, ["error": "unauthorized"]))
        }
        let interactive = req.path.hasPrefix("/interactive/")
        let path = interactive ? String(req.path.dropFirst("/interactive".count)) : req.path
        let json = (try? JSONSerialization.jsonObject(with: req.body)) as? [String: Any] ?? [:]
        switch path {
        case "/v1/embeddings":
            guard json["model"] as? String == "baai/bge-m3" else {
                return reply(Response(404, ["error": "unknown model"]))
            }
            let input = json["input"] as? [String] ?? []
            work.submit(urgent: interactive) { reply(self.embed(input)) }
        case "/v1/chat/completions":
            let messages = json["messages"] as? [[String: String]] ?? []
            work.submit(urgent: true) { reply(self.chat(messages)) }
        // Load and unload wait for the running job in the same queue: never free a model in use.
        case "/mnema/load":
            work.submit(urgent: false) { reply(self.load(["chat", "embed"])) }
        case "/mnema/unload":
            let names = json["models"] as? [String] ?? ["chat", "embed"]
            work.submit(urgent: false) {
                self.models?.engine.unload(names)
                self.setLoaded(names, false)
                reply(Response(204))
            }
        case "/v1/models":
            lock.lock(); defer { lock.unlock() }
            reply(Response(200, ["data": [
                ["id": "baai/bge-m3", "loaded": loaded["embed"]!],
                ["id": "gemma-4-e2b-it", "loaded": loaded["chat"]!],
            ]]))
        default:
            reply(Response(404, ["error": "not found"]))
        }
    }

    private func setLoaded(_ models: [String], _ value: Bool) {
        lock.lock(); defer { lock.unlock() }
        for m in models where loaded[m] != nil { loaded[m] = value }
    }

    /// Runs on the work queue. Loads the models that are not loaded yet.
    private func load(_ names: [String]) -> Response {
        if let m = models {
            if case .failure(let e) = blocking({ try await m.engine.load(embedDir: m.embedDir, chatDir: m.chatDir, models: names) }) {
                return Response(500, ["error": "load: \(e)"])
            }
        }
        setLoaded(names, true)
        return Response(204)
    }

    private func chat(_ messages: [[String: String]]) -> Response {
        guard let m = models else { return Response(200, ["choices": [["message": ["content": "stub <c>1</c>"]]]]) }
        let r = load(["chat"])
        guard r.status == 204 else { return r }
        switch blocking({ try await m.engine.chat(messages) }) {
        case .success(let content): return Response(200, ["choices": [["message": ["content": content]]]])
        case .failure(let e): return Response(500, ["error": "chat: \(e)"])
        }
    }

    private func embed(_ inputs: [String]) -> Response {
        if let m = models {
            let r = load(["embed"])
            guard r.status == 204 else { return r }
            switch blocking({ try await m.engine.embed(inputs) }) {
            case .success(let vecs):
                return Response(200, ["data": vecs.enumerated().map { ["index": $0.offset, "embedding": $0.element] }])
            case .failure(let e): return Response(500, ["error": "embed: \(e)"])
            }
        }
        if embedDelayMs > 0 { usleep(UInt32(embedDelayMs) * 1000) }
        let data = inputs.indices.map { i -> [String: Any] in
            var v = [Double](repeating: 0, count: 1024)
            if i < v.count { v[i] = 1.0 }
            return ["index": i, "embedding": v]
        }
        return Response(200, ["data": data])
    }
}

/// The work queue is a plain thread; the engine is async. Block the thread until the job is done.
private func blocking<T>(_ f: @escaping () async throws -> T) -> Result<T, Error> {
    var out: Result<T, Error>!
    let done = DispatchSemaphore(value: 0)
    Task {
        do { out = .success(try await f()) } catch { out = .failure(error) }
        done.signal()
    }
    done.wait()
    return out
}
