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

/// 🔴 The sibling half of the rule above, and PR 8a Task 6 review round 1 found
/// nothing asserting it. `a_user_prefix_removes_its_subtree` fixes `private`
/// and `public`, two names sharing no prefix, so a matcher comparing bare
/// string prefixes instead of whole path components passes it. `private2/` is
/// the name that separates the two forms: it starts with `private` and is not
/// under it.
///
/// Measured, not assumed: `grep` for a sibling-prefix fixture across every
/// `.rs` under `crates/` and `src-tauri/` found none, and mutating
/// `anchored_pattern` to `!/{escaped}*` — a rule that swallows siblings — killed
/// this test **alone** among the 22 in this file, leaving
/// `a_user_prefix_removes_its_subtree` green.
///
/// The direction that costs: a rule that over-matches DELETES from the index
/// what the person never excluded — the same `should_delete` pass that
/// `walk.rs` runs after a completed walk. The other direction is worse under
/// D29: a rule that stops matching leaves the text in the index and sends it
/// to a third-party provider on the next pass.
///
/// This is also the Rust half of the pair holding `Folders.svelte`'s own
/// `under`, which counts what a person is about to lose before the rule is
/// stored. That copy is pinned by `Folders.test.ts`'s `a sibling whose name
/// merely starts with the prefix is not counted`; Rust and TypeScript share no
/// compiler, and neither half closes the gap alone.
#[test]
fn a_user_prefix_does_not_remove_a_sibling_whose_name_starts_with_it() {
    let root = tempfile::tempdir().unwrap();
    for name in ["private/a.txt", "private2/b.txt", "privateer.txt"] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["private".to_string()]).unwrap(),
    );
    let mut names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();
    names.sort_unstable();

    // Both directions: the sibling folder and the sibling FILE survive, and
    // `private/a.txt` is genuinely gone — without that last clause the
    // assertion is satisfied by a rule that excludes nothing at all.
    assert_eq!(names, ["private2/b.txt", "privateer.txt"]);
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

/// The pattern engine silently trims TRAILING whitespace off a line unless
/// it ends in an escaped space, which nothing here ever emits — so a
/// prefix naming a folder with a trailing space used to compile to a
/// pattern for the SAME name without one. LEADING whitespace is the
/// opposite failure, found only in review fix round 3: it is NOT trimmed
/// by the pattern engine, so it compiles fine and matches only a folder
/// that literally has that leading space — almost never what a stray
/// keystroke meant, and `new` returned `Ok` either way. Both edges are
/// refused now, not only the trailing one round 2 caught (review fix
/// round 3, Important finding).
#[test]
fn a_prefix_with_surrounding_whitespace_is_refused() {
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

    for malformed in ["Photos ", " Photos"] {
        let result = WalkRules::new(false, false, vec![malformed.to_string()]);
        assert!(
            matches!(result, Err(RulesError::SurroundingWhitespace { .. })),
            "{malformed:?} must be refused, not silently compiled into a rule for a \
             differently-named folder"
        );
    }
}

/// `.` and `..` are navigation, not folder names — and round 2's single
/// `strip_prefix("./")` only ever ran once, so `././Photos` (a repeated
/// `./`) slipped through it entirely, still compiling to a pattern that
/// matches nothing (review fix round 3, Critical finding). Both halves,
/// with a SURVIVING SIBLING FILE this time: the earlier `./`-prefix test
/// only ever wrote one file, so `found.is_empty()` could not tell "excluded
/// exactly `private`" apart from "excluded everything" — measured, that
/// test stayed green even with `anchored_pattern` mutated to `"!/**"`
/// (review fix round 3, first test-gap finding).
#[test]
fn a_dot_or_dotdot_component_is_refused() {
    let root = tempfile::tempdir().unwrap();
    for name in ["private/a.txt", "kept.txt"] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["private".to_string()]).unwrap(),
    );
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();
    assert_eq!(
        names,
        ["kept.txt"],
        "the well-formed prefix excludes exactly `private`, nothing more and nothing less"
    );

    for malformed in ["./private", "././private", "private/.", "private/..", ".."] {
        let result = WalkRules::new(false, false, vec![malformed.to_string()]);
        assert!(
            matches!(result, Err(RulesError::DotComponent { .. })),
            "{malformed:?} must be refused, not silently compiled into a rule that matches \
             nothing"
        );
    }
}

/// A leading `/`, a trailing `/`, and a doubled `/` (`Photos//sub`, found
/// in review fix round 3) all produce an empty path component once split —
/// including the shape an absolute filesystem path takes
/// (`/Users/example/private` starts with `/`), which round 2 caught with a
/// platform-dependent `Path::is_absolute()` call that this whitelist
/// replaces (see `RulesError::DriveLetterPrefix`'s doc comment for why that
/// call had to go). Both halves for each shape.
#[test]
fn an_empty_path_component_is_refused() {
    let root = tempfile::tempdir().unwrap();
    for name in ["private/a.txt", "kept.txt"] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["private".to_string()]).unwrap(),
    );
    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();
    assert_eq!(names, ["kept.txt"]);

    for malformed in [
        "/private",
        "private/",
        "Photos//sub",
        "/Users/example/private",
    ] {
        let result = WalkRules::new(false, false, vec![malformed.to_string()]);
        assert!(
            matches!(result, Err(RulesError::EmptyComponent { .. })),
            "{malformed:?} must be refused, not silently compiled into a rule that matches \
             nothing"
        );
    }
}

/// A control character cannot be part of a folder name a person typed on
/// purpose.
#[test]
fn a_prefix_containing_a_control_character_is_refused() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("private")).unwrap();
    fs::write(root.path().join("private/a.txt"), b"x").unwrap();

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["private".to_string()]).unwrap(),
    );
    assert!(walked.found.is_empty());

    let result = WalkRules::new(false, false, vec!["priv\u{0007}ate".to_string()]);
    assert!(matches!(
        result,
        Err(RulesError::ContainsControlCharacter { .. })
    ));
}

/// A single ASCII letter followed by `:` as the first component is the
/// shape of a Windows drive letter — the platform-specific replacement for
/// round 2's `Path::is_absolute()` check, which the review measured
/// refuses `/private` on macOS while silently accepting it, compiling to a
/// pattern that matches nothing, on Windows (`Path::new("/private")
/// .is_absolute()` is FALSE there — confirmed against the doc comment on
/// `Path::is_absolute`, which this crate no longer calls at all). Checking
/// the shape of the first component instead of asking the platform means
/// this refuses `C:` the same way on every platform this ships to (review
/// fix round 3, Critical finding).
#[test]
fn a_prefix_shaped_like_a_windows_drive_letter_is_refused() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("private")).unwrap();
    fs::write(root.path().join("private/a.txt"), b"x").unwrap();

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["private".to_string()]).unwrap(),
    );
    assert!(walked.found.is_empty());

    for malformed in ["C:", "C:/Users/example/private"] {
        let result = WalkRules::new(false, false, vec![malformed.to_string()]);
        assert!(matches!(result, Err(RulesError::DriveLetterPrefix { .. })));
    }
    // Two letters and a colon, but NOT the drive-letter shape (more than one
    // letter before the `:`) — must not be caught by the same check.
    let ok = WalkRules::new(false, false, vec!["CD:".to_string()]);
    assert!(
        ok.is_ok(),
        "`CD:` is not a drive letter and must be accepted"
    );
}

