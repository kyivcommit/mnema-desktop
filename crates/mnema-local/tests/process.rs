//! The supervisor against `fake-mlx`, a real process. Unix only: `kill`/`ESRCH`
//! have no Windows form, and CI's `check` job runs Windows.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use mnema_local::{Error, Sidecar};

const FAKE: &str = env!("CARGO_BIN_EXE_mnema-local-fake-mlx");

fn start(env: &[(&str, &str)]) -> Sidecar {
    Sidecar::start_with_env(Path::new(FAKE), Path::new("/e"), Path::new("/c"), env).unwrap()
}

fn status(url: &str, token: &str) -> u16 {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into();
    agent
        .get(url)
        .header("authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .status()
        .as_u16()
}

#[test]
fn start_reads_port_and_token() {
    let s = start(&[]);
    let e = s.endpoint();
    assert!(e.is_ok(), "endpoint: {e:?}");
    let e = e.unwrap();
    assert!(e.base.starts_with("http://127.0.0.1:"), "{}", e.base);
    let port: u16 = e.base["http://127.0.0.1:".len()..]
        .trim_end_matches("/v1")
        .parse()
        .unwrap();
    assert_ne!(port, 0);
    assert!(e.token.len() >= 32);
    assert_eq!(status(&format!("{}/models", e.base), &e.token), 200);
}

/// The fake exits right after answering; wait until it has (without reaping it,
/// which is the supervisor's job): `waitid(WNOWAIT)` sees a zombie.
fn let_it_die(pid: u32) {
    for _ in 0..100 {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let r = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if r == 0 && unsafe { info.si_pid() } != 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("pid {pid} never exited");
}

#[test]
fn a_dead_process_is_restarted_once_with_a_new_token_and_port() {
    let s = start(&[("FAKE_MLX_DIE_AFTER", "1")]);
    let old = s.endpoint().unwrap();
    let old_pid = s.pid();
    assert_eq!(status(&format!("{}/models", old.base), &old.token), 200);
    let_it_die(old_pid);

    let new = s.endpoint().unwrap();
    assert_ne!(new.token, old.token);
    assert_ne!(s.pid(), old_pid);
    // The old token against the new port: refused (and not counted by the fake).
    assert_eq!(status(&format!("{}/models", new.base), &old.token), 401);
    assert_eq!(status(&format!("{}/models", new.base), &new.token), 200);
}

fn spawns(log: &Path) -> usize {
    std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .filter(|l| l.starts_with("spawn "))
        .count()
}

#[test]
fn a_second_death_is_an_error_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let log: PathBuf = dir.path().join("fake.log");
    let s = start(&[
        ("FAKE_MLX_DIE_AFTER", "1"),
        ("FAKE_MLX_LOG", log.to_str().unwrap()),
    ]);
    for _ in 0..2 {
        let e = s.endpoint().unwrap();
        assert_eq!(status(&format!("{}/models", e.base), &e.token), 200);
        let_it_die(s.pid());
    }
    for _ in 0..2 {
        match s.endpoint() {
            Err(Error::Crashed { stderr_tail }) => {
                // Both deaths' reasons, not only the last one.
                assert_eq!(
                    stderr_tail.matches("fake-mlx: dying").count(),
                    2,
                    "{stderr_tail}"
                )
            }
            other => panic!("expected Crashed, got {other:?}"),
        }
    }
    assert_eq!(spawns(&log), 2);
}

/// `kill(pid, 0) == -1 && errno == ESRCH` within 3 s.
fn gone_within_3s(pid: u32) -> bool {
    for _ in 0..30 {
        let r = unsafe { libc::kill(pid as i32, 0) };
        if r == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn dropping_leaves_no_orphan() {
    // One that leaves on EOF, and one that ignores it and has to be killed.
    for env in [&[][..], &[("FAKE_MLX_IGNORE_STDIN", "1")][..]] {
        let s = start(env);
        let pid = s.pid();
        s.endpoint().unwrap();
        drop(s);
        assert!(
            gone_within_3s(pid),
            "pid {pid} survived the drop (env {env:?})"
        );
    }
}

/// Guard, not a cycle: documents the transport (OS + `ureq`), not this crate's
/// code. A request in flight when the process is killed fails when the socket
/// closes, not when the 60 s local chat limit runs out.
#[test]
fn a_process_dying_mid_request_fails_at_once() {
    use mnema_provider::{Message, MessageRole};
    let s = start(&[("FAKE_MLX_HANG", "1")]);
    let e = s.endpoint().unwrap();
    let pid = s.pid();
    let call = std::thread::spawn(move || {
        let messages = [Message {
            role: MessageRole::User,
            content: "hi".into(),
        }];
        mnema_provider::complete(&e.base, &e.token, "chat", &messages)
    });
    std::thread::sleep(Duration::from_secs(1));
    assert!(
        !call.is_finished(),
        "the call must still be waiting before the kill"
    );
    let killed = std::time::Instant::now();
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    while !call.is_finished() && killed.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        call.is_finished() && killed.elapsed() <= Duration::from_secs(2),
        "took {:?}",
        killed.elapsed()
    );
    assert!(call.join().unwrap().is_err());
}

#[test]
fn load_and_unload_reach_the_root_routes_with_the_right_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("fake.log");
    let s = start(&[("FAKE_MLX_LOG", log.to_str().unwrap())]);
    s.load().unwrap();
    s.unload(None).unwrap();
    s.unload(Some(mnema_local::ModelId::Chat)).unwrap();
    s.unload(Some(mnema_local::ModelId::Embed)).unwrap();
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("end POST /mnema/load"), "{text}");
    assert!(text.lines().any(|l| l == "body /mnema/unload "), "{text}");
    assert!(
        text.contains(r#"body /mnema/unload {"models":["chat"]}"#),
        "{text}"
    );
    assert!(
        text.contains(r#"body /mnema/unload {"models":["embed"]}"#),
        "{text}"
    );
}

/// The sidecar's spawn takes the process-wide guard it shares with `mnema-pool`:
/// while another spawner holds it, `start` waits; once released, it proceeds.
#[test]
fn start_waits_for_the_shared_spawn_guard() {
    let guard = mnema_pool::spawn_guard();
    let t = std::thread::spawn(|| start(&[]));
    std::thread::sleep(Duration::from_millis(700));
    assert!(
        !t.is_finished(),
        "start spawned while the shared guard was held"
    );
    drop(guard);
    let s = t.join().unwrap();
    assert!(s.endpoint().is_ok());
}

#[test]
fn the_process_gets_embed_then_chat_and_logs_paths_as_sent() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("fake.log");
    let s = Sidecar::start_with_env(
        Path::new(FAKE),
        Path::new("/the/embed"),
        Path::new("/the/chat"),
        &[("FAKE_MLX_LOG", log.to_str().unwrap())],
    )
    .unwrap();
    let e = s.endpoint().unwrap();
    assert_eq!(status(&format!("{}/models", e.query_base), &e.token), 200);
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(
        text.lines()
            .any(|l| l.ends_with(" --embed /the/embed --chat /the/chat")),
        "{text}"
    );
    assert!(text.contains("start GET /interactive/v1/models"), "{text}");
    assert!(text.contains("end GET /interactive/v1/models"), "{text}");
}

#[test]
fn debug_never_prints_the_token() {
    let s = start(&[]);
    let e = s.endpoint().unwrap();
    assert!(!format!("{e:?}").contains(&e.token));
    assert!(!format!("{:?}", s.endpoint()).contains(&e.token));
}

#[test]
fn available_is_true_here_only_on_apple_silicon() {
    // The dev machine and CI's macOS leg run Apple Silicon on macOS 14+.
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        assert!(mnema_local::available());
    } else {
        assert!(!mnema_local::available());
    }
}
