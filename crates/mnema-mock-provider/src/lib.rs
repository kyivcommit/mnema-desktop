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
    /// Bytes to add to the declared `content-length` beyond `body.len()`,
    /// without ever writing them (spec review round 1, item A). Zero for
    /// every constructor except `truncated`, which is the one reply that
    /// lies about its own length on purpose — the real wire shape of a
    /// connection that stops mid-transfer.
    declared_extra: usize,
}

impl Reply {
    pub fn ok(body: &str) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
            delay: Duration::ZERO,
            declared_extra: 0,
        }
    }
    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            delay: Duration::ZERO,
            declared_extra: 0,
        }
    }
    pub fn slow(seconds: u64) -> Self {
        Self {
            status: 200,
            body: "{}".into(),
            delay: Duration::from_secs(seconds),
            declared_extra: 0,
        }
    }

    /// A `200` whose `content-length` promises more bytes than the connection
    /// ever sends before closing — the real wire shape of a response cut off
    /// mid-transfer (spec review round 1, item A). This is not the same thing
    /// as a `body` that merely fails to parse as JSON: that shape is fully
    /// received and only wrong in content, while this one is never fully
    /// received at all, and a client reading a length-delimited body errors
    /// out on the read rather than returning the partial bytes (see
    /// `mnema_provider::Error::BodyUnreadable`).
    pub fn truncated(body: &str) -> Self {
        Self::truncated_status(200, body)
    }

    /// Same wire shape as `truncated` — a `content-length` that promises more
    /// bytes than the connection ever sends — but at a status other than 200.
    /// `mnema-provider`'s `check_key` needs to prove that a body cut off on a
    /// non-200 reply still gives the verdict its status implies
    /// (`Error::BodyUnreadable { status, .. }` carries the status precisely so
    /// a caller like that one can use it instead of losing it), and nothing
    /// before this could produce that shape at any status but 200.
    pub fn truncated_status(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            delay: Duration::ZERO,
            declared_extra: 64,
        }
    }
}

pub struct MockServer {
    base: String,
    seen: Receiver<String>,
}

impl MockServer {
    /// Answers `replies` in order, one per connection. A request past the end
    /// of `replies` does not hang and does not go unanswered (Task 2 review
    /// round 1): `listener.incoming().zip(replies)` used to simply stop
    /// accepting once `replies` ran out, which left a test that accidentally
    /// made one call too many to either hang or pass for a reason that had
    /// nothing to do with what it claimed to test. Past the end, the server
    /// answers with a status no real provider or proxy sends and a body that
    /// names the mistake, so that test fails loudly instead.
    ///
    /// The loop itself never ends (Task 2 review round 3, Minor, reversing
    /// round 2's `break`): ending it after the first surplus request meant a
    /// *second* surplus request got a connection refusal instead of the
    /// sentinel — `Error::Transport`, precisely the shape the sentinel exists
    /// to keep a test from mistaking for something else. Every surplus
    /// request past the first is still answered with `599` for as long as
    /// the server lives. What round 2 was really protecting — `request()`
    /// failing fast when there is nothing left to report — comes from
    /// dropping the sender instead: `tx` moves into an `Option` and is taken
    /// once, right after the first surplus request is reported, so `seen`
    /// disconnects and a later `request()` call still fails immediately
    /// rather than waiting out the full 10 s `recv_timeout`.
    pub fn new(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a port");
        let port = listener.local_addr().expect("read the port").port();
        let (tx, seen): (Sender<String>, Receiver<String>) = channel();

        thread::spawn(move || {
            let mut replies = replies.into_iter();
            let mut tx = Some(tx);
            for (index, stream) in listener.incoming().enumerate() {
                let mut stream = stream.expect("accept");
                let request = read_request(&mut stream);
                if let Some(sender) = &tx {
                    let _ = sender.send(request);
                }
                match replies.next() {
                    Some(reply) => {
                        thread::sleep(reply.delay);
                        write_reply(&mut stream, reply.status, &reply.body, reply.declared_extra);
                    }
                    None => {
                        write_reply(
                            &mut stream,
                            599,
                            &format!(
                                "mnema-mock-provider: request #{index} arrived with no Reply \
                                 configured for it — the test sent more requests than it \
                                 prepared replies for"
                            ),
                            0,
                        );
                        // Only the first surplus request is reported on
                        // `seen` — see the doc comment above.
                        tx = None;
                    }
                }
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

    /// The next request if one has **already** arrived, and `None` if none has.
    ///
    /// For the opposite claim from [`MockServer::request`]'s: that a call was
    /// never made at all, which no amount of waiting can establish — so this
    /// one does not wait.
    ///
    /// It is sound only about a client that has already returned, and that is
    /// the whole of its contract. The order on this side is fixed: the request
    /// goes onto the channel *before* the reply is written, so any request the
    /// client got an answer to is already queued here by the time the call
    /// under test returns. A `None` from a client still in flight would mean
    /// nothing.
    pub fn request_if_any(&self) -> Option<String> {
        self.seen.try_recv().ok()
    }
}

/// Writes a status line, headers and `body`, declaring `content-length` as
/// `body.len() + declared_extra` — but only ever writing `body`. When
/// `declared_extra` is nonzero, the promised bytes never arrive and the
/// connection simply closes when this function returns, which is what a
/// response cut off mid-transfer looks like on the wire (spec review round 1,
/// item A).
fn write_reply(stream: &mut TcpStream, status: u16, body: &str, declared_extra: usize) {
    let head = format!(
        "HTTP/1.1 {status} MOCK\r\ncontent-type: application/json\r\n\
         content-length: {}\r\nconnection: close\r\n\r\n",
        body.len() + declared_extra
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
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
