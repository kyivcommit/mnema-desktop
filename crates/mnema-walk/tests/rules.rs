use mnema_walk::{WalkRules, enumerate};
use std::fs;

/// The trap this test exists for: `ignore`'s `require_git` defaults to TRUE, so
/// outside a git repository NO ignore rule applies at all, and the failure is
/// silent — every excluded file quietly enters the index. A watched folder is
/// normally not a repository, and D25's measured 60% reduction assumed the
/// opposite. This test must go red if `require_git(false)` is ever removed.
#[test]
fn gitignore_applies_in_a_folder_that_is_not_a_repository() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".gitignore"), "secret.txt\n").unwrap();
    fs::write(root.path().join("secret.txt"), b"x").unwrap();
    fs::write(root.path().join("kept.txt"), b"x").unwrap();
    // Deliberately NOT a git repository: no `.git` directory anywhere.
    assert!(!root.path().join(".git").exists());

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, true, Vec::new()).unwrap(),
    );
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();

    assert!(names.contains(&"kept.txt"));
    assert!(
        !names.contains(&"secret.txt"),
        "`.gitignore` did not apply: `require_git` is back on"
    );
}

/// The unconditional part of the built-in list — `node_modules` is never a
/// document folder — plus `target`, which since review fix round 1 only
/// disappears next to its marker (`Cargo.toml`). Measured: 399,042 files with
/// no rules against 246 with the built-in list alone in this repository
/// today (`target/` is 16 GB); the earlier "411 vs 384,275" comment named a
/// different `ignore` configuration than this code ever runs, not this one.
#[test]
fn the_builtin_list_removes_build_directories() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Cargo.toml"), b"[package]\n").unwrap();
    for name in [
        "target/debug/huge.bin",
        "node_modules/pkg/index.js",
        "src/main.rs",
    ] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }

    let walked = enumerate(
        root.path(),
        &WalkRules::new(true, false, Vec::new()).unwrap(),
    );
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();

    assert_eq!(names, ["Cargo.toml", "src/main.rs"]);
    // Renamed from `excluded` in Task 1's fix round: the field counts entries
    // the walk could not READ. Rule removals are counted nowhere — a rule that
    // removes files is not an error, and one counter cannot mean both.
    assert_eq!(walked.unreadable, 0, "rule hits are not read failures");
}

/// `target`, `build` and `dist` are ordinary English words as well as build
/// output — unconditional removal took real documents with them. `build`
/// next to nothing is an ordinary folder; `build` next to `package.json` is
/// build output (review fix round 1, Important finding, measured on
/// `Projects/House/build/permits.pdf`).
#[test]
fn the_anchored_list_only_removes_build_output_next_to_its_marker() {
    let root = tempfile::tempdir().unwrap();
    for name in ["Projects/House/build/permits.pdf", "code/build/artifact.o"] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }
    fs::write(root.path().join("code/package.json"), b"{}").unwrap();

    let walked = enumerate(
        root.path(),
        &WalkRules::new(true, false, Vec::new()).unwrap(),
    );
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();

    assert!(
        names.contains(&"Projects/House/build/permits.pdf"),
        "`build` with no marker beside it is an ordinary folder, not build output"
    );
    assert!(
        !names.contains(&"code/build/artifact.o"),
        "`build` next to package.json is build output"
    );
}

/// `.DS_Store` moved out of `BUILTIN_DIRS` (a list documented as directories)
/// into its own file-pattern list (review fix round 1, Minor finding) — this
/// pins that the move did not also silently drop the exclusion.
#[test]
fn the_builtin_list_removes_ds_store_as_a_file() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".DS_Store"), b"x").unwrap();
    fs::write(root.path().join("kept.txt"), b"x").unwrap();

    let walked = enumerate(
        root.path(),
        &WalkRules::new(true, false, Vec::new()).unwrap(),
    );
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();

    assert_eq!(names, ["kept.txt"]);
}

/// A user rule is a path prefix relative to the root — the shape `ignore_rule`
/// already stores (`schema.sql:50`).
#[test]
fn a_user_prefix_removes_its_subtree() {
    let root = tempfile::tempdir().unwrap();
    for name in ["private/a.txt", "public/b.txt"] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["private".to_string()]).unwrap(),
    );
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();

    assert_eq!(names, ["public/b.txt"]);
}

/// A prefix is a path the user typed, not a glob pattern. Before review fix
/// round 1, an unescaped prefix read `[2023]` as a character class rather
/// than four literal characters, so a folder literally named `Photos
/// [2023]`, excluded by that exact name, silently stayed in `found` — with
/// no error anywhere, because the pattern still compiled. `globset::escape`
/// closes that (Critical finding).
#[test]
fn a_user_prefix_excludes_a_name_with_glob_metacharacters_literally() {
    let root = tempfile::tempdir().unwrap();
    for name in ["Photos [2023]/a.jpg", "Photos 2024/b.jpg"] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["Photos [2023]".to_string()]).unwrap(),
    );
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();

    assert_eq!(names, ["Photos 2024/b.jpg"]);
}

/// `globset::escape` escapes every glob metacharacter except a trailing
/// backslash — itself a glob escape character — so a prefix ending in one
/// still fails to compile even after escaping. `WalkRules::new` is where
/// that has to be caught: it is the only place left with a human in front of
/// it who can fix the rule (review fix round 1, Critical finding).
#[test]
fn a_prefix_that_cannot_compile_is_refused_by_new() {
    let result = WalkRules::new(false, false, vec!["secret\\".to_string()]);

    assert!(
        result.is_err(),
        "a trailing backslash is a dangling glob escape even after `globset::escape`; \
         `new` must refuse it rather than accept a rule that will silently fail later"
    );
}

/// The built-in list is a default, not a law: a user who wants `target/`
/// indexed must be able to have it.
#[test]
fn the_builtin_list_can_be_turned_off() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("target/debug/x.txt");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"x").unwrap();

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, Vec::new()).unwrap(),
    );

    assert_eq!(walked.found.len(), 1);
}

/// `WalkRules::new` refuses a prefix that cannot compile ALONE, but cannot
/// see the aggregate: several prefixes, each individually fine, can still
/// exceed the pattern engine's size limit once combined into one `Override`.
/// Measured: 5 distinct ~100,000-character prefixes combine over the limit
/// while any one of them alone does not (each passes `WalkRules::new`'s
/// per-prefix probe). `Walked::rules_applied` is where that failure has to
/// surface instead, since by the time `builder()` runs there is no human
/// left to ask (review fix round 1, Critical finding).
#[test]
fn rules_applied_is_false_when_the_combined_rule_set_is_too_large() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("kept.txt"), b"x").unwrap();

    let huge_prefixes: Vec<String> = (0..5)
        .map(|i| format!("{}_{i}", "?".repeat(100_000)))
        .collect();
    let rules = WalkRules::new(false, false, huge_prefixes)
        .expect("each prefix alone is well under the per-prefix size limit");

    let walked = enumerate(root.path(), &rules);

    assert!(
        !walked.rules_applied,
        "the combined override set should have failed to build"
    );
}
