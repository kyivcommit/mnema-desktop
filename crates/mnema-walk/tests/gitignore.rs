//! The `.gitignore` layer (D202, folders tree PR 3): this crate decides
//! `.gitignore` itself, so the folders tree and the walk ask ONE matcher, and
//! includes can lift it for a path.
//!
//! The first two tests are the fence: with no includes, the walk must keep
//! exactly what `ignore`'s own walker kept under the configuration `builder()`
//! ran before this layer existed. The oracle below IS that configuration, so a
//! drift in either direction fails here rather than in a person's index.

use ignore::WalkBuilder;
use mnema_walk::{GitState, GitignoreLayer, WalkRules, enumerate};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// What `ignore` keeps under `builder()`'s configuration before PR 3, files
/// only, `/`-joined and relative to `root`.
fn oracle(root: &Path) -> BTreeSet<String> {
    WalkBuilder::new(root)
        .follow_links(false)
        .hidden(false)
        .git_global(false)
        .git_exclude(true)
        .git_ignore(true)
        .require_git(false)
        .ignore(false)
        .parents(false)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| relative(root, e.path()))
        .collect()
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .components()
        .map(|c| c.as_os_str().to_str().unwrap())
        .collect::<Vec<_>>()
        .join("/")
}

fn walked(root: &Path, rules: &WalkRules) -> BTreeSet<String> {
    enumerate(root, rules)
        .found
        .into_iter()
        .map(|f| f.relative)
        .collect()
}

fn gitignore_only() -> WalkRules {
    WalkRules::new(false, true, Vec::new()).unwrap()
}

/// `files` are `(path, contents)`; a path ending in `/` is a directory.
fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = root.path().join(path);
        if path.ends_with('/') {
            fs::create_dir_all(&full).unwrap();
        } else {
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(&full, contents).unwrap();
        }
    }
    root
}

fn assert_agrees(name: &str, root: &Path) {
    let want = oracle(root);
    let got = walked(root, &gitignore_only());
    assert_eq!(
        got, want,
        "fixture `{name}`: the layer and `ignore` disagree"
    );
}

#[test]
fn the_layer_agrees_with_ignore_on_fixtures() {
    let fixtures: &[(&str, &[(&str, &str)])] = &[
        (
            "a nested `!` brings back what the parent named",
            &[
                (".gitignore", "*.log\n"),
                ("a.log", "x"),
                ("sub/.gitignore", "!keep.log\n"),
                ("sub/keep.log", "x"),
                ("sub/drop.log", "x"),
            ],
        ),
        (
            "`foo/` prunes a folder, not a file",
            &[
                (".gitignore", "foo/\n"),
                ("foo", "x"),
                ("a/foo/in.txt", "x"),
                ("b.txt", "x"),
            ],
        ),
        (
            "`/top.txt` is anchored to its own folder",
            &[
                (".gitignore", "/top.txt\n"),
                ("top.txt", "x"),
                ("a/top.txt", "x"),
            ],
        ),
        (
            "`**` in both positions",
            &[
                (".gitignore", "**/deep.txt\na/**/b\n"),
                ("x/y/deep.txt", "x"),
                ("a/q/r/b", "x"),
                ("a/b", "x"),
                ("a/c", "x"),
            ],
        ),
        (
            "`.git/info/exclude` with no `.gitignore`",
            &[
                (".git/info/exclude", "secret.txt\n"),
                ("secret.txt", "x"),
                ("kept.txt", "x"),
            ],
        ),
        (
            "`.gitignore` outranks `info/exclude`",
            &[
                (".git/info/exclude", "x.txt\ny.txt\n"),
                (".gitignore", "!x.txt\n"),
                ("x.txt", "x"),
                ("y.txt", "x"),
            ],
        ),
        (
            "a deeper `info/exclude` loses to a shallower `.gitignore`",
            &[
                (".gitignore", "!z.txt\n"),
                ("sub/.git/info/exclude", "z.txt\n"),
                ("sub/z.txt", "x"),
            ],
        ),
        (
            "`!` under an ignored folder does not bring a file back",
            &[
                (".gitignore", "build/\n!build/keep.txt\n"),
                ("build/keep.txt", "x"),
                ("build/sub/.gitignore", "!*\n"),
                ("build/sub/x.txt", "x"),
            ],
        ),
        (
            "a nested repository does not stop the parent `.gitignore`",
            &[
                (".gitignore", "*.tmp\n"),
                ("module/.git/HEAD", "ref: refs/heads/main\n"),
                ("module/a.tmp", "x"),
                ("module/b.txt", "x"),
            ],
        ),
        (
            "a `.git` FILE reads no `info/exclude`",
            &[(".git", "gitdir: /nowhere\n"), ("w.txt", "x")],
        ),
        (
            "a deeper `.gitignore` ignores what a shallower `!` kept",
            &[
                (".gitignore", "!*.md\n"),
                ("docs/.gitignore", "draft.md\n"),
                ("docs/draft.md", "x"),
                ("docs/final.md", "x"),
            ],
        ),
        (
            "comments, blank lines and trailing spaces",
            &[
                (".gitignore", "# a comment\n\nspace.txt   \n\\#hash.txt\n"),
                ("space.txt", "x"),
                ("#hash.txt", "x"),
                ("plain.txt", "x"),
            ],
        ),
    ];
    for (name, files) in fixtures {
        let root = tree(files);
        assert_agrees(name, root.path());
    }
}

