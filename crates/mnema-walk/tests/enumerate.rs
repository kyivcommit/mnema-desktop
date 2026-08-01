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
        let skip = walked
            .skipped
            .iter()
            .find(|s| s.rule == PreSkipRule::Unreadable)
            .expect("the locked directory should be recorded");
        // Root-relative, matching `Found::relative`'s form — NOT an
        // absolute path (fix round 2, Important finding).
        assert_eq!(skip.relative.as_deref(), Some("locked"));
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

/// A symlink is neither `is_file` nor `is_dir` once `follow_links(false)`
/// makes `metadata()` an lstat, regardless of what it points at. It must be
/// named, not silently dropped — but not following it is a decision, not a
/// failure, so `complete` stays true (fix round 2, Important finding: every
/// symlink vanished before this, not only ones pointing outside the root).
#[test]
#[cfg(unix)]
fn a_symlink_is_named_but_does_not_mark_the_walk_incomplete() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("target.txt"), b"x").unwrap();
    symlink(root.path().join("target.txt"), root.path().join("link.txt")).unwrap();

    let walked = enumerate(root.path(), &WalkRules::none());

    assert!(walked.complete);
    assert_eq!(walked.found.len(), 1);
    assert_eq!(walked.found[0].relative, "target.txt");
    let skip = walked
        .skipped
        .iter()
        .find(|s| s.rule == PreSkipRule::NotAFile)
        .expect("the symlink should be recorded as NotAFile");
    assert_eq!(skip.relative.as_deref(), Some("link.txt"));
}

/// The follow that tells `NotAFile` apart from `NotAFileSubtree` (`lib.rs`)
/// fails for a dangling symlink, the same way it fails for nothing at all —
/// that failure must read as "not a directory", not as a read error:
/// `unreadable` stays 0 and `complete` stays true. Until this test, only the
/// succeeding-metadata case (a symlink to a live file) exercised the
/// `NotAFile` arm; the failing-metadata case was covered by nothing (fix
/// round 4, Minor finding).
#[test]
#[cfg(unix)]
fn a_dangling_symlink_is_named_but_does_not_mark_the_walk_incomplete() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    symlink(
        root.path().join("does_not_exist.txt"),
        root.path().join("broken.txt"),
    )
    .unwrap();

    let walked = enumerate(root.path(), &WalkRules::none());

    assert!(walked.complete);
    assert_eq!(walked.unreadable, 0);
    assert!(walked.found.is_empty());
    let skip = walked
        .skipped
        .iter()
        .find(|s| s.rule == PreSkipRule::NotAFile)
        .expect("the dangling symlink should be recorded as NotAFile");
    assert_eq!(skip.relative.as_deref(), Some("broken.txt"));
}

/// Nothing in the signature says `root` must be a directory. Without a
/// check, `enumerate` on a plain file returns an empty `found` with
/// `complete` left true — indistinguishable from an empty, real folder
/// (fix round 2, Important finding).
#[test]
fn a_root_that_is_a_file_marks_the_walk_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_folder = dir.path().join("not_a_folder.txt");
    fs::write(&not_a_folder, b"x").unwrap();

    let walked = enumerate(&not_a_folder, &WalkRules::none());

    assert!(!walked.complete);
    assert!(walked.found.is_empty());
    assert_eq!(walked.skipped.len(), 1);
    assert_eq!(walked.skipped[0].rule, PreSkipRule::Unreadable);
    assert!(walked.skipped[0].detail.ends_with("not_a_folder.txt"));
}

