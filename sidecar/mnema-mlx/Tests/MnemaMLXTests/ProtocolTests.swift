import Foundation
import XCTest

/// Spawns the built `mnema-mlx` (`--stub` unless `args` says otherwise); returns the process and its first stdout line.
func startStubRaw(token: String = "t", env: [String: String] = [:], stderr: Pipe? = nil,
                  args: [String] = ["--stub"]) throws -> (Process, String) {
    let exe = Bundle(for: StubAnchor.self).bundleURL.deletingLastPathComponent()
        .appendingPathComponent("mnema-mlx")
    let p = Process()
    p.executableURL = exe
    p.arguments = args
    var e = ProcessInfo.processInfo.environment
    e["MNEMA_MLX_TOKEN"] = token
    for (k, v) in env { e[k] = v }
    p.environment = e
    let out = Pipe()
    p.standardOutput = out
    p.standardInput = Pipe()
    p.standardError = stderr ?? FileHandle.nullDevice
    try p.run()
    // Read on a side thread: a binary that never prints a newline must fail the test, not hang the run.
    var line = Data()
    let got = DispatchSemaphore(value: 0)
    Thread.detachNewThread {
        while let b = try? out.fileHandleForReading.read(upToCount: 1), !b.isEmpty, b[0] != 10 {
            line.append(b)
        }
        got.signal()
    }
    if got.wait(timeout: .now() + 10) == .timedOut {
        p.terminate()
        got.wait()
    }
    return (p, String(decoding: line, as: UTF8.self))
}

func startStub(token: String = "t", env: [String: String] = [:]) throws -> (Process, port: Int) {
    let (p, line) = try startStubRaw(token: token, env: env)
    return (p, Int(line.dropFirst(5)) ?? 0)
}

final class StubAnchor {}

/// The stub's stderr trace (`stub: queued … embed`, `stub: started embed`), read on a side thread so a
/// test can wait until a job is queued or running instead of sleeping a fixed time and hoping.
final class StubTrace {
    let pipe = Pipe()
    private let cond = NSCondition()
    private var lines: [String] = []

    init() {
        let handle = pipe.fileHandleForReading
        Thread.detachNewThread { [self] in
            var buf = Data()
            while let b = try? handle.read(upToCount: 1), !b.isEmpty {
                if b[0] == 10 {
                    cond.lock(); lines.append(String(decoding: buf, as: UTF8.self)); cond.broadcast(); cond.unlock()
                    buf.removeAll()
                } else {
                    buf.append(b)
                }
            }
        }
    }

    /// Waits until `line` has appeared `count` times; false after 10 s.
    func wait(for line: String, count: Int = 1) -> Bool {
        let deadline = Date().addingTimeInterval(10)
        cond.lock(); defer { cond.unlock() }
        while lines.filter({ $0 == line }).count < count {
            if !cond.wait(until: deadline) { return false }
        }
        return true
    }
}

func startTracedStub(embedMs: Int) throws -> (Process, port: Int, StubTrace) {
    let trace = StubTrace()
    let (p, line) = try startStubRaw(env: ["MNEMA_STUB_EMBED_MS": "\(embedMs)"], stderr: trace.pipe)
    return (p, Int(line.dropFirst(5)) ?? 0, trace)
}