/// A tiny deterministic generator: no `rand` in this crate, and the seed in
/// the failure message is what reproduces a case.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
        from[(self.next() as usize) % from.len()]
    }
}

#[test]
fn the_layer_agrees_with_ignore_on_random_trees() {
    let names = ["a", "b", "build", "x.txt", "y.log", "z.md", "keep.txt"];
    let lines = [
        "*.log",
        "!keep.txt",
        "build/",
        "/a",
        "a/",
        "**/b",
        "x.txt",
        "!*.md",
        "*.txt",
        "!y.log",
        "b/**",
        "!build/",
        "# c",
        "",
    ];
    // Positive control: a generator whose `.gitignore` lines never matched
    // would pass this test against a layer that ignores nothing.
    let mut ignored_total = 0usize;
    for seed in 0..200u64 {
        let mut g = Lcg(seed + 1);
        let root = tempfile::tempdir().unwrap();
        let mut dirs = vec![String::new()];
        for _ in 0..12 {
            let parent = dirs[(g.next() as usize) % dirs.len()].clone();
            let depth = parent.matches('/').count() + usize::from(!parent.is_empty());
            let name = g.pick(&names);
            let path = if parent.is_empty() {
                name.to_string()
            } else {
                format!("{parent}/{name}")
            };
            let full = root.path().join(&path);
            if full.exists() {
                continue;
            }
            if depth < 4 && g.next().is_multiple_of(2) {
                fs::create_dir_all(&full).unwrap();
                dirs.push(path);
            } else {
                fs::write(&full, "x").unwrap();
            }
        }
        for dir in dirs.clone() {
            if !g.next().is_multiple_of(3) {
                continue;
            }
            let body: Vec<&str> = (0..1 + g.next() % 4).map(|_| g.pick(&lines)).collect();
            let at = root.path().join(&dir);
            if at.is_dir() {
                fs::write(at.join(".gitignore"), body.join("\n") + "\n").unwrap();
            }
        }
        let want = oracle(root.path());
        let everything = walked(root.path(), &WalkRules::none());
        ignored_total += everything.len() - want.len();
        let got = walked(root.path(), &gitignore_only());
        assert_eq!(got, want, "seed {seed}: the layer and `ignore` disagree");
    }
    assert!(
        ignored_total > 100,
        "the random trees ignored only {ignored_total} files"
    );
}

#[test]
fn an_include_inside_an_ignored_folder_opens_only_its_path() {
    let root = tree(&[
        (".gitignore", "build/\n"),
        ("build/x/keep.txt", "x"),
        ("build/x/other.txt", "x"),
        ("build/y.txt", "x"),
        ("kept.txt", "x"),
    ]);
    let rules = gitignore_only()
        .with_includes(vec!["build/x/keep.txt".into()])
        .unwrap();
    let got = walked(root.path(), &rules);
    assert!(got.contains("build/x/keep.txt"), "{got:?}");
    assert!(got.contains("kept.txt"), "{got:?}");
    assert!(!got.contains("build/x/other.txt"), "{got:?}");
    assert!(!got.contains("build/y.txt"), "{got:?}");
}

#[test]
fn an_include_lifts_gitignore_files_below_it() {
    let root = tree(&[
        ("vendor/.gitignore", "*.txt\n"),
        ("vendor/a.txt", "x"),
        ("other/.gitignore", "*.txt\n"),
        ("other/b.txt", "x"),
    ]);
    let rules = gitignore_only()
        .with_includes(vec!["vendor".into()])
        .unwrap();
    let got = walked(root.path(), &rules);
    assert!(got.contains("vendor/a.txt"), "{got:?}");
    assert!(!got.contains("other/b.txt"), "{got:?}");
}

#[test]
fn an_exclusion_above_an_include_wins() {
    let root = tree(&[(".gitignore", "build/\n"), ("build/x/keep.txt", "x")]);
    let rules = WalkRules::new(false, true, vec!["build".into()])
        .unwrap()
        .with_includes(vec!["build/x/keep.txt".into()])
        .unwrap();
    let got = walked(root.path(), &rules);
    assert!(
        !got.iter().any(|p| p.starts_with("build/")),
        "an include outran the person's own exclusion: {got:?}"
    );
}