/// `~` as a first component is a shell convention for a home directory that
/// this crate does not expand — taken literally it names an ordinary
/// folder called `~`, which essentially never exists under a watched root,
/// so `~/Photos` used to compile fine and exclude nothing (review fix
/// round 3, Critical finding; not one of the review's five literal
/// whitelist bullets, added because the same message named `~/Photos` as
/// one of the four forms the whitelist was supposed to close and the
/// literal five bullets alone do not catch it — see the report).
#[test]
fn a_prefix_starting_with_a_home_directory_shorthand_is_refused() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("private")).unwrap();
    fs::write(root.path().join("private/a.txt"), b"x").unwrap();

    let walked = enumerate(
        root.path(),
        &WalkRules::new(false, false, vec!["private".to_string()]).unwrap(),
    );
    assert!(walked.found.is_empty());

    let result = WalkRules::new(false, false, vec!["~/Photos".to_string()]);
    assert!(matches!(
        result,
        Err(RulesError::HomeDirectoryShorthand { .. })
    ));
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
/// `WalkRules::new`'s compile-probe (a single prefix, alone, in a throwaway
/// `OverrideBuilder`) had zero coverage: review fix round 3 measured that
/// deleting that whole block left all 15 tests from round 2 green, because
/// every prefix any test used was well under the size limit on its own —
/// the only test that pushed a single prefix that large combined FIVE of
/// them (`rules_applied_is_false_when_the_combined_rule_set_is_too_large`),
/// which exercises the AGGREGATE `builder()` path, not this per-prefix one
/// in `WalkRules::new`. Measured directly against this code (not the
/// review's own numbers, which were close but not identical — a different
/// construction): a single `?`-repeated prefix passes at 300,000 characters
/// and fails at 350,000. 600,000 gives comfortable margin.
#[test]
fn a_single_prefix_past_the_size_limit_is_refused_by_new() {
    let huge = "?".repeat(600_000);

    let result = WalkRules::new(false, false, vec![huge]);

    assert!(
        matches!(result, Err(RulesError::InvalidPrefix { .. })),
        "a single prefix this large must be refused by `new`'s own compile-probe, not only \
         discovered later as part of an aggregate `rules_applied == false`"
    );
}

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

/// `WalkRules::builtin_layers` and the walker must agree, because they are two
/// readings of the same layers and the whole point of the predicate is that a
/// caller can ask what the walk will do without running it.
///
/// Asserted against a real `enumerate`, not against a list written by hand: the
/// question is "does this answer match the walk", and only the walk can say.
///
/// 🔴 **The fixture's directories are GENERATED from `BUILTIN_DIRS` and
/// `BUILTIN_FILES`, not typed out**, and that is fix round 2's correction
/// rather than tidiness. The previous version listed eight directories chosen
/// from the same five-row enumeration the predicate was written from, so it
/// agreed with itself: `BUILTIN_FILES` was in neither, and a folder named
/// `.DS_Store` — which the walk prunes — was reported excludable for a whole
/// round. Generated from the constants, every entry of either list gets a
/// directory whether or not anyone remembered it, and a name added to either
/// list arrives with its own fixture.
///
/// **What generation costs, and what is added back by hand.** A fixture
/// derived from a constant cannot notice that constant shrinking: delete
/// `.git` from `BUILTIN_DIRS` and both the fixture and the expectation lose it
/// together. So the built-in names are **written out by hand below, all of
/// them**, and checked against the constants as a set.
///
/// Fix round 3 measured what "three names, one of each shape" was actually
/// worth: deleting `.hg` from `BUILTIN_DIRS` left the whole workspace green —
/// three of thirteen is not "the membership half", it is three names, and the
/// sentence that stood here said otherwise. Writing all thirteen out closes it
/// in both directions: **deleting** a name leaves the hand list demanding a
/// folder nothing prunes, and **adding** one fails the set comparison until
/// somebody puts it here deliberately, which is the point — a name joining the
/// list that the walk prunes should be a decision, not a diff nobody read.
///
/// **What it still cannot see**, and it is the third derivation of this set
/// saying so: a new layer in `builder()` that is neither an override nor
/// `ANCHORED_DIRS`. Point 1 of `builtin_layers`' doc covers every future
/// override for free; nothing here can anticipate a second `filter_entry`.
///
/// `gitignore: false`, deliberately: the predicate does not answer for that
/// layer and never claims to, so leaving it on would compare against a walk
/// making a decision the predicate is not party to.
#[test]
fn builtin_layers_agree_with_what_the_walk_enumerates() {
    let root = tempfile::tempdir().unwrap();

    // Generated: one directory per built-in name, with a file inside it, so
    // "the walk reached it" is observable.
    let mut dirs: Vec<String> = WalkRules::BUILTIN_DIRS
        .iter()
        .chain(WalkRules::BUILTIN_FILES)
        .map(|name| format!("{name}/inner"))
        .collect();
    // The anchored layer, both directions, plus ordinary controls. Typed out
    // because this layer is a name AND a sibling marker, which no list of
    // names can generate.
    dirs.extend(
        [
            "crate/target/debug",
            "code/build",
            "code/dist",
            "house/target",
            "Projects/House/build",
            "notes/2019",
            // On neither list, and the walk keeps it because `builder()` sets
            // `hidden(false)`. It is here so that a change to that SETTING —
            // the shape of layer no grep over `filter_entry|over.add` finds,
            // and the one fix round 3 measured slipping past everything —
            // turns this guard red instead of nothing at all.
            ".config/2019",
        ]
        .map(str::to_string),
    );
    for dir in &dirs {
        let path = root.path().join(dir);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("file.txt"), b"x").unwrap();
    }
    // The two markers, which are what make `crate/target` and `code/build`
    // build output while `house/target` and `Projects/House/build` are folders.
    fs::write(root.path().join("crate/Cargo.toml"), b"[package]\n").unwrap();
    fs::write(root.path().join("code/package.json"), b"{}").unwrap();

    let walked = enumerate(
        root.path(),
        &WalkRules::new(true, false, Vec::new()).unwrap(),
    );
    let layers = WalkRules::builtin_layers(root.path());

    let mut pruned = Vec::new();
    let mut kept = Vec::new();
    for dir in &dirs {
        let reached = walked
            .found
            .iter()
            .any(|f| f.relative.starts_with(&format!("{dir}/")));
        let predicted = layers.prunes(dir);
        assert_eq!(
            predicted, !reached,
            "the predicate says pruned={predicted} for {dir}, the walk reached it={reached}"
        );
        if predicted { &mut pruned } else { &mut kept }.push(dir.as_str());
    }

    // The fixture really did build both sides, rather than agreeing trivially
    // because one of them is empty — and the kept side is named in full,
    // because "everything is pruned" would satisfy the loop above.
    assert_eq!(
        kept,
        vec![
            "house/target",
            "Projects/House/build",
            "notes/2019",
            ".config/2019"
        ],
        "the folders the walk keeps: {kept:?}"
    );
    assert_eq!(
        pruned.len(),
        WalkRules::BUILTIN_DIRS.len() + WalkRules::BUILTIN_FILES.len() + 3,
        "every built-in name plus crate/target/debug, code/build and code/dist: {pruned:?}"
    );

    // The membership the generated half cannot hold, written out rather than
    // derived — every name on both lists. Deleting any of them from its
    // constant would otherwise remove it from the fixture and the expectation
    // together, and this test would still pass (measured on `.hg`, fix round
    // 3: the whole workspace stayed green).
    let pinned = [
        ".git",
        ".hg",
        ".svn",
        "node_modules",
        "__pycache__",
        ".mypy_cache",
        ".pytest_cache",
        ".gradle",
        ".idea",
        ".vscode",
        ".venv",
        "venv",
        ".DS_Store",
    ];
    for name in pinned {
        assert!(
            pruned.contains(&format!("{name}/inner").as_str()),
            "{name} is no longer pruned by the built-in layers: {pruned:?}"
        );
    }
    // The other direction, and the reason the list above may be written by
    // hand at all: it must be exactly the two constants. A name ADDED to
    // either one fails here until somebody adds it here too — which is a
    // deliberate act rather than a diff that slid past, and it is what keeps
    // this hand-written half from decaying into a sample of the generated one.
    let mut from_constants: Vec<&str> = WalkRules::BUILTIN_DIRS
        .iter()
        .chain(WalkRules::BUILTIN_FILES)
        .copied()
        .collect();
    from_constants.sort_unstable();
    let mut written_out = pinned.to_vec();
    written_out.sort_unstable();
    assert_eq!(
        written_out, from_constants,
        "the hand-pinned names have drifted from BUILTIN_DIRS/BUILTIN_FILES — add or remove the \
         name here on purpose"
    );
}

