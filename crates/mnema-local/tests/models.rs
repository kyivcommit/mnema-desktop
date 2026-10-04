//! Model files against `mnema-mock-provider`: strings for bodies, one reply per
//! connection in download order.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

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
fn sha(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn manifest() -> Manifest {
    let files = |extra: Option<FileSpec>| {
        let mut v = vec![
            FileSpec::new("big.bin", 65_536, &sha(&big())),
            FileSpec::new("small.txt", 1024, &sha(&small())),
        ];
        v.extend(extra);
        v
    };
    let spec = |repo: &str, files| ModelSpec {
        repo: repo.into(),
        commit: "c0ffee".into(),
        files,
    };
    let cfg_sha: String = Sha256::digest(CONFIG)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Manifest {
        embed: spec("org/embed", files(None)),
        chat: spec(
            "org/chat",
            files(Some(FileSpec::bundled("config.json", CONFIG, &cfg_sha))),
        ),
    }
}

fn store(dir: &Path, server: &MockServer) -> Store {
    Store::new(dir.to_path_buf(), server.base().to_string())
        .with_manifest(manifest())
        .with_free_space_probe(|_| u64::MAX)
}

fn quiet(_: u64, _: u64) {}

fn dl(s: &Store, id: ModelId) -> Result<(), Error> {
    s.download(id, &quiet, &AtomicBool::new(false))
}

#[test]
fn a_fresh_download_is_ready_and_byte_identical() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![Reply::ok(&big()), Reply::ok(&small())]);
    let s = store(t.path(), &server);
    dl(&s, ModelId::Embed).unwrap();
    assert_eq!(s.state(ModelId::Embed), ModelState::Ready);
    let d = s.dir(ModelId::Embed);
    assert_eq!(fs::read_to_string(d.join("big.bin")).unwrap(), big());
    assert_eq!(fs::read_to_string(d.join("small.txt")).unwrap(), small());
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
    assert!(dl(&s, ModelId::Embed).is_err());
    let first = server.request();
    assert!(!first.to_ascii_lowercase().contains("range:"));
    dl(&s, ModelId::Embed).unwrap();
    let second = server.request().to_ascii_lowercase();
    assert!(second.contains("range: bytes=32768-"), "{second}");
    assert_eq!(
        fs::read_to_string(s.dir(ModelId::Embed).join("big.bin")).unwrap(),
        b
    );
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
    assert!(dl(&s, ModelId::Embed).is_err());
    dl(&s, ModelId::Embed).unwrap();
    let got = fs::read(s.dir(ModelId::Embed).join("big.bin")).unwrap();
    assert_eq!(got.len(), 65_536);
    assert_eq!(s.state(ModelId::Embed), ModelState::Ready);
}

#[test]
fn a_sha_mismatch_is_never_ready() {
    let t = tempfile::tempdir().unwrap();
    let mut bad = big().into_bytes();
    bad[100] = b'#';
    let server = MockServer::new(vec![Reply::ok(&String::from_utf8(bad).unwrap())]);
    let s = store(t.path(), &server);
    let err = dl(&s, ModelId::Embed).unwrap_err();
    assert!(
        matches!(&err, Error::Checksum { file } if file == "big.bin"),
        "{err:?}"
    );
    let d = s.dir(ModelId::Embed);
    assert!(!d.join("big.bin.part").exists() && !d.join("big.bin").exists());
    assert!(matches!(s.state(ModelId::Embed), ModelState::Failed(_)));
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
    dl(&s, ModelId::Chat).unwrap();
    assert_eq!(
        fs::read(s.dir(ModelId::Chat).join("config.json")).unwrap(),
        CONFIG
    );
    assert_eq!(s.state(ModelId::Chat), ModelState::Ready);
    let a = server.request();
    let b = server.request();
    assert!(!a.contains("config.json") && !b.contains("config.json"));
    assert!(server.request_if_any().is_none());
}

#[test]
fn remove_deletes_the_directory_and_state_is_absent() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![Reply::ok(&big()), Reply::ok(&small())]);
    let s = store(t.path(), &server);
    dl(&s, ModelId::Embed).unwrap();
    s.remove(ModelId::Embed).unwrap();
    assert!(!s.dir(ModelId::Embed).exists());
    assert_eq!(s.state(ModelId::Embed), ModelState::Absent);
}

#[test]
fn no_space_writes_nothing() {
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![]);
    let s = store(t.path(), &server).with_free_space_probe(|_| 1_048_576);
    let err = dl(&s, ModelId::Embed).unwrap_err();
    match err {
        Error::NoSpace { needed, free } => {
            assert_eq!(free, 1_048_576);
            assert!(needed > 512 * 1024 * 1024);
        }
        e => panic!("{e:?}"),
    }
    assert!(!s.dir(ModelId::Embed).exists());
    assert_eq!(s.state(ModelId::Embed), ModelState::Absent);
    assert!(server.request_if_any().is_none());
}

#[cfg(unix)]
#[test]
fn a_write_error_mid_download_keeps_the_part() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let server = MockServer::new(vec![
        Reply::ok(&big()),
        Reply::status(206, &big()[16_384..]),
        Reply::ok(&small()),
    ]);
    let s = store(t.path(), &server);
    let part = s.dir(ModelId::Embed).join("big.bin.part");
    let locked = std::cell::Cell::new(false);
    let lock = |done: u64, _| {
        if done > 0 && !locked.replace(true) {
            fs::set_permissions(&part, fs::Permissions::from_mode(0o444)).unwrap();
        }
    };
    let err = s
        .download(ModelId::Embed, &lock, &AtomicBool::new(false))
        .unwrap_err();
    assert!(matches!(&err, Error::Io(m) if !m.is_empty()), "{err:?}");
    assert!(part.exists());
    assert_ne!(s.state(ModelId::Embed), ModelState::Ready);
    let kept = fs::metadata(&part).unwrap().len();
    assert!(kept > 0 && kept < 65_536, "{kept}");
    fs::set_permissions(&part, fs::Permissions::from_mode(0o644)).unwrap();
    // The server will be asked to continue from wherever the part stopped.
    let b = big();
    let server2 = MockServer::new(vec![
        Reply::status(206, &b[kept as usize..]),
        Reply::ok(&small()),
    ]);
    let s2 = store(t.path(), &server2);
    dl(&s2, ModelId::Embed).unwrap();
    assert_eq!(s2.state(ModelId::Embed), ModelState::Ready);
    assert_eq!(
        fs::read_to_string(s2.dir(ModelId::Embed).join("big.bin")).unwrap(),
        b
    );
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
    let err = s.download(ModelId::Embed, &stop, &cancel).unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert!(s.dir(ModelId::Embed).join("big.bin.part").exists());
    assert_eq!(s.state(ModelId::Embed), ModelState::Absent);
}
