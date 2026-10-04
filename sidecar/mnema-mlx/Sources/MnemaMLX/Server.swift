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

    public init(token: String, embedDelayMs: Int = 0) throws {
        self.token = token
        self.embedDelayMs = embedDelayMs
        let params = NWParameters.tcp
        params.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: 0)
        listener = try NWListener(using: params)
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
            work.submit(urgent: interactive) { reply(self.embed(json["input"] as? [String] ?? [])) }
        case "/v1/chat/completions":
            work.submit(urgent: true) { reply(Response(200, ["choices": [["message": ["content": "stub <c>1</c>"]]]])) }
        case "/mnema/load":
            setLoaded(["chat", "embed"], true)
            reply(Response(204))
        case "/mnema/unload":
            setLoaded(json["models"] as? [String] ?? ["chat", "embed"], false)
            reply(Response(204))
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

    private func embed(_ inputs: [String]) -> Response {
        if embedDelayMs > 0 { usleep(UInt32(embedDelayMs) * 1000) }
        let data = inputs.indices.map { i -> [String: Any] in
            var v = [Double](repeating: 0, count: 1024)
            if i < v.count { v[i] = 1.0 }
            return ["index": i, "embedding": v]
        }
        return Response(200, ["data": data])
    }
}