/// `WalkRules::check_prefix` must answer exactly what `WalkRules::new` answers
/// for the same prefix, because its whole purpose is to let a caller ask the
/// question **before** offering a control that `new` would then refuse. A
/// wrapper that drifted from the thing it mirrors would put the desktop's
/// folder listing back where fix round 1 found it: offering an action that
/// cannot succeed.
///
/// Twelve prefixes, six of each answer, covering every rule
/// `validate_component` applies plus the two it applies to the first component
/// only — `a/~` is accepted and `~` is not, which is the direction a wrapper
/// that checked one component at a time would get wrong.
#[test]
fn check_prefix_answers_exactly_what_new_answers() {
    for prefix in [
        "plain",
        "Work/private",
        "a/~",
        "Photos [2023]",
        "~tilde-inside",
        "",
        " lead",
        "trail ",
        "back\\slash",
        "..",
        "~",
        "C:",
    ] {
        let wrapper = WalkRules::check_prefix(prefix).is_ok();
        let real = WalkRules::new(false, false, vec![prefix.to_string()]).is_ok();
        assert_eq!(
            wrapper, real,
            "check_prefix and new disagree about {prefix:?}: {wrapper} vs {real}"
        );
    }
    // Both answers really occur, so the loop is not twelve trivial agreements
    // on one side.
    assert!(WalkRules::check_prefix("a/~").is_ok());
    assert!(WalkRules::check_prefix("~").is_err());
}

// ---------------------------------------------------------------------------
// User file masks (PR 8b, Task 9 — D-c)
//
// A mask is a glob over a file's NAME, global to the index, and it never
// prunes a directory. Every case below asserts both directions: the file the
// mask names goes, and a neighbour it does not name survives. A one-sided
// assertion here is satisfied by a walk that found nothing at all.
// ---------------------------------------------------------------------------

/// Builds a tree, walks it with `masks` and no other user rules, and returns
/// the relative paths the walk kept. Written once because every mask case
/// needs the same three lines, and a case that built its own walker could
/// quietly walk with different settings.
fn walk_with_masks(files: &[&str], builtin: bool, masks: &[&str]) -> Vec<String> {
    let root = tempfile::tempdir().unwrap();
    for name in files {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }
    let rules = WalkRules::new(builtin, false, Vec::new())
        .unwrap()
        .with_masks(masks.iter().map(|m| m.to_string()).collect())
        .unwrap();
    let walked = enumerate(root.path(), &rules);
    walked
        .found
        .iter()
        .map(|f| f.relative.clone())
        .collect::<Vec<_>>()
}

/// One mask, one name, straight through `MaskLayer::matches` — for cases
/// that assert on the match itself rather than on what a walk keeps or
/// removes, and so need no filesystem at all.
fn m(mask: &str, name: &str) -> bool {
    WalkRules::none()
        .with_masks(vec![mask.to_string()])
        .unwrap()
        .masks()
        .matches(name)
}

/// A mask is a glob over a file's **name**, at every depth — asserted, not
/// inherited from `ignore`'s override parser, which stops being the thing that
/// matches a mask the moment the directory rule below evicts masks from the
/// `Override` (D-c, amended).
///
/// The fixture separates three readings of "matches" that a name-only matcher
/// and a path matcher answer differently:
/// - `sub/report2.pdf` goes, so the layer is not anchored to the root and is
///   not matching the whole relative path (`report*.pdf` against
///   `sub/report2.pdf` would fail — the path does not start with `report`);
/// - `sub/myreport.pdf` stays, so the whole name has to match, not a substring;
/// - `sub/notes.txt` stays, which is the plain surviving neighbour.
#[test]
fn a_mask_removes_a_matching_file_at_every_depth() {
    let kept = walk_with_masks(
        &[
            "report1.pdf",
            "sub/report2.pdf",
            "sub/myreport.pdf",
            "sub/notes.txt",
        ],
        false,
        &["report*.pdf"],
    );

    assert_eq!(kept, ["sub/myreport.pdf", "sub/notes.txt"]);
}

/// A mask containing `/` is refused, never reinterpreted as a path. `/` is
/// what a **prefix** expresses, and a prefix comes from disk where the
/// byte-equality question is settled (D-a); confining a typed rule to one path
/// component is what keeps it from having to be a correct path as well.
///
/// Measured on this repository's pinned `globset 0.4.19`: `logs/*.tmp`
/// compiles without error and then matches nothing, because the layer is asked
/// about a file's name and a name never contains `/`. So the refusal cannot
/// come from the compile probe — it has to be its own check.
#[test]
fn a_mask_containing_a_slash_is_refused() {
    let kept = walk_with_masks(&["logs/a.tmp", "logs/b.txt"], false, &["*.tmp"]);
    assert_eq!(
        kept,
        ["logs/b.txt"],
        "the well-formed mask removes the file it names, at depth"
    );

    let result = WalkRules::none().with_masks(vec!["logs/*.tmp".to_string()]);
    assert!(
        matches!(result, Err(RulesError::MaskContainsSlash { .. })),
        "a mask with a `/` must be refused, not compiled into a rule that matches nothing"
    );
}

/// The blank-row case, mirroring `validate_prefix`'s one deliberate non-error
/// (`rules.rs`): an empty mask is `Ok`, and it stores nothing.
///
/// "Stores nothing" is asserted positively rather than as "no error": an empty
/// glob compiles fine and matches the empty string, so a layer that pushed it
/// anyway would answer `true` here while every walk stayed green — a file name
/// is never empty.
#[test]
fn an_empty_mask_is_accepted_and_stores_no_rule() {
    let rules = WalkRules::none()
        .with_masks(vec![String::new(), "*.tmp".to_string()])
        .expect("an empty mask is the blank-row case, not a malformed rule");

    assert!(
        rules.masks().matches("a.tmp"),
        "the non-empty mask beside it still applies"
    );
    assert!(
        !rules.masks().matches(""),
        "the empty mask became a stored glob: it matches the empty name"
    );

    let kept = walk_with_masks(&["a.tmp", "b.txt"], false, &["", "*.tmp"]);
    assert_eq!(kept, ["b.txt"]);
}

/// A mask that cannot compile on its own is refused with a sentence, the same
/// way a prefix is. Measured: `[` fails to compile (`unclosed character class;
/// missing ']'`), while `[ab]*.txt` compiles and matches.
#[test]
fn a_mask_that_cannot_compile_alone_is_refused() {
    let kept = walk_with_masks(&["a1.txt", "c1.txt"], false, &["[ab]*.txt"]);
    assert_eq!(
        kept,
        ["c1.txt"],
        "a well-formed character class is an ordinary mask"
    );

    let result = WalkRules::none().with_masks(vec!["[".to_string()]);
    assert!(
        matches!(result, Err(RulesError::InvalidMask { .. })),
        "a mask that cannot compile must be refused"
    );
}

/// A control character cannot be part of a file name a person typed on
/// purpose — the same judgement `validate_component` makes about a prefix.
#[test]
fn a_mask_containing_a_control_character_is_refused() {
    let kept = walk_with_masks(&["report.pdf", "notes.txt"], false, &["*.pdf"]);
    assert_eq!(kept, ["notes.txt"]);

    let result = WalkRules::none().with_masks(vec!["*.p\u{7}df".to_string()]);
    assert!(
        matches!(result, Err(RulesError::MaskContainsControlCharacter { .. })),
        "a control character must be refused, not compiled into a rule for a name nobody has"
    );
}

