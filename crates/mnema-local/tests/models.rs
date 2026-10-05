//! Model files against `mnema-mock-provider`: strings for bodies, one reply per
//! connection in download order. Every outcome is checked with an assertion
//! (never an `unwrap` on the thing under test), so a broken implementation
//! fails on a message rather than on a panic.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mnema_local::{Error, FileSpec, Manifest, ModelId, ModelSpec, ModelState, Store};
use mnema_mock_provider::{MockServer, Reply};
use sha2::{Digest, Sha256};

const CONFIG: &[u8] = include_bytes!("../assets/gemma-config.json");

fn big() -> String {
    (0..65_536)
        .map(|i| (b'a' + (i % 26) as u8) as char)
        .collect()
}
fn small() -> String {
    (0..1024).map(|i| (b'A' + (i % 26) as u8) as char).collect()
}
fn sha(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn manifest() -> Manifest {
    let files = |extra: Option<FileSpec>| {
        let mut v = vec![
            FileSpec::new("big.bin", 65_536, &sha(big().as_bytes())),
            FileSpec::new("small.txt", 1024, &sha(small().as_bytes())),
        ];
        v.extend(extra);
        v
    };
    let spec = |repo: &str, files| ModelSpec {
        repo: repo.into(),
        commit: "c0ffee".into(),
        files,
    };
    Manifest {
        embed: spec("org/embed", files(None)),
        chat: spec(
            "org/chat",
            files(Some(FileSpec::bundled("config.json", CONFIG, &sha(CONFIG)))),
        ),
    }
}

fn store(dir: &Path, server: &MockServer) -> Store {
    Store::new(dir.to_path_buf(), server.base().to_string())
        .with_manifest(manifest())
        .with_free_space_probe(|_| u64::MAX)
        .with_transfer(16_384, Duration::from_secs(60))
}

fn quiet(_: u64, _: u64) {}

fn dl(s: &Store, id: ModelId) -> Result<(), Error> {
    s.download(id, &quiet, &AtomicBool::new(false))
}

fn dl_ok(s: &Store, id: ModelId) {
    let r = dl(s, id);
    assert!(r.is_ok(), "the download should succeed: {r:?}");
}

/// Empty when unreadable, so a missing file is a failed comparison.
fn read(p: impl AsRef<Path>) -> Vec<u8> {
    fs::read(p).unwrap_or_default()
}

#[test]
fn a_fresh_download_is_ready_and_byte_identical() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![Reply::ok(&big()), Reply::ok(&small())]);
    let s = store(t.path(), &server);
    dl_ok(&s, ModelId::Embed);
    assert_eq!(s.state(ModelId::Embed), ModelState::Ready);
    let d = s.dir(ModelId::Embed);
    assert_eq!(read(d.join("big.bin")), big().as_bytes());
    assert_eq!(read(d.join("small.txt")), small().as_bytes());
    assert!(!d.join("big.bin.part").exists() && !d.join("small.txt.part").exists());
    assert!(d.ends_with("embed@c0ffee"));
}

#[test]
fn resume_continues_byte_for_byte() {
    let t = tempfile::tempdir().unwrap();
    let b = big();
    let server = MockServer::new(vec![
        Reply::truncated(&b[..32_768]),
        Reply::status(206, &b[32_768..]),
        Reply::ok(&small()),
    ]);
    let s = store(t.path(), &server);
    let e = dl(&s, ModelId::Embed);
    assert!(matches!(e, Err(Error::Http(_))), "{e:?}");
    let first = server.request().to_ascii_lowercase();
    assert!(!first.contains("range:"), "{first}");
    dl_ok(&s, ModelId::Embed);
    let second = server.request().to_ascii_lowercase();
    assert!(second.contains("range: bytes=32768-"), "{second}");
    assert_eq!(read(s.dir(ModelId::Embed).join("big.bin")), b.as_bytes());
    assert_eq!(s.state(ModelId::Embed), ModelState::Ready);
}