/// Final review, Important 1. `ignore` passes over an ignore file it cannot
/// READ — `push_ignore_io` drops every `is_io()` error (`lib.rs:380-384`) and
/// `add_child` says so (`dir.rs:267`) — so before this layer such a root still
/// walked complete and deletions applied. Marking it incomplete would stop
/// every deletion under the root for as long as the file stays unreadable.
#[cfg(unix)]
#[test]
fn an_unreadable_gitignore_is_passed_over_as_ignore_did() {
    use std::os::unix::fs::PermissionsExt;
    let root = tree(&[
        ("sub/.gitignore", "secret.txt\n"),
        ("sub/secret.txt", "x"),
        ("dir/.gitignore/inside.txt", "x"),
        ("dir/kept.txt", "x"),
    ]);
    let gi = root.path().join("sub/.gitignore");
    fs::set_permissions(&gi, fs::Permissions::from_mode(0o000)).unwrap();
    let want = oracle(root.path());
    let walked = enumerate(root.path(), &gitignore_only());
    fs::set_permissions(&gi, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(walked.complete, "{:?}", walked.skipped);
    let got: BTreeSet<String> = walked.found.into_iter().map(|f| f.relative).collect();
    assert_eq!(got, want);
}

/// The other half: a line `ignore` cannot parse IS an error it reports, the
/// walk is incomplete, and the lines that did parse still apply.
#[test]
fn a_gitignore_line_that_does_not_parse_marks_the_walk_incomplete() {
    let root = tree(&[
        (".gitignore", "secret.txt\n[z-a]\n"),
        ("secret.txt", "x"),
        ("kept.txt", "x"),
    ]);
    let walked = enumerate(root.path(), &gitignore_only());
    assert!(
        !walked.complete,
        "a broken `.gitignore` line left the walk complete"
    );
    assert!(
        walked
            .skipped
            .iter()
            .any(|s| s.detail.contains(".gitignore")),
        "no skip names the `.gitignore`: {:?}",
        walked.skipped
    );
    let got: BTreeSet<String> = walked.found.into_iter().map(|f| f.relative).collect();
    assert_eq!(got, oracle(root.path()));
}

/// Under an include `.gitignore` does not apply at all, so a `.gitignore`
/// there is not even read — and one that cannot be read costs nothing.
#[cfg(unix)]
#[test]
fn a_gitignore_under_an_include_is_not_read() {
    use std::os::unix::fs::PermissionsExt;
    let root = tree(&[("vendor/.gitignore", "*.txt\n"), ("vendor/a.txt", "x")]);
    let gi = root.path().join("vendor/.gitignore");
    fs::set_permissions(&gi, fs::Permissions::from_mode(0o000)).unwrap();
    let rules = gitignore_only()
        .with_includes(vec!["vendor".into()])
        .unwrap();
    let walked = enumerate(root.path(), &rules);
    fs::set_permissions(&gi, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(walked.complete, "{:?}", walked.skipped);
    assert!(walked.found.iter().any(|f| f.relative == "vendor/a.txt"));
}

#[test]
fn an_include_rejects_a_path_no_rule_can_carry() {
    assert!(
        gitignore_only()
            .with_includes(vec!["../up".into()])
            .is_err()
    );
    assert!(gitignore_only().with_includes(vec!["a//b".into()]).is_err());
}

#[test]
fn state_names_who_holds_a_row() {
    let root = tree(&[
        (".gitignore", "build/\n*.log\n"),
        ("build/x/keep.txt", "x"),
        ("build/x/other.txt", "x"),
        ("a/x.log", "x"),
        ("a/y.txt", "x"),
    ]);
    let layer = GitignoreLayer::new(root.path(), &["build/x/keep.txt".to_string()]);
    assert_eq!(layer.state("a/y.txt", false), GitState::Clear);
    assert_eq!(layer.state("a", true), GitState::Clear);
    assert_eq!(
        layer.state("a/x.log", false),
        GitState::Ignored {
            by: "a/x.log".into()
        }
    );
    assert_eq!(
        layer.state("build", true),
        GitState::IgnoredOpen { by: "build".into() }
    );
    assert_eq!(
        layer.state("build/x", true),
        GitState::IgnoredOpen { by: "build".into() }
    );
    assert_eq!(
        layer.state("build/x/other.txt", false),
        GitState::Ignored { by: "build".into() }
    );
    assert_eq!(
        layer.state("build/x/keep.txt", false),
        GitState::Included {
            include: "build/x/keep.txt".into()
        }
    );
}

#[test]
fn an_include_on_a_row_nothing_holds_is_clear() {
    let root = tree(&[("a/y.txt", "x")]);
    let layer = GitignoreLayer::new(root.path(), &["a".to_string()]);
    assert_eq!(layer.state("a/y.txt", false), GitState::Clear);
}

#[test]
fn held_below_lists_the_topmost_held_entries() {
    let root = tree(&[
        (".gitignore", "build/\n*.log\n"),
        ("build/in.txt", "x"),
        ("build/deep/z.log", "x"),
        ("a/x.log", "x"),
        ("a/y.txt", "x"),
    ]);
    let layer = GitignoreLayer::new(root.path(), &[]);
    assert_eq!(
        layer.held_below("", &|_, _| false),
        vec!["a/x.log".to_string(), "build".to_string()]
    );
    assert_eq!(
        layer.held_below("a", &|_, _| false),
        vec!["a/x.log".to_string()]
    );
    // A held folder is its own topmost held row.
    assert_eq!(
        layer.held_below("build", &|_, _| false),
        vec!["build".to_string()]
    );
}
