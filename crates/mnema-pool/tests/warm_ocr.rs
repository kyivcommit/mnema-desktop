//! `warm_ocr`, the one-shot warm-up call, against shell stand-ins for the
//! worker's `--warm-ocr` branch: the argument it passes, and how it ends wrongly.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mnema_pool::{PoolError, warm_ocr};

/// Writes an executable script into `dir` and returns its path. Linux refuses
/// to run a script another thread is still forking past (ETXTBSY) for a moment
/// after it was written, so the file is run once before it is handed out.
fn stand_in(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("worker");
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let began = Instant::now();
    loop {
        match std::process::Command::new(&path)
            .arg("--probe")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Ok(_) => return path,
            Err(e)
                if e.kind() == std::io::ErrorKind::ExecutableFileBusy
                    && began.elapsed() < Duration::from_secs(5) =>
            {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(e) => panic!("the stand-in will not run: {e}"),
        }
    }
}

const SECOND: Duration = Duration::from_secs(5);

#[test]
fn the_worker_is_started_with_the_warm_ocr_argument_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("args");
    let worker = stand_in(
        dir.path(),
        &format!(
            "[ \"$1\" = --probe ] && exit 0\necho \"$# $*\" > '{}'\n",
            log.display()
        ),
    );

    warm_ocr(&worker, SECOND).unwrap();

    assert_eq!(std::fs::read_to_string(&log).unwrap(), "1 --warm-ocr\n");
}

#[test]
fn a_worker_that_outlives_the_deadline_is_killed_and_gone() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let worker = stand_in(
        dir.path(),
        &format!(
            "[ \"$1\" = --probe ] && exit 0\necho $$ > '{}'\nexec sleep 30\n",
            pid_file.display()
        ),
    );

    let began = Instant::now();
    let error = warm_ocr(&worker, Duration::from_millis(500)).unwrap_err();

    assert!(matches!(error, PoolError::WarmTimeout { .. }), "{error:?}");
    assert!(began.elapsed() < SECOND, "{:?}", began.elapsed());
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // Reaped by the call, so the pid names nothing: signal 0 finds no process.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    assert!(!alive, "worker {pid} is still there after the error");
}

#[test]
fn a_worker_that_exits_nonzero_is_an_error_with_what_it_said() {
    let dir = tempfile::tempdir().unwrap();
    let worker = stand_in(
        dir.path(),
        "[ \"$1\" = --probe ] && exit 0\necho vision exploded >&2\nexit 3\n",
    );

    let error = warm_ocr(&worker, SECOND).unwrap_err();

    let PoolError::Protocol { line, detail } = &error else {
        panic!("{error:?}");
    };
    assert!(line.contains("vision exploded"), "{line}");
    assert!(detail.contains("--warm-ocr"), "{detail}");
}

#[test]
fn a_worker_that_is_not_there_is_a_spawn_error() {
    let error = warm_ocr(Path::new("/nonexistent/worker"), SECOND).unwrap_err();
    assert!(matches!(error, PoolError::Spawn { .. }), "{error:?}");
}
