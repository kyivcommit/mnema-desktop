import Foundation
import XCTest

/// Spawns the built `mnema-mlx --stub`; returns the process and its first stdout line.
func startStubRaw(token: String = "t", env: [String: String] = [:]) throws -> (Process, String) {
    let exe = Bundle(for: StubAnchor.self).bundleURL.deletingLastPathComponent()
        .appendingPathComponent("mnema-mlx")
    let p = Process()
    p.executableURL = exe
    p.arguments = ["--stub"]
    var e = ProcessInfo.processInfo.environment
    e["MNEMA_MLX_TOKEN"] = token
    for (k, v) in env { e[k] = v }
    p.environment = e
    let out = Pipe()
    p.standardOutput = out
    p.standardInput = Pipe()
    p.standardError = FileHandle.nullDevice
    try p.run()
    var line = Data()
    while let b = try out.fileHandleForReading.read(upToCount: 1), !b.isEmpty, b[0] != 10 {
        line.append(b)
    }
    return (p, String(decoding: line, as: UTF8.self))
}

func startStub(token: String = "t", env: [String: String] = [:]) throws -> (Process, port: Int) {
    let (p, line) = try startStubRaw(token: token, env: env)
    return (p, Int(line.dropFirst(5)) ?? 0)
}

final class StubAnchor {}

/// Blocking HTTP call; status 0 means no answer within `timeout`.
func http(_ port: Int, _ path: String, method: String = "GET", token: String? = "t",
          body: String? = nil, timeout: TimeInterval = 3) -> (status: Int, data: Data) {
    var req = URLRequest(url: URL(string: "http://127.0.0.1:\(port)\(path)")!, timeoutInterval: timeout)
    req.httpMethod = method
    if let token { req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization") }
    if let body {
        req.httpBody = Data(body.utf8)
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
    }
    var out: (Int, Data) = (0, Data())
    let done = DispatchSemaphore(value: 0)
    URLSession(configuration: .ephemeral).dataTask(with: req) { d, r, _ in
        out = ((r as? HTTPURLResponse)?.statusCode ?? 0, d ?? Data())
        done.signal()
    }.resume()
    done.wait()
    return out
}

/// True when a TCP connection to host:port succeeds.
func connects(host: String, port: Int) -> Bool {
    let fd = socket(AF_INET, SOCK_STREAM, 0)
    defer { close(fd) }
    var a = sockaddr_in()
    a.sin_family = sa_family_t(AF_INET)
    a.sin_port = in_port_t(port).bigEndian
    inet_pton(AF_INET, host, &a.sin_addr)
    return withUnsafePointer(to: &a) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
            connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
        }
    } == 0
}

/// `loaded` flags of /v1/models keyed by model id.
func loaded(_ port: Int) throws -> [String: Bool] {
    let r = http(port, "/v1/models")
    let data = (try JSONSerialization.jsonObject(with: r.data) as? [String: Any])?["data"] as? [[String: Any]] ?? []
    return Dictionary(uniqueKeysWithValues: data.map { ($0["id"] as! String, $0["loaded"] as! Bool) })
}

final class ProtocolTests: XCTestCase {
    func test_prints_port_first() throws {
        let (p, line) = try startStubRaw()
        defer { p.terminate() }
        XCTAssertNotNil(line.range(of: "^PORT [1-9][0-9]*$", options: .regularExpression), line)
        XCTAssertTrue(connects(host: "127.0.0.1", port: Int(line.dropFirst(5)) ?? 0))
    }

    func test_401_without_token() throws {
        let (p, port) = try startStub()
        defer { p.terminate() }
        XCTAssertEqual(http(port, "/v1/models", token: nil).status, 401)
        XCTAssertEqual(http(port, "/v1/models", token: "x").status, 401)
        XCTAssertEqual(http(port, "/v1/models", token: "t").status, 200)
    }

    func test_listens_only_on_loopback() throws {
        var list: UnsafeMutablePointer<ifaddrs>?
        getifaddrs(&list)
        defer { freeifaddrs(list) }
        var other: String?
        var cur = list
        while let i = cur?.pointee {
            if let a = i.ifa_addr, a.pointee.sa_family == sa_family_t(AF_INET), i.ifa_flags & UInt32(IFF_LOOPBACK) == 0 {
                var sin = UnsafeRawPointer(a).load(as: sockaddr_in.self)
                var buf = [CChar](repeating: 0, count: Int(INET_ADDRSTRLEN))
                inet_ntop(AF_INET, &sin.sin_addr, &buf, socklen_t(INET_ADDRSTRLEN))
                other = String(cString: buf)
                break
            }
            cur = i.ifa_next
        }
        guard let other else { throw XCTSkip("no non-loopback IPv4 address on this machine") }
        let (p, port) = try startStub()
        defer { p.terminate() }
        XCTAssertTrue(connects(host: "127.0.0.1", port: port))
        XCTAssertFalse(connects(host: other, port: port), "reachable on \(other)")
    }

