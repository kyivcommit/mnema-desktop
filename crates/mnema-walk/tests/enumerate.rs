use mnema_walk::{WalkRules, enumerate};
use std::fs;

/// The order is not cosmetic. `document.id` is the sha256 of the bytes, so the
/// same bytes under `notes.txt` and `notes.md` are ONE document with two
/// readings, and the reading that wins is the one the walk reached first
/// (D41's recorded hole). Sorting does not make the winner right; it makes it
/// the same on two machines, which is what an index has to be.
#[test]
fn the_walk_is_ordered_by_relative_path() {
    let root = tempfile::tempdir().unwrap();
    for name in ["zeta.txt", "alpha.txt", "middle/inner.txt", "middle.txt"] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }

    let walked = enumerate(root.path(), &WalkRules::none());
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();

    assert_eq!(
        names,
        ["alpha.txt", "middle.txt", "middle/inner.txt", "zeta.txt"]
    );
}

/// The root itself is not a found file, and a directory is never one either.
#[test]
fn only_files_are_found() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("sub")).unwrap();
    fs::write(root.path().join("sub/a.txt"), b"x").unwrap();

    let walked = enumerate(root.path(), &WalkRules::none());

    assert_eq!(walked.found.len(), 1);
    assert_eq!(walked.found[0].relative, "sub/a.txt");
}

/// Size and mtime come from the walk and from nowhere else: they are what the
/// cheap arm compares, and a second `stat` inside `ingest_file` would be a
/// second reading of a file that can change between the two (§5).
#[test]
fn every_found_file_carries_its_size_and_mtime() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.txt"), b"twelve bytes").unwrap();

    let walked = enumerate(root.path(), &WalkRules::none());

    assert_eq!(walked.found[0].on_disk.size_bytes, 12);
    assert!(walked.found[0].on_disk.mtime != 0);
}
