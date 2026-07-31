use mnema_walk::{PreSkipRule, WalkRules, enumerate};
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

/// A directory the walk cannot read takes its whole subtree down with it —
/// nothing under `locked/` becomes `found`, and by itself that looks
/// exactly like an empty or a deleted directory. `complete` is what tells a
/// caller the difference, and `skipped` must carry the path or the loss is
/// silent (fix round 1, Critical finding).
#[test]
#[cfg(unix)]
fn an_unreadable_directory_marks_the_walk_incomplete() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let locked = root.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("secret.txt"), b"x").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    // Root reads through any permission bits, which would make this test
    // pass for the wrong reason (or not at all, depending on the platform).
    let root_can_still_read = fs::read_dir(&locked).is_ok();
    if !root_can_still_read {
        let walked = enumerate(root.path(), &WalkRules::none());

        assert!(!walked.complete);
        assert!(walked.found.is_empty());
        assert!(
            walked.skipped.iter().any(|s| {
                s.rule == PreSkipRule::Unreadable && s.display_path.ends_with("locked")
            })
        );
    }

    // Restore permissions unconditionally so `tempdir`'s Drop can clean up.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    if root_can_still_read {
        eprintln!(
            "skipped an_unreadable_directory_marks_the_walk_incomplete: \
             running as root, chmod 000 has no effect"
        );
    }
}

/// `WalkRules::none()` must mean no rules at all. The `ignore` crate's
/// `ignore` option defaults to true regardless — a `.ignore` file inside the
/// watched root would otherwise remove files it names even with every rule
/// layer this crate builds turned off (fix round 1, Important finding).
#[test]
fn an_ignore_file_inside_the_root_does_not_remove_a_file_it_names() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".ignore"), b"secret.txt\n").unwrap();
    fs::write(root.path().join("secret.txt"), b"x").unwrap();

    let walked = enumerate(root.path(), &WalkRules::none());
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();

    assert!(names.contains(&"secret.txt"));
}

/// The `ignore` crate's `parents` option defaults to true and climbs above
/// the walked root looking for more `.ignore`/`.gitignore` files to apply.
/// Measured by the reviewer: this repository has exactly such a file above
/// `crates/`, so left at the default, `WalkRules::none()` would still remove
/// files with `unreadable == 0` — invisibly, from a user's point of view
/// inside their own folder (fix round 1, Important finding).
#[test]
fn an_ignore_file_above_the_root_has_no_effect() {
    let parent = tempfile::tempdir().unwrap();
    fs::write(parent.path().join(".ignore"), b"target.txt\n").unwrap();
    let root = parent.path().join("watched");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("target.txt"), b"x").unwrap();

    let walked = enumerate(&root, &WalkRules::none());

    assert_eq!(walked.found.len(), 1);
    assert_eq!(walked.found[0].relative, "target.txt");
}