/// 🔴 **Acceptance regression, first half (D-c).** `ignore` decides file from
/// directory on `if !glob.is_only_dir() || is_dir`
/// (`ignore-0.4.31/src/gitignore.rs:273`), so a pattern that is not explicitly
/// directory-only matches **both** — and `.gitignore` syntax can express
/// directory-only (a trailing `/`) and cannot express file-only. Measured in
/// the plan with `!*.pdf` as the only override over exactly this tree: the walk
/// kept `["notes.txt"]` alone, so a whole subtree disappeared on a rule a
/// person wrote about files.
///
/// The mask therefore cannot live in the `Override` at all. Both directions:
/// `report.pdf` goes because the mask names it, and `archive.pdf/keep.txt`
/// stays because a directory is never a mask's business.
#[test]
fn a_mask_never_prunes_a_directory() {
    let kept = walk_with_masks(
        &["archive.pdf/keep.txt", "report.pdf", "notes.txt"],
        false,
        &["*.pdf"],
    );

    assert_eq!(kept, ["archive.pdf/keep.txt", "notes.txt"]);
}

/// 🔴 **Acceptance regression, second half (D-c).** `WalkBuilder::filter_entry`
/// **replaces** the predicate rather than adding one — "only one filter
/// predicate can be applied to a `WalkBuilder`. Calling this subsequent times
/// overrides previous filter predicates" (`ignore-0.4.31/src/walk.rs:1042-1044`)
/// — and the crate's one slot already holds the `ANCHORED_DIRS` layer. A mask
/// added through a second `filter_entry` call therefore silently un-anchors
/// `target`/`build`/`dist`, indexes them, and under D29 sends their contents to
/// a third-party provider, **with every other mask test still green**, because
/// no other mask test looks at `ANCHORED_DIRS`.
///
/// This case exists to go red on exactly that, so it asserts all four
/// directions: the anchored directory is still pruned, the mask still removes
/// its own file, and the two ordinary files are still there.
#[test]
fn a_stored_mask_does_not_un_anchor_the_builtin_layer() {
    let kept = walk_with_masks(
        &[
            "Cargo.toml",
            "target/debug/huge.bin",
            "notes.tmp",
            "src/main.rs",
        ],
        true,
        &["*.tmp"],
    );

    assert_eq!(kept, ["Cargo.toml", "src/main.rs"]);
}

/// 🔴 **The owner's ruling of 2026-08-31, and the reason is the failure
/// direction, not simplicity.** A case-sensitive mask means a person writes
/// `*.pdf`, `REPORT.PDF` is indexed anyway, and under D29 its text goes to a
/// third-party provider — the same under-exclusion hole D-a exists to avoid,
/// arriving through a typed rule. Erring toward excluding too much is the
/// direction a person can see and undo.
///
/// 🔴 **This is the case the Unicode work must not cost, which is why it is
/// listed first among the mask cases and not folded into one of them.** An
/// uppercase extension is what cameras, scanners and Windows produce, so this is
/// the shape that actually occurs; a change that made `Копія*` work and stopped
/// `*.PDF` from removing `report.pdf` would have broken the common case to fix
/// the rare one. Measured: it dies from either half of `caseless_form` being
/// removed, so it is a live guard on the new mechanism and not a leftover from
/// the flag that used to deliver the ruling.
#[test]
fn a_mask_matches_a_file_name_whatever_its_ascii_case() {
    let kept = walk_with_masks(
        &["report.pdf", "summary.PDF", "notes.txt"],
        false,
        &["*.PDF"],
    );

    assert_eq!(kept, ["notes.txt"]);
}

/// The other half of the same ruling, and it is a separate case because folding
/// applied one layer too wide would satisfy the one above while silently
/// changing exclusion semantics for rules that come from **disk** and need no
/// help. Two ways that could happen, and both are live: `caseless_form` could be
/// called on a prefix as well as on a mask, or — the older shape, from when the
/// ruling was delivered by a builder flag — `OverrideBuilder::case_insensitive`
/// is scoped by add order rather than per builder
/// (`ignore-0.4.31/src/overrides.rs:149-151`), so one shared builder can hold
/// both semantics at once.
///
/// The fixture is one folder, not two, on purpose: macOS's default filesystem
/// is case-insensitive at lookup, so `Photos` and `photos` cannot both exist.
/// Both directions are still asserted, by walking the same tree twice.
#[test]
fn a_user_prefix_is_still_byte_exact_beside_a_mask() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("Photos")).unwrap();
    fs::write(root.path().join("Photos/a.jpg"), b"x").unwrap();
    fs::write(root.path().join("keep.txt"), b"x").unwrap();

    let exact = WalkRules::new(false, false, vec!["Photos".to_string()])
        .unwrap()
        .with_masks(vec!["*.pdf".to_string()])
        .unwrap();
    let names: Vec<String> = enumerate(root.path(), &exact)
        .found
        .iter()
        .map(|f| f.relative.clone())
        .collect();
    assert_eq!(
        names,
        ["keep.txt"],
        "the prefix as typed excludes its folder"
    );

    let wrong_case = WalkRules::new(false, false, vec!["photos".to_string()])
        .unwrap()
        .with_masks(vec!["*.pdf".to_string()])
        .unwrap();
    let names: Vec<String> = enumerate(root.path(), &wrong_case)
        .found
        .iter()
        .map(|f| f.relative.clone())
        .collect();
    assert_eq!(
        names,
        ["Photos/a.jpg", "keep.txt"],
        "the mask layer's case-insensitivity leaked into the prefix layer"
    );
}

/// 🔴 **The other half of "one folding mechanism, and only in the mask layer":
/// a prefix must not acquire the normalisation bridge either.** A prefix comes
/// from disk (D-a) — the window hands back the bytes the walk itself reported —
/// so the two forms are never a question there, and making the prefix layer
/// normalise would silently widen every stored exclusion instead.
///
/// Written against the form the filesystem actually returned, for the same
/// reason the mask case is: APFS stores what it is given and HFS+ converted to
/// NFD, so the on-disk form is not a constant this case can assume.
///
/// Both directions: the prefix in the form the walk reported excludes the
/// folder, and the prefix in the other form excludes nothing.
#[test]
fn a_user_prefix_is_not_normalised_beside_a_mask() {
    const KYIV_NFC: &str = "\u{41a}\u{438}\u{457}\u{432}";
    const KYIV_NFD: &str = "\u{41a}\u{438}\u{456}\u{308}\u{432}";

    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(KYIV_NFD)).unwrap();
    fs::write(root.path().join(KYIV_NFD).join("a.jpg"), b"x").unwrap();
    fs::write(root.path().join("keep.txt"), b"x").unwrap();

    let walk = |prefix: &str| -> Vec<String> {
        let rules = WalkRules::new(false, false, vec![prefix.to_string()])
            .unwrap()
            .with_masks(vec!["*.pdf".to_string()])
            .unwrap();
        enumerate(root.path(), &rules)
            .found
            .iter()
            .map(|f| f.relative.clone())
            .collect()
    };

    let on_disk = enumerate(root.path(), &WalkRules::none())
        .found
        .iter()
        .map(|f| f.relative.clone())
        .find(|p| p.ends_with("/a.jpg"))
        .expect("the walk found the file inside the folder this case is about");
    let folder = on_disk
        .strip_suffix("/a.jpg")
        .expect("the path this case just matched on")
        .to_string();
    let other_form = if folder == KYIV_NFD {
        KYIV_NFC
    } else {
        KYIV_NFD
    };
    assert_ne!(
        folder.as_str(),
        other_form,
        "the two forms of the folder name must be different byte strings, or this case proves \
         nothing"
    );

    assert_eq!(
        walk(&folder),
        ["keep.txt"],
        "the prefix in the form the walk reported must exclude its folder"
    );
    assert_eq!(
        // Sorted by path bytes — `k` (0x6b) precedes the first byte of `К`
        // (0xd0), so the folder that survived comes second.
        walk(other_form),
        ["keep.txt", on_disk.as_str()],
        "the mask layer's normalisation leaked into the prefix layer"
    );
}

