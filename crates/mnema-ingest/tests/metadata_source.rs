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

use mnema_core::OnDisk;
use mnema_core::manifest::Manifest;
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
    /// The real worker's own reader manifest, asked once. Nothing in this file
    /// is about which reader takes a file — it is about which *numbers* the
    /// cheap arm compares — so every call here hands in the manifest of the
    /// binary that is about to answer, which is what makes the reader condition
    /// agree and leaves the two numbers as the only thing that can decide.
    manifest: Manifest,
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
        let manifest = pool.manifest().unwrap();
        Fixture {
            db,
            pool,
            root_id,
            root,
            manifest,
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
        &fixture.manifest,
    )
    .unwrap();
    assert!(matches!(first, Ingested::Indexed { .. }));

    // Second pass: the same untouched file, but the caller reports a
    // different size. The arm must believe the caller and re-read.
    let lying = OnDisk {
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
        &fixture.manifest,
    )
    .unwrap();
    assert!(
        !matches!(second, Ingested::Unchanged { .. }),
        "ingest_file is still statting for itself"
    );
}

/// The cost of trusting the caller, pinned rather than left as prose.
///
/// `mnema_walk`'s `mtime_nanos` saturates at `i64::MAX` instead of refusing
/// past roughly year 2262 (`crates/mnema-walk/src/lib.rs`), which is what
/// makes a file with such a timestamp indexable at all — Task 5 retired the
/// `mnema-ingest` copy that used to return `None` there and fall back to
/// `SkipRule::Unreadable`, so this state is only reachable now. It is real on
/// ext4 (to year 2446) and Windows `FILETIME` (to year 30828), not only in a
/// hand-built `OnDisk`; macOS just cannot produce one to `stat` honestly,
/// which is why this test builds the value rather than writing a file old
/// enough to earn it.
///
/// The cost: once an mtime has saturated, the cheap arm can no longer tell a
/// file that has not moved from one that has moved again beyond the ceiling.
/// A same-length edit made after saturation is invisible for ever — silently,
/// with nothing in the journal, indistinguishable from every other unchanged
/// file. This is not a bug this task introduced; it is the documented price
/// of saturating rather than refusing, made newly reachable by handing
/// `on_disk` in rather than measuring it here. Pinned so the price is a
/// number a future change has to notice breaking, not a claim in a comment.
#[test]
fn a_saturated_mtime_hides_a_same_length_edit_forever() {
    let fixture = Fixture::new();
    let path = fixture.write("a.txt", "hello");

    let saturated = OnDisk {
        size_bytes: 5,
        mtime: i64::MAX,
    };
    let first = ingest_file(
        &fixture.pool,
        &fixture.db,
        fixture.root_id,
        &path,
        "a.txt",
        Some(saturated),
        &fixture.manifest,
    )
    .unwrap();
    assert!(matches!(first, Ingested::Indexed { .. }));
    assert!(!fixture.db.search_lexical("hello", 10).unwrap().is_empty());

    // Same length, different content, same (already saturated) mtime —
    // nothing the cheap arm can see has moved.
    std::fs::write(&path, "world").unwrap();
    let second = ingest_file(
        &fixture.pool,
        &fixture.db,
        fixture.root_id,
        &path,
        "a.txt",
        Some(saturated),
        &fixture.manifest,
    )
    .unwrap();
    assert!(
        matches!(second, Ingested::Unchanged { .. }),
        "expected the cheap arm to stay blind to a same-length edit under an \
         already-saturated mtime, got {second:?}"
    );

    // The index still answers with the OLD text — this is the silent cost,
    // not a side effect to clean up.
    assert!(
        !fixture.db.search_lexical("hello", 10).unwrap().is_empty(),
        "the old text should still be all the index has"
    );
    assert!(
        fixture.db.search_lexical("world", 10).unwrap().is_empty(),
        "the new text reached the index, so the file was read after all — \
         this test's premise is wrong"
    );
}