    func test_exits_when_stdin_closes() throws {
        let (p, _) = try startStub()
        defer { if p.isRunning { p.terminate() } }
        try (p.standardInput as! Pipe).fileHandleForWriting.close()
        let deadline = Date().addingTimeInterval(2)
        while p.isRunning && Date() < deadline { usleep(20_000) }
        XCTAssertFalse(p.isRunning, "still running 2 s after stdin closed")
        if !p.isRunning { XCTAssertEqual(p.terminationStatus, 0) }
    }

    func test_embeddings_shape() throws {
        let (p, port) = try startStub()
        defer { p.terminate() }
        let r = http(port, "/v1/embeddings", method: "POST",
                     body: #"{"model":"baai/bge-m3","input":["a","b","c"]}"#)
        XCTAssertEqual(r.status, 200)
        let data = (try JSONSerialization.jsonObject(with: r.data) as? [String: Any])?["data"] as? [[String: Any]]
        XCTAssertEqual(data?.count, 3)
        for (i, item) in (data ?? []).enumerated() {
            XCTAssertEqual(item["index"] as? Int, i)
            let v = item["embedding"] as? [Double] ?? []
            XCTAssertEqual(v.count, 1024)
            XCTAssertEqual(v.firstIndex(of: 1.0), i)
            XCTAssertEqual(v.reduce(0, +), 1.0)
        }
    }

    func test_chat_shape() throws {
        let (p, port) = try startStub()
        defer { p.terminate() }
        let r = http(port, "/v1/chat/completions", method: "POST",
                     body: #"{"model":"gemma-4-e2b-it","messages":[{"role":"user","content":"hi"}]}"#)
        XCTAssertEqual(r.status, 200)
        let choices = (try JSONSerialization.jsonObject(with: r.data) as? [String: Any])?["choices"] as? [[String: Any]]
        let message = choices?.first?["message"] as? [String: Any]
        XCTAssertEqual(message?["content"] as? String, "stub <c>1</c>")
    }

    func test_load_unload_204() throws {
        let (p, port) = try startStub()
        defer { p.terminate() }
        XCTAssertEqual(http(port, "/mnema/load", method: "POST").status, 204)
        XCTAssertEqual(try loaded(port), ["baai/bge-m3": true, "gemma-4-e2b-it": true])
        XCTAssertEqual(http(port, "/mnema/unload", method: "POST").status, 204)
        XCTAssertEqual(try loaded(port), ["baai/bge-m3": false, "gemma-4-e2b-it": false])
    }

    func test_unload_one_model() throws {
        let (p, port) = try startStub()
        defer { p.terminate() }
        XCTAssertEqual(http(port, "/mnema/load", method: "POST").status, 204)
        XCTAssertEqual(http(port, "/mnema/unload", method: "POST", body: #"{"models":["chat"]}"#).status, 204)
        XCTAssertEqual(try loaded(port), ["baai/bge-m3": true, "gemma-4-e2b-it": false])
    }

    func test_interactive_jumps_the_queue() throws {
        let (p, port) = try startStub(env: ["MNEMA_STUB_EMBED_MS": "500"])
        defer { p.terminate() }
        let lock = NSLock()
        var finished: [String] = []
        let group = DispatchGroup()
        func fire(_ name: String, _ path: String, after ms: UInt32) {
            group.enter()
            Thread.detachNewThread {
                usleep(ms * 1000)
                let r = http(port, path, method: "POST", body: #"{"model":"baai/bge-m3","input":["x"]}"#, timeout: 10)
                lock.lock(); finished.append(r.status == 200 ? name : "\(name):\(r.status)"); lock.unlock()
                group.leave()
            }
        }
        for i in 0..<3 { fire("scan\(i)", "/v1/embeddings", after: UInt32(i) * 10) }
        fire("interactive", "/interactive/v1/embeddings", after: 100)
        group.wait()
        XCTAssertEqual(finished, ["scan0", "interactive", "scan1", "scan2"])
    }

    func test_unknown_embed_model_is_404() throws {
        let (p, port) = try startStub()
        defer { p.terminate() }
        let foreign = http(port, "/v1/embeddings", method: "POST",
                           body: #"{"model":"openai/text-embedding-3-small","input":["a"]}"#)
        XCTAssertEqual(foreign.status, 404)
        XCTAssertEqual(String(decoding: foreign.data, as: UTF8.self), #"{"error":"unknown model"}"#)
        XCTAssertEqual(http(port, "/v1/embeddings", method: "POST",
                            body: #"{"model":"baai/bge-m3","input":["a"]}"#).status, 200)
        XCTAssertEqual(http(port, "/v1/chat/completions", method: "POST",
                            body: #"{"model":"whatever","messages":[]}"#).status, 200)
    }
}
