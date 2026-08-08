//! A rude little HTTP server for the tests.
//!
//! Handwritten rather than a dependency: what these tests need is the ability
//! to answer *badly* — a truncated body, a silence, a 200 with the wrong shape
//! — and a well-behaved mock library is built to make that awkward.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

pub struct Reply {
    pub status: u16,
    pub body: String,
    /// Answer this late. `Duration::ZERO` for everything except the timeout case.
    pub delay: Duration,
}

impl Reply {
    pub fn ok(body: &str) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
            delay: Duration::ZERO,
        }
    }
    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            delay: Duration::ZERO,
        }
    }
    pub fn slow(seconds: u64) -> Self {
        Self {
            status: 200,
            body: "{}".into(),
            delay: Duration::from_secs(seconds),
        }
    }
}

pub struct MockServer {
    base: String,
    seen: Receiver<String>,
}

impl MockServer {
    /// Answers `replies` in order, one per connection.
    pub fn new(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a port");
        let port = listener.local_addr().expect("read the port").port();
        let (tx, seen): (Sender<String>, Receiver<String>) = channel();

        thread::spawn(move || {
            for (stream, reply) in listener.incoming().zip(replies) {
                let mut stream = stream.expect("accept");
                let request = read_request(&mut stream);
                let _ = tx.send(request);
                thread::sleep(reply.delay);
                let head = format!(
                    "HTTP/1.1 {} MOCK\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n",
                    reply.status,
                    reply.body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(reply.body.as_bytes());
            }
        });

        Self {
            base: format!("http://127.0.0.1:{port}"),
            seen,
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// The next request as raw text, headers and body together. Panics if none
    /// arrived — a probe that made no call is a test result, not a hang.
    pub fn request(&self) -> String {
        self.seen
            .recv_timeout(Duration::from_secs(10))
            .expect("the client made no request")
    }
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut reader = BufReader::new(stream.try_clone().expect("clone the socket"));
    let mut head = String::new();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).expect("read a header line") == 0 {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            length = rest.trim().parse().unwrap_or(0);
        }
        let blank = line == "\r\n" || line == "\n";
        head.push_str(&line);
        if blank {
            break;
        }
    }
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body).expect("read the body");
    }
    head + &String::from_utf8_lossy(&body)
}

/// Two vectors of `width`, differing in the first component, in the shape the
/// provider answers a two-text request with. Here rather than in a test file
/// because both packages' tests build the same body.
pub fn two_vectors(width: usize) -> String {
    let row = |hot: usize| -> String {
        (0..width)
            .map(|i| if i == hot { "1.0" } else { "0.0" })
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        r#"{{"data":[{{"embedding":[{}],"index":0}},{{"embedding":[{}],"index":1}}]}}"#,
        row(0),
        row(1)
    )
}