#[test]
fn a_200_to_a_range_request_restarts_the_file() {
    let t = tempfile::tempdir().unwrap();
    let b = big();
    let server = MockServer::new(vec![
        Reply::truncated(&b[..32_768]),
        Reply::ok(&b),
        Reply::ok(&small()),
    ]);
    let s = store(t.path(), &server);
    let e = dl(&s, ModelId::Embed);
    assert!(matches!(e, Err(Error::Http(_))), "{e:?}");
    // A 200 appended to the kept 32 KB would be 96 KB: rejected, not Ready.
    dl_ok(&s, ModelId::Embed);
    assert_eq!(read(s.dir(ModelId::Embed).join("big.bin")).len(), 65_536);
    assert_eq!(s.state(ModelId::Embed), ModelState::Ready);
}

#[test]
fn a_sha_mismatch_is_never_ready() {
    let t = tempfile::tempdir().unwrap();
    let mut bad = big().into_bytes();
    bad[100] = b'#';
    let server = MockServer::new(vec![Reply::ok(&String::from_utf8(bad).unwrap())]);
    let s = store(t.path(), &server);
    let r = dl(&s, ModelId::Embed);
    assert!(
        matches!(&r, Err(Error::Checksum { file }) if file == "big.bin"),
        "{r:?}"
    );
    let d = s.dir(ModelId::Embed);
    assert!(!d.join("big.bin.part").exists(), "the bad part must go");
    assert!(!d.join("big.bin").exists());
    let st = s.state(ModelId::Embed);
    assert!(matches!(st, ModelState::Failed(_)), "{st:?}");
    let fresh = store(t.path(), &server);
    assert_ne!(fresh.state(ModelId::Embed), ModelState::Ready);
}

#[test]
fn a_part_file_after_restart_is_not_a_model() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![]);
    let s = store(t.path(), &server);
    let d = s.dir(ModelId::Embed);
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("big.bin.part"), big()).unwrap();
    fs::write(d.join("small.txt"), small()).unwrap();
    assert_ne!(
        store(t.path(), &server).state(ModelId::Embed),
        ModelState::Ready
    );
}

#[test]
fn our_config_replaces_upstreams() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![Reply::ok(&big()), Reply::ok(&small())]);
    let s = store(t.path(), &server);
    dl_ok(&s, ModelId::Chat);
    assert_eq!(read(s.dir(ModelId::Chat).join("config.json")), CONFIG);
    assert_eq!(s.state(ModelId::Chat), ModelState::Ready);
    let a = server.request();
    let b = server.request();
    assert!(!a.contains("config.json") && !b.contains("config.json"));
    assert!(server.request_if_any().is_none());
}

#[test]
fn the_pinned_config_hash_is_the_assets() {
    let m = Manifest::pinned();
    let f = m
        .chat
        .files
        .iter()
        .find(|f| f.name == "config.json")
        .unwrap();
    assert_eq!(f.sha256, sha(CONFIG));
    assert_eq!(f.size, CONFIG.len() as u64);
}

#[test]
fn remove_deletes_the_directory_and_state_is_absent() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![Reply::ok(&big()), Reply::ok(&small())]);
    let s = store(t.path(), &server);
    dl_ok(&s, ModelId::Embed);
    s.remove(ModelId::Embed).unwrap();
    assert!(!s.dir(ModelId::Embed).exists());
    assert_eq!(s.state(ModelId::Embed), ModelState::Absent);
}

#[test]
fn no_space_writes_nothing() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![]);
    let s = store(t.path(), &server).with_free_space_probe(|_| 1_048_576);
    match dl(&s, ModelId::Embed) {
        Err(Error::NoSpace { needed, free }) => {
            assert_eq!(free, 1_048_576);
            assert!(needed > 512 * 1024 * 1024);
        }
        r => panic!("expected NoSpace, got {r:?}"),
    }
    assert!(!s.dir(ModelId::Embed).exists());
    assert_eq!(s.state(ModelId::Embed), ModelState::Absent);
    assert!(server.request_if_any().is_none());
}