/// `entry.metadata()` can fail even when the walker itself successfully
/// lists an entry: a directory with read but not execute permission lets
/// `readdir` return names (it only needs read), but resolving any child's
/// metadata needs the execute (search) bit and fails. This is a different
/// failure site from the walker's own `Err`
/// (`an_unreadable_directory_marks_the_walk_incomplete` exercises that one,
/// via a `chmod 000` directory, which fails to list at all) and nothing
/// covered it — reverting the `metadata()` branch left every other test in
/// this file green.
#[test]
#[cfg(unix)]
fn a_failed_metadata_call_is_recorded_and_marks_the_walk_incomplete() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let noexec = root.path().join("noexec");
    fs::create_dir(&noexec).unwrap();
    fs::write(noexec.join("a.txt"), b"x").unwrap();
    fs::set_permissions(&noexec, fs::Permissions::from_mode(0o400)).unwrap();

    // Root resolves paths through a missing execute bit too.
    let root_can_still_stat = fs::metadata(noexec.join("a.txt")).is_ok();
    if !root_can_still_stat {
        let walked = enumerate(root.path(), &WalkRules::none());

        assert!(!walked.complete);
        assert!(walked.found.is_empty());
        let skip = walked
            .skipped
            .iter()
            .find(|s| s.rule == PreSkipRule::Unreadable)
            .expect("a.txt's failed metadata() call should be recorded");
        assert_eq!(skip.relative.as_deref(), Some("noexec/a.txt"));
    }

    // Restore permissions unconditionally so `tempdir`'s Drop can clean up.
    fs::set_permissions(&noexec, fs::Permissions::from_mode(0o755)).unwrap();
    if root_can_still_stat {
        eprintln!(
            "skipped a_failed_metadata_call_is_recorded_and_marks_the_walk_incomplete: \
             running as root, chmod 0o400 has no effect"
        );
    }
}

/// A `chmod 000` ROOT is a second way to reach "the walk could not read the
/// root" — distinct from `a_root_that_is_a_file_marks_the_walk_incomplete`
/// (that one never gets past `root.is_dir()`) and from
/// `an_unreadable_directory_marks_the_walk_incomplete` (that one is a
/// SUBdirectory, so `relative_of` strips to a real, non-empty key). Here
/// `relative_of(root, root)` strips to nothing: `relative_string` must map
/// the empty component list to `None`, not `Some("")` — a key that could
/// never equal any real `Found::relative` (fix round 3, Important finding).
#[test]
#[cfg(unix)]
fn an_unreadable_root_carries_no_relative_key() {
    use std::os::unix::fs::PermissionsExt;

    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("locked_root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("secret.txt"), b"x").unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();

    let root_can_still_read = fs::read_dir(&root).is_ok();
    if !root_can_still_read {
        let walked = enumerate(&root, &WalkRules::none());

        assert!(!walked.complete);
        assert!(walked.found.is_empty());
        let skip = walked
            .skipped
            .iter()
            .find(|s| s.rule == PreSkipRule::Unreadable)
            .expect("the locked root should be recorded");
        assert_eq!(skip.relative, None);
    }

    // Restore permissions unconditionally so `tempdir`'s Drop can clean up.
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    if root_can_still_read {
        eprintln!(
            "skipped an_unreadable_root_carries_no_relative_key: \
             running as root, chmod 000 has no effect"
        );
    }
}

/// A directory that is a symlink loses its whole subtree in one step — this
/// is the hole `PreSkipRule::NotAFileSubtree` exists to make visible: without
/// it, `docs/one.txt` and `docs/deep/two.txt` exist on disk under the
/// watched root and would appear in neither `found` nor `skipped`, with
/// `complete == true` claiming nothing was missed (fix round 3, Important
/// finding).
#[test]
#[cfg(unix)]
fn a_symlinked_directory_is_named_as_a_subtree_not_a_file() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("kept.txt"), b"x").unwrap();

    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(outside.path().join("deep")).unwrap();
    fs::write(outside.path().join("one.txt"), b"x").unwrap();
    fs::write(outside.path().join("deep/two.txt"), b"x").unwrap();
    symlink(outside.path(), root.path().join("docs")).unwrap();

    let walked = enumerate(root.path(), &WalkRules::none());

    assert!(walked.complete);
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();
    assert_eq!(names, ["kept.txt"]);
    let skip = walked
        .skipped
        .iter()
        .find(|s| s.rule == PreSkipRule::NotAFileSubtree)
        .expect("the symlinked directory should be recorded as a subtree skip");
    assert_eq!(skip.relative.as_deref(), Some("docs"));
}

