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