/// 🔴 **The hole Task 9 measured, closed: a mask and a name in different
/// normalisation forms now match.** `café.pdf` composed (`caf\u{e9}.pdf`) and
/// decomposed (`cafe\u{301}.pdf`) are different byte strings that name the same
/// file to a person, and the form on disk is not predictable — APFS stores what
/// it was given, HFS+ converted to NFD. A mask that only matched one form left
/// the file indexed, and under D29 its text goes to a provider.
///
/// **Both directions, and the fixture is built to make them two directions
/// rather than one twice.** `café.pdf` is created composed and `Київ.txt` is
/// created decomposed, so the two on-disk names are in opposite forms; each is
/// then masked with *its* counterpart form. The guard below fails loudly if the
/// filesystem normalised them into the same form, because at that point the
/// case would be asserting one direction twice and looking like two.
///
/// Every constant is compared against **the form the walk handed back**, never
/// against a guessed one — the discipline Task 9's version of this case
/// established, and the reason it does not pass for the wrong reason on one of
/// two filesystems.
///
/// In Ukrainian only `й` and `ї` decompose at all, which is why the second name
/// is `Київ` and not, say, `звіт`: a name with nothing to decompose would make
/// this case green while proving nothing.
#[test]
fn a_mask_bridges_unicode_normalisation_in_both_directions() {
    const CAFE_NFC: &str = "caf\u{e9}.pdf";
    const CAFE_NFD: &str = "cafe\u{301}.pdf";
    const KYIV_NFC: &str = "\u{41a}\u{438}\u{457}\u{432}.txt";
    const KYIV_NFD: &str = "\u{41a}\u{438}\u{456}\u{308}\u{432}.txt";

    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(CAFE_NFC), b"x").unwrap();
    fs::write(root.path().join(KYIV_NFD), b"x").unwrap();
    fs::write(root.path().join("notes.txt"), b"x").unwrap();

    let walk = |masks: &[&str]| -> Vec<String> {
        let rules = WalkRules::none()
            .with_masks(masks.iter().map(|m| m.to_string()).collect())
            .unwrap();
        enumerate(root.path(), &rules)
            .found
            .iter()
            .map(|f| f.relative.clone())
            .collect()
    };

    let on_disk = walk(&[]);
    let cafe = on_disk
        .iter()
        .find(|p| p.ends_with(".pdf"))
        .expect("the walk found the composed-form file this case is about")
        .clone();
    let kyiv = on_disk
        .iter()
        .find(|p| p.ends_with(".txt") && *p != "notes.txt")
        .expect("the walk found the decomposed-form file this case is about")
        .clone();

    let cafe_other = if cafe == CAFE_NFD { CAFE_NFC } else { CAFE_NFD };
    let kyiv_other = if kyiv == KYIV_NFD { KYIV_NFC } else { KYIV_NFD };
    assert_ne!(
        cafe.as_str(),
        cafe_other,
        "the two forms of café must be different byte strings, or this case proves nothing"
    );
    assert_ne!(
        kyiv.as_str(),
        kyiv_other,
        "the two forms of Київ must be different byte strings, or this case proves nothing"
    );
    assert_ne!(
        cafe == CAFE_NFC,
        kyiv == KYIV_NFC,
        "the fixture's two files landed in the SAME normalisation form ({cafe:?}, {kyiv:?}) — \
         this filesystem normalises names, so the two assertions below would be one direction \
         asserted twice rather than both"
    );

    // Direction one: a composed mask against a decomposed name, or the reverse
    // — whichever way round this filesystem put café.
    let kept = walk(&[cafe_other]);
    assert!(
        !kept.contains(&cafe),
        "a mask in the other normalisation form did not remove {cafe:?}; kept = {kept:?}"
    );
    assert!(
        kept.contains(&kyiv) && kept.contains(&"notes.txt".to_string()),
        "the mask took a neighbour with it; kept = {kept:?}"
    );

    // Direction two: the same question with the forms the other way round.
    let kept = walk(&[kyiv_other]);
    assert!(
        !kept.contains(&kyiv),
        "a mask in the other normalisation form did not remove {kyiv:?}; kept = {kept:?}"
    );
    assert!(
        kept.contains(&cafe) && kept.contains(&"notes.txt".to_string()),
        "the mask took a neighbour with it; kept = {kept:?}"
    );

    // And the plain case a transform could break while fixing the two above: a
    // mask in the *same* form as the name still matches.
    let kept = walk(&[&cafe, &kyiv]);
    assert_eq!(
        kept,
        ["notes.txt"],
        "a mask in the form the filesystem uses stopped removing the file"
    );
}

/// 🔴 **The guard on the ORDER inside the transform, and it exists because
/// removing that order changed nothing anywhere else.** `caseless_form` is
/// `NFC(fold(NFD(x)))`, and the inner `NFD` was measured to be load-bearing:
/// `U+0345` COMBINING GREEK YPOGEGRAMMENI has canonical combining class 240 and
/// case-folds to `U+03B9` GREEK SMALL LETTER IOTA, class 0 — so folding first
/// destroys the ordering information the normalisation afterwards needs. Run
/// before the code was written, on `α U+0345 U+0300` against its own canonical
/// ordering `α U+0300 U+0345`: `NFC(fold(x))` gives `U+03B1 U+1F76` for the
/// first and `U+1F70 U+03B9` for the second, while `NFC(fold(NFD(x)))` gives
/// `U+1F70 U+03B9` for both.
///
/// 🔴 **Every other case in this file has inputs that are already in a normal
/// form, so deleting the inner `NFD` left all forty-three of them green.**
/// Measured by deleting it. A file name is arbitrary bytes and nothing
/// guarantees a normal form — any program can create one — so the state is
/// reachable, and this is the only case that builds it.
///
/// It goes through `MaskLayer::matches` rather than through a walk because
/// creating the file would put the question to the filesystem, which on APFS
/// normalises at lookup and would answer it for the wrong reason.
///
/// Both directions, plus a neighbour that must not match, so a `matches` that
/// had become "always true" would not pass this.
#[test]
fn a_mask_matches_a_name_that_is_in_no_normal_form_at_all() {
    // α + YPOGEGRAMMENI + GRAVE: not canonically ordered, because 240 > 230.
    const UNORDERED: &str = "\u{3b1}\u{345}\u{300}.txt";
    // The same string, canonically ordered, and written in capitals so the
    // fold is exercised at the same time.
    const ORDERED_UPPER: &str = "\u{391}\u{300}\u{345}.txt";

    assert_ne!(
        UNORDERED, ORDERED_UPPER,
        "the two spellings must be different byte strings, or this case proves nothing"
    );

    let rules = WalkRules::none()
        .with_masks(vec![ORDERED_UPPER.to_string()])
        .unwrap();
    assert!(
        rules.masks().matches(UNORDERED),
        "a canonically ordered mask did not match an unordered name — the inner NFD is gone"
    );

    let rules = WalkRules::none()
        .with_masks(vec![UNORDERED.to_string()])
        .unwrap();
    assert!(
        rules.masks().matches(ORDERED_UPPER),
        "an unordered mask did not match a canonically ordered name — the inner NFD is gone"
    );
    assert!(
        !rules.masks().matches("\u{3b2}\u{300}.txt"),
        "the mask matched a different Greek name, so the two assertions above say nothing"
    );
}