/// The path is stored exactly as the system gave it, with no normalisation.
/// macOS hands out decomposed names — U+0069 U+0308, not U+00EF — and this
/// is a deliberate exception to D32, which puts NFC normalisation on
/// document CONTENT: there, normalisation is a condition of being found;
/// here it would be a condition of NOT being openable, because a normalised
/// string does not open the file on Linux, where lookup is byte-exact.
///
/// The equality assertion is load-bearing, not decorative: measured on this
/// filesystem, storage keeps the bytes verbatim but lookup is
/// normalisation-insensitive, so `root.join(&found.relative)` would still
/// open the file even if `relative` had been silently normalised to NFC —
/// the reopen checks alone cannot detect a normalising walk. Only comparing
/// the string itself can.
#[test]
fn the_relative_path_the_walk_reports_can_reopen_the_file() {
    let root = tempfile::tempdir().unwrap();
    // U+0069 U+0308 — decomposed "ï", the form macOS hands out.
    let decomposed = "i\u{0308}.txt";
    fs::write(root.path().join(decomposed), b"x").unwrap();

    let walked = enumerate(root.path(), &WalkRules::none());

    assert_eq!(walked.found.len(), 1);
    let found = &walked.found[0];
    assert!(fs::read(&found.absolute).is_ok());
    assert!(fs::read(root.path().join(&found.relative)).is_ok());
    assert_eq!(found.relative, decomposed);
}

/// The root itself can legitimately be a symlink to a directory — this must
/// walk exactly as if it were the real directory. The `entry.depth() == 0`
/// check added to close the two-syscall race (`root.is_dir()`, then the
/// walk) must not mistake this ordinary case for that race: the walker's own
/// `metadata()` for the depth-0 entry is an lstat (`follow_links(false)`)
/// and reports `is_dir() == false` for any symlink, root included, which
/// would wrongly mark every symlinked root incomplete if the depth-0 check
/// used it directly instead of following the symlink the way `root.is_dir()`
/// already does (fix round 3, Minor finding — found while implementing it,
/// not part of the original review).
#[test]
#[cfg(unix)]
fn a_symlinked_root_is_walked_normally() {
    use std::os::unix::fs::symlink;

    let real = tempfile::tempdir().unwrap();
    fs::write(real.path().join("a.txt"), b"x").unwrap();

    let parent = tempfile::tempdir().unwrap();
    let link = parent.path().join("link");
    symlink(real.path(), &link).unwrap();

    let walked = enumerate(&link, &WalkRules::none());

    assert!(walked.complete);
    assert_eq!(walked.found.len(), 1);
    assert_eq!(walked.found[0].relative, "a.txt");
    assert!(walked.skipped.is_empty());
}

// -------------------------------------------------------- filesystem stand
//
// The three tests below need to run on more than one filesystem to say
// anything: a case-sensitive volume and a case-insensitive one behave
// differently by design, not by bug, and Windows enforces a legacy path
// length limit no Unix filesystem has. Each test measures which regime it is
// actually running under — rather than assuming one — and prints what it saw
// (`cargo test -- --nocapture`), so the same source runs meaningfully on
// every machine instead of only being honest on one of them.

