//! The walk is the single place that stats a file (§5). This is the test that
//! states it structurally: `ingest_file` must compare the metadata it is
//! handed, not metadata it takes for itself.
//!
//! One fixture, one test — everything else this task touches already has
//! coverage in `slice.rs` and `randomised.rs`. Duplicated `worker()` rather
//! than shared with them for the same reason `randomised.rs` gives: cargo
//! compiles each file under `tests/` as its own binary, so a shared `mod`
//! would have to be introduced into files this task has no business editing
//! beyond their `ingest_file` call sites.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use mnema_index::{Db, open, register_vector_extension};
use mnema_ingest::{Ingested, ingest_file};
use mnema_pool::{Pool, PoolConfig};

fn worker() -> &'static Path {
    static WORKER: OnceLock<PathBuf> = OnceLock::new();
    WORKER.get_or_init(|| {
        let exe = std::env::current_exe().expect("a test binary knows its own path");
        let profile_dir = exe
            .parent()
            .and_then(Path::parent)
            .expect("a test binary sits in <target>/<profile>/deps");
        let target_dir = profile_dir
            .parent()
            .expect("<target>/<profile> sits inside <target>");
        let profile = profile_dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("the profile directory is named");
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("crates/mnema-ingest sits two levels below the workspace root");

        let mut cargo = Command::new(env!("CARGO"));
        cargo
            .args([
                "build",
                "-p",
                "mnema-extract",
                "--bin",
                "mnema-extract-worker",
            ])
            .arg("--manifest-path")
            .arg(workspace.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target_dir);
        if profile != "debug" {
            cargo.args(["--profile", profile]);
        }
        let status = cargo.status().expect("cargo runs");
        assert!(
            status.success(),
            "the extraction worker did not build, so this whole file is unanswered \
             rather than passing"
        );

        let path = profile_dir.join(format!(
            "mnema-extract-worker{}",
            std::env::consts::EXE_SUFFIX
        ));
        assert!(
            path.exists(),
            "cargo reported success but {} is not there",
            path.display()
        );
        path
    })
}

/// A watched root with an index beside it, and a pool over the real worker.
struct Fixture {
    db: Db,
    pool: Pool,
    root_id: i64,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        register_vector_extension().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("watched");
        std::fs::create_dir_all(&root).unwrap();
        let db = open(&dir.path().join("index.sqlite")).unwrap();
        let root_id = db
            .insert_watched_root(root.to_str().expect("a temp path is UTF-8"))
            .unwrap();
        let pool = Pool::new(PoolConfig {
            workers: 1,
            batch: 100,
            ..PoolConfig::new(worker())
        })
        .unwrap();
        Fixture {
            db,
            pool,
            root_id,
            root,
            _dir: dir,
        }
    }

    /// Writes `content` at `relative` inside the watched root and returns the
    /// absolute path.
    fn write(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::write(&path, content.as_bytes()).unwrap();
        path
    }
}

/// The cheap arm must compare the file against the numbers the WALK took, not
/// against numbers this function takes for itself. With two stats there is a
/// window between them in which the file changes, and then the walk counted
/// one size while the arm compared another — a difference that only shows up
/// as an index quietly holding an old version.
///
/// The test states it structurally: handing in metadata that does NOT match
/// the disk must change the decision. If `ingest_file` still stats for
/// itself, the handed-in values are ignored and the file is treated as
/// unchanged.
#[test]
fn the_handed_in_metadata_is_what_the_cheap_arm_compares() {
    let fixture = Fixture::new();
    let path = fixture.write("a.txt", "hello");

    // First pass: index it for real, with honest metadata.
    let honest = mnema_walk::stat(&path).unwrap();
    let first = ingest_file(
        &fixture.pool,
        &fixture.db,
        fixture.root_id,
        &path,
        "a.txt",
        Some(honest),
    )
    .unwrap();
    assert!(matches!(first, Ingested::Indexed { .. }));

    // Second pass: the same untouched file, but the caller reports a
    // different size. The arm must believe the caller and re-read.
    let lying = mnema_walk::OnDisk {
        size_bytes: honest.size_bytes + 1,
        mtime: honest.mtime,
    };
    let second = ingest_file(
        &fixture.pool,
        &fixture.db,
        fixture.root_id,
        &path,
        "a.txt",
        Some(lying),
    )
    .unwrap();
    assert!(
        !matches!(second, Ingested::Unchanged { .. }),
        "ingest_file is still statting for itself"
    );
}