/// 🔴 **The second hole Task 9 measured, closed: case folding is no longer
/// ASCII-only.** `globset` compiles to a non-Unicode regex — `ÜBUNG.TXT` became
/// `(?-u)(?i)^\xc3\x9cBUNG\.TXT$` — so its `(?i)` folded the ASCII bytes and
/// left the two bytes of `Ü` alone. A person writing a mask in their own
/// alphabet got a rule that did not fire.
///
/// Three pairs, each a different alphabet and each asserted in both directions
/// so that a one-sided fold would be visible: `Ü`/`ü` (the pair Task 9
/// measured), Cyrillic `Копія`/`КОПІЯ`, and Greek final `ς` against `Σ` — the
/// last one is the pair that tells full case folding apart from
/// `str::to_lowercase`, which lowercases `Σ` to `σ` and leaves `ς` as `ς`.
///
/// The Cyrillic half carries a surviving neighbour, because a fold that
/// removed everything would satisfy a one-sided assertion just as well.
#[test]
fn a_mask_folds_case_outside_ascii() {
    let kept = walk_with_masks(&["\u{fc}bung.txt", "notes.txt"], false, &["\u{dc}BUNG.TXT"]);
    assert_eq!(
        kept,
        ["notes.txt"],
        "an uppercase mask did not remove the lowercase name"
    );

    let kept = walk_with_masks(&["\u{dc}BUNG.TXT", "notes.txt"], false, &["\u{fc}bung.txt"]);
    assert_eq!(
        kept,
        ["notes.txt"],
        "a lowercase mask did not remove the uppercase name"
    );

    // `Копія*` against `КОПІЯ звіту.docx`: the exact pair the plan named.
    // Sorted by path bytes — `n` (0x6e) precedes the first byte of `К` (0xd0).
    let kept = walk_with_masks(
        &[
            "\u{41a}\u{41e}\u{41f}\u{406}\u{42f} \u{437}\u{432}\u{456}\u{442}\u{443}.docx",
            "\u{41a}\u{43e}\u{43d}\u{442}\u{440}\u{430}\u{43a}\u{442}.docx",
            "notes.txt",
        ],
        false,
        &["\u{41a}\u{43e}\u{43f}\u{456}\u{44f}*"],
    );
    assert_eq!(
        kept,
        [
            "notes.txt",
            "\u{41a}\u{43e}\u{43d}\u{442}\u{440}\u{430}\u{43a}\u{442}.docx"
        ],
        "a Cyrillic mask either missed the file it names or took its neighbour with it"
    );

    // Greek final sigma. `str::to_lowercase` maps `Σ` to `σ` and leaves `ς`
    // alone, so under it this mask misses; full case folding maps both to `σ`.
    let kept = walk_with_masks(
        &["\u{3bb}\u{3bf}\u{3b3}\u{3bf}\u{3c2}.txt", "notes.txt"],
        false,
        &["\u{39b}\u{39f}\u{393}\u{39f}\u{3a3}.txt"],
    );
    assert_eq!(
        kept,
        ["notes.txt"],
        "final sigma did not fold to sigma — this is `to_lowercase`, not case folding"
    );
}

/// 🔴 **`case_insensitive(true)` is not behaviour-neutral to remove — it acts
/// on the *pattern*, not only on the subject.** A character-class range with
/// uncased endpoints spanning ASCII upper case folds under `(?i)` to also
/// accept lower case; `caseless_form` folds a mask's literal characters but
/// never its range syntax, so this is the one shape where the flag stays
/// observable even though both sides already went through `caseless_form`.
/// Measured: 54 differing scalars for `[@-a]x.txt` alone, and upward of a
/// million once a `*` is present (independent review, Important 3). This is
/// what makes "exactly one folding mechanism is live"
/// (`validate_mask`'s doc comment) a behavioural guarantee rather than a
/// maintenance wish.
#[test]
fn a_mask_glob_is_compiled_case_sensitive() {
    // `[@-a]` has uncased endpoints spanning ASCII upper case, so `(?i)` — and
    // only `(?i)` — folds the class to also accept b..z. The name side is
    // already folded, so this is the one shape where the flag is still visible.
    assert!(
        m("[@-a]x.txt", "ax.txt"),
        "the class stopped matching its own member"
    );
    assert!(
        !m("[@-a]x.txt", "Bx.txt"),
        "`case_insensitive(true)` is live again"
    );
    assert!(
        !m("[@-a]x.txt", "bx.txt"),
        "`case_insensitive(true)` is live again"
    );
}

/// 🔴 **The outer `.nfc()` in `caseless_form` is observable, and it is not an
/// equivalent mutant.** The doc comment on `caseless_form` calls the outer
/// form "free" — true of the *equivalence relation* (NFC and NFD are in
/// bijection, so either gives the same answer to "are these two names the
/// same"), false of the *matcher*: `globset` compares bytes and `?` counts
/// bytes, so a composed name is shorter than its decomposed spelling and the
/// outer form decides what `?` — and the contents of `[...]` — see
/// (independent review, Important 2).
#[test]
fn a_mask_compares_names_in_composed_form() {
    // `?` is one byte to `globset`; composed `é` is two bytes, decomposed is three.
    assert!(m("??.txt", "\u{e9}.txt"), "the outer NFC is gone");
    assert!(
        m("??.txt", "e\u{301}.txt"),
        "the decomposed name must give the same answer"
    );
    assert!(
        !m("?.txt", "\u{e9}.txt"),
        "one `?` is one byte, not one character"
    );
}

/// 🔴 **A second, independent narrowing shape, distinct from the character-class
/// folding `caseless_form`'s doc comment already names: the same outer `.nfc()`
/// composes a combining mark into the base letter a mask's literal was
/// matching, so a mask that ends right where the mark begins loses the
/// literal it was looking for.** Not a regression — it is the direct cost of
/// the fix for the opposite gap (`café*` now matches a decomposed name it used
/// to miss) — but a decision, pinned here so the direction is a fact the next
/// session can check rather than rediscover (independent review, Important 1).
#[test]
fn a_mask_literal_can_be_swallowed_by_a_following_combining_mark() {
    // `cafe` + COMBINING ACUTE ACCENT composes to `café`, so the ASCII literal
    // `cafe*` no longer prefixes the folded name.
    assert!(
        !m("cafe*", "cafe\u{301}.pdf"),
        "an ASCII literal survived composition with the mark that follows it"
    );
    // The already-composed spelling in the mask still matches the same name,
    // because both sides fold to the same composed form — this is the gap the
    // commit closed.
    assert!(
        m("caf\u{e9}*", "cafe\u{301}.pdf"),
        "a mask already spelled in composed form must still match the decomposed name"
    );
}

/// 🔴 **`.` and `..` are the two names no file can have**, so a mask spelling
/// one of them is a rule that stores fine, compiles fine, and can never fire —
/// the under-exclusion shape, arriving as a rule a person believes they wrote.
/// Refused with a sentence, the same way `validate_prefix` refuses a `.` path
/// component.
///
/// **Both directions, and the second one is the whole reason this is a check on
/// two exact names rather than on a leading dot.** `.hidden` and `..чернетка`
/// are ordinary file names — the walk sets `hidden(false)` precisely because a
/// dotfile in a watched folder is an ordinary document — so they stay legal, and
/// they are shown to *actually remove a file*, not merely to be accepted.
#[test]
fn a_mask_that_can_never_name_a_file_is_refused() {
    for mask in [".", ".."] {
        // Matched rather than `expect_err`, whose panic prints the whole
        // compiled `WalkRules` and buries the thing that went wrong.
        let sentence = match WalkRules::none().with_masks(vec![mask.to_string()]) {
            Ok(_) => panic!("the mask {mask:?} was accepted, and no file can be named that"),
            Err(refusal) => refusal.to_string(),
        };
        assert!(
            sentence.contains(mask),
            "the refusal does not name the mask that was typed: {sentence:?}"
        );
        assert!(
            sentence.contains("no file"),
            "the refusal does not say why the mask can never fire: {sentence:?}"
        );
    }

    let kept = walk_with_masks(
        &[
            ".hidden",
            "..\u{447}\u{435}\u{440}\u{43d}\u{435}\u{442}\u{43a}\u{430}",
            "notes.txt",
        ],
        false,
        &[".hidden"],
    );
    assert_eq!(
        kept,
        [
            "..\u{447}\u{435}\u{440}\u{43d}\u{435}\u{442}\u{43a}\u{430}",
            "notes.txt"
        ],
        "`.hidden` is a legal mask and must remove the file of that name"
    );

    let kept = walk_with_masks(
        &[
            ".hidden",
            "..\u{447}\u{435}\u{440}\u{43d}\u{435}\u{442}\u{43a}\u{430}",
            "notes.txt",
        ],
        false,
        &["..\u{447}\u{435}\u{440}\u{43d}\u{435}\u{442}\u{43a}\u{430}"],
    );
    assert_eq!(
        kept,
        [".hidden", "notes.txt"],
        "`..чернетка` is a legal mask and must remove the file of that name"
    );
}