/// `path.relative_path` is compared as text, and the filesystem may or may
/// not agree that two spellings are one file. On a case-insensitive volume
/// `Notes.txt` and `notes.txt` are one file the walk must not report twice;
/// on a case-sensitive one they are two documents and must both be found.
///
/// Which regime applies is measured directly off the filesystem
/// (`fs::read_dir`'s own entry count) rather than inferred from whether the
/// second `fs::write` succeeded — it succeeds either way: on a
/// case-insensitive volume it overwrites the first file instead of failing,
/// so success alone cannot tell the two regimes apart. The two branches below
/// each assert a property real enough to fail on a real bug, instead of
/// collapsing behind one guard that asserts nothing on one of the two
/// regimes: on a case-sensitive volume, a bug that reported only one of the
/// two names fails an unconditional `names.len() == 2`; on a
/// case-insensitive one, a bug that kept a stale read of the file before it
/// was overwritten fails the content check.
#[test]
fn case_only_neighbours_are_reported_consistently() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("notes.txt"), b"lower").unwrap();
    fs::write(root.path().join("Notes.txt"), b"upper").unwrap();

    // Two directory entries mean two distinct on-disk files; one means the
    // second write overwrote the first. This is the fact under test, read
    // straight off the filesystem rather than through `enumerate`.
    let case_sensitive = fs::read_dir(root.path()).unwrap().count() == 2;

    let walked = enumerate(root.path(), &WalkRules::none());
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();

    // Whatever the volume decided, the walk must report each distinct
    // relative path exactly once — never the same one twice.
    assert_eq!(
        names.len(),
        walked
            .found
            .iter()
            .map(|f| &f.relative)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        "the walk reported the same relative path more than once: {names:?}"
    );

    if case_sensitive {
        eprintln!(
            "case_only_neighbours_are_reported_consistently: CASE-SENSITIVE volume at {:?} \
             — expecting both spellings",
            root.path()
        );
        assert_eq!(
            names.len(),
            2,
            "a case-sensitive volume must report both notes.txt and Notes.txt: {names:?}"
        );
        assert!(names.contains(&"notes.txt") && names.contains(&"Notes.txt"));
    } else {
        eprintln!(
            "case_only_neighbours_are_reported_consistently: CASE-INSENSITIVE volume at {:?} \
             — expecting one file, freshly overwritten",
            root.path()
        );
        assert_eq!(
            names.len(),
            1,
            "a case-insensitive volume holds one file under two spellings: {names:?}"
        );
        assert_eq!(
            fs::read(&walked.found[0].absolute).unwrap(),
            b"upper",
            "the walk's one entry must reflect the second write, not a stale read of the first"
        );
    }
}

/// Windows enforces a legacy 260-character `MAX_PATH` on Win32 calls unless a
/// caller opts in to long paths. `std::fs` dodges this itself by prefixing
/// `\\?\` internally, but `enumerate` walks through the `ignore` crate, a
/// different caller `std::fs`'s workaround does not automatically cover — so
/// this pins `enumerate` directly rather than assuming std's own behaviour
/// extends to it.
///
/// The property under test is not "does it enumerate" — that answer is
/// allowed to differ by platform and configuration — it is that a file
/// genuinely on disk must land in exactly one of `found` or `skipped`, never
/// neither. A silent third outcome — present on disk, absent from both —
/// is the one defect no counter anywhere would catch: nothing downstream
/// would know the file was ever there. macOS and Linux have no such limit at
/// any length this test can practically construct, so on those platforms the
/// test is still honest: it runs the same three-way check and always lands
/// in the "enumerated" arm, which it says so via `eprintln!` rather than
/// silently short-circuiting.
#[test]
fn a_long_path_is_enumerated_or_refused_with_a_reason_never_silently_dropped() {
    const NAME: &str = "a_long_path_is_enumerated_or_refused_with_a_reason_never_silently_dropped";

    let root = tempfile::tempdir().unwrap();
    // Six 50-character segments plus separators and the file name clear 260
    // characters on their own (305 + "/f.txt"), before the platform's own
    // temp-directory prefix adds any more.
    let segment = "a".repeat(50);
    let mut dir = root.path().to_path_buf();
    let mut relative_parts: Vec<&str> = Vec::new();
    for _ in 0..6 {
        dir = dir.join(&segment);
        relative_parts.push(segment.as_str());
    }
    fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("f.txt");
    let relative = format!("{}/f.txt", relative_parts.join("/"));

    let total_len = file_path.to_string_lossy().len();
    assert!(
        total_len > 260,
        "the constructed path is only {total_len} characters wide; the test does not clear \
         the 260-character boundary it exists to probe"
    );

    if let Err(err) = fs::write(&file_path, b"x") {
        eprintln!(
            "{NAME}: fs::write itself refused the {total_len}-character path ({err}); \
             enumerate was not exercised"
        );
        return;
    }

    let walked = enumerate(root.path(), &WalkRules::none());
    let found = walked.found.iter().find(|f| f.relative == relative);
    let skip = walked
        .skipped
        .iter()
        .find(|s| s.relative.as_deref() == Some(relative.as_str()));

    match (found, skip) {
        (Some(_), None) => eprintln!("{NAME}: ENUMERATED at {total_len} characters"),
        (None, Some(s)) => eprintln!(
            "{NAME}: REFUSED at {total_len} characters — rule {:?}, detail {:?}",
            s.rule, s.detail
        ),
        (Some(_), Some(_)) => panic!(
            "the {total_len}-character path appears in BOTH found and skipped at once, \
             which is its own defect"
        ),
        (None, None) => panic!(
            "a {total_len}-character path that was just written to disk is absent from BOTH \
             `found` and `skipped` — a silent skip, which is the defect this test exists to \
             catch"
        ),
    }
}