#[test]
fn a_finished_model_needs_no_free_space() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![Reply::ok(&big()), Reply::ok(&small())]);
    let s = store(t.path(), &server);
    dl_ok(&s, ModelId::Embed);
    let tight = store(t.path(), &server).with_free_space_probe(|_| 0);
    dl_ok(&tight, ModelId::Embed);
    assert_eq!(tight.state(ModelId::Embed), ModelState::Ready);
}

#[test]
fn a_body_longer_than_pinned_stops_and_is_not_ready() {
    let t = tempfile::tempdir().unwrap();
    let longer = format!("{}{}", big(), "z".repeat(40_000));
    let server = MockServer::new(vec![Reply::ok(&longer)]);
    let s = store(t.path(), &server);
    let r = dl(&s, ModelId::Embed);
    assert!(matches!(&r, Err(Error::Checksum { .. })), "{r:?}");
    let d = s.dir(ModelId::Embed);
    assert!(!d.join("big.bin.part").exists() && !d.join("big.bin").exists());
}

#[cfg(unix)]
#[test]
fn a_write_error_mid_download_keeps_the_part() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root ignores file permissions");
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![Reply::ok(&big())]);
    let s = store(t.path(), &server);
    let part = s.dir(ModelId::Embed).join("big.bin.part");
    let locked = std::cell::Cell::new(false);
    let lock = |done: u64, _| {
        if done > 0 && !locked.replace(true) {
            fs::set_permissions(&part, fs::Permissions::from_mode(0o444)).unwrap();
        }
    };
    let r = s.download(ModelId::Embed, &lock, &AtomicBool::new(false));
    assert!(
        matches!(&r, Err(Error::Io(m)) if m.to_lowercase().contains("permission")),
        "{r:?}"
    );
    assert!(part.exists());
    assert_ne!(s.state(ModelId::Embed), ModelState::Ready);
    let kept = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    assert!(kept > 0 && kept < 65_536, "{kept}");
    fs::set_permissions(&part, fs::Permissions::from_mode(0o644)).unwrap();
    let b = big();
    let server2 = MockServer::new(vec![
        Reply::status(206, &b[kept as usize..]),
        Reply::ok(&small()),
    ]);
    let s2 = store(t.path(), &server2);
    dl_ok(&s2, ModelId::Embed);
    assert_eq!(s2.state(ModelId::Embed), ModelState::Ready);
    assert_eq!(read(s2.dir(ModelId::Embed).join("big.bin")), b.as_bytes());
}

#[test]
fn cancel_stops_and_keeps_the_part() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![Reply::ok(&big())]);
    let s = store(t.path(), &server);
    let cancel = AtomicBool::new(false);
    let stop = |done: u64, _| {
        if done >= 16_384 {
            cancel.store(true, Ordering::Relaxed);
        }
    };
    let r = s.download(ModelId::Embed, &stop, &cancel);
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
    assert!(s.dir(ModelId::Embed).join("big.bin.part").exists());
    assert_eq!(s.state(ModelId::Embed), ModelState::Absent);
}

// A reply that sends 20 000 bytes of the body and then goes silent for 6 s.
fn stalled() -> Reply {
    Reply::stalled(6, &big()[..20_000])
}

#[test]
fn a_silent_body_times_out_and_keeps_the_part() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![stalled()]);
    let s = store(t.path(), &server).with_transfer(16_384, Duration::from_millis(800));
    let started = Instant::now();
    let r = dl(&s, ModelId::Embed);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert!(
        matches!(&r, Err(Error::Http(m)) if m.contains("no data")),
        "{r:?}"
    );
    let kept = fs::metadata(s.dir(ModelId::Embed).join("big.bin.part"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert!(kept > 0, "the bytes that arrived are kept");
    assert_ne!(s.state(ModelId::Embed), ModelState::Ready);
}

#[test]
fn cancel_works_on_a_silent_body() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![stalled()]);
    let s = store(t.path(), &server);
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    let r = std::thread::scope(|sc| {
        sc.spawn(|| {
            std::thread::sleep(Duration::from_millis(500));
            cancel.store(true, Ordering::Relaxed);
        });
        s.download(ModelId::Embed, &quiet, &cancel)
    });
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert!(s.dir(ModelId::Embed).join("big.bin.part").exists());
}