/// 🔴 **A mask's glob metacharacters have to survive the transform as
/// metacharacters.** Case folding and normalisation rewrite the mask before it
/// is compiled, so the question is not academic: a transform that turned `*`
/// into a literal would leave every wildcard mask matching nothing, and one that
/// *manufactured* a `[` would turn a literal name into a malformed pattern.
/// Measured over every Unicode scalar value before this was written: no scalar
/// transforms into any of `* ? [ ] { } ! , - \ /`, and the transform never
/// empties a non-empty string.
///
/// Each metacharacter is asserted in both directions — the thing it must match
/// and the thing it must not — because a `*` that had become a literal would
/// satisfy "keep.txt survives" perfectly well.
///
/// The `[A-Z]` half is the one that shows the class content folding *with* the
/// mask rather than around it: it is compiled as `[a-z]` and asked of a folded
/// name, which is what makes an uppercase-written class mean the same rule as a
/// lowercase-written one.
#[test]
fn a_masks_glob_metacharacters_survive_the_transform() {
    let kept = walk_with_masks(&["a.tmp", "keep.txt"], false, &["*.tmp"]);
    assert_eq!(kept, ["keep.txt"], "`*` stopped being a wildcard");

    let kept = walk_with_masks(&["a.txt", "ab.txt"], false, &["?.txt"]);
    assert_eq!(
        kept,
        ["ab.txt"],
        "`?` matched either nothing or more than one character"
    );

    let kept = walk_with_masks(&["Alpha.txt", "9.txt"], false, &["[a-z]*.txt"]);
    assert_eq!(
        kept,
        ["9.txt"],
        "`[a-z]` stopped being a class: it must match the folded `Alpha.txt` and not `9.txt`"
    );

    let kept = walk_with_masks(&["Alpha.txt", "9.txt"], false, &["[A-Z]*.txt"]);
    assert_eq!(
        kept,
        ["9.txt"],
        "a class written in uppercase must be the same rule as the same class in lowercase"
    );
}

/// ⚠️ **A limitation this task does NOT close, pinned so a later session finds
/// it written down instead of discovering it.** `globset` compiles with Unicode
/// off (`(?-u)`, hardcoded at `globset-0.4.19/src/glob.rs:675`), so a character
/// class is a class of **bytes**: `[Гґ]` compiles to a class over the four bytes
/// of those two letters and can only ever match a single byte, which no
/// well-formed UTF-8 name offers there. The class matches nothing.
///
/// Measured identical before and after this commit's transform — the transform
/// folds the class content (`[ГҐ]` becomes `[гґ]`) and changes the answer not at
/// all, because the byte semantics were never about case. Booked for Task 11:
/// the editor is where a person could be told, and `mask_preview` will show a
/// zero.
///
/// Both directions, so this is a statement about the class rather than about
/// the walk finding nothing: the file the class names survives, and a plain
/// literal mask over the same non-ASCII letter removes it.
#[test]
fn a_character_class_of_non_ascii_letters_still_matches_nothing() {
    let kept = walk_with_masks(
        &["\u{413}file.txt", "notes.txt"],
        false,
        &["[\u{413}\u{490}]file.txt"],
    );
    assert_eq!(
        kept,
        ["notes.txt", "\u{413}file.txt"],
        "a non-ASCII character class started matching — `globset`'s byte-class semantics have \
         changed and this limitation can be lifted"
    );

    let kept = walk_with_masks(
        &["\u{413}file.txt", "notes.txt"],
        false,
        &["\u{433}file.txt"],
    );
    assert_eq!(
        kept,
        ["notes.txt"],
        "the same letter as a literal must still remove the file, or the case above is only \
         saying the mask was broken some other way"
    );
}

/// A `.gitignore` parser edge, decided rather than inherited: a mask never
/// reaches `GitignoreBuilder::add_line`, so a leading `#` is an ordinary
/// character and not a comment marker. Given a stated meaning rather than
/// refused, because a person typing `#notes.txt` has no competing intent — the
/// only thing that string can mean here is the file of that name.
///
/// Both directions, and they are what tells the two parsers apart: under the
/// gitignore parser the whole mask would be a comment and `#notes.txt` would
/// survive.
#[test]
fn a_leading_hash_in_a_mask_is_an_ordinary_character() {
    let kept = walk_with_masks(&["#notes.txt", "notes.txt"], false, &["#notes.txt"]);

    assert_eq!(kept, ["notes.txt"]);
}

/// A `.gitignore` parser edge, decided rather than inherited, and decided the
/// other way from `#`: a leading `!` **is** refused. Under the gitignore parser
/// it means re-include; under `globset` it is an ordinary character; so a
/// person who knows the first meaning gets, silently, a mask that matches a
/// file name almost nobody has. That is the under-exclusion direction, and a
/// sentence is the only thing that can tell them.
///
/// Scoped to the **leading** position, which is the direction a blanket refusal
/// would get wrong: `!` inside a character class is `globset`'s own negation
/// and keeps working.
#[test]
fn a_leading_exclamation_mark_in_a_mask_is_refused() {
    let kept = walk_with_masks(&["a1.txt", "b1.txt"], false, &["[!a]*.txt"]);
    assert_eq!(
        kept,
        ["a1.txt"],
        "`!` inside a character class is globset's own negation and is not touched"
    );

    let result = WalkRules::none().with_masks(vec!["!*.pdf".to_string()]);
    assert!(
        matches!(
            result,
            Err(RulesError::MaskStartsWithExclamationMark { .. })
        ),
        "a leading `!` must be refused: it means re-include to the person typing it and an \
         ordinary character to the matcher"
    );
}

/// A `.gitignore` parser edge, decided rather than inherited: the gitignore
/// line parser silently trims trailing whitespace, `globset` does not. Measured
/// on the pinned version — `"*.pdf "` matches `"report.pdf "` and not
/// `"report.pdf"` — so a stray keystroke compiles into a mask for a name almost
/// nobody has. Refused at both edges, exactly as `SurroundingWhitespace`
/// refuses it for a prefix, and for the same reason.
#[test]
fn a_mask_with_surrounding_whitespace_is_refused() {
    let kept = walk_with_masks(&["report.pdf", "notes.txt"], false, &["*.pdf"]);
    assert_eq!(kept, ["notes.txt"], "the well-formed mask removes its file");

    for malformed in ["*.pdf ", " *.pdf"] {
        let result = WalkRules::none().with_masks(vec![malformed.to_string()]);
        assert!(
            matches!(result, Err(RulesError::MaskSurroundingWhitespace { .. })),
            "{malformed:?} must be refused, not compiled into a rule for a name with a space in it"
        );
    }
}