/// `bad\xffname.txt` is a filename Linux allows (any byte but NUL and `/`)
/// that Rust's `&str` cannot represent, and storing it lossily would store a
/// string that no longer opens the file — the whole reason
/// `PreSkipRule::UnrepresentableName` exists. APFS refuses to create the name
/// at all, enforcing valid UTF-8 at the filesystem level, so this test can
/// stay green on macOS for a reason that has nothing to do with
/// `enumerate`'s own `UnrepresentableName` branch: if `fs::write` never
/// created the file, the walk had nothing to skip, and an assertion that
/// only checked `names == ["good.txt"]` would stay green even if
/// `UnrepresentableName` were deleted outright.
///
/// So this reports both facts a reader needs to tell the two cases apart —
/// whether the write succeeded, and whether the rule actually fired — via
/// `eprintln!` (`cargo test -- --nocapture`), and asserts a real property on
/// each of the two branches rather than only on the one this machine
/// happens to take.
#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_is_skipped_rather_than_mangled() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let root = tempfile::tempdir().unwrap();
    // 0xFF is not valid UTF-8 in any position.
    let bad = root.path().join(OsStr::from_bytes(b"bad\xffname.txt"));
    let write_result = fs::write(&bad, b"x");
    fs::write(root.path().join("good.txt"), b"x").unwrap();

    let walked = enumerate(root.path(), &WalkRules::none());
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();
    let unrepresentable_fired = walked
        .skipped
        .iter()
        .any(|s| s.rule == PreSkipRule::UnrepresentableName);

    eprintln!(
        "a_name_that_is_not_utf8_is_skipped_rather_than_mangled: fs::write of the invalid \
         name {}; UnrepresentableName fired: {unrepresentable_fired}",
        if write_result.is_ok() {
            "SUCCEEDED"
        } else {
            "FAILED"
        }
    );

    match write_result {
        Ok(()) => {
            // The interesting case: the file exists on disk under a name
            // Rust cannot represent as a `String`, and `enumerate` must not
            // mangle it into one — it must be a `PreSkip`, not a `Found`.
            assert_eq!(names, ["good.txt"]);
            assert_eq!(walked.skipped.len(), 1);
            assert!(
                unrepresentable_fired,
                "the invalid name was created but UnrepresentableName never fired: {:?}",
                walked.skipped
            );
        }
        Err(_) => {
            // The filesystem itself refused the name (measured: APFS does
            // this) — nothing is left on disk for `enumerate` to see, so
            // this branch proves nothing about `UnrepresentableName` either
            // way beyond "it did not fire for a name that was never
            // created." The `eprintln!` above is what a reader must check
            // instead of this branch's assertions.
            assert_eq!(names, ["good.txt"]);
            assert!(
                !unrepresentable_fired,
                "UnrepresentableName fired for a name that was never created — nothing on \
                 disk could have produced this skip: {:?}",
                walked.skipped
            );
        }
    }
}