/// Sends `text` verbatim over a raw socket; returns everything read back ("" when the peer closes silently).
func raw(_ port: Int, _ text: String) -> String {
    let fd = socket(AF_INET, SOCK_STREAM, 0)
    defer { close(fd) }
    var tv = timeval(tv_sec: 3, tv_usec: 0)
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))
    var a = sockaddr_in()
    a.sin_family = sa_family_t(AF_INET)
    a.sin_port = in_port_t(port).bigEndian
    inet_pton(AF_INET, "127.0.0.1", &a.sin_addr)
    let rc = withUnsafePointer(to: &a) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
    }
    guard rc == 0 else { return "" }
    _ = text.withCString { send(fd, $0, strlen($0), 0) }
    var out = Data()
    var buf = [UInt8](repeating: 0, count: 4096)
    while true {
        let n = recv(fd, &buf, buf.count, 0)
        if n <= 0 { break }
        out.append(contentsOf: buf[..<n])
    }
    return String(decoding: out, as: UTF8.self)
}

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
    let session = URLSession(configuration: .ephemeral)
    defer { session.invalidateAndCancel() }
    session.dataTask(with: req) { d, r, _ in
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
        XCTAssertEqual(http(port, "/mnema/load", method: "POST").status, 204)
        XCTAssertEqual(http(port, "/mnema/unload", method: "POST", body: #"{"models":["embed"]}"#).status, 204)
        XCTAssertEqual(try loaded(port), ["baai/bge-m3": false, "gemma-4-e2b-it": true])
    }

    func test_interactive_jumps_the_queue() throws {
        let (p, port, trace) = try startTracedStub(embedMs: 500)
        defer { p.terminate() }
        let lock = NSLock()
        var finished: [String] = []
        let group = DispatchGroup()
        func fire(_ name: String, _ path: String) {
            group.enter()
            Thread.detachNewThread {
                let r = http(port, path, method: "POST", body: #"{"model":"baai/bge-m3","input":["x"]}"#, timeout: 10)
                lock.lock(); finished.append(r.status == 200 ? name : "\(name):\(r.status)"); lock.unlock()
                group.leave()
            }
        }
        for i in 0..<3 { fire("scan\(i)", "/v1/embeddings") }
        // The interactive request goes in only once all three scans are queued and one of them runs.
        XCTAssertTrue(trace.wait(for: "stub: queued scan embed", count: 3), "scans never queued")
        XCTAssertTrue(trace.wait(for: "stub: started embed"), "no scan started")
        fire("interactive", "/interactive/v1/embeddings")
        group.wait()
        // Only the claim under test: the interactive request finishes second, behind the job already running.
        // Which of the three scans arrives first is up to the scheduler, so their order is not asserted.
        XCTAssertEqual(finished.count, 4)
        XCTAssertEqual(finished[1], "interactive", "\(finished)")
        XCTAssertFalse(finished.contains { $0.contains(":") }, "\(finished)")
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
                            body: #"{"model":"whatever","messages":[{"role":"user","content":"hi"}]}"#).status, 200)
    }

    /// Unloading while a job runs would free a model in use: unload queues behind the running embed.
    func test_unload_waits_for_the_running_job() throws {
        let (p, port, trace) = try startTracedStub(embedMs: 500)
        defer { p.terminate() }
        let lock = NSLock()
        var finished: [String] = []
        let group = DispatchGroup()
        func fire(_ name: String, _ path: String, _ body: String) {
            group.enter()
            Thread.detachNewThread {
                let r = http(port, path, method: "POST", body: body, timeout: 10)
                lock.lock(); finished.append("\(name):\(r.status)"); lock.unlock()
                group.leave()
            }
        }
        fire("embed", "/v1/embeddings", #"{"model":"baai/bge-m3","input":["x"]}"#)
        // The unload goes in only once the embed is running, so it arrives during the job.
        XCTAssertTrue(trace.wait(for: "stub: started embed"), "the embed never started")
        fire("unload", "/mnema/unload", #"{"models":["embed"]}"#)
        group.wait()
        XCTAssertEqual(finished, ["embed:200", "unload:204"])
    }

    func test_negative_content_length_does_not_kill_the_process() throws {
        let (p, port) = try startStub()
        defer { p.terminate() }
        let r = raw(port, "POST /v1/embeddings HTTP/1.1\r\nContent-Length: -1\r\n\r\n")
        XCTAssertTrue(r.hasPrefix("HTTP/1.1 4"), "got: \(r.prefix(40))")
        XCTAssertEqual(http(port, "/v1/models").status, 200)
        XCTAssertTrue(p.isRunning)
    }

    func test_refuses_to_start_without_a_token() throws {
        let err = Pipe()
        let (p, line) = try startStubRaw(token: "", stderr: err)
        defer { if p.isRunning { p.terminate() } }
        let deadline = Date().addingTimeInterval(3)
        while p.isRunning && Date() < deadline { usleep(20_000) }
        XCTAssertFalse(p.isRunning, "started with an empty token")
        if !p.isRunning { XCTAssertNotEqual(p.terminationStatus, 0) }
        XCTAssertFalse(line.hasPrefix("PORT"), line)
        p.terminate()
        let reason = String(decoding: err.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        XCTAssertTrue(reason.contains("MNEMA_MLX_TOKEN"), reason)
    }

    func test_embed_input_must_be_a_string_array() throws {
        let (p, port) = try startStub()
        defer { p.terminate() }
        for body in [#"{"model":"baai/bge-m3"}"#, #"{"model":"baai/bge-m3","input":"text"}"#,
                     #"{"model":"baai/bge-m3","input":[1,2]}"#] {
            XCTAssertEqual(http(port, "/v1/embeddings", method: "POST", body: body).status, 400, body)
        }
    }

    func test_routes_check_method_and_interactive_prefix() throws {
        let (p, port) = try startStub()
        defer { p.terminate() }
        XCTAssertEqual(http(port, "/v1/embeddings").status, 405)
        XCTAssertEqual(http(port, "/v1/models", method: "POST").status, 405)
        XCTAssertEqual(http(port, "/interactive/mnema/load", method: "POST").status, 404)
        XCTAssertEqual(try loaded(port)["gemma-4-e2b-it"], false)
    }

    func test_malformed_start_line_is_answered() throws {
        let (p, port) = try startStub()
        defer { p.terminate() }
        XCTAssertTrue(raw(port, "GARBAGE\r\n\r\n").hasPrefix("HTTP/1.1 400"))
    }

    /// A message that does not decode must not silently drop the conversation and run the model on nothing.
    func test_malformed_messages_are_400() throws {
        let (p, port) = try startStub()
        defer { p.terminate() }
        for messages in [
            #"[{"role":"user","content":null}]"#,
            #"[{"role":"user"}]"#,
            #"[{"content":"hi"}]"#,
            #"[{"role":"user","content":[{"type":"text","text":"hi"}]}]"#,
            #"[]"#,
            #""hi""#,
        ] {
            let r = http(port, "/v1/chat/completions", method: "POST", body: #"{"model":"x","messages":"# + messages + "}")
            XCTAssertEqual(r.status, 400, messages)
        }
        XCTAssertEqual(http(port, "/v1/chat/completions", method: "POST", body: #"{"model":"x"}"#).status, 400, "no messages")
        XCTAssertEqual(http(port, "/v1/chat/completions", method: "POST",
                            body: #"{"model":"x","messages":[{"role":"user","content":"hi"}]}"#).status, 200)
    }
}