/// A `.gitignore` parser edge, decided rather than inherited, and the one whose
/// meaning is **platform-dependent**: `globset`'s `backslash_escape` defaults to
/// on where `\` is not a path separator and off where it is. Measured on this
/// platform, `a\bee.txt` matches `abee.txt` — the named file survives and an
/// unrelated one is removed in its place — while the same mask on Windows would
/// match `a\bee.txt` literally. No rewrite is unambiguous, so it is refused,
/// exactly as `ContainsBackslash` refuses it for a prefix.
#[test]
fn a_mask_containing_a_backslash_is_refused() {
    let kept = walk_with_masks(&["abee.txt", "notes.txt"], false, &["abee.txt"]);
    assert_eq!(
        kept,
        ["notes.txt"],
        "the well-formed mask removes exactly the file it names"
    );

    let result = WalkRules::none().with_masks(vec!["a\\bee.txt".to_string()]);
    assert!(
        matches!(result, Err(RulesError::MaskContainsBackslash { .. })),
        "a backslash must be refused, not silently compiled into a rule for `abee.txt` on one \
         platform and `a\\bee.txt` on another"
    );
}

/// 🔴 **Step 3b: the predicate Task 10's `mask_preview` will count with.** It
/// exists so the preview stands on **this** matcher rather than a second copy
/// of the rule — a second copy is the defect `mask_preview` exists to prevent,
/// and it would disagree at exactly the edges the cases above spent this task
/// pinning down.
///
/// So the guard is not "the predicate returns the right answers"; it is "the
/// predicate and the walk answer the same question about the same paths".
///
/// 🔴 **Over regular files, which is the whole of its fixture and the whole of
/// its claim.** Review measured the two apart on a symlink and on a FIFO: the
/// walk asks `is_file()` first and this predicate asks nothing about the entry,
/// so it calls them removed and the walk keeps them. The skew only ever
/// overstates — nothing the walk removes is called safe — and it cannot be seen
/// from here, because every name below is an ordinary file. The `#[cfg(unix)]`
/// case further down is the one that pins the walk's half.
///
/// 🔴 **`sub/report.pdf` is the path that makes it a guard rather than a
/// coincidence.** The walk hands the predicate a bare file name; `mask_preview`
/// will hand it an indexed relative path. A `matches` that compared the whole
/// path would still agree with the walk on every `*.pdf`-shaped mask, because
/// `globset`'s `*` crosses `/` by default — `report*.pdf` is the shape that
/// tells the two readings apart, since `sub/report.pdf` does not start with
/// `report` and its name does. `other/REPORT2.PDF` sits in a folder of its own
/// because macOS's default filesystem could not hold it beside `report.pdf`.
///
/// 🔴 **Four of the names exist to make the fold-and-normalise transform part of
/// what this guard checks.** The walk hands the predicate a bare file name and a
/// caller hands it a relative path; both must be transformed by the *same* call,
/// and a fixture of ASCII names would prove nothing about that, because on ASCII
/// the transform is plain lowercasing and any second copy of it would agree.
/// So: `sub/ÜBUNG.TXT` (a fold that crosses out of ASCII),
/// `sub/КОПІЯ звіту.docx` (Cyrillic, matched by a lowercase-Cyrillic mask),
/// `sub/cafe\u{301}.pdf` (a decomposed name matched by a composed mask) and
/// `Контракт.docx` (the non-matching Cyrillic neighbour).
///
/// The loop runs over **the paths the walk reported**, not over the ones this
/// case asked for: a filesystem that normalises hands back a different byte
/// string than it was given, and the guard would then be asking the predicate
/// about a path that does not exist.
#[test]
fn the_mask_predicate_and_the_walk_agree_on_regular_files() {
    let paths = [
        "report.pdf",
        "sub/report.pdf",
        "other/REPORT2.PDF",
        "sub/myreport.pdf",
        "notes.txt",
        "sub/notes.txt",
        "#literal.txt",
        "sub/deep/a.tmp",
        "sub/deep/keep.md",
        "sub/\u{dc}BUNG.TXT",
        "sub/\u{41a}\u{41e}\u{41f}\u{406}\u{42f} \u{437}\u{432}\u{456}\u{442}\u{443}.docx",
        "sub/cafe\u{301}.pdf",
        "\u{41a}\u{43e}\u{43d}\u{442}\u{440}\u{430}\u{43a}\u{442}.docx",
    ];

    let root = tempfile::tempdir().unwrap();
    for name in paths {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }
    let rules = WalkRules::none()
        .with_masks(vec![
            "report*.pdf".to_string(),
            "*.tmp".to_string(),
            "#literal.txt".to_string(),
            "\u{fc}bung.txt".to_string(),
            "\u{41a}\u{43e}\u{43f}\u{456}\u{44f}*".to_string(),
            "caf\u{e9}.pdf".to_string(),
        ])
        .unwrap();

    let on_disk: Vec<String> = enumerate(root.path(), &WalkRules::none())
        .found
        .iter()
        .map(|f| f.relative.clone())
        .collect();
    assert_eq!(
        on_disk.len(),
        paths.len(),
        "the fixture did not land on disk as {} separate files: {on_disk:?}",
        paths.len()
    );

    let kept: Vec<String> = enumerate(root.path(), &rules)
        .found
        .iter()
        .map(|f| f.relative.clone())
        .collect();

    for path in &on_disk {
        let predicate_says_removed = rules.masks().matches(path);
        let walk_removed = !kept.contains(path);
        assert_eq!(
            predicate_says_removed, walk_removed,
            "the predicate and the walk disagree about {path:?}: {predicate_says_removed} vs \
             {walk_removed}"
        );
    }
    // Both answers really occur, so the loop is not thirteen trivial agreements
    // on one side — and the count pins that the four non-ASCII names were
    // *removed*, rather than agreed about by both sides saying "kept".
    assert_eq!(kept.len(), 5, "kept = {kept:?}");
    for survivor in [
        "notes.txt",
        "sub/deep/keep.md",
        "sub/myreport.pdf",
        "sub/notes.txt",
        "\u{41a}\u{43e}\u{43d}\u{442}\u{440}\u{430}\u{43a}\u{442}.docx",
    ] {
        assert!(
            kept.contains(&survivor.to_string()),
            "{survivor:?} should have survived; kept = {kept:?}"
        );
    }
}

/// 🔴 **The "what disappears" question, asked of this layer.** A mask removes
/// files, and `filter_entry` removes them *before* `enumerate` ever sees them —
/// so anything the mask drops is dropped with nothing anywhere saying so, the
/// same as every other rule layer. That is correct for a file the person asked
/// not to index, and wrong for everything else, which is why the layer asks
/// `is_file()` rather than `!is_dir()`.
///
/// A symlink is the entry that separates the two: it is neither, it is never
/// indexed under `follow_links(false)` either way, and `enumerate` names it in
/// `skipped` as `NotAFile`. A mask written with `!is_dir()` would take that
/// notice away and change nothing else — a disclosure lost for no gain.
///
/// Both directions: the real file the mask names goes, and the symlink wearing
/// the same-shaped name is still reported.
#[cfg(unix)]
#[test]
fn a_mask_does_not_swallow_what_the_walk_reports_as_not_a_file() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("elsewhere.txt"), b"x").unwrap();
    fs::write(root.path().join("real.pdf"), b"x").unwrap();
    symlink(
        root.path().join("elsewhere.txt"),
        root.path().join("link.pdf"),
    )
    .unwrap();

    let rules = WalkRules::none()
        .with_masks(vec!["*.pdf".to_string()])
        .unwrap();
    let walked = enumerate(root.path(), &rules);

    let names: Vec<&str> = walked.found.iter().map(|f| f.relative.as_str()).collect();
    assert_eq!(names, ["elsewhere.txt"], "the mask removes the real file");

    let reported: Vec<&str> = walked
        .skipped
        .iter()
        .filter_map(|s| s.relative.as_deref())
        .collect();
    assert_eq!(
        reported,
        ["link.pdf"],
        "the symlink is still named — a mask must not take a disclosure away"
    );
    assert!(walked.complete, "naming a symlink is not a read failure");
}
