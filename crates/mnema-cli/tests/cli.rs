//! The `mnema` binary against a fake agent socket: a `UnixListener` that records
//! each request line and answers a canned one (or hangs up without answering).

use mnema_core::agent_wire::{Envelope, ErrKind, Reply, VERSION, WireError, reply_line};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

struct Fake {
    _dir: tempfile::TempDir,
    path: PathBuf,
    lines: Receiver<String>,
}

/// Serves connections forever, answering each with `answer` (None = hang up).
fn fake(answer: Option<String>) -> Fake {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let (tx, lines) = channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let _ = tx.send(line);
            if let Some(a) = &answer {
                let _ = stream.write_all(a.as_bytes());
            }
        }
    });
    Fake {
        _dir: dir,
        path,
        lines,
    }
}

fn ok(v: Value) -> String {
    reply_line(&Envelope {
        v: VERSION,
        reply: Reply::Ok(v),
    })
}

fn err(kind: ErrKind, message: &str) -> String {
    reply_line(&Envelope {
        v: VERSION,
        reply: Reply::Err(WireError {
            kind,
            message: message.into(),
        }),
    })
}

fn mnema(sock: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mnema"))
        .args(args)
        .env("MNEMA_AGENT_SOCKET", sock)
        .output()
        .unwrap()
}

fn stdout_json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {:?}", o.stdout))
}

#[test]
fn search_prints_the_ok_payload_and_exits_zero() {
    let f = fake(Some(ok(
        json!({"empty": false, "passages": [{"text": "fox"}]}),
    )));
    let o = mnema(&f.path, &["search", "fox", "--json"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        stdout_json(&o),
        json!({"empty": false, "passages": [{"text": "fox"}]})
    );
}

#[test]
fn an_err_reply_prints_the_error_and_exits_one() {
    let f = fake(Some(err(ErrKind::StaleId, "re-indexed")));
    let o = mnema(&f.path, &["passage", "1:ab:0", "--json"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        stdout_json(&o),
        json!({"kind": "stale_id", "message": "re-indexed"})
    );
}

#[test]
fn no_socket_with_the_override_set_is_not_running_without_launching() {
    let dir = tempfile::tempdir().unwrap();
    let start = Instant::now();
    let o = mnema(&dir.path().join("absent.sock"), &["status", "--json"]);
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "waited {:?}",
        start.elapsed()
    );
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout_json(&o)["kind"], "not_running");
}

#[test]
fn a_dropped_connection_reads_as_disabled() {
    let f = fake(None);
    let o = mnema(&f.path, &["status", "--json"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout_json(&o)["kind"], "disabled");
}

#[test]
fn the_request_carries_v_and_the_op() {
    let f = fake(Some(ok(json!({}))));
    mnema(&f.path, &["search", "fox", "--k", "3", "--json"]);
    let sent: Value =
        serde_json::from_str(&f.lines.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
    assert_eq!(sent["v"], 1);
    assert_eq!(sent["op"], "search");
    assert_eq!(sent["query"], "fox");
    assert_eq!(sent["k"], 3);
    mnema(&f.path, &["rescan", "--wait", "--json"]);
    let sent: Value =
        serde_json::from_str(&f.lines.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
    assert_eq!(
        (sent["op"].as_str(), sent["wait"].as_bool()),
        (Some("rescan"), Some(true))
    );
}

#[test]
fn a_request_over_the_limit_is_refused_locally_without_connecting() {
    let f = fake(Some(ok(json!({}))));
    let big = "x".repeat(70_000);
    let o = mnema(&f.path, &["search", &big, "--json"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout_json(&o)["kind"], "bad_request");
    assert!(
        f.lines.recv_timeout(Duration::from_millis(300)).is_err(),
        "the fake was contacted"
    );
}

// ---- MCP over stdio ----

struct Mcp {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
}

impl Mcp {
    fn start(sock: &std::path::Path) -> Mcp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mnema"))
            .arg("mcp")
            .env("MNEMA_AGENT_SOCKET", sock)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        Mcp { child, out }
    }
    fn send(&mut self, v: Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{v}").unwrap();
    }
    fn recv(&mut self) -> Value {
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line:?}"))
    }
    fn handshake(&mut self) -> Value {
        self.send(
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"claude-test","version":"9"}}}),
        );
        let init = self.recv();
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        init
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn mcp_lists_four_tools_and_calls_search() {
    let payload = json!({"text": "рядок\nдругий", "passages": []});
    let f = fake(Some(ok(payload.clone())));
    let mut m = Mcp::start(&f.path);
    let init = m.handshake();
    assert_eq!(init["result"]["serverInfo"]["name"], "mnema");
    assert_eq!(
        init["result"]["instructions"],
        "Mnema is the user's local document library: their own books, papers and files, indexed on this machine. Use `search_library` whenever the user asks what their documents, books or library say about something, or asks to find something in them — in any language (e.g. «мої книжки», «мої документи», «моя бібліотека»). Answer from the passages it returns and cite each fact with its path and locator."
    );

    m.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let tools = m.recv();
    let mut names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["passage", "rescan", "search_library", "status"]);

    m.send(json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"search_library","arguments":{"query":"лис","k":2}}}));
    let res = m.recv();
    assert_ne!(res["result"]["isError"], json!(true));
    let text = res["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<Value>(text).unwrap(), payload);

    let sent: Value =
        serde_json::from_str(&f.lines.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
    assert_eq!(sent["client"], "claude-test");
    assert_eq!(
        (
            sent["op"].as_str(),
            sent["query"].as_str(),
            sent["k"].as_u64()
        ),
        (Some("search"), Some("лис"), Some(2))
    );
}

#[test]
fn mcp_turns_an_err_into_an_error_result() {
    let f = fake(None);
    let mut m = Mcp::start(&f.path);
    m.handshake();
    m.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"status","arguments":{}}}));
    let res = m.recv();
    assert_eq!(res["result"]["isError"], json!(true));
    let text = res["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap()["kind"],
        "disabled"
    );
}

#[test]
fn mcp_descriptions_tell_the_agent_what_started_false_empty_false_and_not_found_mean() {
    let f = fake(None);
    let mut m = Mcp::start(&f.path);
    m.handshake();
    m.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let tools = m.recv();
    let described = |name: &str| -> String {
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()["description"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert!(
        described("search_library").contains(
            "If there are no passages and `empty` is false, the search could not run: report the reason given in `arms`."
        )
    );
    assert!(
        described("passage").contains(
            "`stale_id` or `not_found` means the document was re-indexed or the passage is gone: run `search_library` again."
        )
    );
    assert!(described("rescan").contains("`started: false` means no new scan was started."));
}
