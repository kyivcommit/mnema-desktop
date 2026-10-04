//! A stand-in for the MLX sidecar (`sidecar/mnema-mlx`), for the supervisor's
//! tests. Like the real one it refuses to start without `MNEMA_MLX_TOKEN`,
//! prints `PORT <n>` first, answers 401 without the bearer token, and exits when
//! stdin closes. Knobs (env): `FAKE_MLX_DIE_AFTER=<n>` (authorised requests), `FAKE_MLX_HANG=1`,
//! `FAKE_MLX_CHAT_MS`, `FAKE_MLX_LOAD_MS`, `FAKE_MLX_IGNORE_STDIN=1`,
//! `FAKE_MLX_LOG=<file>`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

fn env_num(name: &str) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn log(line: &str) {
    if let Ok(path) = std::env::var("FAKE_MLX_LOG")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}

fn main() {
    let token = std::env::var("MNEMA_MLX_TOKEN").unwrap_or_default();
    if token.is_empty() {
        eprintln!("fake-mlx: MNEMA_MLX_TOKEN is empty");
        std::process::exit(2);
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    println!("PORT {}", listener.local_addr().unwrap().port());
    std::io::stdout().flush().unwrap();
    log(&format!(
        "spawn {} {}",
        std::process::id(),
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    ));

    if std::env::var("FAKE_MLX_IGNORE_STDIN").is_err() {
        std::thread::spawn(|| {
            let mut sink = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut sink);
            std::process::exit(0);
        });
    }

    static SERVED: AtomicU64 = AtomicU64::new(0);
    let hang = std::env::var("FAKE_MLX_HANG").is_ok();
    let mut held = Vec::new();
    for stream in listener.incoming().flatten() {
        if hang {
            held.push(stream); // accept, never answer
            continue;
        }
        let token = token.clone();
        std::thread::spawn(move || {
            // Only authorised requests count, so a 401 probe does not use one up.
            if handle(stream, &token) != 200 {
                return;
            }
            let n = SERVED.fetch_add(1, Ordering::SeqCst) + 1;
            let limit = env_num("FAKE_MLX_DIE_AFTER");
            if limit > 0 && n >= limit {
                eprintln!("fake-mlx: dying");
                std::process::exit(1);
            }
        });
    }
}

fn handle(stream: TcpStream, token: &str) -> u16 {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return 0;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
    );
    let (mut auth, mut len) = (String::new(), 0usize);
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.trim().split_once(':') {
            match k.to_ascii_lowercase().as_str() {
                "authorization" => auth = v.trim().to_string(),
                "content-length" => len = v.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    let mut body = vec![0u8; len];
    let _ = reader.read_exact(&mut body);
    let body = String::from_utf8_lossy(&body).to_string();

    log(&format!("start {method} {path}"));
    // Routing ignores the `/interactive` prefix; the log keeps the path as sent.
    let sent = path.clone();
    let path = path
        .strip_prefix("/interactive")
        .unwrap_or(&path)
        .to_string();
    let (status, answer) = if auth != format!("Bearer {token}") {
        (401, r#"{"error":"unauthorized"}"#.to_string())
    } else {
        match path.as_str() {
            "/v1/models" => (200, r#"{"object":"list","data":[]}"#.to_string()),
            "/v1/embeddings" => {
                let n = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| v["input"].as_array().map(Vec::len))
                    .unwrap_or(0);
                log(&format!("input {n}"));
                (200, r#"{"data":[]}"#.to_string())
            }
            "/v1/chat/completions" => {
                log(&format!("body {sent} {body}"));
                std::thread::sleep(Duration::from_millis(env_num("FAKE_MLX_CHAT_MS")));
                (
                    200,
                    r#"{"choices":[{"message":{"role":"assistant","content":"ok"}}]}"#.to_string(),
                )
            }
            "/mnema/load" => {
                std::thread::sleep(Duration::from_millis(env_num("FAKE_MLX_LOAD_MS")));
                (200, "{}".to_string())
            }
            "/mnema/unload" => {
                log(&format!("body {sent} {body}"));
                (200, "{}".to_string())
            }
            _ => (404, r#"{"error":"not found"}"#.to_string()),
        }
    };
    let head = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        answer.len()
    );
    let mut s = stream;
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(answer.as_bytes());
    let _ = s.flush();
    log(&format!("end {method} {sent}"));
    status
}
