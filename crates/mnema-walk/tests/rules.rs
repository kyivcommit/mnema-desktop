use mnema_walk::{RulesError, WalkRules, enumerate};
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

/// `globset::escape` does not touch `\` at all, and the pattern engine
/// compiles every glob with backslash-escape semantics unconditionally
/// (`gitignore.rs`), so a `\` ANYWHERE in a prefix — not only trailing —
/// is read as an escape character. `a\bee` used to compile to the literal
/// `abee`: the excluded folder survived, and an unrelated `abee/` was
/// removed in its place. Both halves: the well-formed prefix (no
/// backslash) excludes correctly; the backslash-containing one is refused
/// outright rather than silently compiling to a rule for a different name
/// (review fix round 2, Critical finding).
#[test]
fn a_prefix_containing_a_backslash_is_refused() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("abee")).unwrap();
    fs::write(root.path().join("abee/f.txt"), b"x").unwrap();

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["abee".to_string()]).unwrap(),
    );
    assert!(
        walked.found.is_empty(),
        "the well-formed prefix excludes its own folder"
    );

    let result = WalkRules::new(false, false, vec!["a\\bee".to_string()]);
    assert!(
        matches!(result, Err(RulesError::ContainsBackslash { .. })),
        "a backslash mid-prefix must be refused, not silently compiled into a rule for `abee`"
    );
}

/// The pattern engine silently trims trailing whitespace off a line unless
/// it ends in an escaped space, which nothing here ever emits — so a
/// prefix naming a folder with a trailing space used to compile to a
/// pattern for the SAME name without one. Both halves: the well-formed
/// prefix excludes correctly; the one with trailing whitespace is refused
/// (review fix round 2, Critical finding).
#[test]
fn a_prefix_with_trailing_whitespace_is_refused() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("Photos")).unwrap();
    fs::write(root.path().join("Photos/f.txt"), b"x").unwrap();

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["Photos".to_string()]).unwrap(),
    );
    assert!(
        walked.found.is_empty(),
        "the well-formed prefix excludes its own folder"
    );

    let result = WalkRules::new(false, false, vec!["Photos ".to_string()]);
    assert!(
        matches!(result, Err(RulesError::TrailingWhitespace { .. })),
        "trailing whitespace must be refused, not silently trimmed into a rule for `Photos`"
    );
}

/// A leading `./` is the ordinary "this directory" idiom — genuinely
/// relative once it is dropped — so it is normalised rather than refused,
/// unlike the other three forms in this round. Before the fix, `./private`
/// stayed `./private` in the compiled pattern, which (since real relative
/// paths from the walk never start with `./`) matched nothing at all: the
/// rule silently excluded no files while `new` still returned `Ok` (review
/// fix round 2, Critical finding).
#[test]
fn a_leading_dot_slash_is_normalised_not_refused() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("private")).unwrap();
    fs::write(root.path().join("private/a.txt"), b"x").unwrap();

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["./private".to_string()]).unwrap(),
    );

    assert!(
        walked.found.is_empty(),
        "`./private` must exclude exactly what `private` excludes, not match nothing"
    );
}

/// An absolute filesystem path — the shape a folder picker or a path
/// pasted from Finder produces — compiles to a pattern that can only ever
/// match the beginning of a path relative to the watched root, which an
/// absolute path never is. Before the fix, `WalkRules::new` trimmed only
/// surrounding `/` characters, so `/Users/example/private` silently
/// degraded to `Users/example/private`: a rule that excludes nothing,
/// forever, under any real watched root, while still returning `Ok`. Both
/// halves: the relative prefix excludes correctly; the absolute one is
/// refused (review fix round 2, Critical finding).
#[test]
fn an_absolute_prefix_is_refused() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("private")).unwrap();
    fs::write(root.path().join("private/a.txt"), b"x").unwrap();

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["private".to_string()]).unwrap(),
    );
    assert!(
        walked.found.is_empty(),
        "the well-formed, relative prefix excludes its own folder"
    );

    let result = WalkRules::new(false, false, vec!["/Users/example/private".to_string()]);
    assert!(
        matches!(result, Err(RulesError::AbsolutePrefix { .. })),
        "an absolute path must be refused, not silently compiled into a rule that matches nothing"
    );
}

/// Without an explicit root anchor, the pattern engine prepends `**/` to
/// any pattern with no `/` in it at all, matching at every depth instead of
/// only the root — so a one-component rule `private` used to also remove
/// `Work/deep/deeper/private/`, which is more than the user asked for and,
/// because a removed rule deletes on the next walk, the dangerous direction
/// to get wrong (review fix round 2, Important finding).
#[test]
fn a_one_component_user_prefix_is_anchored_to_the_root_only() {
    let root = tempfile::tempdir().unwrap();
    for name in ["private/a.txt", "Work/deep/deeper/private/b.txt"] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["private".to_string()]).unwrap(),
    );
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();

    assert!(
        !names.contains(&"private/a.txt"),
        "the root-level `private/` is still excluded"
    );
    assert!(
        names.contains(&"Work/deep/deeper/private/b.txt"),
        "a one-component rule must not remove `private` at other depths"
    );
}

/// A pattern with a `/` in it was already anchored to the root before this
/// fix — pinned here so the leading `/` `anchored_pattern` now adds to
/// every prefix does not change that (review fix round 2, Important
/// finding).
#[test]
fn a_multi_component_user_prefix_is_anchored_to_the_root_too() {
    let root = tempfile::tempdir().unwrap();
    for name in ["Work/private/a.txt", "Elsewhere/Work/private/b.txt"] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["Work/private".to_string()]).unwrap(),
    );
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();

    assert!(!names.contains(&"Work/private/a.txt"));
    assert!(
        names.contains(&"Elsewhere/Work/private/b.txt"),
        "a multi-component rule is anchored to the root too, not matched at any depth"
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

/// The pathological shape above (5 giant prefixes) pins that a size limit
/// exists at all, but not where the realistic threshold is: review fix
/// round 2 measured that the failure point depends on whether prefixes
/// carry a glob metacharacter, not on their length, because a pure literal
/// routes to a matching strategy with no size limit. Ordinary-shaped
/// names — the kind an import or a sync path full of `Photos [2023]`-style
/// folders produces — DO have one: measured against this code, the
/// aggregate flips to `false` somewhere between 13,000 and 13,500 prefixes
/// of about 21 characters each, one bracket pair apiece. 16,000 gives
/// comfortable margin while staying a realistic rule-list size, not a
/// deliberately pathological one — a change that halved the real threshold
/// would still redden this test even though it would leave the pathological
/// one above untouched.
#[test]
fn rules_applied_is_false_at_a_realistic_prefix_count() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("kept.txt"), b"x").unwrap();

    let prefixes: Vec<String> = (0..16_000)
        .map(|i| format!("folder[{i:05}]abcdefg"))
        .collect();
    let rules = WalkRules::new(false, false, prefixes)
        .expect("each ordinary-shaped prefix compiles fine on its own");

    let walked = enumerate(root.path(), &rules);

    assert!(
        !walked.rules_applied,
        "an ordinary-shaped rule list of realistic length should overflow the pattern engine \
         in aggregate too, not only a deliberately pathological one"
    );
}
